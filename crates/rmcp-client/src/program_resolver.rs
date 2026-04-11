//! Platform-specific program resolution for MCP server execution.
//!
//! This module provides a unified interface for resolving executable paths
//! across different operating systems. The key challenge it addresses is that
//! Windows cannot execute script files (e.g., `.cmd`, `.bat`) directly through
//! `Command::new()` without their file extensions, while Unix systems handle
//! scripts natively through shebangs.
//!
//! The `resolve` function abstracts these platform differences:
//! - On Unix: Returns the program unchanged (OS handles script execution)
//! - On Windows: Uses the `which` crate to resolve full paths including extensions

use std::collections::HashMap;
use std::ffi::OsString;

/// Resolves a program to its executable path.
///
/// Unix systems handle PATH resolution and script execution natively through
/// the kernel's shebang (`#!`) mechanism, so this function simply returns
/// the program name unchanged.
pub fn resolve(program: OsString, _env: &HashMap<OsString, OsString>) -> std::io::Result<OsString> {
    Ok(program)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::create_env_for_mcp_server;
    use anyhow::Result;
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;
    use tokio::process::Command;

    /// Verifies the OS handles script execution without file extensions.
    #[tokio::test]
    async fn test_unix_executes_script_without_extension() -> Result<()> {
        let env = TestExecutableEnv::new()?;
        let mut cmd = Command::new(&env.program_name);
        cmd.envs(&env.mcp_env);

        let output = cmd.output().await;
        assert!(output.is_ok(), "Unix should execute scripts directly");
        Ok(())
    }

    /// Verifies program resolution enables successful execution.
    #[tokio::test]
    async fn test_resolved_program_executes_successfully() -> Result<()> {
        let env = TestExecutableEnv::new()?;
        let program = OsString::from(&env.program_name);

        let resolved = resolve(program, &env.mcp_env)?;

        let mut cmd = Command::new(resolved);
        cmd.envs(&env.mcp_env);
        let output = cmd.output().await;

        assert!(
            output.is_ok(),
            "Resolved program should execute successfully"
        );
        Ok(())
    }

    // Test fixture for creating temporary executables in a controlled environment.
    struct TestExecutableEnv {
        // Held to prevent the temporary directory from being deleted.
        _temp_dir: TempDir,
        program_name: String,
        mcp_env: HashMap<OsString, OsString>,
    }

    impl TestExecutableEnv {
        const TEST_PROGRAM: &'static str = "test_mcp_server";

        fn new() -> Result<Self> {
            let temp_dir = TempDir::new()?;
            let dir_path = temp_dir.path();

            Self::create_executable(dir_path)?;

            // Build a clean environment with the temp dir in the PATH.
            let mut extra_env = HashMap::new();
            extra_env.insert(OsString::from("PATH"), Self::build_path_env_var(dir_path));

            let mcp_env = create_env_for_mcp_server(Some(extra_env), &[]);

            Ok(Self {
                _temp_dir: temp_dir,
                program_name: Self::TEST_PROGRAM.to_string(),
                mcp_env,
            })
        }

        /// Creates a simple executable script.
        fn create_executable(dir: &Path) -> Result<()> {
            let file = dir.join(Self::TEST_PROGRAM);
            fs::write(&file, "#!/bin/sh\nexit 0")?;
            Self::set_executable(&file)?;
            Ok(())
        }

        fn set_executable(path: &Path) -> Result<()> {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(path)?.permissions();
            perms.set_mode(0o755);
            fs::set_permissions(path, perms)?;
            Ok(())
        }

        /// Prepends the given directory to the system's PATH variable.
        fn build_path_env_var(dir: &Path) -> OsString {
            let mut path = OsString::from(dir.as_os_str());
            if let Some(current) = std::env::var_os("PATH") {
                path.push(":");
                path.push(current);
            }
            path
        }
    }
}
