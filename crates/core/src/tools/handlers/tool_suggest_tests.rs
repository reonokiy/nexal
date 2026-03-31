use super::*;
use crate::plugins::PluginsManager;
use crate::plugins::test_support::write_plugins_feature_config;

#[tokio::test]
async fn verified_plugin_suggestion_completed_always_returns_false() {
    let nexal_home = tempfile::tempdir().expect("tempdir should succeed");
    write_plugins_feature_config(nexal_home.path());

    let config = crate::plugins::test_support::load_plugins_config(nexal_home.path()).await;
    let plugins_manager = PluginsManager::new(nexal_home.path().to_path_buf());

    // With marketplace flow removed, plugin suggestions can never be completed.
    assert!(!verified_plugin_suggestion_completed(
        "sample@openai-curated",
        &config,
        &plugins_manager,
    ));
}
