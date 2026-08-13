#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ProjectAgentWorkbenchAction {
    Open {
        selected_task_id: Option<String>,
    },
    PromptCreateRoot,
    PromptCreateChild(String),
    Create {
        parent_task_id: Option<String>,
        input: String,
    },
    PromptAppendRequirement(String),
    AppendRequirement {
        task_id: String,
        requirement: String,
    },
    PromptStartExecution(String),
    StartExecution {
        task_id: String,
        agent_id: String,
    },
    OpenConversation {
        task_id: String,
        task_title: String,
        agent_id: String,
        session_thread_id: String,
    },
    Refresh(String),
}
