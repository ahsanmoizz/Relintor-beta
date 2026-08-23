use base64::{engine::general_purpose::STANDARD, Engine as _};
use ed25519_dalek::SigningKey;
use relintor_evidence::test_support;
use relintor_evidence::test_support::*;
use relintor_evidence::*;
use relintor_standards::{
    builtin_registry, scope_fingerprint, AcceptanceCriterion, ApplicabilityContext,
    AuthorityEngine, DecisionActor, DecisionKind, EvidenceClass, EvidenceConfidence,
    EvidenceObligation, FactValue, ProjectAuthorityInput, ProjectRequirementSeed,
    RequirementPriority, RequirementRisk, RequirementSource, RequirementStatus, TrustedSigner,
    TrustedSignerSet, VerificationPolicy,
};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use tempfile::{tempdir, TempDir};

trait TestAiJudgementInjection {
    fn with_ai_judgements(
        self,
        judgements: Vec<AiVerifierJudgement>,
    ) -> Result<VerificationEngine, EvidenceError>;
}

impl TestAiJudgementInjection for VerificationEngine {
    fn with_ai_judgements(
        self,
        judgements: Vec<AiVerifierJudgement>,
    ) -> Result<VerificationEngine, EvidenceError> {
        self.with_test_ai_judgements(judgements)
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
        revision: "p8-context-1".into(),
    }
}

fn seed(id: &str) -> ProjectRequirementSeed {
    seed_with_risk(id, RequirementRisk::Medium)
}

fn seed_with_risk(id: &str, risk: RequirementRisk) -> ProjectRequirementSeed {
    ProjectRequirementSeed {
        requirement_id: Some(id.into()),
        title: format!("P8 {id}"),
        intent: format!("Verify {id} independently"),
        source: RequirementSource::User {
            reference: format!("p8-{id}"),
        },
        priority: RequirementPriority::P2,
        acceptance_criteria: vec![AcceptanceCriterion {
            criterion_id: format!("{id}-criterion"),
            statement: "A deterministic collector proves the requirement".into(),
            criterion_type: "test".into(),
            machine_checkable: true,
        }],
        verification_policy: VerificationPolicy {
            obligations: vec![EvidenceObligation {
                class: EvidenceClass::TestOutput,
                minimum_confidence: EvidenceConfidence::StrongDeterministic,
                rationale: "required P8 test evidence".into(),
                required: true,
            }],
            p8_collector_required: true,
        },
        dependencies: Vec::new(),
        risk,
        requirement_type: "p8-test-scope".into(),
    }
}

fn authority_fixture() -> (VerificationAuthority, FreshnessContext, TempDir) {
    authority_fixture_with_risk(RequirementRisk::Medium)
}

fn authority_fixture_with_risk(
    risk: RequirementRisk,
) -> (VerificationAuthority, FreshnessContext, TempDir) {
    authority_fixture_with_decisions(risk, Vec::new())
}

fn authority_fixture_with_decisions(
    risk: RequirementRisk,
    decisions: Vec<relintor_standards::ExplicitDecision>,
) -> (VerificationAuthority, FreshnessContext, TempDir) {
    let signing_key = SigningKey::from_bytes(&[9_u8; 32]);
    let mut registry = builtin_registry();
    registry
        .sign("p8-test-signer", &signing_key)
        .expect("sign registry");
    let trusted = TrustedSignerSet {
        signers: vec![TrustedSigner {
            key_id: "p8-test-signer".into(),
            algorithm: "Ed25519".into(),
            public_key: STANDARD.encode(signing_key.verifying_key().to_bytes()),
        }],
    };
    let project_authority = ProjectAuthorityInput {
        requirements: vec![seed_with_risk("P8-TEST-01", risk), seed("P8-TEST-02")],
        decisions: decisions.clone(),
        source_revision: "p8-fixture-source".into(),
        source_fingerprint: "p8-fixture-input".into(),
        ..ProjectAuthorityInput::default()
    };
    let draft = AuthorityEngine
        .build_draft_with_project_authority(
            "mission-p8-test",
            "project-p8-test",
            "p8-fixture-source",
            "p8-workspace-source",
            registry.clone(),
            &context(),
            scope_fingerprint(BTreeMap::from([("root".into(), "p8".into())])),
            project_authority,
            Vec::new(),
        )
        .expect("build authority fixture");
    let (revision, handoff) = AuthorityEngine
        .seal(&draft, &trusted, "2026-08-15T00:00:00Z")
        .expect("seal authority fixture");
    let root = tempdir().expect("temp workspace");
    let authority = VerificationAuthority {
        p7_run_id: "p7-run-p8-fixture".into(),
        p7_state: "EXECUTION_TASKS_FINISHED_AWAITING_VERIFICATION".into(),
        workspace_fingerprint: "workspace-fingerprint-p8".into(),
        source_revision: Some(revision.contract.project_source_revision.clone()),
        environment_fingerprint: "environment-fingerprint-p8".into(),
        revision,
        handoff,
        registry,
        trusted_signers: trusted,
    };
    let current = FreshnessContext {
        mission_id: authority.revision.seal.mission_id.clone(),
        mission_revision: authority.revision.revision,
        p6_seal_hash: authority.revision.seal.contract_hash.clone(),
        workspace_fingerprint: authority.workspace_fingerprint.clone(),
        source_revision: authority.source_revision.clone(),
        environment_fingerprint: authority.environment_fingerprint.clone(),
        dependency_lock_hashes: BTreeMap::from([("Cargo.lock".into(), "lock-p8".into())]),
        workspace_root: Some(root.path().to_path_buf()),
    };
    (authority, current, root)
}

fn store(root: &Path) -> EvidenceStore {
    EvidenceStore::new(root.join("evidence"), b"p8-test-local-key").expect("store")
}

#[test]
fn node_beta_workspace_discovers_test_security_and_accessibility_collectors() {
    let root = tempdir().expect("workspace");
    fs::write(
        root.path().join("package.json"),
        r#"{"scripts":{"test":"node --test","security-scan":"node security.js","accessibility-audit":"node accessibility.js","lint":"node lint.js"}}"#,
    )
    .expect("package manifest");
    fs::create_dir_all(root.path().join("tests")).expect("test directory");
    fs::write(
        root.path().join("tests/example.test.js"),
        "// TAP test source\n",
    )
    .expect("test source");

    let plan = VerificationCollectorPlan::discover(root.path()).expect("collector discovery");
    for expected in [
        EvidenceClass::TestOutput,
        EvidenceClass::SecurityScan,
        EvidenceClass::AccessibilityResult,
        EvidenceClass::LintStaticAnalysis,
    ] {
        assert!(plan
            .collectors
            .iter()
            .any(|collector| collector.evidence_class == expected));
    }
}

