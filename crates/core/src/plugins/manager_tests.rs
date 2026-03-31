use super::*;
use crate::config::CONFIG_TOML_FILE;
use crate::config::ConfigBuilder;
use crate::config::types::McpServerTransportConfig;
use crate::config_loader::ConfigLayerEntry;
use crate::config_loader::ConfigLayerStack;
use crate::config_loader::ConfigRequirements;
use crate::config_loader::ConfigRequirementsToml;
use crate::plugins::LoadedPlugin;
use crate::plugins::PluginLoadOutcome;
use crate::plugins::test_support::write_file;
use nexal_app_server_protocol::ConfigLayerSource;
use nexal_protocol::protocol::Product;
use pretty_assertions::assert_eq;
use tempfile::TempDir;
use toml::Value;

const MAX_CAPABILITY_SUMMARY_DESCRIPTION_LEN: usize = 1024;

fn plugin_config_toml(enabled: bool, plugins_feature_enabled: bool) -> String {
    let mut root = toml::map::Map::new();

    let mut features = toml::map::Map::new();
    features.insert(
        "plugins".to_string(),
        Value::Boolean(plugins_feature_enabled),
    );
    root.insert("features".to_string(), Value::Table(features));

    let mut plugin = toml::map::Map::new();
    plugin.insert("enabled".to_string(), Value::Boolean(enabled));

    let mut plugins = toml::map::Map::new();
    plugins.insert("sample@test".to_string(), Value::Table(plugin));
    root.insert("plugins".to_string(), Value::Table(plugins));

    toml::to_string(&Value::Table(root)).expect("plugin test config should serialize")
}

fn load_plugins_from_config(config_toml: &str, nexal_home: &Path) -> PluginLoadOutcome {
    write_file(&nexal_home.join(CONFIG_TOML_FILE), config_toml);
    let config = load_config_blocking(nexal_home, nexal_home);
    PluginsManager::new(nexal_home.to_path_buf()).plugins_for_config(&config)
}

async fn load_config(nexal_home: &Path, cwd: &Path) -> crate::config::Config {
    ConfigBuilder::default()
        .nexal_home(nexal_home.to_path_buf())
        .fallback_cwd(Some(cwd.to_path_buf()))
        .build()
        .await
        .expect("config should load")
}

fn load_config_blocking(nexal_home: &Path, cwd: &Path) -> crate::config::Config {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime should build")
        .block_on(load_config(nexal_home, cwd))
}

