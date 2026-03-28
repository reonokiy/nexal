use crate::landlock::NEXAL_LINUX_SANDBOX_ARG0;
use crate::landlock::allow_network_for_proxy;
use crate::landlock::create_linux_sandbox_command_args_for_policies;
use crate::policy_transforms::EffectiveSandboxPermissions;
use crate::policy_transforms::effective_file_system_sandbox_policy;
use crate::policy_transforms::effective_network_sandbox_policy;
use crate::policy_transforms::should_require_platform_sandbox;
#[cfg(target_os = "macos")]
use crate::seatbelt::MACOS_PATH_TO_SEATBELT_EXECUTABLE;
#[cfg(target_os = "macos")]
use crate::seatbelt::create_seatbelt_command_args_for_policies_with_extensions;
use nexal_network_proxy::NetworkProxy;
use nexal_protocol::config_types::WindowsSandboxLevel;
#[cfg(target_os = "macos")]
use nexal_protocol::models::MacOsSeatbeltProfileExtensions;
use nexal_protocol::models::PermissionProfile;
use nexal_protocol::permissions::FileSystemSandboxPolicy;
use nexal_protocol::permissions::NetworkSandboxPolicy;
use nexal_protocol::protocol::SandboxPolicy;
use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SandboxType {
    None,
    MacosSeatbelt,
    LinuxSeccomp,
    WindowsRestrictedToken,
    /// Run commands inside a Podman container.
    Podman,
}

impl SandboxType {
    pub fn as_metric_tag(self) -> &'static str {
        match self {
            SandboxType::None => "none",
            SandboxType::MacosSeatbelt => "seatbelt",
            SandboxType::LinuxSeccomp => "seccomp",
            SandboxType::WindowsRestrictedToken => "windows_sandbox",
            SandboxType::Podman => "podman",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SandboxablePreference {
    Auto,
    Require,
    Forbid,
}

pub fn get_platform_sandbox(_windows_sandbox_enabled: bool) -> Option<SandboxType> {
    // Podman is the default sandbox. Set NEXAL_SANDBOX=none to disable.
    if let Ok(val) = std::env::var("NEXAL_SANDBOX") {
        return match val.to_lowercase().as_str() {
            "none" | "off" | "disabled" => None,
            _ => Some(SandboxType::Podman),
        };
    }
    // Default: Podman
    Some(SandboxType::Podman)
}

#[derive(Debug)]
pub struct SandboxCommand {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    pub env: HashMap<String, String>,
    pub additional_permissions: Option<PermissionProfile>,
}

#[derive(Debug)]
pub struct SandboxExecRequest {
    pub command: Vec<String>,
    pub cwd: PathBuf,
    pub env: HashMap<String, String>,
    pub network: Option<NetworkProxy>,
    pub sandbox: SandboxType,
    pub windows_sandbox_level: WindowsSandboxLevel,
    pub windows_sandbox_private_desktop: bool,
    pub sandbox_policy: SandboxPolicy,
    pub file_system_sandbox_policy: FileSystemSandboxPolicy,
    pub network_sandbox_policy: NetworkSandboxPolicy,
    pub arg0: Option<String>,
}

/// Bundled arguments for sandbox transformation.
///
/// This keeps call sites self-documenting when several fields are optional.
pub struct SandboxTransformRequest<'a> {
    pub command: SandboxCommand,
    pub policy: &'a SandboxPolicy,
    pub file_system_policy: &'a FileSystemSandboxPolicy,
    pub network_policy: NetworkSandboxPolicy,
    pub sandbox: SandboxType,
    pub enforce_managed_network: bool,
    // TODO(viyatb): Evaluate switching this to Option<Arc<NetworkProxy>>
    // to make shared ownership explicit across runtime/sandbox plumbing.
    pub network: Option<&'a NetworkProxy>,
    pub sandbox_policy_cwd: &'a Path,
    #[cfg(target_os = "macos")]
    pub macos_seatbelt_profile_extensions: Option<&'a MacOsSeatbeltProfileExtensions>,
    pub nexal_linux_sandbox_exe: Option<&'a PathBuf>,
    pub use_legacy_landlock: bool,
    pub windows_sandbox_level: WindowsSandboxLevel,
    pub windows_sandbox_private_desktop: bool,
}

#[derive(Debug)]
pub enum SandboxTransformError {
    MissingLinuxSandboxExecutable,
    #[cfg(not(target_os = "macos"))]
    SeatbeltUnavailable,
}

impl std::fmt::Display for SandboxTransformError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingLinuxSandboxExecutable => {
                write!(f, "missing nexal-linux-sandbox executable path")
            }
            #[cfg(not(target_os = "macos"))]
            Self::SeatbeltUnavailable => write!(f, "seatbelt sandbox is only available on macOS"),
        }
    }
}

