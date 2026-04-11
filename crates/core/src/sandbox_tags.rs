use crate::protocol::SandboxPolicy;
use nexal_sandboxing::SandboxType;
use nexal_sandboxing::get_platform_sandbox;

pub(crate) fn sandbox_tag(policy: &SandboxPolicy) -> &'static str {
    if matches!(policy, SandboxPolicy::DangerFullAccess) {
        return "none";
    }
    if matches!(policy, SandboxPolicy::ExternalSandbox { .. }) {
        return "external";
    }

    get_platform_sandbox()
        .map(SandboxType::as_metric_tag)
        .unwrap_or("none")
}
