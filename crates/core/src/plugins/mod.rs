use crate::config::types::McpServerConfig;

mod injection;
mod manager;
mod manifest;
mod mentions;
mod render;
#[cfg(test)]
pub(crate) mod test_support;

pub use nexal_plugin::AppConnectorId;
pub use nexal_plugin::EffectiveSkillRoots;
pub use nexal_plugin::PluginCapabilitySummary;
pub use nexal_plugin::PluginId;
pub use nexal_plugin::PluginIdError;
pub use nexal_plugin::PluginTelemetryMetadata;

pub type LoadedPlugin = nexal_plugin::LoadedPlugin<McpServerConfig>;
pub type PluginLoadOutcome = nexal_plugin::PluginLoadOutcome<McpServerConfig>;

pub(crate) use injection::build_plugin_injections;
pub use manager::PluginsManager;
pub use manager::load_plugin_apps;
pub use manager::load_plugin_mcp_servers;
pub use manager::plugin_telemetry_metadata_from_root;
pub use manifest::PluginManifestInterface;
pub(crate) use manifest::PluginManifestPaths;
pub(crate) use manifest::load_plugin_manifest;
pub(crate) use render::render_explicit_plugin_instructions;
pub(crate) use render::render_plugins_section;
pub(crate) use mentions::build_connector_slug_counts;
pub(crate) use mentions::build_skill_name_counts;
pub(crate) use mentions::collect_explicit_app_ids;
pub(crate) use mentions::collect_explicit_plugin_mentions;
pub(crate) use mentions::collect_tool_mentions_from_messages;
