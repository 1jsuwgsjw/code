CREATE TABLE IF NOT EXISTS research_projects (
    project_id TEXT PRIMARY KEY,
    revision INTEGER NOT NULL DEFAULT 0 CHECK (revision >= 0),
    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS research_project_aliases (
    alias TEXT PRIMARY KEY,
    project_id TEXT NOT NULL,
    created_at_ms INTEGER NOT NULL,
    FOREIGN KEY(project_id) REFERENCES research_projects(project_id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS research_project_aliases_project_id_idx
    ON research_project_aliases(project_id);

CREATE TABLE IF NOT EXISTS research_events (
    event_id INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id TEXT NOT NULL,
    revision INTEGER NOT NULL CHECK (revision > 0),
    event_index INTEGER NOT NULL CHECK (event_index >= 0),
    operation TEXT NOT NULL,
    entry_id TEXT NOT NULL,
    scope TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    created_at_ms INTEGER NOT NULL,
    FOREIGN KEY(project_id) REFERENCES research_projects(project_id) ON DELETE CASCADE,
    UNIQUE(project_id, revision, event_index)
);

CREATE INDEX IF NOT EXISTS research_events_project_revision_idx
    ON research_events(project_id, revision);

CREATE TABLE IF NOT EXISTS research_projections (
    project_id TEXT NOT NULL,
    scope TEXT NOT NULL,
    entry_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    subject TEXT NOT NULL,
    statement TEXT NOT NULL,
    status TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    revision INTEGER NOT NULL CHECK (revision > 0),
    updated_at_ms INTEGER NOT NULL,
    PRIMARY KEY(project_id, scope, entry_id),
    FOREIGN KEY(project_id) REFERENCES research_projects(project_id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS research_projections_project_revision_idx
    ON research_projections(project_id, revision);
