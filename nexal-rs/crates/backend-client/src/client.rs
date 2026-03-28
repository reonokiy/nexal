use crate::types::CodeTaskDetailsResponse;
use crate::types::ConfigFileResponse;
use crate::types::PaginatedListTaskListItem;
use crate::types::TurnAttemptsSiblingTurnsResponse;
use anyhow::Result;
use nexal_core::auth::NexalAuth;
use nexal_protocol::protocol::RateLimitSnapshot;
use std::fmt;

/// Error type preserved for API compatibility.
#[derive(Debug)]
pub enum RequestError {
    Other(anyhow::Error),
}

impl RequestError {
    pub fn status(&self) -> Option<u16> {
        None
    }

    pub fn is_unauthorized(&self) -> bool {
        false
    }
}

impl fmt::Display for RequestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Other(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for RequestError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Other(err) => Some(err.as_ref()),
        }
    }
}

impl From<anyhow::Error> for RequestError {
    fn from(err: anyhow::Error) -> Self {
        Self::Other(err)
    }
}

/// Stub ChatGPT backend client. All network methods return an error.
#[derive(Clone, Debug)]
pub struct Client {
    _private: (),
}

impl Client {
    pub fn new(_base_url: impl Into<String>) -> Result<Self> {
        Ok(Self { _private: () })
    }

    pub fn from_auth(_base_url: impl Into<String>, _auth: &NexalAuth) -> Result<Self> {
        Ok(Self { _private: () })
    }

    pub fn with_bearer_token(self, _token: impl Into<String>) -> Self {
        self
    }

    pub fn with_user_agent(self, _ua: impl Into<String>) -> Self {
        self
    }

    pub fn with_chatgpt_account_id(self, _account_id: impl Into<String>) -> Self {
        self
    }

    pub async fn get_rate_limits_many(&self) -> Result<Vec<RateLimitSnapshot>> {
        anyhow::bail!("backend client is disabled")
    }

    pub async fn list_tasks(
        &self,
        _limit: Option<i32>,
        _task_filter: Option<&str>,
        _environment_id: Option<&str>,
        _cursor: Option<&str>,
    ) -> Result<PaginatedListTaskListItem> {
        anyhow::bail!("backend client is disabled")
    }

    pub async fn get_task_details(&self, _task_id: &str) -> Result<CodeTaskDetailsResponse> {
        anyhow::bail!("backend client is disabled")
    }

    pub async fn get_task_details_with_body(
        &self,
        _task_id: &str,
    ) -> Result<(CodeTaskDetailsResponse, String, String)> {
        anyhow::bail!("backend client is disabled")
    }

    pub async fn list_sibling_turns(
        &self,
        _task_id: &str,
        _turn_id: &str,
    ) -> Result<TurnAttemptsSiblingTurnsResponse> {
        anyhow::bail!("backend client is disabled")
    }

    pub async fn get_config_requirements_file(
        &self,
    ) -> std::result::Result<ConfigFileResponse, RequestError> {
        Err(RequestError::Other(anyhow::anyhow!(
            "backend client is disabled"
        )))
    }

    pub async fn create_task(&self, _request_body: serde_json::Value) -> Result<String> {
        anyhow::bail!("backend client is disabled")
    }
}
