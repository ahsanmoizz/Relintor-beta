use base64::{engine::general_purpose::STANDARD, Engine as _};
use ed25519_dalek::SigningKey;
use relintor_evidence::{
    test_support::put_fixture, EvidenceFreshness, EvidenceMetadata, EvidenceResult, EvidenceStore,
    FreshnessContext, VerificationAuthority, VerificationEngine,
};
use relintor_standards::{
    builtin_registry, scope_fingerprint, AcceptanceCriterion, ApplicabilityContext,
    AuthorityEngine, EvidenceClass, EvidenceConfidence, EvidenceObligation, FactValue,
    ProjectAuthorityInput, ProjectRequirementSeed, RequirementPriority, RequirementRisk,
    RequirementSource, TrustedSigner, TrustedSignerSet, VerificationPolicy,
};
use std::collections::{BTreeMap, BTreeSet};
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
        revision: "p8-cc-e2e".into(),
    }
}

#[test]
fn unrelated_runtime_pass_cannot_prove_non_machine_criterion_without_criterion_binding() {
    let signing = SigningKey::from_bytes(&[121_u8; 32]);
    let mut registry = builtin_registry();
    registry.sign("p8-cc-e2e", &signing).unwrap();
    let trusted = TrustedSignerSet {
        signers: vec![TrustedSigner {
            key_id: "p8-cc-e2e".into(),
            algorithm: "Ed25519".into(),
            public_key: STANDARD.encode(signing.verifying_key().to_bytes()),
        }],
    };

    let seed = ProjectRequirementSeed {
        requirement_id: Some("P8-NON-MACHINE".into()),
        title: "Human/runtime reviewed behavior".into(),
        intent: "A specific runtime behavior must be reviewed".into(),
        source: RequirementSource::User {
            reference: "audit://p8-cc-e2e".into(),
        },
        priority: RequirementPriority::P1,
        acceptance_criteria: vec![AcceptanceCriterion {
            criterion_id: "specific-runtime-behavior".into(),
            statement: "The specific runtime behavior is correct".into(),
            criterion_type: "review".into(),
            machine_checkable: false,
        }],
        verification_policy: VerificationPolicy {
            obligations: vec![EvidenceObligation {
                class: EvidenceClass::BrowserRecording,
                minimum_confidence: EvidenceConfidence::StrongRuntime,
                rationale: "runtime evidence required".into(),
                required: true,
            }],
            p8_collector_required: true,
        },
        dependencies: vec![],
        risk: RequirementRisk::Medium,
        requirement_type: "functional".into(),
    };

    let input = ProjectAuthorityInput {
        requirements: vec![seed],
        source_revision: "source-v1".into(),
        source_fingerprint: "source-fingerprint".into(),
        ..ProjectAuthorityInput::default()
    };

    let draft = AuthorityEngine
        .build_draft_with_project_authority(
            "mission-p8-cc-e2e",
            "project-p8-cc-e2e",
            "source-v1",
            "workspace-v1",
            registry.clone(),
            &context(),
            scope_fingerprint(BTreeMap::from([("root".into(), "closure".into())])),
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
        p7_run_id: "p7-test".into(),
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

    let store = EvidenceStore::new(root.path().join("evidence"), b"cc-e2e-key").unwrap();
    let bytes = b"some unrelated browser route passed";
    let metadata = EvidenceMetadata {
        evidence_id: "unrelated-browser-pass".into(),
        class: EvidenceClass::BrowserRecording,
        mission_id: authority.revision.seal.mission_id.clone(),
        mission_revision: authority.revision.revision,
        p6_seal_hash: authority.revision.seal.contract_hash.clone(),
        requirement_ids: vec!["P8-NON-MACHINE".into()],
        task_id: None,
        p7_attempt: None,
        execution_identities: vec![],
        workspace_fingerprint: current.workspace_fingerprint.clone(),
        source_revision: current.source_revision.clone(),
        collector: relintor_evidence::CollectorIdentity::new("test-browser", "test"),
        command_digest: relintor_evidence::sha256(b"browser probe"),
        environment_fingerprint: current.environment_fingerprint.clone(),
        created_at_ms: 1,
        artifact_digest: relintor_evidence::sha256(bytes),
        confidence: EvidenceConfidence::StrongRuntime,
        freshness: EvidenceFreshness::Fresh,
        result: EvidenceResult::Pass,
        required: true,
        accepted_criteria: BTreeSet::new(),
        relevant_paths: BTreeSet::new(),
        test_inventory: vec![],
        dependency_lock_hashes: BTreeMap::new(),
        scope_fingerprint: None,
    };
    put_fixture(&store, metadata, bytes).unwrap();

    let report = VerificationEngine::new_for_test(store, authority, current, vec![])
        .unwrap()
        .evaluate(None)
        .unwrap();

    assert_ne!(
        report.requirement_statuses[0].status,
        relintor_standards::RequirementStatus::Verified,
        "unrelated runtime evidence satisfied a criterion it did not claim"
    );
    assert!(report.requirement_statuses[0]
        .missing_acceptance_criteria
        .contains(&"specific-runtime-behavior".to_string()));
}
