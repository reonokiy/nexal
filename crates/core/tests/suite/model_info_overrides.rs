use nexal_core::models_manager::collaboration_mode_presets::CollaborationModesConfig;
use nexal_core::models_manager::manager::ModelsManager;
use nexal_protocol::openai_models::TruncationPolicyConfig;
use core_test_support::load_default_config_for_test;
use pretty_assertions::assert_eq;
use tempfile::TempDir;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn offline_model_info_without_tool_output_override() {
    let nexal_home = TempDir::new().expect("create temp dir");
    let config = load_default_config_for_test(&nexal_home).await;
    let manager = ModelsManager::new(
        config.nexal_home.clone(),
        None,
        CollaborationModesConfig::default(),
    );

    let model_info = manager.get_model_info("gpt-5.1", &config).await;

    assert_eq!(
        model_info.truncation_policy,
        TruncationPolicyConfig::bytes(10_000)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn offline_model_info_with_tool_output_override() {
    let nexal_home = TempDir::new().expect("create temp dir");
    let mut config = load_default_config_for_test(&nexal_home).await;
    config.tool_output_token_limit = Some(123);
    let manager = ModelsManager::new(
        config.nexal_home.clone(),
        None,
        CollaborationModesConfig::default(),
    );

    let model_info = manager.get_model_info("gpt-5.1-nexal", &config).await;

    assert_eq!(
        model_info.truncation_policy,
        TruncationPolicyConfig::tokens(123)
    );
}
