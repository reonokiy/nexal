/*
Module: runtimes

Concrete ToolRuntime implementations for specific tools. Each runtime stays
small and focused and reuses the orchestrator for approvals + sandbox + retry.
*/
use crate::tools::sandboxing::ToolError;
use nexal_protocol::models::PermissionProfile;
use nexal_sandboxing::SandboxCommand;
use std::collections::HashMap;
use std::path::Path;

pub mod apply_patch;
pub mod shell;
pub mod unified_exec;

/// Shared helper to construct sandbox transform inputs from a tokenized command line.
/// Validates that at least a program is present.
pub(crate) fn build_sandbox_command(
    command: &[String],
    cwd: &Path,
    env: &HashMap<String, String>,
    additional_permissions: Option<PermissionProfile>,
) -> Result<SandboxCommand, ToolError> {
    let (program, args) = command
        .split_first()
        .ok_or_else(|| ToolError::Rejected("command args are empty".to_string()))?;
    Ok(SandboxCommand {
        program: program.clone(),
        args: args.to_vec(),
        cwd: cwd.to_path_buf(),
        env: env.clone(),
        additional_permissions,
    })
}