#[test]
fn mutable_workspace_plan_cannot_mint_criterion_authority() {
    let (authority, current, root) = authority_fixture();
    let config_dir = root.path().join(".relintor");
    fs::create_dir_all(&config_dir).expect("verification config directory");
    let command = command_for_exit(root.path(), true);
    let plan = serde_json::json!({
        "collectors": [{
            "class": "TEST_OUTPUT",
            "program": &command.program,
            "args": &command.args,
            "criteria": [{
                "requirement_id": "P8-TEST-01",
                "criterion_id": "P8-TEST-01-criterion",
                "probe_identity": "cmd-exit-probe"
            }]
        }]
    });
    fs::write(
        config_dir.join("verification-plan.json"),
        serde_json::to_vec(&plan).expect("portable verification plan"),
    )
    .expect("verification plan");
    let store = store(root.path());
    let result = VerificationCollectorOrchestrator::new(root.path())
        .collect_required_evidence(&authority, &current, &store)
        .expect("criterion-aware orchestration");

    assert!(result
        .executed
        .iter()
        .all(|entry| !entry.contains("P8-TEST-01-criterion")));
    let artifacts = store.list().expect("stored collector evidence");
    assert!(artifacts
        .iter()
        .all(|artifact| artifact.metadata.accepted_criteria.is_empty()));
}

#[test]
fn protected_criterion_mapping_mints_evidence_only_for_matching_candidate() {
    let (authority, current, root) = authority_fixture();
    let config_dir = root.path().join(".relintor");
    fs::create_dir_all(&config_dir).expect("verification config directory");
    let command = command_for_exit(root.path(), true);
    let plan = serde_json::json!({
        "collectors": [{
            "class": "TEST_OUTPUT",
            "program": &command.program,
            "args": &command.args
        }]
    });
    fs::write(
        config_dir.join("verification-plan.json"),
        serde_json::to_vec(&plan).expect("portable verification plan"),
    )
    .expect("verification plan");
    let protected = ProtectedCriterionVerificationPlan::from_test_support(
        &authority,
        vec![CriterionProbeAuthorizationInput {
            requirement_id: "P8-TEST-01".into(),
            criterion_id: "P8-TEST-01-criterion".into(),
            evidence_class: EvidenceClass::TestOutput,
            collector_identity: CollectorIdentity::new("test-collector", "p8-v1"),
            probe_identity: "cmd-exit-probe".into(),
            command_digest: command.digest().unwrap(),
        }],
    )
    .expect("protected criterion authority");
    let store = store(root.path());
    let result = VerificationCollectorOrchestrator::new(root.path())
        .collect_required_evidence_with_protected_plan(&authority, &current, &store, &protected)
        .expect("protected criterion-aware orchestration");

    assert!(result
        .executed
        .iter()
        .any(|entry| entry.contains("P8-TEST-01-criterion")));
    let artifacts = store.list().expect("stored collector evidence");
    assert!(artifacts.iter().any(|artifact| {
        artifact.metadata.requirement_ids == vec!["P8-TEST-01".to_string()]
            && artifact
                .metadata
                .accepted_criteria
                .contains("P8-TEST-01-criterion")
    }));
}

#[test]
fn protected_mapping_rejects_candidate_command_digest_change() {
    let (authority, current, root) = authority_fixture();
    let config_dir = root.path().join(".relintor");
    fs::create_dir_all(&config_dir).expect("verification config directory");
    fs::write(
        config_dir.join("verification-plan.json"),
        r#"{"collectors":[
            {"class":"TEST_OUTPUT","program":"cmd","args":["/C","exit","1"]}
        ]}"#,
    )
    .expect("verification plan");
    let authorized_command = CommandSpec {
        program: "cmd".into(),
        args: vec!["/C".into(), "exit".into(), "0".into()],
        working_directory: root.path().to_path_buf(),
        environment: BTreeMap::new(),
    };
    let protected = ProtectedCriterionVerificationPlan::from_test_support(
        &authority,
        vec![CriterionProbeAuthorizationInput {
            requirement_id: "P8-TEST-01".into(),
            criterion_id: "P8-TEST-01-criterion".into(),
            evidence_class: EvidenceClass::TestOutput,
            collector_identity: CollectorIdentity::new("test-collector", "p8-v1"),
            probe_identity: "cmd-exit-probe".into(),
            command_digest: authorized_command.digest().unwrap(),
        }],
    )
    .expect("protected criterion authority");
    let store = store(root.path());
    let result = VerificationCollectorOrchestrator::new(root.path())
        .collect_required_evidence_with_protected_plan(&authority, &current, &store, &protected)
        .expect("protected criterion-aware orchestration");
    assert!(result
        .executed
        .iter()
        .all(|entry| !entry.contains("P8-TEST-01-criterion")));
    assert!(store
        .list()
        .unwrap()
        .iter()
        .all(|artifact| artifact.metadata.accepted_criteria.is_empty()));
}

#[test]
fn protected_mapping_rejects_criterion_and_evidence_class_mismatch() {
    let (authority, _current, _root) = authority_fixture();
    let invalid_criterion = ProtectedCriterionVerificationPlan::from_test_support(
        &authority,
        vec![CriterionProbeAuthorizationInput {
            requirement_id: "P8-TEST-01".into(),
            criterion_id: "not-sealed".into(),
            evidence_class: EvidenceClass::TestOutput,
            collector_identity: CollectorIdentity::new("test-collector", "p8-v1"),
            probe_identity: "probe".into(),
            command_digest: "0".repeat(64),
        }],
    );
    assert!(invalid_criterion.is_err());

    let invalid_class = ProtectedCriterionVerificationPlan::from_test_support(
        &authority,
        vec![CriterionProbeAuthorizationInput {
            requirement_id: "P8-TEST-01".into(),
            criterion_id: "P8-TEST-01-criterion".into(),
            evidence_class: EvidenceClass::BuildOutput,
            collector_identity: CollectorIdentity::new("build-collector", "p8-v1"),
            probe_identity: "probe".into(),
            command_digest: "0".repeat(64),
        }],
    );
    assert!(invalid_class.is_err());
}

