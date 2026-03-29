use std::env;
use std::fmt::Debug;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::RwLock;

use nexal_app_server_protocol::AuthMode as ApiAuthMode;
use nexal_protocol::config_types::ForcedLoginMethod;

pub use crate::auth::storage::AuthCredentialsStoreMode;
pub use crate::auth::storage::AuthDotJson;
use crate::auth::storage::create_auth_storage;

#[derive(Debug, Clone, PartialEq)]
pub enum NexalAuth {
    ApiKey(ApiKeyAuth),
}

#[derive(Debug, Clone)]
pub struct ApiKeyAuth {
    api_key: String,
}

impl PartialEq for ApiKeyAuth {
    fn eq(&self, other: &Self) -> bool {
        self.api_key == other.api_key
    }
}

impl NexalAuth {
    fn from_auth_dot_json(
        _nexal_home: &Path,
        auth_dot_json: AuthDotJson,
        _auth_credentials_store_mode: AuthCredentialsStoreMode,
    ) -> std::io::Result<Self> {
        let api_key = auth_dot_json
            .openai_api_key
            .ok_or_else(|| std::io::Error::other("API key auth is missing a key."))?;
        Ok(Self::from_api_key(&api_key))
    }

    pub fn from_auth_storage(
        nexal_home: &Path,
        auth_credentials_store_mode: AuthCredentialsStoreMode,
    ) -> std::io::Result<Option<Self>> {
        load_auth(nexal_home, false, auth_credentials_store_mode)
    }

    pub fn auth_mode(&self) -> crate::AuthMode { crate::AuthMode::ApiKey }
    pub fn api_auth_mode(&self) -> ApiAuthMode { ApiAuthMode::ApiKey }

    pub fn api_key(&self) -> Option<&str> {
        match self { Self::ApiKey(auth) => Some(auth.api_key.as_str()) }
    }

    pub fn get_token_data(&self) -> Result<crate::token_data::TokenData, std::io::Error> {
        Err(std::io::Error::other("Token data is not available (API key auth only)."))
    }

    pub fn get_token(&self) -> Result<String, std::io::Error> {
        match self { Self::ApiKey(auth) => Ok(auth.api_key.clone()) }
    }


    pub fn create_dummy_chatgpt_auth_for_testing() -> Self {
        Self::from_api_key("dummy-chatgpt-test-key")
    }

    pub fn from_api_key(api_key: &str) -> Self {
        Self::ApiKey(ApiKeyAuth { api_key: api_key.to_owned() })
    }
}

pub const OPENAI_API_KEY_ENV_VAR: &str = "OPENAI_API_KEY";
pub const NEXAL_API_KEY_ENV_VAR: &str = "NEXAL_API_KEY";

