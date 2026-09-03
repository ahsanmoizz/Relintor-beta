use base64::{engine::general_purpose::STANDARD, Engine as _};
use ed25519_dalek::SigningKey;
use relintor_execution::{
    inventory_fingerprint, workspace_inventory, ActionRequest, ActionResult,
    ExecutionError, ExecutionEventKind, ExecutionRun, ExecutionRunState, FailureClass,
    RecoveryAttemptTarget, ReviewedRecoveryDelta, SchedulerPolicy, TaskAttemptState,
    WatchdogState,
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
        revision: "drift-context-1".into(),
    }
}

fn evidence() -> EvidenceObligation {
    EvidenceObligation {
        class: EvidenceClass::TestOutput,
        minimum_confidence: EvidenceConfidence::StrongDeterministic,
        rationale: "drift test output".into(),
        required: true,
    }
}

fn seed(id: &str, dependencies: Vec<&str>) -> ProjectRequirementSeed {
    ProjectRequirementSeed {
        requirement_id: Some(id.into()),
        title: format!("Drift test {id}"),
        intent: format!("Execute the {id} objective"),
        source: RequirementSource::User {
            reference: format!("drift-test-{id}"),
        },
        priority: RequirementPriority::P2,
        acceptance_criteria: vec![AcceptanceCriterion {
            criterion_id: format!("{id}-criterion"),
            statement: "The deterministic drift behavior is observed".into(),
            criterion_type: "test".into(),
            machine_checkable: true,
        }],
        verification_policy: VerificationPolicy {
            obligations: vec![evidence()],
            p8_collector_required: false,
        },
        dependencies: dependencies.into_iter().map(str::to_owned).collect(),
        risk: RequirementRisk::Medium,
        requirement_type: "drift-test-scope".into(),
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
        .sign("drift-test-signer", &signing_key)
        .expect("registry signs");
    let trusted = TrustedSignerSet {
        signers: vec![TrustedSigner {
            key_id: "drift-test-signer".into(),
            algorithm: "Ed25519".into(),
            public_key: STANDARD.encode(signing_key.verifying_key().to_bytes()),
        }],
    };
    let mut reqs = Vec::new();
    for i in 1..=req_count {
        let deps = if i > 1 {
            vec![format!("D-{:02}", i - 1)]
        } else {
            vec![]
        };
        let deps_str: Vec<&str> = deps.iter().map(|s| s.as_str()).collect();
        reqs.push(seed(&format!("D-{:02}", i), deps_str));
    }
    let authority = ProjectAuthorityInput {
        requirements: reqs,
        source_revision: "drift-fixture-revision".into(),
        source_fingerprint: "drift-fixture-fingerprint".into(),
        ..ProjectAuthorityInput::default()
    };
    let draft = AuthorityEngine
        .build_draft_with_project_authority(
            "mission-drift-test",
            "project-drift-test",
            "project-source-1",
            "workspace-source-1",
            registry.clone(),
            &context(),
            scope_fingerprint(BTreeMap::from([("root".into(), "drift-test".into())])),
            authority,
            Vec::new(),
        )
        .expect("build draft");
    let sealed = AuthorityEngine
        .seal(&draft, &trusted, "2026-08-15T00:00:00Z")
        .expect("seal fixture");
    (sealed.0, sealed.1, registry, trusted)
}

fn temp_workspace(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "relintor-drift-test-{}-{}-{}",
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

fn interrupt_current_attempt(run: &mut ExecutionRun, task_id: &str, ended_at: u64) -> RecoveryAttemptTarget {
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

#[test]
fn test_mandatory_reviewed_retry_drift_baseline_lifecycle() {
    let ws = temp_workspace("lifecycle");
    fs::write(ws.join("initial_a.txt"), "baseline A content").unwrap();
    let mut run = seed_run(&ws, 1);
    let baseline_a = run.workspace_fingerprint.clone();

    let task_id = run.runnable_tasks().into_iter().next().unwrap();
    let now = 1_000_000;

    // 1. Attempt 1 starts
    let packet1 = run.start_task(&task_id, now).expect("start attempt 1");
    assert_eq!(packet1.attempt_number, 1);
    assert_eq!(packet1.workspace_fingerprint, baseline_a);

    // 2. Attempt 1 changes file1 and file2
    fs::write(ws.join("file1.txt"), "partial work in file 1").unwrap();
    fs::write(ws.join("file2.rs"), "fn partial_work() {}").unwrap();

    // 3. Mark external process started, then interrupt attempt 1
    let target = interrupt_current_attempt(&mut run, &task_id, now + 10_000);

    // 4. Exact file1/file2 delta reviewed by user
    let reviewed_files = vec!["file1.txt".to_string(), "file2.rs".to_string()];
    let delta = ReviewedRecoveryDelta {
        mission_id: run.mission_id.clone(),
        mission_revision: run.mission_revision,
        seal_hash: run.seal_hash.clone(),
        task_id: task_id.clone(),
        attempt_id: target.attempt_id.clone(),
        checkpoint_id: Some("chk-interrupted-1".into()),
        affected_paths: reviewed_files.clone(),
        baseline_fingerprint: baseline_a.clone(),
        authorized_starting_fingerprint: String::new(),
        authorized_at_ms: now + 20_000,
    };

    // 5. RecoveryRevalidated & 6. RetryAuthorized
    run.authorize_manual_recovery_retry_with_delta(&target, Some(&delta), now + 20_000)
        .expect("authorize manual retry with reviewed delta");

    assert_eq!(run.state, ExecutionRunState::Ready);
    assert_eq!(run.watchdog_state, WatchdogState::Healthy);
    // Baseline is now A + file1 + file2
    assert_ne!(run.workspace_fingerprint, baseline_a);
    let baseline_after_delta = run.workspace_fingerprint.clone();

    // 7. Fresh retry starts from A + file1 + file2: EXPECTED: NO STARTUP DRIFT
    let packet2 = run.start_task(&task_id, now + 30_000).expect("start fresh retry attempt 2");
    assert_eq!(packet2.attempt_number, 2);
    assert_eq!(packet2.workspace_fingerprint, baseline_after_delta);
    assert_eq!(run.watchdog_state, WatchdogState::Healthy);

    // 8. Then executor changes authorized file3: PASS
    let action_file3 = ActionRequest {
        tool: "antigravity".into(),
        operation: "write_file".into(),
        arguments: vec!["file3.json".into()],
        working_scope: ws.display().to_string(),
        environment_identity: "test".into(),
        mutable: true,
        paths: vec![ws.join("file3.json").display().to_string()],
        external_authority: None,
    };

    run.authorize_action(&task_id, &packet2.lease_id, &packet2, &action_file3, now + 35_000)
        .expect("authorize action for authorized file3");

    fs::write(ws.join("file3.json"), "{\"status\": \"ok\"}").unwrap();
    let inv_after_file3 = workspace_inventory(&ws).unwrap();
    let fp_after_file3 = inventory_fingerprint(&inv_after_file3).unwrap();

    let action_result = ActionResult {
        success: true,
        failure_class: None,
        failure_message: None,
        changed_paths: vec!["file3.json".into()],
        workspace_fingerprint: fp_after_file3,
        passing_tests: 1,
        useful_artifacts: 1,
        wall_time_ms: 100,
        estimated_cost_micros: None,
    };

    run.record_action(&task_id, &action_file3, action_result, now + 36_000)
        .expect("record successful action for file3");
    assert_eq!(run.watchdog_state, WatchdogState::Healthy);

    // 9. Then external/unreviewed fileX changes: TASK_DRIFT_DENIED
    let action_outside = ActionRequest {
        tool: "antigravity".into(),
        operation: "write_file".into(),
        arguments: vec!["outside.txt".into()],
        working_scope: ws.display().to_string(),
        environment_identity: "test".into(),
        mutable: true,
        paths: vec!["D:/unauthorized/outside.txt".into()],
        external_authority: None,
    };

    let drift_err = run.authorize_action(&task_id, &packet2.lease_id, &packet2, &action_outside, now + 40_000)
        .unwrap_err();
    assert!(matches!(drift_err, ExecutionError::TaskDrift));
    assert_eq!(run.watchdog_state, WatchdogState::DriftDetected);

    fs::write(ws.join("fileX_unreviewed.txt"), "unreviewed payload").unwrap();
    let drift_detect_err = run.detect_workspace_drift(now + 45_000);
    assert!(drift_detect_err.is_err() || run.watchdog_state == WatchdogState::DriftDetected);

    let _ = fs::remove_dir_all(&ws);
}

#[test]
fn test_stale_recovery_delta_denied() {
    let ws = temp_workspace("stale_delta");
    fs::write(ws.join("initial.txt"), "baseline").unwrap();
    let mut run = seed_run(&ws, 1);
    let task_id = run.runnable_tasks().into_iter().next().unwrap();
    let now = 1_000_000;

    let _packet = run.start_task(&task_id, now).unwrap();
    fs::write(ws.join("edit.txt"), "edit 1").unwrap();
    let target = interrupt_current_attempt(&mut run, &task_id, now + 1_000);

    let delta = ReviewedRecoveryDelta {
        mission_id: run.mission_id.clone(),
        mission_revision: run.mission_revision,
        seal_hash: run.seal_hash.clone(),
        task_id: task_id.clone(),
        attempt_id: target.attempt_id.clone(),
        checkpoint_id: None,
        affected_paths: vec!["edit.txt".into()],
        baseline_fingerprint: run.workspace_fingerprint.clone(),
        authorized_starting_fingerprint: String::new(),
        authorized_at_ms: now + 2_000,
    };

    // First retry authorization succeeds
    run.authorize_manual_recovery_retry_with_delta(&target, Some(&delta), now + 2_000)
        .expect("first retry authorization");

    // Second retry authorization with the EXACT SAME stale delta must fail closed
    run.state = ExecutionRunState::RevalidationRequired;
    run.attempts.last_mut().unwrap().state = TaskAttemptState::Failed;
    let stale_err = run.authorize_manual_recovery_retry_with_delta(&target, Some(&delta), now + 3_000)
        .unwrap_err();
    assert!(stale_err.to_string().contains("stale"));

    let _ = fs::remove_dir_all(&ws);
}

#[test]
fn test_wrong_task_delta_denied() {
    let ws = temp_workspace("wrong_task");
    fs::write(ws.join("initial.txt"), "baseline").unwrap();
    let mut run = seed_run(&ws, 2);
    let task_id = run.runnable_tasks().into_iter().next().unwrap();
    let now = 1_000_000;

    let _packet = run.start_task(&task_id, now).unwrap();
    fs::write(ws.join("edit.txt"), "edit").unwrap();
    let target = interrupt_current_attempt(&mut run, &task_id, now + 1_000);

    let delta = ReviewedRecoveryDelta {
        mission_id: run.mission_id.clone(),
        mission_revision: run.mission_revision,
        seal_hash: run.seal_hash.clone(),
        task_id: "wrong-task-id".into(),
        attempt_id: target.attempt_id.clone(),
        checkpoint_id: None,
        affected_paths: vec!["edit.txt".into()],
        baseline_fingerprint: run.workspace_fingerprint.clone(),
        authorized_starting_fingerprint: String::new(),
        authorized_at_ms: now + 2_000,
    };

    let err = run.authorize_manual_recovery_retry_with_delta(&target, Some(&delta), now + 2_000)
        .unwrap_err();
    assert!(err.to_string().contains("task mismatch"));

    let _ = fs::remove_dir_all(&ws);
}

#[test]
fn test_wrong_mission_delta_denied() {
    let ws = temp_workspace("wrong_mission");
    fs::write(ws.join("initial.txt"), "baseline").unwrap();
    let mut run = seed_run(&ws, 1);
    let task_id = run.runnable_tasks().into_iter().next().unwrap();
    let now = 1_000_000;

    let _packet = run.start_task(&task_id, now).unwrap();
    fs::write(ws.join("edit.txt"), "edit").unwrap();
    let target = interrupt_current_attempt(&mut run, &task_id, now + 1_000);

    let delta = ReviewedRecoveryDelta {
        mission_id: "other-mission-unauthorized".into(),
        mission_revision: run.mission_revision,
        seal_hash: run.seal_hash.clone(),
        task_id: task_id.clone(),
        attempt_id: target.attempt_id.clone(),
        checkpoint_id: None,
        affected_paths: vec!["edit.txt".into()],
        baseline_fingerprint: run.workspace_fingerprint.clone(),
        authorized_starting_fingerprint: String::new(),
        authorized_at_ms: now + 2_000,
    };

    let err = run.authorize_manual_recovery_retry_with_delta(&target, Some(&delta), now + 2_000)
        .unwrap_err();
    assert!(err.to_string().contains("mission mismatch"));

    let _ = fs::remove_dir_all(&ws);
}

#[test]
fn test_wrong_revision_delta_denied() {
    let ws = temp_workspace("wrong_rev");
    fs::write(ws.join("initial.txt"), "baseline").unwrap();
    let mut run = seed_run(&ws, 1);
    let task_id = run.runnable_tasks().into_iter().next().unwrap();
    let now = 1_000_000;

    let _packet = run.start_task(&task_id, now).unwrap();
    fs::write(ws.join("edit.txt"), "edit").unwrap();
    let target = interrupt_current_attempt(&mut run, &task_id, now + 1_000);

    let delta = ReviewedRecoveryDelta {
        mission_id: run.mission_id.clone(),
        mission_revision: 999,
        seal_hash: run.seal_hash.clone(),
        task_id: task_id.clone(),
        attempt_id: target.attempt_id.clone(),
        checkpoint_id: None,
        affected_paths: vec!["edit.txt".into()],
        baseline_fingerprint: run.workspace_fingerprint.clone(),
        authorized_starting_fingerprint: String::new(),
        authorized_at_ms: now + 2_000,
    };

    let err = run.authorize_manual_recovery_retry_with_delta(&target, Some(&delta), now + 2_000)
        .unwrap_err();
    assert!(err.to_string().contains("revision mismatch"));

    let _ = fs::remove_dir_all(&ws);
}

#[test]
fn test_unreviewed_extra_file_at_retry_authorization_denied_with_task_drift() {
    let ws = temp_workspace("extra_file_drift");
    fs::write(ws.join("initial.txt"), "baseline").unwrap();
    let mut run = seed_run(&ws, 1);
    let task_id = run.runnable_tasks().into_iter().next().unwrap();
    let now = 1_000_000;

    let _packet = run.start_task(&task_id, now).unwrap();
    fs::write(ws.join("file1.txt"), "reviewed file").unwrap();
    let target = interrupt_current_attempt(&mut run, &task_id, now + 1_000);

    // Unreviewed extra file added before review authorization
    fs::write(ws.join("unreviewed_fileX.txt"), "sneaked in modification").unwrap();

    let delta = ReviewedRecoveryDelta {
        mission_id: run.mission_id.clone(),
        mission_revision: run.mission_revision,
        seal_hash: run.seal_hash.clone(),
        task_id: task_id.clone(),
        attempt_id: target.attempt_id.clone(),
        checkpoint_id: None,
        affected_paths: vec!["file1.txt".into()],
        baseline_fingerprint: run.workspace_fingerprint.clone(),
        authorized_starting_fingerprint: String::new(),
        authorized_at_ms: now + 2_000,
    };

    let err = run.authorize_manual_recovery_retry_with_delta(&target, Some(&delta), now + 2_000)
        .unwrap_err();
    assert!(matches!(err, ExecutionError::TaskDrift));
    assert_eq!(run.watchdog_state, WatchdogState::DriftDetected);
    assert_eq!(
        run.events.last().unwrap().kind,
        ExecutionEventKind::TaskDriftDenied
    );

    let _ = fs::remove_dir_all(&ws);
}

#[test]
fn test_restart_reconstruction_deterministic() {
    let ws = temp_workspace("restart_deterministic");
    fs::write(ws.join("initial.txt"), "baseline").unwrap();
    let mut run = seed_run(&ws, 1);
    let task_id = run.runnable_tasks().into_iter().next().unwrap();
    let now = 1_000_000;

    let _packet = run.start_task(&task_id, now).unwrap();
    fs::write(ws.join("f1.txt"), "f1").unwrap();
    let target = interrupt_current_attempt(&mut run, &task_id, now + 1_000);

    let delta = ReviewedRecoveryDelta {
        mission_id: run.mission_id.clone(),
        mission_revision: run.mission_revision,
        seal_hash: run.seal_hash.clone(),
        task_id: task_id.clone(),
        attempt_id: target.attempt_id.clone(),
        checkpoint_id: None,
        affected_paths: vec!["f1.txt".into()],
        baseline_fingerprint: run.workspace_fingerprint.clone(),
        authorized_starting_fingerprint: String::new(),
        authorized_at_ms: now + 2_000,
    };

    run.authorize_manual_recovery_retry_with_delta(&target, Some(&delta), now + 2_000)
        .unwrap();

    let json = run.snapshot_json().expect("serialize ledger");
    let restored = ExecutionRun::restore_json(&json).expect("restore ledger");

    assert_eq!(restored.run_id, run.run_id);
    assert_eq!(restored.workspace_fingerprint, run.workspace_fingerprint);
    assert_eq!(restored.reviewed_recovery_deltas.len(), 1);
    assert_eq!(
        restored.reviewed_recovery_deltas[0].affected_paths,
        vec!["f1.txt".to_string()]
    );
    assert_eq!(restored.watchdog_state, WatchdogState::Healthy);

    let _ = fs::remove_dir_all(&ws);
}

#[test]
fn test_second_reviewed_retry_works() {
    let ws = temp_workspace("second_retry");
    fs::write(ws.join("initial.txt"), "baseline").unwrap();
    let mut run = seed_run(&ws, 1);
    let task_id = run.runnable_tasks().into_iter().next().unwrap();
    let now = 1_000_000;

    let _packet1 = run.start_task(&task_id, now).unwrap();
    fs::write(ws.join("f1.txt"), "attempt 1 content").unwrap();
    let t1 = interrupt_current_attempt(&mut run, &task_id, now + 1_000);

    let delta1 = ReviewedRecoveryDelta {
        mission_id: run.mission_id.clone(),
        mission_revision: run.mission_revision,
        seal_hash: run.seal_hash.clone(),
        task_id: task_id.clone(),
        attempt_id: t1.attempt_id.clone(),
        checkpoint_id: None,
        affected_paths: vec!["f1.txt".into()],
        baseline_fingerprint: run.workspace_fingerprint.clone(),
        authorized_starting_fingerprint: String::new(),
        authorized_at_ms: now + 2_000,
    };
    run.authorize_manual_recovery_retry_with_delta(&t1, Some(&delta1), now + 2_000)
        .unwrap();

    let packet2 = run.start_task(&task_id, now + 3_000).expect("start attempt 2");
    assert_eq!(packet2.attempt_number, 2);
    fs::write(ws.join("f2.txt"), "attempt 2 content").unwrap();
    let t2 = interrupt_current_attempt(&mut run, &task_id, now + 4_000);

    let delta2 = ReviewedRecoveryDelta {
        mission_id: run.mission_id.clone(),
        mission_revision: run.mission_revision,
        seal_hash: run.seal_hash.clone(),
        task_id: task_id.clone(),
        attempt_id: t2.attempt_id.clone(),
        checkpoint_id: None,
        affected_paths: vec!["f2.txt".into()],
        baseline_fingerprint: run.workspace_fingerprint.clone(),
        authorized_starting_fingerprint: String::new(),
        authorized_at_ms: now + 5_000,
    };
    run.authorize_manual_recovery_retry_with_delta(&t2, Some(&delta2), now + 5_000)
        .expect("authorize second manual retry");

    let packet3 = run.start_task(&task_id, now + 6_000).expect("start attempt 3");
    assert_eq!(packet3.attempt_number, 3);
    assert_eq!(run.watchdog_state, WatchdogState::Healthy);
    assert_eq!(run.reviewed_recovery_deltas.len(), 2);

    let _ = fs::remove_dir_all(&ws);
}

#[test]
fn test_mixed_language_fixture_works() {
    let ws = temp_workspace("mixed_lang");
    fs::create_dir_all(ws.join("apps/android/src/main/java")).unwrap();
    fs::create_dir_all(ws.join("scripts")).unwrap();
    fs::create_dir_all(ws.join("docs")).unwrap();
    fs::write(ws.join("docs/GUIDE.md"), "# Guide").unwrap();

    let mut run = seed_run(&ws, 1);
    let task_id = run.runnable_tasks().into_iter().next().unwrap();
    let now = 1_000_000;

    let _packet = run.start_task(&task_id, now).unwrap();

    let f_kt = "apps/android/src/main/java/SecurityScan.kt";
    let f_py = "scripts/security_scan.py";
    let f_js = "scripts/security-scan.js";
    let f_md = "docs/SECURITY_REPORT.md";

    fs::write(ws.join(f_kt), "package org.ciphrchat\nclass SecurityScan {}").unwrap();
    fs::write(ws.join(f_py), "def scan(): pass").unwrap();
    fs::write(ws.join(f_js), "console.log('scan');").unwrap();
    fs::write(ws.join(f_md), "# Security Report").unwrap();

    let target = interrupt_current_attempt(&mut run, &task_id, now + 1_000);

    let delta = ReviewedRecoveryDelta {
        mission_id: run.mission_id.clone(),
        mission_revision: run.mission_revision,
        seal_hash: run.seal_hash.clone(),
        task_id: task_id.clone(),
        attempt_id: target.attempt_id.clone(),
        checkpoint_id: None,
        affected_paths: vec![f_kt.into(), f_py.into(), f_js.into(), f_md.into()],
        baseline_fingerprint: run.workspace_fingerprint.clone(),
        authorized_starting_fingerprint: String::new(),
        authorized_at_ms: now + 2_000,
    };

    run.authorize_manual_recovery_retry_with_delta(&target, Some(&delta), now + 2_000)
        .expect("mixed language retry authorization");

    let fresh = run.start_task(&task_id, now + 3_000).expect("fresh retry start");
    assert_eq!(fresh.attempt_number, 2);
    assert_eq!(run.watchdog_state, WatchdogState::Healthy);

    let _ = fs::remove_dir_all(&ws);
}

#[test]
fn test_larger_project_fixture_works() {
    let ws = temp_workspace("larger_project");
    for i in 1..=25 {
        let dir = ws.join(format!("module_{:02}/src", i));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("lib.rs"), format!("pub fn mod_{i}() -> u32 {{ {i} }}")).unwrap();
    }

    let mut run = seed_run(&ws, 1);
    let task_id = run.runnable_tasks().into_iter().next().unwrap();
    let now = 1_000_000;

    let _packet = run.start_task(&task_id, now).unwrap();
    fs::write(ws.join("module_05/src/lib.rs"), "pub fn mod_05() -> u32 { 555 }").unwrap();
    fs::write(ws.join("module_12/src/lib.rs"), "pub fn mod_12() -> u32 { 1212 }").unwrap();

    let target = interrupt_current_attempt(&mut run, &task_id, now + 1_000);

    let delta = ReviewedRecoveryDelta {
        mission_id: run.mission_id.clone(),
        mission_revision: run.mission_revision,
        seal_hash: run.seal_hash.clone(),
        task_id: task_id.clone(),
        attempt_id: target.attempt_id.clone(),
        checkpoint_id: None,
        affected_paths: vec![
            "module_05/src/lib.rs".into(),
            "module_12/src/lib.rs".into(),
        ],
        baseline_fingerprint: run.workspace_fingerprint.clone(),
        authorized_starting_fingerprint: String::new(),
        authorized_at_ms: now + 2_000,
    };

    run.authorize_manual_recovery_retry_with_delta(&target, Some(&delta), now + 2_000)
        .expect("larger project retry authorization");

    let fresh = run.start_task(&task_id, now + 3_000).expect("fresh retry start");
    assert_eq!(fresh.attempt_number, 2);
    assert_eq!(run.watchdog_state, WatchdogState::Healthy);

    let _ = fs::remove_dir_all(&ws);
}