-- Additive P6 persistence repair. Migration 006 remains unchanged.
-- Stable authority IDs are scoped by mission and revision so historical
-- revisions and multiple projects can retain the same logical identifiers.

ALTER TABLE authority_requirements RENAME TO authority_requirements_legacy_006;
CREATE TABLE authority_requirements (
    requirement_id TEXT NOT NULL,
    mission_id TEXT NOT NULL,
    revision INTEGER NOT NULL,
    status TEXT NOT NULL,
    applicability TEXT NOT NULL,
    origin_rule_id TEXT,
    requirement_json TEXT NOT NULL,
    PRIMARY KEY (mission_id, revision, requirement_id)
);
INSERT INTO authority_requirements(requirement_id, mission_id, revision, status, applicability, origin_rule_id, requirement_json)
    SELECT requirement_id, mission_id, revision, status, applicability, origin_rule_id, requirement_json
    FROM authority_requirements_legacy_006;
DROP TABLE authority_requirements_legacy_006;

ALTER TABLE authority_tasks RENAME TO authority_tasks_legacy_006;
CREATE TABLE authority_tasks (
    task_id TEXT NOT NULL,
    mission_id TEXT NOT NULL,
    revision INTEGER NOT NULL,
    status TEXT NOT NULL,
    task_json TEXT NOT NULL,
    PRIMARY KEY (mission_id, revision, task_id)
);
INSERT INTO authority_tasks(task_id, mission_id, revision, status, task_json)
    SELECT task_id, mission_id, revision, status, task_json
    FROM authority_tasks_legacy_006;
DROP TABLE authority_tasks_legacy_006;

ALTER TABLE authority_decisions RENAME TO authority_decisions_legacy_006;
CREATE TABLE authority_decisions (
    decision_id TEXT NOT NULL,
    mission_id TEXT NOT NULL,
    revision INTEGER NOT NULL,
    decision_json TEXT NOT NULL,
    PRIMARY KEY (mission_id, revision, decision_id)
);
INSERT INTO authority_decisions(decision_id, mission_id, revision, decision_json)
    SELECT decision_id, mission_id, revision, decision_json
    FROM authority_decisions_legacy_006;
DROP TABLE authority_decisions_legacy_006;
