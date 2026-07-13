<!-- CTF|认证 -->

# Codex CLI 缓存机制原理与新功能设计基线

## 1. 文档目标

本文不把“缓存”当成一个单独模块，而是按数据流拆解 Codex CLI 中与复用有关的机制。目的是在增加读取工具、搜索工具、UI 效果或其他工作流能力时，明确哪些数据必须稳定、哪些变化会导致缓存失效，以及哪些状态不能被错误复用。

第一性结论：缓存命中依赖“可复用的稳定前缀与稳定身份”，而不是简单依赖某个布尔开关。新功能如果持续改变请求前缀、工具定义或历史内容，即使 `prompt_cache_key` 不变，也无法保证缓存命中。

## 2. 缓存分层总览

Codex 当前至少存在以下几类与本次设计相关的复用机制：

| 层级 | 复用对象 | 主要键或条件 | 生命周期 | 主要源码 |
| --- | --- | --- | --- | --- |
| 服务端提示缓存 | 模型请求中的稳定提示前缀 | `prompt_cache_key` + 实际请求前缀 | 由服务端决定，客户端按线程提供稳定键 | `core/src/client.rs` |
| Responses 增量复用 | 已提交的请求输入和上一次响应 | 请求属性相等、输入严格延伸、`previous_response_id` 可用 | WebSocket 会话状态 | `core/src/client.rs` |
| WebSocket 连接复用 | Responses WebSocket 连接及相关状态 | 同一 `ModelClient`，连接健康，未切换 HTTP fallback | Codex 会话级 | `core/src/client.rs` |
| Tool Search 缓存 | BM25 工具搜索处理器和索引 | `search_infos` 完全相等 | `SessionServices` 会话级 | `tools/handlers/tool_search.rs` |
| 上下文基线复用 | 设置与 world state 的差量基线 | 历史未被替换、基线仍有效 | 线程历史级 | `context_manager/history.rs` |
| 本地图像估算 LRU | 原始图片的 token 成本估算 | 图片 data URL 的 SHA-1 | 进程级、最多 32 项 | `context_manager/history.rs` |

不是所有缓存都直接提高模型提示缓存命中率。它们分别减少网络传输、服务端前缀计算、本地索引构建、上下文重复注入或图片重复解码。

## 3. 服务端提示缓存

### 3.1 客户端实际做了什么

文件：`codex-rs/core/src/client.rs`

`ModelClient::prompt_cache_key()` 的逻辑是：

```text
存在 prompt_cache_key_override
    → 使用 override
否则
    → 使用 thread_id 的字符串
```

在 `build_responses_request()` 中，该值被放入 `ResponsesApiRequest.prompt_cache_key`。普通 Responses 请求和 Compact 请求都会传递这个键。

客户端没有在本地保存模型推理结果，也没有在本地根据该键返回旧答案。它只是给服务端提供一个稳定的缓存归属键。真正的前缀缓存建立、匹配、淘汰和命中由服务端完成。

### 3.2 `prompt_cache_key` 不是什么

它不是：

- 请求正文的内容哈希。
- “只要键相同就必然命中”的强制开关。
- 工具结果缓存。
- 对话历史数据库主键。
- 本地响应缓存。

更准确的理解是：它给服务端提供稳定的缓存分组或亲和标识；在这个标识下，实际请求仍需拥有可复用的相同前缀。

### 3.3 如何观察是否命中

服务端 usage 中的 `cached_input_tokens` 表示缓存输入 token。相关类型位于：

- `codex-rs/protocol/src/protocol.rs`
- `codex-rs/core/src/client.rs`

代码还会区分：

- `cached_input()`
- `non_cached_input()`

因此，判断新功能是否伤害缓存，应该比较相同任务条件下的 `cached_input_tokens` 和 `non_cached_input_tokens`，而不是只检查 `prompt_cache_key` 是否存在。

### 3.4 提示前缀由什么组成

文件：`codex-rs/core/src/client_common.rs`

`Prompt` 包含：

- `input`：对话历史和工具结果。
- `tools`：模型可见工具，包括 MCP 等外部来源。
- `parallel_tool_calls`。
- `base_instructions`。
- 输出 Schema 配置。

文件：`codex-rs/core/src/client.rs`

最终请求还包含：

- 模型名称。
- instructions。
- 工具 JSON。
- tool choice。
- reasoning 配置。
- service tier。
- text/verbosity/输出格式配置。

这些内容发生变化时，可复用前缀可能缩短或完全消失。

### 3.5 Responses Lite 的差异

在普通 Responses 模式中，工具通常位于请求的 `tools` 字段，基础指令位于 `instructions`。

在 Responses Lite 模式中：

- 工具被转换为 `ResponseItem::AdditionalTools`。
- 基础指令被转换为 developer message。
- 二者被插入 `input` 的最前面。
- 请求顶层 `tools` 变为 `None`，`instructions` 变为空字符串。

