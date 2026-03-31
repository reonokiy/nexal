use super::LoadedPlugin;
use super::PluginLoadOutcome;
use super::PluginManifestPaths;
use super::load_plugin_manifest;
use crate::SkillMetadata;
use crate::config::Config;
use crate::config::types::McpServerConfig;
use crate::config::types::PluginConfig;
use crate::config_loader::ConfigLayerStack;
use crate::config_rules::SkillConfigRules;
use crate::config_rules::resolve_disabled_skill_paths;
use crate::config_rules::skill_config_rules_from_stack;
use crate::loader::SkillRoot;
use crate::loader::load_skills_from_roots;
use nexal_features::Feature;
use nexal_plugin::AppConnectorId;
use nexal_plugin::PluginCapabilitySummary;
use nexal_plugin::PluginId;
use nexal_plugin::PluginTelemetryMetadata;
use nexal_plugin::validate_plugin_segment;
use nexal_protocol::protocol::Product;
use nexal_protocol::protocol::SkillScope;
use nexal_utils_absolute_path::AbsolutePathBuf;
use serde::Deserialize;
use serde_json::Map as JsonMap;
use serde_json::Value as JsonValue;
use std::collections::HashMap;
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::sync::RwLock;
use tracing::warn;

const DEFAULT_SKILLS_DIR_NAME: &str = "skills";
const DEFAULT_MCP_CONFIG_FILE: &str = ".mcp.json";
const DEFAULT_APP_CONFIG_FILE: &str = ".app.json";
const PLUGINS_CACHE_DIR: &str = "plugins/cache";
const DEFAULT_PLUGIN_VERSION: &str = "local";

pub struct PluginsManager {
    plugin_cache_root: AbsolutePathBuf,
    cached_enabled_outcome: RwLock<Option<PluginLoadOutcome>>,
    restriction_product: Option<Product>,
}

impl PluginsManager {
    pub fn new(nexal_home: PathBuf) -> Self {
        Self::new_with_restriction_product(nexal_home, Some(Product::Nexal))
    }

    pub fn new_with_restriction_product(
        nexal_home: PathBuf,
        restriction_product: Option<Product>,
    ) -> Self {
        let plugin_cache_root =
            AbsolutePathBuf::try_from(nexal_home.join(PLUGINS_CACHE_DIR))
                .unwrap_or_else(|err| panic!("plugin cache root should be absolute: {err}"));
        Self {
            plugin_cache_root,
            cached_enabled_outcome: RwLock::new(None),
            restriction_product,
        }
    }

    pub fn restriction_product_matches(&self, products: Option<&[Product]>) -> bool {
        match products {
            None => true,
            Some([]) => false,
            Some(products) => self
                .restriction_product
                .is_some_and(|product| product.matches_product_restriction(products)),
        }
    }

    pub fn plugins_for_config(&self, config: &Config) -> PluginLoadOutcome {
        self.plugins_for_config_with_force_reload(config, /*force_reload*/ false)
    }

    pub(crate) fn plugins_for_config_with_force_reload(
        &self,
        config: &Config,
        force_reload: bool,
    ) -> PluginLoadOutcome {
        if !config.features.enabled(Feature::Plugins) {
            return PluginLoadOutcome::default();
        }

        if !force_reload && let Some(outcome) = self.cached_enabled_outcome() {
            return outcome;
        }

        let outcome = load_plugins_from_layer_stack(
            &config.config_layer_stack,
            &self.plugin_cache_root,
            self.restriction_product,
        );
        log_plugin_load_errors(&outcome);
        let mut cache = match self.cached_enabled_outcome.write() {
            Ok(cache) => cache,
            Err(err) => err.into_inner(),
        };
        *cache = Some(outcome.clone());
        outcome
    }

    pub fn clear_cache(&self) {
        let mut cached_enabled_outcome = match self.cached_enabled_outcome.write() {
            Ok(cache) => cache,
            Err(err) => err.into_inner(),
        };
        *cached_enabled_outcome = None;
    }

