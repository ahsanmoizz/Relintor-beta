-- Additive P6 standards, requirement graph, mission and sealing persistence.
-- Private signing keys are never persisted here.

CREATE TABLE IF NOT EXISTS standards_registries (
    registry_id TEXT NOT NULL,
    registry_version INTEGER NOT NULL,
    schema_version INTEGER NOT NULL,
    registry_digest TEXT NOT NULL,
    signer_key_id TEXT,
    signature_algorithm TEXT,
    signature TEXT,
    created_at TEXT NOT NULL,
    registry_json TEXT NOT NULL,
    PRIMARY KEY (registry_id, registry_version)
);

CREATE TABLE IF NOT EXISTS standards_packs (
    registry_id TEXT NOT NULL,
    registry_version INTEGER NOT NULL,
    pack_id TEXT NOT NULL,
    pack_version TEXT NOT NULL,
    domain TEXT NOT NULL,
    pack_digest TEXT NOT NULL,
    rule_count INTEGER NOT NULL,
    PRIMARY KEY (registry_id, registry_version, pack_id),
    FOREIGN KEY (registry_id, registry_version)
        REFERENCES standards_registries(registry_id, registry_version)
        ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS standards_rules (
    registry_id TEXT NOT NULL,
    registry_version INTEGER NOT NULL,
    pack_id TEXT NOT NULL,
    rule_id TEXT NOT NULL,
    rule_version TEXT NOT NULL,
    severity TEXT NOT NULL,
    applicability TEXT NOT NULL,
    rule_json TEXT NOT NULL,
    PRIMARY KEY (registry_id, registry_version, rule_id),
    FOREIGN KEY (registry_id, registry_version, pack_id)
        REFERENCES standards_packs(registry_id, registry_version, pack_id)
        ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS applicability_evaluations (
    evaluation_id TEXT PRIMARY KEY,
    mission_id TEXT,
    rule_id TEXT NOT NULL,
    outcome TEXT NOT NULL,
    reason TEXT NOT NULL,
    facts_json TEXT NOT NULL,
    predicate_version TEXT NOT NULL,
    evaluated_at_revision TEXT NOT NULL,
    requirement_id TEXT
);

CREATE TABLE IF NOT EXISTS authority_requirements (
    requirement_id TEXT PRIMARY KEY,
    mission_id TEXT NOT NULL,
    revision INTEGER NOT NULL,
    status TEXT NOT NULL,
    applicability TEXT NOT NULL,
    origin_rule_id TEXT,
    requirement_json TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS authority_requirement_dependencies (
    mission_id TEXT NOT NULL,
    revision INTEGER NOT NULL,
    requirement_id TEXT NOT NULL,
    depends_on TEXT NOT NULL,
    edge_json TEXT NOT NULL,
    PRIMARY KEY (mission_id, revision, requirement_id, depends_on)
);

CREATE TABLE IF NOT EXISTS authority_acceptance_criteria (
    mission_id TEXT NOT NULL,
    revision INTEGER NOT NULL,
    requirement_id TEXT NOT NULL,
    criterion_id TEXT NOT NULL,
    criterion_json TEXT NOT NULL,
    PRIMARY KEY (mission_id, revision, requirement_id, criterion_id)
);

CREATE TABLE IF NOT EXISTS authority_evidence_policies (
    mission_id TEXT NOT NULL,
    revision INTEGER NOT NULL,
    requirement_id TEXT NOT NULL,
    evidence_class TEXT NOT NULL,
    policy_json TEXT NOT NULL,
    PRIMARY KEY (mission_id, revision, requirement_id, evidence_class)
);

CREATE TABLE IF NOT EXISTS authority_tasks (
    task_id TEXT PRIMARY KEY,
    mission_id TEXT NOT NULL,
    revision INTEGER NOT NULL,
    status TEXT NOT NULL,
    task_json TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS authority_task_dependencies (
    mission_id TEXT NOT NULL,
    revision INTEGER NOT NULL,
    task_id TEXT NOT NULL,
    depends_on TEXT NOT NULL,
    dependency_json TEXT NOT NULL,
    PRIMARY KEY (mission_id, revision, task_id, depends_on)
);

CREATE TABLE IF NOT EXISTS authority_task_links (
    mission_id TEXT NOT NULL,
    revision INTEGER NOT NULL,
    task_id TEXT NOT NULL,
    requirement_id TEXT NOT NULL,
    PRIMARY KEY (mission_id, revision, task_id, requirement_id)
);

CREATE TABLE IF NOT EXISTS authority_decisions (
    decision_id TEXT PRIMARY KEY,
    mission_id TEXT NOT NULL,
    revision INTEGER NOT NULL,
    decision_json TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS mission_drafts (
    mission_id TEXT NOT NULL,
    revision INTEGER NOT NULL,
    project_id TEXT NOT NULL,
    scope_fingerprint TEXT NOT NULL,
    draft_json TEXT NOT NULL,
    PRIMARY KEY (mission_id, revision)
);

CREATE TABLE IF NOT EXISTS mission_revisions (
    mission_id TEXT NOT NULL,
    revision INTEGER NOT NULL,
    contract_hash TEXT NOT NULL,
    contract_json TEXT NOT NULL,
    created_at TEXT NOT NULL,
    PRIMARY KEY (mission_id, revision)
);

CREATE TABLE IF NOT EXISTS mission_seals (
    mission_id TEXT NOT NULL,
    revision INTEGER NOT NULL,
    contract_hash TEXT NOT NULL,
    manifest_json TEXT NOT NULL,
    seal_json TEXT NOT NULL,
    PRIMARY KEY (mission_id, revision),
    FOREIGN KEY (mission_id, revision)
        REFERENCES mission_revisions(mission_id, revision)
        ON DELETE RESTRICT
);

CREATE TABLE IF NOT EXISTS mission_revalidation (
    revalidation_id TEXT PRIMARY KEY,
    mission_id TEXT NOT NULL,
    from_revision INTEGER NOT NULL,
    to_revision INTEGER,
    status TEXT NOT NULL,
    reasons_json TEXT NOT NULL,
    created_at TEXT NOT NULL
);
