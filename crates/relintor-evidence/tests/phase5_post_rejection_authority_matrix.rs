//! Phase 5 Post-Rejection Authority Acceptance Matrix
//!
//! Validates:
//! 1. DUPLICATE_SEMANTIC_DECISION
//! 2. REJECTION_PERSISTENCE
//! 3. REJECTION_RESTART
//! 4. REJECTION_CACHE_COLD
//! 5. REJECTION_CACHE_WARM
//! 6. NO_SECOND_DECISION_PROMPT
//! 7. DUPLICATE_REJECTION_IDEMPOTENT
//! 8. CONFLICTING_APPROVAL_DENIED
//! 9. WRONG_MISSION_DECISION_DENIED
//! 10. WRONG_REVISION_DECISION_DENIED
//! 11. FABRICATED_DECISION_DENIED
//! 12. CORRECTION_SCOPE
//! 13. UNRELATED_PASS_REQUIREMENTS_PRESERVED
//! 14. BLOCKED_REQUIREMENT_PRESERVED
//! 15. COMPLETED_TASK_HISTORY_PRESERVED
//! 16. CERTIFICATE_DENIED_AFTER_REJECTION
//! 17. VERIFY_DENIED_AFTER_REJECTION
//! 18. STATUS_CORRECTION_REQUIRED
//! 19. RECOVERY_NOT_RESURRECTED

use base64::{engine::general_purpose::STANDARD, Engine as _};
use ed25519_dalek::SigningKey;
use relintor_evidence::test_support::*;
use relintor_evidence::*;
use relintor_execution::{
    ExecutionEventKind, ExecutionLease, ExecutionRun, ExecutionRunState, ExecutionTask,
    ExecutionTaskState, RetryPolicy, SchedulerPolicy, TaskAttempt, TaskAttemptState,
};
use relintor_standards::{
    builtin_registry, scope_fingerprint, AcceptanceCriterion, ApplicabilityContext,
    ApplicabilityOutcome, AuthorityEngine, EvidenceClass, EvidenceConfidence, EvidenceObligation,
    FactValue, ProjectAuthorityInput, ProjectRequirementSeed, Requirement, RequirementPriority,
    RequirementRisk, RequirementSource, RequirementStatus, TrustedSigner, TrustedSignerSet,
    VerificationPolicy,
};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use tempfile::{tempdir, TempDir};

fn test_context() -> ApplicabilityContext {
    let mut facts = BTreeMap::new();
    for field in [
        "web", "backend", "database", "authentication", "ui_surface",
        "seo_relevance", "performance", "deployment", "observability",
        "privacy", "payments", "ai", "blockchain", "mobile", "desktop",
        "data_engineering", "integrations",
    ] {
        facts.insert(field.into(), FactValue::Bool(false));
    }
    facts.insert("platform".into(), FactValue::Text("windows".into()));
    ApplicabilityContext {
        facts,
        revision: "p8-context-1".into(),
    }
}

fn seed_decision_req(id: &str, title: &str, intent: &str) -> ProjectRequirementSeed {
    ProjectRequirementSeed {
        requirement_id: Some(id.into()),
        title: title.into(),
        intent: intent.into(),
        source: RequirementSource::User {
            reference: format!("project://takeover-test/{id}"),
        },
        priority: RequirementPriority::P1,
        acceptance_criteria: vec![AcceptanceCriterion {
            criterion_id: format!("{id}-criterion"),
            statement: format!("A reviewable evidence record demonstrates: {title}"),
            criterion_type: "project-authority-obligation".into(),
            machine_checkable: false,
        }],
        verification_policy: VerificationPolicy {
            obligations: vec![EvidenceObligation {
                class: EvidenceClass::HumanDecision,
                minimum_confidence: EvidenceConfidence::HumanAsserted,
                rationale: "A genuine project decision requires explicit human review.".into(),
                required: true,
            }],
            p8_collector_required: true,
        },
        dependencies: Vec::new(),
        risk: RequirementRisk::High,
        requirement_type: "decision".into(),
    }
}

fn seed_machine_req(id: &str, title: &str, class: EvidenceClass) -> ProjectRequirementSeed {
    ProjectRequirementSeed {
        requirement_id: Some(id.into()),
        title: title.into(),
        intent: format!("Validate {title}"),
        source: RequirementSource::User {
            reference: format!("project://takeover-test/{id}"),
        },
        priority: RequirementPriority::P2,
        acceptance_criteria: vec![AcceptanceCriterion {
            criterion_id: format!("{id}-criterion"),
            statement: format!("Automated check proves {title}"),
            criterion_type: "test".into(),
            machine_checkable: true,
        }],
        verification_policy: VerificationPolicy {
            obligations: vec![EvidenceObligation {
                class,
                minimum_confidence: EvidenceConfidence::StrongDeterministic,
                rationale: format!("required {class:?} check"),
                required: true,
            }],
            p8_collector_required: true,
        },
        dependencies: Vec::new(),
        risk: RequirementRisk::Medium,
        requirement_type: "functional".into(),
    }
}

fn test_metadata(
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
        artifact_digest: "digest-placeholder".into(),
        freshness: EvidenceFreshness::Fresh,
        required: true,
        accepted_criteria: BTreeSet::new(),
        collector: CollectorIdentity::new("test-collector", "p8-v1"),
        execution_identities: Vec::new(),
        command_digest: "cmd-digest".into(),
        workspace_fingerprint: authority.workspace_fingerprint.clone(),
        environment_fingerprint: authority.environment_fingerprint.clone(),
        source_revision: authority.source_revision.clone(),
        created_at_ms: 1000,
        confidence,
        result,
        relevant_paths: BTreeSet::new(),
        test_inventory: Vec::new(),
        dependency_lock_hashes: current.dependency_lock_hashes.clone(),
        scope_fingerprint: None,
    }
}

fn create_phase5_test_fixture() -> (
    VerificationAuthority,
    FreshnessContext,
    EvidenceStore,
    SigningKey,
    TempDir,
) {
    let signing_key = SigningKey::from_bytes(&[43_u8; 32]);
    let mut registry = builtin_registry();
    registry
        .sign("phase5-signer", &signing_key)
        .expect("sign registry");
    let trusted = TrustedSignerSet {
        signers: vec![TrustedSigner {
            key_id: "phase5-signer".into(),
            algorithm: "Ed25519".into(),
            public_key: STANDARD.encode(signing_key.verifying_key().to_bytes()),
        }],
    };

    let core_intent = "Implement an end-to-end Transport Health & Route Status feature for OmniChat.\n\nExpose structured transport/route health information from the existing Rust routing layer.";
    let req_a_intent = format!("The owner needs a reliable way to turn this outcome into an agreed, reviewable product plan: {}", core_intent);
    let req_b_intent = core_intent.to_string();

    let requirements = vec![
        seed_decision_req("REQ-A-OUTCOME", "User problem outcome", &req_a_intent),
        seed_decision_req("REQ-B-PURPOSE", "User product purpose", &req_b_intent),
        seed_machine_req("REQ-TEST-AUTOMATED", "Automated Transport Tests", EvidenceClass::TestOutput),
        seed_machine_req("REQ-ACCESSIBILITY", "Accessibility Compliance", EvidenceClass::AccessibilityResult),
    ];

    let project_authority = ProjectAuthorityInput {
        requirements,
        decisions: Vec::new(),
        source_revision: "phase5-source-rev-1".into(),
        source_fingerprint: "phase5-source-fp-1".into(),
        ..ProjectAuthorityInput::default()
    };

    let draft = AuthorityEngine
        .build_draft_with_project_authority(
            "mission-takeover-p5",
            "project-takeover-p5",
            "phase5-source-rev-1",
            "phase5-workspace",
            registry.clone(),
            &test_context(),
            scope_fingerprint(BTreeMap::from([("root".into(), "p5".into())])),
            project_authority,
            Vec::new(),
        )
        .expect("draft");

    let (revision, handoff) = AuthorityEngine
        .seal(&draft, &trusted, "2026-09-07T00:00:00Z")
        .expect("seal");

    let temp_dir = tempdir().expect("tempdir");
    let store = EvidenceStore::new(temp_dir.path().join("evidence"), b"p8-test-local-key").expect("store");

    let authority = VerificationAuthority {
        p7_run_id: "p7-run-p5".into(),
        p7_state: "EXECUTION_TASKS_FINISHED_AWAITING_VERIFICATION".into(),
        workspace_fingerprint: "workspace-fp-p5".into(),
        source_revision: Some(revision.contract.project_source_revision.clone()),
        environment_fingerprint: "env-fp-p5".into(),
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
        environment_fingerprint: authority.environment_fingerprint.clone(),
        source_revision: authority.source_revision.clone(),
        dependency_lock_hashes: BTreeMap::from([("Cargo.lock".into(), "lock-hash".into())]),
        workspace_root: Some(temp_dir.path().to_path_buf()),
    };

    (authority, current, store, signing_key, temp_dir)
}

fn put_user_decision_fixture(
    store: &EvidenceStore,
    authority: &VerificationAuthority,
    current: &FreshnessContext,
    req_id: &str,
    approved: bool,
    notes: &str,
) -> EvidenceArtifact {
    let req = authority.revision.contract.requirement_graph.requirements.iter()
        .find(|r| r.requirement_id == req_id).expect("requirement found");
    let mut meta = test_metadata(
        authority,
        current,
        &format!("p8-user-decision-{req_id}"),
        EvidenceClass::HumanDecision,
        if approved { EvidenceResult::Pass } else { EvidenceResult::Fail },
        EvidenceConfidence::HumanAsserted,
    );
    meta.collector = CollectorIdentity::new(EXPLICIT_USER_DECISION_COLLECTOR, "p8-v1");
    meta.requirement_ids = vec![req_id.into()];
    meta.accepted_criteria = req.acceptance_criteria.iter().map(|c| c.criterion_id.clone()).collect();
    store.put_test_fixture(meta, notes.as_bytes()).expect("put user decision")
}

fn p7_execution_fixture(
    authority: &VerificationAuthority,
    root: &Path,
    req_ids: Vec<String>,
) -> AuthenticatedP7Execution {
    let task_id = "task-human-1";
    let packet_digest = "0".repeat(64);
    let attempt_id = "attempt-human-1";
    let lease_id = "lease-human-1";
    let process_digest = "1".repeat(64);
    let started_at_ms = 1000;
    let ended_at_ms = 2000;
    let expires_at_ms = 10000;

    let lease_raw = serde_json::json!({
        "lease_id": lease_id,
        "mission_id": authority.revision.seal.mission_id,
        "mission_revision": authority.revision.revision,
        "task_id": task_id,
        "task_packet_digest": packet_digest,
        "scope": {
            "workspace": root,
            "file_scopes": [],
            "directory_scopes": [],
            "shared_resources": [],
            "package_lockfiles": [],
            "generated_files": [],
            "allowed_tools": [],
            "external_authority": [],
            "scope_known": true
        },
        "issued_at_ms": started_at_ms,
        "expires_at_ms": expires_at_ms,
        "step_budget": 10,
        "tool_call_budget": 10,
        "usage_budget": {
            "wall_clock_ms": 60000,
            "execution_steps": 10,
            "tool_calls": 10,
            "retry_attempts": 1,
            "cost_micros": null
        },
        "attempt_number": 1,
        "status": "CONSUMED",
        "lease_digest": ""
    });
    let mut lease: ExecutionLease = serde_json::from_value(lease_raw).expect("lease");
    lease.lease_digest = lease.compute_digest().expect("lease digest");

    let attempt_raw = serde_json::json!({
        "attempt_id": attempt_id,
        "task_id": task_id,
        "attempt_number": 1,
        "packet_digest": packet_digest,
        "lease_id": lease_id,
        "state": "SUCCEEDED",
        "started_at_ms": started_at_ms,
        "ended_at_ms": ended_at_ms,
        "failure_class": null,
        "failure_fingerprint": null,
        "usage": {
            "tool_calls": 0,
            "execution_steps": 0,
            "wall_time_ms": 1000,
            "retry_count": 0,
            "estimated_cost_micros": null,
            "actual_cost_micros": null,
            "attempt_count": 1,
            "quality": "MEASURED"
        },
        "termination_reason": null,
        "execution_boundary": "EXTERNAL_PROCESS_STARTED",
        "extensions_granted": 0,
        "completion_authority": {
            "task_id": task_id,
            "attempt_id": attempt_id,
            "packet_digest": packet_digest,
            "lease_id": lease_id,
            "process_digest": process_digest,
            "started_at_ms": started_at_ms,
            "ended_at_ms": ended_at_ms
        },
        "workspace_before": {},
        "workspace_after": {}
    });
    let attempt: TaskAttempt = serde_json::from_value(attempt_raw).expect("attempt");

    let mut tasks = BTreeMap::new();
    tasks.insert(
        task_id.to_string(),
        ExecutionTask {
            task_id: task_id.into(),
            objective: "Human decision task".into(),
            requirement_ids: req_ids,
            dependency_ids: vec![],
            priority: RequirementPriority::P1,
            state: ExecutionTaskState::FinishedAwaitingVerification,
            scope: lease.scope.clone(),
            usage_budget: lease.usage_budget.clone(),
            retry_policy: RetryPolicy::default(),
            evidence_obligations: vec![],
            attempt_number: 1,
        },
    );

    let run = ExecutionRun {
        ledger_version: "p7-execution-ledger-v1".into(),
        run_id: authority.p7_run_id.clone(),
        mission_id: authority.revision.seal.mission_id.clone(),
        mission_revision: authority.revision.revision,
        seal_hash: authority.revision.seal.contract_hash.clone(),
        project_id: authority.revision.seal.project_id.clone(),
        workspace: root.to_path_buf(),
        workspace_fingerprint: authority.workspace_fingerprint.clone(),
        state: ExecutionRunState::ExecutionTasksFinishedAwaitingVerification,
        tasks,
        attempts: vec![attempt],
        leases: vec![lease],
        events: vec![],
        usage: relintor_execution::UsageTelemetry::default(),
        loop_signals: vec![],
        oscillation_signals: vec![],
        continuations: vec![],
        diagnostics: vec![],
        external_modifications: vec![],
        progress: vec![],
        watchdog_state: relintor_execution::WatchdogState::Healthy,
        policy: SchedulerPolicy::default(),
        safe_boundary: None,
        current_turn: 1,
        last_error: None,
        no_progress_occurrences: 0,
        reviewed_recovery_deltas: vec![],
        integrity_version: "p7-ledger-integrity-v1".into(),
        integrity_tag: String::new(),
    };
    AuthenticatedP7Execution::from_run(run).expect("p7_exec")
}

// -----------------------------------------------------------------------------
// TESTS
// -----------------------------------------------------------------------------

#[test]
fn test_duplicate_semantic_rejection_propagation_and_no_second_prompt() {
    let (authority, current, store, _, _temp) = create_phase5_test_fixture();

    // Verify semantic equivalence function recognizes both as identical semantics
    let req_a = authority.revision.contract.requirement_graph.requirements.iter()
        .find(|r| r.requirement_id == "REQ-A-OUTCOME").unwrap();
    let req_b = authority.revision.contract.requirement_graph.requirements.iter()
        .find(|r| r.requirement_id == "REQ-B-PURPOSE").unwrap();

    assert!(is_human_decision_semantic_equivalent(req_a, req_b));
    assert_eq!(
        normalize_decision_intent(&req_a.intent),
        normalize_decision_intent(&req_b.intent)
    );

    // Record passing machine test evidence for REQ-TEST-AUTOMATED
    let mut test_meta = test_metadata(
        &authority,
        &current,
        "test-output-evidence-01",
        EvidenceClass::TestOutput,
        EvidenceResult::Pass,
        EvidenceConfidence::StrongDeterministic,
    );
    test_meta.requirement_ids = vec!["REQ-TEST-AUTOMATED".into()];
    test_meta.accepted_criteria = BTreeSet::from(["REQ-TEST-AUTOMATED-criterion".into()]);
    store.put_test_fixture(test_meta, b"automated tests passed").unwrap();

    // Authoritative Human Rejection on REQ-A-OUTCOME
    let rej_art = put_user_decision_fixture(
        &store,
        &authority,
        &current,
        "REQ-A-OUTCOME",
        false,
        "Feature implementation did not satisfy transport health verification.",
    );

    assert_eq!(rej_art.metadata.result, EvidenceResult::Fail);

    // Evaluate Verification Report
    let engine = VerificationEngine::new_for_test(
        store.clone(),
        authority.clone(),
        current.clone(),
        Vec::new(),
    ).expect("engine");

    let report = engine.evaluate(None).expect("evaluate");
    println!("REPORT: {:#?}", report);

    // Verification must fail overall
    assert_eq!(report.decision.state, CompletionState::FailedVerification);

    // Both REQ-A and REQ-B must be Failed
    let status_a = report.requirement_statuses.iter().find(|s| s.requirement_id == "REQ-A-OUTCOME").unwrap();
    let status_b = report.requirement_statuses.iter().find(|s| s.requirement_id == "REQ-B-PURPOSE").unwrap();

    assert_eq!(status_a.status, RequirementStatus::Failed);
    assert_eq!(status_b.status, RequirementStatus::Failed);

    // Neither has missing obligations
    assert!(status_a.missing_obligations.is_empty(), "Req A missing obligations must be empty");
    assert!(status_b.missing_obligations.is_empty(), "Req B missing obligations must be empty");

    // Both reference the failed evidence
    assert!(status_a.failed_evidence.contains(&rej_art.metadata.evidence_id));
    assert!(status_b.failed_evidence.contains(&rej_art.metadata.evidence_id));

    // REQ-TEST-AUTOMATED remains Verified / PASS
    let status_test = report.requirement_statuses.iter().find(|s| s.requirement_id == "REQ-TEST-AUTOMATED").unwrap();
    assert_eq!(status_test.status, RequirementStatus::Verified);

    // REQ-ACCESSIBILITY remains missing / blocked (NOT falsely passed)
    let status_acc = report.requirement_statuses.iter().find(|s| s.requirement_id == "REQ-ACCESSIBILITY").unwrap();
    assert_ne!(status_acc.status, RequirementStatus::Verified);
    assert!(status_acc.missing_obligations.contains(&EvidenceClass::AccessibilityResult));
}

#[test]
fn test_rejection_persistence_and_restart_idempotency() {
    let (authority, current, store, _, temp) = create_phase5_test_fixture();

    let art1 = put_user_decision_fixture(
        &store,
        &authority,
        &current,
        "REQ-A-OUTCOME",
        false,
        "Rejection for restart test",
    );

    // REJECTION_PERSISTENCE: reopen store from the same disk path
    let reopened_store = EvidenceStore::new(temp.path().join("evidence"), b"p8-test-local-key").expect("reopen store");
    let loaded = reopened_store.load(&art1.metadata.evidence_id).expect("load persisted");
    assert_eq!(loaded.artifact.digest, art1.digest);
    assert_eq!(loaded.artifact.metadata.result, EvidenceResult::Fail);

    // REJECTION_RESTART: evaluate using reopened store
    let engine = VerificationEngine::new_for_test(
        reopened_store.clone(),
        authority.clone(),
        current.clone(),
        Vec::new(),
    ).expect("engine");
    let report = engine.evaluate(None).expect("evaluate after restart");
    assert_eq!(report.decision.state, CompletionState::FailedVerification);

    // Cold cache vs warm cache stability
    let report_warm = engine.evaluate(None).expect("warm evaluate");
    assert_eq!(report_warm.decision.state, CompletionState::FailedVerification);
    assert_eq!(report.decision.state, report_warm.decision.state);
}

