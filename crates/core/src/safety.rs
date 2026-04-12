use std::path::Component;
use std::path::Path;
use std::path::PathBuf;

use crate::protocol::AskForApproval;
use crate::protocol::FileSystemSandboxPolicy;
use crate::protocol::SandboxPolicy;
use crate::util::resolve_path;
use nexal_apply_patch::ApplyPatchAction;
use nexal_apply_patch::ApplyPatchFileChange;
use nexal_sandboxing::SandboxType;
use nexal_sandboxing::get_platform_sandbox;

#[derive(Debug, PartialEq)]
pub enum SafetyCheck {
    AutoApprove {
        sandbox_type: SandboxType,
        user_explicitly_approved: bool,
    },
    AskUser,
    Reject {
        reason: String,
    },
}

pub fn assess_patch_safety(
    action: &ApplyPatchAction,
    policy: AskForApproval,
    sandbox_policy: &SandboxPolicy,
    file_system_sandbox_policy: &FileSystemSandboxPolicy,
    cwd: &Path,
) -> SafetyCheck {
    if action.is_empty() {
        return SafetyCheck::Reject {
            reason: "empty patch".to_string(),
        };
    }

    match policy {
        AskForApproval::OnFailure
        | AskForApproval::Never
        | AskForApproval::OnRequest
        | AskForApproval::Granular(_) => {
            // Continue to see if this can be auto-approved.
        }
        // TODO(ragona): I'm not sure this is actually correct? I believe in this case
        // we want to continue to the writable paths check before asking the user.
        AskForApproval::UnlessTrusted => {
            return SafetyCheck::AskUser;
        }
    }

    let rejects_sandbox_approval = matches!(policy, AskForApproval::Never)
        || matches!(
            policy,
            AskForApproval::Granular(granular_config) if !granular_config.sandbox_approval
        );

    // Even though the patch appears to be constrained to writable paths, it is
    // possible that paths in the patch are hard links to files outside the
    // writable roots, so we should still run `apply_patch` in a sandbox in that case.
    if is_write_patch_constrained_to_writable_paths(action, file_system_sandbox_policy, cwd)
        || matches!(policy, AskForApproval::OnFailure)
    {
        if matches!(
            sandbox_policy,
            SandboxPolicy::DangerFullAccess | SandboxPolicy::ExternalSandbox { .. }
        ) {
            // DangerFullAccess is intended to bypass sandboxing entirely.
            SafetyCheck::AutoApprove {
                sandbox_type: SandboxType::None,
                user_explicitly_approved: false,
            }
        } else {
            // Only auto‑approve when we can actually enforce a sandbox. Otherwise
            // fall back to asking the user because the patch may touch arbitrary
            // paths outside the project.
            SafetyCheck::AutoApprove {
                sandbox_type: get_platform_sandbox(),
                user_explicitly_approved: false,
            }
        }
    } else if rejects_sandbox_approval {
        SafetyCheck::Reject {
            reason: "writing outside of the project; rejected by user approval settings"
                .to_string(),
        }
    } else {
        SafetyCheck::AskUser
    }
}

fn is_write_patch_constrained_to_writable_paths(
    action: &ApplyPatchAction,
    file_system_sandbox_policy: &FileSystemSandboxPolicy,
    cwd: &Path,
) -> bool {
    // Normalize a path by removing `.` and resolving `..` without touching the
    // filesystem (works even if the file does not exist).
    fn normalize(path: &Path) -> Option<PathBuf> {
        let mut out = PathBuf::new();
        for comp in path.components() {
            match comp {
                Component::ParentDir => {
                    out.pop();
                }
                Component::CurDir => { /* skip */ }
                other => out.push(other.as_os_str()),
            }
        }
        Some(out)
    }

    // Determine whether `path` is inside **any** writable root. Both `path`
    // and roots are converted to absolute, normalized forms before the
    // prefix check.
    let is_path_writable = |p: &PathBuf| {
        let abs = resolve_path(cwd, p);
        let abs = match normalize(&abs) {
            Some(v) => v,
            None => return false,
        };

        file_system_sandbox_policy.can_write_path_with_cwd(&abs, cwd)
    };

    for (path, change) in action.changes() {
        match change {
            ApplyPatchFileChange::Add { .. } | ApplyPatchFileChange::Delete { .. } => {
                if !is_path_writable(path) {
                    return false;
                }
            }
            ApplyPatchFileChange::Update { move_path, .. } => {
                if !is_path_writable(path) {
                    return false;
                }
                if let Some(dest) = move_path
                    && !is_path_writable(dest)
                {
                    return false;
                }
            }
        }
    }

    true
}