fn metadata(
    authority: &VerificationAuthority,
    current: &FreshnessContext,
    id: &str,
    class: EvidenceClass,
    result: EvidenceResult,
    confidence: EvidenceConfidence,
) -> EvidenceMetadata {
    EvidenceMetadata {
        evidence_id: id.into(),
        class,
        mission_id: authority.revision.seal.mission_id.clone(),
        mission_revision: authority.revision.revision,
        p6_seal_hash: authority.revision.seal.contract_hash.clone(),
        requirement_ids: authority
            .revision
            .contract
            .requirement_graph
            .requirements
            .iter()
            .map(|r| r.requirement_id.clone())
            .collect(),
        task_id: None,
        p7_attempt: Some(1),
        execution_identities: vec![],
        workspace_fingerprint: current.workspace_fingerprint.clone(),
        source_revision: current.source_revision.clone(),
        collector: CollectorIdentity::new("p8-test-collector", "1"),
        command_digest: "command-p8".into(),
        environment_fingerprint: current.environment_fingerprint.clone(),
        created_at_ms: 1,
        artifact_digest: "0".repeat(64),
        confidence,
        freshness: EvidenceFreshness::Fresh,
        result,
        required: true,
        accepted_criteria: BTreeSet::new(),
        relevant_paths: BTreeSet::new(),
        test_inventory: Vec::new(),
        dependency_lock_hashes: current.dependency_lock_hashes.clone(),
        scope_fingerprint: None,
    }
}

fn add_for_all(
    store: &EvidenceStore,
    authority: &VerificationAuthority,
    current: &FreshnessContext,
) {
    for requirement in &authority.revision.contract.requirement_graph.requirements {
        for (index, obligation) in requirement
            .verification_policy
            .obligations
            .iter()
            .filter(|obligation| obligation.required)
            .enumerate()
        {
            let mut item = metadata(
                authority,
                current,
                &format!("pass-{}-{index}", requirement.requirement_id),
                obligation.class,
                EvidenceResult::Pass,
                EvidenceConfidence::StrongDeterministic,
            );
            item.requirement_ids = vec![requirement.requirement_id.clone()];
            item.accepted_criteria = requirement
                .acceptance_criteria
                .iter()
                .filter(|criterion| criterion.machine_checkable)
                .map(|criterion| criterion.criterion_id.clone())
                .collect();
            store
                .put_test_fixture(item, b"test passed")
                .expect("store pass evidence");
        }
    }
}

fn report_with_all_evidence() -> (
    TempDir,
    EvidenceStore,
    VerificationAuthority,
    FreshnessContext,
    VerificationReport,
) {
    let (authority, current, root) = authority_fixture();
    let store = store(root.path());
    add_for_all(&store, &authority, &current);
    let engine = VerificationEngine::new_for_test(
        store.clone(),
        authority.clone(),
        current.clone(),
        Vec::new(),
    )
    .expect("engine");
    let report = engine.evaluate(Some("DONE".into())).expect("report");
    (root, store, authority, current, report)
}

#[test]
fn valid_build_evidence_is_accepted() {
    let (authority, current, root) = authority_fixture();
    let store = store(root.path());
    let mut item = metadata(
        &authority,
        &current,
        "build-pass",
        EvidenceClass::BuildOutput,
        EvidenceResult::Pass,
        EvidenceConfidence::StrongDeterministic,
    );
    item.requirement_ids = vec![
        authority.revision.contract.requirement_graph.requirements[0]
            .requirement_id
            .clone(),
    ];
    let artifact = test_support::put_fixture(&store, item, b"exit=0").unwrap();
    assert_eq!(
        store
            .load(&artifact.metadata.evidence_id)
            .unwrap()
            .artifact
            .digest,
        artifact.digest
    );
}

#[test]
fn failed_build_is_rejected() {
    let (authority, current, root) = authority_fixture();
    let store = store(root.path());
    let mut item = metadata(
        &authority,
        &current,
        "build-fail",
        EvidenceClass::BuildOutput,
        EvidenceResult::Fail,
        EvidenceConfidence::StrongDeterministic,
    );
    item.requirement_ids = vec![
        authority.revision.contract.requirement_graph.requirements[0]
            .requirement_id
            .clone(),
    ];
    test_support::put_fixture(&store, item, b"exit=1").unwrap();
    let report = VerificationEngine::new_for_test(store, authority, current, Vec::new())
        .unwrap()
        .evaluate(None)
        .unwrap();
    assert_eq!(report.decision.state, CompletionState::FailedVerification);
}

#[test]
fn valid_test_evidence_is_accepted() {
    let (_, _, _, _, report) = report_with_all_evidence();
    assert!(report
        .requirement_statuses
        .iter()
        .all(|item| item.status == RequirementStatus::Verified));
}

#[test]
fn failed_required_test_remains_visible() {
    let (authority, current, root) = authority_fixture();
    let store = store(root.path());
    let mut item = metadata(
        &authority,
        &current,
        "test-fail",
        EvidenceClass::TestOutput,
        EvidenceResult::Fail,
        EvidenceConfidence::StrongDeterministic,
    );
    item.requirement_ids = vec![
        authority.revision.contract.requirement_graph.requirements[0]
            .requirement_id
            .clone(),
    ];
    test_support::put_fixture(&store, item, b"failed test").unwrap();
    let report = VerificationEngine::new_for_test(store, authority, current, Vec::new())
        .unwrap()
        .evaluate(None)
        .unwrap();
    assert!(!report.requirement_statuses[0].failed_evidence.is_empty());
}

#[test]
fn skipped_required_test_blocks_completion() {
    let (authority, current, root) = authority_fixture();
    let store = store(root.path());
    let mut item = metadata(
        &authority,
        &current,
        "test-skipped",
        EvidenceClass::TestOutput,
        EvidenceResult::Skipped,
        EvidenceConfidence::StrongDeterministic,
    );
    item.requirement_ids = vec![
        authority.revision.contract.requirement_graph.requirements[0]
            .requirement_id
            .clone(),
    ];
    test_support::put_fixture(&store, item, b"ignored=1").unwrap();
    let report = VerificationEngine::new_for_test(store, authority, current, Vec::new())
        .unwrap()
        .evaluate(None)
        .unwrap();
    assert_ne!(report.decision.state, CompletionState::VerifiedComplete);
}

#[test]
fn missing_required_test_blocks_completion() {
    let (authority, current, root) = authority_fixture();
    let store = store(root.path());
    let requirement = authority.revision.contract.requirement_graph.requirements[0]
        .requirement_id
        .clone();
    let mut item = metadata(
        &authority,
        &current,
        "one-pass",
        EvidenceClass::TestOutput,
        EvidenceResult::Pass,
        EvidenceConfidence::StrongDeterministic,
    );
    item.requirement_ids = vec![requirement];
    test_support::put_fixture(&store, item, b"one test").unwrap();
    let report = VerificationEngine::new_for_test(store, authority, current, Vec::new())
        .unwrap()
        .evaluate(None)
        .unwrap();
    assert_ne!(report.decision.state, CompletionState::VerifiedComplete);
}

