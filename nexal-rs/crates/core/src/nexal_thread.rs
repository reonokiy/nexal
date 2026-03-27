use crate::agent::AgentStatus;
use crate::nexal::Nexal;
use crate::nexal::SteerInputError;
use crate::config::ConstraintResult;
use crate::error::NexalErr;
use crate::error::Result as NexalResult;
use crate::file_watcher::WatchRegistration;
use crate::protocol::Event;
use crate::protocol::Op;
use crate::protocol::Submission;
use nexal_features::Feature;
use nexal_protocol::config_types::ApprovalsReviewer;
use nexal_protocol::config_types::Personality;
use nexal_protocol::config_types::ServiceTier;
use nexal_protocol::models::ContentItem;
use nexal_protocol::models::ResponseInputItem;
use nexal_protocol::models::ResponseItem;
use nexal_protocol::openai_models::ReasoningEffort;
use nexal_protocol::protocol::AskForApproval;
use nexal_protocol::protocol::SandboxPolicy;
use nexal_protocol::protocol::SessionSource;
use nexal_protocol::protocol::TokenUsage;
use nexal_protocol::protocol::W3cTraceContext;
use nexal_protocol::user_input::UserInput;
use std::path::PathBuf;
use tokio::sync::Mutex;
use tokio::sync::watch;

use crate::state_db::StateDbHandle;

#[derive(Clone, Debug)]
pub struct ThreadConfigSnapshot {
    pub model: String,
    pub model_provider_id: String,
    pub service_tier: Option<ServiceTier>,
    pub approval_policy: AskForApproval,
    pub approvals_reviewer: ApprovalsReviewer,
    pub sandbox_policy: SandboxPolicy,
    pub cwd: PathBuf,
    pub ephemeral: bool,
    pub reasoning_effort: Option<ReasoningEffort>,
    pub personality: Option<Personality>,
    pub session_source: SessionSource,
}

pub struct NexalThread {
    pub(crate) nexal: Nexal,
    rollout_path: Option<PathBuf>,
    out_of_band_elicitation_count: Mutex<u64>,
    _watch_registration: WatchRegistration,
}

/// Conduit for the bidirectional stream of messages that compose a thread
/// (formerly called a conversation) in Nexal.
impl NexalThread {
    pub(crate) fn new(
        nexal: Nexal,
        rollout_path: Option<PathBuf>,
        watch_registration: WatchRegistration,
    ) -> Self {
        Self {
            nexal,
            rollout_path,
            out_of_band_elicitation_count: Mutex::new(0),
            _watch_registration: watch_registration,
        }
    }

    pub async fn submit(&self, op: Op) -> NexalResult<String> {
        self.nexal.submit(op).await
    }

    pub async fn shutdown_and_wait(&self) -> NexalResult<()> {
        self.nexal.shutdown_and_wait().await
    }

    pub async fn submit_with_trace(
        &self,
        op: Op,
        trace: Option<W3cTraceContext>,
    ) -> NexalResult<String> {
        self.nexal.submit_with_trace(op, trace).await
    }

    pub async fn steer_input(
        &self,
        input: Vec<UserInput>,
        expected_turn_id: Option<&str>,
    ) -> Result<String, SteerInputError> {
        self.nexal.steer_input(input, expected_turn_id).await
    }

    pub async fn set_app_server_client_name(
        &self,
        app_server_client_name: Option<String>,
    ) -> ConstraintResult<()> {
        self.nexal
            .set_app_server_client_name(app_server_client_name)
            .await
    }

    /// Use sparingly: this is intended to be removed soon.
    pub async fn submit_with_id(&self, sub: Submission) -> NexalResult<()> {
        self.nexal.submit_with_id(sub).await
    }

    pub async fn next_event(&self) -> NexalResult<Event> {
        self.nexal.next_event().await
    }

    pub async fn agent_status(&self) -> AgentStatus {
        self.nexal.agent_status().await
    }

