use base64::{engine::general_purpose::STANDARD, Engine as _};
use ed25519_dalek::SigningKey;
use relintor_evidence::{
    CollectorBinding, CommandSpec, CompletionState, EvidenceStore, FreshnessContext, TestCollector,
    VerificationAuthority, VerificationEngine,
};
use relintor_standards::{
    builtin_registry, scope_fingerprint, AcceptanceCriterion, ApplicabilityContext,
    AuthorityEngine, EvidenceClass, EvidenceConfidence, EvidenceObligation, FactValue,
    ProjectAuthorityInput, ProjectRequirementSeed, RequirementPriority, RequirementRisk,
    RequirementSource, TrustedSigner, TrustedSignerSet, VerificationPolicy,
};
use std::collections::BTreeMap;
use tempfile::tempdir;

fn context() -> ApplicabilityContext {
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
        facts.insert(field.into(), FactValue::Bool(false));
    }
    facts.insert("platform".into(), FactValue::Text("windows".into()));
    ApplicabilityContext {
        facts,
        revision: "p8-certification-guard".into(),
    }
}

#[cfg(target_os = "windows")]
fn one_test_command(dir: &std::path::Path) -> CommandSpec {
    CommandSpec {
        program: "cmd".into(),
        args: vec!["/C".into(), "echo test login_only ... ok".into()],
        working_directory: dir.to_path_buf(),
        environment: BTreeMap::new(),
    }
}

#[cfg(not(target_os = "windows"))]
fn one_test_command(dir: &std::path::Path) -> CommandSpec {
    CommandSpec {
        program: "sh".into(),
        args: vec!["-c".into(), "echo 'test login_only ... ok'".into()],
        working_directory: dir.to_path_buf(),
        environment: BTreeMap::new(),
    }
}

#[test]
fn one_generic_test_pass_cannot_prove_two_distinct_functional_criteria() {
    let signing = SigningKey::from_bytes(&[119_u8; 32]);
    let mut registry = builtin_registry();
    registry.sign("p8-certification-guard", &signing).unwrap();
    let trusted = TrustedSignerSet {
        signers: vec![TrustedSigner {
            key_id: "p8-certification-guard".into(),
            algorithm: "Ed25519".into(),
            public_key: STANDARD.encode(signing.verifying_key().to_bytes()),
        }],
    };

    let requirement = ProjectRequirementSeed {
        requirement_id: Some("P8-CRITERION-SPECIFICITY".into()),
        title: "Two separate functional behaviors".into(),
        intent: "Verify login rejection and refund behavior independently".into(),
        source: RequirementSource::User {
            reference: "audit://criterion-specificity".into(),
        },
        priority: RequirementPriority::P1,
        acceptance_criteria: vec![
            AcceptanceCriterion {
                criterion_id: "login-rejects-invalid".into(),
                statement: "Invalid login is rejected".into(),
                criterion_type: "functional".into(),
                machine_checkable: true,
            },
            AcceptanceCriterion {
                criterion_id: "refund-succeeds".into(),
                statement: "Refund succeeds".into(),
                criterion_type: "functional".into(),
                machine_checkable: true,
            },
        ],
        verification_policy: VerificationPolicy {
            obligations: vec![EvidenceObligation {
                class: EvidenceClass::TestOutput,
                minimum_confidence: EvidenceConfidence::StrongDeterministic,
                rationale: "functional test proof".into(),
                required: true,
            }],
            p8_collector_required: true,
        },
        dependencies: vec![],
        risk: RequirementRisk::Medium,
        requirement_type: "functional".into(),
    };

    let input = ProjectAuthorityInput {
        requirements: vec![requirement],
        source_revision: "source-v1".into(),
        source_fingerprint: "source-fingerprint".into(),
        ..ProjectAuthorityInput::default()
    };

    let draft = AuthorityEngine
        .build_draft_with_project_authority(
            "mission-p8-certification-guard",
            "project-p8-certification-guard",
            "source-v1",
            "workspace-v1",
            registry.clone(),
            &context(),
            scope_fingerprint(BTreeMap::from([("root".into(), "guard".into())])),
            input,
            vec![],
        )
        .unwrap();

    let (revision, handoff) = AuthorityEngine
        .seal(&draft, &trusted, "2026-08-16T00:00:00Z")
        .unwrap();

    let root = tempdir().unwrap();
    let authority = VerificationAuthority {
        revision,
        handoff,
        registry,
        trusted_signers: trusted,
        p7_run_id: "p7-run".into(),
        p7_state: "EXECUTION_TASKS_FINISHED_AWAITING_VERIFICATION".into(),
        workspace_fingerprint: "workspace-fingerprint".into(),
        source_revision: Some("source-v1".into()),
        environment_fingerprint: "environment".into(),
    };
    let current = FreshnessContext {
        mission_id: authority.revision.seal.mission_id.clone(),
        mission_revision: authority.revision.revision,
        p6_seal_hash: authority.revision.seal.contract_hash.clone(),
        workspace_fingerprint: authority.workspace_fingerprint.clone(),
        source_revision: authority.source_revision.clone(),
        environment_fingerprint: authority.environment_fingerprint.clone(),
        dependency_lock_hashes: BTreeMap::new(),
        workspace_root: Some(root.path().to_path_buf()),
    };

    let store = EvidenceStore::new(root.path().join("evidence"), b"guard-key").unwrap();
    let binding = CollectorBinding::for_requirement(
        &authority,
        &current,
        "single-functional-test",
        "P8-CRITERION-SPECIFICITY",
        EvidenceClass::TestOutput,
    )
    .unwrap();

    let collected = TestCollector::default()
        .collect(
            &binding,
            &one_test_command(root.path()),
            "rust-test-like-fixture",
            Default::default(),
        )
        .unwrap();

    store
        .put(collected.receipt, &collected.artifact_bytes)
        .unwrap();

    let report = VerificationEngine::new_for_test(store, authority, current, vec![])
        .unwrap()
        .evaluate(None)
        .unwrap();

    assert_ne!(
        report.decision.state,
        CompletionState::VerifiedComplete,
        "one unrelated generic test receipt proved two independent functional criteria"
    );

    let requirement = &report.requirement_statuses[0];
    assert!(
        !requirement.missing_acceptance_criteria.is_empty(),
        "all same-class criteria were automatically claimed by a single generic test receipt"
    );
}
