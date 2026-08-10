use std::sync::Arc;
use std::sync::Weak;

use codex_config::default_project_root_markers;
use codex_core::ThreadManager;
use codex_core::config::Config;
use codex_exec_server::EnvironmentManager;
use codex_exec_server::ExecutorFileSystem;
use codex_extension_api::ContextContributor;
use codex_extension_api::ExtensionData;
use codex_extension_api::ExtensionFuture;
use codex_extension_api::ExtensionRegistryBuilder;
use codex_extension_api::PromptFragment;
use codex_extension_api::ThreadLifecycleContributor;
use codex_extension_api::ThreadStartInput;
use codex_extension_api::ToolCall;
use codex_extension_api::ToolContributor;
use codex_extension_api::ToolExecutor;
use codex_extension_api::ToolVisibilityContributor;
use codex_extension_api::ToolVisibilityPolicy;
use codex_project_agents::ProjectAgentEntry;
use codex_project_agents::ProjectAgentFileSystemScope;
use codex_project_agents::ProjectAgentRuntime;
use codex_project_agents::ProjectAgentStore;
use codex_project_agents::RelativeProjectAgentPath;
use codex_project_agents::resolve_project_root;
use codex_protocol::ThreadId;
use codex_protocol::protocol::TurnEnvironmentSelection;

use crate::delegate::ProjectAgentDelegateTool;
use crate::delegate::worker_context_prompt;
use crate::x_tools::project_agent_x_tools;
use crate::x_tools::worker_visible_tool_names;

pub(crate) struct ProjectAgentRootContext {
    pub(crate) config: Config,
    pub(crate) environments: Vec<TurnEnvironmentSelection>,
    pub(crate) primary_environment_id: String,
    pub(crate) file_system: Arc<dyn ExecutorFileSystem>,
    pub(crate) store: ProjectAgentStore,
    pub(crate) enabled_agents: Vec<ProjectAgentEntry>,
}

#[derive(Clone)]
pub(crate) struct ProjectAgentWorkerContext {
    pub(crate) thread_id: Option<ThreadId>,
    pub(crate) store: ProjectAgentStore,
    pub(crate) runtime: ProjectAgentRuntime,
    pub(crate) task_id: String,
    pub(crate) task: String,
    pub(crate) primary_environment_id: String,
}

pub(crate) struct ProjectAgentExtension {
    thread_manager: Weak<ThreadManager>,
    environment_manager: Arc<EnvironmentManager>,
}

impl ProjectAgentExtension {
    pub(crate) fn new(
        thread_manager: Weak<ThreadManager>,
        environment_manager: Arc<EnvironmentManager>,
    ) -> Self {
        Self {
            thread_manager,
            environment_manager,
        }
    }
}