    /// Resolve plugin skill roots for a config layer stack without touching the plugins cache.
    pub fn effective_skill_roots_for_layer_stack(
        &self,
        config_layer_stack: &ConfigLayerStack,
        plugins_feature_enabled: bool,
    ) -> Vec<PathBuf> {
        if !plugins_feature_enabled {
            return Vec::new();
        }
        load_plugins_from_layer_stack(
            config_layer_stack,
            &self.plugin_cache_root,
            self.restriction_product,
        )
        .effective_skill_roots()
    }

    fn cached_enabled_outcome(&self) -> Option<PluginLoadOutcome> {
        match self.cached_enabled_outcome.read() {
            Ok(cache) => cache.clone(),
            Err(err) => err.into_inner().clone(),
        }
    }

    pub fn marketplace_roots(&self, additional_roots: &[AbsolutePathBuf]) -> Vec<AbsolutePathBuf> {
        let mut roots = additional_roots.to_vec();
        roots.sort_unstable_by(|left, right| left.as_path().cmp(right.as_path()));
        roots.dedup();
        roots
    }
}

fn log_plugin_load_errors(outcome: &PluginLoadOutcome) {
    for plugin in outcome
        .plugins()
        .iter()
        .filter(|plugin| plugin.error.is_some())
    {
        if let Some(error) = plugin.error.as_deref() {
            warn!(
                plugin = plugin.config_name,
                path = %plugin.root.display(),
                "failed to load plugin: {error}"
            );
        }
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PluginMcpFile {
    #[serde(default)]
    mcp_servers: HashMap<String, JsonValue>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PluginAppFile {
    #[serde(default)]
    apps: HashMap<String, PluginAppConfig>,
}

#[derive(Debug, Default, Deserialize)]
struct PluginAppConfig {
    id: String,
}

pub(crate) fn load_plugins_from_layer_stack(
    config_layer_stack: &ConfigLayerStack,
    plugin_cache_root: &AbsolutePathBuf,
    restriction_product: Option<Product>,
) -> PluginLoadOutcome {
    let skill_config_rules = skill_config_rules_from_stack(config_layer_stack);
    let mut configured_plugins: Vec<_> = configured_plugins_from_stack(config_layer_stack)
        .into_iter()
        .collect();
    configured_plugins.sort_unstable_by(|(a, _), (b, _)| a.cmp(b));

    let mut plugins = Vec::with_capacity(configured_plugins.len());
    let mut seen_mcp_server_names = HashMap::<String, String>::new();
    for (configured_name, plugin) in configured_plugins {
        let loaded_plugin = load_plugin(
            configured_name.clone(),
            &plugin,
            plugin_cache_root,
            restriction_product,
            &skill_config_rules,
        );
        for name in loaded_plugin.mcp_servers.keys() {
            if let Some(previous_plugin) =
                seen_mcp_server_names.insert(name.clone(), configured_name.clone())
            {
                warn!(
                    plugin = configured_name,
                    previous_plugin,
                    server = name,
                    "skipping duplicate plugin MCP server name"
                );
            }
        }
        plugins.push(loaded_plugin);
    }

    PluginLoadOutcome::from_plugins(plugins)
}

fn configured_plugins_from_stack(
    config_layer_stack: &ConfigLayerStack,
) -> HashMap<String, PluginConfig> {
    // Plugin entries remain persisted user config only.
    let Some(user_layer) = config_layer_stack.get_user_layer() else {
        return HashMap::new();
    };
    let Some(plugins_value) = user_layer.config.get("plugins") else {
        return HashMap::new();
    };
    match plugins_value.clone().try_into() {
        Ok(plugins) => plugins,
        Err(err) => {
            warn!("invalid plugins config: {err}");
            HashMap::new()
        }
    }
}

// ---------------------------------------------------------------------------
// Plugin-cache helpers (inlined from the removed store module)
// ---------------------------------------------------------------------------

fn plugin_cache_base_root(cache_root: &AbsolutePathBuf, plugin_id: &PluginId) -> AbsolutePathBuf {
    AbsolutePathBuf::try_from(
        cache_root
            .as_path()
            .join(&plugin_id.marketplace_name)
            .join(&plugin_id.plugin_name),
    )
    .unwrap_or_else(|err| panic!("plugin cache path should resolve to an absolute path: {err}"))
}

fn active_plugin_version(cache_root: &AbsolutePathBuf, plugin_id: &PluginId) -> Option<String> {
    let base = plugin_cache_base_root(cache_root, plugin_id);
    let mut discovered_versions = fs::read_dir(base.as_path())
        .ok()?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            entry.file_type().ok().filter(std::fs::FileType::is_dir)?;
            entry.file_name().into_string().ok()
        })
        .filter(|version| validate_plugin_segment(version, "plugin version").is_ok())
        .collect::<Vec<_>>();
    discovered_versions.sort_unstable();
    if discovered_versions.is_empty() {
        None
    } else if discovered_versions
        .iter()
        .any(|version| version == DEFAULT_PLUGIN_VERSION)
    {
        Some(DEFAULT_PLUGIN_VERSION.to_string())
    } else {
        discovered_versions.pop()
    }
}

fn active_plugin_root(cache_root: &AbsolutePathBuf, plugin_id: &PluginId) -> Option<AbsolutePathBuf> {
    active_plugin_version(cache_root, plugin_id).map(|version| {
        AbsolutePathBuf::try_from(
            plugin_cache_base_root(cache_root, plugin_id)
                .as_path()
                .join(version),
        )
        .unwrap_or_else(|err| {
            panic!("plugin cache path should resolve to an absolute path: {err}")
        })
    })
}

// ---------------------------------------------------------------------------

fn load_plugin(
    config_name: String,
    plugin: &PluginConfig,
    plugin_cache_root: &AbsolutePathBuf,
    restriction_product: Option<Product>,
    skill_config_rules: &SkillConfigRules,
) -> LoadedPlugin {
    let plugin_id = PluginId::parse(&config_name);
    let active_root = plugin_id
        .as_ref()
        .ok()
        .and_then(|pid| active_plugin_root(plugin_cache_root, pid));
    let root = active_root
        .clone()
        .unwrap_or_else(|| match &plugin_id {
            Ok(pid) => plugin_cache_base_root(plugin_cache_root, pid),
            Err(_) => plugin_cache_root.clone(),
        });
    let mut loaded_plugin = LoadedPlugin {
        config_name,
        manifest_name: None,
        manifest_description: None,
        root,
        enabled: plugin.enabled,
        skill_roots: Vec::new(),
        disabled_skill_paths: HashSet::new(),
        has_enabled_skills: false,
        mcp_servers: HashMap::new(),
        apps: Vec::new(),
        error: None,
    };

    if !plugin.enabled {
        return loaded_plugin;
    }

    let plugin_root = match plugin_id {
        Ok(_) => match active_root {
            Some(plugin_root) => plugin_root,
            None => {
                loaded_plugin.error = Some("plugin is not installed".to_string());
                return loaded_plugin;
            }
        },
        Err(err) => {
            loaded_plugin.error = Some(err.to_string());
            return loaded_plugin;
        }
    };

    if !plugin_root.as_path().is_dir() {
        loaded_plugin.error = Some("path does not exist or is not a directory".to_string());
        return loaded_plugin;
    }

    let Some(manifest) = load_plugin_manifest(plugin_root.as_path()) else {
        loaded_plugin.error = Some("missing or invalid .nexal-plugin/plugin.json".to_string());
        return loaded_plugin;
    };

    let manifest_paths = &manifest.paths;
    loaded_plugin.manifest_name = manifest
        .interface
        .as_ref()
        .and_then(|interface| interface.display_name.as_deref())
        .map(str::trim)
        .filter(|display_name| !display_name.is_empty())
        .map(str::to_string)
        .or_else(|| Some(manifest.name.clone()));
    loaded_plugin.manifest_description = manifest.description.clone();
    loaded_plugin.skill_roots = plugin_skill_roots(plugin_root.as_path(), manifest_paths);
    let resolved_skills = load_plugin_skills(
        plugin_root.as_path(),
        manifest_paths,
        restriction_product,
        skill_config_rules,
    );
    let has_enabled_skills = resolved_skills.has_enabled_skills();
    loaded_plugin.disabled_skill_paths = resolved_skills.disabled_skill_paths;
    loaded_plugin.has_enabled_skills = has_enabled_skills;
    let mut mcp_servers = HashMap::new();
    for mcp_config_path in plugin_mcp_config_paths(plugin_root.as_path(), manifest_paths) {
        let plugin_mcp = load_mcp_servers_from_file(plugin_root.as_path(), &mcp_config_path);
        for (name, config) in plugin_mcp.mcp_servers {
            if mcp_servers.insert(name.clone(), config).is_some() {
                warn!(
                    plugin = %plugin_root.display(),
                    path = %mcp_config_path.display(),
                    server = name,
                    "plugin MCP file overwrote an earlier server definition"
                );
            }
        }
    }
    loaded_plugin.mcp_servers = mcp_servers;
    loaded_plugin.apps = load_plugin_apps(plugin_root.as_path());
    loaded_plugin
}

struct ResolvedPluginSkills {
    skills: Vec<SkillMetadata>,
    disabled_skill_paths: HashSet<PathBuf>,
    had_errors: bool,
}

impl ResolvedPluginSkills {
    fn has_enabled_skills(&self) -> bool {
        // Keep the plugin visible in capability summaries if skill loading was partial.
        self.had_errors
            || self
                .skills
                .iter()
                .any(|skill| !self.disabled_skill_paths.contains(&skill.path_to_skills_md))
    }
}

fn load_plugin_skills(
    plugin_root: &Path,
    manifest_paths: &PluginManifestPaths,
    restriction_product: Option<Product>,
    skill_config_rules: &SkillConfigRules,
) -> ResolvedPluginSkills {
    let outcome = load_skills_from_roots(
        plugin_skill_roots(plugin_root, manifest_paths)
            .into_iter()
            .map(|path| SkillRoot {
                path,
                scope: SkillScope::User,
            }),
    );
    let had_errors = !outcome.errors.is_empty();
    let skills = outcome
        .skills
        .into_iter()
        .filter(|skill| skill.matches_product_restriction_for_product(restriction_product))
        .collect::<Vec<_>>();
    let disabled_skill_paths = resolve_disabled_skill_paths(&skills, skill_config_rules);

    ResolvedPluginSkills {
        skills,
        disabled_skill_paths,
        had_errors,
    }
}

fn plugin_skill_roots(plugin_root: &Path, manifest_paths: &PluginManifestPaths) -> Vec<PathBuf> {
    let mut paths = default_skill_roots(plugin_root);
    if let Some(path) = &manifest_paths.skills {
        paths.push(path.to_path_buf());
    }
    paths.sort_unstable();
    paths.dedup();
    paths
}

fn default_skill_roots(plugin_root: &Path) -> Vec<PathBuf> {
    let skills_dir = plugin_root.join(DEFAULT_SKILLS_DIR_NAME);
    if skills_dir.is_dir() {
        vec![skills_dir]
    } else {
        Vec::new()
    }
}

fn plugin_mcp_config_paths(
    plugin_root: &Path,
    manifest_paths: &PluginManifestPaths,
) -> Vec<AbsolutePathBuf> {
    if let Some(path) = &manifest_paths.mcp_servers {
        return vec![path.clone()];
    }
    default_mcp_config_paths(plugin_root)
}

fn default_mcp_config_paths(plugin_root: &Path) -> Vec<AbsolutePathBuf> {
    let mut paths = Vec::new();
    let default_path = plugin_root.join(DEFAULT_MCP_CONFIG_FILE);
    if default_path.is_file()
        && let Ok(default_path) = AbsolutePathBuf::try_from(default_path)
    {
        paths.push(default_path);
    }
    paths.sort_unstable_by(|left, right| left.as_path().cmp(right.as_path()));
    paths.dedup_by(|left, right| left.as_path() == right.as_path());
    paths
}

pub fn load_plugin_apps(plugin_root: &Path) -> Vec<AppConnectorId> {
    if let Some(manifest) = load_plugin_manifest(plugin_root) {
        return load_apps_from_paths(
            plugin_root,
            plugin_app_config_paths(plugin_root, &manifest.paths),
        );
    }
    load_apps_from_paths(plugin_root, default_app_config_paths(plugin_root))
}

fn plugin_app_config_paths(
    plugin_root: &Path,
    manifest_paths: &PluginManifestPaths,
) -> Vec<AbsolutePathBuf> {
    if let Some(path) = &manifest_paths.apps {
        return vec![path.clone()];
    }
    default_app_config_paths(plugin_root)
}

fn default_app_config_paths(plugin_root: &Path) -> Vec<AbsolutePathBuf> {
    let mut paths = Vec::new();
    let default_path = plugin_root.join(DEFAULT_APP_CONFIG_FILE);
    if default_path.is_file()
        && let Ok(default_path) = AbsolutePathBuf::try_from(default_path)
    {
        paths.push(default_path);
    }
    paths.sort_unstable_by(|left, right| left.as_path().cmp(right.as_path()));
    paths.dedup_by(|left, right| left.as_path() == right.as_path());
    paths
}

fn load_apps_from_paths(
    plugin_root: &Path,
    app_config_paths: Vec<AbsolutePathBuf>,
) -> Vec<AppConnectorId> {
    let mut connector_ids = Vec::new();
    for app_config_path in app_config_paths {
        let Ok(contents) = fs::read_to_string(app_config_path.as_path()) else {
            continue;
        };
        let parsed = match serde_json::from_str::<PluginAppFile>(&contents) {
            Ok(parsed) => parsed,
            Err(err) => {
                warn!(
                    path = %app_config_path.display(),
                    "failed to parse plugin app config: {err}"
                );
                continue;
            }
        };

        let mut apps: Vec<PluginAppConfig> = parsed.apps.into_values().collect();
        apps.sort_unstable_by(|left, right| left.id.cmp(&right.id));

        connector_ids.extend(apps.into_iter().filter_map(|app| {
            if app.id.trim().is_empty() {
                warn!(
                    plugin = %plugin_root.display(),
                    "plugin app config is missing an app id"
                );
                None
            } else {
                Some(AppConnectorId(app.id))
            }
        }));
    }
    connector_ids.dedup();
    connector_ids
}

pub fn plugin_telemetry_metadata_from_root(
    plugin_id: &PluginId,
    plugin_root: &Path,
) -> PluginTelemetryMetadata {
    let Some(manifest) = load_plugin_manifest(plugin_root) else {
        return PluginTelemetryMetadata::from_plugin_id(plugin_id);
    };

    let manifest_paths = &manifest.paths;
    let has_skills = !plugin_skill_roots(plugin_root, manifest_paths).is_empty();
    let mut mcp_server_names = Vec::new();
    for path in plugin_mcp_config_paths(plugin_root, manifest_paths) {
        mcp_server_names.extend(
            load_mcp_servers_from_file(plugin_root, &path)
                .mcp_servers
                .into_keys(),
        );
    }
    mcp_server_names.sort_unstable();
    mcp_server_names.dedup();

    PluginTelemetryMetadata {
        plugin_id: plugin_id.clone(),
        capability_summary: Some(PluginCapabilitySummary {
            config_name: plugin_id.as_key(),
            display_name: plugin_id.plugin_name.clone(),
            description: None,
            has_skills,
            mcp_server_names,
            app_connector_ids: load_plugin_apps(plugin_root),
        }),
    }
}

pub fn load_plugin_mcp_servers(plugin_root: &Path) -> HashMap<String, McpServerConfig> {
    let Some(manifest) = load_plugin_manifest(plugin_root) else {
        return HashMap::new();
    };

    let mut mcp_servers = HashMap::new();
    for mcp_config_path in plugin_mcp_config_paths(plugin_root, &manifest.paths) {
        let plugin_mcp = load_mcp_servers_from_file(plugin_root, &mcp_config_path);
        for (name, config) in plugin_mcp.mcp_servers {
            mcp_servers.entry(name).or_insert(config);
        }
    }

    mcp_servers
}


fn load_mcp_servers_from_file(
    plugin_root: &Path,
    mcp_config_path: &AbsolutePathBuf,
) -> PluginMcpDiscovery {
    let Ok(contents) = fs::read_to_string(mcp_config_path.as_path()) else {
        return PluginMcpDiscovery::default();
    };
    let parsed = match serde_json::from_str::<PluginMcpFile>(&contents) {
        Ok(parsed) => parsed,
        Err(err) => {
            warn!(
                path = %mcp_config_path.display(),
                "failed to parse plugin MCP config: {err}"
            );
            return PluginMcpDiscovery::default();
        }
    };
    normalize_plugin_mcp_servers(
        plugin_root,
        parsed.mcp_servers,
        mcp_config_path.to_string_lossy().as_ref(),
    )
}

fn normalize_plugin_mcp_servers(
    plugin_root: &Path,
    plugin_mcp_servers: HashMap<String, JsonValue>,
    source: &str,
) -> PluginMcpDiscovery {
    let mut mcp_servers = HashMap::new();

    for (name, config_value) in plugin_mcp_servers {
        let normalized = normalize_plugin_mcp_server_value(plugin_root, config_value);
        match serde_json::from_value::<McpServerConfig>(JsonValue::Object(normalized)) {
            Ok(config) => {
                mcp_servers.insert(name, config);
            }
            Err(err) => {
                warn!(
                    plugin = %plugin_root.display(),
                    server = name,
                    "failed to parse plugin MCP server from {source}: {err}"
                );
            }
        }
    }

    PluginMcpDiscovery { mcp_servers }
}

fn normalize_plugin_mcp_server_value(
    plugin_root: &Path,
    value: JsonValue,
) -> JsonMap<String, JsonValue> {
    let mut object = match value {
        JsonValue::Object(object) => object,
        _ => return JsonMap::new(),
    };

    if let Some(JsonValue::String(transport_type)) = object.remove("type") {
        match transport_type.as_str() {
            "http" | "streamable_http" | "streamable-http" => {}
            "stdio" => {}
            other => {
                warn!(
                    plugin = %plugin_root.display(),
                    transport = other,
                    "plugin MCP server uses an unknown transport type"
                );
            }
        }
    }

    if let Some(JsonValue::Object(oauth)) = object.remove("oauth")
        && oauth.contains_key("callbackPort")
    {
        warn!(
            plugin = %plugin_root.display(),
            "plugin MCP server OAuth callbackPort is ignored; Nexal uses global MCP OAuth callback settings"
        );
    }

    if let Some(JsonValue::String(cwd)) = object.get("cwd")
        && !Path::new(cwd).is_absolute()
    {
        object.insert(
            "cwd".to_string(),
            JsonValue::String(plugin_root.join(cwd).display().to_string()),
        );
    }

    object
}

#[derive(Debug, Default)]
struct PluginMcpDiscovery {
    mcp_servers: HashMap<String, McpServerConfig>,
}

#[cfg(test)]
#[path = "manager_tests.rs"]
mod tests;