impl std::error::Error for SandboxTransformError {}

#[derive(Default)]
pub struct SandboxManager;

impl SandboxManager {
    pub fn new() -> Self {
        Self
    }

    pub fn select_initial(
        &self,
        file_system_policy: &FileSystemSandboxPolicy,
        network_policy: NetworkSandboxPolicy,
        pref: SandboxablePreference,
        windows_sandbox_level: WindowsSandboxLevel,
        has_managed_network_requirements: bool,
    ) -> SandboxType {
        // When NEXAL_SANDBOX=podman is explicitly set, always use Podman
        // regardless of policy (the container IS the sandbox).
        if let Ok(val) = std::env::var("NEXAL_SANDBOX") {
            if val.eq_ignore_ascii_case("podman") {
                return SandboxType::Podman;
            }
        }

        match pref {
            SandboxablePreference::Forbid => SandboxType::None,
            SandboxablePreference::Require => {
                get_platform_sandbox(windows_sandbox_level != WindowsSandboxLevel::Disabled)
                    .unwrap_or(SandboxType::None)
            }
            SandboxablePreference::Auto => {
                if should_require_platform_sandbox(
                    file_system_policy,
                    network_policy,
                    has_managed_network_requirements,
                ) {
                    get_platform_sandbox(windows_sandbox_level != WindowsSandboxLevel::Disabled)
                        .unwrap_or(SandboxType::None)
                } else {
                    SandboxType::None
                }
            }
        }
    }