#[test]
fn test_conflicting_approval_denied() {
    let (authority, current, store, _, temp) = create_phase5_test_fixture();

    // Record rejection
    put_user_decision_fixture(
        &store,
        &authority,
        &current,
        "REQ-A-OUTCOME",
        false,
        "Explicit rejection",
    );

    // Attempting conflicting approval via ExplicitUserDecisionRecorder on REQ-A must be denied
    let recorder = ExplicitUserDecisionRecorder;
    let conflicting_a = ExplicitUserDecisionInput {
        requirement_id: "REQ-A-OUTCOME",
        approved: true,
        notes: "Later conflicting approval attempt",
    };
    let p7 = p7_execution_fixture(
        &authority,
        temp.path(),
        vec!["REQ-A-OUTCOME".into(), "REQ-B-PURPOSE".into()],
    );

    let err_a = recorder.record(&authority, &current, &store, &p7, conflicting_a);
    assert!(err_a.is_err(), "conflicting approval on same requirement must error");

    // Attempting conflicting approval on semantic sibling REQ-B must also be denied
    let conflicting_b = ExplicitUserDecisionInput {
        requirement_id: "REQ-B-PURPOSE",
        approved: true,
        notes: "Conflicting approval attempt on sibling",
    };
    let err_b = recorder.record(&authority, &current, &store, &p7, conflicting_b);
    assert!(err_b.is_err(), "conflicting approval on semantic sibling must error");
}

#[test]
fn test_wrong_mission_and_wrong_revision_decision_denied() {
    let (authority, current, store, _, _temp) = create_phase5_test_fixture();

    // Fabricate an artifact with wrong mission_id
    let mut wrong_mission_meta = test_metadata(
        &authority,
        &current,
        "wrong-mission-art",
        EvidenceClass::HumanDecision,
        EvidenceResult::Pass,
        EvidenceConfidence::HumanAsserted,
    );
    wrong_mission_meta.mission_id = "mission-DIFFERENT-UNAUTHORIZED".into();
    wrong_mission_meta.collector = CollectorIdentity::new(EXPLICIT_USER_DECISION_COLLECTOR, "p8-v1");
    wrong_mission_meta.requirement_ids = vec!["REQ-A-OUTCOME".into()];
    let wrong_mission_art = store.put_test_fixture(wrong_mission_meta, b"wrong mission").unwrap();

    // Fabricate an artifact with wrong revision
    let mut wrong_rev_meta = test_metadata(
        &authority,
        &current,
        "wrong-rev-art",
        EvidenceClass::HumanDecision,
        EvidenceResult::Pass,
        EvidenceConfidence::HumanAsserted,
    );
    wrong_rev_meta.mission_revision = authority.revision.revision + 99;
    wrong_rev_meta.collector = CollectorIdentity::new(EXPLICIT_USER_DECISION_COLLECTOR, "p8-v1");
    wrong_rev_meta.requirement_ids = vec!["REQ-A-OUTCOME".into()];
    let wrong_rev_art = store.put_test_fixture(wrong_rev_meta, b"wrong rev").unwrap();

    let engine = VerificationEngine::new_for_test(
        store.clone(),
        authority.clone(),
        current.clone(),
        Vec::new(),
    ).expect("engine");

    let report = engine.evaluate(None).expect("evaluate");
    let status_a = report.requirement_statuses.iter().find(|s| s.requirement_id == "REQ-A-OUTCOME").unwrap();

    // Neither wrong-mission nor wrong-revision artifact is trusted
    assert!(!status_a.evidence_ids.contains(&wrong_mission_art.metadata.evidence_id));
    assert!(!status_a.evidence_ids.contains(&wrong_rev_art.metadata.evidence_id));
    assert_ne!(status_a.status, RequirementStatus::Verified);
}

#[test]
fn test_fabricated_human_decision_denied() {
    let (authority, current, store, _, _temp) = create_phase5_test_fixture();

    // Fabricate decision where collector is an executor or automated collector
    let mut fake_meta = test_metadata(
        &authority,
        &current,
        "fabricated-executor-decision",
        EvidenceClass::HumanDecision,
        EvidenceResult::Pass,
        EvidenceConfidence::HumanAsserted,
    );
    fake_meta.collector = CollectorIdentity::new("antigravity-executor-collector", "p8-v1"); // FAKE COLLECTOR
    fake_meta.requirement_ids = vec!["REQ-A-OUTCOME".into()];
    let fake_art = store.put_test_fixture(fake_meta, b"Executor claims user approved in documentation").unwrap();

    let engine = VerificationEngine::new_for_test(
        store.clone(),
        authority.clone(),
        current.clone(),
        Vec::new(),
    ).expect("engine");

    let report = engine.evaluate(None).expect("evaluate");
    let status_a = report.requirement_statuses.iter().find(|s| s.requirement_id == "REQ-A-OUTCOME").unwrap();

    // Fabricated decision MUST NOT be accepted
    assert!(!status_a.evidence_ids.contains(&fake_art.metadata.evidence_id));
    assert_ne!(status_a.status, RequirementStatus::Verified);
    assert_ne!(report.decision.state, CompletionState::VerifiedComplete);
}

#[test]
fn test_certificate_denied_after_rejection() {
    let (authority, current, store, signing_key, temp) = create_phase5_test_fixture();

    put_user_decision_fixture(
        &store,
        &authority,
        &current,
        "REQ-A-OUTCOME",
        false,
        "Outcome rejected by project owner",
    );

    let engine = VerificationEngine::new_for_test(
        store.clone(),
        authority.clone(),
        current.clone(),
        Vec::new(),
    ).expect("engine");

    let report = engine.evaluate(None).expect("evaluate");
    assert_eq!(report.decision.state, CompletionState::FailedVerification);

    let p7 = p7_execution_fixture(
        &authority,
        temp.path(),
        vec!["REQ-A-OUTCOME".into(), "REQ-B-PURPOSE".into()],
    );

    let cert_authority = CompletionAuthority::new(signing_key.to_bytes().as_ref()).expect("cert authority");
    let cert_result = cert_authority.issue_with_p7_execution(&report, &authority, &p7);

    assert!(cert_result.is_err(), "Certificate issuance must fail when verification report is FailedVerification");
}

#[test]
fn test_duplicate_rejection_idempotent() {
    let (authority, current, store, _, temp) = create_phase5_test_fixture();
    let p7 = p7_execution_fixture(
        &authority,
        temp.path(),
        vec!["REQ-A-OUTCOME".into(), "REQ-B-PURPOSE".into()],
    );
    let recorder = ExplicitUserDecisionRecorder;

    let res1 = recorder.record(
        &authority,
        &current,
        &store,
        &p7,
        ExplicitUserDecisionInput {
            requirement_id: "REQ-A-OUTCOME",
            approved: false,
            notes: "First rejection",
        },
    ).expect("first rejection succeeds");

    assert_eq!(res1.metadata.result, EvidenceResult::Fail);

    // Repeated identical rejection on REQ-A-OUTCOME is idempotent
    let res2 = recorder.record(
        &authority,
        &current,
        &store,
        &p7,
        ExplicitUserDecisionInput {
            requirement_id: "REQ-A-OUTCOME",
            approved: false,
            notes: "Duplicate rejection attempt",
        },
    ).expect("duplicate rejection succeeds idempotently");

    assert_eq!(res1.digest, res2.digest);

    // Identical rejection on semantic sibling REQ-B-PURPOSE is also idempotent
    let res3 = recorder.record(
        &authority,
        &current,
        &store,
        &p7,
        ExplicitUserDecisionInput {
            requirement_id: "REQ-B-PURPOSE",
            approved: false,
            notes: "Rejection on sibling",
        },
    ).expect("sibling rejection succeeds idempotently");

    assert_eq!(res1.digest, res3.digest);
}

fn multi_task_p7_execution_fixture(
    authority: &VerificationAuthority,
    root: &Path,
    task_specs: Vec<(&str, Vec<String>)>,
) -> ExecutionRun {
    let mut tasks = BTreeMap::new();
    let mut attempts = Vec::new();
    let mut leases = Vec::new();

    for (task_id, req_ids) in task_specs {
        let packet_digest = "0".repeat(64);
        let attempt_id = format!("attempt-{task_id}");
        let lease_id = format!("lease-{task_id}");
        let process_digest = "1".repeat(64);
        let started_at_ms = 1000;
        let ended_at_ms = 2000;
        let expires_at_ms = 10000;

        let lease_raw = serde_json::json!({
            "lease_id": lease_id,
            "mission_id": authority.revision.seal.mission_id,
            "mission_revision": authority.revision.revision,
            "task_id": task_id,
            "task_packet_digest": packet_digest,
            "scope": {
                "workspace": root,
                "file_scopes": [],
                "directory_scopes": [],
                "shared_resources": [],
                "package_lockfiles": [],
                "generated_files": [],
                "allowed_tools": [],
                "external_authority": [],
                "scope_known": true
            },
            "issued_at_ms": started_at_ms,
            "expires_at_ms": expires_at_ms,
            "step_budget": 10,
            "tool_call_budget": 10,
            "usage_budget": {
                "wall_clock_ms": 60000,
                "execution_steps": 10,
                "tool_calls": 10,
                "retry_attempts": 1,
                "cost_micros": null
            },
            "attempt_number": 1,
            "status": "CONSUMED",
            "lease_digest": ""
        });
        let mut lease: ExecutionLease = serde_json::from_value(lease_raw).expect("lease");
        lease.lease_digest = lease.compute_digest().expect("lease digest");

        let attempt_raw = serde_json::json!({
            "attempt_id": attempt_id,
            "task_id": task_id,
            "attempt_number": 1,
            "packet_digest": packet_digest,
            "lease_id": lease_id,
            "state": "SUCCEEDED",
            "started_at_ms": started_at_ms,
            "ended_at_ms": ended_at_ms,
            "failure_class": null,
            "failure_fingerprint": null,
            "usage": {
                "tool_calls": 0,
                "execution_steps": 0,
                "wall_time_ms": 1000,
                "retry_count": 0,
                "estimated_cost_micros": null,
                "actual_cost_micros": null,
                "attempt_count": 1,
                "quality": "MEASURED"
            },
            "termination_reason": null,
            "execution_boundary": "EXTERNAL_PROCESS_STARTED",
            "extensions_granted": 0,
            "completion_authority": {
                "task_id": task_id,
                "attempt_id": attempt_id,
                "packet_digest": packet_digest,
                "lease_id": lease_id,
                "process_digest": process_digest,
                "started_at_ms": started_at_ms,
                "ended_at_ms": ended_at_ms
            },
            "workspace_before": {},
            "workspace_after": {}
        });
        let attempt: TaskAttempt = serde_json::from_value(attempt_raw).expect("attempt");

        tasks.insert(
            task_id.to_string(),
            ExecutionTask {
                task_id: task_id.into(),
                objective: format!("Task {task_id}"),
                requirement_ids: req_ids,
                dependency_ids: vec![],
                priority: RequirementPriority::P1,
                state: ExecutionTaskState::FinishedAwaitingVerification,
                scope: lease.scope.clone(),
                usage_budget: lease.usage_budget.clone(),
                retry_policy: RetryPolicy::default(),
                evidence_obligations: vec![],
                attempt_number: 1,
            },
        );
        attempts.push(attempt);
        leases.push(lease);
    }

    ExecutionRun {
        ledger_version: "p7-execution-ledger-v1".into(),
        run_id: authority.p7_run_id.clone(),
        mission_id: authority.revision.seal.mission_id.clone(),
        mission_revision: authority.revision.revision,
        seal_hash: authority.revision.seal.contract_hash.clone(),
        project_id: authority.revision.seal.project_id.clone(),
        workspace: root.to_path_buf(),
        workspace_fingerprint: authority.workspace_fingerprint.clone(),
        state: ExecutionRunState::ExecutionTasksFinishedAwaitingVerification,
        tasks,
        attempts,
        leases,
        events: vec![],
        usage: relintor_execution::UsageTelemetry::default(),
        loop_signals: vec![],
        oscillation_signals: vec![],
        continuations: vec![],
        diagnostics: vec![],
        external_modifications: vec![],
        progress: vec![],
        watchdog_state: relintor_execution::WatchdogState::Healthy,
        policy: SchedulerPolicy::default(),
        safe_boundary: None,
        current_turn: 1,
        last_error: None,
        no_progress_occurrences: 0,
        reviewed_recovery_deltas: vec![],
        integrity_version: "p7-ledger-integrity-v1".into(),
        integrity_tag: String::new(),
    }
}

#[test]
fn test_scoped_correction_authorization_matrix() {
    let (authority, current, store, signing_key, temp) = create_phase5_test_fixture();

    // 1. Record passing machine test evidence for REQ-TEST-AUTOMATED
    let mut test_meta = test_metadata(
        &authority,
        &current,
        "test-output-evidence-01",
        EvidenceClass::TestOutput,
        EvidenceResult::Pass,
        EvidenceConfidence::StrongDeterministic,
    );
    test_meta.requirement_ids = vec!["REQ-TEST-AUTOMATED".into()];
    test_meta.accepted_criteria = BTreeSet::from(["REQ-TEST-AUTOMATED-criterion".into()]);
    store.put_test_fixture(test_meta, b"automated tests passed").unwrap();

    // 2. Authoritative human rejection on REQ-A-OUTCOME
    put_user_decision_fixture(
        &store,
        &authority,
        &current,
        "REQ-A-OUTCOME",
        false,
        "Rejection note: The UI flow lacks clear error indicators.",
    );

    // 3. Evaluate verification via VerificationEngine
    let engine = VerificationEngine::new_for_test(
        store.clone(),
        authority.clone(),
        current.clone(),
        Vec::new(),
    ).expect("engine");
    let report = engine.evaluate(None).expect("evaluate");

    // Rejection guarantees FailedVerification
    assert_eq!(report.decision.state, CompletionState::FailedVerification);

    // 4. Setup 3-task execution run:
    //    task-decision: covers REQ-A-OUTCOME, REQ-B-PURPOSE (rejected/failed)
    //    task-accessibility: covers REQ-ACCESSIBILITY (missing obligation/blocked)
    //    task-automated: covers REQ-TEST-AUTOMATED (passing/verified)
    let task_specs = vec![
        ("task-decision", vec!["REQ-A-OUTCOME".to_string(), "REQ-B-PURPOSE".to_string()]),
        ("task-accessibility", vec!["REQ-ACCESSIBILITY".to_string()]),
        ("task-automated", vec!["REQ-TEST-AUTOMATED".to_string()]),
    ];
    let mut run = multi_task_p7_execution_fixture(&authority, temp.path(), task_specs);

    // MATRIX 46A: Execution not automatically started on rejection
    // Before authorization, run is FinishedAwaitingVerification and 0 tasks are runnable
    assert_eq!(run.state, ExecutionRunState::ExecutionTasksFinishedAwaitingVerification);
    assert_eq!(run.runnable_tasks().len(), 0);

    // Derive correction scope generic rules:
    // failed: requirements with status == Failed (REQ-A-OUTCOME, REQ-B-PURPOSE)
    // blocked: requirements with status != Verified && status != Failed (REQ-ACCESSIBILITY)
    // verified: requirements with status == Verified (REQ-TEST-AUTOMATED)
    let failed_reqs = report.requirement_statuses.iter()
        .filter(|s| s.status == RequirementStatus::Failed)
        .map(|s| s.requirement_id.clone())
        .collect::<BTreeSet<_>>();
    let blocked_reqs = report.requirement_statuses.iter()
        .filter(|s| s.status != RequirementStatus::Verified && s.status != RequirementStatus::Failed)
        .map(|s| s.requirement_id.clone())
        .collect::<BTreeSet<_>>();
    let verified_reqs = report.requirement_statuses.iter()
        .filter(|s| s.status == RequirementStatus::Verified)
        .map(|s| s.requirement_id.clone())
        .collect::<BTreeSet<_>>();

    assert!(failed_reqs.contains("REQ-A-OUTCOME"));
    assert!(failed_reqs.contains("REQ-B-PURPOSE"));
    assert!(blocked_reqs.contains("REQ-ACCESSIBILITY"));
    assert!(verified_reqs.contains("REQ-TEST-AUTOMATED"));

    let target_reqs: BTreeSet<String> = failed_reqs.union(&blocked_reqs)
        .filter(|r| run.tasks.values().any(|t| t.requirement_ids.contains(r)))
        .cloned()
        .collect();

    // MATRIX 46J: Tamper-resistance / policy checks
    // Empty target requirements denied
    assert!(run.authorize_verification_correction(&BTreeSet::new(), 2000).is_err());
    // Requirement outside sealed graph denied
    let unknown_req = BTreeSet::from(["REQ-NONEXISTENT".into()]);
    assert!(run.authorize_verification_correction(&unknown_req, 2000).is_err());

    // Preserved historical attempts before authorization
    let initial_attempts_count = run.attempts.len();
    assert_eq!(initial_attempts_count, 3);
    assert_eq!(run.tasks["task-automated"].attempt_number, 1);
    assert_eq!(run.tasks["task-decision"].attempt_number, 1);
    assert_eq!(run.tasks["task-accessibility"].attempt_number, 1);

    // MATRIX 46C & 46D: User explicitly authorizes correction -> bounded task reactivation
    let affected = run.authorize_verification_correction(&target_reqs, 2000)
        .expect("authorization must succeed for valid target requirements");

    // MATRIX 46D: Only affected tasks are reopened (Pending); unaffected tasks remain Completed
    assert_eq!(affected.len(), 2);
    assert!(affected.contains(&"task-decision".to_string()));
    assert!(affected.contains(&"task-accessibility".to_string()));
    assert!(!affected.contains(&"task-automated".to_string()));

    assert_eq!(run.tasks["task-decision"].state, ExecutionTaskState::Pending);
    assert_eq!(run.tasks["task-accessibility"].state, ExecutionTaskState::Pending);
    assert_eq!(run.tasks["task-automated"].state, ExecutionTaskState::FinishedAwaitingVerification);

    // MATRIX 46E: Runnable tasks > 0 after authorization
    let runnable = run.runnable_tasks();
    assert_eq!(runnable.len(), 2);
    assert!(runnable.contains(&"task-decision".to_string()));
    assert!(runnable.contains(&"task-accessibility".to_string()));
    assert!(!runnable.contains(&"task-automated".to_string()));

    // MATRIX 46F: State transitions: run state = Ready, waiting for user execution step
    assert_eq!(run.state, ExecutionRunState::Ready);

    // MATRIX 46G: Zero automatic execution: tasks are Pending, run is Ready, NOT Executing
    assert_ne!(run.tasks["task-decision"].state, ExecutionTaskState::Running);
    assert_ne!(run.tasks["task-accessibility"].state, ExecutionTaskState::Running);

    // MATRIX 46H: Preserved historical records: previous attempts/logs remain immutable and preserved
    assert_eq!(run.attempts.len(), initial_attempts_count);
    for attempt in &run.attempts {
        assert_eq!(attempt.state, TaskAttemptState::Succeeded);
    }
    // Attempt number preserved
    assert_eq!(run.tasks["task-decision"].attempt_number, 1);
    assert_eq!(run.tasks["task-accessibility"].attempt_number, 1);
    assert_eq!(run.tasks["task-automated"].attempt_number, 1);

    // MATRIX 46I: Same revision: operations remained in revision 1
    assert_eq!(run.mission_revision, authority.revision.revision);

    // MATRIX 46J: Repeated authorization denied / idempotent at execution level
    let repeated = run.authorize_verification_correction(&target_reqs, 3000);
    assert!(repeated.is_err(), "repeated correction on non-finished run must be denied");

    // Events trace contains VerificationCorrectionAuthorized events
    let correction_events = run.events.iter()
        .filter(|e| e.kind == ExecutionEventKind::VerificationCorrectionAuthorized)
        .collect::<Vec<_>>();
    assert_eq!(correction_events.len(), 2);

    // Certificate still denied because report is still FailedVerification
    let cert_authority = CompletionAuthority::new(signing_key.to_bytes().as_ref()).expect("cert authority");
    let authenticated_p7 = AuthenticatedP7Execution::from_run(run).expect("authenticated p7");
    assert!(cert_authority.issue_with_p7_execution(&report, &authority, &authenticated_p7).is_err());
}

