<!-- CTF|认证 -->

# Codex CLI 工具源码信息与扩展约束

## 目的

这份文档用于持续积累 Codex CLI 工具系统的源码入口、运行链路与扩展约束。当前阶段只做源码地图和设计基线，不提交上游，也不直接改变现有工具行为。

我们的扩展目标是：把新能力建立在现有工具系统内部，复用已有的注册、路由、权限、沙箱、事件和缓存机制，而不是另建一条互不兼容的执行路线。

## 1. CMD、PowerShell、curl 与文件查阅能力

### 1.1 第一性结论

Codex 并不是分别为 `curl`、`cmd.exe` 或 `Get-Content` 建立专用工具，而是向模型提供通用 Shell 工具，再由 Shell 工具调用当前平台可以执行的程序和命令。因此，文件查阅、`curl` 网络请求和 `cmd.exe /c ...` 都属于 Shell 工具的执行能力。

### 1.2 模型可见的工具说明

文件：`codex-rs/core/src/tools/handlers/shell_spec.rs`

`create_shell_command_tool()` 定义了 `shell_command` 的模型可见说明：

- `command` 是“在用户默认 Shell 中运行的 Shell 脚本”。
- Windows 版本明确说明该工具运行 PowerShell 命令并返回输出。
- 示例包含 `Get-ChildItem`、`Select-String`、环境变量和内联 Python。
- 参数包含 `workdir`、`timeout_ms`、登录 Shell 选项和权限申请参数。

这意味着 Windows 下可以直接执行：

```powershell
Get-Content -LiteralPath README.md
Select-String -Path .\src\*.rs -Pattern 'ToolRegistry'
curl.exe https://example.com
cmd.exe /c dir
```

`curl` 是否能够联网还会受到网络权限、沙箱和执行策略约束，但它作为可执行程序可以由 Shell 工具启动。

### 1.3 Shell 类型选择

文件：`codex-rs/tools/src/tool_config.rs`

关键函数：

- `shell_command_backend_for_features()`
- `unified_exec_feature_mode_for_features()`
- `shell_type_for_model_and_features()`

这些函数根据功能开关、模型声明、平台能力和终端支持情况，在以下执行表面之间选择：

- `shell_command`
- `exec_command` + `write_stdin`（Unified Exec）
- 禁用 Shell

文件：`codex-rs/core/src/shell.rs`

该模块负责检测和选择用户 Shell，包括 PowerShell、Bash、Zsh、Sh 和 Cmd 等类型。

### 1.4 CMD 与 curl 的源码证据

以下测试说明执行策略确实把它们当作 Shell 命令处理：

- `codex-rs/core/src/exec_policy_windows_tests.rs`
  - 使用 `cmd.exe /c dir` 验证 Windows 命令策略。
- `codex-rs/core/src/exec_policy_tests.rs`
  - 包含 `curl` 的 allow/forbidden 前缀规则和复合 Shell 命令测试。
- `codex-rs/core/src/tools/network_approval_tests.rs`
  - 使用 `curl https://example.com` 验证网络权限申请流程。
- `codex-rs/core/src/guardian/tests.rs`
  - 包含 `curl` 工具调用的审查与权限场景。

因此，不需要为了“可以执行 curl 或 cmd”增加新的基础执行器；若要改善体验，应在现有 Shell 工具、专用读取工具或 UI 展示层上扩展。

## 2. 当前工具系统的主要运行链路

```text
模型收到 ToolSpec
       ↓
模型返回 FunctionCall / CustomToolCall / ToolSearchCall
       ↓
ToolRouter::build_tool_call
       ↓
ToolRegistry 查找对应 CoreToolRuntime
       ↓
ToolExecutor::handle 执行
       ↓
ToolOutput 转换为模型输入与 UI/协议事件
```

### 2.1 工具规划与来源汇总

文件：`codex-rs/core/src/tools/spec_plan.rs`

关键入口：

- `build_tool_router()`
- `build_tool_specs_and_registry()`
- `add_tool_sources()`
- `build_model_visible_specs_and_registry()`

`PlannedTools` 同时收集：

- 本地运行时工具
- 模型托管工具规格
- MCP 工具
- Dynamic Tools
- Extension Tools
- Tool Search 延迟发现工具
- Code Mode 工具

最终由 `ToolRegistry::from_tools()` 建立执行注册表，并生成稳定的模型可见 `ToolSpec` 列表。

### 2.2 工具路由

文件：`codex-rs/core/src/tools/router.rs`

关键职责：