pub fn read_openai_api_key_from_env() -> Option<String> {
    env::var(OPENAI_API_KEY_ENV_VAR).ok().map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

pub fn read_nexal_api_key_from_env() -> Option<String> {
    env::var(NEXAL_API_KEY_ENV_VAR).ok().map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

pub fn logout(nexal_home: &Path, mode: AuthCredentialsStoreMode) -> std::io::Result<bool> {
    create_auth_storage(nexal_home.to_path_buf(), mode).delete()
}

pub fn login_with_api_key(nexal_home: &Path, api_key: &str, mode: AuthCredentialsStoreMode) -> std::io::Result<()> {
    let auth = AuthDotJson {
        auth_mode: Some(ApiAuthMode::ApiKey),
        openai_api_key: Some(api_key.to_string()),
        tokens: None,
        last_refresh: None,
    };
    save_auth(nexal_home, &auth, mode)
}

pub fn save_auth(nexal_home: &Path, auth: &AuthDotJson, mode: AuthCredentialsStoreMode) -> std::io::Result<()> {
    create_auth_storage(nexal_home.to_path_buf(), mode).save(auth)
}

pub fn load_auth_dot_json(nexal_home: &Path, mode: AuthCredentialsStoreMode) -> std::io::Result<Option<AuthDotJson>> {
    create_auth_storage(nexal_home.to_path_buf(), mode).load()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthConfig {
    pub nexal_home: PathBuf,
    pub auth_credentials_store_mode: AuthCredentialsStoreMode,
    pub forced_login_method: Option<ForcedLoginMethod>,
}

pub fn enforce_login_restrictions(config: &AuthConfig) -> std::io::Result<()> {
    let Some(_auth) = load_auth(&config.nexal_home, true, config.auth_credentials_store_mode)? else {
        return Ok(());
    };
    if let Some(required_method) = config.forced_login_method {
        if matches!(required_method, ForcedLoginMethod::Chatgpt) {
            return logout_with_message(&config.nexal_home,
                "ChatGPT login is required, but an API key is currently being used. Logging out.".to_string(),
                config.auth_credentials_store_mode);
        }
    }
    Ok(())
}

fn logout_with_message(nexal_home: &Path, message: String, mode: AuthCredentialsStoreMode) -> std::io::Result<()> {
    match logout(nexal_home, mode) {
        Ok(_) => Err(std::io::Error::other(message)),
        Err(err) => Err(std::io::Error::other(format!("{message}. Failed to remove auth.json: {err}"))),
    }
}

fn load_auth(nexal_home: &Path, enable_env: bool, mode: AuthCredentialsStoreMode) -> std::io::Result<Option<NexalAuth>> {
    if enable_env { if let Some(k) = read_nexal_api_key_from_env() { return Ok(Some(NexalAuth::from_api_key(&k))); } }
    let storage = create_auth_storage(nexal_home.to_path_buf(), mode);
    match storage.load()? {
        Some(adj) => Ok(Some(NexalAuth::from_auth_dot_json(nexal_home, adj, mode)?)),
        None => Ok(None),
    }
}

pub const CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
pub const REFRESH_TOKEN_URL_OVERRIDE_ENV_VAR: &str = "NEXAL_REFRESH_TOKEN_URL_OVERRIDE";

#[derive(Debug)]
pub struct AuthManager {
    nexal_home: PathBuf,
    inner: RwLock<Option<NexalAuth>>,
    enable_nexal_api_key_env: bool,
    auth_credentials_store_mode: AuthCredentialsStoreMode,
}

impl AuthManager {
    pub fn new(nexal_home: PathBuf, enable_nexal_api_key_env: bool, mode: AuthCredentialsStoreMode) -> Self {
        let auth = load_auth(&nexal_home, enable_nexal_api_key_env, mode).ok().flatten();
        Self { nexal_home, inner: RwLock::new(auth), enable_nexal_api_key_env, auth_credentials_store_mode: mode }
    }

    pub fn from_auth_for_testing(auth: NexalAuth) -> Arc<Self> {
        Arc::new(Self { nexal_home: PathBuf::from("non-existent"), inner: RwLock::new(Some(auth)), enable_nexal_api_key_env: false, auth_credentials_store_mode: AuthCredentialsStoreMode::File })
    }

    pub fn from_auth_for_testing_with_home(auth: NexalAuth, nexal_home: PathBuf) -> Arc<Self> {
        Arc::new(Self { nexal_home, inner: RwLock::new(Some(auth)), enable_nexal_api_key_env: false, auth_credentials_store_mode: AuthCredentialsStoreMode::File })
    }

    pub fn auth_cached(&self) -> Option<NexalAuth> { self.inner.read().ok().and_then(|c| c.clone()) }
    pub async fn auth(&self) -> Option<NexalAuth> { self.auth_cached() }

    pub fn reload(&self) -> bool {
        tracing::info!("Reloading auth");
        let new = load_auth(&self.nexal_home, self.enable_nexal_api_key_env, self.auth_credentials_store_mode).ok().flatten();
        if let Ok(mut g) = self.inner.write() { let c = *g != new; tracing::info!("Reloaded auth, changed: {c}"); *g = new; c } else { false }
    }

    pub fn logout(&self) -> std::io::Result<bool> {
        let r = logout(&self.nexal_home, self.auth_credentials_store_mode)?; self.reload(); Ok(r)
    }

    pub fn get_api_auth_mode(&self) -> Option<ApiAuthMode> { self.auth_cached().as_ref().map(NexalAuth::api_auth_mode) }
    pub fn auth_mode(&self) -> Option<crate::AuthMode> { self.auth_cached().as_ref().map(NexalAuth::auth_mode) }
    pub fn nexal_api_key_env_enabled(&self) -> bool { self.enable_nexal_api_key_env }

    pub fn shared(nexal_home: PathBuf, enable_nexal_api_key_env: bool, mode: AuthCredentialsStoreMode) -> Arc<Self> {
        Arc::new(Self::new(nexal_home, enable_nexal_api_key_env, mode))
    }

    pub fn unauthorized_recovery(self: &Arc<Self>) -> super::UnauthorizedRecovery { super::UnauthorizedRecovery { _private: () } }
}