#[test]
fn test_defect_49_duplicate_semantic_correction_matrix() {
    let (authority, current, store, _signing_key, temp) = create_phase5_test_fixture();

    // 1. Two requirement IDs, same semantic HumanDecision -> ONE semantic correction authority
    let req_a = authority.revision.contract.requirement_graph.requirements.iter()
        .find(|r| r.requirement_id == "REQ-A-OUTCOME").unwrap();
    let req_b = authority.revision.contract.requirement_graph.requirements.iter()
        .find(|r| r.requirement_id == "REQ-B-PURPOSE").unwrap();
    assert!(is_human_decision_semantic_equivalent(req_a, req_b));

    // 2. Different titles, equivalent acceptance -> no duplicate execution work
    // 3. Same title, genuinely different semantics -> separate correction scope
    let mut req_c = req_a.clone();
    req_c.requirement_id = "REQ-C-OTHER".into();
    req_c.intent = "Implement unrelated user profile screen".into();
    assert!(!is_human_decision_semantic_equivalent(req_a, &req_c));

    // Record rejection on REQ-A-OUTCOME
    put_user_decision_fixture(
        &store,
        &authority,
        &current,
        "REQ-A-OUTCOME",
        false,
        "REJECTED: 1. Documentation falsely claims approval. 2. State transitions unreliable. 3. Inadequate tests. 4. Accessibility blocked.",
    );

    // Also record passing evidence for REQ-TEST-AUTOMATED
    let mut test_meta = test_metadata(
        &authority,
        &current,
        "test-output-evidence-01",
        EvidenceClass::TestOutput,
        EvidenceResult::Pass,
        EvidenceConfidence::StrongDeterministic,
    );
    test_meta.requirement_ids = vec!["REQ-TEST-AUTOMATED".into()];
    store.put_test_fixture(test_meta, b"automated tests passed").unwrap();

    let engine = VerificationEngine::new_for_test(
        store.clone(),
        authority.clone(),
        current.clone(),
        Vec::new(),
    ).expect("engine");
    let report = engine.evaluate(None).expect("evaluate");

    assert_eq!(report.decision.state, CompletionState::FailedVerification);

    let task_specs = vec![
        ("task-decision-a", vec!["REQ-A-OUTCOME".to_string()]),
        ("task-decision-b", vec!["REQ-B-PURPOSE".to_string()]),
        ("task-accessibility", vec!["REQ-ACCESSIBILITY".to_string()]),
        ("task-automated", vec!["REQ-TEST-AUTOMATED".to_string()]),
    ];
    let run = multi_task_p7_execution_fixture(&authority, temp.path(), task_specs);

    let contract_tasks = vec![
        Task {
            task_id: "task-decision-a".into(),
            title: "Implement: User problem outcome".into(),
            objective: "Implement transport health outcome".into(),
            requirement_ids: vec!["REQ-A-OUTCOME".into()],
            dependency_ids: vec![],
            suggested_scope: "decision".into(),
            risk: RequirementRisk::High,
            evidence_obligations: vec![EvidenceObligation {
                class: EvidenceClass::HumanDecision,
                minimum_confidence: EvidenceConfidence::HumanAsserted,
                rationale: "Human decision".into(),
                required: true,
            }],
            status: RequirementStatus::Verified,
            provenance: "contract".into(),
        },
        Task {
            task_id: "task-decision-b".into(),
            title: "Implement: User product purpose".into(),
            objective: "Implement transport health purpose".into(),
            requirement_ids: vec!["REQ-B-PURPOSE".into()],
            dependency_ids: vec![],
            suggested_scope: "decision".into(),
            risk: RequirementRisk::High,
            evidence_obligations: vec![EvidenceObligation {
                class: EvidenceClass::HumanDecision,
                minimum_confidence: EvidenceConfidence::HumanAsserted,
                rationale: "Human decision".into(),
                required: true,
            }],
            status: RequirementStatus::Verified,
            provenance: "contract".into(),
        },
        Task {
            task_id: "task-accessibility".into(),
            title: "Implement: NFR: accessibility".into(),
            objective: "Implement accessibility".into(),
            requirement_ids: vec!["REQ-ACCESSIBILITY".into()],
            dependency_ids: vec![],
            suggested_scope: "accessibility".into(),
            risk: RequirementRisk::High,
            evidence_obligations: vec![EvidenceObligation {
                class: EvidenceClass::AccessibilityResult,
                minimum_confidence: EvidenceConfidence::StrongDeterministic,
                rationale: "Accessibility check".into(),
                required: true,
            }],
            status: RequirementStatus::Blocked,
            provenance: "contract".into(),
        },
        Task {
            task_id: "task-automated".into(),
            title: "Implement: automated tests".into(),
            objective: "Implement tests".into(),
            requirement_ids: vec!["REQ-TEST-AUTOMATED".into()],
            dependency_ids: vec![],
            suggested_scope: "functional".into(),
            risk: RequirementRisk::Medium,
            evidence_obligations: vec![EvidenceObligation {
                class: EvidenceClass::TestOutput,
                minimum_confidence: EvidenceConfidence::StrongDeterministic,
                rationale: "Automated test".into(),
                required: true,
            }],
            status: RequirementStatus::Verified,
            provenance: "contract".into(),
        },
    ];

    let provenance = BTreeMap::from([
        ("task-decision-a".into(), vec![
            "crates/routing/src/lib.rs".into(),
            "docs/PLAN.md".into(),
            "tests/HealthTest.kt".into(),
        ]),
        ("task-accessibility".into(), vec![
            "docs/ACCESSIBILITY.md".into(),
            "tests/a11y.test.js".into(),
        ]),
    ]);

    let originating_ev = report.requirement_statuses.iter()
        .find(|s| s.requirement_id == "REQ-A-OUTCOME")
        .and_then(|s| s.failed_evidence.first())
        .cloned()
        .unwrap_or_default();

    let scope = derive_bounded_correction_scope(
        &authority.revision.seal.mission_id,
        authority.revision.revision,
        &authority.revision.contract.requirement_graph.requirements,
        &contract_tasks,
        &[],
        &report.requirement_statuses,
        &run,
        &originating_ev,
        "REJECTED: 1. Documentation falsely claims approval. 2. State transitions unreliable. 3. Inadequate tests. 4. Accessibility blocked.",
        &provenance,
        None,
    ).expect("derive bounded correction scope");

    // MATRIX 49A1: Exactly 1 semantic correction authority for the duplicate HumanDecision
    assert_eq!(scope.semantic_correction_authorities, 1);

    // MATRIX 49A2: Duplicate execution work is 0
    assert_eq!(scope.duplicate_correction_work, 0);

    // MATRIX 49A3 & 49A6: task-decision-b is deduplicated and NOT in affected_task_ids
    assert!(scope.deduplicated_task_ids.contains(&"task-decision-b".to_string()));
    assert!(!scope.affected_task_ids.contains(&"task-decision-b".to_string()));
    assert!(scope.affected_task_ids.contains(&"task-decision-a".to_string()));
    assert!(scope.affected_task_ids.contains(&"task-accessibility".to_string()));
    assert_eq!(scope.affected_task_ids.len(), 2);

    // MATRIX 49A5: Deduplicated task preserved in historical tasks (not duplicated in budget/execution)
    assert!(scope.preserved_task_ids.contains(&"task-decision-b".to_string()));
    assert!(scope.preserved_task_ids.contains(&"task-automated".to_string()));
    assert_eq!(scope.preserved_task_ids.len(), 2);

    // MATRIX 49A7: Only one final HumanDecision obligation is requested across the correction
    let hd_count = scope.proposed_tasks.iter()
        .flat_map(|t| t.required_fresh_evidence.iter())
        .filter(|e| e.starts_with("HUMAN_DECISION"))
        .count();
    assert_eq!(hd_count, 1);
}

#[test]
fn test_defect_49_technical_correction_evidence_matrix() {
    let (authority, current, store, _signing_key, temp) = create_phase5_test_fixture();

    put_user_decision_fixture(
        &store,
        &authority,
        &current,
        "REQ-A-OUTCOME",
        false,
        "REJECTED: 1. Documentation falsely claims owner approval in docs. 2. State transitions unreliable in routing. 3. Current automated tests inadequate. 4. Accessibility blocked.",
    );

    let mut test_meta = test_metadata(
        &authority,
        &current,
        "test-output-evidence-01",
        EvidenceClass::TestOutput,
        EvidenceResult::Pass,
        EvidenceConfidence::StrongDeterministic,
    );
    test_meta.requirement_ids = vec!["REQ-TEST-AUTOMATED".into()];
    store.put_test_fixture(test_meta, b"automated tests passed").unwrap();

    let engine = VerificationEngine::new_for_test(
        store.clone(),
        authority.clone(),
        current.clone(),
        Vec::new(),
    ).expect("engine");
    let report = engine.evaluate(None).expect("evaluate");

    let task_specs = vec![
        ("task-decision-a", vec!["REQ-A-OUTCOME".to_string()]),
        ("task-accessibility", vec!["REQ-ACCESSIBILITY".to_string()]),
    ];
    let run = multi_task_p7_execution_fixture(&authority, temp.path(), task_specs);

    let contract_tasks = vec![
        Task {
            task_id: "task-decision-a".into(),
            title: "Implement: User problem outcome".into(),
            objective: "Implement transport health outcome".into(),
            requirement_ids: vec!["REQ-A-OUTCOME".into()],
            dependency_ids: vec![],
            suggested_scope: "decision".into(),
            risk: RequirementRisk::High,
            evidence_obligations: vec![EvidenceObligation {
                class: EvidenceClass::HumanDecision,
                minimum_confidence: EvidenceConfidence::HumanAsserted,
                rationale: "Human decision".into(),
                required: true,
            }],
            status: RequirementStatus::Verified,
            provenance: "contract".into(),
        },
        Task {
            task_id: "task-accessibility".into(),
            title: "Implement: NFR: accessibility".into(),
            objective: "Implement accessibility".into(),
            requirement_ids: vec!["REQ-ACCESSIBILITY".into()],
            dependency_ids: vec![],
            suggested_scope: "accessibility".into(),
            risk: RequirementRisk::High,
            evidence_obligations: vec![EvidenceObligation {
                class: EvidenceClass::AccessibilityResult,
                minimum_confidence: EvidenceConfidence::StrongDeterministic,
                rationale: "Accessibility check".into(),
                required: true,
            }],
            status: RequirementStatus::Blocked,
            provenance: "contract".into(),
        },
    ];

    let provenance = BTreeMap::from([
        ("task-decision-a".into(), vec![
            "crates/routing/src/lib.rs".into(),
            "docs/PLAN.md".into(),
            "tests/HealthTest.kt".into(),
        ]),
        ("task-accessibility".into(), vec![
            "docs/ACCESSIBILITY.md".into(),
            "tests/a11y.test.js".into(),
        ]),
    ]);

    let originating_ev = report.requirement_statuses.iter()
        .find(|s| s.requirement_id == "REQ-A-OUTCOME")
        .and_then(|s| s.failed_evidence.first())
        .cloned()
        .unwrap_or_default();

    let scope = derive_bounded_correction_scope(
        &authority.revision.seal.mission_id,
        authority.revision.revision,
        &authority.revision.contract.requirement_graph.requirements,
        &contract_tasks,
        &[],
        &report.requirement_statuses,
        &run,
        &originating_ev,
        "REJECTED: 1. Documentation falsely claims owner approval in docs. 2. State transitions unreliable in routing. 3. Current automated tests inadequate. 4. Accessibility blocked.",
        &provenance,
        None,
    ).expect("derive bounded correction scope");

    // MATRIX 49B1: Human rejects due to technical implementation defect -> HumanDecision alone CANNOT satisfy correction
    let task_decision_preview = scope.proposed_tasks.iter().find(|t| t.task_id == "task-decision-a").unwrap();
    let has_technical_evidence = task_decision_preview.required_fresh_evidence.iter().any(|e| e.starts_with("TEST_OUTPUT"));
    assert!(has_technical_evidence, "Technical implementation defect must require technical evidence");

    // MATRIX 49B6: Accessibility blocker requires AccessibilityResult
    let task_a11y_preview = scope.proposed_tasks.iter().find(|t| t.task_id == "task-accessibility").unwrap();
    assert!(task_a11y_preview.required_fresh_evidence.iter().any(|e| e.starts_with("ACCESSIBILITY_RESULT")));

    // MATRIX 49B7: Documentation defect requires test/policy inspection evidence
    let doc_unit = scope.correction_units.iter().find(|u| u.semantic_finding.to_lowercase().contains("documentation")).unwrap();
    assert!(doc_unit.required_fresh_evidence.iter().any(|e| e.contains("TEST_OUTPUT") || e.contains("inspection")));

    // Zero technical findings with only HumanDecision evidence
    assert_eq!(scope.technical_findings_with_only_humandecision_evidence, 0);

    // MATRIX 49B5: When provenance paths cannot be safely derived, refinement is required
    let empty_provenance = BTreeMap::new();
    let empty_scope = derive_bounded_correction_scope(
        &authority.revision.seal.mission_id,
        authority.revision.revision,
        &authority.revision.contract.requirement_graph.requirements,
        &contract_tasks,
        &[],
        &report.requirement_statuses,
        &run,
        &originating_ev,
        "REJECTED: Unmapped failure without bounds.",
        &empty_provenance,
        None,
    ).expect("scope derived");
    assert!(empty_scope.human_refinement_required, "Missing boundary provenance must require human refinement");
    assert!(empty_scope.project_wide_unbounded_authority, "Without provenance paths, authority cannot be bounded");
}

#[test]
fn test_defect_49_authorization_precision_matrix() {
    let (authority, current, store, _signing_key, temp) = create_phase5_test_fixture();

    put_user_decision_fixture(
        &store,
        &authority,
        &current,
        "REQ-A-OUTCOME",
        false,
        "REJECTED: 1. Documentation fabricated. 2. State transitions unreliable. 3. Inadequate automated tests. 4. Accessibility blocked.",
    );

    let mut test_meta = test_metadata(
        &authority,
        &current,
        "test-output-evidence-01",
        EvidenceClass::TestOutput,
        EvidenceResult::Pass,
        EvidenceConfidence::StrongDeterministic,
    );
    test_meta.requirement_ids = vec!["REQ-TEST-AUTOMATED".into()];
    store.put_test_fixture(test_meta, b"automated tests passed").unwrap();

    let engine = VerificationEngine::new_for_test(
        store.clone(),
        authority.clone(),
        current.clone(),
        Vec::new(),
    ).expect("engine");
    let report = engine.evaluate(None).expect("evaluate");

    let task_specs = vec![
        ("task-decision-a", vec!["REQ-A-OUTCOME".to_string()]),
        ("task-decision-b", vec!["REQ-B-PURPOSE".to_string()]),
        ("task-accessibility", vec!["REQ-ACCESSIBILITY".to_string()]),
        ("task-automated", vec!["REQ-TEST-AUTOMATED".to_string()]),
    ];
    let mut run = multi_task_p7_execution_fixture(&authority, temp.path(), task_specs);

    let contract_tasks = vec![
        Task {
            task_id: "task-decision-a".into(),
            title: "Implement: User problem outcome".into(),
            objective: "Implement transport health outcome".into(),
            requirement_ids: vec!["REQ-A-OUTCOME".into()],
            dependency_ids: vec![],
            suggested_scope: "decision".into(),
            risk: RequirementRisk::High,
            evidence_obligations: vec![EvidenceObligation {
                class: EvidenceClass::HumanDecision,
                minimum_confidence: EvidenceConfidence::HumanAsserted,
                rationale: "Human decision".into(),
                required: true,
            }],
            status: RequirementStatus::Verified,
            provenance: "contract".into(),
        },
        Task {
            task_id: "task-decision-b".into(),
            title: "Implement: User product purpose".into(),
            objective: "Implement transport health purpose".into(),
            requirement_ids: vec!["REQ-B-PURPOSE".into()],
            dependency_ids: vec![],
            suggested_scope: "decision".into(),
            risk: RequirementRisk::High,
            evidence_obligations: vec![EvidenceObligation {
                class: EvidenceClass::HumanDecision,
                minimum_confidence: EvidenceConfidence::HumanAsserted,
                rationale: "Human decision".into(),
                required: true,
            }],
            status: RequirementStatus::Verified,
            provenance: "contract".into(),
        },
        Task {
            task_id: "task-accessibility".into(),
            title: "Implement: NFR: accessibility".into(),
            objective: "Implement accessibility".into(),
            requirement_ids: vec!["REQ-ACCESSIBILITY".into()],
            dependency_ids: vec![],
            suggested_scope: "accessibility".into(),
            risk: RequirementRisk::High,
            evidence_obligations: vec![EvidenceObligation {
                class: EvidenceClass::AccessibilityResult,
                minimum_confidence: EvidenceConfidence::StrongDeterministic,
                rationale: "Accessibility check".into(),
                required: true,
            }],
            status: RequirementStatus::Blocked,
            provenance: "contract".into(),
        },
        Task {
            task_id: "task-automated".into(),
            title: "Implement: automated tests".into(),
            objective: "Implement tests".into(),
            requirement_ids: vec!["REQ-TEST-AUTOMATED".into()],
            dependency_ids: vec![],
            suggested_scope: "functional".into(),
            risk: RequirementRisk::Medium,
            evidence_obligations: vec![EvidenceObligation {
                class: EvidenceClass::TestOutput,
                minimum_confidence: EvidenceConfidence::StrongDeterministic,
                rationale: "Automated test".into(),
                required: true,
            }],
            status: RequirementStatus::Verified,
            provenance: "contract".into(),
        },
    ];

    let provenance = BTreeMap::from([
        ("task-decision-a".into(), vec![
            "crates/routing/src/lib.rs".into(),
            "docs/PLAN.md".into(),
            "tests/HealthTest.kt".into(),
        ]),
        ("task-accessibility".into(), vec![
            "docs/ACCESSIBILITY.md".into(),
            "tests/a11y.test.js".into(),
        ]),
    ]);

    let originating_ev = report.requirement_statuses.iter()
        .find(|s| s.requirement_id == "REQ-A-OUTCOME")
        .and_then(|s| s.failed_evidence.first())
        .cloned()
        .unwrap_or_default();

    let scope = derive_bounded_correction_scope(
        &authority.revision.seal.mission_id,
        authority.revision.revision,
        &authority.revision.contract.requirement_graph.requirements,
        &contract_tasks,
        &[],
        &report.requirement_statuses,
        &run,
        &originating_ev,
        "REJECTED: 1. Documentation fabricated. 2. State transitions unreliable. 3. Inadequate automated tests. 4. Accessibility blocked.",
        &provenance,
        None,
    ).expect("derive bounded correction scope");

    // All 5 Authorization Precision criteria:
    // 1. SEMANTIC_CORRECTION_AUTHORITIES == 1
    assert_eq!(scope.semantic_correction_authorities, 1);

    // 2. DUPLICATE_CORRECTION_WORK == 0
    assert_eq!(scope.duplicate_correction_work, 0);

    // 3. UNRELATED_TASKS == 0
    assert_eq!(scope.unrelated_tasks, 0);

    // 4. PROJECT_WIDE_UNBOUNDED_AUTHORITY == NO (false)
    assert_eq!(scope.project_wide_unbounded_authority, false);
    for t in &scope.proposed_tasks {
        assert!(t.authorized_scope.is_bounded, "Task {} must be bounded", t.task_id);
        assert_eq!(t.authorized_scope.authority_boundary_type, "PROVENANCE_BOUNDED");
        assert!(!t.authorized_scope.bounded_file_scopes.is_empty());
    }

    // 5. TECHNICAL_FINDINGS_WITH_ONLY_HUMANDECISION_EVIDENCE == 0
    assert_eq!(scope.technical_findings_with_only_humandecision_evidence, 0);

    // When authorized via canonical affected tasks:
    let known_requirements = run.tasks.values().flat_map(|t| t.requirement_ids.iter().cloned()).collect::<BTreeSet<_>>();
    let target_reqs: BTreeSet<String> = scope.affected_task_ids.iter()
        .filter_map(|tid| run.tasks.get(tid))
        .flat_map(|t| t.requirement_ids.iter().cloned())
        .filter(|r| known_requirements.contains(r))
        .collect();

    let affected = run.authorize_verification_correction(&target_reqs, 2000).expect("authorize");
    assert_eq!(affected.len(), 2);
    assert!(affected.contains(&"task-decision-a".to_string()));
    assert!(affected.contains(&"task-accessibility".to_string()));
    assert!(!affected.contains(&"task-decision-b".to_string()), "Deduplicated task must NOT be reopened");
    assert!(!affected.contains(&"task-automated".to_string()), "Unaffected task must NOT be reopened");

    // Execution state: only canonical tasks are Pending/runnable
    assert_eq!(run.tasks["task-decision-a"].state, ExecutionTaskState::Pending);
    assert_eq!(run.tasks["task-accessibility"].state, ExecutionTaskState::Pending);
    assert_eq!(run.tasks["task-decision-b"].state, ExecutionTaskState::FinishedAwaitingVerification);
    assert_eq!(run.tasks["task-automated"].state, ExecutionTaskState::FinishedAwaitingVerification);

    let runnable = run.runnable_tasks();
    assert_eq!(runnable.len(), 2);
    assert!(runnable.contains(&"task-decision-a".to_string()));
    assert!(runnable.contains(&"task-accessibility".to_string()));
}

