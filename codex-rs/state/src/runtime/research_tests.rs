use super::*;
use crate::runtime::test_support::unique_temp_dir;
use pretty_assertions::assert_eq;

fn upsert(id: &str, statement: &str) -> ResearchDelta {
    ResearchDelta {
        operation: ResearchOperation::Upsert,
        id: id.to_string(),
        scope: ResearchScope::Project,
        kind: Some(ResearchEntryKind::Hypothesis),
        subject: Some("runtime".to_string()),
        statement: Some(statement.to_string()),
        status: Some(ResearchStatus::Open),
    }
}

#[tokio::test]
async fn aliases_bridge_identity_changes() -> anyhow::Result<()> {
    let runtime = StateRuntime::init(unique_temp_dir(), "test-provider".to_string()).await?;
    let first = runtime
        .resolve_research_project(&["remote:one".to_string(), "path:stable".to_string()])
        .await?;
    let second = runtime
        .resolve_research_project(&["path:stable".to_string(), "remote:two".to_string()])
        .await?;
    let moved = runtime
        .resolve_research_project(&["remote:two".to_string(), "path:moved".to_string()])
        .await?;

    assert_eq!(second.project_id, first.project_id);
    assert_eq!(moved.project_id, first.project_id);
    runtime.close().await;
    Ok(())
}

#[tokio::test]
async fn project_deltas_persist_without_idempotent_event_churn() -> anyhow::Result<()> {
    let runtime = StateRuntime::init(unique_temp_dir(), "test-provider".to_string()).await?;
    let project = runtime
        .resolve_research_project(&["remote:project".to_string()])
        .await?;
    let delta = upsert("phase3", "Project research should survive sessions");

    let changed = runtime
        .apply_research_project_deltas(project.project_id.as_str(), std::slice::from_ref(&delta))
        .await?;
    let unchanged = runtime
        .apply_research_project_deltas(project.project_id.as_str(), std::slice::from_ref(&delta))
        .await?;
    let snapshot = runtime
        .load_research_project_snapshot(project.project_id.as_str())
        .await?;
    let event_count =
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM research_events WHERE project_id = ?")
            .bind(project.project_id.as_str())
            .fetch_one(runtime.pool.as_ref())
            .await?;

    assert_eq!(
        (
            changed.revision,
            changed.changed,
            unchanged.revision,
            unchanged.changed
        ),
        (1, true, 1, false)
    );
    assert_eq!(
        snapshot,
        ResearchStateSnapshot {
            revision: changed.revision,
            entries: changed.entries,
        }
    );
    assert_eq!(event_count, 1);
    runtime.close().await;
    Ok(())
}