    pub fn transform(
        &self,
        request: SandboxTransformRequest<'_>,
    ) -> Result<SandboxExecRequest, SandboxTransformError> {
        let SandboxTransformRequest {
            mut command,
            policy,
            file_system_policy,
            network_policy,
            sandbox,
            enforce_managed_network,
            network,
            sandbox_policy_cwd,
            #[cfg(target_os = "macos")]
            macos_seatbelt_profile_extensions,
            nexal_linux_sandbox_exe,
            use_legacy_landlock,
            windows_sandbox_level,
            windows_sandbox_private_desktop,
        } = request;
        #[cfg(not(target_os = "macos"))]
        let macos_seatbelt_profile_extensions = None;
        let additional_permissions = command.additional_permissions.take();
        let EffectiveSandboxPermissions {
            sandbox_policy: effective_policy,
            #[cfg(target_os = "macos")]
                macos_seatbelt_profile_extensions: effective_macos_seatbelt_profile_extensions,
            #[cfg(not(target_os = "macos"))]
                macos_seatbelt_profile_extensions: _,
        } = EffectiveSandboxPermissions::new(
            policy,
            macos_seatbelt_profile_extensions,
            additional_permissions.as_ref(),
        );
        let effective_file_system_policy = effective_file_system_sandbox_policy(
            file_system_policy,
            additional_permissions.as_ref(),
        );
        let effective_network_policy =
            effective_network_sandbox_policy(network_policy, additional_permissions.as_ref());
        let mut argv = Vec::with_capacity(1 + command.args.len());
        argv.push(command.program);
        argv.append(&mut command.args);

        let (argv, arg0_override) = match sandbox {
            SandboxType::None => (argv, None),
            #[cfg(target_os = "macos")]
            SandboxType::MacosSeatbelt => {
                let mut args = create_seatbelt_command_args_for_policies_with_extensions(
                    argv.clone(),
                    &effective_file_system_policy,
                    effective_network_policy,
                    sandbox_policy_cwd,
                    enforce_managed_network,
                    network,
                    effective_macos_seatbelt_profile_extensions.as_ref(),
                );
                let mut full_command = Vec::with_capacity(1 + args.len());
                full_command.push(MACOS_PATH_TO_SEATBELT_EXECUTABLE.to_string());
                full_command.append(&mut args);
                (full_command, None)
            }
            #[cfg(not(target_os = "macos"))]
            SandboxType::MacosSeatbelt => return Err(SandboxTransformError::SeatbeltUnavailable),
            SandboxType::LinuxSeccomp => {
                let exe = nexal_linux_sandbox_exe
                    .ok_or(SandboxTransformError::MissingLinuxSandboxExecutable)?;
                let allow_proxy_network = allow_network_for_proxy(enforce_managed_network);
                let mut args = create_linux_sandbox_command_args_for_policies(
                    argv.clone(),
                    command.cwd.as_path(),
                    &effective_policy,
                    &effective_file_system_policy,
                    effective_network_policy,
                    sandbox_policy_cwd,
                    use_legacy_landlock,
                    allow_proxy_network,
                );
                let mut full_command = Vec::with_capacity(1 + args.len());
                full_command.push(exe.to_string_lossy().to_string());
                full_command.append(&mut args);
                (
                    full_command,
                    Some(linux_sandbox_arg0_override(exe.as_path())),
                )
            }
            #[cfg(target_os = "windows")]
            SandboxType::WindowsRestrictedToken => (argv, None),
            #[cfg(not(target_os = "windows"))]
            SandboxType::WindowsRestrictedToken => (argv, None),
            SandboxType::Podman => {
                // Use a persistent container via `podman exec`.
                // The container must be pre-created and its name stored in
                // NEXAL_SANDBOX_CONTAINER.  If the env var is missing, fall
                // back to an ephemeral `podman run --rm`.
                if let Ok(container) = std::env::var("NEXAL_SANDBOX_CONTAINER") {
                    // Map host workspace path to container-side /workspace.
                    // The container has the host workspace bind-mounted at /workspace.
                    let container_cwd = map_host_to_container_cwd(
                        &command.cwd,
                    );
                    // Persistent container: podman exec <name> <command>
                    // Remap host shell paths to container equivalents.
                    let container_argv = remap_for_container(&argv);
                    let mut podman_argv = vec![
                        "podman".to_string(),
                        "exec".to_string(),
                        "-w".to_string(),
                        container_cwd.clone(),
                        container.clone(),
                    ];
                    podman_argv.extend(container_argv);
                    tracing::debug!(
                        container = %container,
                        cwd = %container_cwd,
                        cmd = ?argv,
                        "podman exec sandbox"
                    );
                    (podman_argv, None)
                } else {
                    // Fallback: ephemeral container per command
                    let image = std::env::var("SANDBOX_IMAGE")
                        .unwrap_or_else(|_| "ghcr.io/reonokiy/nexal-sandbox:python3.13-debian13".to_string());
                    let workspace_dir = command.cwd.to_string_lossy().to_string();
                    let network = if !effective_network_policy.is_enabled() {
                        "none"
                    } else {
                        "pasta"
                    };
                    let mut podman_argv = vec![
                        "podman".to_string(),
                        "run".to_string(),
                        "--rm".to_string(),
                        "--userns=keep-id".to_string(),
                        "--security-opt".to_string(),
                        "no-new-privileges".to_string(),
                        "--cap-drop=ALL".to_string(),
                        format!("--network={network}"),
                        "-v".to_string(),
                        format!("{workspace_dir}:/workspace"),
                        "-w".to_string(),
                        "/workspace".to_string(),
                        image,
                    ];
                    podman_argv.extend(argv);
                    (podman_argv, None)
                }
            }
        };

        Ok(SandboxExecRequest {
            command: argv,
            cwd: command.cwd,
            env: command.env,
            network: network.cloned(),
            sandbox,
            windows_sandbox_level,
            windows_sandbox_private_desktop,
            sandbox_policy: effective_policy,
            file_system_sandbox_policy: effective_file_system_policy,
            network_sandbox_policy: effective_network_policy,
            arg0: arg0_override,
        })
    }
}

