use nexal_core::config::Config;
use nexal_core::connectors::filter_disallowed_connectors;
use nexal_core::connectors::merge_connectors;
use nexal_core::connectors::merge_plugin_apps;
use nexal_core::plugins::AppConnectorId;
use std::collections::HashSet;

// Re-exports from nexal_core::connectors — these are the real implementations.
pub use nexal_core::connectors::AppInfo;
pub use nexal_core::connectors::connector_display_label;
pub use nexal_core::connectors::list_accessible_connectors_from_mcp_tools;
pub use nexal_core::connectors::list_accessible_connectors_from_mcp_tools_with_options;
pub use nexal_core::connectors::list_accessible_connectors_from_mcp_tools_with_options_and_status;
pub use nexal_core::connectors::list_cached_accessible_connectors_from_mcp_tools;
pub use nexal_core::connectors::with_app_enabled_state;

/// Stub: always returns an empty list (ChatGPT backend not available).
pub async fn list_connectors(_config: &Config) -> anyhow::Result<Vec<AppInfo>> {
    Ok(Vec::new())
}

/// Stub: always returns an empty list.
pub async fn list_all_connectors(_config: &Config) -> anyhow::Result<Vec<AppInfo>> {
    Ok(Vec::new())
}

/// Stub: always returns an empty list.
pub async fn list_cached_all_connectors(_config: &Config) -> Option<Vec<AppInfo>> {
    Some(Vec::new())
}

/// Stub: always returns an empty list.
pub async fn list_all_connectors_with_options(
    _config: &Config,
    _force_refetch: bool,
) -> anyhow::Result<Vec<AppInfo>> {
    Ok(Vec::new())
}

pub fn connectors_for_plugin_apps(
    connectors: Vec<AppInfo>,
    plugin_apps: &[AppConnectorId],
) -> Vec<AppInfo> {
    let plugin_app_ids = plugin_apps
        .iter()
        .map(|connector_id| connector_id.0.as_str())
        .collect::<HashSet<_>>();

    filter_disallowed_connectors(merge_plugin_apps(connectors, plugin_apps.to_vec()))
        .into_iter()
        .filter(|connector| plugin_app_ids.contains(connector.id.as_str()))
        .collect()
}

pub fn merge_connectors_with_accessible(
    connectors: Vec<AppInfo>,
    accessible_connectors: Vec<AppInfo>,
    all_connectors_loaded: bool,
) -> Vec<AppInfo> {
    let accessible_connectors = if all_connectors_loaded {
        let connector_ids: HashSet<&str> = connectors
            .iter()
            .map(|connector| connector.id.as_str())
            .collect();
        accessible_connectors
            .into_iter()
            .filter(|connector| connector_ids.contains(connector.id.as_str()))
            .collect()
    } else {
        accessible_connectors
    };
    let merged = merge_connectors(connectors, accessible_connectors);
    filter_disallowed_connectors(merged)
}