- 保存 `model_visible_specs`。
- 将模型响应转换成统一的 `ToolCall`。
- 创建 `ToolInvocation`。
- 把调用分发给 `ToolRegistry`。
- 查询工具是否支持并行调用。

### 2.3 工具注册与生命周期

文件：`codex-rs/core/src/tools/registry.rs`

关键类型：

- `ToolRegistry`
- `CoreToolRuntime`
- `ToolExecutor<ToolInvocation>`
- `ToolOutput`
- `ToolArgumentDiffConsumer`

现有注册层已经统一处理：

- 工具名称与重复注册检查
- 工具执行分发
- 调用前钩子
- 调用后钩子
- 输入重写
- 遥测标签
- 工具开始和结束事件
- 取消与并行能力
- 模型可见输出转换

新工具应接入这里，而不是绕过这里直接从 UI 或会话代码启动进程。

## 3. 缓存机制与不可破坏的约束

更完整的缓存原理、命中条件、失效条件和新功能设计检查表见：

`notes/cache-mechanism-design.md`

原生研究状态系统的当前设计见：

`notes/research-state-design.md`

用于防止上下文折叠后丢失完整目标的总纲：

`notes/research-state-master-plan.md`

这里需要区分三类相关机制：模型提示缓存、增量响应复用和本地工具搜索缓存。

### 3.1 模型提示缓存

文件：`codex-rs/core/src/client.rs`

`ModelClientSession` 维护稳定的 `prompt_cache_key`。请求使用该键帮助服务端复用相同前缀的提示内容，并通过返回的 `cached_input_tokens` 统计缓存命中量。

扩展工具时应保持：

1. 工具名称稳定。
2. 工具描述稳定，不要注入时间、随机值或每轮变化的数据。
3. JSON Schema 的字段和顺序保持确定性。
4. 工具列表顺序保持确定性。
5. 不要每轮无条件添加和删除模型可见工具。

工具规格属于模型请求的一部分。工具列表、工具描述或 Schema 频繁变化，会改变请求前缀并降低提示缓存命中率。

### 3.2 `previous_response_id` 增量复用

文件：`codex-rs/core/src/client.rs`

客户端会判断前后请求是否兼容，兼容时可以复用连接和 `previous_response_id`。兼容性比较包含：

- 模型
- 基础指令
- 工具列表
- 工具选择策略
- 并行工具调用配置
- reasoning 配置
- service tier
- `prompt_cache_key`
- text 配置

因此，工具列表或 `ToolSpec` 在会话过程中发生变化，不只是影响提示缓存，也可能使增量响应复用失效。

设计约束：

- 新工具优先在会话开始时稳定注册。
- 如果必须按需加载，复用现有 Deferred Tool / Tool Search 机制。
- 不通过改写历史来“补”工具信息。
- 不无条件调用 `reset_client_session`。

### 3.3 Tool Search 本地缓存

文件：`codex-rs/core/src/tools/handlers/tool_search.rs`

`ToolSearchHandlerCache` 缓存已经建立的 `ToolSearchHandler`。当新的 `search_infos` 与缓存内容完全相等时，直接复用现有搜索处理器；只有工具搜索信息改变时才重建 BM25 搜索索引。

设计约束：

- `ToolSearchInfo` 必须可确定地生成。
- 不在搜索文本中加入每轮变化的内容。
- 新工具如果适合延迟发现，应进入现有 Tool Search，而不是建立第二套搜索缓存。

### 3.4 上下文缓存友好原则

根目录 `AGENTS.md` 已明确要求：

- 上下文必须增量构建，不重写历史。
- 避免频繁改变上下文导致缓存未命中。
- 注入内容必须有明确上限。
- 单个注入项不得超过 10K tokens。
- 注入到模型上下文的片段应使用 `core/context` 中的结构，并实现 `ContextualUserFragment`。

未来的读取工具尤其需要遵守这些规则：读取结果必须分页、截断并带有硬上限，不能把任意大文件无边界地塞进模型上下文。

## 4. 我们的工具应该如何建立在现有体系中

### 4.1 推荐结构

一个原生工具至少包含：

```text
handlers/<tool>.rs           执行逻辑
handlers/<tool>_spec.rs      ToolSpec 与 JSON Schema
handlers/<tool>_tests.rs     独立测试文件
```

然后：

1. 在 `handlers/mod.rs` 内部导出处理器。
2. 在 `spec_plan.rs` 的现有工具来源中注册。
3. 复用 `CoreToolRuntime`、`ToolExecutor` 和 `ToolOutput`。
4. 复用权限、沙箱、事件、截断和遥测机制。
5. 如需延迟发现，接入 Tool Search。
6. 如需外部独立服务，优先使用 MCP。

