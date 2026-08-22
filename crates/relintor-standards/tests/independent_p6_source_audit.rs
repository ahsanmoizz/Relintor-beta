use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use ed25519_dalek::SigningKey;
use relintor_standards::*;
use rusqlite::Connection;
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

fn signed_registry() -> (StandardsRegistry, TrustedSignerSet) {
    let key = SigningKey::from_bytes(&[23_u8; 32]);
    let mut registry = builtin_registry();
    registry.sign("independent-p6-audit", &key).unwrap();
    let trusted = TrustedSignerSet {
        signers: vec![TrustedSigner {
            key_id: "independent-p6-audit".into(),
            algorithm: "Ed25519".into(),
            public_key: BASE64.encode(key.verifying_key().to_bytes()),
        }],
    };
    (registry, trusted)
}

fn context(fields: &[&str]) -> ApplicabilityContext {
    // Final P6 preseal requires every authority field to be resolved. The
    // fixture keeps the original selected fields true and explicitly resolves
    // every other domain false; the sparse unknown fixture remains separate.
    let mut facts = BTreeMap::from([
        ("web".into(), FactValue::Bool(false)),
        ("backend".into(), FactValue::Bool(false)),
        ("database".into(), FactValue::Bool(false)),
        ("authentication".into(), FactValue::Bool(false)),
        ("ui_surface".into(), FactValue::Bool(false)),
        ("seo_relevance".into(), FactValue::Bool(false)),
        ("performance".into(), FactValue::Bool(false)),
        ("deployment".into(), FactValue::Bool(false)),
        ("observability".into(), FactValue::Bool(false)),
        ("privacy".into(), FactValue::Bool(false)),
        ("payments".into(), FactValue::Bool(false)),
        ("ai".into(), FactValue::Bool(false)),
        ("blockchain".into(), FactValue::Bool(false)),
        ("mobile".into(), FactValue::Bool(false)),
        ("desktop".into(), FactValue::Bool(false)),
        ("data_engineering".into(), FactValue::Bool(false)),
        ("integrations".into(), FactValue::Bool(false)),
    ]);
    for field in fields {
        facts.insert((*field).to_string(), FactValue::Bool(true));
    }
    facts.insert("platform".into(), FactValue::Text("fixture".into()));
    ApplicabilityContext {
        revision: "independent-audit-facts-v1".into(),
        facts,
    }
}

fn empty_context() -> ApplicabilityContext {
    ApplicabilityContext {
        revision: "independent-audit-unknown-v1".into(),
        facts: BTreeMap::new(),
    }
}

fn draft_for(
    mission_id: &str,
    fields: &[&str],
) -> (AuthorityEngine, MissionDraft, TrustedSignerSet) {
    let (registry, trusted) = signed_registry();
    let engine = AuthorityEngine;
    let draft = engine
        .build_draft(
            mission_id,
            "project-independent-audit",
            "source-revision-1",
            "project-fingerprint-1",
            registry,
            &context(fields),
            scope_fingerprint(BTreeMap::from([(
                String::from("root"),
                String::from("fixture"),
            )])),
            Vec::new(),
        )
        .unwrap();
    (engine, draft, trusted)
}

fn sqlite_path(label: &str) -> PathBuf {
    let base = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    fs::create_dir_all(&base).unwrap();
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_nanos())
        .unwrap_or_default();
    base.join(format!(
        "relintor-p6-independent-{label}-{}-{nonce}.sqlite",
        std::process::id()
    ))
}

fn init_authority_schema(path: &PathBuf) {
    let connection = Connection::open(path).unwrap();
    connection
        .execute_batch(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../db/migrations/006_authority_foundation.sql"
        )))
        .unwrap();
}

fn first_critical_requirement(draft: &MissionDraft) -> (String, String) {
    for pack in &draft.registry.packs {
        for rule in &pack.rules {
            if rule.severity != RuleSeverity::Critical {
                continue;
            }
            if let Some(result) = draft
                .applicability
                .iter()
                .find(|result| result.rule_id == rule.rule_id)
            {
                if result.outcome == ApplicabilityOutcome::Applicable {
                    return (
                        result
                            .requirement_id
                            .clone()
                            .expect("critical requirement id"),
                        rule.rule_id.clone(),
                    );
                }
            }
        }
    }
    panic!("fixture did not produce an applicable critical rule");
}

#[test]
fn missing_authority_fact_is_unknown_not_silent_na() {
    let (registry, _) = signed_registry();
    let (ledger, _) = AuthorityEngine
        .evaluate(&registry, &empty_context())
        .unwrap();

    let backend = ledger
        .iter()
        .find(|result| result.rule_id.contains("BACKEND_API"))
        .expect("backend rule");

    assert!(
        matches!(
            backend.outcome,
            ApplicabilityOutcome::BlockedByUnknown | ApplicabilityOutcome::NeedsDecision
        ),
        "missing backend fact was silently classified as {:?}",
        backend.outcome
    );
}

