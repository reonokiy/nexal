pub mod auth;
pub mod token_data;

pub use nexal_client::BuildCustomCaTransportError as BuildLoginHttpClientError;

pub use auth::AuthConfig;
pub use auth::AuthCredentialsStoreMode;
pub use auth::AuthDotJson;
pub use auth::AuthManager;
pub use auth::CLIENT_ID;
pub use auth::NEXAL_API_KEY_ENV_VAR;
pub use auth::NexalAuth;
pub use auth::OPENAI_API_KEY_ENV_VAR;
pub use auth::REFRESH_TOKEN_URL_OVERRIDE_ENV_VAR;
pub use auth::RefreshTokenError;
pub use auth::RefreshTokenFailedError;
pub use auth::RefreshTokenFailedReason;
pub use auth::UnauthorizedRecovery;
pub use auth::UnauthorizedRecoveryStepResult;
pub use auth::default_client;
pub use auth::enforce_login_restrictions;
pub use auth::login_with_api_key;
pub use auth::logout;
pub use auth::read_openai_api_key_from_env;
pub use auth::save_auth;
pub use nexal_app_server_protocol::AuthMode;
pub use token_data::TokenData;