#[test]
fn load_plugins_loads_default_skills_and_mcp_servers() {
    let nexal_home = TempDir::new().unwrap();
    let plugin_root = nexal_home
        .path()
        .join("plugins/cache")
        .join("test/sample/local");

    write_file(
        &plugin_root.join(".nexal-plugin/plugin.json"),
        r#"{
  "name": "sample",
  "description": "Plugin that includes the sample MCP server and Skills"
}"#,
    );
    write_file(
        &plugin_root.join("skills/sample-search/SKILL.md"),
        "---\nname: sample-search\ndescription: search sample data\n---\n",
    );
    write_file(
        &plugin_root.join(".mcp.json"),
        r#"{
  "mcpServers": {
    "sample": {
      "type": "http",
      "url": "https://sample.example/mcp",
      "oauth": {
        "clientId": "client-id",
        "callbackPort": 3118
      }
    }
  }
}"#,
    );
    write_file(
        &plugin_root.join(".app.json"),
        r#"{
  "apps": {
    "example": {
      "id": "connector_example"
    }
  }
}"#,
    );

    let outcome = load_plugins_from_config(&plugin_config_toml(true, true), nexal_home.path());

    assert_eq!(
        outcome.plugins(),
        vec![LoadedPlugin {
            config_name: "sample@test".to_string(),
            manifest_name: Some("sample".to_string()),
            manifest_description: Some(
                "Plugin that includes the sample MCP server and Skills".to_string(),
            ),
            root: AbsolutePathBuf::try_from(plugin_root.clone()).unwrap(),
            enabled: true,
            skill_roots: vec![plugin_root.join("skills")],
            disabled_skill_paths: HashSet::new(),
            has_enabled_skills: true,
            mcp_servers: HashMap::from([(
                "sample".to_string(),
                McpServerConfig {
                    transport: McpServerTransportConfig::StreamableHttp {
                        url: "https://sample.example/mcp".to_string(),
                        bearer_token_env_var: None,
                        http_headers: None,
                        env_http_headers: None,
                    },
                    enabled: true,
                    required: false,
                    disabled_reason: None,
                    startup_timeout_sec: None,
                    tool_timeout_sec: None,
                    enabled_tools: None,
                    disabled_tools: None,
                    scopes: None,
                    oauth_resource: None,
                    tools: HashMap::new(),
                },
            )]),
            apps: vec![AppConnectorId("connector_example".to_string())],
            error: None,
        }]
    );
    assert_eq!(
        outcome.capability_summaries(),
        &[PluginCapabilitySummary {
            config_name: "sample@test".to_string(),
            display_name: "sample".to_string(),
            description: Some("Plugin that includes the sample MCP server and Skills".to_string(),),
            has_skills: true,
            mcp_server_names: vec!["sample".to_string()],
            app_connector_ids: vec![AppConnectorId("connector_example".to_string())],
        }]
    );
    assert_eq!(
        outcome.effective_skill_roots(),
        vec![plugin_root.join("skills")]
    );
    assert_eq!(outcome.effective_mcp_servers().len(), 1);
    assert_eq!(
        outcome.effective_apps(),
        vec![AppConnectorId("connector_example".to_string())]
    );
}

#[test]
fn load_plugins_resolves_disabled_skill_names_against_loaded_plugin_skills() {
    let nexal_home = TempDir::new().unwrap();
    let plugin_root = nexal_home
        .path()
        .join("plugins/cache")
        .join("test/sample/local");
    let skill_path = plugin_root.join("skills/sample-search/SKILL.md");

    write_file(
        &plugin_root.join(".nexal-plugin/plugin.json"),
        r#"{"name":"sample"}"#,
    );
    write_file(
        &skill_path,
        "---\nname: sample-search\ndescription: search sample data\n---\n",
    );

    let config_toml = r#"[features]
plugins = true

[[skills.config]]
name = "sample:sample-search"
enabled = false

[plugins."sample@test"]
enabled = true
"#;
    let outcome = load_plugins_from_config(config_toml, nexal_home.path());
    let skill_path = dunce::canonicalize(skill_path).expect("skill path should canonicalize");

    assert_eq!(
        outcome.plugins()[0].disabled_skill_paths,
        HashSet::from([skill_path])
    );
    assert!(!outcome.plugins()[0].has_enabled_skills);
    assert!(outcome.capability_summaries().is_empty());
}

#[test]
fn load_plugins_ignores_unknown_disabled_skill_names() {
    let nexal_home = TempDir::new().unwrap();
    let plugin_root = nexal_home
        .path()
        .join("plugins/cache")
        .join("test/sample/local");

    write_file(
        &plugin_root.join(".nexal-plugin/plugin.json"),
        r#"{"name":"sample"}"#,
    );
    write_file(
        &plugin_root.join("skills/sample-search/SKILL.md"),
        "---\nname: sample-search\ndescription: search sample data\n---\n",
    );

    let config_toml = r#"[features]
plugins = true

[[skills.config]]
name = "sample:missing-skill"
enabled = false

[plugins."sample@test"]
enabled = true
"#;
    let outcome = load_plugins_from_config(config_toml, nexal_home.path());

    assert!(outcome.plugins()[0].disabled_skill_paths.is_empty());
    assert!(outcome.plugins()[0].has_enabled_skills);
    assert_eq!(
        outcome.capability_summaries(),
        &[PluginCapabilitySummary {
            config_name: "sample@test".to_string(),
            display_name: "sample".to_string(),
            description: None,
            has_skills: true,
            mcp_server_names: Vec::new(),
            app_connector_ids: Vec::new(),
        }]
    );
}

