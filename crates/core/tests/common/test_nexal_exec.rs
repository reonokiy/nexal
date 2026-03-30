#![allow(clippy::expect_used)]
use nexal_core::auth::NEXAL_API_KEY_ENV_VAR;
use std::path::Path;
use tempfile::TempDir;

pub struct TestNexalExecBuilder {
    home: TempDir,
    cwd: TempDir,
}

impl TestNexalExecBuilder {
    pub fn cmd(&self) -> assert_cmd::Command {
        let mut cmd = assert_cmd::Command::new(
            nexal_utils_cargo_bin::cargo_bin("nexal-exec")
                .expect("should find binary for nexal-exec"),
        );
        cmd.current_dir(self.cwd.path())
            .env("NEXAL_HOME", self.home.path())
            .env(NEXAL_API_KEY_ENV_VAR, "dummy");
        cmd
    }
    pub fn cwd_path(&self) -> &Path {
        self.cwd.path()
    }
    pub fn home_path(&self) -> &Path {
        self.home.path()
    }
}

pub fn test_nexal_exec() -> TestNexalExecBuilder {
    TestNexalExecBuilder {
        home: TempDir::new().expect("create temp home"),
        cwd: TempDir::new().expect("create temp cwd"),
    }
}
