-- Additive P6 final re-audit repair.
-- Existing takeover and authority migrations remain immutable.

ALTER TABLE project_takeovers ADD COLUMN project_id TEXT;

CREATE INDEX IF NOT EXISTS idx_project_takeovers_project_created
    ON project_takeovers(project_id, created_at DESC);

CREATE TABLE IF NOT EXISTS authority_reviews (
    project_id TEXT PRIMARY KEY,
    review_digest TEXT NOT NULL,
    facts_json TEXT NOT NULL,
    reviewed_at TEXT NOT NULL
);

INSERT INTO schema_migrations(version, name, applied_at)
VALUES (6, '008_takeover_project_binding', strftime('%s','now'))
ON CONFLICT(version) DO NOTHING;