#[test]
fn plugin_telemetry_metadata_uses_default_mcp_config_path() {
    let nexal_home = TempDir::new().unwrap();
    let plugin_root = nexal_home
        .path()
        .join("plugins/cache")
        .join("test/sample/local");

    write_file(
        &plugin_root.join(".nexal-plugin/plugin.json"),
        r#"{
  "name": "sample"
}"#,
    );
    write_file(
        &plugin_root.join(".mcp.json"),
        r#"{
  "mcpServers": {
    "sample": {
      "type": "http",
      "url": "https://sample.example/mcp"
    }
  }
}"#,
    );

    let metadata = plugin_telemetry_metadata_from_root(
        &PluginId::parse("sample@test").expect("plugin id should parse"),
        &plugin_root,
    );

    assert_eq!(
        metadata.capability_summary,
        Some(PluginCapabilitySummary {
            config_name: "sample@test".to_string(),
            display_name: "sample".to_string(),
            description: None,
            has_skills: false,
            mcp_server_names: vec!["sample".to_string()],
            app_connector_ids: Vec::new(),
        })
    );
}

#[test]
fn capability_summary_sanitizes_plugin_descriptions_to_one_line() {
    let nexal_home = TempDir::new().unwrap();
    let plugin_root = nexal_home
        .path()
        .join("plugins/cache")
        .join("test/sample/local");

    write_file(
        &plugin_root.join(".nexal-plugin/plugin.json"),
        r#"{
  "name": "sample",
  "description": "Plugin that\n includes   the sample\tserver"
}"#,
    );
    write_file(
        &plugin_root.join("skills/sample-search/SKILL.md"),
        "---\nname: sample-search\ndescription: search sample data\n---\n",
    );

    let outcome = load_plugins_from_config(&plugin_config_toml(true, true), nexal_home.path());

    assert_eq!(
        outcome.plugins()[0].manifest_description.as_deref(),
        Some("Plugin that\n includes   the sample\tserver")
    );
    assert_eq!(
        outcome.capability_summaries()[0].description.as_deref(),
        Some("Plugin that includes the sample server")
    );
}

#[test]
fn capability_summary_truncates_overlong_plugin_descriptions() {
    let nexal_home = TempDir::new().unwrap();
    let plugin_root = nexal_home
        .path()
        .join("plugins/cache")
        .join("test/sample/local");
    let too_long = "x".repeat(MAX_CAPABILITY_SUMMARY_DESCRIPTION_LEN + 1);

    write_file(
        &plugin_root.join(".nexal-plugin/plugin.json"),
        &format!(
            r#"{{
  "name": "sample",
  "description": "{too_long}"
}}"#
        ),
    );
    write_file(
        &plugin_root.join("skills/sample-search/SKILL.md"),
        "---\nname: sample-search\ndescription: search sample data\n---\n",
    );

    let outcome = load_plugins_from_config(&plugin_config_toml(true, true), nexal_home.path());

    assert_eq!(
        outcome.plugins()[0].manifest_description.as_deref(),
        Some(too_long.as_str())
    );
    assert_eq!(
        outcome.capability_summaries()[0].description,
        Some("x".repeat(MAX_CAPABILITY_SUMMARY_DESCRIPTION_LEN))
    );
}