#[test]
fn deleted_test_invalidates_old_evidence() {
    let (authority, current, root) = authority_fixture();
    let path = root.path().join("required_test.rs");
    fs::write(&path, b"test").unwrap();
    let hash = hash_file(&path).unwrap();
    let store = store(root.path());
    let mut item = metadata(
        &authority,
        &current,
        "deleted-test",
        EvidenceClass::TestOutput,
        EvidenceResult::Pass,
        EvidenceConfidence::StrongDeterministic,
    );
    item.test_inventory = vec![TestInventoryEntry {
        test_id: "required".into(),
        source_path: "required_test.rs".into(),
        source_hash: hash,
        enabled: true,
    }];
    test_support::put_fixture(&store, item, b"passed").unwrap();
    fs::remove_file(path).unwrap();
    assert_eq!(
        store.freshness("deleted-test", &current).unwrap(),
        EvidenceFreshness::Stale
    );
}

#[test]
fn stale_source_evidence_is_rejected() {
    let (authority, current, root) = authority_fixture();
    let store = store(root.path());
    let item = metadata(
        &authority,
        &current,
        "stale-source",
        EvidenceClass::BuildOutput,
        EvidenceResult::Pass,
        EvidenceConfidence::StrongDeterministic,
    );
    test_support::put_fixture(&store, item, b"old").unwrap();
    let mut changed = current.clone();
    changed.workspace_fingerprint = "changed".into();
    assert_eq!(
        store.freshness("stale-source", &changed).unwrap(),
        EvidenceFreshness::Stale
    );
}

#[test]
fn wrong_source_revision_is_rejected() {
    let (authority, current, root) = authority_fixture();
    let store = store(root.path());
    store
        .put_test_fixture(
            metadata(
                &authority,
                &current,
                "wrong-revision",
                EvidenceClass::TestOutput,
                EvidenceResult::Pass,
                EvidenceConfidence::StrongDeterministic,
            ),
            b"old",
        )
        .unwrap();
    let mut changed = current.clone();
    changed.source_revision = Some("other-source".into());
    assert_eq!(
        store.freshness("wrong-revision", &changed).unwrap(),
        EvidenceFreshness::Stale
    );
}

#[test]
fn changed_lockfile_invalidates_impacted_evidence() {
    let (authority, current, root) = authority_fixture();
    let store = store(root.path());
    store
        .put_test_fixture(
            metadata(
                &authority,
                &current,
                "lock-impact",
                EvidenceClass::BuildOutput,
                EvidenceResult::Pass,
                EvidenceConfidence::StrongDeterministic,
            ),
            b"build",
        )
        .unwrap();
    assert!(store
        .impacted_by_change(
            "lock-impact",
            &WorkspaceChangeSet {
                lockfile_changed: true,
                ..WorkspaceChangeSet::default()
            }
        )
        .unwrap());
}

#[test]
fn artifact_bytes_tampering_is_rejected() {
    let (authority, current, root) = authority_fixture();
    let store = store(root.path());
    let artifact = store
        .put_test_fixture(
            metadata(
                &authority,
                &current,
                "tamper-bytes",
                EvidenceClass::TestOutput,
                EvidenceResult::Pass,
                EvidenceConfidence::StrongDeterministic,
            ),
            b"good",
        )
        .unwrap();
    fs::write(store.root().join("blobs").join(&artifact.digest), b"bad").unwrap();
    assert!(matches!(
        store.load("tamper-bytes"),
        Err(EvidenceError::InvalidDigest(_))
    ));
}

#[test]
fn evidence_metadata_tampering_is_rejected() {
    let (authority, current, root) = authority_fixture();
    let store = store(root.path());
    let artifact = store
        .put_test_fixture(
            metadata(
                &authority,
                &current,
                "tamper-metadata",
                EvidenceClass::TestOutput,
                EvidenceResult::Pass,
                EvidenceConfidence::StrongDeterministic,
            ),
            b"good",
        )
        .unwrap();
    let path = store.root().join("metadata").join("tamper-metadata.json");
    let mut bytes = fs::read(&path).unwrap();
    if let Some(position) = bytes
        .windows(b"tamper-metadata".len())
        .position(|window| window == b"tamper-metadata")
    {
        bytes[position] = b"x"[0];
    }
    fs::write(path, bytes).unwrap();
    assert!(store.load(&artifact.metadata.evidence_id).is_err());
}

#[test]
fn atomic_evidence_commit_reloads_and_partial_metadata_is_never_accepted() {
    let (authority, current, root) = authority_fixture();
    let initial = store(root.path());
    let artifact = initial
        .put_test_fixture(
            metadata(
                &authority,
                &current,
                "atomic-reload",
                EvidenceClass::TestOutput,
                EvidenceResult::Pass,
                EvidenceConfidence::StrongDeterministic,
            ),
            b"durable proof",
        )
        .expect("atomic evidence commit");
    let reopened = store(root.path());
    assert_eq!(
        reopened
            .load("atomic-reload")
            .expect("durable reload")
            .artifact,
        artifact
    );

    fs::write(
        reopened.root().join("metadata").join("atomic-reload.json"),
        b"{\"artifact\":".as_slice(),
    )
    .expect("simulate interrupted final metadata");
    assert!(store(root.path()).load("atomic-reload").is_err());
}

#[test]
fn unknown_evidence_store_version_is_rejected_before_use() {
    let (authority, current, root) = authority_fixture();
    let store = store(root.path());
    store
        .put_test_fixture(
            metadata(
                &authority,
                &current,
                "unknown-version",
                EvidenceClass::TestOutput,
                EvidenceResult::Pass,
                EvidenceConfidence::StrongDeterministic,
            ),
            b"proof",
        )
        .expect("stored evidence");
    let path = store.root().join("metadata").join("unknown-version.json");
    let mut envelope: serde_json::Value =
        serde_json::from_slice(&fs::read(&path).expect("metadata")).expect("metadata json");
    envelope["artifact"]["store_version"] = serde_json::Value::String("future-v99".into());
    fs::write(
        &path,
        serde_json::to_vec(&envelope).expect("mutated metadata"),
    )
    .expect("write unknown version");
    assert!(matches!(
        store.load("unknown-version"),
        Err(EvidenceError::IntegrityFailure(message)) if message.contains("unsupported evidence store version")
    ));
}

