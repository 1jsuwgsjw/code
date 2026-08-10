use super::ChatWidget;
use codex_app_server_protocol::ThreadProjectAgentMaintenanceRunResponse;
use codex_app_server_protocol::ThreadProjectAgentMaintenanceStatusUpdatedNotification;

#[derive(Default)]
pub(super) struct ProjectAgentMaintenanceState {
    last_status: Option<ThreadProjectAgentMaintenanceStatusUpdatedNotification>,
    last_reminded: Option<(String, u64, u64)>,
}

impl ChatWidget {
    pub(super) fn on_project_agent_maintenance_status_updated(
        &mut self,
        notification: ThreadProjectAgentMaintenanceStatusUpdatedNotification,
    ) {
        let Some(thread_id) = self.thread_id() else {
            return;
        };
        if notification.thread_id != thread_id.to_string() {
            return;
        }

        if notification.pending_count == 0 {
            self.project_agent_maintenance.last_reminded = None;
        }
        self.project_agent_maintenance.last_status = Some(notification);
        self.maybe_show_project_agent_maintenance_reminder();
    }

    pub(super) fn maybe_show_project_agent_maintenance_reminder(&mut self) {
        if self.input_queue.user_turn_pending_start || self.turn_lifecycle.agent_turn_running {
            return;
        }

        let Some(status) = self.project_agent_maintenance.last_status.as_ref() else {
            return;
        };
        let Some(thread_id) = self.thread_id() else {
            return;
        };
        if status.thread_id != thread_id.to_string() || status.pending_count == 0 {
            return;
        }

        let reminder_key = (
            status.thread_id.clone(),
            status.pending_count,
            status.catalog_revision,
        );
        if self.project_agent_maintenance.last_reminded.as_ref() == Some(&reminder_key) {
            return;
        }

        let pending_count = status.pending_count;
        self.project_agent_maintenance.last_reminded = Some(reminder_key);
        self.add_info_message(
            format!("Project AGENT maintenance has {pending_count} pending items."),
            Some(
                "Run /agents-maintain when you're ready; active tasks are never interrupted."
                    .to_string(),
            ),
        );
    }

    pub(crate) fn on_project_agent_maintenance_completed(
        &mut self,
        response: ThreadProjectAgentMaintenanceRunResponse,
    ) {
        let ThreadProjectAgentMaintenanceRunResponse {
            accepted_count,
            rejected_count,
            status,
        } = response;
        let Some(thread_id) = self.thread_id() else {
            return;
        };
        if status.thread_id != thread_id.to_string() {
            return;
        }

        if status.pending_count == 0 {
            self.project_agent_maintenance.last_reminded = None;
        }
        self.project_agent_maintenance.last_status = Some(status);
        self.add_info_message(
            format!(
                "Project AGENT maintenance completed: {accepted_count} accepted, {rejected_count} rejected."
            ),
            /*hint*/ None,
        );
    }
}