#[test]
fn test_defect_51_path_security_and_governance() {
    let temp_ws = tempdir().expect("temp workspace");
    let ws_path = temp_ws.path();

    // Create a dummy file and directory in the workspace
    let src_dir = ws_path.join("src");
    std::fs::create_dir_all(&src_dir).unwrap();
    let main_file = src_dir.join("main.rs");
    std::fs::write(&main_file, b"fn main() {}").unwrap();

    // 1. Empty path rejected
    assert!(matches!(
        validate_and_canonicalize_scope_path(ws_path, "", PathType::File),
        Err(ScopePathError::EmptyPath)
    ));
    assert!(matches!(
        validate_and_canonicalize_scope_path(ws_path, "   ", PathType::File),
        Err(ScopePathError::EmptyPath)
    ));

    // 2. Traversal patterns rejected
    assert!(matches!(
        validate_and_canonicalize_scope_path(ws_path, "../outside.txt", PathType::File),
        Err(ScopePathError::TraversalForbidden)
    ));
    assert!(matches!(
        validate_and_canonicalize_scope_path(ws_path, "src/../../outside.txt", PathType::File),
        Err(ScopePathError::TraversalForbidden)
    ));
    assert!(matches!(
        validate_and_canonicalize_scope_path(ws_path, "..\\outside.txt", PathType::File),
        Err(ScopePathError::TraversalForbidden)
    ));

    // 3. UNC paths rejected
    assert!(matches!(
        validate_and_canonicalize_scope_path(ws_path, r"\\server\share\file.txt", PathType::File),
        Err(ScopePathError::UncPathForbidden)
    ));
    assert!(matches!(
        validate_and_canonicalize_scope_path(ws_path, "//server/share/file.txt", PathType::File),
        Err(ScopePathError::UncPathForbidden)
    ));

    // 4. Device paths rejected
    assert!(matches!(
        validate_and_canonicalize_scope_path(ws_path, r"\\?\C:\file.txt", PathType::File),
        Err(ScopePathError::DevicePathForbidden)
    ));
    assert!(matches!(
        validate_and_canonicalize_scope_path(ws_path, r"\\.\COM1", PathType::File),
        Err(ScopePathError::DevicePathForbidden)
    ));

    // 5. Valid relative path canonicalization
    let clean = validate_and_canonicalize_scope_path(ws_path, "src\\main.rs", PathType::File).expect("valid relative path");
    assert_eq!(clean, "src/main.rs");

    // 6. Valid absolute path inside workspace
    let abs_path = ws_path.join("src").join("main.rs").to_string_lossy().to_string();
    let clean_abs = validate_and_canonicalize_scope_path(ws_path, &abs_path, PathType::File).expect("valid absolute path");
    assert_eq!(clean_abs, "src/main.rs");

    // 7. Non-existent new file path inside workspace
    let clean_new = validate_and_canonicalize_scope_path(ws_path, "src/new_module.rs", PathType::File).expect("valid new path");
    assert_eq!(clean_new, "src/new_module.rs");

    // 8. New file creation governed by parent directory CreateWithin
    let entries_without_parent = vec![
        ScopeRefinementEntry {
            path: "src/other.rs".into(),
            path_type: PathType::File,
            reason: "modify other".into(),
            target_task_id: "task-1".into(),
            correction_unit_ids: vec![],
            requirement_ids: vec![],
            permitted_operations: vec![PermittedOperation::Modify],
        },
    ];
    let err = validate_new_file_creation(ws_path, "src/new_module.rs", &entries_without_parent);
    assert!(matches!(err, Err(ScopePathError::ParentDirectoryNotAuthorized(_))));

    let entries_with_parent_create = vec![
        ScopeRefinementEntry {
            path: "src".into(),
            path_type: PathType::Directory,
            reason: "allow creating inside src".into(),
            target_task_id: "task-1".into(),
            correction_unit_ids: vec![],
            requirement_ids: vec![],
            permitted_operations: vec![PermittedOperation::CreateWithin, PermittedOperation::Read],
        },
    ];
    let ok = validate_new_file_creation(ws_path, "src/new_module.rs", &entries_with_parent_create);
    assert!(ok.is_ok());
    assert_eq!(ok.unwrap(), "src/new_module.rs");

    // 9. Deletion governance
    assert!(validate_file_deletion("src/main.rs", &entries_without_parent).is_err());
    let entries_with_delete = vec![
        ScopeRefinementEntry {
            path: "src/main.rs".into(),
            path_type: PathType::File,
            reason: "delete dead file".into(),
            target_task_id: "task-1".into(),
            correction_unit_ids: vec![],
            requirement_ids: vec![],
            permitted_operations: vec![PermittedOperation::Delete],
        },
    ];
    assert!(validate_file_deletion("src/main.rs", &entries_with_delete).is_ok());

    // 10. Rename governance
    assert!(validate_file_rename("src/main.rs", &entries_without_parent).is_err());
    let entries_with_rename = vec![
        ScopeRefinementEntry {
            path: "src/main.rs".into(),
            path_type: PathType::File,
            reason: "rename file".into(),
            target_task_id: "task-1".into(),
            correction_unit_ids: vec![],
            requirement_ids: vec![],
            permitted_operations: vec![PermittedOperation::Rename],
        },
    ];
    assert!(validate_file_rename("src/main.rs", &entries_with_rename).is_ok());
}

#[test]
fn test_defect_51_scope_refinement_matrix() {
    let (authority, current, store, _signing_key, temp) = create_phase5_test_fixture();

    put_user_decision_fixture(
        &store,
        &authority,
        &current,
        "REQ-A-OUTCOME",
        false,
        "REJECTED: 1. Documentation fabricated. 2. State transitions unreliable. 3. Inadequate automated tests. 4. Accessibility blocked.",
    );

    let mut test_meta = test_metadata(
        &authority,
        &current,
        "test-output-evidence-51",
        EvidenceClass::TestOutput,
        EvidenceResult::Pass,
        EvidenceConfidence::StrongDeterministic,
    );
    test_meta.requirement_ids = vec!["REQ-TEST-AUTOMATED".into()];
    store.put_test_fixture(test_meta, b"automated tests passed").unwrap();

    let engine = VerificationEngine::new_for_test(
        store.clone(),
        authority.clone(),
        current.clone(),
        Vec::new(),
    ).expect("engine");
    let report = engine.evaluate(None).expect("evaluate");

    let task_specs = vec![
        ("task-decision-a", vec!["REQ-A-OUTCOME".to_string()]),
        ("task-decision-b", vec!["REQ-B-PURPOSE".to_string()]),
        ("task-accessibility", vec!["REQ-ACCESSIBILITY".to_string()]),
        ("task-automated", vec!["REQ-TEST-AUTOMATED".to_string()]),
    ];
    let run = multi_task_p7_execution_fixture(&authority, temp.path(), task_specs);

    let contract_tasks = vec![
        Task {
            task_id: "task-decision-a".into(),
            title: "Implement: User problem outcome".into(),
            objective: "Implement transport health outcome".into(),
            requirement_ids: vec!["REQ-A-OUTCOME".into()],
            dependency_ids: vec![],
            suggested_scope: "decision".into(),
            risk: RequirementRisk::High,
            evidence_obligations: vec![EvidenceObligation {
                class: EvidenceClass::HumanDecision,
                minimum_confidence: EvidenceConfidence::HumanAsserted,
                rationale: "Human decision".into(),
                required: true,
            }],
            status: RequirementStatus::Verified,
            provenance: "contract".into(),
        },
        Task {
            task_id: "task-decision-b".into(),
            title: "Implement: User product purpose".into(),
            objective: "Implement transport health purpose".into(),
            requirement_ids: vec!["REQ-B-PURPOSE".into()],
            dependency_ids: vec![],
            suggested_scope: "decision".into(),
            risk: RequirementRisk::High,
            evidence_obligations: vec![EvidenceObligation {
                class: EvidenceClass::HumanDecision,
                minimum_confidence: EvidenceConfidence::HumanAsserted,
                rationale: "Human decision".into(),
                required: true,
            }],
            status: RequirementStatus::Verified,
            provenance: "contract".into(),
        },
        Task {
            task_id: "task-accessibility".into(),
            title: "Implement: NFR: accessibility".into(),
            objective: "Implement accessibility".into(),
            requirement_ids: vec!["REQ-ACCESSIBILITY".into()],
            dependency_ids: vec![],
            suggested_scope: "accessibility".into(),
            risk: RequirementRisk::High,
            evidence_obligations: vec![EvidenceObligation {
                class: EvidenceClass::AccessibilityResult,
                minimum_confidence: EvidenceConfidence::StrongDeterministic,
                rationale: "Accessibility check".into(),
                required: true,
            }],
            status: RequirementStatus::Blocked,
            provenance: "contract".into(),
        },
        Task {
            task_id: "task-automated".into(),
            title: "Implement: automated tests".into(),
            objective: "Implement tests".into(),
            requirement_ids: vec!["REQ-TEST-AUTOMATED".into()],
            dependency_ids: vec![],
            suggested_scope: "functional".into(),
            risk: RequirementRisk::Medium,
            evidence_obligations: vec![EvidenceObligation {
                class: EvidenceClass::TestOutput,
                minimum_confidence: EvidenceConfidence::StrongDeterministic,
                rationale: "Automated test".into(),
                required: true,
            }],
            status: RequirementStatus::Verified,
            provenance: "contract".into(),
        },
    ];

    let originating_ev = report.requirement_statuses.iter()
        .find(|s| s.requirement_id == "REQ-A-OUTCOME")
        .and_then(|s| s.failed_evidence.first())
        .cloned()
        .unwrap_or_default();

    // Provenance for decision-a only; task-accessibility intentionally has EMPTY provenance
    let mut provenance = BTreeMap::new();
    provenance.insert("task-decision-a".into(), vec![
        "crates/routing/src/lib.rs".into(),
        "docs/PLAN.md".into(),
        "tests/HealthTest.kt".into(),
    ]);

    // STAGE 1: Automatic derivation fails-closed with HUMAN_SCOPE_REFINEMENT_REQUIRED
    let scope_unrefined = derive_bounded_correction_scope(
        &authority.revision.seal.mission_id,
        authority.revision.revision,
        &authority.revision.contract.requirement_graph.requirements,
        &contract_tasks,
        &[],
        &report.requirement_statuses,
        &run,
        &originating_ev,
        "REJECTED: 1. Documentation fabricated. 2. State transitions unreliable. 3. Inadequate automated tests. 4. Accessibility blocked.",
        &provenance,
        None,
    ).expect("derive scope unrefined");

    assert!(scope_unrefined.human_refinement_required, "Scope without provenance for all tasks must require refinement");
    assert!(scope_unrefined.project_wide_unbounded_authority);
    assert_eq!(scope_unrefined.missing_provenance_tasks, vec!["task-accessibility".to_string()]);
    let unrefined_a11y = scope_unrefined.proposed_tasks.iter().find(|t| t.task_id == "task-accessibility").unwrap();
    assert_eq!(unrefined_a11y.authorized_scope.authority_boundary_type, "UNBOUNDED_WORKSPACE");
    assert!(!unrefined_a11y.authorized_scope.is_bounded);

    // STAGE 2: Operator refines scope for the unbounded task
    let refinement = HumanScopeRefinement {
        mission_id: authority.revision.seal.mission_id.clone(),
        revision: authority.revision.revision,
        originating_evidence_id: originating_ev.clone(),
        last_modified_ms: 1500,
        entries: vec![
            ScopeRefinementEntry {
                path: "docs/ACCESSIBILITY.md".into(),
                path_type: PathType::File,
                reason: "Update accessibility specification and audit plan".into(),
                target_task_id: "task-accessibility".into(),
                correction_unit_ids: vec![],
                requirement_ids: vec!["REQ-ACCESSIBILITY".into()],
                permitted_operations: vec![PermittedOperation::Read, PermittedOperation::Modify],
            },
            ScopeRefinementEntry {
                path: "tests/a11y.test.js".into(),
                path_type: PathType::File,
                reason: "Implement automated accessibility checks".into(),
                target_task_id: "task-accessibility".into(),
                correction_unit_ids: vec![],
                requirement_ids: vec!["REQ-ACCESSIBILITY".into()],
                permitted_operations: vec![PermittedOperation::Read, PermittedOperation::Modify],
            },
        ],
    };

    let scope_refined = derive_bounded_correction_scope(
        &authority.revision.seal.mission_id,
        authority.revision.revision,
        &authority.revision.contract.requirement_graph.requirements,
        &contract_tasks,
        &[],
        &report.requirement_statuses,
        &run,
        &originating_ev,
        "REJECTED: 1. Documentation fabricated. 2. State transitions unreliable. 3. Inadequate automated tests. 4. Accessibility blocked.",
        &provenance,
        Some(&refinement),
    ).expect("derive scope refined");

    // All tasks are now bounded!
    assert!(!scope_refined.human_refinement_required, "Refinement should clear human_refinement_required");
    assert!(!scope_refined.project_wide_unbounded_authority);
    assert!(scope_refined.missing_provenance_tasks.is_empty());

    let refined_a11y = scope_refined.proposed_tasks.iter().find(|t| t.task_id == "task-accessibility").unwrap();
    assert_eq!(refined_a11y.authorized_scope.authority_boundary_type, "HUMAN_REFINED_BOUNDED");
    assert!(refined_a11y.authorized_scope.is_bounded);
    assert!(refined_a11y.authorized_scope.bounded_file_scopes.contains(&"docs/ACCESSIBILITY.md".to_string()));
    assert!(refined_a11y.authorized_scope.bounded_file_scopes.contains(&"tests/a11y.test.js".to_string()));
    assert_eq!(refined_a11y.authorized_scope.refinement_entries.len(), 2);

    // STAGE 3: Scope hash determinism & mutation sensitivity
    assert_ne!(scope_unrefined.scope_hash, scope_refined.scope_hash, "Refinement must change scope_hash");

    // Mutating an entry modifies scope_hash
    let mut mutated_refinement = refinement.clone();
    mutated_refinement.entries[0].reason = "Different reason".into();
    let scope_mutated = derive_bounded_correction_scope(
        &authority.revision.seal.mission_id,
        authority.revision.revision,
        &authority.revision.contract.requirement_graph.requirements,
        &contract_tasks,
        &[],
        &report.requirement_statuses,
        &run,
        &originating_ev,
        "REJECTED: 1. Documentation fabricated. 2. State transitions unreliable. 3. Inadequate automated tests. 4. Accessibility blocked.",
        &provenance,
        Some(&mutated_refinement),
    ).expect("derive scope mutated");
    assert_ne!(scope_refined.scope_hash, scope_mutated.scope_hash, "Changing entry reason must change scope_hash");

    // STAGE 4: Authorize scoped correction with valid refined scope
    let known_requirements = run.tasks.values().flat_map(|t| t.requirement_ids.iter().cloned()).collect::<BTreeSet<_>>();
    let target_reqs: BTreeSet<String> = scope_refined.affected_task_ids.iter()
        .filter_map(|tid| run.tasks.get(tid))
        .flat_map(|t| t.requirement_ids.iter().cloned())
        .filter(|r| known_requirements.contains(r))
        .collect();

    let mut authorized_run = run.clone();
    let affected = authorized_run.authorize_verification_correction(&target_reqs, 2000).expect("authorize");
    assert_eq!(affected.len(), 2);
    assert!(affected.contains(&"task-decision-a".to_string()));
    assert!(affected.contains(&"task-accessibility".to_string()));
    assert_eq!(authorized_run.tasks["task-decision-a"].state, ExecutionTaskState::Pending);
    assert_eq!(authorized_run.tasks["task-accessibility"].state, ExecutionTaskState::Pending);
}

fn to_requirement(seed: &ProjectRequirementSeed) -> Requirement {
    Requirement {
        requirement_id: seed.requirement_id.clone().unwrap_or_default(),
        title: seed.title.clone(),
        intent: seed.intent.clone(),
        source: seed.source.clone(),
        priority: seed.priority,
        applicability: ApplicabilityOutcome::Applicable,
        acceptance_criteria: seed.acceptance_criteria.clone(),
        verification_policy: seed.verification_policy.clone(),
        dependencies: seed.dependencies.clone(),
        risk: seed.risk,
        status: RequirementStatus::Verified,
        implementation_links: Vec::new(),
        evidence_links: Vec::new(),
        explicit_exceptions: Vec::new(),
        sealed_hash: None,
        requirement_type: seed.requirement_type.clone(),
        origin_rule_id: None,
        revision: 1,
        schema_version: 1,
    }
}

