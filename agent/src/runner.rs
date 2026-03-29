//! Low-level agent helpers shared by [`crate::pool`].

use std::sync::Arc;

use anyhow::Context;
use nexal_app_server_client::AppServerClient;
use nexal_app_server_client::DEFAULT_IN_PROCESS_CHANNEL_CAPACITY;
use nexal_app_server_client::InProcessAppServerClient;
use nexal_app_server_client::InProcessClientStartArgs;
use nexal_app_server_protocol::AskForApproval as ApiAskForApproval;
use nexal_app_server_protocol::ClientRequest;
use nexal_app_server_protocol::JSONRPCErrorError;
use nexal_app_server_protocol::RequestId;
use nexal_app_server_protocol::SandboxMode as ApiSandboxMode;
use nexal_app_server_protocol::ServerRequest;
use nexal_app_server_protocol::ThreadStartParams;
use nexal_app_server_protocol::ThreadStartResponse;
use nexal_arg0::Arg0DispatchPaths;
use nexal_config_loader::CloudRequirementsLoader;
use nexal_config_loader::LoaderOverrides;
use nexal_core::config::Config;
use nexal_core::config::ConfigBuilder;
use nexal_core::config::ConfigOverrides;
use nexal_core::config::find_nexal_home;
use nexal_feedback::NexalFeedback;
use nexal_protocol::config_types::SandboxMode as CoreSandboxMode;
use nexal_protocol::protocol::AskForApproval as CoreAskForApproval;
use nexal_protocol::protocol::SessionSource;
use nexal_config::NexalConfig;
use tracing::warn;

/// Build an [`InProcessAppServerClient`] from a nexal config.
pub(crate) async fn build_client(
    nexal_config_loader: Arc<Config>,
    cli_overrides: Vec<(String, toml::Value)>,
) -> anyhow::Result<InProcessAppServerClient> {
    let start_args = InProcessClientStartArgs {
        arg0_paths: Arg0DispatchPaths::default(),
        config: Arc::clone(&nexal_config_loader),
        cli_overrides,
        loader_overrides: LoaderOverrides::default(),
        cloud_requirements: CloudRequirementsLoader::default(),
        feedback: NexalFeedback::new(),
        config_warnings: vec![],
        session_source: SessionSource::Custom("nexal".to_string()),
        enable_nexal_api_key_env: true,
        client_name: "nexal".to_string(),
        client_version: env!("CARGO_PKG_VERSION").to_string(),
        experimental_api: true,
        opt_out_notification_methods: vec![],
        channel_capacity: DEFAULT_IN_PROCESS_CHANNEL_CAPACITY,
    };

    InProcessAppServerClient::start(start_args)
        .await
        .context("starting in-process app-server client")
}

/// Send `ThreadStart` and return the new thread ID.
pub(crate) async fn start_thread(
    client: &mut InProcessAppServerClient,
    config: &Config,
) -> anyhow::Result<String> {
    let resp: ThreadStartResponse = client
        .request_typed(ClientRequest::ThreadStart {
            request_id: RequestId::Integer(0),
            params: ThreadStartParams {
                model: config.model.clone(),
                model_provider: Some(config.model_provider_id.clone()),
                cwd: Some(config.cwd.to_string_lossy().to_string()),
                approval_policy: Some(ApiAskForApproval::Never),
                sandbox: Some(ApiSandboxMode::WorkspaceWrite),
                ephemeral: Some(false),
                ..Default::default()
            },
        })
        .await
        .map_err(|e| anyhow::anyhow!("thread/start: {e}"))?;
    Ok(resp.thread.id)
}

/// Build a codex `Config` from nexal config + environment.
pub(crate) async fn build_nexal_config_loader(nc: &NexalConfig, soul: String) -> anyhow::Result<Config> {
    let nexal_home = match nc.nexal_home.clone() {
        Some(h) => h,
        None => find_nexal_home().context("finding codex home")?,
    };

    let cwd = nc.workspace.clone();
    tokio::fs::create_dir_all(&cwd)
        .await
        .context("creating workspace dir")?;

    let overrides = ConfigOverrides {
        approval_policy: Some(CoreAskForApproval::Never),
        sandbox_mode: Some(CoreSandboxMode::WorkspaceWrite),
        cwd: Some(cwd),
        base_instructions: Some(soul),
        ..Default::default()
    };

    let cli_overrides = providers_to_cli_overrides_full(nc);

    ConfigBuilder::default()
        .nexal_home(nexal_home)
        .harness_overrides(overrides)
        .cli_overrides(cli_overrides)
        .build()
        .await
        .context("building codex config")
}

