pub mod auth;
mod skill_dependencies;
pub(crate) use skill_dependencies::maybe_prompt_and_install_mcp_dependencies;

use std::collections::HashMap;
use std::env;
use std::path::PathBuf;
use std::sync::Arc;

use async_channel::unbounded;
use nexal_protocol::mcp::Resource;
use nexal_protocol::mcp::ResourceTemplate;
use nexal_protocol::mcp::Tool;
use nexal_protocol::protocol::McpListToolsResponseEvent;
use nexal_protocol::protocol::SandboxPolicy;
use serde_json::Value;

use crate::AuthManager;
use crate::NexalAuth;
use crate::config::Config;
use crate::config::types::McpServerConfig;
use crate::mcp::auth::compute_auth_statuses;
use crate::mcp_connection_manager::McpConnectionManager;
use crate::mcp_connection_manager::SandboxState;
use crate::mcp_connection_manager::nexal_apps_tools_cache_key;
use crate::plugins::PluginsManager;

const MCP_TOOL_NAME_PREFIX: &str = "mcp";
const MCP_TOOL_NAME_DELIMITER: &str = "__";
/// Name of the legacy nexal_apps MCP server (kept as a constant so that
/// existing code that filters on this name still compiles; no server with
/// this name will ever be injected).
pub(crate) const NEXAL_APPS_MCP_SERVER_NAME: &str = "nexal_apps";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ToolPluginProvenance;

impl ToolPluginProvenance {
    pub fn plugin_display_names_for_connector_id(&self, _connector_id: &str) -> &[String] {
        &[]
    }

    pub fn plugin_display_names_for_mcp_server_name(&self, _server_name: &str) -> &[String] {
        &[]
    }
}

/// No-op: nexal_apps MCP is removed. MCP servers come from user config only.
pub(crate) fn with_nexal_apps_mcp(
    servers: HashMap<String, McpServerConfig>,
    _connectors_enabled: bool,
    _auth: Option<&NexalAuth>,
    _config: &Config,
) -> HashMap<String, McpServerConfig> {
    servers
}

pub struct McpManager {
    // Kept for API compatibility; plugins no longer inject MCP servers.
    _plugins_manager: Arc<PluginsManager>,
}

impl McpManager {
    pub fn new(plugins_manager: Arc<PluginsManager>) -> Self {
        Self { _plugins_manager: plugins_manager }
    }

    pub fn configured_servers(&self, config: &Config) -> HashMap<String, McpServerConfig> {
        config.mcp_servers.get().clone()
    }

    pub fn effective_servers(
        &self,
        config: &Config,
        _auth: Option<&NexalAuth>,
    ) -> HashMap<String, McpServerConfig> {
        // Only user-configured MCP servers from [mcp] config section.
        config.mcp_servers.get().clone()
    }

    pub fn tool_plugin_provenance(&self, _config: &Config) -> ToolPluginProvenance {
        ToolPluginProvenance
    }
}

pub async fn collect_mcp_snapshot(config: &Config) -> McpListToolsResponseEvent {
    let auth_manager = AuthManager::shared(
        config.nexal_home.clone(),
        /*enable_nexal_api_key_env*/ false,
        config.cli_auth_credentials_store_mode,
    );
    let auth = auth_manager.auth().await;
    let mcp_manager = McpManager::new(Arc::new(PluginsManager::new(config.nexal_home.clone())));
    let mcp_servers = mcp_manager.effective_servers(config, auth.as_ref());
    let tool_plugin_provenance = mcp_manager.tool_plugin_provenance(config);
    if mcp_servers.is_empty() {
        return McpListToolsResponseEvent {
            tools: HashMap::new(),
            resources: HashMap::new(),
            resource_templates: HashMap::new(),
            auth_statuses: HashMap::new(),
        };
    }

    let auth_status_entries =
        compute_auth_statuses(mcp_servers.iter(), config.mcp_oauth_credentials_store_mode).await;

    let (tx_event, rx_event) = unbounded();
    drop(rx_event);

    // Use ReadOnly sandbox policy for MCP snapshot collection (safest default)
    let sandbox_state = SandboxState {
        sandbox_policy: SandboxPolicy::new_read_only_policy(),
        nexal_linux_sandbox_exe: config.nexal_linux_sandbox_exe.clone(),
        sandbox_cwd: env::current_dir().unwrap_or_else(|_| PathBuf::from("/")),
        use_legacy_landlock: config.features.use_legacy_landlock(),
    };

    let (mcp_connection_manager, cancel_token) = McpConnectionManager::new(
        &mcp_servers,
        config.mcp_oauth_credentials_store_mode,
        auth_status_entries.clone(),
        &config.permissions.approval_policy,
        tx_event,
        sandbox_state,
        config.nexal_home.clone(),
        nexal_apps_tools_cache_key(auth.as_ref()),
        tool_plugin_provenance,
    )
    .await;

    let snapshot =
        collect_mcp_snapshot_from_manager(&mcp_connection_manager, auth_status_entries).await;

    cancel_token.cancel();

    snapshot
}