因此，工具定义的变化在 Responses Lite 中会直接改变输入历史前缀；设计新工具时仍然必须保持工具定义稳定。

## 4. 工具定义为何会影响缓存

### 4.1 工具列表进入模型请求

文件：

- `codex-rs/core/src/tools/spec_plan.rs`
- `codex-rs/tools/src/tool_spec.rs`

工具构建流程为：

```text
各类 Tool Runtime / Hosted Spec
        ↓
PlannedTools
        ↓
生成 model_visible_specs
        ↓
create_tools_json_for_responses_api()
        ↓
ResponsesApiRequest
```

`create_tools_json_for_responses_api()` 会按照收到的切片顺序逐个序列化工具，不会对顶层工具列表做一次全局排序。

`merge_into_namespaces()` 会：

- 按首次出现的位置保留 namespace 的顶层位置。
- 合并同名 namespace。
- 对 namespace 内的 function 按名称排序。

所以顶层注册顺序仍然是缓存稳定性的一部分。

### 4.2 会导致工具前缀变化的因素

- 新增、删除或重命名模型可见工具。
- 修改工具描述。
- 修改 JSON Schema。
- 修改 strict、namespace、defer loading 等属性。
- 改变工具注册顺序。
- 功能开关使工具每轮出现或消失。
- MCP server 动态改变工具列表或工具说明。
- Dynamic Tool / Extension Tool 集合变化。
- 环境数量变化导致 Schema 增加或移除 `environment_id`。
- Code Mode 改变工具暴露方式或规格包装。
- Web Search 模式变化。

### 4.3 稳定工具定义的设计原则

1. Schema 使用确定性容器；已有工具 Schema 广泛使用 `BTreeMap`。
2. 描述文本不能包含时间、随机数、当前目录扫描结果或会话统计。
3. 顶层工具注册顺序固定。
4. 能用参数表达的差异，不通过每轮重建不同 Schema 表达。
5. 大型或低频工具优先通过 Deferred Tool / Tool Search 延迟发现。
6. 工具能力变化应有明确的会话边界，不在单个会话中无规律抖动。

## 5. Responses WebSocket 增量复用

提示缓存解决的是服务端稳定前缀复用；WebSocket 增量复用解决的是客户端不重复发送已经提交过的输入。这是两个相关但不同的机制。

### 5.1 保存的状态

`WebsocketSession` 保存：

- 当前连接。
- `last_request`：上一个完整请求。
- `last_response_rx`：上一个响应完成后得到的 response id 和新增 items。
- 上一次响应是否来自无 trace 的 warmup。
- 连接是否已复用。

`ModelClientState` 中还保存一个 `cached_websocket_session`。创建新的 `ModelClientSession` 时取出，`ModelClientSession::drop()` 时再放回，因此健康连接可以跨 turn 继续使用；每个 turn 仍会创建新的 `turn_state`，避免把 sticky-routing token 错带到下一轮。

### 5.2 请求属性相等条件

`responses_request_properties_match()` 要求以下字段相等：

- model
- instructions
- tools
- tool_choice
- parallel_tool_calls
- reasoning
- store
- stream
- include
- service_tier
- prompt_cache_key
- text

该比较明确忽略：

- `input`：由后续的前缀算法单独比较。
- `client_metadata`：不属于模型上下文。
- `stream_options`：只控制本次响应的交付方式，不属于 `previous_response_id` 指向的上下文。

### 5.3 输入严格延伸算法

`get_incremental_items()` 的核心过程：

1. 取上一个完整 request 的 `input`。
2. 追加服务端上一次响应新增的 items。
3. 清理不影响模型语义的内部 chat metadata。
4. 取当前 request 相同长度的前缀。
5. 比较二者是否完全相等。
6. 如果相等，当前 request 剩余的 items 就是增量 delta。
7. 如果不相等，发送完整 request。

随后 `prepare_websocket_request()` 将上一次 `response_id` 放入 `previous_response_id`，只发送增量 items。

### 5.4 增量复用的失效条件

- 非 input 请求属性发生变化。
- 当前历史比旧基线短。
- 历史中间内容被改写。
- 工具结果被重新格式化。
- compaction 或 rollback 替换了历史。
- 上一次 response id 为空或不可用。
- 连接发生错误并被 reset。
- 会话切换到 HTTP fallback。

### 5.5 对新功能的直接要求

- 历史只追加，不修改旧 item。
- UI 动画、进度和折叠状态不能写进模型历史。
- 工具输出一旦写入历史，不要在后续轮次重新格式化。
- 如果需要补充工具状态，追加一个有界的新事件或新 item，而不是修改旧输出。
- 工具调用与输出配对必须保持稳定。

