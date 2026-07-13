<!-- CTF|认证 -->

# Codex 原生研究状态系统：总体目标与完整实施方案

## 0. 文档地位

本文是本项目的目标登记与实施总纲，用于在会话压缩、上下文折叠、开发中断或人员切换后恢复完整方向。

任何后续实现都应回答：

1. 是否推进了本文定义的最终目标？
2. 是否保持了作用域、证据和缓存边界？
3. 是否只是增加了记录，而没有提高研究能力？
4. 是否把请求者偏好误当成产品真相？
5. 是否给未来阶段留下稳定的扩展边界？

若实现与本文冲突，应先更新本文并记录原因，再修改代码。

## 1. 最终目标

在原生 Codex 中建立一个可持续演化的研究思考状态，使模型在请求者输入有限、知识不完整或只描述表面方案时，仍能够：

- 主动理解项目，而不是只围绕用户提到的文件做补丁。
- 区分请求者要求、产品目标、候选方案、主观反馈和客观证据。
- 识别重要未知项，并区分哪些可以自己研究、哪些必须询问。
- 建立、验证、否定和替代研究假设。
- 识别重复变化背后的可配置维度和缺失抽象。
- 考虑当前修改对项目未来多个会话的结构压力。
- 在任务结束后更新项目认知和研究方法。
- 让下一次任务从更高的研究起点开始，而不是重新发现同样的问题。

系统追求的不是记住答案，而是提高：

```text
问题建模能力
主动探索能力
变量与不变量识别能力
抽象时机判断能力
产品影响判断能力
长期工程判断能力
验证与反证能力
下一次研究起点质量
```

## 2. 核心闭环

```text
Observe
  ↓
Frame
  ↓
Hypothesize
  ↓
Explore
  ↓
Decide
  ↓
Execute
  ↓
Evaluate
  ↓
Distill
  ↓
Reframe
```

中文表达：

```text
观察 → 建模 → 假设 → 探索 → 决策 → 执行 → 验证 → 提炼 → 更新研究起点
```

闭环的产物不是“上次做了什么”的流水账，而是：

- 更准确的项目模型。
- 更清楚的不确定性边界。
- 更有效的探索路径。
- 更可靠的条件性决策。
- 对未来结构压力的更早识别。
- 经证据支持的研究方法。

## 3. 系统不是什么

本系统不是：

- 用户偏好放大器。
- 自动迎合请求者的个性化系统。
- UI/UX 专用属性系统。
- 任务原始对话归档。
- 固定答案知识库。
- 每次都要求用户补充大量信息的问卷工具。
- 绕过现有 Codex 工具系统的第二套执行框架。
- 自动把一次任务结论提升成全局规则的机制。
- 替代模型推理的静态规则引擎。

## 4. 请求者输入的地位

请求者输入是：

- 研究起点。
- 约束来源。
- 方案来源。
- 决策权来源。
- 反馈来源之一。

请求者输入不是自动成立的产品真相。

系统必须区分：

```text
硬约束
产品目标
请求者提出的方案
个人倾向
观察反馈
项目事实
模型推断
测试证据
分析证据
其他相关对象反馈
```

“有权决定”和“有证据支持”必须作为不同维度保存。

## 5. 产品与项目影响模型

产品不是只服务请求者本人。研究状态后续应能够表达：

- 最终用户。
- 新用户与熟练用户。
- 管理员、运营、客服和维护人员。
- 开发者和后续接手者。
- 外部集成者。
- 当前受影响模块。
- 相邻功能和共享组件。
- 短期收益。
- 长期副作用。
- 可逆性和迁移成本。

最终知识单元应逐渐从：

```text
条件 + 动作 → 结果
```

升级为：

```text
目标 + 受影响对象 + 条件 + 动作 → 结果 + 外部影响 + 证据
```

## 6. 作用域模型

### 6.1 Task

仅适用于当前任务：

- 当前任务事实。
- 当前限制。
- 当前未知项。
- 当前假设。
- 当前探索计划。
- 当前决策和结果。

### 6.2 Project

仅适用于当前项目：

- 项目架构事实。
- 项目设计原则。
- 已知技术约束。
- 反复出现的变化维度。
- 尚未达到重构时机的结构压力。
- 已确认的项目发展方向。

### 6.3 Stakeholder

后续阶段使用。记录角色、场景、目标、影响和证据来源，不记录“某个人喜欢什么”作为产品结论。

### 6.4 Domain

后续阶段使用。保存只在特定领域内成立的研究模式，例如 UI、学习、后端架构、内容生产或运维。

