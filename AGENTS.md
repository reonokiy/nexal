# AGENTS.md

> 这是 Nexal 项目的 agent 指南。完整的项目结构文档见 [`docs/structure.md`](./docs/structure.md)。

## 项目简介

Nexal 是一个多通道 AI 代理运行时，包含四个核心组件：

1. **LLM Server** (`src/`) — Bun/TypeScript 守护进程，运行 Agent、通道、命令、Worker 和工具
2. **Gateway** (`crates/nexal-gateway/`) — Rust WebSocket 网关，认证请求并管理沙箱后端
3. **Agent** (`crates/nexal-agent/`) — Rust 沙箱执行器，在容器内提供 bash、文件和进程 JSON-RPC 工具
4. **Web UI** (`web/`) — Svelte 5 前端，通过 WebSocket 连接到 LLM Server

## 快速导航

- **架构总览** → `docs/structure.md`
- **TypeScript 核心** → `src/` (CLI 入口 `src/cli.ts`, 启动实现 `src/index.ts`, 配置 `src/config.ts`)
- **Rust 工作空间** → `crates/` (网关 `crates/nexal-gateway/`, 代理 `crates/nexal-agent/`)
- **Web 前端** → `web/src/` (入口 `web/src/main.ts`, 根组件 `web/src/App.svelte`)
- **共享传输包** → `packages/transport/`（含 chat 协议）
- **Tape 类型包** → `packages/tape/`
- **部署配置** → `deploy/{gateway,server,agent,sandbox}/`, `.github/workflows/`
- **数据库迁移** → `drizzle/`

## Agent 操作须知

### 技术栈

- **后端**: Bun >= 1.3 + TypeScript (`pi-agent-core`, `pi-ai`, Drizzle ORM)
- **网关**: Rust + Tokio + Axum
- **前端**: Svelte 5 + Tailwind CSS 4 + Vite
- **数据库**: Postgres (Neon)
- **认证**: Supabase Auth/JWT
- **部署**: Fly.io + GitHub Actions

### 修改代码时的惯例

1. **TypeScript 优先用 Bun**: 运行用 `bun run <file>`，包管理用 `bun install`
2. **Rust 用 Cargo**: `cargo check`, `cargo test`, `cargo build --release`
3. **Web UI 改动后检查**: `cd web && bun run check`
4. **数据库 schema 变更**: 编辑 `src/schema.ts` 或相关 schema 后运行 `bun run db:generate`
5. **环境变量**: 开发变量写在 `.env`（已 gitignored），模板在 `.env.example`

### 配置模型

- `src/config.ts` 加载默认值、`~/.nexal/config.toml` 或 `NEXAL_CONFIG_PATH`、以及 `NEXAL_*` 环境变量覆盖。
- Gateway 凭据、数据库连接、执行器和存储等基础配置仍来自 config/env。
- 模型、Provider、认证、Channel 和工具 API key 配置主要存储在 Postgres settings KV 中，见 `src/settings.ts`。
- 这些 DB-backed settings 可通过 Web UI 或 `/model`、`/apikey`、`/providers`、`/settings` 等命令管理。

### 关键文件位置

| 功能 | 文件 |
|------|------|
| LLM Server CLI 入口 | `src/cli.ts` |
| LLM Server 启动实现 | `src/index.ts` |
| 配置加载 | `src/config.ts` |
| DB settings | `src/settings.ts` |
| Supabase/JWT 认证 | `src/auth.ts` |
| Agent 池 | `src/agent-pool.ts` |
| 通道管理 | `src/channels/manager.ts` |
| WebSocket 通道 | `src/channels/ws.ts` |
| Telegram 通道 | `src/channels/telegram/channel.ts` |
| GitHub 通道 | `src/channels/github.ts` |
| 网关客户端 | `src/gateway/client.ts` |
| Bash 工具 | `src/tools/bash.ts` |
| Worker/Coordinator 工具 | `src/tools/coordinator/` |
| 内置命令 | `src/commands/builtin.ts` |
| Web UI 根 | `web/src/App.svelte` |
| Web UI 设置 | `web/src/lib/settings.svelte.ts` |
| Web UI 认证 | `web/src/lib/supabase.svelte.ts` |
| Web UI 聊天视图 | `web/src/lib/views/chat-view.svelte` |
| 聊天客户端 | `packages/chat-client/src/client.ts` |
| 网关服务器 | `crates/nexal-gateway/src/server.rs` |
| 网关后端 | `crates/nexal-gateway/src/backend/` |
| 代理服务器 | `crates/nexal-agent/src/server.rs` |

### 部署注意事项

- **Gateway** 部署到 `nexal` Fly app (`deploy/gateway/fly.toml`)
- **LLM Server** 部署到 `nexal-server` Fly app (`deploy/server/fly.toml`)
- **Gateway 认证**: 通过 `NEXAL_GATEWAY_ACCESS_KEY` + `NEXAL_GATEWAY_SECRET_KEY` 的 HMAC-SHA256 签名
- **CI/CD**: `.github/workflows/` 包含 gateway/server/web 部署、web build、sandbox Docker image 和 release workflow

### 本地开发

```bash
# LLM Server (带热重载)
bun run dev

# TypeScript 类型检查和测试
bun run typecheck
bun test

# 构建/检查 Rust
cargo check
cargo test
cargo build --release

# Web UI
cd web && bun run dev    # http://localhost:5173
cd web && bun run check
```

### Web UI 默认配置

- **默认后端 URL**: `wss://nexal-server.fly.dev`，可用 `VITE_NEXAL_BACKEND` 或设置页覆盖
- **设置存储**: `localStorage`，前缀 `nexal.`

## 更多文档

- [`docs/structure.md`](./docs/structure.md) — 完整的目录结构、架构图、数据流
- [`crates/nexal-agent/README.md`](./crates/nexal-agent/README.md) — Agent JSON-RPC API 文档
- [`crates/README.md`](./crates/README.md) — Rust workspace 说明
