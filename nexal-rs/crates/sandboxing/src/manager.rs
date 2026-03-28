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

pub fn get_platform_sandbox(windows_sandbox_enabled: bool) -> Option<SandboxType> {
    // Allow overriding via NEXAL_SANDBOX env var.
    if let Ok(val) = std::env::var("NEXAL_SANDBOX") {
        return match val.to_lowercase().as_str() {
            "podman" => Some(SandboxType::Podman),
            "none" | "off" | "disabled" => None,
            // Fall through to platform default for other values
            _ => get_platform_default_sandbox(windows_sandbox_enabled),
        };
    }
    get_platform_default_sandbox(windows_sandbox_enabled)
}

fn get_platform_default_sandbox(windows_sandbox_enabled: bool) -> Option<SandboxType> {
    if cfg!(target_os = "macos") {
        Some(SandboxType::MacosSeatbelt)
    } else if cfg!(target_os = "linux") {
        Some(SandboxType::LinuxSeccomp)
    } else if cfg!(target_os = "windows") {
        if windows_sandbox_enabled {
            Some(SandboxType::WindowsRestrictedToken)
        } else {
            None
        }
    } else {
        None
    }
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
                // Build a podman run command that:
                // - Runs the command in a container
                // - Mounts the workspace at /workspace
                // - Uses the configured image
                // - Drops all capabilities, no new privileges
                // - Removes the container after exit
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