#[test]
fn requirement_evidence_link_tampering_is_rejected_by_coverage() {
    let (authority, current, root) = authority_fixture();
    let store = store(root.path());
    let mut item = metadata(
        &authority,
        &current,
        "wrong-link",
        EvidenceClass::TestOutput,
        EvidenceResult::Pass,
        EvidenceConfidence::StrongDeterministic,
    );
    item.requirement_ids = vec!["wrong-requirement".into()];
    test_support::put_fixture(&store, item, b"pass").unwrap();
    let report = VerificationEngine::new_for_test(store, authority, current, Vec::new())
        .unwrap()
        .evaluate(None)
        .unwrap();
    assert_ne!(report.decision.state, CompletionState::VerifiedComplete);
}

#[test]
fn fresh_unrelated_evidence_remains_valid() {
    let (authority, current, root) = authority_fixture();
    let store = store(root.path());
    let mut item = metadata(
        &authority,
        &current,
        "unrelated",
        EvidenceClass::DatabaseQuery,
        EvidenceResult::Pass,
        EvidenceConfidence::StrongRuntime,
    );
    item.relevant_paths
        .insert("services/cloud-api/src/lib.rs".into());
    test_support::put_fixture(&store, item, b"database").unwrap();
    assert!(!store
        .impacted_by_change(
            "unrelated",
            &WorkspaceChangeSet {
                changed_paths: BTreeSet::from(["README.md".into()]),
                ..WorkspaceChangeSet::default()
            }
        )
        .unwrap());
}

#[test]
fn api_evidence_pass_is_structured() {
    let (authority, current, root) = authority_fixture();
    let binding = CollectorBinding::from_authority(
        &authority,
        &current,
        "api-pass",
        vec!["P8-TEST-01".into()],
        true,
        BTreeSet::from(["P8-TEST-01-criterion".into()]),
        BTreeSet::new(),
    )
    .unwrap();
    let command = command_for_exit(root.path(), true);
    let collected = ApiDatabaseCollector::default()
        .collect(&binding, &command)
        .unwrap();
    assert_eq!(collected.observation.status, GateStatus::Pass);
    assert_eq!(collected.observation.result, EvidenceResult::Pass);
}

#[test]
fn database_evidence_failure_is_structured() {
    let (authority, current, root) = authority_fixture();
    let binding = CollectorBinding::from_authority(
        &authority,
        &current,
        "api-fail",
        vec!["P8-TEST-01".into()],
        true,
        BTreeSet::from(["P8-TEST-01-criterion".into()]),
        BTreeSet::new(),
    )
    .unwrap();
    let command = command_for_exit(root.path(), false);
    let collected = ApiDatabaseCollector::default()
        .collect(&binding, &command)
        .unwrap();
    assert_eq!(collected.observation.result, EvidenceResult::Fail);
}

fn command_for_exit(root: &Path, success: bool) -> CommandSpec {
    let mut command = if cfg!(windows) {
        CommandSpec::new("cmd.exe", root)
    } else {
        CommandSpec::new("sh", root)
    };
    let code = if success { "0" } else { "1" };
    if cfg!(windows) {
        command.args = vec!["/C".into(), format!("echo collector && exit /B {code}")];
    } else {
        command.args = vec!["-c".into(), format!("printf collector; exit {code}")];
    }
    command
}

struct BrowserFixture;
impl BrowserRuntimeAdapter for BrowserFixture {
    fn run_route(&mut self, route: &str, steps: &[String]) -> Result<Vec<u8>, String> {
        Ok(format!("{route}:{}", steps.join(",")).into_bytes())
    }
}

#[test]
fn browser_runtime_evidence_binds_route_and_artifact() {
    let (authority, current, root) = authority_fixture();
    let binding = CollectorBinding::from_authority(
        &authority,
        &current,
        "browser-pass",
        vec!["P8-TEST-01".into()],
        true,
        BTreeSet::from(["P8-TEST-01-criterion".into()]),
        BTreeSet::new(),
    )
    .unwrap();
    let mut adapter = BrowserFixture;
    let collected = BrowserRuntimeCollector
        .collect(
            &binding,
            &mut adapter,
            "/verification",
            &["load".into(), "click".into()],
        )
        .unwrap();
    assert_eq!(collected.observation.route, "/verification");
    assert_eq!(collected.observation.result, EvidenceResult::Pass);
    drop(root);
}

#[test]
fn screenshot_alone_cannot_meet_strong_requirement() {
    assert!(!confidence_meets(
        EvidenceConfidence::Weak,
        EvidenceConfidence::StrongDeterministic
    ));
}

#[test]
fn accessibility_result_records_violations() {
    let (authority, current, root) = authority_fixture();
    let binding = CollectorBinding::from_authority(
        &authority,
        &current,
        "a11y-pass",
        vec!["P8-TEST-01".into()],
        true,
        BTreeSet::from(["P8-TEST-01-criterion".into()]),
        BTreeSet::new(),
    )
    .unwrap();
    let collected = AccessibilityCollector::default()
        .collect_process(&binding, &command_for_exit(root.path(), true), "/home")
        .unwrap();
    assert_eq!(collected.observation.result, EvidenceResult::Pass);
}

#[test]
fn performance_threshold_failure_is_visible() {
    let (authority, current, root) = authority_fixture();
    let binding = CollectorBinding::from_authority(
        &authority,
        &current,
        "perf-fail",
        vec!["P8-TEST-01".into()],
        true,
        BTreeSet::from(["P8-TEST-01-criterion".into()]),
        BTreeSet::new(),
    )
    .unwrap();
    let collected = PerformanceCollector::default()
        .measure(&binding, "p95", 400.0, 200.0, "ms", "real-runtime")
        .unwrap();
    assert_eq!(collected.observation.result, EvidenceResult::Fail);
    drop(root);
}

#[test]
fn security_deterministic_failure_is_visible() {
    let (authority, current, root) = authority_fixture();
    let binding = CollectorBinding::from_authority(
        &authority,
        &current,
        "security-fail",
        vec!["P8-TEST-01".into()],
        true,
        BTreeSet::from(["P8-TEST-01-criterion".into()]),
        BTreeSet::new(),
    )
    .unwrap();
    let collected = SecurityCollector::default()
        .collect_process(
            &binding,
            &command_for_exit(root.path(), false),
            CollectorIdentity::new("gitleaks", "8.30.1"),
            "crates",
        )
        .unwrap();
    assert_eq!(collected.observation.result, EvidenceResult::Fail);
}

#[test]
fn environment_fingerprint_mismatch_is_stale() {
    let (authority, current, root) = authority_fixture();
    let store = store(root.path());
    store
        .put_test_fixture(
            metadata(
                &authority,
                &current,
                "environment-mismatch",
                EvidenceClass::EnvironmentFingerprint,
                EvidenceResult::Pass,
                EvidenceConfidence::StrongDeterministic,
            ),
            b"env-a",
        )
        .unwrap();
    let mut changed = current.clone();
    changed.environment_fingerprint = "env-b".into();
    assert_eq!(
        store.freshness("environment-mismatch", &changed).unwrap(),
        EvidenceFreshness::Stale
    );
}

