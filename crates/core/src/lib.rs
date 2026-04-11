//! Root of the `nexal-core` library.

// Prevent accidental direct writes to stdout/stderr in library code. All
// user-visible output must go through the appropriate abstraction (e.g.,
// the TUI or the tracing stack).
#![deny(clippy::print_stdout, clippy::print_stderr)]
#![recursion_limit = "256"]

mod api_bridge;
mod apply_patch;
mod apps;
mod arc_monitor;
pub use nexal_login as auth;
mod auth_env_telemetry;
mod client;
mod client_common;
pub mod nexal;
mod realtime_context;
mod realtime_conversation;
pub use nexal::SteerInputError;
mod nexal_thread;
mod compact_remote;
pub use nexal_thread::NexalThread;
pub use nexal_thread::ThreadConfigSnapshot;
mod agent;
mod nexal_delegate;
mod command_canonicalization;
mod commit_attribution;
pub mod config;
pub mod config_loader;
pub mod connectors;
mod context_manager;
mod contextual_user_message;
pub mod custom_prompts;
mod environment_context;
pub mod error;
pub mod exec;
pub mod exec_env;
mod exec_policy;
pub mod external_agent_config;
pub mod file_watcher;
mod flags;

mod guardian;
mod hook_runtime;
mod instructions;
pub mod mcp;
mod mcp_connection_manager;
mod mcp_tool_approval_templates;
pub mod models_manager;
mod network_policy_decision;
mod original_image_detail;
pub(crate) use mcp_connection_manager::SandboxState;
pub use text_encoding::bytes_to_string_smart;
mod mcp_tool_call;
mod memories;
pub mod mention_syntax;
pub mod message_history;
mod model_provider_info;
pub mod utils;
pub use utils::path_utils;
pub mod personality_migration;
pub mod plugins;
pub(crate) mod mentions {
    pub(crate) use crate::plugins::build_connector_slug_counts;
    pub(crate) use crate::plugins::build_skill_name_counts;
    pub(crate) use crate::plugins::collect_explicit_app_ids;
    pub(crate) use crate::plugins::collect_explicit_plugin_mentions;
    pub(crate) use crate::plugins::collect_tool_mentions_from_messages;
}
mod sandbox_tags;
pub mod sandboxing;
mod session_prefix;
mod session_startup_prewarm;
mod shell_detect;
pub mod skills;
pub(crate) use skills::SkillError;
pub(crate) use skills::SkillInjections;
pub(crate) use skills::SkillLoadOutcome;
pub(crate) use skills::SkillMetadata;
pub(crate) use skills::SkillsLoadInput;
pub(crate) use skills::SkillsManager;
pub(crate) use skills::build_skill_injections;
pub(crate) use skills::build_skill_name_counts;
pub(crate) use skills::collect_env_var_dependencies;
pub(crate) use skills::collect_explicit_skill_mentions;
pub(crate) use skills::config_rules;
pub(crate) use skills::injection;
pub(crate) use skills::loader;
pub(crate) use skills::manager;
pub(crate) use skills::maybe_emit_implicit_skill_invocation;
pub(crate) use skills::model;
pub(crate) use skills::render_skills_section;
pub(crate) use skills::resolve_skill_dependencies_for_turn;
pub(crate) use skills::skills_load_input_from_config;
mod skills_watcher;
mod stream_events_utils;
pub mod test_support;
mod text_encoding;
mod unified_exec;
pub use client::X_RESPONSESAPI_INCLUDE_TIMING_METRICS_HEADER;
pub use model_provider_info::DEFAULT_LMSTUDIO_PORT;
pub use model_provider_info::DEFAULT_OLLAMA_PORT;
pub use model_provider_info::LMSTUDIO_OSS_PROVIDER_ID;
pub use model_provider_info::ModelProviderInfo;
pub use model_provider_info::OLLAMA_OSS_PROVIDER_ID;
pub use model_provider_info::WireApi;
pub use model_provider_info::create_oss_provider_with_base_url;
mod event_mapping;
mod response_debug_context;
pub mod review_format;
pub mod review_prompts;
mod thread_manager;
pub mod web_search;
pub use thread_manager::ForkSnapshot;
pub use thread_manager::NewThread;
pub use thread_manager::ThreadManager;
// Re-export common auth types for workspace consumers
pub use auth::AuthManager;
pub use auth::NexalAuth;
/// Default Nexal HTTP client headers and reqwest construction.
///
/// Implemented in [`nexal_login::default_client`]; this module re-exports that API for crates
/// that import `nexal_core::default_client`.
pub mod default_client {
    pub use nexal_login::default_client::*;
}
pub mod project_doc;
mod rollout;
pub(crate) mod safety;
mod session_rollout_init_error;
pub mod shell;
#[allow(dead_code)]
mod shell_snapshot;
pub mod spawn;
pub use nexal_rollout::state_db;
mod thread_rollout_truncation;
mod tools;
mod turn_diff_tracker;
mod turn_metadata;
mod turn_timing;
pub use rollout::ARCHIVED_SESSIONS_SUBDIR;
pub use rollout::INTERACTIVE_SESSION_SOURCES;
pub use rollout::RolloutRecorder;
pub use rollout::RolloutRecorderParams;
pub use rollout::SESSIONS_SUBDIR;
pub use rollout::SessionMeta;
pub use rollout::append_thread_name;
pub use rollout::find_archived_thread_path_by_id_str;
pub use rollout::find_thread_name_by_id;
pub use rollout::find_thread_path_by_id_str;
pub use rollout::find_thread_path_by_name_str;
pub use rollout::list::Cursor;
pub use rollout::list::ThreadItem;
pub use rollout::list::ThreadSortKey;
pub use rollout::list::ThreadsPage;
pub use rollout::list::parse_cursor;
pub use rollout::list::read_head_for_summary;
pub use rollout::list::read_session_meta_line;
pub use rollout::policy::EventPersistenceMode;
pub use rollout::rollout_date_parts;
pub use rollout::session_index::find_thread_names_by_ids;
mod function_tool;
mod state;
mod tasks;
mod user_shell_command;
pub mod util;
pub(crate) use nexal_protocol::protocol;
pub(crate) use nexal_shell_command::bash;
pub(crate) use nexal_shell_command::is_dangerous_command;
pub(crate) use nexal_shell_command::is_safe_command;
pub(crate) use nexal_shell_command::parse_command;
pub(crate) use nexal_shell_command::powershell;

