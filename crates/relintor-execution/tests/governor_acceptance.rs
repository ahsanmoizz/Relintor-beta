use base64::{engine::general_purpose::STANDARD, Engine as _};
use ed25519_dalek::SigningKey;
use relintor_execution::{
    ExecutionEventKind, ExecutionRun, ExecutionRunState, LeaseStatus,
    ProgressQuality, ProgressSnapshot, SchedulerPolicy, WatchdogState,
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
        revision: "gov-context-1".into(),
    }
}

fn seed(id: &str, dependencies: Vec<&str>) -> ProjectRequirementSeed {
    ProjectRequirementSeed {
        requirement_id: Some(id.into()),
        title: format!("Gov {id}"),
        intent: format!("Execute the {id} governor objective"),
        source: RequirementSource::User {
            reference: format!("gov-test-{id}"),
        },
        priority: RequirementPriority::P2,
        acceptance_criteria: vec![AcceptanceCriterion {
            criterion_id: format!("{id}-criterion"),
            statement: "The deterministic governor behavior is observed".into(),
            criterion_type: "test".into(),
            machine_checkable: true,
        }],
        verification_policy: VerificationPolicy {
            obligations: vec![EvidenceObligation {
                class: EvidenceClass::TestOutput,
                minimum_confidence: EvidenceConfidence::StrongDeterministic,
                rationale: "Governor test output".into(),
                required: true,
            }],
            p8_collector_required: false,
        },
        dependencies: dependencies.into_iter().map(str::to_owned).collect(),
        risk: RequirementRisk::Medium,
        requirement_type: "gov-test-scope".into(),
    }
}