struct SupportedProvider;
impl IndependentAiProvider for SupportedProvider {
    fn judge(&self, input: AiVerifierInput) -> Result<AiVerifierJudgement, String> {
        Ok(AiVerifierJudgement {
            requirement_id: input.requirement_id.clone(),
            kind: AiJudgementKind::Supported,
            reasoning_summary: format!("reviewed {}", input.requirement_id),
            evidence_ids: input
                .evidence
                .into_iter()
                .map(|item| item.evidence_id)
                .collect(),
            confidence: EvidenceConfidence::AiReviewed,
            identified_gaps: vec![],
        })
    }
}

struct InconclusiveProvider;
impl IndependentAiProvider for InconclusiveProvider {
    fn judge(&self, _input: AiVerifierInput) -> Result<AiVerifierJudgement, String> {
        Ok(AiVerifierJudgement {
            requirement_id: "P8-TEST-01".into(),
            kind: AiJudgementKind::Inconclusive,
            reasoning_summary: "insufficient proof".into(),
            evidence_ids: vec![],
            confidence: EvidenceConfidence::Weak,
            identified_gaps: vec!["missing runtime".into()],
        })
    }
}

struct UnavailableProvider;
impl IndependentAiProvider for UnavailableProvider {
    fn judge(&self, _input: AiVerifierInput) -> Result<AiVerifierJudgement, String> {
        Err("provider unavailable".into())
    }
}

fn ai_input() -> AiVerifierInput {
    AiVerifierInput {
        requirement_id: "P8-TEST-01".into(),
        acceptance_criteria: vec!["criterion".into()],
        evidence: vec![],
        deterministic_gate_results: vec![],
        explicit_decisions: vec![],
    }
}

#[test]
fn ai_verifier_supported_judgement_is_structured() {
    assert_eq!(
        IndependentAiVerifier::new(SupportedProvider)
            .verify(ai_input())
            .unwrap()
            .kind,
        AiJudgementKind::Supported
    );
}

#[test]
fn ai_verifier_inconclusive_judgement_is_preserved() {
    assert_eq!(
        IndependentAiVerifier::new(InconclusiveProvider)
            .verify(ai_input())
            .unwrap()
            .kind,
        AiJudgementKind::Inconclusive
    );
}

#[test]
fn deterministic_failure_wins_over_ai_supported() {
    let (authority, current, root) = authority_fixture();
    let store = store(root.path());
    let mut item = metadata(
        &authority,
        &current,
        "deterministic-fail",
        EvidenceClass::TestOutput,
        EvidenceResult::Fail,
        EvidenceConfidence::StrongDeterministic,
    );
    item.requirement_ids = vec![
        authority.revision.contract.requirement_graph.requirements[0]
            .requirement_id
            .clone(),
    ];
    test_support::put_fixture(&store, item, b"exit=1").unwrap();
    let ai = IndependentAiVerifier::new(SupportedProvider)
        .verify(ai_input())
        .unwrap();
    let report = VerificationEngine::new_for_test(store, authority, current, Vec::new())
        .unwrap()
        .with_ai_judgements(vec![ai.clone()])
        .unwrap()
        .evaluate(None)
        .unwrap();
    assert_eq!(ai.kind, AiJudgementKind::Supported);
    assert_eq!(report.decision.state, CompletionState::FailedVerification);
}

#[test]
fn ai_provider_unavailable_is_truthful() {
    assert!(IndependentAiVerifier::new(UnavailableProvider)
        .verify(ai_input())
        .is_err());
    assert_eq!(
        production_ai_verifier_status().status,
        "PENDING_EXTERNAL_ENVIRONMENT"
    );

    let root = tempdir().unwrap();
    fs::write(
        root.path().join("package.json"),
        r#"{"scripts":{"build":"vite build","test":"vitest run","lint":"eslint ."}}"#,
    )
    .unwrap();
    let node_plan = VerificationCollectorPlan::discover(root.path()).unwrap();
    assert_eq!(node_plan.project_kind, "node");
    assert_eq!(
        node_plan
            .for_class(EvidenceClass::BuildOutput)
            .unwrap()
            .command
            .as_ref()
            .unwrap()
            .program,
        "npm"
    );
    assert!(!node_plan
        .for_class(EvidenceClass::BuildOutput)
        .unwrap()
        .command
        .as_ref()
        .unwrap()
        .args
        .iter()
        .any(|arg| arg == "cargo"));

    let config_dir = root.path().join(".relintor");
    fs::create_dir_all(&config_dir).unwrap();
    fs::write(
        config_dir.join("verification-plan.json"),
        r#"{"collectors":[
            {"class":"API_RESPONSE","program":"node","args":["api-probe.js"]},
            {"class":"BROWSER_RECORDING","program":"node","args":["browser-probe.js"],"route":"/health"},
            {"class":"SECURITY_SCAN","program":"gitleaks","args":["dir"]}
        ]}"#,
    )
    .unwrap();
    let configured_plan = VerificationCollectorPlan::discover(root.path()).unwrap();
    assert_eq!(
        configured_plan
            .for_class(EvidenceClass::ApiResponse)
            .unwrap()
            .command
            .as_ref()
            .unwrap()
            .program,
        "node"
    );
    assert_eq!(
        configured_plan
            .for_class(EvidenceClass::BrowserRecording)
            .unwrap()
            .route
            .as_deref(),
        Some("/health")
    );
    assert_eq!(
        configured_plan
            .for_class(EvidenceClass::SecurityScan)
            .unwrap()
            .command
            .as_ref()
            .unwrap()
            .program,
        "gitleaks"
    );
    assert!(configured_plan
        .for_class(EvidenceClass::PerformanceResult)
        .is_none());
}

#[test]
fn human_assertion_cannot_satisfy_machine_checkable_obligation() {
    let (authority, current, root) = authority_fixture();
    let store = store(root.path());
    let mut item = metadata(
        &authority,
        &current,
        "human-only",
        EvidenceClass::HumanDecision,
        EvidenceResult::Pass,
        EvidenceConfidence::HumanAsserted,
    );
    item.requirement_ids = vec![
        authority.revision.contract.requirement_graph.requirements[0]
            .requirement_id
            .clone(),
    ];
    test_support::put_fixture(&store, item, b"human says done").unwrap();
    let report = VerificationEngine::new_for_test(store, authority, current, Vec::new())
        .unwrap()
        .evaluate(Some("DONE".into()));
    assert_ne!(
        report.unwrap().decision.state,
        CompletionState::VerifiedComplete
    );
}

