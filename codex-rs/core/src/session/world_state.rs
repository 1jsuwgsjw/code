use super::session::Session;
use super::step_context::StepContext;
use crate::connectors;
use crate::context::world_state::AgentsMdState;
use crate::context::world_state::AppsInstructionsState;
use crate::context::world_state::EnvironmentsState;
use crate::context::world_state::PluginsInstructionsState;
use crate::context::world_state::ResearchContextState;
use crate::context::world_state::WorkflowRuntimeWorldState;
use crate::context::world_state::WorldState;
use codex_extension_api::WorldStateContributionInput;
use codex_research_state::ResearchContextCacheKey;
use codex_research_state::ResearchContextProjection;
use codex_research_state::project_research_context;

impl Session {
    #[tracing::instrument(name = "world_state.build", level = "info", skip_all)]
    pub(crate) async fn build_world_state_for_step(
        &self,
        step_context: &StepContext,
    ) -> WorldState {
        let turn_context = step_context.turn.as_ref();
        tracing::trace!(
            selected_capability_root_count = step_context.selected_capability_roots.len(),
            "building step world state"
        );
        let environment_subagents = if turn_context.config.include_environment_context {
            self.services
                .agent_control
                .format_environment_context_subagents(self.thread_id)
                .await
        } else {
            String::new()
        };

        let mut world_state = WorldState::default();
        world_state.add_section(AgentsMdState::new(step_context.loaded_agents_md.as_deref()));
        let (research_task_context, research_project_revision, research_entries, workflow_runtime) = {
            let state = self.state.lock().await;
            (
                state.research_task_context.clone(),
                state.research_project_revision,
                state.research_state.entries(),
                state.workflow_runtime.clone(),
            )
        };
        world_state.add_section(WorkflowRuntimeWorldState::new(workflow_runtime));
        let research_projection = if research_task_context.signature.is_empty() {
            ResearchContextProjection::default()
        } else {
            let project_id = self
                .services
                .research_project_id
                .clone()
                .unwrap_or_else(|| format!("session:{}", self.session_id()));
            let cache_key = ResearchContextCacheKey::new(
                project_id,
                research_project_revision,
                research_task_context.signature,
            );
            if let Some(projection) = self.services.research_context_cache.get(&cache_key) {
                tracing::trace!(
                    project_revision = research_project_revision,
                    entry_count = projection.entries.len(),
                    "research context projection cache hit"
                );
                projection
            } else {
                let projection = project_research_context(
                    research_entries.as_slice(),
                    research_task_context.text.as_str(),
                );
                self.services
                    .research_context_cache
                    .insert(cache_key, projection.clone());
                tracing::trace!(
                    project_revision = research_project_revision,
                    entry_count = projection.entries.len(),
                    omitted_count = projection.omitted_count,
                    "research context projection cache miss"
                );
                projection
            }
        };
        world_state.add_section(ResearchContextState::new(research_projection));
        if turn_context.config.include_environment_context {
            world_state.add_section(
                EnvironmentsState::from_turn_context_with_environments(
                    turn_context,
                    &step_context.environments,
                )
                .with_subagents(environment_subagents),
            );
        }
        let apps_available =
            if turn_context.config.include_apps_instructions && turn_context.apps_enabled() {
                let tools = step_context.mcp_tools().await;
                connectors::with_app_enabled_state(
                    connectors::accessible_connectors_from_mcp_tools(tools),
                    &turn_context.config,
                )
                .into_iter()
                .any(|connector| connector.is_accessible && connector.is_enabled)
            } else {
                false
            };
        world_state.add_section(AppsInstructionsState::new(apps_available));
        world_state.add_section(PluginsInstructionsState::new(
            step_context.mcp.plugins_available(),
        ));
        let environments = step_context.environments.to_selections();
        let ready_selected_capability_roots = step_context
            .selected_capability_roots
            .iter()
            .map(|root| root.selected_root().clone())
            .collect::<Vec<_>>();
        for contributor in self.services.extensions.context_contributors() {
            for section in contributor
                .contribute_world_state(WorldStateContributionInput {
                    thread_id: self.thread_id(),
                    turn_id: turn_context.sub_id.as_str(),
                    environments: &environments,
                    ready_selected_capability_roots: &ready_selected_capability_roots,
                    session_store: &self.services.session_extension_data,
                    thread_store: &self.services.thread_extension_data,
                    turn_store: turn_context.extension_data.as_ref(),
                })
                .await
            {
                world_state.add_extension_section(section);
            }
        }
        world_state
    }
}