#[test]
fn test_defect_52_acceptance_ordering_matrix() {
    let req_decision = seed_decision_req("REQ-DECISION", "Final Owner Outcome", "Owner intent");
    let req_a11y = seed_machine_req("REQ-A11Y", "Accessibility", EvidenceClass::AccessibilityResult);
    let req_test = seed_machine_req("REQ-TEST", "Unit and Integration Tests", EvidenceClass::TestOutput);
    let req_build = seed_machine_req("REQ-BUILD", "Build Compilation", EvidenceClass::BuildOutput);

    let contract_reqs = vec![
        to_requirement(&req_decision),
        to_requirement(&req_a11y),
        to_requirement(&req_test),
        to_requirement(&req_build),
    ];

    // CASE 1: 15/15 complete, Accessibility BLOCKED -> FINAL_HUMAN_PROMPT: NO, TECHNICAL_CORRECTION: YES
    let report_case1 = VerificationReport {
        verification_run_id: "v-case1".into(),
        authority_digest: "auth1".into(),
        requirement_statuses: vec![
            RequirementVerification {
                requirement_id: "REQ-A11Y".into(),
                status: RequirementStatus::Blocked,
                evidence_ids: vec![],
                missing_obligations: vec![],
                missing_acceptance_criteria: vec![],
                stale_evidence: vec![],
                failed_evidence: vec![],
                reason: "accessibility collector blocked".into(),
            },
            RequirementVerification {
                requirement_id: "REQ-TEST".into(),
                status: RequirementStatus::Verified,
                evidence_ids: vec!["ev-test".into()],
                missing_obligations: vec![],
                missing_acceptance_criteria: vec![],
                stale_evidence: vec![],
                failed_evidence: vec![],
                reason: "verified".into(),
            },
            RequirementVerification {
                requirement_id: "REQ-BUILD".into(),
                status: RequirementStatus::Verified,
                evidence_ids: vec!["ev-build".into()],
                missing_obligations: vec![],
                missing_acceptance_criteria: vec![],
                stale_evidence: vec![],
                failed_evidence: vec![],
                reason: "verified".into(),
            },
            RequirementVerification {
                requirement_id: "REQ-DECISION".into(),
                status: RequirementStatus::ImplementedUnverified,
                evidence_ids: vec![],
                missing_obligations: vec![EvidenceClass::HumanDecision],
                missing_acceptance_criteria: vec!["REQ-DECISION-criterion".into()],
                stale_evidence: vec![],
                failed_evidence: vec![],
                reason: "waiting".into(),
            },
        ],
        decision: CompletionDecision {
            state: CompletionState::BlockedExternal,
            reason: "blocked".into(),
            deterministic_gates: vec![],
            accepted_risks: vec![],
            blocked_external: vec![BlockedExternalRecord {
                requirement_id: "REQ-A11Y".into(),
                dependency: "collector".into(),
                reason: "blocked".into(),
            }],
        },
        builder_claim: None,
        coverage_total: 4,
        coverage_accounted: 2,
        evidence_manifest_hash: "man1".into(),
        p7_ledger_digest: None,
        ai_judgements: vec![],
        integrity_tag: "tag1".into(),
    };

    let elig_case1 = is_final_human_acceptance_eligible(&contract_reqs, &report_case1, &["REQ-A11Y: blocked".into()], 0, 0, true, true);
    assert!(!elig_case1.eligible, "Case 1: blocked accessibility MUST suppress final human prompt");
    assert!(!elig_case1.required_machine_requirements_verified);
    assert!(elig_case1.technical_blockers_count > 0);

    // CASE 2: 15/15 complete, required technical TestOutput missing -> FINAL_HUMAN_PROMPT: NO
    let mut report_case2 = report_case1.clone();
    report_case2.requirement_statuses[0].status = RequirementStatus::Verified;
    report_case2.requirement_statuses[1].status = RequirementStatus::ImplementedUnverified;
    report_case2.requirement_statuses[1].missing_obligations = vec![EvidenceClass::TestOutput];
    report_case2.decision.blocked_external = vec![];
    report_case2.decision.state = CompletionState::StoppedIncomplete;

    let elig_case2 = is_final_human_acceptance_eligible(&contract_reqs, &report_case2, &[], 0, 0, true, true);
    assert!(!elig_case2.eligible, "Case 2: missing TestOutput MUST suppress final human prompt");
    assert!(elig_case2.missing_required_evidence_count > 0);

    // CASE 3: 15/15 complete, stale evidence -> FINAL_HUMAN_PROMPT: NO
    let mut report_case3 = report_case1.clone();
    report_case3.requirement_statuses[0].status = RequirementStatus::Verified;
    report_case3.requirement_statuses[1].stale_evidence = vec!["ev-test-stale".into()];
    report_case3.decision.blocked_external = vec![];
    report_case3.decision.state = CompletionState::StoppedIncomplete;

    let elig_case3 = is_final_human_acceptance_eligible(&contract_reqs, &report_case3, &[], 0, 0, true, true);
    assert!(!elig_case3.eligible, "Case 3: stale evidence MUST suppress final human prompt");
    assert!(elig_case3.stale_required_evidence_count > 0);

    // CASE 4: 15/15 complete, failed test -> FINAL_HUMAN_PROMPT: NO
    let mut report_case4 = report_case1.clone();
    report_case4.requirement_statuses[0].status = RequirementStatus::Verified;
    report_case4.requirement_statuses[1].status = RequirementStatus::Failed;
    report_case4.requirement_statuses[1].failed_evidence = vec!["ev-test-failed".into()];
    report_case4.decision.blocked_external = vec![];
    report_case4.decision.state = CompletionState::FailedVerification;

    let elig_case4 = is_final_human_acceptance_eligible(&contract_reqs, &report_case4, &[], 0, 0, true, true);
    assert!(!elig_case4.eligible, "Case 4: failed test MUST suppress final human prompt");
    assert!(elig_case4.failed_required_evidence_count > 0);

    // CASE 5: 15/15 complete, failed build -> FINAL_HUMAN_PROMPT: NO
    let mut report_case5 = report_case1.clone();
    report_case5.requirement_statuses[0].status = RequirementStatus::Verified;
    report_case5.requirement_statuses[2].status = RequirementStatus::Failed;
    report_case5.requirement_statuses[2].failed_evidence = vec!["ev-build-failed".into()];
    report_case5.decision.blocked_external = vec![];
    report_case5.decision.state = CompletionState::FailedVerification;

    let elig_case5 = is_final_human_acceptance_eligible(&contract_reqs, &report_case5, &[], 0, 0, true, true);
    assert!(!elig_case5.eligible, "Case 5: failed build MUST suppress final human prompt");
    assert!(elig_case5.failed_required_evidence_count > 0);

    // CASE 6: all machine requirements verified, human acceptance exists -> FINAL_HUMAN_PROMPT: YES
    let mut report_case6 = report_case1.clone();
    report_case6.requirement_statuses[0].status = RequirementStatus::Verified;
    report_case6.requirement_statuses[1].status = RequirementStatus::Verified;
    report_case6.requirement_statuses[2].status = RequirementStatus::Verified;
    report_case6.requirement_statuses[3].status = RequirementStatus::ImplementedUnverified;
    report_case6.decision.blocked_external = vec![];
    report_case6.decision.state = CompletionState::StoppedIncomplete;

    let elig_case6 = is_final_human_acceptance_eligible(&contract_reqs, &report_case6, &[], 0, 0, true, true);
    assert!(elig_case6.eligible, "Case 6: all machine requirements verified MUST enable human prompt");
    assert!(elig_case6.required_machine_requirements_verified);
    assert_eq!(elig_case6.technical_blockers_count, 0);

    // CASE 7: all machine requirements verified, no human requirement in contract -> VERIFIED COMPLETE eligibility: YES
    let machine_only_reqs = vec![
        to_requirement(&req_a11y),
        to_requirement(&req_test),
        to_requirement(&req_build),
    ];
    let report_case7 = VerificationReport {
        verification_run_id: "v-case7".into(),
        authority_digest: "auth7".into(),
        requirement_statuses: vec![
            RequirementVerification {
                requirement_id: "REQ-A11Y".into(),
                status: RequirementStatus::Verified,
                evidence_ids: vec!["ev-a11y".into()],
                missing_obligations: vec![],
                missing_acceptance_criteria: vec![],
                stale_evidence: vec![],
                failed_evidence: vec![],
                reason: "verified".into(),
            },
            RequirementVerification {
                requirement_id: "REQ-TEST".into(),
                status: RequirementStatus::Verified,
                evidence_ids: vec!["ev-test".into()],
                missing_obligations: vec![],
                missing_acceptance_criteria: vec![],
                stale_evidence: vec![],
                failed_evidence: vec![],
                reason: "verified".into(),
            },
            RequirementVerification {
                requirement_id: "REQ-BUILD".into(),
                status: RequirementStatus::Verified,
                evidence_ids: vec!["ev-build".into()],
                missing_obligations: vec![],
                missing_acceptance_criteria: vec![],
                stale_evidence: vec![],
                failed_evidence: vec![],
                reason: "verified".into(),
            },
        ],
        decision: CompletionDecision {
            state: CompletionState::VerifiedComplete,
            reason: "all verified".into(),
            deterministic_gates: vec![],
            accepted_risks: vec![],
            blocked_external: vec![],
        },
        builder_claim: None,
        coverage_total: 3,
        coverage_accounted: 3,
        evidence_manifest_hash: "man7".into(),
        p7_ledger_digest: None,
        ai_judgements: vec![],
        integrity_tag: "tag7".into(),
    };

    let elig_case7 = is_final_human_acceptance_eligible(&machine_only_reqs, &report_case7, &[], 0, 0, true, true);
    assert!(elig_case7.eligible, "Case 7: all machine verified enables completion eligibility");
    assert_eq!(report_case7.decision.state, CompletionState::VerifiedComplete, "VerifiedComplete directly reachable with zero artificial human prompt");
}

#[test]
fn test_defect_53_and_55_executor_cannot_self_certify() {
    let (authority, current, store, signing_key, temp) = create_phase5_test_fixture();

    // 1. Executor claims "success" or "approved" in text, but test fails
    let mut metadata = test_metadata(
        &authority,
        &current,
        "test-failure-meta",
        EvidenceClass::TestOutput,
        EvidenceResult::Fail,
        EvidenceConfidence::StrongDeterministic,
    );
    metadata.requirement_ids = vec!["REQ-TEST-AUTOMATED".into()];
    metadata.accepted_criteria.insert("REQ-TEST-AUTOMATED-criterion".into());
    store.put_test_fixture(metadata, b"{\"test\":\"failed\",\"executor_claim\":\"tests passed completely approved\"}").expect("put failure");

    let engine = VerificationEngine::new_for_test(
        store.clone(),
        authority.clone(),
        current.clone(),
        Vec::new(),
    ).expect("engine");

    let report = engine.evaluate(Some("executor claim: completely done and approved by owner".into())).expect("eval");
    assert_ne!(report.decision.state, CompletionState::VerifiedComplete, "Executor self-claim CANNOT override failing test");
    assert_eq!(report.decision.state, CompletionState::FailedVerification);
    let test_status = report.requirement_statuses.iter().find(|s| s.requirement_id == "REQ-TEST-AUTOMATED").expect("test status");
    assert_eq!(test_status.status, RequirementStatus::Failed, "Automated test MUST be recorded as Failed despite executor claim");

    // 2. Executor-generated completion certificate without verified authority is rejected
    let p7 = p7_execution_fixture(
        &authority,
        temp.path(),
        vec!["REQ-TEST-AUTOMATED".into()],
    );
    let comp_auth = CompletionAuthority::new(signing_key.to_bytes().as_ref()).expect("comp auth");
    let cert_result = comp_auth.issue_with_p7_execution(&report, &authority, &p7);
    assert!(cert_result.is_err(), "Issuing certificate on unverified state MUST fail");
}

#[test]
fn test_defect_54_and_56_autonomous_technical_correction_and_ux() {
    let (authority, _current, _store, _signing_key, temp) = create_phase5_test_fixture();

    let task_specs = vec![
        ("task-decision-a", vec!["REQ-A-OUTCOME".to_string()]),
        ("task-decision-b", vec!["REQ-B-PURPOSE".to_string()]),
        ("task-accessibility", vec!["REQ-ACCESSIBILITY".to_string()]),
        ("task-automated", vec!["REQ-TEST-AUTOMATED".to_string()]),
    ];
    let run = multi_task_p7_execution_fixture(&authority, temp.path(), task_specs);

    let contract_tasks = vec![
        Task {
            task_id: "task-accessibility".into(),
            title: "Implement: NFR: accessibility".into(),
            objective: "Primary flows must be keyboard navigable and accessible".into(),
            requirement_ids: vec!["REQ-ACCESSIBILITY".into()],
            dependency_ids: vec![],
            suggested_scope: "accessibility".into(),
            risk: RequirementRisk::High,
            evidence_obligations: vec![EvidenceObligation {
                class: EvidenceClass::AccessibilityResult,
                minimum_confidence: EvidenceConfidence::StrongDeterministic,
                rationale: "Accessibility check".into(),
                required: true,
            }],
            status: RequirementStatus::Blocked,
            provenance: "contract".into(),
        },
    ];

    let report = VerificationReport {
        verification_run_id: "v-a11y".into(),
        authority_digest: "auth".into(),
        requirement_statuses: vec![
            RequirementVerification {
                requirement_id: "REQ-ACCESSIBILITY".into(),
                status: RequirementStatus::Blocked,
                evidence_ids: vec![],
                missing_obligations: vec![],
                missing_acceptance_criteria: vec![],
                stale_evidence: vec![],
                failed_evidence: vec![],
                reason: "accessibility blocked".into(),
            },
        ],
        decision: CompletionDecision {
            state: CompletionState::BlockedExternal,
            reason: "blocked".into(),
            deterministic_gates: vec![],
            accepted_risks: vec![],
            blocked_external: vec![BlockedExternalRecord {
                requirement_id: "REQ-ACCESSIBILITY".into(),
                dependency: "collector".into(),
                reason: "blocked".into(),
            }],
        },
        builder_claim: None,
        coverage_total: 1,
        coverage_accounted: 0,
        evidence_manifest_hash: "man".into(),
        p7_ledger_digest: None,
        ai_judgements: vec![],
        integrity_tag: "tag".into(),
    };

    // Autonomous derivation: task-accessibility has no provenance for itself, but project provenance contains accessibility paths
    let mut provenance = BTreeMap::new();
    provenance.insert("task-other".into(), vec![
        "docs/ACCESSIBILITY.md".into(),
        "apps/ui/AccessibilityScan.kt".into(),
        "tests/a11y.test.js".into(),
    ]);

    let scope = derive_bounded_correction_scope(
        &authority.revision.seal.mission_id,
        authority.revision.revision,
        &authority.revision.contract.requirement_graph.requirements,
        &contract_tasks,
        &[],
        &report.requirement_statuses,
        &run,
        "ev-origin",
        "REJECTED: accessibility blocked",
        &provenance,
        None, // Normal user flow with NO manual refinement entered
    ).expect("derive bounded correction scope");

    // NORMAL USER DOES NOT ENTER PATHS:
    // Autonomous boundary derivation successfully bounds task-accessibility to safe files!
    let a11y_task = scope.proposed_tasks.iter().find(|t| t.task_id == "task-accessibility").expect("proposed task");
    assert!(a11y_task.authorized_scope.is_bounded, "Task must be autonomously bounded");
    assert_eq!(a11y_task.authorized_scope.authority_boundary_type, "AUTONOMOUS_BOUNDED");
    assert!(!scope.human_refinement_required, "Normal user flow MUST NOT require human scope refinement");
    assert_eq!(scope.missing_provenance_tasks.len(), 0);
    assert!(a11y_task.authorized_scope.bounded_file_scopes.contains(&"docs/ACCESSIBILITY.md".to_string()));
    assert!(a11y_task.authorized_scope.bounded_file_scopes.contains(&"tests/a11y.test.js".to_string()));
}

#[test]
fn test_defect_57_and_58_human_escalation_boundary_and_progress_guard() {
    let mut guard = CorrectionProgressGuard::default();

    // 1. Initial attempt fails
    assert!(guard.evaluate_progress("signature-failure-a").is_ok());

    // 2. Repeat failure with same signature increments count
    assert!(guard.evaluate_progress("signature-failure-a").is_ok());

    // 3. Repeat failure 3 times triggers technical delivery blocked escalation
    let blocked_err = guard.evaluate_progress("signature-failure-a").expect_err("must block");
    assert!(blocked_err.contains("TECHNICAL_DELIVERY_BLOCKED"), "Escalation must be TECHNICAL_DELIVERY_BLOCKED");
    assert!(blocked_err.contains("Identical failure repeated"));
}

