<!-- CTF|认证 -->

# 原生研究状态系统设计

完整目标、阶段路线和完成状态以 `notes/research-state-master-plan.md` 为准。本文保留第一阶段的具体设计说明。

## 1. 定位

该功能不是用户偏好记忆、任务答案仓库或 UI/UX 专用属性系统。它用于维护一种可演化的研究思考状态，使 Codex 在用户只给出有限输入时，仍能主动识别未知项、建立假设、探索项目、评估长期影响，并把经过证据支持的方法用于后续任务。

核心闭环：

```text
观察 → 建模 → 假设 → 探索 → 决策 → 执行 → 验证 → 提炼 → 更新研究起点
```

`update_plan` 仍是模型可见的计划/Todo 入口。研究状态通过可选增量并入该工具，完整状态由原生服务维护，不把完整知识库反复写入模型上下文。

## 2. 不是用户偏好系统

请求者输入是研究来源之一，不等于产品真相。系统必须区分：

- 硬约束
- 产品目标
- 请求者提出的方案
- 主观倾向
- 可观察反馈
- 代码与项目事实
- 测试、分析或其他证据

最终研究对象是产品或项目在具体条件下对相关对象产生的效果，而不是单独贴合请求者的偏好。

## 3. 作用域

第一阶段只实现：

- `task`：仅属于当前任务的研究状态。
- `project`：属于当前项目的事实、假设和未来压力。

后续可能增加：

- `stakeholder`：角色、目标、影响和证据来源，不是偏好库。
- `domain`：可跨项目复用的领域研究模式。
- `global`：跨领域仍成立的通用研究方法。

知识不能从一次任务直接提升到全局。提升必须经过重复证据、冲突检查和作用域审查。

## 4. 研究条目

第一阶段支持以下类型：

- `fact`：已确认事实。
- `constraint`：执行边界。
- `goal`：希望改变的效果。
- `unknown`：会影响决策但尚不明确的信息。
- `hypothesis`：需要研究或验证的解释。
- `exploration`：应主动进行的调查。
- `decision`：已经形成的选择及其依据。
- `future_pressure`：当前局部实现对未来产生的结构压力。
- `outcome`：执行后观察到的结果。

条目状态：

- `open`
- `supported`
- `rejected`
- `resolved`
- `superseded`

## 5. 增量而非完整快照

`update_plan` 增加可选的 `research_delta`。模型只提交变化：

```json
{
  "plan": [
    {
      "step": "检查现有主题系统",
      "status": "in_progress"
    }
  ],
  "research_delta": [
    {
      "operation": "upsert",
      "id": "theme-as-variable",
      "scope": "project",
      "kind": "hypothesis",
      "subject": "theme",
      "statement": "明暗需求连续出现，主题可能应升级为可配置维度",
      "status": "open"
    }
  ]
}
```

后续确认时只提交：

```json
{
  "plan": [],
  "research_delta": [
    {
      "operation": "set_status",
      "id": "theme-as-variable",
      "scope": "project",
      "status": "supported"
    }
  ]
}
```

## 6. 缓存边界

### 模型工具缓存

- `update_plan` 的 ToolSpec 只进行一次稳定扩展。
- 不根据 UI、学习、后端等领域动态生成字段。
- 使用固定枚举与通用字段。
- 使用确定性字段顺序和稳定描述。

### 模型上下文

- 完整研究状态不进入每次工具调用。
- 变更使用增量事件追加，不改写旧历史。
- 后续上下文投影必须有硬上限和稳定排序。
- UI 动画、展开状态和本地交互不进入模型历史。

### 本地状态

- 第一阶段为会话内状态，验证交互模型。
- 状态带 `revision`，仅在成功应用变更后递增。
- 后续持久化时使用 append-only events 和 materialized projection。

## 7. 原生并入

```text
update_plan ToolSpec
        ↓
PlanHandler
        ├── 保留现有 PlanUpdate 事件
        └── 应用 research_delta
                  ↓
           ResearchState reducer
                  ↓
           SessionState research projection
```

第一阶段不新增模型可见工具，不修改 app-server 的现有 `turn/plan/updated` 结构，也不增加数据库迁移。

## 8. 第一阶段修改范围

新增：

```text
codex-rs/research-state/
```

修改：

```text
codex-rs/Cargo.toml
codex-rs/protocol/Cargo.toml
codex-rs/protocol/src/plan_tool.rs
codex-rs/core/Cargo.toml
codex-rs/core/src/state/session.rs
codex-rs/core/src/session/mod.rs
codex-rs/core/src/tools/handlers/plan.rs
codex-rs/core/src/tools/handlers/plan_spec.rs
codex-rs/tui/src/history_cell/plans.rs
```

## 9. 第一阶段验收

- 旧的 `update_plan` 参数继续有效。
- 不提供 `research_delta` 时行为与当前版本一致。
- 可以在一个 `update_plan` 调用中新增、更新和删除研究条目。
- 同一增量重复提交不会制造无意义 revision。
- 无效状态变更返回明确错误，不部分写入状态。
- 当前 TUI 和 app-server 继续显示原有计划投影并忽略研究增量；独立研究状态 UI 留到后续阶段。
- ToolSpec 在不同任务领域中保持完全相同。

## 10. 后续阶段

1. 项目稳定身份和 SQLite 持久化。
2. 研究状态恢复与跨线程共享。
3. 有界 ResearchContext 投影与缓存。
4. stakeholder/domain/global 作用域。
5. 证据、冲突和知识提升流程。
6. 独立研究状态 UI。

## 11. 当前实现状态

第一阶段基础代码已经建立：

- 新增 `codex-research-state` crate。
- 实现 task/project 两种作用域。
- 实现通用研究条目类型、状态和增量操作。
- 实现事务式状态归约、幂等更新和 revision。
- `update_plan` 新增可选 `research_delta`。
- `PlanHandler` 在发送原有计划事件前应用研究增量。
- 研究状态暂存在 `SessionState`，尚未持久化或注入模型上下文。
- app-server 和 TUI 继续兼容原有计划投影，暂不展示研究状态。

Phase 1 已经通过 GitHub Actions 编译测试和真实 EXE 的会话内
`upsert -> set_status` 冒烟测试。

Phase 2 当前增加：

- 只读 `ResearchStateSnapshot`。
- 有界工具结果摘要，不把完整状态回灌模型上下文。
- 独立 `ResearchStateUpdated` 事件。
- app-server `turn/researchState/updated` 通知。
- 最小 Research State TUI 展示。
- revision、changed、entry_count 日志和测试。

Phase 2 通过真实构建与运行时复验后，再进入项目身份、持久化和有界上下文投影。
