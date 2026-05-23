//! Channel method routing types.

use serde::{Deserialize, Serialize};

/// Parsed channel method name, grouped by domain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Lifecycle(LifecycleMethod),
    Process(ProcessMethod),
    FileSystem(FsMethod),
    Proxy(ProxyMethod),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecycleMethod {
    Initialize,
    Initialized,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessMethod {
    Start,
    Read,
    Write,
    Terminate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FsMethod {
    ReadFile,
    WriteFile,
    CreateDirectory,
    GetMetadata,
    ReadDirectory,
    Remove,
    Copy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProxyMethod {
    Register,
    Unregister,
}

/// Parse a wire-format method name into a [`Method`].
pub fn parse_method(s: &str) -> Option<Method> {
    match s {
        "initialize" => Some(Method::Lifecycle(LifecycleMethod::Initialize)),
        "initialized" => Some(Method::Lifecycle(LifecycleMethod::Initialized)),
        "process/start" => Some(Method::Process(ProcessMethod::Start)),
        "process/read" => Some(Method::Process(ProcessMethod::Read)),
        "process/write" => Some(Method::Process(ProcessMethod::Write)),
        "process/terminate" => Some(Method::Process(ProcessMethod::Terminate)),
        "fs/read_file" => Some(Method::FileSystem(FsMethod::ReadFile)),
        "fs/write_file" => Some(Method::FileSystem(FsMethod::WriteFile)),
        "fs/create_directory" => Some(Method::FileSystem(FsMethod::CreateDirectory)),
        "fs/get_metadata" => Some(Method::FileSystem(FsMethod::GetMetadata)),
        "fs/read_directory" => Some(Method::FileSystem(FsMethod::ReadDirectory)),
        "fs/remove" => Some(Method::FileSystem(FsMethod::Remove)),
        "fs/copy" => Some(Method::FileSystem(FsMethod::Copy)),
        "proxy/register" => Some(Method::Proxy(ProxyMethod::Register)),
        "proxy/unregister" => Some(Method::Proxy(ProxyMethod::Unregister)),
        _ => None,
    }
}

/// Notification method names (agent → gateway, no request ID).
pub const EXEC_OUTPUT_DELTA_METHOD: &str = "process/output";
pub const EXEC_EXITED_METHOD: &str = "process/exited";
pub const EXEC_CLOSED_METHOD: &str = "process/closed";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecOutputStream {
    Stdout,
    Stderr,
    Pty,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WriteStatus {
    Accepted,
    UnknownProcess,
    StdinClosed,
    Starting,
}
