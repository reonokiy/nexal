# Rust crates to keep on nexal-pi branch

Only the sandbox/exec runtime is kept. Everything else (agent, channels,
TUI, config, core, app-server, etc.) is replaced by the Bun/TS port in
`packages/nexal/`.

## Keep (transitive closure of exec-server)

- `crates/exec-server`                      — the binary
- `crates/app-server-protocol`              — wire types
- `crates/protocol`                         — core protocol types
- `crates/experimental-api-macros`          — derive macros used by protocol
- `crates/git-utils`                        — used by app-server-protocol
- `crates/utils/absolute-path`
- `crates/utils/pty`
- `crates/utils/cargo-bin`
- (verify with `cargo tree -p nexal-exec-server --prefix none | sort -u`)

## Possibly keep (used by sandboxing runtime at runtime)

- `crates/sandboxing`
- `crates/linux-sandbox`
- `crates/shell-command`
- `crates/ansi-escape`

## Remove (once TS port subsumes their role)

- `crates/agent` → replaced by `packages/nexal/src/agent-pool.ts`
- `crates/channel-*` → replaced by `packages/nexal/src/channels/*`
- `crates/nexal`, `crates/nexal-config`, `crates/nexal-state`
- `crates/core`, `crates/tui`, `crates/tui-render`
- `crates/app-server`, `crates/app-server-client`
- `crates/codex-*`, `crates/rmcp-client`, `crates/plugin`
- `crates/config`, `crates/core-skills`, `crates/skills`
- Anything else in `crates/` that is not in the "Keep" list above

Cleanup happens in a follow-up commit once the TS port is running end-to-end.