/// Remap host-side paths in argv for container compatibility.
///
/// 1. argv[0]: replace absolute shell paths with just the binary name
/// 2. All args: replace host shell paths embedded in -c script content
/// 3. All args: replace host workspace paths with /workspace
fn remap_for_container(argv: &[String]) -> Vec<String> {
    argv.iter()
        .enumerate()
        .map(|(i, arg)| {
            let mut result = arg.clone();

            if i == 0 {
                // argv[0]: if it's an absolute shell path, strip to just the name
                let base = Path::new(&result)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(&result)
                    .to_string();
                if matches!(base.as_str(), "bash" | "sh" | "zsh" | "fish" | "dash") {
                    result = base;
                }
            } else {
                // Embedded in script content: replace known host shell paths.
                // Order: longest first to avoid substring collisions
                // (e.g. /bin/bash is a suffix of /usr/bin/bash)
                for (from, to) in &[
                    ("/usr/sbin/bash", "/usr/bin/bash"),
                    ("/usr/sbin/sh", "/usr/bin/sh"),
                    ("/usr/sbin/zsh", "/usr/bin/zsh"),
                    // Don't replace /usr/bin/* or /bin/* — they're already correct
                    // for Debian-based containers
                ] {
                    result = result.replace(from, to);
                }
            }

            // Replace host snapshot paths — these don't exist in the container
            // Just remove the snapshot sourcing line entirely
            if let Ok(home) = std::env::var("HOME") {
                let snapshot_dir = format!("{home}/.nexal/shell_snapshots/");
                if result.contains(&snapshot_dir) {
                    // Remove the "if . 'snapshot' >/dev/null 2>&1; then :; fi\n\n" block
                    if let Some(exec_pos) = result.find("\nexec ") {
                        result = result[exec_pos + 1..].to_string();
                    } else if let Some(exec_pos) = result.find("\n\nexec ") {
                        result = result[exec_pos + 2..].to_string();
                    }
                }
            }

            result
        })
        .collect()
}

/// Map a host-side cwd to the container-side path.
///
/// The container has the workspace bind-mounted at `/workspace`.
/// If cwd is inside the workspace, remap it; otherwise default to `/workspace`.
fn map_host_to_container_cwd(host_cwd: &Path) -> String {
    // Read the host workspace root from env (set by nexal config).
    if let Ok(workspace) = std::env::var("NEXAL_WORKSPACE") {
        let workspace_path = Path::new(&workspace);
        if let Ok(suffix) = host_cwd.strip_prefix(workspace_path) {
            let container_path = Path::new("/workspace").join(suffix);
            return container_path.to_string_lossy().to_string();
        }
    }
    // Also try the default ~/.nexal/workspace
    if let Ok(home) = std::env::var("HOME") {
        let default_workspace = PathBuf::from(&home).join(".nexal").join("workspace");
        if let Ok(suffix) = host_cwd.strip_prefix(&default_workspace) {
            let container_path = Path::new("/workspace").join(suffix);
            return container_path.to_string_lossy().to_string();
        }
    }
    // Fallback: use /workspace
    "/workspace".to_string()
}

fn linux_sandbox_arg0_override(exe: &Path) -> String {
    if exe.file_name().and_then(|name| name.to_str()) == Some(NEXAL_LINUX_SANDBOX_ARG0) {
        exe.to_string_lossy().into_owned()
    } else {
        NEXAL_LINUX_SANDBOX_ARG0.to_string()
    }
}

#[cfg(test)]
#[path = "manager_tests.rs"]
mod tests;
