# AGENTS.md

> 这是 Nexal 项目的 agent 指南。完整的项目结构文档见 [`docs/structure.md`](./docs/structure.md)。

## 项目简介

Nexal 是一个多通道 AI 代理运行时，包含四个核心组件：

1. **LLM Server** (`src/`) — Bun/TypeScript 守护进程，运行 Agent、通道和工具
2. **Gateway** (`crates/nexal-gateway/`) — Rust WebSocket 网关，管理沙箱容器
3. **Agent** (`crates/nexal-agent/`) — Rust 沙箱执行器，在容器内运行 bash/文件/进程工具
4. **Web UI** (`web/`) — Svelte 5 前端，通过 WebSocket 连接到 LLM Server

## 快速导航

- **架构总览** → `docs/structure.md`
- **TypeScript 核心** → `src/` (入口 `src/index.ts`, 配置 `src/config.ts`)
- **Rust 工作空间** → `crates/` (网关 `crates/nexal-gateway/`, 代理 `crates/nexal-agent/`)
- **Web 前端** → `web/src/` (入口 `web/src/App.svelte`, 设置 `web/src/lib/settings.svelte.ts`)
- **共享客户端库** → `packages/chat-client/`
- **部署配置** → `fly.toml`, `deploy/server/fly.toml`, `.github/workflows/`
- **数据库迁移** → `drizzle/`

## Agent 操作须知

### 技术栈

- **后端**: Bun ≥ 1.3 + TypeScript (`pi-agent-core`, `pi-ai`, Drizzle ORM)
- **网关**: Rust + Tokio + Axum
- **前端**: Svelte 5 + Tailwind CSS 4 + Vite
- **数据库**: Postgres (Neon)
- **部署**: Fly.io

### 修改代码时的惯例

1. **TypeScript 优先用 Bun**: 运行用 `bun run <file>`，包管理用 `bun install`
2. **Rust 用 Cargo**: `cargo build --release`, `cargo test`
3. **Web UI 改动后检查**: `cd web && bun run check` (Svelte type check)
4. **数据库 schema 变更**: 编辑 `src/schema.ts` 后运行 `bun run db:generate`
5. **环境变量**: 开发变量写在 `.env`（已 gitignored），模板在 `.env.example`

### 关键文件位置

| 功能 | 文件 |
|------|------|
| LLM Server 入口 | `src/index.ts` |
| 配置加载 | `src/config.ts` |
| Agent 池 | `src/agent-pool.ts` |
| 通道管理 | `src/channels/manager.ts` |
| 网关客户端 | `src/gateway/client.ts` |
| Bash 工具 | `src/tools/bash.ts` |
| Worker 工具 | `src/tools/worker.ts` |
| 内置命令 | `src/commands/builtin.ts` |
| Web UI 根 | `web/src/App.svelte` |
| Web UI 设置 | `web/src/lib/settings.svelte.ts` |
| Web UI 聊天视图 | `web/src/lib/views/chat-view.svelte` |
| 聊天客户端 | `packages/chat-client/src/client.ts` |
| 网关服务器 | `crates/nexal-gateway/src/server.rs` |
| 网关后端 | `crates/nexal-gateway/src/backend/` |
| 代理服务器 | `crates/nexal-agent/src/server.rs` |

### 部署注意事项

- **Gateway** 部署到 `nexal` Fly app (`fly.toml`)
- **LLM Server** 部署到 `nexal-server` Fly app (`deploy/server/fly.toml`)
- **自定义域名**: `gateway.nexal.nokiy.net` → `nexal.fly.dev`
- **Gateway 认证**: 通过 `NEXAL_GATEWAY_ACCESS_KEY` + `NEXAL_GATEWAY_SECRET_KEY` 的 HMAC-SHA256 签名
- **CI/CD**: `.github/workflows/deploy-gateway.yml` 和 `deploy-server.yml` 在 push 到 main 时自动部署

### 本地开发

```bash
# LLM Server (带热重载)
bun run dev

# 构建 Rust
cargo build --release

# Web UI
cd web && bun run dev    # http://localhost:5173

# TypeScript 类型检查
bun run typecheck

# Svelte 检查
cd web && bun run check
```

### Web UI 默认配置

- **默认后端 URL**: `wss://gateway.nexal.nokiy.net`
- **HTTP 端点推导**: `wss://` → `https://` (自动替换协议)
- **设置存储**: `localStorage`，前缀 `nexal.`

## 更多文档

- [`docs/structure.md`](./docs/structure.md) — 完整的目录结构、架构图、数据流
- [`crates/nexal-agent/README.md`](./crates/nexal-agent/README.md) — Agent JSON-RPC API 文档
- [`crates/README.md`](./crates/README.md) — Rust crate 分层架构说明