#[test]
fn fully_unknown_project_cannot_seal_an_empty_authority_mission() {
    let (registry, trusted) = signed_registry();
    let engine = AuthorityEngine;
    let draft = engine
        .build_draft(
            "unknown-mission",
            "unknown-project",
            "source-1",
            "fingerprint-1",
            registry,
            &empty_context(),
            scope_fingerprint(BTreeMap::new()),
            Vec::new(),
        )
        .unwrap();

    assert!(
        matches!(
            engine.preseal(&draft, &trusted),
            Err(AuthorityError::UnknownApplicability(_))
        ),
        "an underspecified mission must not seal by converting unknown facts to N/A"
    );
}

#[test]
fn critical_rule_removal_with_matching_task_cleanup_still_blocks_seal() {
    let (engine, mut draft, trusted) = draft_for("critical-removal", &["backend"]);
    let (requirement_id, _rule_id) = first_critical_requirement(&draft);

    draft
        .requirement_graph
        .requirements
        .retain(|requirement| requirement.requirement_id != requirement_id);

    let removed_task_ids = draft
        .task_graph
        .tasks
        .iter()
        .filter(|task| task.requirement_ids.contains(&requirement_id))
        .map(|task| task.task_id.clone())
        .collect::<Vec<_>>();

    draft
        .task_graph
        .tasks
        .retain(|task| !removed_task_ids.contains(&task.task_id));
    draft.task_graph.links.retain(|link| {
        link.requirement_id != requirement_id && !removed_task_ids.contains(&link.task_id)
    });
    draft.task_graph.dependencies.retain(|edge| {
        !removed_task_ids.contains(&edge.task_id) && !removed_task_ids.contains(&edge.depends_on)
    });

    assert!(
        matches!(
            engine.preseal(&draft, &trusted),
            Err(AuthorityError::CriticalRuleMissing(_))
                | Err(AuthorityError::MissingRequirement(_))
        ),
        "applicable critical ledger rule vanished after requirement/task cleanup"
    );
}

#[test]
fn non_waivable_critical_rule_cannot_be_human_deferred() {
    let (registry, _trusted) = signed_registry();
    let engine = AuthorityEngine;
    let (_ledger, graph) = engine.evaluate(&registry, &context(&["backend"])).unwrap();

    let requirement = graph
        .requirements
        .iter()
        .find(|requirement| requirement.risk == RequirementRisk::Critical)
        .expect("critical requirement");

    let decision = ExplicitDecision {
        decision_id: "human-defer-critical".into(),
        kind: DecisionKind::Defer,
        actor: DecisionActor::Human,
        requirement_id: requirement.requirement_id.clone(),
        reason: "try to postpone a critical rule".into(),
        risk: "critical".into(),
        timestamp: "2026-08-14T00:00:00Z".into(),
        mission_revision: 1,
        provenance: "independent audit".into(),
    };

    assert!(
        matches!(
            engine.build_draft(
                "non-waivable",
                "project",
                "source",
                "fingerprint",
                registry,
                &context(&["backend"]),
                scope_fingerprint(BTreeMap::new()),
                vec![decision],
            ),
            Err(AuthorityError::InvalidDecision(_))
        ),
        "NON_WAIVABLE critical rules must reject defer decisions"
    );
}

#[test]
fn task_links_must_match_the_tasks_declared_requirement_ids() {
    let (_engine, mut draft, _trusted) = draft_for("task-link-mismatch", &["backend", "database"]);

    assert!(draft.task_graph.links.len() >= 2);
    let first = draft.task_graph.links[0].requirement_id.clone();
    let second = draft.task_graph.links[1].requirement_id.clone();
    assert_ne!(first, second);

    draft.task_graph.links[0].requirement_id = second;
    draft.task_graph.links[1].requirement_id = first;

    assert!(
        draft.task_graph.validate(&draft.requirement_graph).is_err(),
        "task coverage links can disagree with Task.requirement_ids"
    );
}

#[test]
fn registry_digest_is_independent_of_pack_and_rule_order() {
    let mut original = builtin_registry();
    original.refresh_pack_metadata().unwrap();
    let original_digest = original.calculate_digest().unwrap();

    let mut reordered = original.clone();
    reordered.packs.reverse();
    for pack in &mut reordered.packs {
        pack.rules.reverse();
    }
    reordered.refresh_pack_metadata().unwrap();
    let reordered_digest = reordered.calculate_digest().unwrap();

    assert_eq!(
        original_digest, reordered_digest,
        "semantic pack/rule ordering changed the canonical registry digest"
    );
}

