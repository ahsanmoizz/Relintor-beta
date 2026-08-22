use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use ed25519_dalek::SigningKey;
use relintor_standards::*;
use rusqlite::Connection;
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

fn signed_registry() -> (StandardsRegistry, TrustedSignerSet, SigningKey) {
    let key = SigningKey::from_bytes(&[11_u8; 32]);
    let mut registry = builtin_registry();
    registry.sign("p6-test", &key).unwrap();
    let trusted = TrustedSignerSet {
        signers: vec![TrustedSigner {
            key_id: "p6-test".into(),
            algorithm: "Ed25519".into(),
            public_key: BASE64.encode(key.verifying_key().to_bytes()),
        }],
    };
    (registry, trusted, key)
}

fn context(fields: &[&str]) -> ApplicabilityContext {
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
        revision: "fixture-facts-v1".into(),
        facts,
    }
}

fn draft_for(fields: &[&str]) -> (AuthorityEngine, MissionDraft, TrustedSignerSet) {
    let (registry, trusted, _) = signed_registry();
    let engine = AuthorityEngine;
    let draft = engine
        .build_draft(
            "mission-fixture",
            "project-fixture",
            "source-revision-1",
            "takeover-fingerprint-1",
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

#[test]
fn all_eighteen_domain_packs_load_with_real_rules() {
    let registry = builtin_registry();
    assert_eq!(registry.packs.len(), 18);
    assert_eq!(
        registry
            .packs
            .iter()
            .map(|pack| pack.rules.len())
            .sum::<usize>(),
        36
    );
    assert!(registry.packs.iter().all(|pack| !pack.rules.is_empty()));
}

#[test]
fn irrelevant_rule_is_na_and_not_a_requirement() {
    let (registry, _, _) = signed_registry();
    let (ledger, graph) = AuthorityEngine
        .evaluate(&registry, &context(&["backend"]))
        .unwrap();
    let seo = ledger
        .iter()
        .find(|result| result.rule_id.contains("SEO_DISCOVERABILITY"))
        .unwrap();
    assert_eq!(seo.outcome, ApplicabilityOutcome::NotApplicable);
    assert!(seo.requirement_id.is_none());
    assert!(!graph
        .requirements
        .iter()
        .any(|requirement| requirement.requirement_type == "seo-discoverability"));
}

#[test]
fn critical_applicable_rule_cannot_disappear_before_sealing() {
    let (engine, mut draft, trusted) = draft_for(&["backend", "authentication"]);
    let removed = draft
        .requirement_graph
        .requirements
        .remove(0)
        .requirement_id;
    assert!(!draft
        .task_graph
        .links
        .iter()
        .all(|link| link.requirement_id != removed));
    assert!(engine.preseal(&draft, &trusted).is_err());
}

#[test]
fn changed_sealed_requirement_requires_revalidation() {
    let (engine, draft, trusted) = draft_for(&["desktop"]);
    let (sealed, _) = engine
        .seal(&draft, &trusted, "2026-08-14T00:00:00Z")
        .unwrap();
    let mut candidate = sealed.contract.clone();
    candidate.requirement_graph.requirements[0].acceptance_criteria[0]
        .statement
        .push_str(" changed");
    let result = engine.revalidate(&sealed, &candidate).unwrap();
    assert_eq!(result.status, "REVALIDATION_REQUIRED");
    assert!(result.new_revision_required);
}

#[test]
fn missing_acceptance_or_evidence_blocks_seal() {
    let (engine, mut draft, trusted) = draft_for(&["backend"]);
    draft.requirement_graph.requirements[0]
        .acceptance_criteria
        .clear();
    assert!(matches!(
        engine.preseal(&draft, &trusted),
        Err(AuthorityError::MissingAcceptance(_))
    ));
    let (engine, mut draft, trusted) = draft_for(&["backend"]);
    draft.requirement_graph.requirements[0]
        .verification_policy
        .obligations
        .clear();
    assert!(matches!(
        engine.preseal(&draft, &trusted),
        Err(AuthorityError::MissingEvidencePolicy(_))
    ));
}

#[test]
fn signed_registry_update_and_tamper_checks_are_fail_closed() {
    let (registry, trusted, key) = signed_registry();
    registry.verify(&trusted, true).unwrap();
    let mut tampered = registry.clone();
    tampered.packs[0].rules[0].rationale.push('x');
    assert!(tampered.verify(&trusted, true).is_err());
    let mut v2 = registry.clone();
    v2.registry_version = 2;
    v2.packs[0].rules[0].title.push_str(" v2");
    v2.sign("p6-test", &key).unwrap();
    let update = apply_signed_update(Some(&registry), v2, &trusted).unwrap();
    assert_eq!(update.registry.registry_version, 2);
    assert_eq!(registry.registry_version, 1);
    assert_ne!(registry.registry_digest, update.registry.registry_digest);
}

#[test]
fn requirement_graph_cycles_are_rejected() {
    let (engine, mut draft, _) = draft_for(&["backend"]);
    let ids = draft
        .requirement_graph
        .requirements
        .iter()
        .take(2)
        .map(|requirement| requirement.requirement_id.clone())
        .collect::<Vec<_>>();
    draft.requirement_graph.dependencies = vec![
        RequirementDependency {
            requirement_id: ids[0].clone(),
            depends_on: ids[1].clone(),
            reason: "first".into(),
            dependency_type: "build".into(),
        },
        RequirementDependency {
            requirement_id: ids[1].clone(),
            depends_on: ids[0].clone(),
            reason: "cycle".into(),
            dependency_type: "build".into(),
        },
    ];
    assert!(matches!(
        draft.requirement_graph.validate(),
        Err(AuthorityError::DependencyCycle(_))
    ));
    let _ = engine;
}

#[test]
fn task_graph_cycles_and_missing_coverage_are_rejected() {
    let (_engine, mut draft, _trusted) = draft_for(&["backend", "authentication"]);
    let ids = draft
        .task_graph
        .tasks
        .iter()
        .take(2)
        .map(|task| task.task_id.clone())
        .collect::<Vec<_>>();
    draft.task_graph.dependencies = vec![
        TaskDependency {
            task_id: ids[0].clone(),
            depends_on: ids[1].clone(),
            reason: "first".into(),
        },
        TaskDependency {
            task_id: ids[1].clone(),
            depends_on: ids[0].clone(),
            reason: "cycle".into(),
        },
    ];
    assert!(matches!(
        draft.task_graph.validate(&draft.requirement_graph),
        Err(AuthorityError::TaskCycle(_))
    ));
    let (engine, mut draft, trusted) = draft_for(&["backend"]);
    draft.task_graph.links.clear();
    assert!(matches!(
        engine.preseal(&draft, &trusted),
        Err(AuthorityError::MissingTaskCoverage(_))
    ));
}

#[test]
fn task_to_requirement_traceability_is_materialized() {
    let (engine, draft, trusted) = draft_for(&["backend", "database"]);
    engine.preseal(&draft, &trusted).unwrap();
    assert!(!draft.task_graph.tasks.is_empty());
    assert_eq!(
        draft
            .requirement_graph
            .requirements
            .iter()
            .filter(|requirement| requirement.applicability == ApplicabilityOutcome::Applicable)
            .count(),
        draft.task_graph.links.len()
    );
}

#[test]
fn explicit_defer_requires_human_and_remains_in_accounting() {
    let (registry, trusted, _) = signed_registry();
    let engine = AuthorityEngine;
    let (ledger, graph) = engine.evaluate(&registry, &context(&["backend"])).unwrap();
    let requirement_id = ledger
        .iter()
        .find_map(|result| result.requirement_id.clone())
        .unwrap();
    let ai_decision = ExplicitDecision {
        decision_id: "ai-defer".into(),
        kind: DecisionKind::Defer,
        actor: DecisionActor::Ai,
        requirement_id: requirement_id.clone(),
        reason: "later".into(),
        risk: "high".into(),
        timestamp: "now".into(),
        mission_revision: 1,
        provenance: "ai".into(),
    };
    assert!(matches!(
        engine.build_draft(
            "ai",
            "p",
            "s",
            "f",
            registry.clone(),
            &context(&["backend"]),
            scope_fingerprint(BTreeMap::new()),
            vec![ai_decision]
        ),
        Err(AuthorityError::InvalidDecision(_))
    ));
    let human_decision = ExplicitDecision {
        decision_id: "human-defer".into(),
        kind: DecisionKind::Defer,
        actor: DecisionActor::Human,
        requirement_id,
        reason: "scheduled after launch".into(),
        risk: "accepted".into(),
        timestamp: "now".into(),
        mission_revision: 1,
        provenance: "founder review".into(),
    };
    let draft = engine
        .build_draft(
            "human",
            "p",
            "s",
            "f",
            registry,
            &context(&["backend"]),
            scope_fingerprint(BTreeMap::new()),
            vec![human_decision],
        )
        .unwrap();
    assert!(draft
        .requirement_graph
        .requirements
        .iter()
        .any(|requirement| requirement.status == RequirementStatus::DeferredByExplicitDecision));
    let _ = (trusted, graph);
}

#[test]
fn mission_hash_is_order_independent_and_round_trips() {
    let (engine, draft, trusted) = draft_for(&["web", "backend"]);
    let (sealed, handoff) = engine.seal(&draft, &trusted, "fixed").unwrap();
    let mut reordered = sealed.contract.clone();
    reordered.requirement_graph.requirements.reverse();
    reordered.task_graph.tasks.reverse();
    assert_eq!(
        sealed.contract.canonical_hash().unwrap(),
        reordered.canonical_hash().unwrap()
    );
    let encoded = serde_json::to_vec(&sealed).unwrap();
    let decoded: MissionRevision = serde_json::from_slice(&encoded).unwrap();
    assert_eq!(decoded.seal.contract_hash, sealed.seal.contract_hash);
    assert_eq!(handoff.state, "READY_FOR_EXECUTION");
}

#[test]
fn fifteen_profiles_execute_through_applicability_engine() {
    let (registry, _, _) = signed_registry();
    let profiles = [
        ["web", "seo_relevance", "ui_surface"].as_slice(),
        ["web", "backend", "authentication", "database"].as_slice(),
        ["backend", "authentication", "database"].as_slice(),
        ["backend"].as_slice(),
        ["backend", "database", "payments", "authentication"].as_slice(),
        ["web", "backend", "ai", "privacy"].as_slice(),
        ["web", "backend", "blockchain"].as_slice(),
        ["mobile", "authentication"].as_slice(),
        ["desktop"].as_slice(),
        ["data_engineering", "database", "observability"].as_slice(),
        ["privacy", "database", "authentication"].as_slice(),
        ["backend", "integrations"].as_slice(),
        ["backend"].as_slice(),
        ["backend", "authentication"].as_slice(),
        [
            "web",
            "backend",
            "deployment",
            "observability",
            "performance",
        ]
        .as_slice(),
    ];
    for profile in profiles {
        let (ledger, graph) = AuthorityEngine
            .evaluate(&registry, &context(profile))
            .unwrap();
        assert_eq!(ledger.len(), 36);
        assert!(ledger
            .iter()
            .any(|result| result.outcome == ApplicabilityOutcome::Applicable));
        assert!(graph.requirements.iter().all(|requirement| {
            ledger
                .iter()
                .any(|result| result.requirement_id.as_deref() == Some(&requirement.requirement_id))
        }));
    }
}

#[test]
fn authority_registry_and_sealed_revision_persist_and_reload() {
    let (engine, draft, trusted) = draft_for(&["backend", "database"]);
    let (registry, _, _) = signed_registry();
    let (sealed, _) = engine.seal(&draft, &trusted, "fixed").unwrap();
    let path = PathBuf::from("D:\\Relintor\\target\\p6-authority-persistence.sqlite");
    let _ = fs::remove_file(&path);
    let connection = Connection::open(&path).unwrap();
    connection
        .execute_batch(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../db/migrations/006_authority_foundation.sql"
        )))
        .unwrap();
    drop(connection);
    persist_authority(&path, &registry, &sealed).unwrap();
    let loaded = load_mission_revision(&path, &sealed.seal.mission_id, sealed.revision).unwrap();
    assert_eq!(loaded.seal.contract_hash, sealed.seal.contract_hash);
    let connection = Connection::open(&path).unwrap();
    let registry_count: i64 = connection
        .query_row("SELECT COUNT(*) FROM standards_registries", [], |row| {
            row.get(0)
        })
        .unwrap();
    let seal_count: i64 = connection
        .query_row("SELECT COUNT(*) FROM mission_seals", [], |row| row.get(0))
        .unwrap();
    assert_eq!(registry_count, 1);
    assert_eq!(seal_count, 1);
    drop(connection);
    let _ = fs::remove_file(path);
}