#[test]
fn load_plugins_uses_manifest_configured_component_paths() {
    let nexal_home = TempDir::new().unwrap();
    let plugin_root = nexal_home
        .path()
        .join("plugins/cache")
        .join("test/sample/local");

    write_file(
        &plugin_root.join(".nexal-plugin/plugin.json"),
        r#"{
  "name": "sample",
  "skills": "./custom-skills/",
  "mcpServers": "./config/custom.mcp.json",
  "apps": "./config/custom.app.json"
}"#,
    );
    write_file(
        &plugin_root.join("skills/default-skill/SKILL.md"),
        "---\nname: default-skill\ndescription: default skill\n---\n",
    );
    write_file(
        &plugin_root.join("custom-skills/custom-skill/SKILL.md"),
        "---\nname: custom-skill\ndescription: custom skill\n---\n",
    );
    write_file(
        &plugin_root.join(".mcp.json"),
        r#"{
  "mcpServers": {
    "default": {
      "type": "http",
      "url": "https://default.example/mcp"
    }
  }
}"#,
    );
    write_file(
        &plugin_root.join("config/custom.mcp.json"),
        r#"{
  "mcpServers": {
    "custom": {
      "type": "http",
      "url": "https://custom.example/mcp"
    }
  }
}"#,
    );
    write_file(
        &plugin_root.join(".app.json"),
        r#"{
  "apps": {
    "default": {
      "id": "connector_default"
    }
  }
}"#,
    );
    write_file(
        &plugin_root.join("config/custom.app.json"),
        r#"{
  "apps": {
    "custom": {
      "id": "connector_custom"
    }
  }
}"#,
    );

    let outcome = load_plugins_from_config(&plugin_config_toml(true, true), nexal_home.path());

    assert_eq!(
        outcome.plugins()[0].skill_roots,
        vec![
            plugin_root.join("custom-skills"),
            plugin_root.join("skills")
        ]
    );
    assert_eq!(
        outcome.plugins()[0].mcp_servers,
        HashMap::from([(
            "custom".to_string(),
            McpServerConfig {
                transport: McpServerTransportConfig::StreamableHttp {
                    url: "https://custom.example/mcp".to_string(),
                    bearer_token_env_var: None,
                    http_headers: None,
                    env_http_headers: None,
                },
                enabled: true,
                required: false,
                disabled_reason: None,
                startup_timeout_sec: None,
                tool_timeout_sec: None,
                enabled_tools: None,
                disabled_tools: None,
                scopes: None,
                oauth_resource: None,
                tools: HashMap::new(),
            },
        )])
    );
    assert_eq!(
        outcome.plugins()[0].apps,
        vec![AppConnectorId("connector_custom".to_string())]
    );
}

#[test]
fn load_plugins_ignores_manifest_component_paths_without_dot_slash() {
    let nexal_home = TempDir::new().unwrap();
    let plugin_root = nexal_home
        .path()
        .join("plugins/cache")
        .join("test/sample/local");

    write_file(
        &plugin_root.join(".nexal-plugin/plugin.json"),
        r#"{
  "name": "sample",
  "skills": "custom-skills",
  "mcpServers": "config/custom.mcp.json",
  "apps": "config/custom.app.json"
}"#,
    );
    write_file(
        &plugin_root.join("skills/default-skill/SKILL.md"),
        "---\nname: default-skill\ndescription: default skill\n---\n",
    );
    write_file(
        &plugin_root.join("custom-skills/custom-skill/SKILL.md"),
        "---\nname: custom-skill\ndescription: custom skill\n---\n",
    );
    write_file(
        &plugin_root.join(".mcp.json"),
        r#"{
  "mcpServers": {
    "default": {
      "type": "http",
      "url": "https://default.example/mcp"
    }
  }
}"#,
    );
    write_file(
        &plugin_root.join("config/custom.mcp.json"),
        r#"{
  "mcpServers": {
    "custom": {
      "type": "http",
      "url": "https://custom.example/mcp"
    }
  }
}"#,
    );
    write_file(
        &plugin_root.join(".app.json"),
        r#"{
  "apps": {
    "default": {
      "id": "connector_default"
    }
  }
}"#,
    );
    write_file(
        &plugin_root.join("config/custom.app.json"),
        r#"{
  "apps": {
    "custom": {
      "id": "connector_custom"
    }
  }
}"#,
    );

    let outcome = load_plugins_from_config(&plugin_config_toml(true, true), nexal_home.path());

    assert_eq!(
        outcome.plugins()[0].skill_roots,
        vec![plugin_root.join("skills")]
    );
    assert_eq!(
        outcome.plugins()[0].mcp_servers,
        HashMap::from([(
            "default".to_string(),
            McpServerConfig {
                transport: McpServerTransportConfig::StreamableHttp {
                    url: "https://default.example/mcp".to_string(),
                    bearer_token_env_var: None,
                    http_headers: None,
                    env_http_headers: None,
                },
                enabled: true,
                required: false,
                disabled_reason: None,
                startup_timeout_sec: None,
                tool_timeout_sec: None,
                enabled_tools: None,
                disabled_tools: None,
                scopes: None,
                oauth_resource: None,
                tools: HashMap::new(),
            },
        )])
    );
    assert_eq!(
        outcome.plugins()[0].apps,
        vec![AppConnectorId("connector_default".to_string())]
    );
}

