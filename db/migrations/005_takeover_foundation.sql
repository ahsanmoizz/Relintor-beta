-- Additive P5 existing-project takeover persistence.
CREATE TABLE IF NOT EXISTS project_takeovers (
    id TEXT PRIMARY KEY,
    root TEXT NOT NULL,
    fingerprint TEXT NOT NULL,
    scanner_version TEXT NOT NULL,
    created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS repository_snapshots (
    id TEXT PRIMARY KEY,
    takeover_id TEXT NOT NULL REFERENCES project_takeovers(id) ON DELETE CASCADE,
    root TEXT NOT NULL,
    is_git INTEGER NOT NULL,
    file_count INTEGER NOT NULL,
    total_bytes INTEGER NOT NULL,
    fingerprint TEXT NOT NULL,
    created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS takeover_inventory (
    id TEXT PRIMARY KEY,
    snapshot_id TEXT NOT NULL REFERENCES repository_snapshots(id) ON DELETE CASCADE,
    relative_path TEXT NOT NULL,
    canonical_path TEXT NOT NULL,
    file_type TEXT NOT NULL,
    size_bytes INTEGER NOT NULL,
    content_hash TEXT,
    classification TEXT NOT NULL,
    included INTEGER NOT NULL,
    excluded_reason TEXT
);

CREATE TABLE IF NOT EXISTS takeover_capabilities (
    id TEXT PRIMARY KEY,
    takeover_id TEXT NOT NULL REFERENCES project_takeovers(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    classification TEXT NOT NULL,
    confidence TEXT NOT NULL,
    evidence TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS takeover_findings (
    id TEXT PRIMARY KEY,
    takeover_id TEXT NOT NULL REFERENCES project_takeovers(id) ON DELETE CASCADE,
    finding_type TEXT NOT NULL,
    summary TEXT NOT NULL,
    severity TEXT NOT NULL,
    classification TEXT,
    evidence TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS takeover_recommendations (
    id TEXT PRIMARY KEY,
    takeover_id TEXT NOT NULL REFERENCES project_takeovers(id) ON DELETE CASCADE,
    target TEXT NOT NULL,
    kind TEXT NOT NULL,
    current_reality TEXT NOT NULL,
    priority TEXT NOT NULL,
    recommendation TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS takeover_runtime_probes (
    id TEXT PRIMARY KEY,
    takeover_id TEXT NOT NULL REFERENCES project_takeovers(id) ON DELETE CASCADE,
    target TEXT NOT NULL,
    command TEXT NOT NULL,
    safety TEXT NOT NULL,
    status TEXT NOT NULL,
    exit_code INTEGER,
    stdout TEXT NOT NULL,
    stderr TEXT NOT NULL,
    fingerprint TEXT NOT NULL,
    started_at INTEGER NOT NULL,
    ended_at INTEGER
);

CREATE TABLE IF NOT EXISTS takeover_revisions (
    id TEXT PRIMARY KEY,
    takeover_id TEXT NOT NULL REFERENCES project_takeovers(id) ON DELETE CASCADE,
    revision INTEGER NOT NULL,
    fingerprint TEXT NOT NULL,
    changed_paths TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    UNIQUE(takeover_id, revision)
);

INSERT INTO schema_migrations(version, name, applied_at)
VALUES (3, '005_takeover_foundation', strftime('%s','now'))
ON CONFLICT(version) DO NOTHING;
