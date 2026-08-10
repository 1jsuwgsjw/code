use super::App;
use crate::app_server_session::AppServerSession;
use codex_protocol::ThreadId;

impl App {
    pub(super) async fn run_project_agent_maintenance(
        &mut self,
        app_server: &mut AppServerSession,
        thread_id: ThreadId,
    ) {
        let result = app_server
            .thread_project_agent_maintenance_run(thread_id)
            .await;
        if self.current_displayed_thread_id() != Some(thread_id) {
            return;
        }

        match result {
            Ok(response) => self
                .chat_widget
                .on_project_agent_maintenance_completed(response),
            Err(err) => self
                .chat_widget
                .add_error_message(format!("Failed to run project AGENT maintenance: {err}")),
        }
    }
}
