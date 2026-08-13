# CTF|认证

# T-003 冻结任务目标

## 唯一任务目标

在 Codex 中完成一套以“任务”为中心的项目 AGENT 系统。项目 AGENT 是注册在当前项目中的专职执行工具（例如 `@query`），负责执行任务树中的具体任务，但不是 generic sub-agent，也不属于 `/root` 子级、并发槽位或临时协作 Agent 体系。

最终必须同时满足：

1. 主 AGENT 能识别当前项目注册的全部项目 AGENT。
2. 用户点名 `@query` 时，系统真正调用对应项目 AGENT，不能由主 AGENT 自行输出并伪称已转发。
3. 真实调用在主对话中可见，并带有 AGENT 名称、任务 ID、会话 ID、状态、原始要求和结果入口。
4. 每个任务绑定实际项目 AGENT 会话（worker thread/session）。
5. 从任务树可以打开该任务对应的独立会话窗口。
6. 独立会话显示完整聊天历史、工具调用、工具输出、错误、状态事件和最终结构化结果。
7. 用户能在独立会话输入框继续与同一项目 AGENT 交谈，后续消息复用原会话。
8. Codex 重启后任务树、会话历史和工具调用仍可恢复。
9. generic sub-agent 从主产品交互链路移除；旧 rollout 仅保留读取兼容。

## 产品边界

- 围绕任务建树，而不是围绕 Agent 建组织树。
- AGENT 是任务的执行者和工具，不是主 AGENT 的下级角色。
- 任务树负责导航；独立会话负责真实交互；`result/evidence/evaluation` 不能替代聊天记录。
- 所有真实调用必须有可追踪证据，禁止靠主模型口头声称“已经调用”。
- 不保留两套面向用户的 Agent 概念。

## 功能清单

### 1. 注册与主上下文

- 从 `AGENT/registry.toml` 加载启用的项目 AGENT。
- 将名称、职责、描述和调用入口注入主 AGENT 模型上下文，并明确项目 AGENT 不是 sub-agent。
- 询问当前 Agent 时列出 `@query` 等项目 AGENT，不回答 `/root` 或并发槽位。
- 注册变化可通过刷新机制进入现有会话；上下文片段有硬上限。

### 2. generic sub-agent 移除

- 主 AGENT 不再看到或调用 `spawn_agent`、`followup_task`、`send_message`、`wait_agent`、`interrupt_agent`、`list_agents` 等 generic collaboration 工具。
- 移除面向用户的 sub-agent picker、状态列表和“Sub-agents running”内容。
- `/agent` 只代表项目 AGENT 任务系统；内部 review/guardian 线程不出现在项目 AGENT UI。

### 3. 真实调用与可见证据

- 点名项目 AGENT 必须产生真实 `agent.<id>` 工具调用。
- 调用开始、排队、运行、工具调用、完成、拒绝、失败、阻塞均通过主对话事件可见。
- 完成结果提供“打开会话”入口，不能以普通最终文本替代调用卡片。

### 4. 任务树

- `/agent` 打开清晰层级树；支持根任务、子任务、追加要求、选择执行 AGENT、启动、终止、重试、继续、刷新。
- 节点保存 taskId、parentTaskId、标题、目标、要求、状态、agentId、executionTaskId、workerThreadId/sessionId、result、evaluation、时间戳。
- 树是导航入口，不得用右侧摘要冒充完整会话；未启动任务明确显示尚未启动。

### 5. 独立项目 AGENT 会话

- 每个已启动任务可打开真正独立会话窗口，顶部显示任务路径、标题、AGENT、状态、sessionId。
- 中间按时间顺序渲染用户要求、AGENT 回复、追加要求、工具调用/输出、错误、最终结果。
- 工具调用可折叠但不可隐藏；参数和输出有界并标注截断。
- 运行中实时刷新；底部输入框向同一 worker thread 发送后续消息；Esc 返回树且不丢状态。

### 6. 历史与持久化

- 以真实 Codex thread/rollout 为历史来源，不只读取 `current.json`。
- 保存稳定任务↔worker thread 绑定；历史读取有界/分页；重启恢复。
- 继续原会话与基于任务重试必须明确区分，不得静默新建替代会话。

### 7. App-server v2

- 新公共 API 只加 v2；优先复用 `thread/read`、`thread/resume`、`turn/start` 和通知。
- TUI 通过公开 API 读取任务树、会话绑定、历史，向同一会话发送后续消息并接收事件；不得直接读内部 JSON 伪造会话。
- 更新 README/schema；遵守 v2 camelCase、Params/Response/Notification 约定。

### 8. Windows 执行

- 注册命令工具在 Windows 使用支持的执行路径；不能出现 `sandboxed remote process launch is not supported on Windows`。
- 工具开始、输出、退出码和失败出现在独立会话；失败不得伪造 completed。

### 9. TUI 视觉

- 任务树简洁、层级清楚；完整聊天只在独立会话窗口显示。
- 用户消息、AGENT 消息、工具调用、错误有明确视觉区分；不能做成 debug 详情堆叠。
- 所有用户可见变化添加 insta 快照。

## 必须覆盖的测试与验收

必须覆盖注册进入主上下文、generic 工具隐藏、真实 `@query` 调用和主会话调用卡片、任务↔会话绑定、完整历史（用户/AGENT/工具调用/输出）、同会话 follow-up、重启恢复、多任务隔离、失败不伪造成功、Windows 工具执行、任务树操作、app-server v2 集成、TUI 会话快照和 Windows 构建。

最终验收流程：询问当前 Agent 得到 `@query`；要求 `@query` 回复“你好”并在主会话看到真实调用；打开任务独立会话看到完整历史和工具调用；在输入框追加消息且 sessionId 不变；执行注册工具并看到实时调用/输出；重启后恢复；创建子任务、追加要求、查看评测。

## 执行限制

- 这是一个完整一次性交付，禁止先交付任务树再让用户验证、再补聊天或工具调用。
- 不改变本任务目标，不把工作台验证当作中间交付。
- 按用户要求不运行本地测试、编译、Clippy、schema 生成或快照生成；统一通过 GitHub Actions 验证。
- 完成后才提供提交哈希、工作流地址、Windows 产物、SHA-256、启动命令和完整验收说明。
