//! Per-agent HTTP reverse proxy.
//!
//! The frontend registers an upstream + auth headers scoped to a
//! specific `agent_id`. The gateway returns an opaque `token` and the
//! URL the agent's container should hit:
//!
//!   `http://<external_host>/p/<token>/<rest>`
//!
//! When something inside the container makes a request to that URL,
//! the proxy server (this module) looks up the token, strips the
//! `/p/<token>` prefix, injects the registered headers on top of the
//! caller's headers, and forwards to `<upstream_url>/<rest>` with the
//! original method, query string, body, and pass-through headers.
//!
//! Cleanup: when an `agent_id` is killed, every registration belonging
//! to it is dropped automatically (`AgentRegistry.kill` calls
//! `ProxyRegistry::cleanup_for_agent`). Frontends can also call
//! `gateway/unregister_proxy` to drop a specific (`agent_id`, `name`)
//! pair early.

pub mod registry;
pub mod server;

pub use registry::{ProxyEntry, ProxyRegistry, SharedProxyRegistry};
pub use server::serve_proxy;