impl ThreadLifecycleContributor<Config> for ProjectAgentExtension {
    fn on_thread_start<'a>(
        &'a self,
        input: ThreadStartInput<'a, Config>,
    ) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            if let Some(worker_context) = input.thread_store.get::<ProjectAgentWorkerContext>() {
                let mut worker_context = (*worker_context).clone();
                worker_context.thread_id =
                    ThreadId::from_string(input.thread_store.level_id()).ok();
                input.thread_store.insert(worker_context);
                return;
            }

            let Some(primary_environment) = input.environments.first() else {
                return;
            };
            let Some(environment) = self
                .environment_manager
                .get_environment(&primary_environment.environment_id)
            else {
                tracing::warn!(
                    environment_id = %primary_environment.environment_id,
                    "project AGENT extension could not resolve the thread environment"
                );
                return;
            };
            let markers = match default_project_root_markers()
                .into_iter()
                .map(RelativeProjectAgentPath::new)
                .collect::<Result<Vec<_>, _>>()
            {
                Ok(markers) => markers,
                Err(error) => {
                    tracing::warn!(%error, "project AGENT extension rejected a project-root marker");
                    return;
                }
            };
            let file_system = environment.get_filesystem();
            let project_root = match resolve_project_root(
                file_system.as_ref(),
                &primary_environment.cwd,
                &markers,
                ProjectAgentFileSystemScope::Unrestricted,
            )
            .await
            {
                Ok(project_root) => project_root,
                Err(error) => {
                    tracing::warn!(%error, "project AGENT extension could not resolve the project root");
                    return;
                }
            };
            let store = match ProjectAgentStore::new(project_root) {
                Ok(store) => store,
                Err(error) => {
                    tracing::warn!(%error, "project AGENT extension could not initialize its store");
                    return;
                }
            };
            if let Err(error) = store
                .bootstrap(
                    file_system.as_ref(),
                    ProjectAgentFileSystemScope::Unrestricted,
                )
                .await
            {
                tracing::warn!(%error, "project AGENT extension could not bootstrap the registry");
                return;
            }
            let enabled_agents = match store
                .list(
                    file_system.as_ref(),
                    ProjectAgentFileSystemScope::Unrestricted,
                )
                .await
            {
                Ok(entries) => entries.into_iter().filter(|entry| entry.enabled).collect(),
                Err(error) => {
                    tracing::warn!(%error, "project AGENT extension could not load the registry");
                    return;
                }
            };
            input.thread_store.insert(ProjectAgentRootContext {
                config: input.config.clone(),
                environments: input.environments.to_vec(),
                primary_environment_id: primary_environment.environment_id.clone(),
                file_system,
                store,
                enabled_agents,
            });
        })
    }
}

impl ContextContributor for ProjectAgentExtension {
    fn contribute_thread_context<'a>(
        &'a self,
        _session_store: &'a ExtensionData,
        thread_store: &'a ExtensionData,
    ) -> ExtensionFuture<'a, Vec<PromptFragment>> {
        Box::pin(async move {
            thread_store
                .get::<ProjectAgentWorkerContext>()
                .map(|context| {
                    vec![PromptFragment::developer_policy(worker_context_prompt(
                        &context,
                    ))]
                })
                .unwrap_or_default()
        })
    }
}

impl ToolContributor for ProjectAgentExtension {
    fn tools(
        &self,
        _session_store: &ExtensionData,
        thread_store: &ExtensionData,
    ) -> Vec<Arc<dyn ToolExecutor<ToolCall>>> {
        if let Some(context) = thread_store.get::<ProjectAgentRootContext>() {
            return context
                .enabled_agents
                .iter()
                .filter(|entry| entry.enabled)
                .cloned()
                .map(|entry| {
                    Arc::new(ProjectAgentDelegateTool::new(
                        Arc::clone(&context),
                        entry,
                        self.thread_manager.clone(),
                    )) as Arc<dyn ToolExecutor<ToolCall>>
                })
                .collect();
        }
        let Some(context) = thread_store.get::<ProjectAgentWorkerContext>() else {
            return Vec::new();
        };
        project_agent_x_tools(
            context,
            self.thread_manager.clone(),
            Arc::clone(&self.environment_manager),
        )
    }
}

impl ToolVisibilityContributor for ProjectAgentExtension {
    fn visibility(
        &self,
        _session_store: &ExtensionData,
        thread_store: &ExtensionData,
    ) -> ToolVisibilityPolicy {
        let Some(context) = thread_store.get::<ProjectAgentWorkerContext>() else {
            return ToolVisibilityPolicy::default();
        };
        ToolVisibilityPolicy::allow_only(worker_visible_tool_names(&context.runtime.tools))
    }
}

/// Installs project AGENT lifecycle, prompt, tool, and visibility contributors.
pub fn install(
    registry: &mut ExtensionRegistryBuilder<Config>,
    thread_manager: Weak<ThreadManager>,
    environment_manager: Arc<EnvironmentManager>,
) {
    let extension = Arc::new(ProjectAgentExtension::new(
        thread_manager,
        environment_manager,
    ));
    registry.thread_lifecycle_contributor(extension.clone());
    registry.prompt_contributor(extension.clone());
    registry.tool_contributor(extension.clone());
    registry.tool_visibility_contributor(extension);
}