### 6.5 Global

后续阶段使用。只保存跨领域仍成立的研究方法，例如：

- 相反要求反复出现时寻找隐藏变量。
- 相同补丁反复出现时寻找缺失抽象。
- 用户形容词是观察信号，不是最终原因。
- 优先主动研究项目中可发现的信息，再决定是否询问。

## 7. 知识提升边界

知识提升路径：

```text
任务观察
  ↓
项目假设
  ↓
项目证据支持
  ↓
领域候选
  ↓
跨项目重复验证
  ↓
全局研究原则
```

禁止：

- 一次任务直接写入全局规则。
- 单个请求者偏好直接成为领域知识。
- 没有结果证据的模型判断直接标记为已验证。
- 忽略冲突样本，只保留正向案例。

## 8. 原生架构定位

模型可见工具保持精简：

```text
update_plan
```

内部能力：

```text
update_plan
    ↓
PlanHandler
    ├── 原有 Todo/Plan 更新
    └── 可选 research_delta
              ↓
       ResearchState reducer
              ↓
       Session / Project State
              ↓
       Persistence
              ↓
       Bounded Context Projection
              ↓
       TUI / app-server projection
```

`update_plan` 是当前行动投影，不是完整研究数据库。

## 9. 数据模型

### 9.1 ResearchEntryKind

第一阶段：

```text
fact
constraint
goal
unknown
hypothesis
exploration
decision
future_pressure
outcome
```

后续可能增加：

```text
stakeholder
observation
proposal
tradeoff
externality
evidence
research_method
```

### 9.2 ResearchStatus

```text
open
supported
rejected
resolved
superseded
```

### 9.3 ResearchOperation

```text
upsert
set_status
remove
```

后续可增加独立的 evidence 操作，但不应在第一阶段过早扩张 Schema。

### 9.4 Revision

- 成功改变状态后递增。
- 幂等重复增量不递增。
- 无效批次不部分提交。
- 后续用于投影缓存键和持久化并发控制。

## 10. 缓存与上下文原则

### 10.1 ToolSpec

- 只进行稳定、版本化的 Schema 扩展。
- 不根据任务领域动态改变字段。
- 不动态增删模型可见工具。
- 使用确定性字段和枚举顺序。

### 10.2 模型历史

- research state 通过 delta 追加。
- 不改写旧工具调用和旧工具输出。
- 不重复提交完整研究状态。
- UI 状态不进入模型历史。

### 10.3 Context Projection

后续只注入当前任务高度相关的研究投影：

- 项目事实最多 5 条。
- 关键未知项最多 3 条。
- 假设最多 3 条。
- 未来压力最多 2 条。
- 领域方法最多 2 条。
- 全局方法最多 2 条。
- 初始目标建议不超过约 800 tokens。

### 10.4 Projection Cache

预期缓存键：

```text
project_id
+ project_revision
+ task_signature
+ stakeholder/domain/global revision
+ projection_schema_version
```

状态变化只失效相关作用域投影，不全局清空缓存。

## 11. 持久化方案

第一阶段仅会话内状态。

后续使用 `codex-state` 保存：

```text
research_events
research_projections
```

事件表保存 append-only 变化，投影表保存当前可快速读取状态。

项目身份初步由以下信息生成：

```text
canonical workspace root
+ optional git remote identity
+ repository fingerprint
```

必须处理项目移动、无 Git 工作区和多个 worktree 的边界。

## 12. UI 方案

原有 PlanUpdateCell 保持 Todo 投影。

后续独立增加 Research State UI：

```text
• Research State
  ├ Fact        ThemeProvider already exists
  ├ Unknown     Runtime theme switching requirement
  ├ Hypothesis  Theme is becoming a reusable project dimension
  └ Pressure    Hard-coded colors are spreading
```

UI 只消费状态投影和事件，不为动画、展开或颜色效果修改模型上下文。

## 13. 分阶段路线

### Phase 1：会话内研究状态基础

- 独立 `codex-research-state` crate。
- task/project 作用域。
- 通用研究条目、状态和操作。
- 事务式 reducer、幂等和 revision。
- `update_plan.research_delta`。
- SessionState 内存投影。
- 兼容原有 TUI 和 app-server。

### Phase 2：可观测性与读取

- 提供内部只读 snapshot API。
- 增加日志/telemetry。
- 工具结果返回 revision 和 changed 信息。
- 增加独立 ResearchStateUpdated 事件。
- 增加最小 TUI 展示。

### Phase 3：项目身份与持久化

