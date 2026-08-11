#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ProjectAgentWorkbenchAction {
    List,
    Read(String),
    PromptFollowUp(String),
    FollowUp { agent_id: String, message: String },
    Terminate(String),
    Retry { agent_id: String, task_id: String },
    Rebuild(String),
}