#[test]
fn explicit_accepted_risk_is_recorded() {
    let (root, store, authority, current, _) = report_with_all_evidence();
    let requirement = authority.revision.contract.requirement_graph.requirements[0]
        .requirement_id
        .clone();
    let decision = relintor_standards::ExplicitDecision {
        decision_id: "risk-1".into(),
        kind: DecisionKind::ExplicitException,
        actor: DecisionActor::Human,
        requirement_id: requirement,
        reason: "documented low risk".into(),
        risk: "minor".into(),
        timestamp: "2026-08-15".into(),
        mission_revision: authority.revision.revision,
        provenance: "human-decision".into(),
    };
    let report = VerificationEngine::new_for_test(store, authority, current, vec![decision])
        .unwrap()
        .evaluate(None)
        .unwrap();
    assert!(!report.decision.accepted_risks.is_empty());
    drop(root);
}

#[test]
fn non_waivable_requirement_cannot_be_risk_accepted() {
    let (authority, current, root) = authority_fixture_with_risk(RequirementRisk::Critical);
    let store = store(root.path());
    add_for_all(&store, &authority, &current);
    let decision = relintor_standards::ExplicitDecision {
        decision_id: "risk-critical".into(),
        kind: DecisionKind::ExplicitException,
        actor: DecisionActor::Human,
        requirement_id: authority.revision.contract.requirement_graph.requirements[0]
            .requirement_id
            .clone(),
        reason: "not allowed".into(),
        risk: "critical".into(),
        timestamp: "2026-08-15".into(),
        mission_revision: authority.revision.revision,
        provenance: "human".into(),
    };
    let report = VerificationEngine::new_for_test(store, authority, current, vec![decision])
        .unwrap()
        .evaluate(None)
        .unwrap();
    assert!(report.decision.accepted_risks.is_empty());
}

#[test]
fn blocked_external_requirement_prevents_normal_completion() {
    let (authority, current, root) = authority_fixture();
    let store = store(root.path());
    let mut item = metadata(
        &authority,
        &current,
        "external-blocked",
        EvidenceClass::ExternalServiceReceipt,
        EvidenceResult::Blocked,
        EvidenceConfidence::StrongRuntime,
    );
    item.requirement_ids = vec![
        authority.revision.contract.requirement_graph.requirements[0]
            .requirement_id
            .clone(),
    ];
    store
        .put_test_fixture(item, b"postgres unavailable")
        .unwrap();
    let report = VerificationEngine::new_for_test(store, authority, current, Vec::new())
        .unwrap()
        .evaluate(None)
        .unwrap();
    assert_eq!(report.decision.state, CompletionState::BlockedExternal);
}

#[test]
fn one_missing_requirement_prevents_completion() {
    let (authority, current, root) = authority_fixture();
    let store = store(root.path());
    let only = authority.revision.contract.requirement_graph.requirements[0]
        .requirement_id
        .clone();
    let mut item = metadata(
        &authority,
        &current,
        "one-required",
        EvidenceClass::TestOutput,
        EvidenceResult::Pass,
        EvidenceConfidence::StrongDeterministic,
    );
    item.requirement_ids = vec![only];
    test_support::put_fixture(&store, item, b"pass").unwrap();
    let report = VerificationEngine::new_for_test(store, authority, current, Vec::new())
        .unwrap()
        .evaluate(None)
        .unwrap();
    assert_ne!(report.decision.state, CompletionState::VerifiedComplete);
}

#[test]
fn complete_valid_coverage_permits_verified_complete() {
    let (_, _, _, _, report) = report_with_all_evidence();
    assert_eq!(report.decision.state, CompletionState::VerifiedComplete);
}

#[test]
fn completion_certificate_tampering_is_rejected() {
    let (_, _, authority, _, report) = report_with_all_evidence();
    let authority_signer = CompletionAuthority::new(b"p8-test-local-key").unwrap();
    let mut certificate = authority_signer
        .issue_for_test(&report, &authority)
        .unwrap();
    certificate.final_state = CompletionState::FailedVerification;
    assert!(authority_signer.validate(&certificate, &authority).is_err());
}

#[test]
fn evidence_manifest_tampering_changes_digest() {
    let (root, store, authority, _, report) = report_with_all_evidence();
    let mut manifest = export_manifest(&report, &authority, &store, vec![], None).unwrap();
    let old = manifest.digest().unwrap();
    manifest.final_state = CompletionState::FailedVerification;
    assert_ne!(old, manifest.digest().unwrap());
    drop(root);
}

#[test]
fn builder_done_with_missing_evidence_is_rejected() {
    let (authority, current, root) = authority_fixture();
    let store = store(root.path());
    let report = VerificationEngine::new_for_test(store, authority, current, Vec::new())
        .unwrap()
        .evaluate(Some("DONE".into()))
        .unwrap();
    assert_eq!(report.builder_claim.as_deref(), Some("DONE"));
    assert_ne!(report.decision.state, CompletionState::VerifiedComplete);
}

#[test]
fn p7_finished_state_alone_does_not_imply_verified() {
    let (authority, current, root) = authority_fixture();
    let store = store(root.path());
    let report = VerificationEngine::new_for_test(store, authority, current, Vec::new())
        .unwrap()
        .evaluate(None)
        .unwrap();
    assert_ne!(report.decision.state, CompletionState::VerifiedComplete);
}

#[test]
fn p6_mission_revision_change_requires_revalidation() {
    let (mut authority, current, root) = authority_fixture();
    authority.revision.revision += 1;
    assert!(
        VerificationEngine::new_for_test(store(root.path()), authority, current, Vec::new())
            .is_err()
    );
}

#[test]
fn certificate_becomes_invalid_after_authority_source_change() {
    let (_, _, mut authority, _, report) = report_with_all_evidence();
    let signer = CompletionAuthority::new(b"p8-test-local-key").unwrap();
    let certificate = signer.issue_for_test(&report, &authority).unwrap();
    authority.workspace_fingerprint = "changed-workspace".into();
    assert!(signer.validate(&certificate, &authority).is_err());
}

