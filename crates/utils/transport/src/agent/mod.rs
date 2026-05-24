//! Agent protocol — method matrix, typed notifications, client, server.

pub mod client;
pub mod methods;
pub mod notifications;
pub mod server;

// Flatten the method-matrix types into `agent::*` so callers can keep
// writing `use nexal_utils_transport::agent::{AgentMethod, StreamKind, ...}`.
pub use methods::*;