- project_id。
- SQLite migrations。
- append-only events。
- materialized projections。
- 会话恢复和跨线程共享。

### Phase 4：有界上下文投影

- 任务签名。
- 相关性筛选。
- 稳定排序。
- token 硬上限。
- projection cache。
- ContextualUserFragment 接入。

### Phase 5：产品研究模型

- stakeholder。
- observation/proposal/tradeoff/externality/evidence。
- 决策权与证据强度分离。
- 产品目标和受影响范围。

### Phase 6：领域与全局研究积累

- domain/global 作用域。
- 候选知识。
- 重复证据。
- 冲突样本。
- 提升、降级、替代和失效。

### Phase 7：研究质量评价

- 是否减少重复探索。
- 是否减少补丁式修复。
- 是否更早发现项目结构压力。
- 是否提出更少但更关键的问题。
- 是否避免把请求者方案误当目标。
- 是否提高任务完成后的项目认知质量。

## 14. 测试策略

### 14.1 Reducer 单元测试

- upsert。
- set_status。
- remove。
- 幂等重复提交。
- revision。
- 批次事务性。
- 作用域隔离。
- 缺失字段。
- 不存在条目状态更新。

### 14.2 Tool Schema 测试

- 旧版 update_plan 参数仍可解析。
- research_delta 可选。
- Schema 固定且没有领域动态字段。
- operation/scope/kind/status 枚举稳定。
- 工具 Schema 大小受控。

### 14.3 Handler 集成测试

- 有效增量先应用，再发送 PlanUpdate。
- 无效增量不发送部分 PlanUpdate。
- 没有 research_delta 时行为不变。
- 同一增量重复提交不产生无意义 revision。

### 14.4 Persistence 测试

- 写入与恢复一致。
- 事件重放等于实时状态。
- schema migration。
- project_id 隔离。
- 并发 revision 冲突。

### 14.5 Context Cache 测试

- 相同 revision 和 task signature 命中缓存。
- 无关作用域变化不失效。
- 稳定排序产生相同文本。
- 投影严格遵守 token 上限。
- 上下文只追加，不改写历史。

### 14.6 TUI Snapshot 测试

- 研究条目显示。
- 状态颜色与符号。
- 长文本换行。
- 窄终端。
- 空状态和无效状态。

### 14.7 行为评价

准备成对任务：

```text
无研究状态基线
有研究状态版本
```

比较：

- 是否只做局部补丁。
- 是否发现共享结构。
- 是否识别重要未知项。
- 是否产生无关追问。
- 是否考虑受影响对象和长期效果。
- cached_input_tokens / non_cached_input_tokens。
- 上下文增长速度。

## 15. 当前完成状态

已完成：

- 总体方向梳理。
- 缓存机制研究。
- 本文档及相关源码文档。
- `codex-research-state` crate。
- task/project 作用域。
- 第一阶段条目类型、状态、操作。
- reducer、事务性、幂等和 revision。
- `update_plan.research_delta` Schema。
- PlanHandler 会话内应用。
- SessionState 内存状态。
- 旧 TUI/app-server 兼容字段处理。
- 单元测试与 Schema/解析测试源码。
- 格式化与 workspace metadata 验证。

尚未完成：

- 实际 Rust 编译与测试通过。
- 可观测 snapshot/revision 输出。
- 独立研究状态事件。
- TUI 展示。
- 持久化。
- 项目身份。
- 上下文投影与缓存。
- stakeholder/domain/global。
- 知识提升与评价。

## 16. 当前环境阻塞

当前 Windows 环境：

- 没有 WSL Linux 发行版。
- 没有 Docker/Podman。
- 原生 Rust MSVC 缺少 `link.exe` 和 Visual C++ Build Tools。
- Workspace Git 依赖下载曾发生超时。

仓库官方构建说明优先支持 Windows 11 via WSL2。因此在继续扩大改动前，应先建立可重复的 WSL2 测试环境，完成当前 Phase 1 的真实编译与测试。

## 17. 下一步唯一优先事项

在继续持久化、上下文投影或 UI 之前：

1. 建立受支持的 Rust/WSL2 构建环境。
2. 运行 `just fmt`。
3. 运行 `just test -p codex-research-state`。
4. 运行 `just test -p codex-protocol` 或对应定向测试。
5. 运行 `just test -p codex-core` 的计划工具相关测试。
6. 修复所有编译、TS、Schema 和集成问题。
7. 记录 Phase 1 基线测试结果。

基线未通过前，不进入 Phase 2。

