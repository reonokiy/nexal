//! nexal-gateway — host-side multiplexer between an application
//! frontend (e.g. nexal Bun process) and many in-container
//! `nexal-agent` instances.
//!
//! ## Architecture
//!
//! ```text
//!                       ┌──────────────────────┐
//!                       │   nexal-gateway      │
//!  Frontend  ──WS──────▶│  (this crate)        │
//!  (nexal Bun)          │                      │
//!                       │  ┌────────────────┐  │
//!                       │  │ ContainerBkend │  │  ← podman impl
//!                       │  │ (spawn/kill)   │  │
//!                       │  └────────┬───────┘  │
//!                       │           │          │
//!                       │  ┌────────▼───────┐  │
//!                       │  │ AgentRegistry  │  │
//!                       │  │ (id ↔ AgentConn)│  │
//!                       │  └────────┬───────┘  │
//!                       └──────────┬┴──────────┘
//!                                  │ WS (one per agent)
//!                              ┌───▼────┐  ┌─────────┐
//!                              │ Agent  │  │ Agent   │  ...
//!                              │ (cont.)│  │ (cont.) │
//!                              └────────┘  └─────────┘
//! ```
//!
//! The frontend speaks JSON-RPC 2.0 over a single WebSocket. After a
//! `gateway/hello { token, clientName }` handshake, it can:
//!
//! - manage agent lifecycle (`gateway/spawnAgent` / `killAgent` /
//!   `detachAgent` / `attachAgent` / `listAgents`),
//! - forward arbitrary calls to a specific agent
//!   (`agent/invoke { agentId, method, params }`),
//! - receive notifications from agents wrapped as
//!   `agent/notify { agentId, method, params }`.
//!
//! Proxy listeners (Stage 4) and per-frontend authentication are also
//! gateway responsibilities, but live in their own modules to keep the
//! protocol surface small.

pub mod agent_conn;
pub mod backend;
pub mod config;
pub mod protocol;
pub mod registry;
pub mod server;

pub use config::GatewayConfig;
pub use registry::{AgentEntry, AgentId, AgentRegistry};
pub use server::serve;
