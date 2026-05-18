# UI 架构修改计划

> 目标：所有用户输入只发给 Coordinator，不允许直接和子 Agent 对话。

## 当前问题

1. **输入框高度受限** — `composer.svelte` 的 textarea 最大高度限制在 240px，无法容纳长文本输入
2. **消息流混杂** — 用户消息、Coordinator 回复、系统消息、工具执行结果都在同一个列表里，没有层级区分
3. **缺乏 Coordinator 标识** — UI 没有明确告诉用户"你正在和 Coordinator 对话"，也没有阻止用户误以为可以直接和子 Agent 交互

## 目标架构

```
┌─────────────────────────────────────────────────────────┐
│ Header                                                  │
│  [New chat]  [Coordinator ▼]  [sandboxes] [sidebar]    │
├─────────────────────────────────────────────────────────┤
│                                                         │
│  ┌─────────────────────────────────────────────────┐   │
│  │ you                                               │   │
│  │ How do I deploy this?                             │   │
│  └─────────────────────────────────────────────────┘   │
│                                                         │
│  ┌─────────────────────────────────────────────────┐   │
│  │ coordinator                                       │   │
│  │ I'll help you deploy. Let me check the logs...    │   │
│  │                                                   │   │
│  │ ▶ bash: docker logs --tail 100 app              │   │
│  │   [stdout output...]                              │   │
│  │                                                   │   │
│  │ ▶ worker: spawn_worker analyze-logs             │   │
│  │   Status: running  |  Task: analyzing logs...    │   │
│  │   [Click to expand worker output]                │   │
│  └─────────────────────────────────────────────────┘   │
│                                                         │
│  ...                                                    │
│                                                         │
├─────────────────────────────────────────────────────────┤
│ ┌─────────────────────────────────────────────────────┐ │
│ │ Send to Coordinator  ┌──────────────────────────┐   │ │
│ │                      │  [growing textarea...]   │   │ │
│ │                      │  [no height limit]       │   │ │
│ │                      └──────────────────────────┘   │ │
│ │ [model] [mode]                        [voice] [↑]   │ │
│ └─────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────┘
```

## 修改清单

### 1. Composer — 无限高输入框 + Coordinator 标识

**文件**: `web/src/lib/components/composer.svelte`

- **移除高度限制**:
  ```svelte
  // 从
  textareaEl.style.height = Math.min(textareaEl.scrollHeight, 240) + "px";
  // 改为
  textareaEl.style.height = textareaEl.scrollHeight + "px";
  ```
  让 textarea 随内容无限增长。可以保留一个非常大的上限（如 600px）来防止极端情况，但不再限制正常输入。

- **添加 Coordinator 标识**:
  在 textarea 上方或 placeholder 中明确提示发送目标：
  - placeholder 改为: `"Message Coordinator · / for commands"`
  - 或在输入框左上角添加小标签 `"→ Coordinator"`

- **禁用发送目标切换**:
  确保没有 UI 元素允许用户切换发送目标（当前没有，但需要确保未来也不会添加）

### 2. Message — 区分 Coordinator / Tool / Worker

**文件**: `web/src/lib/components/message.svelte`

- **新增 `source` 属性**:
  ```typescript
  interface Props {
    role: "user" | "agent";
    source?: "coordinator" | "tool" | "worker";  // 新增
    text: string;
    ts: number;
    streaming?: boolean;
    toolCall?: { name: string; args: string };   // 工具调用信息
    workerTask?: { id: string; status: string }; // 子agent任务信息
  }
  ```

- **不同来源的展示样式**:
  - `coordinator`: 正常气泡，标头显示 "coordinator"（蓝色）
  - `tool`: 内联折叠卡片，标头显示工具名（如 "bash"），可展开查看输出
  - `worker`: 任务状态卡片，显示任务ID和状态（running/completed/failed），不可回复

- **当前后端还没有传递 source 信息**，需要后端配合修改协议。短期方案：
  - 系统消息中以特定前缀标识工具调用（如 `[tool:bash]`）
  - 前端正则匹配并渲染为工具卡片

### 3. ChatView — 重组消息布局

**文件**: `web/src/lib/views/chat-view.svelte`

