# CTF|认证 Research State Phase 3：项目身份与持久化

## 1. 第一性目标

Phase 3 不是简单地把会话内对象序列化到磁盘，而是让项目研究状态在跨会话、跨线程和并发写入时，仍保持与当前 reducer 完全相同的业务语义。

本阶段必须同时满足：

- `project` scope 可恢复和跨线程共享。
- `task` scope 只属于当前会话，不写入项目数据库。
- reducer 仍是唯一业务规则来源，SQLite 不复制 upsert、set_status、remove 的判断。
- 事件、投影和 project revision 在同一个 SQLite transaction 中提交。
- 幂等重放不增加 revision，也不追加无意义事件。
- 数据库提交失败时，不提前修改会话内状态。
- 持久化状态不会自动进入模型历史，Phase 4 才负责有界上下文投影。

## 2. 项目身份不是绝对路径

项目身份采用“多个不透明别名映射到一个 `project_id`”的方式，而不是直接把当前绝对路径当作长期主键。

Git 项目的证据优先级：

1. canonical Git remote。
2. 无 remote 时使用所有 root commit 组成的 repository fingerprint。
3. canonical workspace root 作为本机连续性别名。

规则：

- 有 remote：使用 remote aliases 与 path alias。
- 无 remote 的 Git 仓库：使用 repository fingerprint alias 与 path alias。
- 非 Git 目录：使用 path alias。
- 所有 alias material 先通过 UUID v5 转成不透明值，数据库不保存 remote URL 或工作区绝对路径原文。
- 同一个 alias 已存在时复用原 `project_id`，并把本次发现的新 aliases 绑定进去。
- 多个 aliases 意外指向不同项目时返回明确冲突，不静默合并状态。

由此得到：

- 项目移动但 remote 不变：remote alias 保持连续。
- remote 改变但路径不变：path alias 找到旧项目，并登记新 remote alias。
- worktree：remote alias 保持一致；无 remote 时 root commits fingerprint 保持一致。
- 无 remote 的 Git 项目移动：root commits fingerprint 保持连续。
- 非 Git 项目移动：没有可携带身份时无法从纯路径可靠推断，当前退化为新项目；未来可增加显式、用户可控的 project marker，但本阶段不自动污染项目目录。

## 3. 数据模型

迁移 `0041_research_state.sql` 增加四张表。

### `research_projects`

- `project_id`：稳定内部标识。
- `revision`：仅 project scope 的持久化 revision。
- `created_at_ms` / `updated_at_ms`。

### `research_project_aliases`

- `alias`：不透明身份别名，唯一。
- `project_id`：指向研究项目。
- `created_at_ms`。

### `research_events`

- append-only。
- 每个成功变化批次共享同一个 `revision`。
- `event_index` 保留批次内顺序。
- 保存 operation、entry id、scope 与原始 delta JSON。
- `(project_id, revision, event_index)` 唯一。

### `research_projections`

- 当前 materialized entries。
- `(project_id, scope, entry_id)` 为主键。
- 保存可查询字段及完整 entry JSON。
- 与事件和 revision 同事务更新。

## 4. 写入算法

`StateRuntime::apply_research_project_deltas`：

1. 拒绝非 `project` scope delta。
2. `BEGIN IMMEDIATE`，序列化同一 SQLite 数据库中的写事务。
3. 在事务内读取当前 project revision 与 projection。
4. 用 `codex-research-state::ResearchState` 从 snapshot 恢复 reducer。
5. 应用完整 delta batch。
6. 若 `changed == false`，不写事件、不增加 revision，直接返回。
7. 若变化：
   - append events；
   - 只更新本批受影响 entry 的 projection；
   - 用旧 revision 条件更新 project revision；
   - commit。

`BEGIN IMMEDIATE` 与 revision 条件更新共同保证不会出现两个 writer 都基于同一旧 revision 静默提交。

## 5. 会话接入

Session 启动：

1. 从初始 cwd 收集项目身份 aliases。
2. 使用 `StateRuntime` 解析或创建 `project_id`。
3. 加载 project snapshot。
4. 用 snapshot 初始化会话 `ResearchState`。
5. 任一步失败时记录 warning，并退化为 Phase 2 的会话内状态，不阻塞 Codex 启动。

`update_plan.research_delta`：

1. 没有持久化上下文时沿用原会话内 reducer。
2. 有持久化上下文时：
   - task deltas 先在内存候选状态中验证；
   - project deltas交给 SQLite 事务中的同一个 reducer；
   - 数据库成功后，把最新 project projection 与 task 候选合并；
   - 会话可观测 revision 只在最终合并状态变化时增加一次。
3. 数据库失败时不修改内存状态，也不发送部分 ResearchStateUpdated 事件。

## 6. 缓存边界

- ToolSpec 不随项目状态变化。
- SQLite snapshot 不写入 rollout，也不自动注入模型消息。
- `project_id + project_revision` 只作为后续 Phase 4 projection cache key 的基础。
- 当前 TUI 继续消费有界 ResearchStateUpdated 事件。
- 恢复的全量 snapshot 只存在于本地状态层和会话状态层。

## 7. 验收条件

- 同项目关闭并重启 Codex 后，project entries 恢复。
- task entries 不跨会话出现。
- 同 delta 重放返回 unchanged，project revision 不增加，events 不增加。
- 两个会话连续写入同一项目时，后写者基于数据库最新 projection 归约。
- alias 桥接可处理常见项目移动、remote 变化和 worktree。
- 数据库损坏或不可用时，Codex 仍可退化为会话内研究状态。
- 不产生模型上下文缓存回退。
