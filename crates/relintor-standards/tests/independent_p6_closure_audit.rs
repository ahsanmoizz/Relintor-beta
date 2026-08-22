use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use ed25519_dalek::SigningKey;
use relintor_standards::*;
use std::collections::BTreeMap;

fn signed_registry() -> (StandardsRegistry, TrustedSignerSet) {
    let key = SigningKey::from_bytes(&[41_u8; 32]);
    let mut registry = builtin_registry();
    registry.sign("p6-closure-audit", &key).unwrap();
    let trusted = TrustedSignerSet {
        signers: vec![TrustedSigner {
            key_id: "p6-closure-audit".into(),
            algorithm: "Ed25519".into(),
            public_key: BASE64.encode(key.verifying_key().to_bytes()),
        }],
    };
    (registry, trusted)
}

fn complete_context() -> ApplicabilityContext {
    let mut facts = BTreeMap::new();
    for field in [
        "web",
        "backend",
        "database",
        "authentication",
        "ui_surface",
        "seo_relevance",
        "performance",
        "deployment",
        "observability",
        "privacy",
        "payments",
        "ai",
        "blockchain",
        "mobile",
        "desktop",
        "data_engineering",
        "integrations",
    ] {
        facts.insert(field.to_string(), FactValue::Bool(false));
    }
    facts.insert("platform".into(), FactValue::Text("fixture".into()));
    ApplicabilityContext {
        facts,
        revision: "p6-closure-context".into(),
    }
}

#[test]
fn human_deferred_project_requirement_seals_but_is_not_executable() {
    let (registry, trusted) = signed_registry();
    let engine = AuthorityEngine;

    let seed = ProjectRequirementSeed {
        requirement_id: Some("project-deferable".into()),
        title: "Optional launch integration".into(),
        intent: "Implement the optional launch integration.".into(),
        source: RequirementSource::User {
            reference: "user://project-deferable".into(),
        },
        priority: RequirementPriority::P2,
        acceptance_criteria: vec![AcceptanceCriterion {
            criterion_id: "project-deferable-acceptance".into(),
            statement: "A runtime test demonstrates the integration.".into(),
            criterion_type: "functional".into(),
            machine_checkable: true,
        }],
        verification_policy: VerificationPolicy {
            obligations: vec![EvidenceObligation {
                class: EvidenceClass::TestOutput,
                minimum_confidence: EvidenceConfidence::StrongRuntime,
                rationale: "Functional behavior requires runtime proof.".into(),
                required: true,
            }],
            p8_collector_required: true,
        },
        dependencies: Vec::new(),
        risk: RequirementRisk::Medium,
        requirement_type: "functional".into(),
    };

    let decision = ExplicitDecision {
        decision_id: "defer-project-deferable".into(),
        kind: DecisionKind::Defer,
        actor: DecisionActor::User,
        requirement_id: "project-deferable".into(),
        reason: "Explicitly deferred to a later mission revision.".into(),
        risk: "accepted for this revision".into(),
        timestamp: "2026-08-15T00:00:00Z".into(),
        mission_revision: 1,
        provenance: "user://mission-review".into(),
    };

    let authority = ProjectAuthorityInput {
        requirements: vec![seed],
        architecture_decisions: Vec::new(),
        assumptions: Vec::new(),
        risks: Vec::new(),
        decisions: vec![decision],
        source_revision: "project-authority-v1".into(),
        source_fingerprint: "project-authority-fingerprint-v1".into(),
    };

    let draft = engine
        .build_draft_with_project_authority(
            "mission-project-defer",
            "project-defer",
            "source-v1",
            "project-fingerprint-v1",
            registry,
            &complete_context(),
            scope_fingerprint(BTreeMap::from([("project".into(), "project-defer".into())])),
            authority,
            Vec::new(),
        )
        .expect("build project draft with explicit human defer");

    engine
        .preseal(&draft, &trusted)
        .expect("a valid explicit project defer must survive preseal");

    let (sealed, handoff) = engine
        .seal(&draft, &trusted, "2026-08-15T00:00:00Z")
        .expect("seal project with explicit defer");
    let canonical_requirement_id = sealed
        .contract
        .project_authority
        .requirements
        .first()
        .and_then(|seed| seed.requirement_id.clone())
        .expect("canonical project requirement ID");

    let requirement = sealed
        .contract
        .requirement_graph
        .requirements
        .iter()
        .find(|requirement| requirement.requirement_id == canonical_requirement_id)
        .expect("deferred requirement remains accounted");

    assert_eq!(
        requirement.status,
        RequirementStatus::DeferredByExplicitDecision
    );

    let linked_task_ids = sealed
        .contract
        .task_graph
        .links
        .iter()
        .filter(|link| link.requirement_id == canonical_requirement_id)
        .map(|link| link.task_id.clone())
        .collect::<Vec<_>>();

    assert!(
        !linked_task_ids.is_empty(),
        "deferred requirement should remain traceable to its task authority"
    );

    for task_id in linked_task_ids {
        let task = sealed
            .contract
            .task_graph
            .tasks
            .iter()
            .find(|task| task.task_id == task_id)
            .expect("linked task");
        assert!(
            matches!(
                task.status,
                RequirementStatus::DeferredByExplicitDecision | RequirementStatus::NotApplicable
            ),
            "task linked only to a deferred requirement is still executable: {:?}",
            task.status
        );
        assert!(
            !handoff.task_order.contains(&task.task_id),
            "deferred task leaked into READY_FOR_EXECUTION handoff"
        );
    }
}
