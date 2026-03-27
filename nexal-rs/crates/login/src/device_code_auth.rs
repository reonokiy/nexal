use crate::server::ServerOptions;

/// Device code returned by the (now stubbed) device-code login flow.
#[derive(Debug, Clone)]
pub struct DeviceCode {
    pub verification_url: String,
    pub user_code: String,
    #[allow(dead_code)]
    device_auth_id: String,
    #[allow(dead_code)]
    interval: u64,
}

/// Stub: ChatGPT device-code login is not supported. Use an API key instead.
pub async fn request_device_code(_opts: &ServerOptions) -> std::io::Result<DeviceCode> {
    Err(std::io::Error::other(
        "ChatGPT device-code login is not supported. Use an API key instead.",
    ))
}

/// Stub: ChatGPT device-code login is not supported. Use an API key instead.
pub async fn complete_device_code_login(
    _opts: ServerOptions,
    _device_code: DeviceCode,
) -> std::io::Result<()> {
    Err(std::io::Error::other(
        "ChatGPT device-code login is not supported. Use an API key instead.",
    ))
}

/// Stub: ChatGPT device-code login is not supported. Use an API key instead.
pub async fn run_device_code_login(_opts: ServerOptions) -> std::io::Result<()> {
    Err(std::io::Error::other(
        "ChatGPT device-code login is not supported. Use an API key instead.",
    ))
}