#[test]
fn load_plugins_preserves_disabled_plugins_without_effective_contributions() {
    let nexal_home = TempDir::new().unwrap();
    let plugin_root = nexal_home
        .path()
        .join("plugins/cache")
        .join("test/sample/local");

    write_file(
        &plugin_root.join(".nexal-plugin/plugin.json"),
        r#"{"name":"sample"}"#,
    );
    write_file(
        &plugin_root.join(".mcp.json"),
        r#"{
  "mcpServers": {
    "sample": {
      "type": "http",
      "url": "https://sample.example/mcp"
    }
  }
}"#,
    );

    let outcome = load_plugins_from_config(&plugin_config_toml(false, true), nexal_home.path());

    assert_eq!(
        outcome.plugins(),
        vec![LoadedPlugin {
            config_name: "sample@test".to_string(),
            manifest_name: None,
            manifest_description: None,
            root: AbsolutePathBuf::try_from(plugin_root).unwrap(),
            enabled: false,
            skill_roots: Vec::new(),
            disabled_skill_paths: HashSet::new(),
            has_enabled_skills: false,
            mcp_servers: HashMap::new(),
            apps: Vec::new(),
            error: None,
        }]
    );
    assert!(outcome.effective_skill_roots().is_empty());
    assert!(outcome.effective_mcp_servers().is_empty());
}

#[test]
fn effective_apps_dedupes_connector_ids_across_plugins() {
    let nexal_home = TempDir::new().unwrap();
    let plugin_a_root = nexal_home
        .path()
        .join("plugins/cache")
        .join("test/plugin-a/local");
    let plugin_b_root = nexal_home
        .path()
        .join("plugins/cache")
        .join("test/plugin-b/local");

    write_file(
        &plugin_a_root.join(".nexal-plugin/plugin.json"),
        r#"{"name":"plugin-a"}"#,
    );
    write_file(
        &plugin_a_root.join(".app.json"),
        r#"{
  "apps": {
    "example": {
      "id": "connector_example"
    }
  }
}"#,
    );
    write_file(
        &plugin_b_root.join(".nexal-plugin/plugin.json"),
        r#"{"name":"plugin-b"}"#,
    );
    write_file(
        &plugin_b_root.join(".app.json"),
        r#"{
  "apps": {
    "chat": {
      "id": "connector_example"
    },
    "gmail": {
      "id": "connector_gmail"
    }
  }
}"#,
    );

    let mut root = toml::map::Map::new();
    let mut features = toml::map::Map::new();
    features.insert("plugins".to_string(), Value::Boolean(true));
    root.insert("features".to_string(), Value::Table(features));

    let mut plugins = toml::map::Map::new();

    let mut plugin_a = toml::map::Map::new();
    plugin_a.insert("enabled".to_string(), Value::Boolean(true));
    plugins.insert("plugin-a@test".to_string(), Value::Table(plugin_a));

    let mut plugin_b = toml::map::Map::new();
    plugin_b.insert("enabled".to_string(), Value::Boolean(true));
    plugins.insert("plugin-b@test".to_string(), Value::Table(plugin_b));

    root.insert("plugins".to_string(), Value::Table(plugins));
    let config_toml =
        toml::to_string(&Value::Table(root)).expect("plugin test config should serialize");

    let outcome = load_plugins_from_config(&config_toml, nexal_home.path());

    assert_eq!(
        outcome.effective_apps(),
        vec![
            AppConnectorId("connector_example".to_string()),
            AppConnectorId("connector_gmail".to_string()),
        ]
    );
}

