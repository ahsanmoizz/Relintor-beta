-- Additive workflow restoration state for the local project journey.
-- The snapshot is Rust-owned and contains only the serialized investigator view;
-- authority, takeover, mission, and execution records remain in their canonical
-- tables and are never replaced by renderer state.
CREATE TABLE IF NOT EXISTS project_workflow_state (
    project_id TEXT PRIMARY KEY REFERENCES projects(id) ON DELETE CASCADE,
    investigation_json TEXT,
    updated_at INTEGER NOT NULL
);

INSERT INTO schema_migrations(version, name, applied_at)
VALUES (7, '009_project_workflow_state', strftime('%s','now'))
ON CONFLICT(version) DO NOTHING;