#[test]
fn not_applicable_and_deferred_states_are_explicitly_accounted() {
    let (_, _, _, _, verified) = report_with_all_evidence();
    assert_eq!(verified.decision.state, CompletionState::VerifiedComplete);

    let deferred = relintor_standards::ExplicitDecision {
        decision_id: "defer-a".into(),
        kind: DecisionKind::Defer,
        actor: DecisionActor::Human,
        requirement_id: "P8-TEST-01".into(),
        reason: "external dependency is not available".into(),
        risk: "temporary local verification gap".into(),
        timestamp: "2026-08-15".into(),
        mission_revision: 1,
        provenance: "human-review".into(),
    };
    let (authority, current, root) =
        authority_fixture_with_decisions(RequirementRisk::Medium, vec![deferred.clone()]);
    let incomplete = VerificationEngine::new_for_test(
        store(root.path()),
        authority,
        current,
        vec![deferred.clone()],
    )
    .unwrap()
    .evaluate(None)
    .unwrap();
    assert_ne!(
        incomplete.decision.state,
        CompletionState::CompleteWithAcceptedRisks
    );

    let defer_a = relintor_standards::ExplicitDecision {
        decision_id: "defer-a-only".into(),
        kind: DecisionKind::Defer,
        actor: DecisionActor::Human,
        requirement_id: "P8-TEST-01".into(),
        reason: "dependency A is unavailable".into(),
        risk: "bounded A risk".into(),
        timestamp: "2026-08-15".into(),
        mission_revision: 1,
        provenance: "human-review".into(),
    };
    let defer_b = relintor_standards::ExplicitDecision {
        decision_id: "defer-b".into(),
        kind: DecisionKind::Defer,
        actor: DecisionActor::Human,
        requirement_id: "P8-TEST-02".into(),
        reason: "dependency B is unavailable".into(),
        risk: "bounded B risk".into(),
        timestamp: "2026-08-15".into(),
        mission_revision: 1,
        provenance: "human-review".into(),
    };
    let (authority, current, root) = authority_fixture_with_decisions(
        RequirementRisk::Medium,
        vec![defer_a.clone(), defer_b.clone()],
    );
    let missing_b_risk = VerificationEngine::new_for_test(
        store(root.path()),
        authority,
        current,
        vec![defer_a.clone()],
    )
    .unwrap()
    .evaluate(None)
    .unwrap();
    assert_ne!(
        missing_b_risk.decision.state,
        CompletionState::CompleteWithAcceptedRisks
    );

    let (authority, current, root) = authority_fixture_with_decisions(
        RequirementRisk::Medium,
        vec![defer_a.clone(), defer_b.clone()],
    );
    let complete_store = store(root.path());
    add_for_all(&complete_store, &authority, &current);
    let complete_with_risk = VerificationEngine::new_for_test(
        complete_store,
        authority,
        current,
        vec![defer_a, defer_b],
    )
    .unwrap()
    .evaluate(None)
    .unwrap();
    assert_eq!(
        complete_with_risk.decision.state,
        CompletionState::CompleteWithAcceptedRisks,
        "decision={:?} statuses={:?}",
        complete_with_risk.decision,
        complete_with_risk.requirement_statuses
    );

    let (authority, current, root) =
        authority_fixture_with_decisions(RequirementRisk::Critical, vec![deferred.clone()]);
    let non_waivable =
        VerificationEngine::new_for_test(store(root.path()), authority, current, vec![deferred])
            .unwrap()
            .evaluate(None)
            .unwrap();
    assert_ne!(
        non_waivable.decision.state,
        CompletionState::CompleteWithAcceptedRisks
    );
}

#[test]
fn duplicate_evidence_does_not_increase_requirement_coverage() {
    let (root, store, authority, current, report) = report_with_all_evidence();
    let first = authority.revision.contract.requirement_graph.requirements[0]
        .requirement_id
        .clone();
    let mut duplicate = metadata(
        &authority,
        &current,
        "duplicate",
        EvidenceClass::TestOutput,
        EvidenceResult::Pass,
        EvidenceConfidence::StrongDeterministic,
    );
    duplicate.requirement_ids = vec![first];
    test_support::put_fixture(&store, duplicate, b"same proof").unwrap();
    let second = VerificationEngine::new_for_test(store, authority, current, Vec::new())
        .unwrap()
        .evaluate(None)
        .unwrap();
    assert_eq!(second.coverage_total, report.coverage_total);
    drop(root);
}

#[test]
fn process_collector_records_actual_exit_and_bounded_output() {
    let root = tempdir().unwrap();
    let mut command = if cfg!(windows) {
        CommandSpec::new("cmd.exe", root.path())
    } else {
        CommandSpec::new("sh", root.path())
    };
    if cfg!(windows) {
        command.args = vec!["/C".into(), "echo p8 && exit /B 0".into()];
    } else {
        command.args = vec!["-c".into(), "printf p8".into()];
    }
    let collector = ProcessCollector {
        max_output_bytes: 16,
        ..ProcessCollector::default()
    };
    let result = collector.run(&command).unwrap();
    assert_eq!(result.exit_code, Some(0));
    assert_eq!(result.result, EvidenceResult::Pass);
    assert!(!result.stdout.is_empty());
}

#[test]
fn process_collector_has_a_bounded_wait() {
    let root = tempdir().unwrap();
    let mut command = if cfg!(windows) {
        CommandSpec::new("ping.exe", root.path())
    } else {
        CommandSpec::new("sleep", root.path())
    };
    if cfg!(windows) {
        command.args = vec!["127.0.0.1".into(), "-n".into(), "20".into()];
    } else {
        command.args = vec!["20".into()];
    }
    let collector = ProcessCollector {
        timeout: std::time::Duration::from_millis(50),
        ..ProcessCollector::default()
    };
    let error = collector
        .run(&command)
        .expect_err("a collector must not wait forever");
    assert!(error.to_string().contains("time limit"));
}

#[test]
fn certificate_validation_rejects_invalidated_evidence() {
    let (root, store, authority, current, report) = report_with_all_evidence();
    let signer = CompletionAuthority::new(b"p8-test-local-key").unwrap();
    let certificate = signer.issue_for_test(&report, &authority).unwrap();
    let manifest = export_manifest(&report, &authority, &store, vec![], None).unwrap();
    signer
        .validate_with_store(&certificate, &authority, &store, &manifest, &current)
        .unwrap();
    let evidence_id = manifest.evidence[0].evidence_id.clone();
    store
        .invalidate(&evidence_id, "source changed after certificate")
        .unwrap();
    assert!(signer
        .validate_with_store(&certificate, &authority, &store, &manifest, &current)
        .is_err());
    drop(root);
}

#[test]
fn source_fingerprint_changes_for_same_size_file_change() {
    let root = tempdir().unwrap();
    let path = root.path().join("same-size.txt");
    fs::write(&path, b"aaaa").unwrap();
    let before = fingerprint_workspace(root.path()).unwrap();
    fs::write(&path, b"bbbb").unwrap();
    let after = fingerprint_workspace(root.path()).unwrap();
    assert_ne!(before, after);
}