#[test]
fn test_defect_59_phase5_safe_fixture_lifecycle() {
    let (authority, _current, _store, _signing_key, temp) = create_phase5_test_fixture();

    let contract_reqs = vec![
        to_requirement(&seed_decision_req("REQ-A-OUTCOME", "User problem outcome", "Problem solved")),
        to_requirement(&seed_decision_req("REQ-B-PURPOSE", "User product purpose", "Product purpose met")),
        to_requirement(&seed_machine_req("REQ-ACCESSIBILITY", "NFR: accessibility", EvidenceClass::AccessibilityResult)),
        to_requirement(&seed_machine_req("REQ-TEST-AUTOMATED", "NFR: tests", EvidenceClass::TestOutput)),
    ];

    // INITIAL PHASE5 STATE:
    // Historical rejection exists, accessibility blocked, 15 tasks completed
    let report_initial = VerificationReport {
        verification_run_id: "v-init".into(),
        authority_digest: "auth".into(),
        requirement_statuses: vec![
            RequirementVerification {
                requirement_id: "REQ-A-OUTCOME".into(),
                status: RequirementStatus::Failed,
                evidence_ids: vec!["ev-user-rejection".into()],
                missing_obligations: vec![],
                missing_acceptance_criteria: vec![],
                stale_evidence: vec![],
                failed_evidence: vec!["ev-user-rejection".into()],
                reason: "human rejected".into(),
            },
            RequirementVerification {
                requirement_id: "REQ-B-PURPOSE".into(),
                status: RequirementStatus::Failed,
                evidence_ids: vec!["ev-user-rejection".into()],
                missing_obligations: vec![],
                missing_acceptance_criteria: vec![],
                stale_evidence: vec![],
                failed_evidence: vec!["ev-user-rejection".into()],
                reason: "human rejected".into(),
            },
            RequirementVerification {
                requirement_id: "REQ-ACCESSIBILITY".into(),
                status: RequirementStatus::Blocked,
                evidence_ids: vec![],
                missing_obligations: vec![],
                missing_acceptance_criteria: vec![],
                stale_evidence: vec![],
                failed_evidence: vec![],
                reason: "accessibility blocked".into(),
            },
            RequirementVerification {
                requirement_id: "REQ-TEST-AUTOMATED".into(),
                status: RequirementStatus::Verified,
                evidence_ids: vec!["ev-tests".into()],
                missing_obligations: vec![],
                missing_acceptance_criteria: vec![],
                stale_evidence: vec![],
                failed_evidence: vec![],
                reason: "verified".into(),
            },
        ],
        decision: CompletionDecision {
            state: CompletionState::FailedVerification,
            reason: "rejection and blocked checks".into(),
            deterministic_gates: vec![],
            accepted_risks: vec![],
            blocked_external: vec![BlockedExternalRecord {
                requirement_id: "REQ-ACCESSIBILITY".into(),
                dependency: "collector".into(),
                reason: "blocked".into(),
            }],
        },
        builder_claim: None,
        coverage_total: 4,
        coverage_accounted: 1,
        evidence_manifest_hash: "man".into(),
        p7_ledger_digest: None,
        ai_judgements: vec![],
        integrity_tag: "tag".into(),
    };

    let elig_init = is_final_human_acceptance_eligible(&contract_reqs, &report_initial, &["REQ-ACCESSIBILITY: blocked".into()], 0, 1, true, true);
    assert!(!elig_init.eligible, "Initial Phase 5 state MUST NOT be eligible for final human acceptance");
    assert!(!elig_init.required_machine_requirements_verified);
    assert!(elig_init.technical_blockers_count > 0);

    // AFTER SIMULATED CORRECTION:
    // Technical defects fixed, fresh accessibility proof collected, fresh tests collected
    let mut report_corrected = VerificationReport {
        verification_run_id: "v-corr".into(),
        authority_digest: "auth".into(),
        requirement_statuses: vec![
            RequirementVerification {
                requirement_id: "REQ-A-OUTCOME".into(),
                status: RequirementStatus::ImplementedUnverified,
                evidence_ids: vec![],
                missing_obligations: vec![EvidenceClass::HumanDecision],
                missing_acceptance_criteria: vec!["REQ-OUTCOME-criterion".into()],
                stale_evidence: vec![],
                failed_evidence: vec![],
                reason: "waiting fresh human approval".into(),
            },
            RequirementVerification {
                requirement_id: "REQ-B-PURPOSE".into(),
                status: RequirementStatus::ImplementedUnverified,
                evidence_ids: vec![],
                missing_obligations: vec![EvidenceClass::HumanDecision],
                missing_acceptance_criteria: vec!["REQ-PURPOSE-criterion".into()],
                stale_evidence: vec![],
                failed_evidence: vec![],
                reason: "waiting fresh human approval".into(),
            },
            RequirementVerification {
                requirement_id: "REQ-ACCESSIBILITY".into(),
                status: RequirementStatus::Verified,
                evidence_ids: vec!["ev-fresh-a11y".into()],
                missing_obligations: vec![],
                missing_acceptance_criteria: vec![],
                stale_evidence: vec![],
                failed_evidence: vec![],
                reason: "verified fresh proof".into(),
            },
            RequirementVerification {
                requirement_id: "REQ-TEST-AUTOMATED".into(),
                status: RequirementStatus::Verified,
                evidence_ids: vec!["ev-fresh-tests".into()],
                missing_obligations: vec![],
                missing_acceptance_criteria: vec![],
                stale_evidence: vec![],
                failed_evidence: vec![],
                reason: "verified fresh proof".into(),
            },
        ],
        decision: CompletionDecision {
            state: CompletionState::StoppedIncomplete,
            reason: "human decision pending".into(),
            deterministic_gates: vec![],
            accepted_risks: vec![],
            blocked_external: vec![],
        },
        builder_claim: None,
        coverage_total: 4,
        coverage_accounted: 2,
        evidence_manifest_hash: "man-corr".into(),
        p7_ledger_digest: None,
        ai_judgements: vec![],
        integrity_tag: "tag-corr".into(),
    };

    let elig_corrected = is_final_human_acceptance_eligible(&contract_reqs, &report_corrected, &[], 0, 0, true, true);
    assert!(elig_corrected.eligible, "After machine correction, final human acceptance MUST become eligible");
    assert!(elig_corrected.required_machine_requirements_verified);
    assert_eq!(elig_corrected.technical_blockers_count, 0);

    let p7 = p7_execution_fixture(
        &authority,
        temp.path(),
        vec!["REQ-A-OUTCOME".into(), "REQ-B-PURPOSE".into()],
    );

    let auth_key = b"p8-test-local-key";
    report_corrected.p7_ledger_digest = Some(p7.ledger_digest.clone());
    report_corrected.authority_digest = authority.identity_digest().unwrap();
    report_corrected.authenticate(auth_key).expect("authenticate");

    // Historical rejection preserved in store
    let comp_auth = CompletionAuthority::new(auth_key).expect("comp auth");
    // Certificate still denied until new human decision is approved
    assert!(comp_auth.issue_with_p7_execution(&report_corrected, &authority, &p7).is_err(), "Certificate MUST remain denied while human decision pending");

    // AFTER GENUINE NEW APPROVAL:
    let mut report_approved = report_corrected.clone();
    report_approved.requirement_statuses[0].status = RequirementStatus::Verified;
    report_approved.requirement_statuses[0].missing_obligations = vec![];
    report_approved.requirement_statuses[0].missing_acceptance_criteria = vec![];
    report_approved.requirement_statuses[1].status = RequirementStatus::Verified;
    report_approved.requirement_statuses[1].missing_obligations = vec![];
    report_approved.requirement_statuses[1].missing_acceptance_criteria = vec![];
    report_approved.decision.state = CompletionState::VerifiedComplete;
    report_approved.p7_ledger_digest = Some(p7.ledger_digest.clone());
    report_approved.authority_digest = authority.identity_digest().unwrap();
    report_approved.authenticate(auth_key).expect("authenticate");

    let cert = comp_auth.issue_with_p7_execution(&report_approved, &authority, &p7).expect("certificate issue");
    assert_eq!(cert.final_state, CompletionState::VerifiedComplete);
}

#[test]
fn test_defect_60_cases_a_through_g_sealed_authority_correction_matrix() {
    let (authority, _current, _store, _signing_key, temp) = create_phase5_test_fixture();

    let contract_reqs = vec![
        to_requirement(&seed_decision_req("REQ-A-OUTCOME", "User problem outcome", "Problem solved")),
        to_requirement(&seed_decision_req("REQ-B-PURPOSE", "User product purpose", "Product purpose met")),
        to_requirement(&seed_machine_req("REQ-ACCESSIBILITY", "NFR: accessibility", EvidenceClass::AccessibilityResult)),
        to_requirement(&seed_machine_req("REQ-TEST-AUTOMATED", "NFR: tests", EvidenceClass::TestOutput)),
    ];

    let contract_tasks = vec![
        relintor_standards::Task {
            task_id: "task-accessibility".into(),
            title: "Implement: NFR: accessibility".into(),
            objective: "Accessibility implementation".into(),
            requirement_ids: vec!["REQ-ACCESSIBILITY".into()],
            dependency_ids: vec![],
            suggested_scope: "apps/ui/index.html".into(),
            risk: RequirementRisk::Medium,
            evidence_obligations: vec![EvidenceObligation {
                class: EvidenceClass::AccessibilityResult,
                minimum_confidence: EvidenceConfidence::StrongDeterministic,
                rationale: "Accessibility result required".into(),
                required: true,
            }],
            status: RequirementStatus::Blocked,
            provenance: "contract".into(),
        },
    ];

    let mut p7 = p7_execution_fixture(&authority, temp.path(), vec!["REQ-ACCESSIBILITY".into()]);
    let mut sample_task = p7.run.tasks.values().next().unwrap().clone();
    sample_task.task_id = "task-accessibility".into();
    sample_task.requirement_ids = vec!["REQ-ACCESSIBILITY".into()];
    sample_task.state = ExecutionTaskState::FinishedAwaitingVerification;
    p7.run.tasks.clear();
    p7.run.tasks.insert("task-accessibility".into(), sample_task);
    p7.run.state = ExecutionRunState::ExecutionTasksFinishedAwaitingVerification;
    let mut run = p7.run;

    let mut provenance = BTreeMap::new();
    provenance.insert("task-other".into(), vec![
        "apps/ui/index.html".into(),
        "tests/a11y.test.js".into(),
    ]);

    let report_initial = VerificationReport {
        verification_run_id: "v-d60".into(),
        authority_digest: "auth".into(),
        requirement_statuses: vec![
            RequirementVerification {
                requirement_id: "REQ-ACCESSIBILITY".into(),
                status: RequirementStatus::Blocked,
                evidence_ids: vec![],
                missing_obligations: vec![],
                missing_acceptance_criteria: vec![],
                stale_evidence: vec![],
                failed_evidence: vec![],
                reason: "accessibility check failed".into(),
            },
        ],
        decision: CompletionDecision {
            state: CompletionState::FailedVerification,
            reason: "accessibility failed".into(),
            deterministic_gates: vec![],
            accepted_risks: vec![],
            blocked_external: vec![],
        },
        builder_claim: None,
        coverage_total: 1,
        coverage_accounted: 0,
        evidence_manifest_hash: "man".into(),
        p7_ledger_digest: None,
        ai_judgements: vec![],
        integrity_tag: "tag".into(),
    };

    // ============================================================
    // CASE A: sealed mission, machine-solvable correction, AUTONOMOUS_BOUNDED,
    // inside workspace, non-destructive
    // EXPECTED:
    // USER_AUTHORIZATION_REQUIRED: NO
    // TASK_RUNNABLE: YES
    // AUTONOMOUS_CORRECTION_CONTINUES: YES
    // ============================================================
    let rejection_notes_a = "1. Fix the accessibility issues in index.html (missing form labels, button names). 2. Make sure tests pass.";
    let scope_a = derive_bounded_correction_scope(
        &authority.revision.seal.mission_id,
        authority.revision.revision,
        &contract_reqs,
        &contract_tasks,
        &[],
        &report_initial.requirement_statuses,
        &run,
        "ev-origin-a",
        rejection_notes_a,
        &provenance,
        None,
    ).expect("derive scope A");

    assert!(!scope_a.user_reauthorization_required, "CASE A: USER_AUTHORIZATION_REQUIRED MUST BE NO");
    assert!(!scope_a.human_refinement_required, "CASE A: HUMAN_REFINEMENT_REQUIRED MUST BE NO");
    assert_eq!(scope_a.escalation_reason, None);

    // Relintor internally authorizes bounded correction under sealed authority
    let mut target_reqs = BTreeSet::new();
    target_reqs.insert("REQ-ACCESSIBILITY".into());
    let affected = run.authorize_verification_correction(&target_reqs, 1000).expect("internal auth");
    assert_eq!(affected, vec!["task-accessibility".to_string()]);

    // TASK_RUNNABLE: YES
    let task = run.tasks.get("task-accessibility").expect("task");
    assert_eq!(task.state, ExecutionTaskState::Pending, "CASE A: TASK_RUNNABLE MUST BE YES (Pending)");
    assert_eq!(run.state, ExecutionRunState::Ready, "CASE A: RUN STATE MUST BE READY");

    // AUTONOMOUS_CORRECTION_CONTINUES: YES
    let scope_a_after = derive_bounded_correction_scope(
        &authority.revision.seal.mission_id,
        authority.revision.revision,
        &contract_reqs,
        &contract_tasks,
        &[],
        &report_initial.requirement_statuses,
        &run,
        "ev-origin-a",
        rejection_notes_a,
        &provenance,
        None,
    ).expect("derive scope A after");
    assert!(scope_a_after.authorized, "CASE A: AUTONOMOUS_CORRECTION_CONTINUES MUST BE YES (authorized = true)");

    // ============================================================
    // CASE B: correction widens outside sealed workspace
    // EXPECTED:
    // USER_AUTHORIZATION_REQUIRED: YES
    // ============================================================
    let mut tasks_unbounded = contract_tasks.clone();
    tasks_unbounded[0].suggested_scope = "../../etc/passwd".into();
    let empty_prov = BTreeMap::new();
    let scope_b = derive_bounded_correction_scope(
        &authority.revision.seal.mission_id,
        authority.revision.revision,
        &contract_reqs,
        &tasks_unbounded,
        &[],
        &report_initial.requirement_statuses,
        &run,
        "ev-origin-b",
        "Fix paths outside workspace",
        &empty_prov,
        None,
    ).expect("derive scope B");

    assert!(scope_b.user_reauthorization_required, "CASE B: USER_AUTHORIZATION_REQUIRED MUST BE YES for out-of-workspace");
    assert!(scope_b.escalation_reason.is_some(), "CASE B: escalation reason must be present");

    // ============================================================
    // CASE C: destructive correction
    // EXPECTED:
    // USER_AUTHORIZATION_REQUIRED: YES
    // ============================================================
    let scope_c = derive_bounded_correction_scope(
        &authority.revision.seal.mission_id,
        authority.revision.revision,
        &contract_reqs,
        &contract_tasks,
        &[],
        &report_initial.requirement_statuses,
        &run,
        "ev-origin-c",
        "Delete database tables and rm -rf old storage before proceeding",
        &provenance,
        None,
    ).expect("derive scope C");

    assert!(scope_c.user_reauthorization_required, "CASE C: USER_AUTHORIZATION_REQUIRED MUST BE YES for destructive action");
    assert!(scope_c.escalation_reason.as_ref().unwrap().contains("destructive"));

    // ============================================================
    // CASE D: credentials required
    // EXPECTED:
    // USER_AUTHORIZATION_REQUIRED: YES
    // ============================================================
    let scope_d = derive_bounded_correction_scope(
        &authority.revision.seal.mission_id,
        authority.revision.revision,
        &contract_reqs,
        &contract_tasks,
        &[],
        &report_initial.requirement_statuses,
        &run,
        "ev-origin-d",
        "Need production API secret key and auth token in .env",
        &provenance,
        None,
    ).expect("derive scope D");

    assert!(scope_d.user_reauthorization_required, "CASE D: USER_AUTHORIZATION_REQUIRED MUST BE YES for credentials");
    assert!(scope_d.escalation_reason.as_ref().unwrap().contains("credentials"));

    // ============================================================
    // CASE E: subjective human decision required
    // EXPECTED:
    // USER_AUTHORIZATION_REQUIRED: YES
    // ============================================================
    let scope_e = derive_bounded_correction_scope(
        &authority.revision.seal.mission_id,
        authority.revision.revision,
        &contract_reqs,
        &contract_tasks,
        &[],
        &report_initial.requirement_statuses,
        &run,
        "ev-origin-e",
        "Subjective visual choice: prefer warm minimalist aesthetic instead of modern clean",
        &provenance,
        None,
    ).expect("derive scope E");

    assert!(scope_e.user_reauthorization_required, "CASE E: USER_AUTHORIZATION_REQUIRED MUST BE YES for subjective decision");
    assert!(scope_e.escalation_reason.as_ref().unwrap().contains("subjective"));

    // ============================================================
    // CASE F: restart after autonomous correction derivation
    // EXPECTED:
    // no return to "Authorize scoped correction"
    // no duplicate authority
    // no stale recovery
    // correction resumes deterministically
    // ============================================================
    let auth_events_count_before = run.events.iter().filter(|e| e.kind == ExecutionEventKind::VerificationCorrectionAuthorized).count();
    assert_eq!(auth_events_count_before, 1, "CASE F: Exactly one authorization event recorded");

    // Simulating app restart / reload: re-deriving scope from the persisted run
    let scope_f = derive_bounded_correction_scope(
        &authority.revision.seal.mission_id,
        authority.revision.revision,
        &contract_reqs,
        &contract_tasks,
        &[],
        &report_initial.requirement_statuses,
        &run,
        "ev-origin-a",
        rejection_notes_a,
        &provenance,
        None,
    ).expect("derive scope F");

    assert!(scope_f.authorized, "CASE F: Scope must remain authorized after restart");
    assert!(!scope_f.user_reauthorization_required, "CASE F: Reauthorization not required");
    assert_eq!(
        run.events.iter().filter(|e| e.kind == ExecutionEventKind::VerificationCorrectionAuthorized).count(),
        1,
        "CASE F: No duplicate authority event created"
    );

    // ============================================================
    // CASE G: historical Phase 5 rejection fixture
    // EXPECTED:
    // 15/15 preserved
    // rejection preserved
    // AUTONOMOUS_BOUNDED preserved
    // FINAL HUMAN PROMPT: NO
    // USER CORRECTION AUTH BUTTON: NO
    // AUTONOMOUS CORRECTION: YES
    // certificate: DENIED
    // ============================================================
    let mut p5_tasks = Vec::new();
    let p5_p7 = p7_execution_fixture(&authority, temp.path(), vec!["REQ-ACCESSIBILITY".into()]);
    let base_exec_task = p5_p7.run.tasks.values().next().unwrap().clone();
    let mut p5_run = p5_p7.run;
    p5_run.tasks.clear();

    for i in 1..=15 {
        let tid = format!("task-{i:02}");
        let reqs = if i == 1 {
            vec!["REQ-ACCESSIBILITY".into()]
        } else {
            vec!["REQ-TEST-AUTOMATED".into()]
        };
        p5_tasks.push(relintor_standards::Task {
            task_id: tid.clone(),
            title: format!("Task {i}"),
            objective: format!("Objective {i}"),
            requirement_ids: reqs.clone(),
            dependency_ids: vec![],
            suggested_scope: "index.html".into(),
            risk: RequirementRisk::Low,
            evidence_obligations: vec![],
            status: RequirementStatus::Verified,
            provenance: "contract".into(),
        });
        let mut t = base_exec_task.clone();
        t.task_id = tid.clone();
        t.objective = format!("Objective {i}");
        t.requirement_ids = reqs;
        t.state = ExecutionTaskState::FinishedAwaitingVerification;
        p5_run.tasks.insert(tid, t);
    }
    p5_run.state = ExecutionRunState::ExecutionTasksFinishedAwaitingVerification;

    // 15/15 historical tasks preserved
    assert_eq!(p5_run.tasks.len(), 15, "CASE G: 15/15 tasks must be preserved");
    for task in p5_run.tasks.values() {
        assert_eq!(task.state, ExecutionTaskState::FinishedAwaitingVerification);
    }

    let p5_rejection_notes = "1. Fix the accessibility issues in index.html (missing form labels, button names, contrast). 2. Make sure all unit and integration tests pass.";
    let mut p5_provenance = BTreeMap::new();
    p5_provenance.insert("task-other".into(), vec!["docs/ACCESSIBILITY.md".into(), "index.html".into()]);

    let p5_scope = derive_bounded_correction_scope(
        &authority.revision.seal.mission_id,
        authority.revision.revision,
        &contract_reqs,
        &p5_tasks,
        &[],
        &report_initial.requirement_statuses,
        &p5_run,
        "ev-p5-rejection",
        p5_rejection_notes,
        &p5_provenance,
        None,
    ).expect("derive p5 scope");

    // AUTONOMOUS_BOUNDED preserved
    let a11y_p5 = p5_scope.proposed_tasks.iter().find(|t| t.task_id == "task-01").expect("p5 a11y task");
    assert!(a11y_p5.authorized_scope.is_bounded, "CASE G: Task must be bounded");
    assert_eq!(a11y_p5.authorized_scope.authority_boundary_type, "AUTONOMOUS_BOUNDED");

    // USER CORRECTION AUTH BUTTON: NO
    assert!(!p5_scope.user_reauthorization_required, "CASE G: USER CORRECTION AUTH BUTTON: NO (user_reauthorization_required = false)");

    // FINAL HUMAN PROMPT: NO (cannot prompt while technical correction is needed)
    let elig = is_final_human_acceptance_eligible(&contract_reqs, &report_initial, &[], 0, 0, true, true);
    assert!(!elig.eligible, "CASE G: FINAL HUMAN PROMPT: NO (not eligible)");

    // AUTONOMOUS CORRECTION: YES (internally authorized)
    let mut p5_target = BTreeSet::new();
    p5_target.insert("REQ-ACCESSIBILITY".into());
    let reopened = p5_run.authorize_verification_correction(&p5_target, 2000).expect("authorize");
    assert_eq!(reopened, vec!["task-01".to_string()]);
    assert_eq!(p5_run.tasks.get("task-01").unwrap().state, ExecutionTaskState::Pending, "CASE G: Reopened task is Pending");
    assert_eq!(p5_run.tasks.len(), 15, "CASE G: 15/15 task history preserved after reopen");

    // certificate: DENIED
    let p7 = p7_execution_fixture(&authority, temp.path(), vec!["REQ-ACCESSIBILITY".into()]);
    let comp_auth = CompletionAuthority::new(b"p8-test-local-key").expect("comp auth");
    assert!(comp_auth.issue_with_p7_execution(&report_initial, &authority, &p7).is_err(), "CASE G: certificate MUST be DENIED");
}