pub fn split_qualified_tool_name(qualified_name: &str) -> Option<(String, String)> {
    let mut parts = qualified_name.split(MCP_TOOL_NAME_DELIMITER);
    let prefix = parts.next()?;
    if prefix != MCP_TOOL_NAME_PREFIX {
        return None;
    }
    let server_name = parts.next()?;
    let tool_name: String = parts.collect::<Vec<_>>().join(MCP_TOOL_NAME_DELIMITER);
    if tool_name.is_empty() {
        return None;
    }
    Some((server_name.to_string(), tool_name))
}

pub fn group_tools_by_server(
    tools: &HashMap<String, Tool>,
) -> HashMap<String, HashMap<String, Tool>> {
    let mut grouped = HashMap::new();
    for (qualified_name, tool) in tools {
        if let Some((server_name, tool_name)) = split_qualified_tool_name(qualified_name) {
            grouped
                .entry(server_name)
                .or_insert_with(HashMap::new)
                .insert(tool_name, tool.clone());
        }
    }
    grouped
}

pub(crate) async fn collect_mcp_snapshot_from_manager(
    mcp_connection_manager: &McpConnectionManager,
    auth_status_entries: HashMap<String, crate::mcp::auth::McpAuthStatusEntry>,
) -> McpListToolsResponseEvent {
    let (tools, resources, resource_templates) = tokio::join!(
        mcp_connection_manager.list_all_tools(),
        mcp_connection_manager.list_all_resources(),
        mcp_connection_manager.list_all_resource_templates(),
    );

    let auth_statuses = auth_status_entries
        .iter()
        .map(|(name, entry)| (name.clone(), entry.auth_status))
        .collect();

    let tools = tools
        .into_iter()
        .filter_map(|(name, tool)| match serde_json::to_value(tool.tool) {
            Ok(value) => match Tool::from_mcp_value(value) {
                Ok(tool) => Some((name, tool)),
                Err(err) => {
                    tracing::warn!("Failed to convert MCP tool '{name}': {err}");
                    None
                }
            },
            Err(err) => {
                tracing::warn!("Failed to serialize MCP tool '{name}': {err}");
                None
            }
        })
        .collect();

    let resources = resources
        .into_iter()
        .map(|(name, resources)| {
            let resources = resources
                .into_iter()
                .filter_map(|resource| match serde_json::to_value(resource) {
                    Ok(value) => match Resource::from_mcp_value(value.clone()) {
                        Ok(resource) => Some(resource),
                        Err(err) => {
                            let (uri, resource_name) = match value {
                                Value::Object(obj) => (
                                    obj.get("uri")
                                        .and_then(|v| v.as_str().map(ToString::to_string)),
                                    obj.get("name")
                                        .and_then(|v| v.as_str().map(ToString::to_string)),
                                ),
                                _ => (None, None),
                            };

                            tracing::warn!(
                                "Failed to convert MCP resource (uri={uri:?}, name={resource_name:?}): {err}"
                            );
                            None
                        }
                    },
                    Err(err) => {
                        tracing::warn!("Failed to serialize MCP resource: {err}");
                        None
                    }
                })
                .collect::<Vec<_>>();
            (name, resources)
        })
        .collect();

    let resource_templates = resource_templates
        .into_iter()
        .map(|(name, templates)| {
            let templates = templates
                .into_iter()
                .filter_map(|template| match serde_json::to_value(template) {
                    Ok(value) => match ResourceTemplate::from_mcp_value(value.clone()) {
                        Ok(template) => Some(template),
                        Err(err) => {
                            let (uri_template, template_name) = match value {
                                Value::Object(obj) => (
                                    obj.get("uriTemplate")
                                        .or_else(|| obj.get("uri_template"))
                                        .and_then(|v| v.as_str().map(ToString::to_string)),
                                    obj.get("name")
                                        .and_then(|v| v.as_str().map(ToString::to_string)),
                                ),
                                _ => (None, None),
                            };

                            tracing::warn!(
                                "Failed to convert MCP resource template (uri_template={uri_template:?}, name={template_name:?}): {err}"
                            );
                            None
                        }
                    },
                    Err(err) => {
                        tracing::warn!("Failed to serialize MCP resource template: {err}");
                        None
                    }
                })
                .collect::<Vec<_>>();
            (name, templates)
        })
        .collect();

    McpListToolsResponseEvent {
        tools,
        resources,
        resource_templates,
        auth_statuses,
    }
}


