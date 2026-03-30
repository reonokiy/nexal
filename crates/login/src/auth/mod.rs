pub mod default_client;
mod storage;

mod manager;

pub use manager::*;

/// Retained for backward compatibility with consumers that reference this type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefreshTokenFailedError {
    pub message: String,
}

impl RefreshTokenFailedError {
    pub fn new(_reason: RefreshTokenFailedReason, message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for RefreshTokenFailedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for RefreshTokenFailedError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefreshTokenFailedReason {
    Expired,
    Exhausted,
    Revoked,
    Other,
}

#[derive(Debug)]
pub enum RefreshTokenError {
    Permanent(RefreshTokenFailedError),
    Transient(std::io::Error),
}

impl From<RefreshTokenError> for std::io::Error {
    fn from(err: RefreshTokenError) -> Self {
        match err {
            RefreshTokenError::Permanent(failed) => std::io::Error::other(failed.message),
            RefreshTokenError::Transient(inner) => inner,
        }
    }
}

impl std::fmt::Display for RefreshTokenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Permanent(e) => write!(f, "{e}"),
            Self::Transient(e) => write!(f, "{e}"),
        }
    }
}

pub struct UnauthorizedRecovery {
    _private: (),
}

impl UnauthorizedRecovery {
    pub fn has_next(&self) -> bool {
        false
    }

    pub fn unavailable_reason(&self) -> &'static str {
        "not_chatgpt_auth"
    }

    pub fn mode_name(&self) -> &'static str {
        "none"
    }

    pub fn step_name(&self) -> &'static str {
        "done"
    }

    pub async fn next(&mut self) -> Result<UnauthorizedRecoveryStepResult, RefreshTokenError> {
        Err(RefreshTokenError::Permanent(RefreshTokenFailedError::new(
            RefreshTokenFailedReason::Other,
            "No recovery steps available (API key auth only).",
        )))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UnauthorizedRecoveryStepResult {
    auth_state_changed: Option<bool>,
}

impl UnauthorizedRecoveryStepResult {
    pub fn auth_state_changed(&self) -> Option<bool> {
        self.auth_state_changed
    }
}
