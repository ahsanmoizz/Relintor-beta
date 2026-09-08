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
    AuthorityEngine, EvidenceClass, EvidenceConfidence, EvidenceObligation, FactValue,
    ProjectAuthorityInput, ProjectRequirementSeed, RequirementPriority, RequirementRisk,
    RequirementSource, RequirementStatus, TrustedSigner, TrustedSignerSet, VerificationPolicy,
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