/// Build CLI overrides from NexalConfig providers, including auto-selection.
///
/// Produces entries like:
///   ("model_providers.moonshot.base_url", "https://api.moonshot.cn/v1")
///   ("model_providers.moonshot.wire_api", "chat")
///   ("model_provider", "moonshot")
pub(crate) fn providers_to_cli_overrides_full(nc: &NexalConfig) -> Vec<(String, toml::Value)> {
    let mut overrides = Vec::new();

    for (name, provider) in &nc.providers {
        if let Some(ref url) = provider.base_url {
            overrides.push((
                format!("model_providers.{name}.base_url"),
                toml::Value::String(url.clone()),
            ));
        }
        if let Some(ref key) = provider.env_key {
            overrides.push((
                format!("model_providers.{name}.env_key"),
                toml::Value::String(key.clone()),
            ));
        }
        if let Some(ref api) = provider.wire_api {
            overrides.push((
                format!("model_providers.{name}.wire_api"),
                toml::Value::String(api.clone()),
            ));
        }
        if provider.thinking_mode {
            overrides.push((
                format!("model_providers.{name}.thinking_mode"),
                toml::Value::Boolean(true),
            ));
        }
        // name is required by core's ModelProviderInfo — default to the key
        overrides.push((
            format!("model_providers.{name}.name"),
            toml::Value::String(provider.name.clone().unwrap_or_else(|| name.clone())),
        ));
    }

    // Auto-select the first provider if any are configured.
    if !nc.providers.is_empty() {
        let provider_id = nc.providers.keys().next().unwrap().clone();
        overrides.push((
            "model_provider".to_string(),
            toml::Value::String(provider_id),
        ));
    }

    overrides
}

/// Reject every incoming server request with a generic error.
pub(crate) async fn reject_all_server_requests(
    client: &AppServerClient,
    req: ServerRequest,
) {
    let request_kind = match &req {
        ServerRequest::CommandExecutionRequestApproval { .. } => "command_execution_approval",
        ServerRequest::FileChangeRequestApproval { .. } => "file_change_approval",
        ServerRequest::ToolRequestUserInput { .. } => "tool_request_user_input",
        ServerRequest::McpServerElicitationRequest { .. } => "mcp_server_elicitation",
        ServerRequest::PermissionsRequestApproval { .. } => "permissions_request_approval",
        ServerRequest::DynamicToolCall { .. } => "dynamic_tool_call",
        ServerRequest::ChatgptAuthTokensRefresh { .. } => "chatgpt_auth_tokens_refresh",
        ServerRequest::ApplyPatchApproval { .. } => "apply_patch_approval",
        ServerRequest::ExecCommandApproval { .. } => "exec_command_approval",
    };
    let error = JSONRPCErrorError {
        code: -32000,
        message: "approval not supported in nexal headless mode".to_string(),
        data: None,
    };

    let request_id = match req {
        ServerRequest::CommandExecutionRequestApproval { request_id, .. } => request_id,
        ServerRequest::FileChangeRequestApproval { request_id, .. } => request_id,
        ServerRequest::ToolRequestUserInput { request_id, .. } => request_id,
        ServerRequest::McpServerElicitationRequest { request_id, .. } => request_id,
        ServerRequest::PermissionsRequestApproval { request_id, .. } => request_id,
        ServerRequest::DynamicToolCall { request_id, .. } => request_id,
        ServerRequest::ChatgptAuthTokensRefresh { request_id, .. } => request_id,
        ServerRequest::ApplyPatchApproval { request_id, .. } => request_id,
        ServerRequest::ExecCommandApproval { request_id, .. } => request_id,
    };

    if let Err(e) = client.reject_server_request(request_id, error).await {
        warn!(request_kind, "failed to reject server request: {e}");
    } else {
        warn!(request_kind, "rejected server request in headless mode");
    }
}

