use base64::{engine::general_purpose::STANDARD, Engine as _};
use ed25519_dalek::SigningKey;
use relintor_evidence::{
    BuildCollector, CollectorBinding, CommandSpec, CompletionState, EvidenceStore,
    FreshnessContext, VerificationAuthority, VerificationEngine,
};
use relintor_standards::{
    builtin_registry, scope_fingerprint, AcceptanceCriterion, ApplicabilityContext,
    AuthorityEngine, EvidenceClass, EvidenceConfidence, EvidenceObligation, FactValue,
    ProjectAuthorityInput, ProjectRequirementSeed, RequirementPriority, RequirementRisk,
    RequirementSource, TrustedSigner, TrustedSignerSet, VerificationPolicy,
};
use std::collections::BTreeMap;
use tempfile::tempdir;

#[cfg(target_os = "windows")]
fn success_command(dir: &std::path::Path) -> CommandSpec {
    CommandSpec {
        program: "cmd".into(),
        args: vec!["/C".into(), "exit 0".into()],
        working_directory: dir.to_path_buf(),
        environment: BTreeMap::new(),
    }
}

#[cfg(not(target_os = "windows"))]
fn success_command(dir: &std::path::Path) -> CommandSpec {
    CommandSpec {
        program: "sh".into(),
        args: vec!["-c".into(), "exit 0".into()],
        working_directory: dir.to_path_buf(),
        environment: BTreeMap::new(),
    }
}

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
        revision: "p8-last-closure".into(),
    }
}

#[test]
fn successful_build_cannot_automatically_prove_unexecuted_runtime_criterion() {
    let signing = SigningKey::from_bytes(&[111_u8; 32]);
    let mut registry = builtin_registry();
    registry.sign("p8-last-closure", &signing).unwrap();
    let trusted = TrustedSignerSet {
        signers: vec![TrustedSigner {
            key_id: "p8-last-closure".into(),
            algorithm: "Ed25519".into(),
            public_key: STANDARD.encode(signing.verifying_key().to_bytes()),
        }],
    };

    let requirement = ProjectRequirementSeed {
        requirement_id: Some("P8-CRITERION-PROVENANCE".into()),
        title: "Build and runtime behavior".into(),
        intent: "The project builds and a runtime route behaves correctly".into(),
        source: RequirementSource::User {
            reference: "audit://criterion-provenance".into(),
        },
        priority: RequirementPriority::P1,
        acceptance_criteria: vec![
            AcceptanceCriterion {
                criterion_id: "builds".into(),
                statement: "Project builds".into(),
                criterion_type: "build".into(),
                machine_checkable: true,
            },
            AcceptanceCriterion {
                criterion_id: "runtime-route".into(),
                statement: "Runtime route returns the expected behavior".into(),
                criterion_type: "runtime".into(),
                machine_checkable: true,
            },
        ],
        verification_policy: VerificationPolicy {
            obligations: vec![
                EvidenceObligation {
                    class: EvidenceClass::BuildOutput,
                    minimum_confidence: EvidenceConfidence::StrongDeterministic,
                    rationale: "build proof".into(),
                    required: true,
                },
                EvidenceObligation {
                    class: EvidenceClass::BrowserRecording,
                    minimum_confidence: EvidenceConfidence::StrongRuntime,
                    rationale: "runtime proof".into(),
                    required: true,
                },
            ],
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
            "mission-p8-last-closure",
            "project-p8-last-closure",
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
        .seal(&draft, &trusted, "2026-08-15T00:00:00Z")
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
    let store = EvidenceStore::new(root.path().join("evidence"), b"closure-key").unwrap();

    let binding = CollectorBinding::for_requirement(
        &authority,
        &current,
        "build-only",
        "P8-CRITERION-PROVENANCE",
        EvidenceClass::BuildOutput,
    )
    .unwrap();
    let build = BuildCollector::default()
        .collect(&binding, &success_command(root.path()))
        .unwrap();
    store.put(build.receipt, &build.artifact_bytes).unwrap();

    // No BrowserRecording/runtime evidence was collected.
    let report = VerificationEngine::new_for_test(store, authority, current, vec![])
        .unwrap()
        .evaluate(None)
        .unwrap();

    assert_ne!(report.decision.state, CompletionState::VerifiedComplete);
    let req = &report.requirement_statuses[0];
    assert!(
        req.missing_acceptance_criteria
            .iter()
            .any(|c| c == "runtime-route")
            || !req.missing_obligations.is_empty(),
        "build evidence incorrectly claimed the unexecuted runtime criterion"
    );
}