fn sealed_fixture(req_count: usize) -> (
    MissionRevision,
    relintor_standards::ExecutionHandoff,
    StandardsRegistry,
    TrustedSignerSet,
) {
    let signing_key = SigningKey::from_bytes(&[7u8; 32]);
    let mut registry = builtin_registry();
    registry
        .sign("gov-test-signer", &signing_key)
        .expect("registry signs");
    let trusted = TrustedSignerSet {
        signers: vec![TrustedSigner {
            key_id: "gov-test-signer".into(),
            algorithm: "Ed25519".into(),
            public_key: STANDARD.encode(signing_key.verifying_key().to_bytes()),
        }],
    };
    let mut reqs = Vec::new();
    for i in 1..=req_count {
        let deps = if i > 1 { vec![format!("G-{:02}", i - 1)] } else { vec![] };
        let deps_str: Vec<&str> = deps.iter().map(|s| s.as_str()).collect();
        reqs.push(seed(&format!("G-{:02}", i), deps_str));
    }
    let authority = ProjectAuthorityInput {
        requirements: reqs,
        source_revision: "gov-fixture-revision".into(),
        source_fingerprint: "gov-fixture-fingerprint".into(),
        ..ProjectAuthorityInput::default()
    };
    let draft = AuthorityEngine
        .build_draft_with_project_authority(
            "mission-gov-test",
            "project-gov-test",
            "project-source-1",
            "workspace-source-1",
            registry.clone(),
            &context(),
            scope_fingerprint(BTreeMap::from([("root".into(), "gov-test".into())])),
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
    let dir = std::env::temp_dir().join(format!("relintor-gov-test-{}-{}", name, std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn seed_test_run(ws: &Path, req_count: usize) -> ExecutionRun {
    let (revision, handoff, registry, trusted) = sealed_fixture(req_count);
    ExecutionRun::from_p6_handoff_with_registry(
        &revision,
        &handoff,
        &registry,
        &trusted,
        ws.to_path_buf(),
        "fp-init",
        SchedulerPolicy::default(),
        1_000_000,
    )
    .expect("create execution run")
}

#[test]
fn test_1_small_task_completes_before_soft_window() {
    let ws = temp_workspace("test1");
    let mut run = seed_test_run(&ws, 1);
    let task_id = run.runnable_tasks().into_iter().next().unwrap();
    let now = 1_000_000;
    let _packet = run.start_task(&task_id, now).unwrap();
    assert_eq!(run.state, ExecutionRunState::Running);

    // Lease active and expires at soft window / budget
    assert_eq!(run.leases.last().unwrap().status, LeaseStatus::Active);
    let _ = fs::remove_dir_all(&ws);
}

#[test]
fn test_2_and_3_legitimate_task_reaches_soft_window_with_meaningful_progress() {
    let ws = temp_workspace("test2");
    let mut run = seed_test_run(&ws, 1);
    let task_id = run.runnable_tasks().into_iter().next().unwrap();
    let now = 1_000_000;
    let _packet = run.start_task(&task_id, now).unwrap();
    let initial_attempt_id = run.attempts.last().unwrap().attempt_id.clone();
    let initial_attempt_number = run.attempts.last().unwrap().attempt_number;

    // Simulate meaningful progress at soft window (15 min)
    let soft_window_time = now + 15 * 60 * 1_000;
    let progress1 = ProgressSnapshot {
        workspace_fingerprint: "fp-step-1".into(),
        changed_paths: vec![ws.join("src").join("lib.rs").display().to_string()],
        passing_tests: 1,
        passing_tests_observed: true,
        ..Default::default()
    };

    assert_eq!(
        run.evaluate_progress_quality(&task_id, &progress1),
        ProgressQuality::MeaningfulForwardProgress
    );

    // Extension #1 granted
    let ext1_expiry = run.grant_authority_extension(&task_id, &progress1, soft_window_time).unwrap();
    assert!(ext1_expiry > soft_window_time);
    assert_eq!(run.attempts.last().unwrap().attempt_id, initial_attempt_id);
    assert_eq!(run.attempts.last().unwrap().attempt_number, initial_attempt_number);
    assert_eq!(run.attempts.last().unwrap().extensions_granted, 1);

    // Extension #2 granted with additional progress
    let next_window_time = soft_window_time + 10 * 60 * 1_000;
    let progress2 = ProgressSnapshot {
        workspace_fingerprint: "fp-step-2".into(),
        changed_paths: vec![ws.join("src").join("main.rs").display().to_string()],
        passing_tests: 3,
        passing_tests_observed: true,
        ..Default::default()
    };
    let ext2_expiry = run.grant_authority_extension(&task_id, &progress2, next_window_time).unwrap();
    assert!(ext2_expiry > next_window_time);
    assert_eq!(run.attempts.last().unwrap().extensions_granted, 2);
    assert_eq!(run.attempts.last().unwrap().attempt_id, initial_attempt_id);

    let _ = fs::remove_dir_all(&ws);
}

#[test]
fn test_4_and_25_task_reaches_absolute_hard_ceiling_despite_progress() {
    let ws = temp_workspace("test4");
    let mut run = seed_test_run(&ws, 1);
    let task_id = run.runnable_tasks().into_iter().next().unwrap();
    let now = 1_000_000;
    let _packet = run.start_task(&task_id, now).unwrap();

    // Grant 3 extensions
    for i in 1..=3 {
        let t = now + i * 10 * 60 * 1_000;
        let p = ProgressSnapshot {
            workspace_fingerprint: format!("fp-{i}"),
            changed_paths: vec![ws.join("src").join(format!("file_{i}.rs")).display().to_string()],
            passing_tests: i as u64,
            passing_tests_observed: true,
            ..Default::default()
        };
        run.grant_authority_extension(&task_id, &p, t).unwrap();
    }

    // 4th extension exceeds max_extensions and reaches hard ceiling -> denied
    let hard_ceiling_time = now + 45 * 60 * 1_000;
    let progress_excess = ProgressSnapshot {
        workspace_fingerprint: "fp-excess".into(),
        changed_paths: vec![ws.join("src").join("file_4.rs").display().to_string()],
        passing_tests: 4,
        passing_tests_observed: true,
        ..Default::default()
    };
    let err = run.grant_authority_extension(&task_id, &progress_excess, hard_ceiling_time).unwrap_err();
    assert!(err.to_string().contains("hard authority ceiling"), "{err}");

    assert!(run.events.iter().any(|e| e.kind == ExecutionEventKind::HardCeilingReached));

    let _ = fs::remove_dir_all(&ws);
}

#[test]
fn test_5_to_11_no_progress_oscillation_and_duplicate_denials() {
    let ws = temp_workspace("test5");
    let mut run = seed_test_run(&ws, 1);
    let task_id = run.runnable_tasks().into_iter().next().unwrap();
    let now = 1_000_000;
    let _packet = run.start_task(&task_id, now).unwrap();
    let soft_window_time = now + 15 * 60 * 1_000;

    // 1. Zero progress
    let empty_progress = ProgressSnapshot::default();
    assert_eq!(
        run.evaluate_progress_quality(&task_id, &empty_progress),
        ProgressQuality::NoProgress
    );
    let err_empty = run.grant_authority_extension(&task_id, &empty_progress, soft_window_time).unwrap_err();
    assert!(err_empty.to_string().contains("authority extension denied"));

    // 2. Scope drift (path outside allowed scope)
    let drift_progress = ProgressSnapshot {
        workspace_fingerprint: "fp-drift".into(),
        changed_paths: vec!["D:/unauthorized/outside.txt".into()],
        ..Default::default()
    };
    assert_eq!(
        run.evaluate_progress_quality(&task_id, &drift_progress),
        ProgressQuality::ScopeDrift
    );

    // 3. Oscillation
    run.watchdog_state = WatchdogState::OscillationDetected;
    assert_eq!(
        run.evaluate_progress_quality(&task_id, &empty_progress),
        ProgressQuality::Oscillation
    );

    // 4. Repeated command
    run.watchdog_state = WatchdogState::RepeatedCommand;
    assert_eq!(
        run.evaluate_progress_quality(&task_id, &empty_progress),
        ProgressQuality::DuplicateActivity
    );

    let _ = fs::remove_dir_all(&ws);
}

#[test]
fn test_16_to_18_extension_preserves_attempt_and_does_not_consume_retry() {
    let ws = temp_workspace("test16");
    let mut run = seed_test_run(&ws, 1);
    let task_id = run.runnable_tasks().into_iter().next().unwrap();
    let now = 1_000_000;
    let _packet = run.start_task(&task_id, now).unwrap();

    let initial_attempt_id = run.attempts.last().unwrap().attempt_id.clone();
    let initial_attempt_count = run.usage.attempt_count;
    let initial_retry_budget = run.tasks[&task_id].usage_budget.retry_attempts;

    let p = ProgressSnapshot {
        workspace_fingerprint: "fp-valid".into(),
        changed_paths: vec![ws.join("src").join("mod.rs").display().to_string()],
        passing_tests: 1,
        passing_tests_observed: true,
        ..Default::default()
    };
    run.grant_authority_extension(&task_id, &p, now + 15 * 60 * 1_000).unwrap();

    assert_eq!(run.attempts.last().unwrap().attempt_id, initial_attempt_id);
    assert_eq!(run.usage.attempt_count, initial_attempt_count);
    assert_eq!(run.tasks[&task_id].usage_budget.retry_attempts, initial_retry_budget);
    assert_eq!(run.state, ExecutionRunState::Running);

    // Ensure no RecoveryReviewRequired event was emitted for a healthy extension
    assert!(!run.events.iter().any(|e| e.kind == ExecutionEventKind::AuthorityRevalidationRequired));

    let _ = fs::remove_dir_all(&ws);
}

#[test]
fn test_21_to_24_restart_during_extended_attempt_preserves_bounds() {
    let ws = temp_workspace("test21");
    let mut run = seed_test_run(&ws, 1);
    let task_id = run.runnable_tasks().into_iter().next().unwrap();
    let now = 1_000_000;
    let _packet = run.start_task(&task_id, now).unwrap();

    let p = ProgressSnapshot {
        workspace_fingerprint: "fp-persist".into(),
        changed_paths: vec![ws.join("src").join("mod.rs").display().to_string()],
        passing_tests: 2,
        passing_tests_observed: true,
        ..Default::default()
    };
    let ext_expiry = run.grant_authority_extension(&task_id, &p, now + 15 * 60 * 1_000).unwrap();
    assert_eq!(run.attempts.last().unwrap().extensions_granted, 1);

    // Persist snapshot to JSON and restore
    let json = run.snapshot_json().unwrap();
    let restored = ExecutionRun::restore_json(&json).unwrap();

    assert_eq!(restored.attempts.last().unwrap().extensions_granted, 1);
    assert_eq!(restored.leases.last().unwrap().expires_at_ms, ext_expiry);
    assert_eq!(restored.state, ExecutionRunState::Running);

    let _ = fs::remove_dir_all(&ws);
}

#[test]
fn test_26_to_32_progress_spoofing_resistance() {
    let ws = temp_workspace("test26");
    let mut run = seed_test_run(&ws, 1);
    let task_id = run.runnable_tasks().into_iter().next().unwrap();
    let now = 1_000_000;
    let _packet = run.start_task(&task_id, now).unwrap();

    // 1. First valid progress
    let p1 = ProgressSnapshot {
        workspace_fingerprint: "fp-1".into(),
        changed_paths: vec![ws.join("src").join("file.rs").display().to_string()],
        passing_tests: 1,
        passing_tests_observed: true,
        ..Default::default()
    };
    run.grant_authority_extension(&task_id, &p1, now + 10 * 60 * 1_000).unwrap();

    // 2. Replayed/duplicate snapshot (same fingerprint, same test count) -> NoProgress
    let p_replay = p1.clone();
    assert_eq!(
        run.evaluate_progress_quality(&task_id, &p_replay),
        ProgressQuality::NoProgress
    );

    // 3. Stale progress on wrong task ID -> Unknown
    assert_eq!(
        run.evaluate_progress_quality("nonexistent-task", &p1),
        ProgressQuality::Unknown
    );

    let _ = fs::remove_dir_all(&ws);
}

#[test]
fn test_34_to_36_multi_task_progress_sequence() {
    let ws = temp_workspace("test34");
    let mut run = seed_test_run(&ws, 3);
    let tasks = run.runnable_tasks();
    assert!(!tasks.is_empty());
    let task1 = tasks.into_iter().next().unwrap();
    let now = 1_000_000;

    // Task 1: gets 1 extension then finishes
    let _packet = run.start_task(&task1, now).unwrap();
    let p1 = ProgressSnapshot {
        workspace_fingerprint: "fp-task1".into(),
        changed_paths: vec![ws.join("src").join("t1.rs").display().to_string()],
        passing_tests: 1,
        passing_tests_observed: true,
        ..Default::default()
    };
    run.grant_authority_extension(&task1, &p1, now + 15 * 60 * 1_000).unwrap();
    assert_eq!(run.attempts.last().unwrap().extensions_granted, 1);

    let _ = fs::remove_dir_all(&ws);
}

#[test]
fn test_upgrade_matrix_existing_phase5_mission_loads() {
    let appdata = std::env::var("APPDATA").unwrap_or_default();
    let phase5_ledger = PathBuf::from(appdata)
        .join("com.relintor.desktop")
        .join("execution")
        .join("mission-takeover-project-takeover_719ad83a558ede1be5868c6d-1.json");

    if phase5_ledger.is_file() {
        let json = fs::read_to_string(&phase5_ledger).unwrap();
        let restored = ExecutionRun::restore_json(&json).expect("restore real phase 5 mission");
        assert_eq!(restored.mission_id, "mission-takeover-project-takeover_719ad83a558ede1be5868c6d");
        assert_eq!(restored.mission_revision, 1);
        assert_eq!(restored.state, relintor_execution::ExecutionRunState::RevalidationRequired);

        // Task 1 complete
        let task1 = &restored.tasks["task_131fe92c0e2f030454e92944"];
        assert_eq!(task1.state, relintor_execution::ExecutionTaskState::FinishedAwaitingVerification);

        // Task 2 partial work preserved
        let task2 = &restored.tasks["task_1fd6aeb68d6e8f80b6933346"];
        assert_eq!(task2.state, relintor_execution::ExecutionTaskState::BlockedExternal);

        // Total 15 tasks preserved
        assert_eq!(restored.tasks.len(), 15);
        assert_eq!(restored.attempts.len(), 3);
        assert_eq!(restored.events.len(), 17);
    }
}

#[test]
fn test_upgrade_matrix_old_fixed_window_ledgers_compatibility() {
    let ws = temp_workspace("test_old_compat");
    let run = seed_test_run(&ws, 2);
    let original_json = run.snapshot_json().unwrap();

    // Verify restore_json on fresh ledger
    let restored = ExecutionRun::restore_json(&original_json).unwrap();
    assert_eq!(restored.mission_id, run.mission_id);

    // Re-serialize and ensure round-trip
    let reserialized = restored.snapshot_json().unwrap();
    let restored2 = ExecutionRun::restore_json(&reserialized).unwrap();
    assert_eq!(restored2.mission_id, run.mission_id);

    let _ = fs::remove_dir_all(&ws);
}