    pub(crate) fn subscribe_status(&self) -> watch::Receiver<AgentStatus> {
        self.nexal.agent_status.clone()
    }

    pub(crate) async fn total_token_usage(&self) -> Option<TokenUsage> {
        self.nexal.session.total_token_usage().await
    }

    /// Records a user-role session-prefix message without creating a new user turn boundary.
    pub(crate) async fn inject_user_message_without_turn(&self, message: String) {
        let message = ResponseItem::Message {
            id: None,
            role: "user".to_string(),
            content: vec![ContentItem::InputText { text: message }],
            end_turn: None,
            phase: None,
        };
        let pending_item = match pending_message_input_item(&message) {
            Ok(pending_item) => pending_item,
            Err(err) => {
                debug_assert!(false, "session-prefix message append should succeed: {err}");
                return;
            }
        };
        if self
            .nexal
            .session
            .inject_response_items(vec![pending_item])
            .await
            .is_err()
        {
            let turn_context = self.nexal.session.new_default_turn().await;
            self.nexal
                .session
                .record_conversation_items(turn_context.as_ref(), &[message])
                .await;
        }
    }

    /// Append a prebuilt message to the thread history without treating it as a user turn.
    ///
    /// If the thread already has an active turn, the message is queued as pending input for that
    /// turn. Otherwise it is queued at session scope and a regular turn is started so the agent
    /// can consume that pending input through the normal turn pipeline.
    #[cfg(test)]
    pub(crate) async fn append_message(&self, message: ResponseItem) -> NexalResult<String> {
        let submission_id = uuid::Uuid::new_v4().to_string();
        let pending_item = pending_message_input_item(&message)?;
        if let Err(items) = self
            .nexal
            .session
            .inject_response_items(vec![pending_item])
            .await
        {
            self.nexal
                .session
                .queue_response_items_for_next_turn(items)
                .await;
            self.nexal
                .session
                .ensure_task_for_queued_response_items()
                .await;
        }

        Ok(submission_id)
    }

    pub fn rollout_path(&self) -> Option<PathBuf> {
        self.rollout_path.clone()
    }

    pub fn state_db(&self) -> Option<StateDbHandle> {
        self.nexal.state_db()
    }

    pub async fn config_snapshot(&self) -> ThreadConfigSnapshot {
        self.nexal.thread_config_snapshot().await
    }

    pub fn enabled(&self, feature: Feature) -> bool {
        self.nexal.enabled(feature)
    }

    pub async fn increment_out_of_band_elicitation_count(&self) -> NexalResult<u64> {
        let mut guard = self.out_of_band_elicitation_count.lock().await;
        let was_zero = *guard == 0;
        *guard = guard.checked_add(1).ok_or_else(|| {
            NexalErr::Fatal("out-of-band elicitation count overflowed".to_string())
        })?;

        if was_zero {
            self.nexal
                .session
                .set_out_of_band_elicitation_pause_state(/*paused*/ true);
        }

        Ok(*guard)
    }

    pub async fn decrement_out_of_band_elicitation_count(&self) -> NexalResult<u64> {
        let mut guard = self.out_of_band_elicitation_count.lock().await;
        if *guard == 0 {
            return Err(NexalErr::InvalidRequest(
                "out-of-band elicitation count is already zero".to_string(),
            ));
        }

        *guard -= 1;
        let now_zero = *guard == 0;
        if now_zero {
            self.nexal
                .session
                .set_out_of_band_elicitation_pause_state(/*paused*/ false);
        }

        Ok(*guard)
    }
}

fn pending_message_input_item(message: &ResponseItem) -> NexalResult<ResponseInputItem> {
    match message {
        ResponseItem::Message { role, content, .. } => Ok(ResponseInputItem::Message {
            role: role.clone(),
            content: content.clone(),
        }),
        _ => Err(NexalErr::InvalidRequest(
            "append_message only supports ResponseItem::Message".to_string(),
        )),
    }
}
