# CTF|认证 Research State Phase 4：有界上下文投影与缓存闭环

## 1. 单一交付目标

Phase 4 不拆分为多个外部阶段。一次性交付从持久化研究状态到模型可用研究上下文的完整闭环：

```text
当前用户任务
  + project research projection
  + project revision
        ↓
相关性排序与硬边界
        ↓
ResearchContext cache
        ↓
Codex WorldState diff
        ↓
模型上下文增量追加
```

它不是把 SQLite 全量状态塞进每个请求，也不是把用户偏好当成产品真相。

## 2. 原生接入位置

Research Context 作为原生 `WorldStateSection` 接入：

- section id：`research_context`
- fragment：实现 `ContextualUserFragment`
- role：`user`
- marker：`<research_context>...</research_context>`
- snapshot：只保存当前投影 digest，不在 WorldState snapshot 重复保存全量研究文本

没有当前投影时，`research_context: {}` 作为稳定的 removal tombstone 保留在 WorldState snapshot 中。它不进入模型文本，但能区分“已经撤销旧投影”和“仍需再次发送撤销说明”，避免每轮重复追加 removal fragment。

采用 WorldState 的原因：

- 初次上下文自动注入。
- 状态未改变时不重复追加。
- 投影改变时追加 replacement fragment，不改写历史。
- compaction 丢失 fragment 时可按 retained matcher 恢复。
- rollout 使用现有 full/patch WorldState 机制，不增加另一套历史协议。

## 3. 任务相关性

每个真实用户任务形成一个有界 `ResearchTaskContext`：

- 从文本、显式 skill 名和 mention 名提取。
- 合并空白。
- 最大 4096 bytes。
- 使用内容 SHA-1 形成不透明 `task_signature`。
- 图片或没有文本的内部继续步骤不覆盖上一任务信号。

投影器只读取 `project` scope，永远不投影 `task` scope。

排序由以下通用信号组成：

- 当前任务与 id、subject、statement 的词项重合。
- subject 与任务文本的直接包含关系。
- kind 的研究优先级。
- status 的证据成熟度。
- 最后使用稳定 entry id 排序，保证相同输入产生相同结果。

没有词项重合或 subject 包含关系的 project 条目不会进入候选集，避免仅凭 kind/status 的通用权重把无关知识注入当前任务。

这不是领域规则；UI、学习、构建、架构或其他任务使用同一投影算法。

## 4. 硬边界

投影必须同时满足：

- 最多 8 个 entries。
- entry projection 总估算不超过 2600 bytes。
- subject 最多 160 bytes。
- statement 最多 640 bytes。
- 超出的 UTF-8 文本在字符边界截断并标记省略号。
- fragment 固定指导语和 marker 加入后仍保持在约 1K token 以下。
- omitted entries 只记录在内部 tracing，不进入模型上下文。

## 5. 缓存

每个 Session 使用容量 16 的 LRU：

```text
project_id
+ project_revision
+ task_signature
+ projection_schema_version
```

缓存值是有界 `ResearchContextProjection`。

- 相同 key：直接复用 projection。
- project revision 变化：重新计算。
- task signature 变化：重新计算相关性。
- 即使 key 变化，只要最终 projection digest 相同，WorldState 不追加新 fragment。
- 因此任务表达变化但相关研究相同，不破坏模型历史前缀。
- task-only 状态变化不会推进独立的 project revision，也不会误击穿 project projection cache。
- retained history 只有旧投影而缺少当前投影时，会按当前投影内容重新注入，而不是把任意旧 marker 当成缓存命中。

## 6. 模型语义

Research Context 明确告诉模型：

- 条目是项目研究证据。
- 不是用户偏好。
- 不是绝对产品真相。
- 必须结合当前任务与当前代码重新验证。
- 新 fragment 替换先前 Research Context 的语义，而不是与旧投影叠加。

## 7. 缓存与历史边界

- ToolSpec 不改变。
- 不修改既有消息。
- 不把 SQLite snapshot 直接注入模型。
- 不在每一步重复全量 Research Context。
- project 状态写入后，当前模型已知道自己的工具调用；更新后的 Research Context 最迟在下一真实任务进入 WorldState。
- raw research events 仍只存在 SQLite；模型只看到选出的有界 projection。

## 8. 验收

- 相关 UI 任务优先选择 UI 条目而不是无关构建条目。
- project scope 可投影，task scope 不可投影。
- 超量条目和长文本严格受边界限制。
- 同一 projection 在连续相关任务中只出现一个 marker。
- project state 变化后新 projection 以 replacement 语义追加。
- compaction/resume 可恢复 Research Context baseline。
- Linux 定向测试、Windows EXE 构建与真实请求上下文验收全部通过。