## 6. WebSocket 预热与连接复用

文件：`codex-rs/core/src/client.rs`

### 6.1 预连接

`preconnect_websocket()` 只建立连接，不发送 prompt。

### 6.2 预热

`prewarm_websocket()` 可以发送 `generate=false` 的 v2 `response.create`，等待完成后，让正式请求复用相同连接和 `previous_response_id`。

### 6.3 失败与回退

WebSocket 不可用或遇到特定错误时会切换到 HTTP。`force_http_fallback()` 会：

- 设置会话级 `disable_websockets`。
- 清空缓存的 WebSocket session。
- 后续 turn 继续使用 HTTP。

这说明连接缓存必须允许失效，不能为了追求复用而继续使用已不可信的连接状态。

## 7. Tool Search 缓存

文件：

- `codex-rs/core/src/tools/handlers/tool_search.rs`
- `codex-rs/core/src/state/service.rs`

`ToolSearchHandlerCache` 位于 `SessionServices`，因此是会话级共享对象。

### 7.1 命中算法

`get_or_build(search_infos)`：

1. 锁定缓存。
2. 如果已有 handler 且 `cached.search_infos == search_infos`，直接克隆 `Arc` 返回。
3. 否则在锁外构建新的 BM25 handler。
4. 再次锁定，防止并发期间另一个调用已经构建同样内容。
5. 如果缓存现在已经相等，复用缓存。
6. 否则替换缓存。

这是一个带 double-check 的单项缓存，不是多版本 LRU。

### 7.2 缓存内容

- 工具搜索规格。
- 搜索来源信息。
- `search_infos`。
- 基于搜索文本构建的 BM25 索引。

### 7.3 失效条件

只要 `search_infos` 不完全相等就重建。工具名称、Schema、搜索文本、来源描述等变化都可能导致失效。

### 7.4 新工具接入原则

- 搜索文本必须稳定且语义明确。
- 不加入当前时间和动态统计。
- 工具集合变化后允许重建，但不要每轮制造无意义差异。
- 不另建第二套工具发现缓存。

## 8. 上下文基线与差量注入

文件：`codex-rs/core/src/context_manager/history.rs`

`ContextManager` 保存：

- 按时间排序的 `ResponseItem` 历史。
- `history_version`。
- token usage 信息。
- `reference_context_item`。
- `world_state_baseline`。

### 8.1 追加与改写的区别

`record_items()` 处理并追加历史，不增加 `history_version`。

`replace()` 用新列表替换历史，并增加 `history_version`。典型场景包括：

- compaction
- rollback
- rollout reconstruction 中的替换基线

这说明历史改写是显式的高影响行为，不应该成为普通工具更新状态的方法。

### 8.2 World State 差量

`update_world_state()` 会把当前 snapshot 与 `world_state_baseline` 比较：

- 没有基线时产生 full item。
- 有基线时产生 merge patch。

历史被替换或关键项被移除时，基线会清空，下一次重新注入完整状态，避免对过期基线计算差量。

### 8.3 工具输出截断

工具输出在写入上下文时会经过统一截断逻辑，主要位于：

- `codex-rs/core/src/tools/context.rs`
- `codex-rs/utils/output-truncation/src/lib.rs`
- `codex-rs/core/src/context_manager/history.rs`

支持按 bytes 或 tokens 限制，并保留明确的截断提示。新读取工具必须复用这一层，或者采用与其兼容的 `TruncationPolicy`，不能把无上限文件内容直接写入历史。

## 9. 本地图像估算 LRU

`ContextManager` 在估算原始图片 token 成本时，会：

1. 对图片 data URL 计算 SHA-1。
2. 查询容量为 32 的 `BlockingLruCache`。
3. 未命中时解码图片并计算 32px patch 数。
4. 缓存估算结果，包括失败时的 `None`。

这个缓存避免重复解码相同内联图片。它不是模型图片结果缓存，只用于本地 token 成本估算。

对未来工具的启发：如果需要缓存纯函数式的昂贵本地计算，应使用内容寻址键、明确容量和可安全重算的值；不要把会话可变状态塞进全局 LRU。

## 10. Compaction 与缓存的关系

Compaction 的首要目标是控制上下文长度，而不是保持旧前缀缓存。

当历史过长时，系统会用压缩后的 replacement history 替换旧历史：

- `history_version` 增加。
- WebSocket 输入不再是旧历史的严格延伸，增量复用可能失效。
- 服务端提示前缀也会变化，旧前缀缓存可能不能继续完整复用。
- 后续请求会围绕新的压缩历史形成新的稳定前缀。

所以“永远不破坏缓存”不是正确目标。正确目标是：普通功能保持稳定追加；只有 compaction、rollback、模型切换、工具配置变化等明确边界才允许主动失效。

