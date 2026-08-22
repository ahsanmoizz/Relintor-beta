use base64::{engine::general_purpose::STANDARD, Engine as _};
use ed25519_dalek::SigningKey;
use relintor_evidence::test_support;
use relintor_evidence::test_support::*;
use relintor_evidence::*;
use relintor_standards::{
    builtin_registry, scope_fingerprint, AcceptanceCriterion, ApplicabilityContext,
    AuthorityEngine, EvidenceClass, EvidenceConfidence, EvidenceObligation, FactValue,
    ProjectAuthorityInput, ProjectRequirementSeed, RequirementPriority, RequirementRisk,
    RequirementSource, TrustedSigner, TrustedSignerSet, VerificationPolicy,
};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use tempfile::{tempdir, TempDir};

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
        revision: "p8-independent-source-audit".into(),
    }
}

fn seed(id: &str, machine_checkable: bool) -> ProjectRequirementSeed {
    ProjectRequirementSeed {
        requirement_id: Some(id.into()),
        title: id.into(),
        intent: format!("Verify {id}"),
        source: RequirementSource::User {
            reference: format!("audit://{id}"),
        },
        priority: RequirementPriority::P2,
        acceptance_criteria: vec![AcceptanceCriterion {
            criterion_id: format!("{id}-criterion"),
            statement: "Required acceptance criterion".into(),
            criterion_type: if machine_checkable { "test" } else { "review" }.into(),
            machine_checkable,
        }],
        verification_policy: VerificationPolicy {
            obligations: vec![EvidenceObligation {
                class: EvidenceClass::TestOutput,
                minimum_confidence: EvidenceConfidence::StrongDeterministic,
                rationale: "independent P8 source audit".into(),
                required: true,
            }],
            p8_collector_required: true,
        },
        dependencies: Vec::new(),
        risk: RequirementRisk::Medium,
        requirement_type: "p8-independent-audit".into(),
    }
}

fn fixture(machine_checkable: bool) -> (VerificationAuthority, FreshnessContext, TempDir) {
    let key = SigningKey::from_bytes(&[83_u8; 32]);
    let mut registry = builtin_registry();
    registry.sign("p8-independent-audit", &key).unwrap();
    let trusted = TrustedSignerSet {
        signers: vec![TrustedSigner {
            key_id: "p8-independent-audit".into(),
            algorithm: "Ed25519".into(),
            public_key: STANDARD.encode(key.verifying_key().to_bytes()),
        }],
    };
    let project_authority = ProjectAuthorityInput {
        requirements: vec![seed("P8-INDEPENDENT", machine_checkable)],
        source_revision: "p8-audit-source".into(),
        source_fingerprint: "p8-audit-fingerprint".into(),
        ..ProjectAuthorityInput::default()
    };
    let draft = AuthorityEngine
        .build_draft_with_project_authority(
            "mission-p8-independent",
            "project-p8-independent",
            "p8-audit-source",
            "p8-audit-workspace",
            registry.clone(),
            &context(),
            scope_fingerprint(BTreeMap::from([("root".into(), "p8-audit".into())])),
            project_authority,
            Vec::new(),
        )
        .unwrap();
    let (revision, handoff) = AuthorityEngine
        .seal(&draft, &trusted, "2026-08-15T00:00:00Z")
        .unwrap();
    let root = tempdir().unwrap();
    let authority = VerificationAuthority {
        revision,
        handoff,
        registry,
        trusted_signers: trusted,
        p7_run_id: "p7-audit-run".into(),
        p7_state: "EXECUTION_TASKS_FINISHED_AWAITING_VERIFICATION".into(),
        workspace_fingerprint: "p8-audit-workspace-fingerprint".into(),
        source_revision: Some("p8-audit-source".into()),
        environment_fingerprint: "p8-audit-environment".into(),
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
    (authority, current, root)
}

fn store(root: &Path) -> EvidenceStore {
    EvidenceStore::new(root.join("evidence"), b"independent-p8-key").unwrap()
}

fn metadata(
    authority: &VerificationAuthority,
    current: &FreshnessContext,
    id: &str,
) -> EvidenceMetadata {
    EvidenceMetadata {
        evidence_id: id.into(),
        class: EvidenceClass::TestOutput,
        mission_id: authority.revision.seal.mission_id.clone(),
        mission_revision: authority.revision.revision,
        p6_seal_hash: authority.revision.seal.contract_hash.clone(),
        requirement_ids: vec![
            authority.revision.contract.requirement_graph.requirements[0]
                .requirement_id
                .clone(),
        ],
        task_id: None,
        p7_attempt: Some(1),
        execution_identities: vec![],
        workspace_fingerprint: current.workspace_fingerprint.clone(),
        source_revision: current.source_revision.clone(),
        collector: CollectorIdentity::new("caller-created-audit-metadata", "1"),
        command_digest: sha256(b"caller-created"),
        environment_fingerprint: current.environment_fingerprint.clone(),
        created_at_ms: 1,
        artifact_digest: "0".repeat(64),
        confidence: EvidenceConfidence::StrongDeterministic,
        freshness: EvidenceFreshness::Fresh,
        result: EvidenceResult::Pass,
        required: true,
        accepted_criteria: BTreeSet::new(),
        relevant_paths: BTreeSet::new(),
        test_inventory: Vec::new(),
        dependency_lock_hashes: current.dependency_lock_hashes.clone(),
        scope_fingerprint: None,
    }
}

#[test]
fn deleted_invalidation_record_cannot_resurrect_old_evidence() {
    let (authority, current, root) = fixture(true);
    let store = store(root.path());
    let item = metadata(&authority, &current, "revoked-proof");
    test_support::put_fixture(&store, item, b"proof").unwrap();
    store
        .invalidate("revoked-proof", "revoked by authority")
        .unwrap();

    let tombstone = store
        .root()
        .join("invalidations")
        .join("revoked-proof.json");
    fs::remove_file(tombstone).unwrap();

    assert_ne!(
        store.freshness("revoked-proof", &current).unwrap(),
        EvidenceFreshness::Fresh,
        "deleting one unauthenticated tombstone resurrected revoked evidence"
    );
}

#[test]
fn caller_mutated_incomplete_report_cannot_be_certified() {
    let (authority, current, root) = fixture(true);
    let store = store(root.path());
    let engine =
        VerificationEngine::new_for_test(store, authority.clone(), current, Vec::new()).unwrap();
    let mut report = engine.evaluate(Some("DONE".into())).unwrap();
    assert_ne!(report.decision.state, CompletionState::VerifiedComplete);

    // Simulate a caller changing only public report data after engine evaluation.
    report.decision.state = CompletionState::VerifiedComplete;
    report.decision.reason = "caller forged completion".into();

    let signer = CompletionAuthority::new(b"independent-p8-key").unwrap();
    assert!(
        signer.issue_for_test(&report, &authority).is_err(),
        "public VerificationReport mutation was accepted by completion authority"
    );
}

#[test]
fn non_machine_acceptance_criterion_cannot_silently_disappear() {
    let (authority, current, root) = fixture(false);
    let store = store(root.path());
    let item = metadata(&authority, &current, "deterministic-proof");
    store
        .put_test_fixture(item, b"deterministic obligation passed")
        .unwrap();

    let report = VerificationEngine::new_for_test(store, authority, current, Vec::new())
        .unwrap()
        .evaluate(None)
        .unwrap();

    assert_ne!(
        report.decision.state,
        CompletionState::VerifiedComplete,
        "non-machine acceptance criterion vanished from the completion gate"
    );
}