### 4.2 读取工具的初步定位

当前没有独立的原生文本 `read_file` handler；文本文件通常通过 Shell 命令读取，MCP 也可以提供 `read_file`。

若建立原生读取工具，第一版应只解决 Shell 读取不够结构化的问题：

- 明确路径
- 起止行或 offset/limit
- 编码信息
- 文件元数据
- 行号
- 硬性最大输出
- 截断标记
- 沙箱路径检查
- 稳定的结构化输出

不应在第一版同时实现索引器、语义搜索、缓存系统和 UI 大改。搜索应优先复用现有文件搜索或 Tool Search，缓存应复用现有会话和上下文机制。

### 4.3 保持缓存友好的读取结果

读取内容本身属于新的工具结果，文件变化时结果变化是正常的；需要避免的是工具定义和历史前缀无意义变化。

建议：

- `ToolSpec` 固定。
- 参数 Schema 固定。
- 输出格式固定。
- 每次输出有字节数和 token 近似上限。
- 大文件必须按范围读取。
- 返回 `truncated`、`next_offset` 或下一段行号。
- 不重复注入已经存在的工具说明。
- 不为了显示 UI 状态而修改模型上下文；UI 状态使用协议事件。

## 5. 现有主要工具源码索引

| 能力 | 主要源码 |
| --- | --- |
| Shell / Unified Exec | `codex-rs/core/src/tools/handlers/shell.rs`、`shell_spec.rs`、`unified_exec.rs` |
| Shell 运行时 | `codex-rs/core/src/tools/runtimes/shell/` |
| Apply Patch | `codex-rs/core/src/tools/handlers/apply_patch.rs`、`apply_patch_spec.rs` |
| 图片读取 | `codex-rs/core/src/tools/handlers/view_image.rs`、`view_image_spec.rs` |
| MCP 工具 | `codex-rs/core/src/tools/handlers/mcp.rs` |
| MCP 资源 | `codex-rs/core/src/tools/handlers/mcp_resource.rs` |
| Tool Search | `codex-rs/core/src/tools/handlers/tool_search.rs`、`tool_search_spec.rs` |
| Dynamic Tools | `codex-rs/core/src/tools/handlers/dynamic.rs` |
| Extension Tools | `codex-rs/core/src/tools/handlers/extension_tools.rs` |
| 权限申请 | `codex-rs/core/src/tools/handlers/request_permissions.rs` |
| 用户输入 | `codex-rs/core/src/tools/handlers/request_user_input.rs` |
| 多智能体工具 | `codex-rs/core/src/tools/handlers/multi_agents.rs`、`multi_agents_v2.rs` |
| 工具规划 | `codex-rs/core/src/tools/spec_plan.rs` |
| 工具注册 | `codex-rs/core/src/tools/registry.rs` |
| 工具路由 | `codex-rs/core/src/tools/router.rs` |
| 工具事件 | `codex-rs/core/src/tools/events.rs`、`lifecycle.rs` |
| 工具上下文与输出 | `codex-rs/core/src/tools/context.rs` |
| 提示缓存与请求复用 | `codex-rs/core/src/client.rs`、`client_common.rs` |

## 6. 后续调查清单

- [ ] 追踪 `shell_command` 从 ToolSpec 到具体进程创建的完整调用路径。
- [ ] 追踪 Windows PowerShell 与 Cmd 的最终选择和参数包装方式。
- [ ] 记录 Shell 文件读取输出的截断规则。
- [ ] 调查已有 `file-search` crate 是否适合作为读取工具的辅助能力。
- [ ] 明确读取工具应该放入现有 crate，还是建立独立的小型 crate，避免继续扩大 `codex-core`。
- [ ] 设计稳定的 `read_file` ToolSpec，但暂不实现。
- [ ] 调查工具生命周期事件到 TUI `history_cell` 的渲染链路。

## 7. 当前阶段结论

1. Codex 已经可以通过 PowerShell、Cmd 和可执行程序执行文件查阅与 `curl`；能力来源是通用 Shell 工具。
2. 新能力必须保持工具规格、顺序和上下文前缀稳定，避免破坏提示缓存和 `previous_response_id` 复用。
3. 新工具应进入 `spec_plan → ToolRegistry → ToolRouter → CoreToolRuntime` 的现有链路。
4. 读取工具值得增加，但应先做稳定、分页、有上限的结构化读取，不重新发明 Shell、搜索或缓存系统。