## 11. 新功能设计的缓存风险分级

### 低风险

- 只改变 TUI 渲染，不改变协议和模型历史。
- 在已有工具结果中使用已有截断策略。
- 新增本地纯计算缓存，键为内容哈希，容量有界。
- 使用已有工具生命周期事件展示进度。

### 中风险

- 新增一个会话开始时稳定注册的工具。
- 给工具输出增加稳定字段。
- 新增 Deferred Tool 或 Tool Search entry。
- 增加有界的 contextual fragment。

需要评估工具列表变化和上下文 token 增量。

### 高风险

- 每轮动态改变工具描述或 Schema。
- 根据工作区扫描结果生成不同的模型可见工具列表。
- 修改已经写入历史的工具输出。
- 在 UI 状态变化时重写模型上下文。
- 无上限地注入文件、索引或日志。
- 不必要地 reset client session。
- 绕过 `ToolRegistry` 建立另一套执行和结果记录链路。

## 12. 原生读取工具的缓存友好设计

### 12.1 稳定 ToolSpec

建议固定参数：

```json
{
  "path": "string",
  "start_line": "integer|null",
  "end_line": "integer|null",
  "offset": "integer|null",
  "limit": "integer|null",
  "include_line_numbers": "boolean"
}
```

不要根据文件类型、文件大小或当前目录动态生成不同 Schema。

### 12.2 有界结果

建议固定输出元数据：

```json
{
  "path": "string",
  "content": "string",
  "encoding": "string",
  "total_lines": "integer|null",
  "returned_start_line": "integer|null",
  "returned_end_line": "integer|null",
  "truncated": "boolean",
  "next_start_line": "integer|null"
}
```

具体文件内容变化是业务数据变化，无法也不应该伪装成相同结果；但工具定义和结果外壳应保持稳定。

### 12.3 不把 UI 状态写进模型历史

以下状态只进入 TUI 事件：

- 正在读取。
- 进度百分比。
- 动画帧。
- 展开/折叠。
- 卡片颜色。
- 用户是否打开详情。

模型历史只保存最终、有界、结构化的读取结果。这样 UI 可以丰富变化，而请求前缀不会因为动画或本地交互抖动。

### 12.4 是否需要文件内容缓存

第一版不建议新增文件内容缓存。操作系统文件缓存已经会降低重复读取成本，而自建缓存还要解决：

- 文件修改失效。
- 符号链接和真实路径。
- 编码变化。
- 权限变化。
- 工作树切换。
- 内存上限。

如果性能数据证明需要，再考虑以 `(canonical_path, file_id, modified_time, size)` 或内容哈希为键的有界缓存，并确保权限检查发生在缓存返回之前。

## 13. 新功能设计检查表

### 模型请求

- [ ] 是否改变基础 instructions？
- [ ] 是否改变顶层工具列表或顺序？
- [ ] 工具描述与 Schema 是否确定？
- [ ] Responses Lite 下是否会改变 input 前缀？
- [ ] 是否改变 model、reasoning、service tier 或 text 配置？

### 历史

- [ ] 功能是否只追加历史？
- [ ] 是否可能修改旧工具输出？
- [ ] 输出是否有 bytes/token 硬上限？
- [ ] function call 与 output 是否保持配对？
- [ ] compaction/rollback 后是否会错误使用旧基线？

### 工具发现

- [ ] 是否可以复用 Deferred Tool / Tool Search？
- [ ] `ToolSearchInfo` 是否稳定？
- [ ] MCP 工具列表是否可能每轮抖动？

### 本地缓存

- [ ] 缓存值是否可以安全重算？
- [ ] 键是否包含所有影响结果的输入？
- [ ] 是否有容量或 TTL 上限？
- [ ] 权限检查是否在缓存命中后仍然有效？
- [ ] 是否需要跨会话缓存，还是会话级已经足够？

### UI

- [ ] UI 状态是否与模型上下文隔离？
- [ ] 是否只消费工具生命周期或协议事件？
- [ ] 是否会为了重绘修改历史？

## 14. 设计基线总结

1. `prompt_cache_key` 是稳定缓存归属键，不是内容哈希，也不保证命中。
2. 真正决定提示缓存效果的是请求前缀是否稳定。
3. 工具定义属于请求前缀；名称、描述、Schema 和顺序都要稳定。
4. WebSocket 增量复用要求请求属性不变、历史严格追加并拥有有效的 `previous_response_id`。
5. Tool Search 只在 `search_infos` 完全相等时复用 BM25 handler。
6. 普通工具状态应追加，不应重写历史；compaction 和 rollback 是有意识的失效边界。
7. 工具输出必须有界，UI 临时状态必须留在 UI/事件层。
8. 新功能不应追求“缓存永不失效”，而应追求“稳定状态下高命中，语义变化时正确失效”。

