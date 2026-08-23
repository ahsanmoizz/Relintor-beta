use base64::{engine::general_purpose::STANDARD, Engine as _};
use ed25519_dalek::SigningKey;
use relintor_evidence::*;
use relintor_standards::{
    builtin_registry, scope_fingerprint, AcceptanceCriterion, ApplicabilityContext,
    AuthorityEngine, EvidenceClass, EvidenceConfidence, EvidenceObligation, FactValue,
    ProjectAuthorityInput, ProjectRequirementSeed, RequirementPriority, RequirementRisk,
    RequirementSource, TrustedSigner, TrustedSignerSet, VerificationPolicy,
};
use std::collections::BTreeMap;
use std::fs;
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
        revision: "p8-zc-absolute".into(),
    }
}

fn authority_fixture() -> (VerificationAuthority, FreshnessContext, TempDir) {
    let signing = SigningKey::from_bytes(&[77_u8; 32]);
    let mut registry = builtin_registry();
    registry.sign("p8-zc-absolute", &signing).unwrap();
    let trusted = TrustedSignerSet {
        signers: vec![TrustedSigner {
            key_id: "p8-zc-absolute".into(),
            algorithm: "Ed25519".into(),
            public_key: STANDARD.encode(signing.verifying_key().to_bytes()),
        }],
    };
    let seed = ProjectRequirementSeed {
        requirement_id: Some("P8-ZC-01".into()),
        title: "Sensitive behavior authority".into(),
        intent: "Only an authorized probe may prove the sensitive behavior".into(),
        source: RequirementSource::User {
            reference: "audit://p8-zc".into(),
        },
        priority: RequirementPriority::P1,
        acceptance_criteria: vec![AcceptanceCriterion {
            criterion_id: "sensitive-behavior".into(),
            statement: "The sensitive behavior is correct".into(),
            criterion_type: "test".into(),
            machine_checkable: true,
        }],
        verification_policy: VerificationPolicy {
            obligations: vec![EvidenceObligation {
                class: EvidenceClass::TestOutput,
                minimum_confidence: EvidenceConfidence::StrongDeterministic,
                rationale: "criterion-specific test evidence".into(),
                required: true,
            }],
            p8_collector_required: true,
        },
        dependencies: vec![],
        risk: RequirementRisk::Medium,
        requirement_type: "functional".into(),
    };
    let draft = AuthorityEngine
        .build_draft_with_project_authority(
            "mission-p8-zc",
            "project-p8-zc",
            "source-v1",
            "workspace-v1",
            registry.clone(),
            &context(),
            scope_fingerprint(BTreeMap::from([("root".into(), "p8-zc".into())])),
            ProjectAuthorityInput {
                requirements: vec![seed],
                source_revision: "source-v1".into(),
                source_fingerprint: "source-fingerprint".into(),
                ..ProjectAuthorityInput::default()
            },
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
        p7_run_id: "p7-zc".into(),
        p7_state: "EXECUTION_TASKS_FINISHED_AWAITING_VERIFICATION".into(),
        workspace_fingerprint: "workspace-zc".into(),
        source_revision: Some("source-v1".into()),
        environment_fingerprint: "environment-zc".into(),
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

fn candidate_command(root: &std::path::Path) -> CommandSpec {
    if cfg!(target_os = "windows") {
        CommandSpec {
            program: "cmd".into(),
            args: vec!["/C".into(), "echo".into(), "test sensitive ... ok".into()],
            working_directory: root.to_path_buf(),
            environment: BTreeMap::new(),
        }
    } else {
        CommandSpec {
            program: "sh".into(),
            args: vec!["-c".into(), "printf 'test sensitive ... ok'".into()],
            working_directory: root.to_path_buf(),
            environment: BTreeMap::new(),
        }
    }
}

fn candidate_plan(root: &std::path::Path) {
    let config_dir = root.join(".relintor");
    fs::create_dir_all(&config_dir).unwrap();
    let command = candidate_command(root);
    let plan = serde_json::json!({
        "collectors": [{
            "class": "TEST_OUTPUT",
            "program": command.program,
            "args": command.args,
            "criteria": [{
                "requirement_id": "P8-ZC-01",
                "criterion_id": "sensitive-behavior",
                "probe_identity": "fake-pass"
            }]
        }]
    });
    fs::write(
        config_dir.join("verification-plan.json"),
        serde_json::to_vec(&plan).unwrap(),
    )
    .unwrap();
}

#[test]
fn mutable_workspace_claim_cannot_verify_sealed_criterion() {
    let (authority, current, root) = authority_fixture();
    candidate_plan(root.path());
    let store = EvidenceStore::new(root.path().join("evidence"), b"zc-key").unwrap();
    VerificationCollectorOrchestrator::new(root.path())
        .collect_required_evidence(&authority, &current, &store)
        .unwrap();
    let report = VerificationEngine::new_for_test(store, authority, current, vec![])
        .unwrap()
        .evaluate(None)
        .unwrap();
    assert!(report.requirement_statuses[0]
        .missing_acceptance_criteria
        .contains(&"sensitive-behavior".to_string()));
}

#[test]
fn protected_mapping_requires_exact_candidate_and_authentication_stays_fail_closed() {
    let (authority, current, root) = authority_fixture();
    candidate_plan(root.path());
    let command = candidate_command(root.path());
    let protected = ProtectedCriterionVerificationPlan::from_test_support(
        &authority,
        vec![CriterionProbeAuthorizationInput {
            requirement_id: "P8-ZC-01".into(),
            criterion_id: "sensitive-behavior".into(),
            evidence_class: EvidenceClass::TestOutput,
            collector_identity: CollectorIdentity::new("test-collector", "p8-v1"),
            probe_identity: "fake-pass".into(),
            command_digest: command.digest().unwrap(),
        }],
    )
    .unwrap();
    let store = EvidenceStore::new(root.path().join("evidence"), b"zc-key").unwrap();
    VerificationCollectorOrchestrator::new(root.path())
        .collect_required_evidence_with_protected_plan(&authority, &current, &store, &protected)
        .unwrap();
    let artifacts = store.list().unwrap();
    assert!(artifacts.iter().any(|artifact| artifact
        .metadata
        .accepted_criteria
        .contains("sensitive-behavior")));

    let input = AiVerifierInput {
        requirement_id: "P8-ZC-01".into(),
        acceptance_criteria: vec!["sensitive-behavior".into()],
        evidence: vec![],
        deterministic_gate_results: vec![],
        explicit_decisions: vec![],
    };
    assert!(ProductionAiProvider::new("", None)
        .judge_with_metadata(input)
        .is_err());
}