#[test]
fn seal_manifest_and_identity_tampering_invalidates_the_seal() {
    let (engine, draft, trusted) = draft_for("seal-tamper", &["backend"]);
    let (sealed, _) = engine
        .seal(&draft, &trusted, "2026-08-14T00:00:00Z")
        .unwrap();

    let mut manifest_tampered = sealed.clone();
    manifest_tampered.seal.manifest.rule_ids.clear();
    assert!(
        !engine.validate_seal(&manifest_tampered).unwrap().valid,
        "manifest tampering must invalidate the seal"
    );

    let mut identity_tampered = sealed.clone();
    identity_tampered.seal.mission_id = "different-mission".into();
    assert!(
        !engine.validate_seal(&identity_tampered).unwrap().valid,
        "seal mission identity tampering must invalidate the seal"
    );
}

#[test]
fn authority_rows_preserve_stable_ids_across_revisions_and_missions() {
    let path = sqlite_path("stable-ids");
    init_authority_schema(&path);

    let (engine, draft1, trusted) = draft_for("mission-a", &["backend"]);
    let registry = draft1.registry.clone();
    let (revision1, _) = engine
        .seal(&draft1, &trusted, "2026-08-14T00:00:00Z")
        .unwrap();
    persist_authority(&path, &registry, &revision1).unwrap();

    let mut draft2 = draft1.clone();
    draft2.revision = 2;
    let (revision2, _) = engine
        .seal(&draft2, &trusted, "2026-08-14T00:01:00Z")
        .unwrap();
    persist_authority(&path, &registry, &revision2).unwrap();

    let (_engine_b, draft_b, trusted_b) = draft_for("mission-b", &["backend"]);
    let registry_b = draft_b.registry.clone();
    let (revision_b, _) = AuthorityEngine
        .seal(&draft_b, &trusted_b, "2026-08-14T00:02:00Z")
        .unwrap();
    persist_authority(&path, &registry_b, &revision_b).unwrap();

    let connection = Connection::open(&path).unwrap();

    let revisions_a: i64 = connection
        .query_row(
            "SELECT COUNT(DISTINCT revision) FROM authority_requirements WHERE mission_id = 'mission-a'",
            [],
            |row| row.get(0),
        )
        .unwrap();

    let missions: i64 = connection
        .query_row(
            "SELECT COUNT(DISTINCT mission_id) FROM authority_requirements",
            [],
            |row| row.get(0),
        )
        .unwrap();

    assert_eq!(
        revisions_a, 2,
        "stable requirement IDs must persist for every mission revision"
    );
    assert_eq!(
        missions, 2,
        "stable requirement IDs must persist independently for different missions"
    );

    drop(connection);
    let _ = fs::remove_file(path);
}

#[test]
fn persistence_rejects_a_tampered_revision_before_writing() {
    let path = sqlite_path("tampered-persist");
    init_authority_schema(&path);

    let (engine, draft, trusted) = draft_for("tampered-persist", &["backend"]);
    let registry = draft.registry.clone();
    let (mut sealed, _) = engine
        .seal(&draft, &trusted, "2026-08-14T00:00:00Z")
        .unwrap();

    sealed.contract.project_id.push_str("-tampered");

    assert!(
        persist_authority(&path, &registry, &sealed).is_err(),
        "persistence accepted a contract whose canonical hash no longer matches its seal"
    );

    let _ = fs::remove_file(path);
}

#[test]
fn persisted_contract_tampering_is_rejected_on_load() {
    let path = sqlite_path("tampered-load");
    init_authority_schema(&path);

    let (engine, draft, trusted) = draft_for("tampered-load", &["backend"]);
    let registry = draft.registry.clone();
    let (sealed, _) = engine
        .seal(&draft, &trusted, "2026-08-14T00:00:00Z")
        .unwrap();
    persist_authority(&path, &registry, &sealed).unwrap();

    let mut tampered_contract = sealed.contract.clone();
    tampered_contract.project_id.push_str("-tampered");
    let tampered_json = serde_json::to_string(&tampered_contract).unwrap();

    let connection = Connection::open(&path).unwrap();
    connection
        .execute(
            "UPDATE mission_revisions SET contract_json = ?1 WHERE mission_id = ?2 AND revision = ?3",
            rusqlite::params![tampered_json, &sealed.seal.mission_id, sealed.revision],
        )
        .unwrap();
    drop(connection);

    assert!(
        load_mission_revision(&path, &sealed.seal.mission_id, sealed.revision).is_err(),
        "validated load returned a contract that no longer matches its persisted seal"
    );

    let _ = fs::remove_file(path);
}
