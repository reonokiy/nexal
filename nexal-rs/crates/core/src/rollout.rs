use crate::config::Config;
pub use nexal_rollout::ARCHIVED_SESSIONS_SUBDIR;
pub use nexal_rollout::INTERACTIVE_SESSION_SOURCES;
pub use nexal_rollout::RolloutRecorder;
pub use nexal_rollout::RolloutRecorderParams;
pub use nexal_rollout::SESSIONS_SUBDIR;
pub use nexal_rollout::SessionMeta;
pub use nexal_rollout::append_thread_name;
pub use nexal_rollout::find_archived_thread_path_by_id_str;
#[deprecated(note = "use find_thread_path_by_id_str")]
pub use nexal_rollout::find_conversation_path_by_id_str;
pub use nexal_rollout::find_thread_name_by_id;
pub use nexal_rollout::find_thread_path_by_id_str;
pub use nexal_rollout::find_thread_path_by_name_str;
pub use nexal_rollout::rollout_date_parts;

impl nexal_rollout::RolloutConfigView for Config {
    fn nexal_home(&self) -> &std::path::Path {
        self.nexal_home.as_path()
    }

    fn sqlite_home(&self) -> &std::path::Path {
        self.sqlite_home.as_path()
    }

    fn cwd(&self) -> &std::path::Path {
        self.cwd.as_path()
    }

    fn model_provider_id(&self) -> &str {
        self.model_provider_id.as_str()
    }

    fn generate_memories(&self) -> bool {
        self.memories.generate_memories
    }
}

pub mod list {
    pub use nexal_rollout::list::*;
}

pub(crate) mod metadata {
    pub(crate) use nexal_rollout::metadata::builder_from_items;
}

pub mod policy {
    pub use nexal_rollout::policy::*;
}

pub mod recorder {
    pub use nexal_rollout::recorder::*;
}

pub mod session_index {
    pub use nexal_rollout::session_index::*;
}

pub(crate) use crate::session_rollout_init_error::map_session_init_error;

pub(crate) mod truncation {
    pub(crate) use crate::thread_rollout_truncation::*;
}
