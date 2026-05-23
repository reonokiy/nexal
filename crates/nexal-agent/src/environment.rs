use std::sync::Arc;

use crate::core::ExecServerError;
use crate::fs::ExecutorFileSystem;
use crate::fs::local::LocalFileSystem;
use crate::process::ExecBackend;
use crate::process::local::LocalProcess;

#[derive(Clone)]
pub struct Environment {
    exec_backend: Arc<dyn ExecBackend>,
}

impl Default for Environment {
    fn default() -> Self {
        let local_process = LocalProcess::default();
        if let Err(err) = local_process.initialize() {
            panic!("default local process initialization should succeed: {err:?}");
        }
        if let Err(err) = local_process.initialized() {
            panic!("default local process should accept initialized notification: {err}");
        }

        Self {
            exec_backend: Arc::new(local_process),
        }
    }
}

impl std::fmt::Debug for Environment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Environment").finish_non_exhaustive()
    }
}

impl Environment {
    pub async fn create() -> Result<Self, ExecServerError> {
        let local_process = LocalProcess::default();
        local_process
            .initialize()
            .map_err(|err| ExecServerError::Protocol(err.message))?;
        local_process
            .initialized()
            .map_err(ExecServerError::Protocol)?;

        Ok(Self {
            exec_backend: Arc::new(local_process),
        })
    }

    pub fn get_exec_backend(&self) -> Arc<dyn ExecBackend> {
        Arc::clone(&self.exec_backend)
    }

    pub fn get_filesystem(&self) -> Arc<dyn ExecutorFileSystem> {
        Arc::new(LocalFileSystem)
    }
}