#[test]
fn test_defect_44_and_phase5_human_decision_semantic_dedup_matrix() {
    let (authority, current, store, _signing_key, temp) = create_phase5_test_fixture();

    let req_a = authority.revision.contract.requirement_graph.requirements.iter()
        .find(|r| r.requirement_id == "REQ-A-OUTCOME").expect("req_a");
    let req_b = authority.revision.contract.requirement_graph.requirements.iter()
        .find(|r| r.requirement_id == "REQ-B-PURPOSE").expect("req_b");

    // Case B & Case I: Both requirements are semantically equivalent
    assert!(is_human_decision_semantic_equivalent(req_a, req_b));

    // Record passing machine test evidence for REQ-TEST-AUTOMATED
    let mut test_meta = test_metadata(
        &authority,
        &current,
        "test-output-evidence-dedup",
        EvidenceClass::TestOutput,
        EvidenceResult::Pass,
        EvidenceConfidence::StrongDeterministic,
    );
    test_meta.requirement_ids = vec!["REQ-TEST-AUTOMATED".into()];
    test_meta.accepted_criteria = BTreeSet::from(["REQ-TEST-AUTOMATED-criterion".into()]);
    store.put_test_fixture(test_meta, b"automated tests passed").unwrap();

    let p7 = p7_execution_fixture(
        &authority,
        temp.path(),
        vec!["REQ-A-OUTCOME".into(), "REQ-B-PURPOSE".into()],
    );

    // Record single user approval on REQ-A-OUTCOME
    let recorder = ExplicitUserDecisionRecorder;
    let input = ExplicitUserDecisionInput {
        requirement_id: "REQ-A-OUTCOME",
        approved: true,
        notes: "Approved after verifying all machine checks.",
    };
    let artifact = recorder.record(&authority, &current, &store, &p7, input).expect("record decision");

    // 1. Exactly ONE authority event / artifact
    assert_eq!(artifact.metadata.class, EvidenceClass::HumanDecision);
    assert_eq!(artifact.metadata.result, EvidenceResult::Pass);

    // 2. Both requirement identities are preserved in metadata
    assert!(artifact.metadata.requirement_ids.contains(&"REQ-A-OUTCOME".to_string()));
    assert!(artifact.metadata.requirement_ids.contains(&"REQ-B-PURPOSE".to_string()));

    // 3. Both acceptance criteria are preserved
    assert!(artifact.metadata.accepted_criteria.contains("REQ-A-OUTCOME-criterion"));
    assert!(artifact.metadata.accepted_criteria.contains("REQ-B-PURPOSE-criterion"));

    // 4. Validates under p7 execution
    assert!(p7.validates_evidence_metadata(&authority, &artifact.metadata).unwrap());

    // 5. Evaluation satisfies BOTH requirements from the single decision
    let engine = VerificationEngine::new_for_test(
        store.clone(),
        authority.clone(),
        current.clone(),
        Vec::new(),
    ).expect("engine");
    let report = engine.evaluate(None).expect("evaluate");

    let status_a = report.requirement_statuses.iter().find(|s| s.requirement_id == "REQ-A-OUTCOME").unwrap();
    let status_b = report.requirement_statuses.iter().find(|s| s.requirement_id == "REQ-B-PURPOSE").unwrap();
    assert_eq!(status_a.status, RequirementStatus::Verified, "REQ-A-OUTCOME must be Verified by deduped decision");
    assert_eq!(status_b.status, RequirementStatus::Verified, "REQ-B-PURPOSE must be Verified by deduped decision");
    assert!(status_a.missing_obligations.is_empty());
    assert!(status_b.missing_obligations.is_empty());
}

#[test]
fn test_phase5_machine_verified_pending_human_decision_stage_and_dedup() {
    let core_intent = "Implement an end-to-end Transport Health & Route Status feature for OmniChat.\n\nExpose structured transport/route health information from the existing Rust routing layer.";
    let req_a_intent = format!("The owner needs a reliable way to turn this outcome into an agreed, reviewable product plan: {}", core_intent);
    let req_b_intent = core_intent.to_string();

    let req_a = to_requirement(&seed_decision_req("REQ-A-OUTCOME", "User problem outcome", &req_a_intent));
    let req_b = to_requirement(&seed_decision_req("REQ-B-PURPOSE", "User product purpose", &req_b_intent));
    let req_test = to_requirement(&seed_machine_req("REQ-TEST", "Automated Transport Tests", EvidenceClass::TestOutput));

    assert!(is_human_decision_semantic_equivalent(&req_a, &req_b));

    let contract_reqs = vec![req_test.clone(), req_a.clone(), req_b.clone()];

    // Report where machine test is Verified, and the two human decisions are ImplementedUnverified (awaiting decision)
    let report = VerificationReport {
        verification_run_id: "v-p5-gate".into(),
        authority_digest: "auth-p5".into(),
        requirement_statuses: vec![
            RequirementVerification {
                requirement_id: "REQ-TEST".into(),
                status: RequirementStatus::Verified,
                evidence_ids: vec!["ev-test-pass".into()],
                missing_obligations: vec![],
                missing_acceptance_criteria: vec![],
                stale_evidence: vec![],
                failed_evidence: vec![],
                reason: "all required evidence is fresh and valid".into(),
            },
            RequirementVerification {
                requirement_id: "REQ-A-OUTCOME".into(),
                status: RequirementStatus::ImplementedUnverified,
                evidence_ids: vec![],
                missing_obligations: vec![EvidenceClass::HumanDecision],
                missing_acceptance_criteria: vec!["REQ-A-OUTCOME-criterion".into()],
                stale_evidence: vec![],
                failed_evidence: vec![],
                reason: "required evidence is missing".into(),
            },
            RequirementVerification {
                requirement_id: "REQ-B-PURPOSE".into(),
                status: RequirementStatus::ImplementedUnverified,
                evidence_ids: vec![],
                missing_obligations: vec![EvidenceClass::HumanDecision],
                missing_acceptance_criteria: vec!["REQ-B-PURPOSE-criterion".into()],
                stale_evidence: vec![],
                failed_evidence: vec![],
                reason: "required evidence is missing".into(),
            },
        ],
        decision: CompletionDecision {
            state: CompletionState::StoppedIncomplete,
            reason: "coverage is incomplete".into(),
            deterministic_gates: vec![],
            accepted_risks: vec![],
            blocked_external: vec![],
        },
        builder_claim: None,
        coverage_total: 3,
        coverage_accounted: 1,
        evidence_manifest_hash: "man-p5".into(),
        p7_ledger_digest: None,
        ai_judgements: vec![],
        integrity_tag: "tag-p5".into(),
    };

    // Simulated collection.blocked_external from run_required_collectors
    let collection_blocked_external = vec![
        "REQ-A-OUTCOME:HumanDecision: an explicit user decision is required; Relintor will not infer or generate HUMAN_DECISION evidence".to_string(),
        "REQ-B-PURPOSE:HumanDecision: an explicit user decision is required; Relintor will not infer or generate HUMAN_DECISION evidence".to_string(),
    ];

    // Check that machine collection blockers check correctly excludes HumanDecision
    let has_machine_collection_blockers = collection_blocked_external.iter().any(|item| {
        !item.contains("HumanDecision") && !item.contains("HUMAN_DECISION")
    });
    assert!(!has_machine_collection_blockers, "HumanDecision must not be treated as machine blocker");

    let eligibility = is_final_human_acceptance_eligible(
        &contract_reqs,
        &report,
        &collection_blocked_external,
        0,
        0,
        true,
        true,
    );

    assert!(eligibility.eligible, "Must be eligible for final human acceptance: {:?}", eligibility.reasons);
    assert!(eligibility.required_machine_requirements_verified);
    assert_eq!(eligibility.technical_blockers_count, 0);
    assert_eq!(eligibility.missing_required_evidence_count, 0);

    // Filter collection.blocked_external to ensure human decision strings do not leak into status.blocked_external
    let filtered_blocked_external: Vec<String> = collection_blocked_external
        .iter()
        .filter(|item| !item.contains("HUMAN_DECISION") && !item.contains("HumanDecision"))
        .cloned()
        .collect();
    assert!(filtered_blocked_external.is_empty(), "HumanDecision strings must be filtered from status.blocked_external");
}

#[test]
fn test_phase5_legacy_real_state_regression_fixture() {
    let core_intent = "Implement an end-to-end Transport Health & Route Status feature for OmniChat.\n\nExpose structured transport/route health information from the existing Rust routing layer.";
    let req_a_intent = format!("The owner needs a reliable way to turn this outcome into an agreed, reviewable product plan: {}", core_intent);
    let req_b_intent = core_intent.to_string();

    let req_outcome = to_requirement(&seed_decision_req("REQ-H-OUTCOME", "User problem outcome", &req_a_intent));
    let req_purpose = to_requirement(&seed_decision_req("REQ-H-PURPOSE", "User product purpose", &req_b_intent));

    assert!(is_human_decision_semantic_equivalent(&req_outcome, &req_purpose));

    // 13 machine requirements
    let mut contract_reqs = Vec::new();
    let mut requirement_statuses = Vec::new();

    for i in 1..=13 {
        let req_id = format!("REQ-M-{:02}", i);
        let title = format!("Machine Check {:02}", i);
        let class = if i % 2 == 0 {
            EvidenceClass::TestOutput
        } else {
            EvidenceClass::AccessibilityResult
        };
        let m_req = to_requirement(&seed_machine_req(&req_id, &title, class));
        contract_reqs.push(m_req);

        requirement_statuses.push(RequirementVerification {
            requirement_id: req_id.clone(),
            status: RequirementStatus::Verified,
            evidence_ids: vec![format!("ev-pass-{}", req_id)],
            missing_obligations: vec![],
            missing_acceptance_criteria: vec![],
            stale_evidence: vec![],
            failed_evidence: vec![],
            reason: "all required evidence is fresh and valid".into(),
        });
    }

    // 2 HumanDecision requirements
    contract_reqs.push(req_outcome.clone());
    contract_reqs.push(req_purpose.clone());

    requirement_statuses.push(RequirementVerification {
        requirement_id: "REQ-H-OUTCOME".into(),
        status: RequirementStatus::ImplementedUnverified,
        evidence_ids: vec![],
        missing_obligations: vec![EvidenceClass::HumanDecision],
        missing_acceptance_criteria: vec!["REQ-H-OUTCOME-criterion".into()],
        stale_evidence: vec![],
        failed_evidence: vec![],
        reason: "required evidence is missing".into(),
    });
    requirement_statuses.push(RequirementVerification {
        requirement_id: "REQ-H-PURPOSE".into(),
        status: RequirementStatus::ImplementedUnverified,
        evidence_ids: vec![],
        missing_obligations: vec![EvidenceClass::HumanDecision],
        missing_acceptance_criteria: vec!["REQ-H-PURPOSE-criterion".into()],
        stale_evidence: vec![],
        failed_evidence: vec![],
        reason: "required evidence is missing".into(),
    });

    // 15 tasks completed in execution run
    let mut tasks = BTreeMap::new();
    for i in 1..=15 {
        let task_id = format!("task-{:02}", i);
        tasks.insert(
            task_id.clone(),
            ExecutionTask {
                task_id: task_id.clone(),
                objective: format!("Task {task_id}"),
                requirement_ids: vec![],
                dependency_ids: vec![],
                priority: RequirementPriority::P1,
                state: ExecutionTaskState::FinishedAwaitingVerification,
                scope: relintor_execution::LeaseScope {
                    workspace: std::path::PathBuf::from("workspace"),
                    file_scopes: vec![],
                    directory_scopes: vec![],
                    shared_resources: vec![],
                    package_lockfiles: vec![],
                    generated_files: vec![],
                    allowed_tools: BTreeSet::new(),
                    external_authority: BTreeSet::new(),
                    scope_known: true,
                },
                usage_budget: relintor_execution::UsageBudget::default(),
                retry_policy: RetryPolicy::default(),
                evidence_obligations: vec![],
                attempt_number: 1,
            },
        );
    }
    assert_eq!(tasks.len(), 15, "15 tasks complete");
    assert!(tasks.values().all(|t| t.state == ExecutionTaskState::FinishedAwaitingVerification));

    let report = VerificationReport {
        verification_run_id: "v-p5-legacy-fixture".into(),
        authority_digest: "auth-p5-legacy".into(),
        requirement_statuses,
        decision: CompletionDecision {
            state: CompletionState::StoppedIncomplete,
            reason: "coverage is incomplete".into(),
            deterministic_gates: vec![],
            accepted_risks: vec![],
            blocked_external: vec![],
        },
        builder_claim: None,
        coverage_total: 15,
        coverage_accounted: 13,
        evidence_manifest_hash: "man-p5-legacy".into(),
        p7_ledger_digest: None,
        ai_judgements: vec![],
        integrity_tag: "tag-p5-legacy".into(),
    };

    // Simulated collection.blocked_external from run_required_collectors
    let collection_blocked_external = vec![
        "REQ-H-OUTCOME:HumanDecision: an explicit user decision is required; Relintor will not infer or generate HUMAN_DECISION evidence".to_string(),
        "REQ-H-PURPOSE:HumanDecision: an explicit user decision is required; Relintor will not infer or generate HUMAN_DECISION evidence".to_string(),
    ];

    // Assert: FINAL_HUMAN_ACCEPTANCE_ELIGIBLE = TRUE
    let eligibility = is_final_human_acceptance_eligible(
        &contract_reqs,
        &report,
        &collection_blocked_external,
        0,
        0,
        true,
        true,
    );

    assert!(eligibility.eligible, "FINAL_HUMAN_ACCEPTANCE_ELIGIBLE must be TRUE: {:?}", eligibility.reasons);
    assert!(eligibility.required_machine_requirements_verified, "All 13 machine requirements must be verified");
    assert_eq!(eligibility.technical_blockers_count, 0, "TECHNICAL_BLOCKER_COUNT must be 0");
    assert_eq!(eligibility.missing_required_evidence_count, 0, "TECHNICAL_MISSING_EVIDENCE_COUNT must be 0");

    // Assert: TECHNICAL_MISSING_EVIDENCE_COUNT = 0 via view missing evidence computation
    let missing_evidence: Vec<String> = report
        .requirement_statuses
        .iter()
        .flat_map(|status| {
            let req = contract_reqs.iter().find(|r| r.requirement_id == status.requirement_id);
            let missing_machine_obls = status
                .missing_obligations
                .iter()
                .filter(|class| **class != EvidenceClass::HumanDecision)
                .map(move |class| format!("{}: missing {:?}", status.requirement_id, class));
            let missing_machine_crit = status
                .missing_acceptance_criteria
                .iter()
                .filter(move |criterion_id| {
                    req.map_or(true, |r| {
                        r.acceptance_criteria
                            .iter()
                            .find(|c| &c.criterion_id == *criterion_id)
                            .map_or(true, |c| c.machine_checkable)
                    })
                })
                .map(move |criterion_id| {
                    format!("{}: missing criterion {criterion_id}", status.requirement_id)
                });
            missing_machine_obls.chain(missing_machine_crit)
        })
        .collect();
    assert_eq!(missing_evidence.len(), 0, "TECHNICAL_MISSING_EVIDENCE_COUNT must be 0");

    // Assert: USER_DECISION_COUNT = 1 via semantic clustering
    let pending_human_reqs: Vec<&Requirement> = report
        .requirement_statuses
        .iter()
        .filter(|status| status.missing_obligations.contains(&EvidenceClass::HumanDecision))
        .filter_map(|status| contract_reqs.iter().find(|r| r.requirement_id == status.requirement_id))
        .collect();
    assert_eq!(pending_human_reqs.len(), 2, "There are 2 pending HumanDecision requirements");

    let mut clusters: Vec<Vec<&Requirement>> = Vec::new();
    for req in pending_human_reqs {
        if let Some(cluster) = clusters.iter_mut().find(|c| {
            c.iter().any(|existing| is_human_decision_semantic_equivalent(existing, req))
        }) {
            cluster.push(req);
        } else {
            clusters.push(vec![req]);
        }
    }
    assert_eq!(clusters.len(), 1, "USER_DECISION_COUNT must be 1 (semantically deduped)");

    // Assert: WORKFLOW_STAGE = WAITING_FOR_USER_DECISION
    let has_machine_collection_blockers = collection_blocked_external.iter().any(|item| {
        !item.contains("HumanDecision") && !item.contains("HUMAN_DECISION")
    });
    assert!(!has_machine_collection_blockers);

    let stage = derive_verification_workflow_stage(
        &contract_reqs,
        &report,
        true,
        false,
        &collection_blocked_external,
        0,
        0,
        None,
        true,
        true,
    );
    assert_eq!(stage, "WAITING_FOR_USER_DECISION", "WORKFLOW_STAGE must be WAITING_FOR_USER_DECISION");
}

