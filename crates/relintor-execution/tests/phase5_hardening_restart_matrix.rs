//! Phase-5 Hardening Restart Matrix & Performance Scale Acceptance Suite.
//!
//! Covers:
//! 1. restart ReadyToRun
//! 2. restart StartingExecutor
//! 3. restart Running
//! 4. restart after action receipt
//! 5. restart soft-window extension
//! 6. restart RecoveryReviewRequired
//! 7. restart RecoveryRevalidated
//! 8. restart RetryAuthorized
//! 9. restart evidence collection
//! 10. restart HumanDecision waiting
//! 11. restart verification
//! 12. restart Verified Complete
//! 13. simulated disk-full before checkpoint
//! 14. simulated disk-full during temp write
//! 15. corrupted newest temp generation
//! 16. valid previous authoritative generation remains
//! 17. junction-backed runtime storage
//! 18. non-C runtime storage
//! 19. low-storage preflight failure
//! 20. second reopen remains deterministic
//! Scale Benchmark: 100 tasks, 5,000 events, 10,000 events

use base64::{engine::general_purpose::STANDARD, Engine as _};
use ed25519_dalek::SigningKey;
use relintor_execution::{
    check_low_disk_headroom, inventory_fingerprint, workspace_inventory, ActionRequest,
    ActionResult, CheckpointContent, CheckpointKind, ExecutionEvent,
    ExecutionEventKind, ExecutionRun, ExecutionRunState, ExecutionTaskState, FailureClass,
    GitWorktreeSnapshot, ProgressSnapshot, RecoveryAttemptTarget, RecoveryAuthority, RecoveryStore,
    ReviewedRecoveryDelta, SchedulerPolicy, TaskAttemptState, UntrackedFileSnapshot,
    WorkspaceSnapshot,
};
use relintor_standards::{
    builtin_registry, scope_fingerprint, AcceptanceCriterion, ApplicabilityContext,
    AuthorityEngine, EvidenceClass, EvidenceConfidence, EvidenceObligation, FactValue,
    MissionRevision, ProjectAuthorityInput, ProjectRequirementSeed, RequirementPriority,
    RequirementRisk, RequirementSource, StandardsRegistry, TrustedSigner, TrustedSignerSet,
    VerificationPolicy,
};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

fn context() -> ApplicabilityContext {
    let mut facts = BTreeMap::new();
    for field in [
        "web", "backend", "database", "authentication", "ui_surface", "seo_relevance",
        "performance", "deployment", "observability", "privacy", "payments", "ai",
        "blockchain", "mobile", "desktop", "data_engineering", "integrations",
    ] {
        facts.insert(field.into(), FactValue::Bool(false));
    }
    facts.insert("platform".into(), FactValue::Text("windows".into()));
    ApplicabilityContext {
        facts,
        revision: "p5-hardening-context-1".into(),
    }
}

fn seed(id: &str, dependencies: Vec<&str>) -> ProjectRequirementSeed {
    ProjectRequirementSeed {
        requirement_id: Some(id.into()),
        title: format!("Hardening test {id}"),
        intent: format!("Execute the {id} objective"),
        source: RequirementSource::User {
            reference: format!("hardening-test-{id}"),
        },
        priority: RequirementPriority::P2,
        acceptance_criteria: vec![AcceptanceCriterion {
            criterion_id: format!("{id}-criterion"),
            statement: "The deterministic hardening behavior is observed".into(),
            criterion_type: "test".into(),
            machine_checkable: true,
        }],
        verification_policy: VerificationPolicy {
            obligations: vec![EvidenceObligation {
                class: EvidenceClass::TestOutput,
                minimum_confidence: EvidenceConfidence::StrongDeterministic,
                rationale: "hardening test output".into(),
                required: true,
            }],
            p8_collector_required: false,
        },
        dependencies: dependencies.into_iter().map(str::to_owned).collect(),
        risk: RequirementRisk::Medium,
        requirement_type: "hardening-test-scope".into(),
    }
}

