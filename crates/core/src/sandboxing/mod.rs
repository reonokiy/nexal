/*
Module: sandboxing

Core-owned adapter types for exec/runtime plumbing. Policy selection and
command transformation live in the nexal-sandboxing crate; this module keeps
the exec-only metadata and translates transformed sandbox commands back into
ExecRequest for execution.
*/

use crate::exec::ExecCapturePolicy;
use crate::exec::ExecExpiration;
use crate::exec::ExecToolCallOutput;
use crate::exec::StdoutStream;
use crate::exec::execute_exec_request;
use crate::spawn::NEXAL_SANDBOX_NETWORK_DISABLED_ENV_VAR;
use nexal_network_proxy::NetworkProxy;
pub use nexal_protocol::models::SandboxPermissions;
use nexal_protocol::permissions::FileSystemSandboxPolicy;
use nexal_protocol::permissions::NetworkSandboxPolicy;
use nexal_protocol::protocol::SandboxPolicy;
use nexal_sandboxing::SandboxExecRequest;
use nexal_sandboxing::SandboxType;
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug)]
pub(crate) struct ExecOptions {
    pub(crate) expiration: ExecExpiration,
    pub(crate) capture_policy: ExecCapturePolicy,
}

#[derive(Debug)]
pub struct ExecRequest {
    pub command: Vec<String>,
    pub cwd: PathBuf,
    pub env: HashMap<String, String>,
    pub network: Option<NetworkProxy>,
    pub expiration: ExecExpiration,
    pub capture_policy: ExecCapturePolicy,
    pub sandbox: SandboxType,
    pub sandbox_policy: SandboxPolicy,
    pub file_system_sandbox_policy: FileSystemSandboxPolicy,
    pub network_sandbox_policy: NetworkSandboxPolicy,
    pub arg0: Option<String>,
}

impl ExecRequest {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        command: Vec<String>,
        cwd: PathBuf,
        env: HashMap<String, String>,
        network: Option<NetworkProxy>,
        expiration: ExecExpiration,
        capture_policy: ExecCapturePolicy,
        sandbox: SandboxType,
        sandbox_policy: SandboxPolicy,
        file_system_sandbox_policy: FileSystemSandboxPolicy,
        network_sandbox_policy: NetworkSandboxPolicy,
        arg0: Option<String>,
    ) -> Self {
        Self {
            command,
            cwd,
            env,
            network,
            expiration,
            capture_policy,
            sandbox,
            sandbox_policy,
            file_system_sandbox_policy,
            network_sandbox_policy,
            arg0,
        }
    }

    pub(crate) fn from_sandbox_exec_request(
        request: SandboxExecRequest,
        options: ExecOptions,
    ) -> Self {
        let SandboxExecRequest {
            command,
            cwd,
            mut env,
            network,
            sandbox,
            sandbox_policy,
            file_system_sandbox_policy,
            network_sandbox_policy,
            arg0,
        } = request;
        let ExecOptions {
            expiration,
            capture_policy,
        } = options;
        if !network_sandbox_policy.is_enabled() {
            env.insert(
                NEXAL_SANDBOX_NETWORK_DISABLED_ENV_VAR.to_string(),
                "1".to_string(),
            );
        }
        Self {
            command,
            cwd,
            env,
            network,
            expiration,
            capture_policy,
            sandbox,
            sandbox_policy,
            file_system_sandbox_policy,
            network_sandbox_policy,
            arg0,
        }
    }
}

pub async fn execute_env(
    exec_request: ExecRequest,
    stdout_stream: Option<StdoutStream>,
) -> crate::error::Result<ExecToolCallOutput> {
    let effective_policy = exec_request.sandbox_policy.clone();
    execute_exec_request(
        exec_request,
        &effective_policy,
        stdout_stream,
        /*after_spawn*/ None,
    )
    .await
}

pub async fn execute_exec_request_with_after_spawn(
    exec_request: ExecRequest,
    stdout_stream: Option<StdoutStream>,
    after_spawn: Option<Box<dyn FnOnce() + Send>>,
) -> crate::error::Result<ExecToolCallOutput> {
    let effective_policy = exec_request.sandbox_policy.clone();
    execute_exec_request(exec_request, &effective_policy, stdout_stream, after_spawn).await
}
