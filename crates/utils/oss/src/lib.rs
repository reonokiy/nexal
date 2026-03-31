//! OSS provider utilities — stubbed out (ollama/lmstudio removed).

/// Returns the default model for a given OSS provider.
pub fn get_default_model_for_oss_provider(_provider_id: &str) -> Option<&'static str> {
    None
}

/// Ensures the specified OSS provider is ready.
pub async fn ensure_oss_provider_ready(
    _provider_id: &str,
    _config: &nexal_core::config::Config,
) -> Result<(), std::io::Error> {
    Ok(())
}