pub use client::ModelClient;
pub use client::ModelClientSession;
pub use client_common::Prompt;
pub use client_common::REVIEW_PROMPT;
pub use client_common::ResponseEvent;
pub use client_common::ResponseStream;
pub use nexal_sandboxing::get_platform_sandbox;
pub use compact::content_items_to_text;
pub use event_mapping::parse_turn_item;
pub use exec_policy::ExecPolicyError;
pub use exec_policy::check_execpolicy_for_warnings;
pub use exec_policy::format_exec_policy_error_with_source;
pub use exec_policy::load_exec_policy;
pub use file_watcher::FileWatcherEvent;
pub use tools::spec::parse_tool_input_schema;
pub mod compact;

/// Convert an [`AuthMode`](nexal_app_server_protocol::AuthMode) to a
/// [`TelemetryAuthMode`](nexal_protocol::telemetry_types::TelemetryAuthMode).
pub fn telemetry_auth_mode(
    mode: nexal_app_server_protocol::AuthMode,
) -> nexal_protocol::telemetry_types::TelemetryAuthMode {
    use nexal_protocol::telemetry_types::TelemetryAuthMode;
    match mode {
        nexal_app_server_protocol::AuthMode::ApiKey => TelemetryAuthMode::ApiKey,
    }
}