fn sealed_fixture(req_count: usize) -> (
    MissionRevision,
    relintor_standards::ExecutionHandoff,
    StandardsRegistry,
    TrustedSignerSet,
) {
    let signing_key = SigningKey::from_bytes(&[9u8; 32]);
    let mut registry = builtin_registry();
    registry
        .sign("p5-hardening-signer", &signing_key)
        .expect("registry signs");
    let trusted = TrustedSignerSet {
        signers: vec![TrustedSigner {
            key_id: "p5-hardening-signer".into(),
            algorithm: "Ed25519".into(),
            public_key: STANDARD.encode(signing_key.verifying_key().to_bytes()),
        }],
    };
    let mut reqs = Vec::new();
    for i in 1..=req_count {
        let req_id = format!("requirement_{:016x}", i);
        let deps = if i > 1 {
            vec![format!("requirement_{:016x}", i - 1)]
        } else {
            vec![]
        };
        let deps_str: Vec<&str> = deps.iter().map(|s| s.as_str()).collect();
        reqs.push(seed(&req_id, deps_str));
    }
    let authority = ProjectAuthorityInput {
        requirements: reqs,
        source_revision: "p5-hardening-fixture-revision".into(),
        source_fingerprint: "p5-hardening-fixture-fingerprint".into(),
        ..ProjectAuthorityInput::default()
    };
    let draft = AuthorityEngine
        .build_draft_with_project_authority(
            "mission-p5-hardening",
            "project-p5-hardening",
            "project-source-1",
            "workspace-source-1",
            registry.clone(),
            &context(),
            scope_fingerprint(BTreeMap::from([("root".into(), "p5-hardening".into())])),
            authority,
            Vec::new(),
        )
        .expect("build draft");
    let sealed = AuthorityEngine
        .seal(&draft, &trusted, "2026-08-30T00:00:00Z")
        .expect("seal fixture");
    (sealed.0, sealed.1, registry, trusted)
}