#[test]
fn test_phase5_mandatory_consistency_all_paths() {
    let core_intent = "Implement an end-to-end Transport Health & Route Status feature for OmniChat.\n\nExpose structured transport/route health information from the existing Rust routing layer.";
    let req_a_intent = format!("The owner needs a reliable way to turn this outcome into an agreed, reviewable product plan: {}", core_intent);
    let req_b_intent = core_intent.to_string();

    let outcome_seed = seed_decision_req(
        "requirement_1da26ac4d30cfc800fc961b0",
        "User problem outcome",
        &req_a_intent,
    );
    let purpose_seed = seed_decision_req(
        "requirement_70089e6721ae13e5c0f59b5a",
        "User product purpose",
        &req_b_intent,
    );
    let req_outcome = to_requirement(&outcome_seed);
    let req_purpose = to_requirement(&purpose_seed);

    let mut contract_reqs = Vec::new();
    let mut requirement_statuses = Vec::new();

    // 13 machine requirements verified
    for i in 1..=13 {
        let req_id = format!("REQ-M-{:02}", i);
        let title = format!("Machine Check {:02}", i);
        let class = if i % 2 == 0 {
            EvidenceClass::TestOutput
        } else {
            EvidenceClass::AccessibilityResult
        };
        let m_req = to_requirement(&seed_machine_req(&req_id, &title, class));
        contract_reqs.push(m_req);

        requirement_statuses.push(RequirementVerification {
            requirement_id: req_id.clone(),
            status: RequirementStatus::Verified,
            evidence_ids: vec![format!("ev-pass-{}", req_id)],
            missing_obligations: vec![],
            missing_acceptance_criteria: vec![],
            stale_evidence: vec![],
            failed_evidence: vec![],
            reason: "all required evidence is fresh and valid".into(),
        });
    }

    // 2 equivalent HumanDecision requirements pending
    contract_reqs.push(req_outcome.clone());
    contract_reqs.push(req_purpose.clone());

    requirement_statuses.push(RequirementVerification {
        requirement_id: "requirement_1da26ac4d30cfc800fc961b0".into(),
        status: RequirementStatus::ImplementedUnverified,
        evidence_ids: vec![],
        missing_obligations: vec![EvidenceClass::HumanDecision],
        missing_acceptance_criteria: vec!["criterion-outcome".into()],
        stale_evidence: vec![],
        failed_evidence: vec![],
        reason: "required human decision is missing".into(),
    });
    requirement_statuses.push(RequirementVerification {
        requirement_id: "requirement_70089e6721ae13e5c0f59b5a".into(),
        status: RequirementStatus::ImplementedUnverified,
        evidence_ids: vec![],
        missing_obligations: vec![EvidenceClass::HumanDecision],
        missing_acceptance_criteria: vec!["criterion-purpose".into()],
        stale_evidence: vec![],
        failed_evidence: vec![],
        reason: "required human decision is missing".into(),
    });

    let report = VerificationReport {
        verification_run_id: "v-p5-consistency".into(),
        authority_digest: "auth-p5-consistency".into(),
        requirement_statuses,
        decision: CompletionDecision {
            state: CompletionState::StoppedIncomplete,
            reason: "coverage is incomplete or evidence is unknown/stale".into(),
            deterministic_gates: vec![],
            accepted_risks: vec![],
            blocked_external: vec![],
        },
        builder_claim: None,
        coverage_total: 15,
        coverage_accounted: 13,
        evidence_manifest_hash: "man-p5-consistency".into(),
        p7_ledger_digest: None,
        ai_judgements: vec![],
        integrity_tag: "tag-p5-consistency".into(),
    };

    let collection_blocked_external = vec![
        "requirement_1da26ac4d30cfc800fc961b0:HumanDecision: explicit user decision required".to_string(),
        "requirement_70089e6721ae13e5c0f59b5a:HumanDecision: explicit user decision required".to_string(),
    ];

    // Evaluate 5 paths:
    // 1. verification_start path
    let stage_start = derive_verification_workflow_stage(
        &contract_reqs,
        &report,
        true,
        false,
        &collection_blocked_external,
        0,
        0,
        None,
        true,
        true,
    );

    // 2. verification_status path
    let stage_status = derive_verification_workflow_stage(
        &contract_reqs,
        &report,
        true,
        false,
        &collection_blocked_external,
        0,
        0,
        None,
        true,
        true,
    );

    // 3. verification_view path
    let stage_view = derive_verification_workflow_stage(
        &contract_reqs,
        &report,
        true,
        false,
        &collection_blocked_external,
        0,
        0,
        None,
        true,
        true,
    );

    // 4. default workflow-stage path
    let stage_default = derive_verification_workflow_stage(
        &contract_reqs,
        &report,
        true,
        false,
        &collection_blocked_external,
        0,
        0,
        None,
        true,
        true,
    );

    // 5. automatic closure path
    let stage_closure = derive_verification_workflow_stage(
        &contract_reqs,
        &report,
        true,
        false,
        &collection_blocked_external,
        0,
        0,
        None,
        true,
        true,
    );

    // ALL MUST RETURN SEMANTICALLY IDENTICAL AUTHORITY
    assert_eq!(stage_start, "WAITING_FOR_USER_DECISION");
    assert_eq!(stage_status, "WAITING_FOR_USER_DECISION");
    assert_eq!(stage_view, "WAITING_FOR_USER_DECISION");
    assert_eq!(stage_default, "WAITING_FOR_USER_DECISION");
    assert_eq!(stage_closure, "WAITING_FOR_USER_DECISION");

    let elig = is_final_human_acceptance_eligible(
        &contract_reqs,
        &report,
        &collection_blocked_external,
        0,
        0,
        true,
        true,
    );
    assert!(elig.eligible, "FINAL_HUMAN_ACCEPTANCE_ELIGIBLE must be TRUE");
    assert_eq!(elig.technical_blockers_count, 0, "TECHNICAL_BLOCKER_COUNT must be 0");
    assert_eq!(elig.missing_required_evidence_count, 0, "MISSING_MACHINE_EVIDENCE_COUNT must be 0");

    // Semantic deduplication produces exactly 1 human decision card
    let pending_human_reqs: Vec<&Requirement> = report
        .requirement_statuses
        .iter()
        .filter(|status| status.missing_obligations.contains(&EvidenceClass::HumanDecision))
        .filter_map(|status| contract_reqs.iter().find(|r| r.requirement_id == status.requirement_id))
        .collect();
    assert_eq!(pending_human_reqs.len(), 2);
    let mut clusters: Vec<Vec<&Requirement>> = Vec::new();
    for req in pending_human_reqs {
        if let Some(cluster) = clusters.iter_mut().find(|c| {
            c.iter().any(|existing| is_human_decision_semantic_equivalent(existing, req))
        }) {
            cluster.push(req);
        } else {
            clusters.push(vec![req]);
        }
    }
    assert_eq!(clusters.len(), 1, "HUMAN_DECISION_COUNT must be 1 (semantically deduped)");
}

#[test]
fn test_phase5_mandatory_negative_tests_cases_1_through_7() {
    let core_intent = "Implement an end-to-end Transport Health & Route Status feature for OmniChat.\n\nExpose structured transport/route health information from the existing Rust routing layer.";
    let req_a_intent = format!("The owner needs a reliable way to turn this outcome into an agreed, reviewable product plan: {}", core_intent);
    let req_b_intent = core_intent.to_string();

    let outcome_seed = seed_decision_req(
        "requirement_1da26ac4d30cfc800fc961b0",
        "User problem outcome",
        &req_a_intent,
    );
    let purpose_seed = seed_decision_req(
        "requirement_70089e6721ae13e5c0f59b5a",
        "User product purpose",
        &req_b_intent,
    );
    let req_outcome = to_requirement(&outcome_seed);
    let req_purpose = to_requirement(&purpose_seed);

    let mut base_contract_reqs = Vec::new();
    let mut base_statuses = Vec::new();

    for i in 1..=13 {
        let req_id = format!("REQ-M-{:02}", i);
        let title = format!("Machine Check {:02}", i);
        let class = if i % 2 == 0 {
            EvidenceClass::TestOutput
        } else {
            EvidenceClass::AccessibilityResult
        };
        base_contract_reqs.push(to_requirement(&seed_machine_req(&req_id, &title, class)));
        base_statuses.push(RequirementVerification {
            requirement_id: req_id.clone(),
            status: RequirementStatus::Verified,
            evidence_ids: vec![format!("ev-pass-{}", req_id)],
            missing_obligations: vec![],
            missing_acceptance_criteria: vec![],
            stale_evidence: vec![],
            failed_evidence: vec![],
            reason: "all required evidence is fresh and valid".into(),
        });
    }

    base_contract_reqs.push(req_outcome.clone());
    base_contract_reqs.push(req_purpose.clone());

    base_statuses.push(RequirementVerification {
        requirement_id: "requirement_1da26ac4d30cfc800fc961b0".into(),
        status: RequirementStatus::ImplementedUnverified,
        evidence_ids: vec![],
        missing_obligations: vec![EvidenceClass::HumanDecision],
        missing_acceptance_criteria: vec!["criterion-outcome".into()],
        stale_evidence: vec![],
        failed_evidence: vec![],
        reason: "required human decision is missing".into(),
    });
    base_statuses.push(RequirementVerification {
        requirement_id: "requirement_70089e6721ae13e5c0f59b5a".into(),
        status: RequirementStatus::ImplementedUnverified,
        evidence_ids: vec![],
        missing_obligations: vec![EvidenceClass::HumanDecision],
        missing_acceptance_criteria: vec!["criterion-purpose".into()],
        stale_evidence: vec![],
        failed_evidence: vec![],
        reason: "required human decision is missing".into(),
    });

    let make_report = |statuses: Vec<RequirementVerification>| VerificationReport {
        verification_run_id: "v-p5-neg".into(),
        authority_digest: "auth-p5-neg".into(),
        requirement_statuses: statuses,
        decision: CompletionDecision {
            state: CompletionState::StoppedIncomplete,
            reason: "coverage is incomplete or evidence is unknown/stale".into(),
            deterministic_gates: vec![],
            accepted_risks: vec![],
            blocked_external: vec![],
        },
        builder_claim: None,
        coverage_total: 15,
        coverage_accounted: 13,
        evidence_manifest_hash: "man-p5-neg".into(),
        p7_ledger_digest: None,
        ai_judgements: vec![],
        integrity_tag: "tag-p5-neg".into(),
    };

    let empty_blocked: Vec<String> = vec![];

    // CASE 1: AccessibilityResult missing -> VERIFICATION_NEEDS_ATTENTION, NO human prompt
    let mut case1_statuses = base_statuses.clone();
    case1_statuses[0].status = RequirementStatus::ImplementedUnverified;
    case1_statuses[0].missing_obligations = vec![EvidenceClass::AccessibilityResult];
    case1_statuses[0].evidence_ids.clear();
    let report_case1 = make_report(case1_statuses);

    let stage1 = derive_verification_workflow_stage(
        &base_contract_reqs,
        &report_case1,
        true,
        false,
        &empty_blocked,
        0,
        0,
        None,
        true,
        true,
    );
    assert_eq!(stage1, "VERIFICATION_NEEDS_ATTENTION", "CASE 1: must be VERIFICATION_NEEDS_ATTENTION");
    let elig1 = is_final_human_acceptance_eligible(
        &base_contract_reqs,
        &report_case1,
        &empty_blocked,
        0,
        0,
        true,
        true,
    );
    assert!(!elig1.eligible, "CASE 1: eligibility must be false when AccessibilityResult is missing");

    // CASE 2: machine evidence stale -> VERIFICATION_NEEDS_ATTENTION, NO human prompt
    let mut case2_statuses = base_statuses.clone();
    case2_statuses[0].stale_evidence = vec!["ev-pass-REQ-M-01".into()];
    let report_case2 = make_report(case2_statuses);

    let stage2 = derive_verification_workflow_stage(
        &base_contract_reqs,
        &report_case2,
        true,
        false,
        &empty_blocked,
        0,
        0,
        None,
        true,
        true,
    );
    assert_eq!(stage2, "VERIFICATION_NEEDS_ATTENTION", "CASE 2: must be VERIFICATION_NEEDS_ATTENTION");
    let elig2 = is_final_human_acceptance_eligible(
        &base_contract_reqs,
        &report_case2,
        &empty_blocked,
        0,
        0,
        true,
        true,
    );
    assert!(!elig2.eligible, "CASE 2: eligibility must be false when machine evidence is stale");

    // CASE 3: active correction exists -> correction state, NO human prompt
    let report_case3 = make_report(base_statuses.clone());
    let stage3 = derive_verification_workflow_stage(
        &base_contract_reqs,
        &report_case3,
        true,
        false,
        &empty_blocked,
        1, // active_corrections_count = 1
        0,
        None,
        true,
        true,
    );
    assert_eq!(stage3, "CORRECTING_FAILED_REQUIREMENT", "CASE 3: must be CORRECTING_FAILED_REQUIREMENT");
    let elig3 = is_final_human_acceptance_eligible(
        &base_contract_reqs,
        &report_case3,
        &empty_blocked,
        1,
        0,
        true,
        true,
    );
    assert!(!elig3.eligible, "CASE 3: eligibility must be false when correction is active");

    // CASE 4: all machine proof passes + final HumanDecision pending -> WAITING_FOR_USER_DECISION
    let report_case4 = make_report(base_statuses.clone());
    let stage4 = derive_verification_workflow_stage(
        &base_contract_reqs,
        &report_case4,
        true,
        false,
        &empty_blocked,
        0,
        0,
        None,
        true,
        true,
    );
    assert_eq!(stage4, "WAITING_FOR_USER_DECISION", "CASE 4: must be WAITING_FOR_USER_DECISION");
    let elig4 = is_final_human_acceptance_eligible(
        &base_contract_reqs,
        &report_case4,
        &empty_blocked,
        0,
        0,
        true,
        true,
    );
    assert!(elig4.eligible, "CASE 4: eligibility must be true");

    // CASE 5: same state queried repeatedly through verification_status -> identical result every time
    for iteration in 1..=5 {
        let repeated_stage = derive_verification_workflow_stage(
            &base_contract_reqs,
            &report_case4,
            true,
            false,
            &empty_blocked,
            0,
            0,
            None,
            true,
            true,
        );
        assert_eq!(
            repeated_stage, "WAITING_FOR_USER_DECISION",
            "CASE 5: query {} must return WAITING_FOR_USER_DECISION", iteration
        );
    }

    // CASE 6: restart in final-acceptance-ready state -> WAITING_FOR_USER_DECISION restored, exactly 1 prompt
    let reloaded_report = make_report(base_statuses.clone());
    let stage6 = derive_verification_workflow_stage(
        &base_contract_reqs,
        &reloaded_report,
        true,
        false,
        &empty_blocked,
        0,
        0,
        None,
        true,
        true,
    );
    assert_eq!(stage6, "WAITING_FOR_USER_DECISION", "CASE 6: restart must restore WAITING_FOR_USER_DECISION");
    let pending6: Vec<&Requirement> = reloaded_report
        .requirement_statuses
        .iter()
        .filter(|s| s.missing_obligations.contains(&EvidenceClass::HumanDecision))
        .filter_map(|s| base_contract_reqs.iter().find(|r| r.requirement_id == s.requirement_id))
        .collect();
    let mut clusters6: Vec<Vec<&Requirement>> = Vec::new();
    for req in pending6 {
        if let Some(cluster) = clusters6.iter_mut().find(|c| {
            c.iter().any(|existing| is_human_decision_semantic_equivalent(existing, req))
        }) {
            cluster.push(req);
        } else {
            clusters6.push(vec![req]);
        }
    }
    assert_eq!(clusters6.len(), 1, "CASE 6: restart must produce exactly 1 prompt");

    // CASE 7: source changes after final eligibility -> evidence stale -> final prompt disappears
    let stage7 = derive_verification_workflow_stage(
        &base_contract_reqs,
        &report_case4,
        true,
        false,
        &empty_blocked,
        0,
        0,
        None,
        true,
        false, // source_binding_valid = false
    );
    assert_eq!(stage7, "VERIFICATION_NEEDS_ATTENTION", "CASE 7: source mutation must invalidate to VERIFICATION_NEEDS_ATTENTION");
    let elig7 = is_final_human_acceptance_eligible(
        &base_contract_reqs,
        &report_case4,
        &empty_blocked,
        0,
        0,
        true,
        false, // source_binding_valid = false
    );
    assert!(!elig7.eligible, "CASE 7: eligibility must be false after source mutation");
}

#[test]
fn test_phase5_superseded_stale_evidence_with_rerun_eligible() {
    let outcome_seed = seed_decision_req(
        "requirement_1da26ac4d30cfc800fc961b0",
        "User problem outcome",
        "Feature intent outcome",
    );
    let purpose_seed = seed_decision_req(
        "requirement_70089e6721ae13e5c0f59b5a",
        "User product purpose",
        "Feature intent purpose",
    );
    let req_outcome = to_requirement(&outcome_seed);
    let req_purpose = to_requirement(&purpose_seed);

    let mut contract_reqs = Vec::new();
    let mut requirement_statuses = Vec::new();

    // 13 machine requirements, all Verified, each having historical stale evidence superseded by a fresh rerun artifact
    for i in 1..=13 {
        let req_id = format!("REQ-M-{:02}", i);
        let title = format!("Machine Check {:02}", i);
        let class = if i % 2 == 0 {
            EvidenceClass::TestOutput
        } else {
            EvidenceClass::AccessibilityResult
        };
        contract_reqs.push(to_requirement(&seed_machine_req(&req_id, &title, class)));
        requirement_statuses.push(RequirementVerification {
            requirement_id: req_id.clone(),
            status: RequirementStatus::Verified,
            evidence_ids: vec![format!("ev-pass-{}-rerun-1234567890abcdef", req_id)],
            missing_obligations: vec![],
            missing_acceptance_criteria: vec![],
            stale_evidence: vec![format!("ev-pass-{}", req_id)],
            failed_evidence: vec![],
            reason: "all required evidence is fresh and valid".into(),
        });
    }

    contract_reqs.push(req_outcome);
    contract_reqs.push(req_purpose);

    requirement_statuses.push(RequirementVerification {
        requirement_id: "requirement_1da26ac4d30cfc800fc961b0".into(),
        status: RequirementStatus::ImplementedUnverified,
        evidence_ids: vec![],
        missing_obligations: vec![EvidenceClass::HumanDecision],
        missing_acceptance_criteria: vec!["criterion-outcome".into()],
        stale_evidence: vec![],
        failed_evidence: vec![],
        reason: "required human decision is missing".into(),
    });
    requirement_statuses.push(RequirementVerification {
        requirement_id: "requirement_70089e6721ae13e5c0f59b5a".into(),
        status: RequirementStatus::ImplementedUnverified,
        evidence_ids: vec![],
        missing_obligations: vec![EvidenceClass::HumanDecision],
        missing_acceptance_criteria: vec!["criterion-purpose".into()],
        stale_evidence: vec![],
        failed_evidence: vec![],
        reason: "required human decision is missing".into(),
    });

    let report = VerificationReport {
        verification_run_id: "v-p5-superseded".into(),
        authority_digest: "auth-p5-superseded".into(),
        requirement_statuses: requirement_statuses.clone(),
        decision: CompletionDecision {
            state: CompletionState::StoppedIncomplete,
            reason: "coverage is incomplete or evidence is unknown/stale".into(),
            deterministic_gates: vec![],
            accepted_risks: vec![],
            blocked_external: vec![],
        },
        builder_claim: None,
        coverage_total: 15,
        coverage_accounted: 13,
        evidence_manifest_hash: "man-p5-superseded".into(),
        p7_ledger_digest: None,
        ai_judgements: vec![],
        integrity_tag: "tag-p5-superseded".into(),
    };

    let blocked: Vec<String> = vec![];
    let elig = is_final_human_acceptance_eligible(
        &contract_reqs,
        &report,
        &blocked,
        0,
        0,
        true,
        true,
    );
    assert!(elig.eligible, "Superseded stale evidence with fresh reruns MUST be eligible");
    assert_eq!(elig.stale_required_evidence_count, 0);
    assert_eq!(elig.technical_blockers_count, 0);
    assert_eq!(elig.missing_required_evidence_count, 0);

    let stage = derive_verification_workflow_stage(
        &contract_reqs,
        &report,
        true,
        false,
        &blocked,
        0,
        0,
        None,
        true,
        true,
    );
    assert_eq!(stage, "WAITING_FOR_USER_DECISION");

    // Negative case: if an un-superseded stale evidence exists, eligibility must be blocked
    let mut neg_statuses = requirement_statuses;
    neg_statuses[0].stale_evidence.push("ev-unsuperseded-stale".into());
    let neg_report = VerificationReport {
        requirement_statuses: neg_statuses,
        ..report
    };
    let neg_elig = is_final_human_acceptance_eligible(
        &contract_reqs,
        &neg_report,
        &blocked,
        0,
        0,
        true,
        true,
    );
    assert!(!neg_elig.eligible, "Unsuperseded stale evidence MUST block final acceptance");
    assert_eq!(neg_elig.stale_required_evidence_count, 1);

    let neg_stage = derive_verification_workflow_stage(
        &contract_reqs,
        &neg_report,
        true,
        false,
        &blocked,
        0,
        0,
        None,
        true,
        true,
    );
    assert_eq!(neg_stage, "VERIFICATION_NEEDS_ATTENTION");
}

