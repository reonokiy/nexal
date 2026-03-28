//! Stubbed-out cloud-tasks-client crate.
//!
//! The ChatGPT-backend cloud task queue integration has been removed.
//! This crate retains the public type definitions so that downstream code
//! continues to compile, but all runtime functionality is a no-op.

mod api;

pub use api::ApplyOutcome;
pub use api::ApplyStatus;
pub use api::AttemptStatus;
pub use api::CloudBackend;
pub use api::CloudTaskError;
pub use api::CreatedTask;
pub use api::DiffSummary;
pub use api::Result;
pub use api::TaskId;
pub use api::TaskListPage;
pub use api::TaskStatus;
pub use api::TaskSummary;
pub use api::TaskText;
pub use api::TurnAttempt;
