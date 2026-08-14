use codex_app_server_protocol::ProjectAgentRosterEntry;
use codex_protocol::ThreadId;

#[derive(Debug, Clone)]
pub(crate) struct ProjectAgentMentionCatalog {
    pub(crate) agents: Vec<ProjectAgentRosterEntry>,
    pub(crate) recent_tasks: Vec<ProjectAgentTaskMention>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProjectAgentTaskMention {
    pub(crate) root_thread_id: ThreadId,
    pub(crate) task_id: String,
    pub(crate) task_title: String,
    pub(crate) agent_id: String,
    pub(crate) session_thread_id: String,
}

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