#[test]
fn capability_index_filters_inactive_and_zero_capability_plugins() {
    let nexal_home = TempDir::new().unwrap();
    let connector = |id: &str| AppConnectorId(id.to_string());
    let http_server = |url: &str| McpServerConfig {
        transport: McpServerTransportConfig::StreamableHttp {
            url: url.to_string(),
            bearer_token_env_var: None,
            http_headers: None,
            env_http_headers: None,
        },
        enabled: true,
        required: false,
        disabled_reason: None,
        startup_timeout_sec: None,
        tool_timeout_sec: None,
        enabled_tools: None,
        disabled_tools: None,
        scopes: None,
        oauth_resource: None,
        tools: HashMap::new(),
    };
    let plugin = |config_name: &str, dir_name: &str, manifest_name: &str| LoadedPlugin {
        config_name: config_name.to_string(),
        manifest_name: Some(manifest_name.to_string()),
        manifest_description: None,
        root: AbsolutePathBuf::try_from(nexal_home.path().join(dir_name)).unwrap(),
        enabled: true,
        skill_roots: Vec::new(),
        disabled_skill_paths: HashSet::new(),
        has_enabled_skills: false,
        mcp_servers: HashMap::new(),
        apps: Vec::new(),
        error: None,
    };
    let summary = |config_name: &str, display_name: &str| PluginCapabilitySummary {
        config_name: config_name.to_string(),
        display_name: display_name.to_string(),
        description: None,
        ..PluginCapabilitySummary::default()
    };
    let outcome = PluginLoadOutcome::from_plugins(vec![
        LoadedPlugin {
            skill_roots: vec![nexal_home.path().join("skills-plugin/skills")],
            has_enabled_skills: true,
            ..plugin("skills@test", "skills-plugin", "skills-plugin")
        },
        LoadedPlugin {
            mcp_servers: HashMap::from([("alpha".to_string(), http_server("https://alpha"))]),
            apps: vec![connector("connector_example")],
            ..plugin("alpha@test", "alpha-plugin", "alpha-plugin")
        },
        LoadedPlugin {
            mcp_servers: HashMap::from([("beta".to_string(), http_server("https://beta"))]),
            apps: vec![connector("connector_example"), connector("connector_gmail")],
            ..plugin("beta@test", "beta-plugin", "beta-plugin")
        },
        plugin("empty@test", "empty-plugin", "empty-plugin"),
        LoadedPlugin {
            enabled: false,
            skill_roots: vec![nexal_home.path().join("disabled-plugin/skills")],
            apps: vec![connector("connector_hidden")],
            ..plugin("disabled@test", "disabled-plugin", "disabled-plugin")
        },
        LoadedPlugin {
            apps: vec![connector("connector_broken")],
            error: Some("failed to load".to_string()),
            ..plugin("broken@test", "broken-plugin", "broken-plugin")
        },
    ]);

    assert_eq!(
        outcome.capability_summaries(),
        &[
            PluginCapabilitySummary {
                has_skills: true,
                ..summary("skills@test", "skills-plugin")
            },
            PluginCapabilitySummary {
                mcp_server_names: vec!["alpha".to_string()],
                app_connector_ids: vec![connector("connector_example")],
                ..summary("alpha@test", "alpha-plugin")
            },
            PluginCapabilitySummary {
                mcp_server_names: vec!["beta".to_string()],
                app_connector_ids: vec![
                    connector("connector_example"),
                    connector("connector_gmail"),
                ],
                ..summary("beta@test", "beta-plugin")
            },
        ]
    );
}