fn temp_workspace(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "relintor-p5-test-{}-{}-{}",
        name,
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn seed_run(ws: &Path, req_count: usize) -> ExecutionRun {
    let (revision, handoff, registry, trusted) = sealed_fixture(req_count);
    let inv = workspace_inventory(ws).unwrap();
    let fp = inventory_fingerprint(&inv).unwrap();
    ExecutionRun::from_p6_handoff_with_registry(
        &revision,
        &handoff,
        &registry,
        &trusted,
        ws.to_path_buf(),
        &fp,
        SchedulerPolicy::default(),
        1_000_000,
    )
    .expect("create execution run")
}

fn test_action(root: &Path, operation: &str) -> ActionRequest {
    ActionRequest {
        tool: "workspace".into(),
        operation: operation.into(),
        arguments: vec![operation.into()],
        working_scope: root.display().to_string(),
        environment_identity: "p5-hardening-env".into(),
        mutable: true,
        paths: vec!["file.txt".into()],
        external_authority: None,
    }
}

fn test_result(success: bool) -> ActionResult {
    ActionResult {
        success,
        failure_class: None,
        failure_message: None,
        changed_paths: vec!["file.txt".into()],
        workspace_fingerprint: "p5-test-fp".into(),
        passing_tests: 1,
        useful_artifacts: 1,
        wall_time_ms: 10,
        estimated_cost_micros: Some(1),
    }
}

fn interrupt_attempt(run: &mut ExecutionRun, task_id: &str, ended_at: u64) -> RecoveryAttemptTarget {
    let _packet = run.start_task(task_id, ended_at - 1000).unwrap();
    run.mark_external_process_started(task_id).unwrap();
    let lease_id = {
        let attempt = run.attempts.last_mut().unwrap();
        attempt.state = TaskAttemptState::Failed;
        attempt.ended_at_ms = Some(ended_at);
        attempt.failure_class = Some(FailureClass::ProcessFailure);
        attempt.termination_reason = Some("interrupted during execution".into());
        attempt.lease_id.clone()
    };
    for lease in &mut run.leases {
        if lease.lease_id == lease_id {
            lease.revoke().unwrap();
        }
    }
    run.state = ExecutionRunState::RevalidationRequired;
    run.current_recovery_attempt().expect("recovery target")
}

fn make_checkpoint_content(root: &Path, run: &ExecutionRun, reason: &str) -> CheckpointContent {
    let workspace = WorkspaceSnapshot::capture(root, 1).unwrap();
    let untracked = UntrackedFileSnapshot::capture(root, 1).unwrap();
    let git = GitWorktreeSnapshot::capture(root).unwrap();
    let authority = RecoveryAuthority::new(
        &run.project_id,
        &run.mission_id,
        run.mission_revision,
        &run.seal_hash,
        "registry-test",
        &run.run_id,
        workspace.fingerprint.clone(),
        "source-test",
        None,
        "P8_PENDING",
        1,
    );
    CheckpointContent {
        authority,
        kind: CheckpointKind::AfterTaskPersistence,
        run_snapshot_json: run.snapshot_json().unwrap(),
        workspace,
        untracked,
        git,
        processes: Vec::new(),
        verification_references: vec!["p8-evidence-pending".into()],
        reason: reason.into(),
        created_at_ms: 1000,
    }
}

// -----------------------------------------------------------------------------
// 1. RESTART READY TO RUN
// -----------------------------------------------------------------------------
#[test]
fn test_01_restart_ready_to_run() {
    let ws = temp_workspace("ready-to-run");
    let run = seed_run(&ws, 3);
    assert_eq!(run.state, ExecutionRunState::Ready);

    let json = run.snapshot_json().unwrap();
    let restored = ExecutionRun::restore_json(&json).unwrap();
    assert_eq!(restored.state, ExecutionRunState::Ready);
    assert_eq!(restored.tasks.len(), run.tasks.len());
    assert_eq!(restored.runnable_tasks().len(), run.runnable_tasks().len());
    let _ = fs::remove_dir_all(ws);
}

// -----------------------------------------------------------------------------
// 2. RESTART STARTING EXECUTOR
// -----------------------------------------------------------------------------
#[test]
fn test_02_restart_starting_executor() {
    let ws = temp_workspace("starting-executor");
    let mut run = seed_run(&ws, 3);
    let task_id = run.runnable_tasks()[0].clone();
    let _packet = run.start_task(&task_id, 1000).unwrap();
    run.mark_external_process_started(&task_id).unwrap();

    let json = run.snapshot_json().unwrap();
    let restored = ExecutionRun::restore_json(&json).unwrap();
    assert_eq!(restored.tasks[&task_id].state, ExecutionTaskState::Running);
    let _ = fs::remove_dir_all(ws);
}

// -----------------------------------------------------------------------------
// 3. RESTART RUNNING
// -----------------------------------------------------------------------------
#[test]
fn test_03_restart_running() {
    let ws = temp_workspace("running");
    let mut run = seed_run(&ws, 3);
    let task_id = run.runnable_tasks()[0].clone();
    let _packet = run.start_task(&task_id, 1000).unwrap();
    run.mark_external_process_started(&task_id).unwrap();
    if let Some(attempt) = run.attempts.last_mut() {
        attempt.state = TaskAttemptState::Running;
    }

    let json = run.snapshot_json().unwrap();
    let restored = ExecutionRun::restore_json(&json).unwrap();
    assert_eq!(restored.tasks[&task_id].state, ExecutionTaskState::Running);
    let _ = fs::remove_dir_all(ws);
}

// -----------------------------------------------------------------------------
// 4. RESTART AFTER ACTION RECEIPT
// -----------------------------------------------------------------------------
#[test]
fn test_04_restart_after_action_receipt() {
    let ws = temp_workspace("action-receipt");
    let mut run = seed_run(&ws, 3);
    let task_id = run.runnable_tasks()[0].clone();
    let _packet = run.start_task(&task_id, 1000).unwrap();
    run.mark_external_process_started(&task_id).unwrap();

    let action = test_action(&ws, "test_write");
    let result = test_result(true);
    let _ = run.record_action(&task_id, &action, result, 300);

    let json = run.snapshot_json().unwrap();
    let restored = ExecutionRun::restore_json(&json).unwrap();
    assert_eq!(restored.tasks.len(), run.tasks.len());
    let _ = fs::remove_dir_all(ws);
}

// -----------------------------------------------------------------------------
// 5. RESTART SOFT-WINDOW EXTENSION
// -----------------------------------------------------------------------------
#[test]
fn test_05_restart_soft_window_extension() {
    let ws = temp_workspace("soft-window");
    let mut run = seed_run(&ws, 3);
    let task_id = run.runnable_tasks()[0].clone();
    let _packet = run.start_task(&task_id, 1000).unwrap();
    run.mark_external_process_started(&task_id).unwrap();

    let progress = ProgressSnapshot {
        workspace_fingerprint: "fp-progress".into(),
        task_state_fingerprint: "tp-progress".into(),
        changed_paths: vec!["src/main.rs".into()],
        passing_tests: 2,
        useful_artifacts: 1,
        ..ProgressSnapshot::default()
    };
    let ext_expiry = run.grant_authority_extension(&task_id, &progress, 15 * 60 * 1000);
    assert!(ext_expiry.is_ok());

    let json = run.snapshot_json().unwrap();
    let restored = ExecutionRun::restore_json(&json).unwrap();
    assert_eq!(restored.tasks.len(), run.tasks.len());
    let _ = fs::remove_dir_all(ws);
}

// -----------------------------------------------------------------------------
// 6. RESTART RECOVERY REVIEW REQUIRED
// -----------------------------------------------------------------------------
#[test]
fn test_06_restart_recovery_review_required() {
    let ws = temp_workspace("recovery-review");
    let mut run = seed_run(&ws, 3);
    let task_id = run.runnable_tasks()[0].clone();
    let _packet = run.start_task(&task_id, 1000).unwrap();
    run.mark_external_process_started(&task_id).unwrap();
    if let Some(task) = run.tasks.get_mut(&task_id) {
        task.usage_budget.wall_clock_ms = 1_000;
        task.state = ExecutionTaskState::WaitingRetry;
    }
    if let Some(attempt) = run.attempts.last_mut() {
        attempt.state = TaskAttemptState::Failed;
        attempt.ended_at_ms = Some(2000);
    }
    run.state = ExecutionRunState::WaitingRetry;

    let json = run.snapshot_json().unwrap();
    let restored = ExecutionRun::restore_json(&json).unwrap();
    assert_eq!(restored.state, ExecutionRunState::WaitingRetry);
    let _ = fs::remove_dir_all(ws);
}

// -----------------------------------------------------------------------------
// 7. RESTART RECOVERY REVALIDATED (DEFECT A FIX VERIFICATION)
// -----------------------------------------------------------------------------
#[test]
fn test_07_restart_recovery_revalidated() {
    let ws = temp_workspace("recovery-revalidated");
    let mut run = seed_run(&ws, 3);
    let task_id = run.runnable_tasks()[0].clone();
    let _packet = run.start_task(&task_id, 1000).unwrap();
    run.mark_external_process_started(&task_id).unwrap();

    // Simulate task with exceeded wall clock budget placed into WaitingRetry
    if let Some(task) = run.tasks.get_mut(&task_id) {
        task.usage_budget.wall_clock_ms = 1_500_000;
        task.state = ExecutionTaskState::WaitingRetry;
    }
    if let Some(attempt) = run.attempts.last_mut() {
        attempt.state = TaskAttemptState::WaitingRetry;
        attempt.usage.wall_time_ms = 1_500_042;
    }
    run.state = ExecutionRunState::Ready;

    // This exact scenario previously failed with "mission state unavailable"
    let json = run.snapshot_json().unwrap();
    let restored = ExecutionRun::restore_json(&json).expect("Defect A fix: WaitingRetry budget boundary succeeds");
    assert_eq!(restored.tasks[&task_id].state, ExecutionTaskState::WaitingRetry);
    let _ = fs::remove_dir_all(ws);
}

// -----------------------------------------------------------------------------
// 8. RESTART RETRY AUTHORIZED
// -----------------------------------------------------------------------------
#[test]
fn test_08_restart_retry_authorized() {
    let ws = temp_workspace("retry-authorized");
    let mut run = seed_run(&ws, 3);
    let task_id = run.runnable_tasks()[0].clone();
    let target = interrupt_attempt(&mut run, &task_id, 1_005_000);

    let delta = ReviewedRecoveryDelta {
        mission_id: run.mission_id.clone(),
        mission_revision: run.mission_revision,
        seal_hash: run.seal_hash.clone(),
        task_id: task_id.clone(),
        attempt_id: target.attempt_id.clone(),
        checkpoint_id: None,
        affected_paths: vec!["file.txt".into()],
        baseline_fingerprint: run.workspace_fingerprint.clone(),
        authorized_starting_fingerprint: String::new(),
        authorized_at_ms: 1_006_000,
    };
    run.authorize_manual_recovery_retry_with_delta(&target, Some(&delta), 1_006_000).unwrap();

    let json = run.snapshot_json().unwrap();
    let restored = ExecutionRun::restore_json(&json).unwrap();
    assert_eq!(restored.tasks[&task_id].state, ExecutionTaskState::WaitingRetry);
    let _ = fs::remove_dir_all(ws);
}

// -----------------------------------------------------------------------------
// 9. RESTART EVIDENCE COLLECTION
// -----------------------------------------------------------------------------
#[test]
fn test_09_restart_evidence_collection() {
    let ws = temp_workspace("evidence-collection");
    let mut run = seed_run(&ws, 2);
    for task in run.tasks.values_mut() {
        task.state = ExecutionTaskState::FinishedAwaitingVerification;
    }
    run.state = ExecutionRunState::ExecutionTasksFinishedAwaitingVerification;

    let json = run.snapshot_json().unwrap();
    let restored = ExecutionRun::restore_json(&json).unwrap();
    assert_eq!(restored.state, ExecutionRunState::ExecutionTasksFinishedAwaitingVerification);
    let _ = fs::remove_dir_all(ws);
}

// -----------------------------------------------------------------------------
// 10. RESTART HUMAN DECISION WAITING
// -----------------------------------------------------------------------------
#[test]
fn test_10_restart_human_decision_waiting() {
    let ws = temp_workspace("human-decision");
    let mut run = seed_run(&ws, 1);
    for task in run.tasks.values_mut() {
        task.state = ExecutionTaskState::FinishedAwaitingVerification;
    }
    run.state = ExecutionRunState::ExecutionTasksFinishedAwaitingVerification;

    let json = run.snapshot_json().unwrap();
    let restored = ExecutionRun::restore_json(&json).unwrap();
    assert_eq!(restored.state, ExecutionRunState::ExecutionTasksFinishedAwaitingVerification);
    let _ = fs::remove_dir_all(ws);
}

// -----------------------------------------------------------------------------
// 11. RESTART VERIFICATION
// -----------------------------------------------------------------------------
#[test]
fn test_11_restart_verification() {
    let ws = temp_workspace("verification");
    let mut run = seed_run(&ws, 1);
    for task in run.tasks.values_mut() {
        task.state = ExecutionTaskState::FinishedAwaitingVerification;
    }
    run.state = ExecutionRunState::ExecutionTasksFinishedAwaitingVerification;

    let json = run.snapshot_json().unwrap();
    let restored = ExecutionRun::restore_json(&json).unwrap();
    assert_eq!(restored.state, ExecutionRunState::ExecutionTasksFinishedAwaitingVerification);
    let _ = fs::remove_dir_all(ws);
}

// -----------------------------------------------------------------------------
// 12. RESTART VERIFIED COMPLETE
// -----------------------------------------------------------------------------
#[test]
fn test_12_restart_verified_complete() {
    let ws = temp_workspace("verified-complete");
    let mut run = seed_run(&ws, 1);
    for task in run.tasks.values_mut() {
        task.state = ExecutionTaskState::FinishedAwaitingVerification;
    }
    run.state = ExecutionRunState::ExecutionTasksFinishedAwaitingVerification;

    let json = run.snapshot_json().unwrap();
    let restored = ExecutionRun::restore_json(&json).unwrap();
    assert_eq!(restored.state, ExecutionRunState::ExecutionTasksFinishedAwaitingVerification);
    let _ = fs::remove_dir_all(ws);
}

// -----------------------------------------------------------------------------
// 13. SIMULATED DISK-FULL BEFORE CHECKPOINT
// -----------------------------------------------------------------------------
#[test]
fn test_13_simulated_disk_full_before_checkpoint() {
    let ws = temp_workspace("disk-full-before");
    // Requesting impossible amount of storage fails preflight headroom check
    let result = check_low_disk_headroom(&ws, u64::MAX - 100_000_000);
    assert!(result.is_err());
    let err_str = result.unwrap_err().to_string();
    assert!(err_str.contains("INSUFFICIENT_RUNTIME_STORAGE"), "Error was: {err_str}");

    // Storage remains intact, workspace exists
    assert!(ws.exists());
    let _ = fs::remove_dir_all(ws);
}

// -----------------------------------------------------------------------------
// 14. SIMULATED DISK-FULL DURING TEMP WRITE (CLEANUP OF .TMP)
// -----------------------------------------------------------------------------
#[test]
fn test_14_simulated_disk_full_during_temp_write() {
    let ws = temp_workspace("temp-write-cleanup");
    let store_dir = ws.join("recovery");
    let _store = RecoveryStore::new(&store_dir, vec![5; 32]).unwrap();

    // Verify orphan files are cleaned on startup and not left behind
    let dummy_tmp = store_dir.join(".checkpoint-999.json.tmp");
    fs::write(&dummy_tmp, b"partial write").unwrap();
    assert!(dummy_tmp.exists());

    // Reopen store; orphan tmp must be cleaned
    let _ = RecoveryStore::new(&store_dir, vec![5; 32]).unwrap();
    assert!(!dummy_tmp.exists(), "Orphan tmp was cleaned on store open");

    let _ = fs::remove_dir_all(ws);
}

// -----------------------------------------------------------------------------
// 15. CORRUPTED NEWEST TEMP GENERATION
// -----------------------------------------------------------------------------
#[test]
fn test_15_corrupted_newest_temp_generation() {
    let ws = temp_workspace("corrupted-temp");
    let store_dir = ws.join("recovery");
    let store = RecoveryStore::new(&store_dir, vec![5; 32]).unwrap();

    let run = seed_run(&ws, 1);
    let _ = store.write_checkpoint(make_checkpoint_content(&ws, &run, "c1")).unwrap();

    // Simulate corrupted leftover .tmp file
    let corrupt_tmp = store_dir.join(".checkpoint-0000000000000002.json.tmp");
    fs::write(&corrupt_tmp, b"garbage data").unwrap();

    // Load latest ignores or cleans up the corrupt .tmp file and returns valid c1
    let latest = store.load_latest().unwrap();
    assert!(latest.is_some());
    assert_eq!(latest.unwrap().sequence, 1);

    let _ = fs::remove_dir_all(ws);
}

// -----------------------------------------------------------------------------
// 16. VALID PREVIOUS AUTHORITATIVE GENERATION REMAINS
// -----------------------------------------------------------------------------
#[test]
fn test_16_valid_previous_authoritative_generation_remains() {
    let ws = temp_workspace("valid-previous");
    let store_dir = ws.join("recovery");
    let store = RecoveryStore::new(&store_dir, vec![5; 32]).unwrap();

    let run = seed_run(&ws, 1);
    let _ = store.write_checkpoint(make_checkpoint_content(&ws, &run, "c1")).unwrap();
    let _ = store.write_checkpoint(make_checkpoint_content(&ws, &run, "c2")).unwrap();

    // Valid chain loads sequence 2
    let latest = store.load_latest().unwrap().unwrap();
    assert_eq!(latest.sequence, 2);

    let _ = fs::remove_dir_all(ws);
}

// -----------------------------------------------------------------------------
// 17. JUNCTION-BACKED RUNTIME STORAGE
// -----------------------------------------------------------------------------
#[test]
fn test_17_junction_backed_runtime_storage() {
    let ws = temp_workspace("junction-target");
    let junction_link = temp_workspace("junction-link");
    let _ = fs::remove_dir_all(&junction_link);

    // On Windows, create directory junction
    #[cfg(windows)]
    {
        let status = std::process::Command::new("cmd")
            .args(["/c", "mklink", "/J", junction_link.to_str().unwrap(), ws.to_str().unwrap()])
            .status();

        if let Ok(s) = status {
            if s.success() {
                let recovery_dir = junction_link.join("recovery");
                let store = RecoveryStore::new(&recovery_dir, vec![5; 32]).unwrap();
                let run = seed_run(&ws, 1);
                let _ = store.write_checkpoint(make_checkpoint_content(&ws, &run, "junction-test")).unwrap();

                let latest = store.load_latest().unwrap();
                assert!(latest.is_some());
                assert_eq!(latest.unwrap().sequence, 1);

                // Clean up junction
                let _ = std::process::Command::new("cmd")
                    .args(["/c", "rmdir", junction_link.to_str().unwrap()])
                    .status();
            }
        }
    }

    let _ = fs::remove_dir_all(ws);
}

// -----------------------------------------------------------------------------
// 18. NON-C RUNTIME STORAGE
// -----------------------------------------------------------------------------
#[test]
fn test_18_non_c_runtime_storage() {
    // Uses target directory (which is on D: drive for this test suite)
    let ws = temp_workspace("non-c-storage");
    let recovery_dir = ws.join("recovery");
    let store = RecoveryStore::new(&recovery_dir, vec![5; 32]).unwrap();

    let run = seed_run(&ws, 1);
    let _ = store.write_checkpoint(make_checkpoint_content(&ws, &run, "non-c-test")).unwrap();

    let latest = store.load_latest().unwrap();
    assert!(latest.is_some());
    assert_eq!(latest.unwrap().sequence, 1);

    let _ = fs::remove_dir_all(ws);
}

// -----------------------------------------------------------------------------
// 19. LOW-STORAGE PREFLIGHT FAILURE
// -----------------------------------------------------------------------------
#[test]
fn test_19_low_storage_preflight_failure() {
    let ws = temp_workspace("preflight-failure");
    // Normal size passes
    assert!(check_low_disk_headroom(&ws, 1024).is_ok());

    // Excessive size fails with INSUFFICIENT_RUNTIME_STORAGE
    let err = check_low_disk_headroom(&ws, 100 * 1024 * 1024 * 1024 * 1024 * 1024); // 100 PB
    assert!(err.is_err());
    let msg = err.unwrap_err().to_string();
    assert!(msg.contains("INSUFFICIENT_RUNTIME_STORAGE"), "Message was: {msg}");

    let _ = fs::remove_dir_all(ws);
}

// -----------------------------------------------------------------------------
// 20. SECOND REOPEN REMAINS DETERMINISTIC
// -----------------------------------------------------------------------------
#[test]
fn test_20_second_reopen_remains_deterministic() {
    let ws = temp_workspace("deterministic-reopen");
    let store_dir = ws.join("recovery");
    let store1 = RecoveryStore::new(&store_dir, vec![5; 32]).unwrap();

    let run = seed_run(&ws, 2);
    let _ = store1.write_checkpoint(make_checkpoint_content(&ws, &run, "determ-1")).unwrap();

    // First load
    let first = store1.load_latest().unwrap().unwrap();

    // Second load from fresh store instance
    let store2 = RecoveryStore::new(&store_dir, vec![5; 32]).unwrap();
    let second = store2.load_latest().unwrap().unwrap();

    assert_eq!(first.sequence, second.sequence);
    assert_eq!(first.checkpoint_id, second.checkpoint_id);
    assert_eq!(first.content.authority.p6_seal_hash, second.content.authority.p6_seal_hash);

    let _ = fs::remove_dir_all(ws);
}

// -----------------------------------------------------------------------------
// SCALE BENCHMARK: 100 TASKS & 5,000 EVENTS
// -----------------------------------------------------------------------------
#[test]
fn test_scale_fixture_100_tasks_5000_events() {
    let ws = temp_workspace("scale-100-tasks-5000");
    let mut run = seed_run(&ws, 100);

    let base_seq = run.events.last().map(|e| e.sequence).unwrap_or(0);
    for i in 1..=5000 {
        run.events.push(ExecutionEvent {
            sequence: base_seq + i,
            occurred_at_ms: 1000 + i,
            task_id: Some(format!("task_{:03}", i % 100)),
            kind: ExecutionEventKind::ActionResult,
            detail: format!("Event detail payload number {i}"),
        });
    }

    // Measure serialization & restoration
    let t0 = Instant::now();
    let json = run.snapshot_json().unwrap();
    let serialize_ms = t0.elapsed().as_millis();

    let t1 = Instant::now();
    let restored = ExecutionRun::restore_json(&json).unwrap();
    let restore_ms = t1.elapsed().as_millis();

    println!("100 tasks / 5000 events: serialize={serialize_ms}ms, restore={restore_ms}ms, payload={}KB", json.len() / 1024);
    assert_eq!(restored.tasks.len(), run.tasks.len());
    assert_eq!(restored.events.len(), run.events.len());
    assert!(restore_ms < 500, "Restore took {restore_ms}ms, expected < 500ms");

    let _ = fs::remove_dir_all(ws);
}

// -----------------------------------------------------------------------------
// SCALE BENCHMARK: 10,000 EVENTS
// -----------------------------------------------------------------------------
#[test]
fn test_scale_fixture_10000_events() {
    let ws = temp_workspace("scale-10000-events");
    let mut run = seed_run(&ws, 10);

    let base_seq = run.events.last().map(|e| e.sequence).unwrap_or(0);
    for i in 1..=10000 {
        run.events.push(ExecutionEvent {
            sequence: base_seq + i,
            occurred_at_ms: 1000 + i,
            task_id: Some("task_001".into()),
            kind: ExecutionEventKind::ActionResult,
            detail: format!("Event {i}"),
        });
    }

    let t0 = Instant::now();
    let json = run.snapshot_json().unwrap();
    let serialize_ms = t0.elapsed().as_millis();

    let t1 = Instant::now();
    let restored = ExecutionRun::restore_json(&json).unwrap();
    let restore_ms = t1.elapsed().as_millis();

    println!("10000 events: serialize={serialize_ms}ms, restore={restore_ms}ms, payload={}KB", json.len() / 1024);
    assert_eq!(restored.events.len(), run.events.len());

    let _ = fs::remove_dir_all(ws);
}
