//! Cloud-hosted config requirements for Nexal (no-op stub).

use nexal_core::AuthManager;
use nexal_core::auth::AuthCredentialsStoreMode;
use nexal_core::config_loader::CloudRequirementsLoader;
use std::path::PathBuf;
use std::sync::Arc;

pub fn cloud_requirements_loader(
    _auth_manager: Arc<AuthManager>,
    _chatgpt_base_url: String,
    _nexal_home: PathBuf,
) -> CloudRequirementsLoader {
    CloudRequirementsLoader::default()
}

pub fn cloud_requirements_loader_for_storage(
    _nexal_home: PathBuf,
    _enable_nexal_api_key_env: bool,
    _credentials_store_mode: AuthCredentialsStoreMode,
    _chatgpt_base_url: String,
) -> CloudRequirementsLoader {
    CloudRequirementsLoader::default()
}
