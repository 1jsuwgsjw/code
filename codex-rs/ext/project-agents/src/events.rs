use std::sync::Arc;

use codex_extension_api::ExtensionEventSink;
use codex_project_agents::ProjectAgentMaintenanceStatus;
use codex_protocol::ThreadId;
use codex_protocol::protocol::Event;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::ThreadProjectAgentMaintenanceStatusUpdatedEvent;

#[derive(Clone)]
pub(crate) struct ProjectAgentEventEmitter {
    sink: Arc<dyn ExtensionEventSink>,
}

impl ProjectAgentEventEmitter {
    pub(crate) fn new(sink: Arc<dyn ExtensionEventSink>) -> Self {
        Self { sink }
    }

    pub(crate) fn maintenance_status_updated(
        &self,
        thread_id: ThreadId,
        project_root: String,
        status: &ProjectAgentMaintenanceStatus,
    ) {
        let pending_count = status.pending().total();
        self.sink.emit(Event {
            id: format!(
                "project-agent-maintenance-{thread_id}-{}-{pending_count}",
                status.catalog_revision
            ),
            msg: EventMsg::ThreadProjectAgentMaintenanceStatusUpdated(
                ThreadProjectAgentMaintenanceStatusUpdatedEvent {
                    thread_id,
                    project_root,
                    pending_count,
                    catalog_revision: status.catalog_revision,
                },
            ),
        });
    }
}
