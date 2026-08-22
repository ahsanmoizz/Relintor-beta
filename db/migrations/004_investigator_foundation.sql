-- Local P4 investigator domain. This migration is additive and repeatable.
CREATE TABLE IF NOT EXISTS project_drafts (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    idea TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    version INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS investigations (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES project_drafts(id) ON DELETE CASCADE,
    protocol_version TEXT NOT NULL,
    policy_version TEXT NOT NULL,
    status TEXT NOT NULL,
    fingerprint TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    version INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS investigation_inputs (
    id TEXT PRIMARY KEY,
    investigation_id TEXT NOT NULL REFERENCES investigations(id) ON DELETE CASCADE,
    kind TEXT NOT NULL,
    value TEXT NOT NULL,
    provenance TEXT NOT NULL,
    content_hash TEXT NOT NULL,
    created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS source_documents (
    id TEXT PRIMARY KEY,
    investigation_id TEXT NOT NULL REFERENCES investigations(id) ON DELETE CASCADE,
    display_name TEXT NOT NULL,
    canonical_path TEXT,
    mime_type TEXT NOT NULL,
    size_bytes INTEGER NOT NULL,
    content_hash TEXT NOT NULL,
    extraction_status TEXT NOT NULL,
    source_kind TEXT NOT NULL,
    provenance TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    UNIQUE(investigation_id, content_hash)
);

CREATE TABLE IF NOT EXISTS investigator_questions (
    id TEXT PRIMARY KEY,
    investigation_id TEXT NOT NULL REFERENCES investigations(id) ON DELETE CASCADE,
    question TEXT NOT NULL,
    why_it_matters TEXT NOT NULL,
    recommended_default TEXT NOT NULL,
    disposition TEXT NOT NULL,
    source_ambiguity TEXT NOT NULL,
    created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS user_answers (
    id TEXT PRIMARY KEY,
    question_id TEXT NOT NULL REFERENCES investigator_questions(id) ON DELETE CASCADE,
    answer TEXT NOT NULL,
    confirmed INTEGER NOT NULL,
    created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS blueprint_revisions (
    id TEXT PRIMARY KEY,
    investigation_id TEXT NOT NULL REFERENCES investigations(id) ON DELETE CASCADE,
    revision INTEGER NOT NULL,
    status TEXT NOT NULL,
    fingerprint TEXT NOT NULL,
    normalized_blueprint TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    UNIQUE(investigation_id, revision)
);

CREATE TABLE IF NOT EXISTS blueprint_approvals (
    id TEXT PRIMARY KEY,
    blueprint_revision_id TEXT NOT NULL REFERENCES blueprint_revisions(id) ON DELETE CASCADE,
    approved_by TEXT NOT NULL,
    created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS architecture_decisions (
    id TEXT PRIMARY KEY,
    investigation_id TEXT NOT NULL REFERENCES investigations(id) ON DELETE CASCADE,
    decision TEXT NOT NULL,
    selected_option TEXT,
    status TEXT NOT NULL,
    source TEXT NOT NULL,
    created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS risks (
    id TEXT PRIMARY KEY,
    investigation_id TEXT NOT NULL REFERENCES investigations(id) ON DELETE CASCADE,
    title TEXT NOT NULL,
    category TEXT NOT NULL,
    severity TEXT NOT NULL,
    status TEXT NOT NULL,
    source TEXT NOT NULL,
    created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS assumptions (
    id TEXT PRIMARY KEY,
    investigation_id TEXT NOT NULL REFERENCES investigations(id) ON DELETE CASCADE,
    statement TEXT NOT NULL,
    confidence TEXT NOT NULL,
    status TEXT NOT NULL,
    source TEXT NOT NULL,
    created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS conflicts (
    id TEXT PRIMARY KEY,
    investigation_id TEXT NOT NULL REFERENCES investigations(id) ON DELETE CASCADE,
    claim_a TEXT NOT NULL,
    claim_b TEXT NOT NULL,
    severity TEXT NOT NULL,
    resolution_state TEXT NOT NULL,
    source_a TEXT NOT NULL,
    source_b TEXT NOT NULL,
    created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS blueprint_candidates (
    id TEXT PRIMARY KEY,
    blueprint_revision_id TEXT NOT NULL REFERENCES blueprint_revisions(id) ON DELETE CASCADE,
    candidate_type TEXT NOT NULL,
    title TEXT NOT NULL,
    rationale TEXT NOT NULL,
    created_at INTEGER NOT NULL
);

INSERT INTO schema_migrations(version, name, applied_at)
VALUES (2, '004_investigator_foundation', strftime('%s','now'))
ON CONFLICT(version) DO NOTHING;
