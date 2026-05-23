# nexal-agent

`nexal-agent` is the sandbox-side JSON-RPC server for Nexal. It runs inside a sandbox container and exposes process execution, PTY I/O, and filesystem operations to the LLM server through the gateway.

It provides:

- a standalone binary: `nexal-agent`
- a WebSocket JSON-RPC server
- protocol types for initialize, process, and filesystem methods
- process execution backed by `nexal-utils-pty`
- filesystem services used by sandbox tools

## Transport

The server listens on a WebSocket URL, defaulting to `ws://127.0.0.1:8765` unless configured otherwise by the binary options or embedding code.

Wire framing:

- WebSocket text frames carry one JSON-RPC message each.
- Clients must complete the `initialize` / `initialized` lifecycle before normal RPC calls.

## Lifecycle

Each connection follows this sequence:

1. Send `initialize`.
2. Wait for the `initialize` response.
3. Send `initialized`.
4. Call process or filesystem RPCs.

If the WebSocket connection closes, the server terminates remaining managed processes for that client connection.

## Process API

### `initialize`

Initial handshake request.

Request params:

```json
{
  "clientName": "my-client"
}
```

Response:

```json
{}
```

### `initialized`

Handshake acknowledgement notification sent by the client after a successful `initialize` response.

### `command/exec`

Starts a new managed process.

Request params:

```json
{
  "processId": "proc-1",
  "argv": ["bash", "-lc", "printf 'hello\\n'"],
  "cwd": "/absolute/working/directory",
  "env": {
    "PATH": "/usr/bin:/bin"
  },
  "tty": true,
  "outputBytesCap": 16384,
  "arg0": null
}
```

Field definitions:

- `processId`: caller-chosen stable id for this process within the connection.
- `argv`: command vector. It must be non-empty.
- `cwd`: absolute working directory used for the child process.
- `env`: environment variables passed to the child process.
- `tty`: when `true`, spawn a PTY-backed interactive process; when `false`, spawn a pipe-backed process with closed stdin.
- `outputBytesCap`: maximum retained stdout/stderr bytes per stream for the in-memory buffer.
- `arg0`: optional argv0 override forwarded to `nexal-utils-pty`.

Response:

```json
{
  "processId": "proc-1",
  "running": true,
  "exitCode": null,
  "stdout": null,
  "stderr": null
}
```

Behavior notes:

- Reusing an existing `processId` is rejected.
- PTY-backed processes accept later writes through `command/exec/write`.
- Pipe-backed processes are launched with stdin closed and reject writes.
- Output is streamed asynchronously via `command/exec/outputDelta`.
- Exit is reported asynchronously via `command/exec/exited`.

### `command/exec/write`

Writes raw bytes to a running PTY-backed process stdin.

Request params:

```json
{
  "processId": "proc-1",
  "chunk": "aGVsbG8K"
}
```

`chunk` is base64-encoded raw bytes. In the example above it is `hello\n`.

Response:

```json
{
  "accepted": true
}
```

### `command/exec/terminate`

Terminates a running managed process.

Request params:

```json
{
  "processId": "proc-1"
}
```

Response:

```json
{
  "running": true
}
```

If the process is already unknown or already removed, the server responds with:

```json
{
  "running": false
}
```

## Notifications

### `command/exec/outputDelta`

Streaming output chunk from a running process.

Params:

```json
{
  "processId": "proc-1",
  "stream": "stdout",
  "chunk": "aGVsbG8K"
}
```

### `command/exec/exited`

Final process exit notification.

Params:

```json
{
  "processId": "proc-1",
  "exitCode": 0
}
```

## Filesystem API

Filesystem RPC services are implemented under `crates/nexal-agent/src/server/services/file_system.rs` and use the types exported from `crates/nexal-agent/src/fs/`. See the protocol module and service implementation for the exact method names and params when adding or updating clients.

## Errors

The server returns JSON-RPC errors with standard JSON-RPC codes such as:

- `-32600`: invalid request
- `-32602`: invalid params
- `-32603`: internal error

Typical error cases include unknown methods, malformed params, empty `argv`, duplicate `processId`, writes to unknown processes, and writes to non-PTY processes.

## Rust Surface

The crate exports server, protocol, process, filesystem, and environment types from `src/lib.rs`, including:

- `Environment`
- `ExecServerError`
- `ProcessId`
- `ExecutorFileSystem` and filesystem option/result types
- `ExecBackend`, `ExecProcess`, and `StartedExecProcess`
- protocol wire and error types
- `DEFAULT_LISTEN_URL`, `run_main()`, and `run_main_with_listen_url()`

## Example Session

Initialize:

```json
{"id":1,"method":"initialize","params":{"clientName":"example-client"}}
{"id":1,"result":{}}
{"method":"initialized","params":{}}
```

Start a process:

```json
{"id":2,"method":"command/exec","params":{"processId":"proc-1","argv":["bash","-lc","printf 'ready\\n'; while IFS= read -r line; do printf 'echo:%s\\n' \"$line\"; done"],"cwd":"/tmp","env":{"PATH":"/usr/bin:/bin"},"tty":true,"outputBytesCap":4096,"arg0":null}}
{"id":2,"result":{"processId":"proc-1","running":true,"exitCode":null,"stdout":null,"stderr":null}}
{"method":"command/exec/outputDelta","params":{"processId":"proc-1","stream":"stdout","chunk":"cmVhZHkK"}}
```

Write to the process:

```json
{"id":3,"method":"command/exec/write","params":{"processId":"proc-1","chunk":"aGVsbG8K"}}
{"id":3,"result":{"accepted":true}}
{"method":"command/exec/outputDelta","params":{"processId":"proc-1","stream":"stdout","chunk":"ZWNobzpoZWxsbwo="}}
```

Terminate it:

```json
{"id":4,"method":"command/exec/terminate","params":{"processId":"proc-1"}}
{"id":4,"result":{"running":true}}
{"method":"command/exec/exited","params":{"processId":"proc-1","exitCode":0}}
```