#[test]
fn load_plugins_returns_empty_when_feature_disabled() {
    let nexal_home = TempDir::new().unwrap();
    let plugin_root = nexal_home
        .path()
        .join("plugins/cache")
        .join("test/sample/local");

    write_file(
        &plugin_root.join(".nexal-plugin/plugin.json"),
        r#"{"name":"sample"}"#,
    );
    write_file(
        &plugin_root.join("skills/sample-search/SKILL.md"),
        "---\nname: sample-search\ndescription: search sample data\n---\n",
    );
    write_file(
        &nexal_home.path().join(CONFIG_TOML_FILE),
        &plugin_config_toml(true, false),
    );

    let config = load_config_blocking(nexal_home.path(), nexal_home.path());
    let outcome = PluginsManager::new(nexal_home.path().to_path_buf()).plugins_for_config(&config);

    assert_eq!(outcome, PluginLoadOutcome::default());
}

#[test]
fn load_plugins_rejects_invalid_plugin_keys() {
    let nexal_home = TempDir::new().unwrap();
    let plugin_root = nexal_home
        .path()
        .join("plugins/cache")
        .join("test/sample/local");

    write_file(
        &plugin_root.join(".nexal-plugin/plugin.json"),
        r#"{"name":"sample"}"#,
    );

    let mut root = toml::map::Map::new();
    let mut features = toml::map::Map::new();
    features.insert("plugins".to_string(), Value::Boolean(true));
    root.insert("features".to_string(), Value::Table(features));

    let mut plugin = toml::map::Map::new();
    plugin.insert("enabled".to_string(), Value::Boolean(true));

    let mut plugins = toml::map::Map::new();
    plugins.insert("sample".to_string(), Value::Table(plugin));
    root.insert("plugins".to_string(), Value::Table(plugins));

    let outcome = load_plugins_from_config(
        &toml::to_string(&Value::Table(root)).expect("plugin test config should serialize"),
        nexal_home.path(),
    );

    assert_eq!(outcome.plugins().len(), 1);
    assert_eq!(
        outcome.plugins()[0].error.as_deref(),
        Some("invalid plugin key `sample`; expected <plugin>@<marketplace>")
    );
    assert!(outcome.effective_skill_roots().is_empty());
    assert!(outcome.effective_mcp_servers().is_empty());
}

#[test]
fn load_plugins_ignores_project_config_files() {
    let nexal_home = TempDir::new().unwrap();
    let project_root = nexal_home.path().join("project");
    let plugin_root = nexal_home
        .path()
        .join("plugins/cache")
        .join("test/sample/local");

    write_file(
        &plugin_root.join(".nexal-plugin/plugin.json"),
        r#"{"name":"sample"}"#,
    );
    write_file(
        &project_root.join(".nexal/config.toml"),
        &plugin_config_toml(true, true),
    );

    let plugin_cache_root =
        AbsolutePathBuf::try_from(nexal_home.path().join(PLUGINS_CACHE_DIR)).unwrap();

    let stack = ConfigLayerStack::new(
        vec![ConfigLayerEntry::new(
            ConfigLayerSource::Project {
                dot_nexal_folder: AbsolutePathBuf::try_from(project_root.join(".nexal")).unwrap(),
            },
            toml::from_str(&plugin_config_toml(true, true)).expect("project config should parse"),
        )],
        ConfigRequirements::default(),
        ConfigRequirementsToml::default(),
    )
    .expect("config layer stack should build");

    let outcome = load_plugins_from_layer_stack(
        &stack,
        &plugin_cache_root,
        Some(Product::Nexal),
    );

    assert_eq!(outcome, PluginLoadOutcome::default());
}