- **消息流只保留两层**:
  1. User 消息
  2. Coordinator 消息（内部可包含工具/子agent执行记录）

- **工具/Worker 记录内联在 Coordinator 消息中**:
  Coordinator 回复不只是一段文本，而是一个"执行会话"，包含：
  - Coordinator 的思考/回复文本
  - 零个或多个工具调用卡片（bash, file_read 等）
  - 零个或多个 Worker 任务卡片

- **修改 `DisplayItem` 类型**:
  ```typescript
  type DisplayItem =
    | (Msg & { kind: "msg"; source?: "coordinator" })
    | { kind: "tool"; id: number; name: string; input: string; output?: string }
    | { kind: "worker"; id: number; taskId: string; status: string; output?: string }
    | { kind: "typing"; id: number };
  ```

- **移除 System 消息居中显示**:
  系统消息（connected/disconnected）改为固定在顶部的小横幅或 toast，不占用消息流空间

### 4. 后端协议配合（短期 vs 长期）

#### 短期方案（不改后端协议）

前端通过文本内容正则解析：
- 如果消息文本包含 `\[tool:(\w+)\] (.*)` → 渲染为工具卡片
- 如果消息文本包含 `\[worker:(\w+)\] status:(\w+)` → 渲染为 Worker 卡片
- 否则 → 正常 Coordinator 消息

#### 长期方案（需要后端修改）

后端在回复消息中添加元数据：
```json
{
  "type": "reply",
  "text": "I'll analyze the logs...",
  "metadata": {
    "source": "coordinator",
    "tool_calls": [
      { "name": "bash", "input": "docker logs app", "output": "..." }
    ],
    "worker_tasks": [
      { "id": "task-1", "status": "running" }
    ]
  }
}
```

### 5. Sidebar 简化

**文件**: `web/src/lib/components/sidebar.svelte`

- 移除 "New chat" 按钮（ChatView header 已有）
- 移除 "Threads" 区域（当前只有一个 default thread）
- 保留 "Sandboxes" 和 "Settings" 入口
- 添加一个小的 Coordinator 状态指示器（在线/离线）

### 6. EmptyState 更新

**文件**: `web/src/lib/components/empty-state.svelte`

- 提示文字改为 "Start a conversation with your Coordinator"
- 快捷建议示例改为 Coordinator 能处理的任务：
  - "Deploy my app"
  - "Fix the bug in src/main.ts"
  - "Run tests and report results"

## 文件改动清单

| 文件 | 改动内容 | 优先级 |
|------|----------|--------|
| `web/src/lib/components/composer.svelte` | 移除高度限制，添加 Coordinator 标识 | P0 |
| `web/src/lib/components/message.svelte` | 新增 source/toolCall/workerTask 属性 | P0 |
| `web/src/lib/views/chat-view.svelte` | 重组消息流，内联工具/Worker卡片 | P0 |
| `web/src/lib/components/sandbox-list.svelte` | 已存在，保持只读监控功能 | P1 |
| `web/src/lib/components/sidebar.svelte` | 简化，添加 Coordinator 状态 | P1 |
| `web/src/lib/components/empty-state.svelte` | 更新提示文案 | P2 |
| `packages/chat-client/src/protocol.ts` | 长期：扩展 ReplyMetadata | P2 |

## 实施顺序

1. **Step 1**: Composer 无限高 + Coordinator 标识（纯前端，无依赖）
2. **Step 2**: Message 组件区分 source（需要后端提供标识或前端正则解析）
3. **Step 3**: ChatView 重组布局，工具/Worker 内联
4. **Step 4**: Sidebar 简化和 EmptyState 更新
5. **Step 5** (可选): 后端协议扩展，传递 source/tool_calls/worker_tasks 元数据

## 验证清单

- [ ] 输入框可以输入超过 10 行文本且自动增长
- [ ] UI 明确显示 "发送至 Coordinator" 标识
- [ ] 没有 UI 元素允许切换发送目标
- [ ] Coordinator 消息显示为独立气泡
- [ ] 工具执行结果显示为可折叠卡片，不是独立对话参与方
- [ ] Worker 任务显示为状态卡片，不可回复
- [ ] SandboxList 仍然可以正常查看所有沙箱状态
