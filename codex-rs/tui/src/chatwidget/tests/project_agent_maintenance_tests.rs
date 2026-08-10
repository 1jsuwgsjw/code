use super::*;
use codex_app_server_protocol::ThreadProjectAgentMaintenanceRunResponse;
use codex_app_server_protocol::ThreadProjectAgentMaintenanceStatusUpdatedNotification;

fn maintenance_status(
    thread_id: ThreadId,
    pending_count: u64,
    catalog_revision: u64,
) -> ThreadProjectAgentMaintenanceStatusUpdatedNotification {
    ThreadProjectAgentMaintenanceStatusUpdatedNotification {
        thread_id: thread_id.to_string(),
        project_root: "/tmp/project".to_string(),
        pending_count,
        catalog_revision,
    }
}

fn rendered_history(rx: &mut tokio::sync::mpsc::UnboundedReceiver<AppEvent>) -> String {
    drain_insert_history(rx)
        .iter()
        .map(|lines| lines_to_single_string(lines))
        .collect::<Vec<_>>()
        .join("")
}

#[tokio::test]
async fn idle_project_agent_maintenance_notification_shows_reminder_snapshot() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual(/*model_override*/ None).await;
    let thread_id = ThreadId::new();
    chat.thread_id = Some(thread_id);

    chat.handle_server_notification(
        ServerNotification::ThreadProjectAgentMaintenanceStatusUpdated(maintenance_status(
            thread_id, /*pending_count*/ 2, /*catalog_revision*/ 7,
        )),
        /*replay_kind*/ None,
    );

    insta::assert_snapshot!(
        rendered_history(&mut rx),
        @"• Project AGENT maintenance has 2 pending items. Run /agents-maintain when you're ready; active tasks are never interrupted."
    );
}

#[tokio::test]
async fn duplicate_project_agent_maintenance_notification_is_reminded_once() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual(/*model_override*/ None).await;
    let thread_id = ThreadId::new();
    chat.thread_id = Some(thread_id);
    let notification = maintenance_status(
        thread_id, /*pending_count*/ 1, /*catalog_revision*/ 3,
    );

    chat.handle_server_notification(
        ServerNotification::ThreadProjectAgentMaintenanceStatusUpdated(notification.clone()),
        /*replay_kind*/ None,
    );
    assert!(!rendered_history(&mut rx).is_empty());

    chat.handle_server_notification(
        ServerNotification::ThreadProjectAgentMaintenanceStatusUpdated(notification),
        /*replay_kind*/ None,
    );

    assert!(drain_insert_history(&mut rx).is_empty());
}

#[tokio::test]
async fn active_turn_defers_project_agent_maintenance_reminder_until_completion() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual(/*model_override*/ None).await;
    let thread_id = ThreadId::new();
    chat.thread_id = Some(thread_id);
    handle_turn_started(&mut chat, "turn-1");

    chat.handle_server_notification(
        ServerNotification::ThreadProjectAgentMaintenanceStatusUpdated(maintenance_status(
            thread_id, /*pending_count*/ 4, /*catalog_revision*/ 9,
        )),
        /*replay_kind*/ None,
    );
    assert!(drain_insert_history(&mut rx).is_empty());

    handle_turn_completed(&mut chat, "turn-1", /*duration_ms*/ None);

    insta::assert_snapshot!(
        rendered_history(&mut rx),
        @"• Project AGENT maintenance has 4 pending items. Run /agents-maintain when you're ready; active tasks are never interrupted."
    );
}

#[tokio::test]
async fn zero_pending_project_agent_maintenance_status_stays_silent() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual(/*model_override*/ None).await;
    let thread_id = ThreadId::new();
    chat.thread_id = Some(thread_id);

    chat.handle_server_notification(
        ServerNotification::ThreadProjectAgentMaintenanceStatusUpdated(maintenance_status(
            thread_id, /*pending_count*/ 0, /*catalog_revision*/ 2,
        )),
        /*replay_kind*/ None,
    );

    assert!(drain_insert_history(&mut rx).is_empty());
}

#[tokio::test]
async fn project_agent_maintenance_notification_for_other_thread_is_ignored() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual(/*model_override*/ None).await;
    chat.thread_id = Some(ThreadId::new());

    chat.handle_server_notification(
        ServerNotification::ThreadProjectAgentMaintenanceStatusUpdated(maintenance_status(
            ThreadId::new(),
            /*pending_count*/ 2,
            /*catalog_revision*/ 5,
        )),
        /*replay_kind*/ None,
    );

    assert!(drain_insert_history(&mut rx).is_empty());
}

#[tokio::test]
async fn agents_maintain_command_targets_current_thread() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual(/*model_override*/ None).await;
    let thread_id = ThreadId::new();
    chat.thread_id = Some(thread_id);

    chat.dispatch_command(SlashCommand::AgentsMaintain);

    assert_matches!(
        rx.try_recv(),
        Ok(AppEvent::RunProjectAgentMaintenance { thread_id: actual }) if actual == thread_id
    );
}

#[tokio::test]
async fn agents_maintain_command_before_session_start_reports_error_snapshot() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual(/*model_override*/ None).await;

    chat.dispatch_command(SlashCommand::AgentsMaintain);

    insta::assert_snapshot!(
        rendered_history(&mut rx),
        @"■ Project AGENT maintenance is unavailable before the session starts."
    );
}

#[tokio::test]
async fn agents_maintain_command_is_disabled_during_active_task_snapshot() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual(/*model_override*/ None).await;
    chat.thread_id = Some(ThreadId::new());
    handle_turn_started(&mut chat, "turn-1");

    chat.dispatch_command(SlashCommand::AgentsMaintain);

    insta::assert_snapshot!(
        rendered_history(&mut rx),
        @"■ '/agents-maintain' is disabled while a task is in progress."
    );
}

#[tokio::test]
async fn project_agent_maintenance_completion_shows_counts_snapshot() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual(/*model_override*/ None).await;
    let thread_id = ThreadId::new();
    chat.thread_id = Some(thread_id);

    chat.on_project_agent_maintenance_completed(ThreadProjectAgentMaintenanceRunResponse {
        accepted_count: 3,
        rejected_count: 1,
        status: maintenance_status(
            thread_id, /*pending_count*/ 0, /*catalog_revision*/ 8,
        ),
    });

    insta::assert_snapshot!(
        rendered_history(&mut rx),
        @"• Project AGENT maintenance completed: 3 accepted, 1 rejected."
    );
}
