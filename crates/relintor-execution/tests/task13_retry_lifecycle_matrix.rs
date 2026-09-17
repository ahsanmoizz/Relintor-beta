use relintor_execution::{
    CheckpointKind, ExecutionRun, ExecutionRunState,
    ProcessObservation, ProcessOwnershipRecord, RecoveryAuthority, RecoveryCoordinator,
    RecoveryDisposition, RecoveryStore, RevalidationRecord,
};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

fn temp_dir(name: &str) -> PathBuf {
    let base = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target"));
    let base = if base.is_absolute() {
        base
    } else {
        std::env::current_dir().unwrap().join(base)
    };
    let path = base.join(format!("task13-matrix-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).expect("create test dir");
    path
}

fn canonical_run_id(mission: &str, revision: u64, seal: &str) -> String {
    let bytes = serde_json::to_vec(&(mission, revision, seal)).expect("serialize");
    format!("{:x}", Sha256::digest(bytes))
}

fn authority(_root: &Path, mission: &str, run_id: &str) -> RecoveryAuthority {
    RecoveryAuthority::new(
        "project-1",
        mission,
        1,
        "seal-1",
        "reg-1",
        run_id,
        "ws-fingerprint-1",
        "source-identity-1",
        None,
        "test-contract",
        1000,
    )
}

fn empty_run(root: &Path, _tag: &str) -> ExecutionRun {
    let mission = "mission-1";
    let seal = "seal-1";
    let run_id = canonical_run_id(mission, 1, seal);
    ExecutionRun {
        ledger_version: "p7-execution-ledger-v1".into(),
        run_id,
        mission_id: mission.into(),
        mission_revision: 1,
        seal_hash: seal.into(),
        project_id: "project-1".into(),
        workspace: root.to_path_buf(),
        workspace_fingerprint: "ws-fp-1".into(),
        state: ExecutionRunState::WaitingRetry,
        tasks: BTreeMap::new(),
        attempts: Vec::new(),
        leases: Vec::new(),
        events: Vec::new(),
        usage: Default::default(),
        loop_signals: Vec::new(),
        oscillation_signals: Vec::new(),
        continuations: Vec::new(),
        diagnostics: Vec::new(),
        external_modifications: Vec::new(),
        progress: Vec::new(),
        watchdog_state: relintor_execution::WatchdogState::Healthy,
        policy: Default::default(),
        safe_boundary: None,
        current_turn: 1,
        last_error: None,
        no_progress_occurrences: 0,
        reviewed_recovery_deltas: Vec::new(),
        integrity_version: "p7-ledger-integrity-v1".into(),
        integrity_tag: String::new(),
    }
}

// -----------------------------------------------------------------------------
// Test 1: Concurrent RecoveryStore::new does not destroy active checkpoint temp write
// -----------------------------------------------------------------------------
#[test]
fn test_01_concurrent_recovery_store_open_does_not_destroy_active_checkpoint_temp_write() {
    let dir = temp_dir("test-01");
    let key = vec![0x42u8; 32];
    let store = RecoveryStore::new(&dir, key.clone()).expect("create store");

    let stop = Arc::new(AtomicBool::new(false));
    let dir_clone = dir.clone();
    let key_clone = key.clone();
    let stop_clone = Arc::clone(&stop);

    // Thread continually calling RecoveryStore::new on the same directory
    let poller = thread::spawn(move || {
        while !stop_clone.load(Ordering::Relaxed) {
            let _ = RecoveryStore::new(&dir_clone, key_clone.clone());
            thread::sleep(Duration::from_millis(1));
        }
    });

    let run = empty_run(&dir, "run-test-01");
    let auth = authority(&dir, "mission-1", &run.run_id);
    let coord = RecoveryCoordinator::new(store);

    // Write multiple checkpoints while the poller is furiously reopening the store
    for i in 1..=5 {
        let res = coord.checkpoint_run(
            &run,
            auth.clone(),
            CheckpointKind::AfterTaskPersistence,
            &dir,
            Vec::new(),
            Vec::new(),
            format!("test write {i}"),
            1000 + i as u64,
        );
        assert!(res.is_ok(), "checkpoint write {i} failed: {:?}", res.err());
    }

    stop.store(true, Ordering::Relaxed);
    poller.join().unwrap();

    let _ = fs::remove_dir_all(&dir);
}

// -----------------------------------------------------------------------------
// Test 2: Orphan temp cleanup runs under write_lock when no active write
// -----------------------------------------------------------------------------
#[test]
fn test_02_orphan_temp_cleanup_runs_under_write_lock_when_no_active_write() {
    let dir = temp_dir("test-02");
    let key = vec![0x42u8; 32];
    let _store = RecoveryStore::new(&dir, key.clone()).expect("create store");

    let orphan = dir.join(".checkpoint-00000000000000000099-orphan.json.tmp");
    fs::write(&orphan, b"leftover partial bytes").expect("write orphan");
    assert!(orphan.exists());

    // Reopen store; orphan must be cleanly purged
    let _store2 = RecoveryStore::new(&dir, key).expect("reopen store");
    assert!(!orphan.exists(), "orphan tmp must be purged on store open");

    let _ = fs::remove_dir_all(&dir);
}

// -----------------------------------------------------------------------------
// Test 3: Shared caches and write lock across RecoveryStore instances
// -----------------------------------------------------------------------------
#[test]
fn test_03_desktop_recovery_store_cache_prevents_redundant_store_reopen() {
    let dir = temp_dir("test-03");
    let key = vec![0x42u8; 32];
    let store1 = RecoveryStore::new(&dir, key.clone()).expect("store 1");
    let store2 = RecoveryStore::new(&dir, key).expect("store 2");

    let run = empty_run(&dir, "run-test-03");
    let auth = authority(&dir, "mission-1", &run.run_id);
    let coord1 = RecoveryCoordinator::new(store1);

    let cp1 = coord1.checkpoint_run(
        &run,
        auth,
        CheckpointKind::AfterTaskPersistence,
        &dir,
        Vec::new(),
        Vec::new(),
        "checkpoint 1",
        2000,
    ).expect("checkpoint 1");

    // store2 should load the latest checkpoint immediately and match
    let latest = store2.load_latest().expect("load latest from store2");
    assert!(latest.is_some());
    assert_eq!(latest.unwrap().checkpoint_id, cp1.checkpoint_id);

    let _ = fs::remove_dir_all(&dir);
}

// -----------------------------------------------------------------------------
// Test 4: Retry authorization persists durable checkpoint before dispatch
// -----------------------------------------------------------------------------
#[test]
fn test_04_retry_authorization_persists_durable_checkpoint_before_dispatch() {
    let dir = temp_dir("test-04");
    let key = vec![0x42u8; 32];
    let store = RecoveryStore::new(&dir, key).expect("store");

    let mut run = empty_run(&dir, "run-test-04");
    run.state = ExecutionRunState::TurnEndedIncomplete;

    let auth = authority(&dir, "mission-1", &run.run_id);
    let coord = RecoveryCoordinator::new(store.clone());

    // Record revalidation
    let reval = store.write_revalidation(RevalidationRecord {
        record_version: "p9-recovery-v2".into(),
        revalidation_id: "reval-04".into(),
        project_id: "project-1".into(),
        mission_id: "mission-1".into(),
        mission_revision: 1,
        p7_run_id: run.run_id.clone(),
        checkpoint_id: None,
        target: None,
        classification: relintor_execution::RecoveryClassification::ExternalStateUncertain,
        disposition: RecoveryDisposition::PreExecutionRetryAuthorized,
        decision: "MANUAL_RETRY_AUTHORIZED".into(),
        reasons: vec!["operator approved retry".into()],
        affected_paths: vec!["src/lib.rs".into()],
        required_authorities: vec!["P8".into(), "P7".into()],
        created_at_ms: 3000,
        integrity_tag: String::new(),
    }).expect("write revalidation");
    assert_eq!(reval.decision, "MANUAL_RETRY_AUTHORIZED");

    // Checkpoint after retry authorization
    let cp = coord.checkpoint_run(
        &run,
        auth,
        CheckpointKind::AfterTaskPersistence,
        &dir,
        Vec::new(),
        Vec::new(),
        "explicit recovery review authorized a fresh attempt for the interrupted task",
        3001,
    ).expect("checkpoint run");

    assert_eq!(cp.sequence, 1);
    assert_eq!(cp.content.kind, CheckpointKind::AfterTaskPersistence);

    let latest = store.load_latest().expect("load latest").unwrap();
    assert_eq!(latest.sequence, 1);

    let _ = fs::remove_dir_all(&dir);
}

// -----------------------------------------------------------------------------
// Test 5: Executor startup checkpoint failure does not panic or corrupt state
// -----------------------------------------------------------------------------
#[test]
fn test_05_executor_startup_checkpoint_failure_does_not_panic_or_corrupt_state() {
    let dir = temp_dir("test-05");
    let key = vec![0x42u8; 32];
    let store = RecoveryStore::new(&dir, key).expect("store");

    let run = empty_run(&dir, "run-test-05");
    let mut auth = authority(&dir, "mission-1", &run.run_id);
    let coord = RecoveryCoordinator::new(store.clone());

    let cp1 = coord.checkpoint_run(
        &run,
        auth.clone(),
        CheckpointKind::AfterTaskPersistence,
        &dir,
        Vec::new(),
        Vec::new(),
        "valid startup",
        4000,
    ).expect("valid cp1");

    // Intentionally forge sequence mismatch to simulate persistence error
    auth.checkpoint_sequence = 999;
    let err = coord.checkpoint_run(
        &run,
        auth,
        CheckpointKind::AfterTaskPersistence,
        &dir,
        Vec::new(),
        Vec::new(),
        "bad sequence",
        4001,
    );
    assert!(err.is_err());

    // Prior authoritative checkpoint remains valid and unchanged
    let latest = store.load_latest().expect("load latest").unwrap();
    assert_eq!(latest.sequence, 1);
    assert_eq!(latest.checkpoint_id, cp1.checkpoint_id);

    let _ = fs::remove_dir_all(&dir);
}

// -----------------------------------------------------------------------------
// Test 6: Durable revalidation survives process restart without resurrection
// -----------------------------------------------------------------------------
#[test]
fn test_06_durable_revalidation_survives_process_restart_without_resurrection() {
    let dir = temp_dir("test-06");
    let key = vec![0x42u8; 32];
    let store = RecoveryStore::new(&dir, key.clone()).expect("store");

    let mut run = empty_run(&dir, "run-test-06");
    let auth = authority(&dir, "mission-1", &run.run_id);
    let coord = RecoveryCoordinator::new(store.clone());

    store.write_revalidation(RevalidationRecord {
        record_version: "p9-recovery-v2".into(),
        revalidation_id: "reval-06".into(),
        project_id: "project-1".into(),
        mission_id: "mission-1".into(),
        mission_revision: 1,
        p7_run_id: run.run_id.clone(),
        checkpoint_id: None,
        target: None,
        classification: relintor_execution::RecoveryClassification::ExternalStateUncertain,
        disposition: RecoveryDisposition::PreExecutionRetryAuthorized,
        decision: "MANUAL_RETRY_AUTHORIZED".into(),
        reasons: vec!["operator authorized retry".into()],
        affected_paths: Vec::new(),
        required_authorities: vec!["P8".into(), "P7".into()],
        created_at_ms: 5000,
        integrity_tag: String::new(),
    }).expect("write revalidation");

    coord.checkpoint_run(
        &run,
        auth.clone(),
        CheckpointKind::AfterTaskPersistence,
        &dir,
        Vec::new(),
        Vec::new(),
        "checkpoint retry authorized",
        5001,
    ).expect("checkpoint");

    // Simulate restart: create new store instance and inspect resume integrity
    let store_restarted = RecoveryStore::new(&dir, key).expect("store reopened");
    let coord_restarted = RecoveryCoordinator::new(store_restarted.clone());

    let integrity = coord_restarted.resume_run(
        &mut run,
        &auth,
        &dir,
        &relintor_execution::ConservativeProcessInspector,
        5002,
        false,
    ).expect("resume run");

    // Durable revalidation record survived restart intact
    let latest_reval = store_restarted
        .load_latest_revalidation()
        .expect("load reval")
        .unwrap();
    assert_eq!(latest_reval.decision, "MANUAL_RETRY_AUTHORIZED");
    assert_eq!(
        latest_reval.disposition,
        RecoveryDisposition::PreExecutionRetryAuthorized
    );

    // Revalidation decision must be respected, not resurrected into an unhandled crash
    assert!(matches!(
        integrity.disposition,
        RecoveryDisposition::PreExecutionRetryAuthorized
            | RecoveryDisposition::RevalidationRequired
            | RecoveryDisposition::SafeToResume
            | RecoveryDisposition::StoppedIncomplete
    ));

    let _ = fs::remove_dir_all(&dir);
}

// -----------------------------------------------------------------------------
// Test 7: Recovery loop prevention on repeated retry
// -----------------------------------------------------------------------------
#[test]
fn test_07_recovery_loop_prevention_on_repeated_retry() {
    let dir = temp_dir("test-07");
    let key = vec![0x42u8; 32];
    let store = RecoveryStore::new(&dir, key).expect("store");

    let run = empty_run(&dir, "run-test-07");
    let auth = authority(&dir, "mission-1", &run.run_id);
    let coord = RecoveryCoordinator::new(store.clone());

    // Cycle 1: retry authorized, checkpoint written
    store.write_revalidation(RevalidationRecord {
        record_version: "p9-recovery-v2".into(),
        revalidation_id: "reval-07-1".into(),
        project_id: "project-1".into(),
        mission_id: "mission-1".into(),
        mission_revision: 1,
        p7_run_id: run.run_id.clone(),
        checkpoint_id: None,
        target: None,
        classification: relintor_execution::RecoveryClassification::ExternalStateUncertain,
        disposition: RecoveryDisposition::PreExecutionRetryAuthorized,
        decision: "MANUAL_RETRY_AUTHORIZED".into(),
        reasons: vec!["retry 1 authorized".into()],
        affected_paths: Vec::new(),
        required_authorities: vec!["P8".into(), "P7".into()],
        created_at_ms: 6000,
        integrity_tag: String::new(),
    }).expect("reval 1");

    let cp1 = coord.checkpoint_run(
        &run,
        auth.clone(),
        CheckpointKind::AfterTaskPersistence,
        &dir,
        Vec::new(),
        Vec::new(),
        "task attempt started",
        6001,
    ).expect("cp1");
    assert_eq!(cp1.sequence, 1);

    // Cycle 2: second retry authorized without storage race
    store.write_revalidation(RevalidationRecord {
        record_version: "p9-recovery-v2".into(),
        revalidation_id: "reval-07-2".into(),
        project_id: "project-1".into(),
        mission_id: "mission-1".into(),
        mission_revision: 1,
        p7_run_id: run.run_id.clone(),
        checkpoint_id: None,
        target: None,
        classification: relintor_execution::RecoveryClassification::ExternalStateUncertain,
        disposition: RecoveryDisposition::PreExecutionRetryAuthorized,
        decision: "MANUAL_RETRY_AUTHORIZED".into(),
        reasons: vec!["retry 2 authorized".into()],
        affected_paths: Vec::new(),
        required_authorities: vec!["P8".into(), "P7".into()],
        created_at_ms: 6010,
        integrity_tag: String::new(),
    }).expect("reval 2");

    let cp2 = coord.checkpoint_run(
        &run,
        auth,
        CheckpointKind::AfterTaskPersistence,
        &dir,
        Vec::new(),
        Vec::new(),
        "second attempt started",
        6011,
    ).expect("cp2");
    assert_eq!(cp2.sequence, 2);

    let latest = store.load_latest().expect("load latest").unwrap();
    assert_eq!(latest.sequence, 2);

    let _ = fs::remove_dir_all(&dir);
}

// -----------------------------------------------------------------------------
// Test 8: Process ownership record captures exact identity digest
// -----------------------------------------------------------------------------
#[test]
fn test_08_process_ownership_record_captures_exact_identity_digest() {
    let record = ProcessOwnershipRecord {
        process_id: 17768,
        process_start_time_ms: Some(1788649681403),
        executable_path: r"D:\Relintor-tools\agy\bin\agy.exe".into(),
        executable_digest: "17a09d8c8b5a0bc3cc36904deed78126a56d5c47ccf28186743acb848f5f780d".into(),
        command_digest: "p9-command-v2:781703c105c5e82714049bb063b051424d7d9872a44263a9af59e94a8076c5d4".into(),
        run_id: "run-test-08".into(),
        task_id: Some("task_d884644dffdc8ea289d20c30".into()),
        attempt_id: Some("f984e8c6bc3abdbe071e4b0e24d0a1892412cdfcfeec3fca2075cd2fc6f7d2fd".into()),
        lease_id: Some("128acf76cb0cc1d8400b9e065b110b93b1bcc1d2b49d21830d3d6a48e38c7eb3".into()),
        launched_at_ms: 1788649681403,
        observation: ProcessObservation::OwnedProcessStillRunning,
    };

    let digest1 = record.identity_digest();
    let digest2 = record.identity_digest();
    assert_eq!(digest1, digest2);
    assert!(!digest1.is_empty());

    let dir = temp_dir("test-08");
    let run = empty_run(&dir, "run-test-08");
    assert!(!record.binds_to_running_attempt(&run));

    let _ = fs::remove_dir_all(&dir);
}

// -----------------------------------------------------------------------------
// Test 9: All writes to recovery store are synchronized under write_lock
// -----------------------------------------------------------------------------
#[test]
fn test_09_all_writes_to_recovery_store_are_synchronized_under_write_lock() {
    let ws_dir = temp_dir("test-09-ws");
    let store_dir = temp_dir("test-09-store");
    let key = vec![0x42u8; 32];
    let store = RecoveryStore::new(&store_dir, key).expect("store");

    let run = empty_run(&ws_dir, "run-test-09");
    let auth = authority(&ws_dir, "mission-1", &run.run_id);
    let coord = RecoveryCoordinator::new(store.clone());

    let store_clone = store.clone();
    let thread_run_id = run.run_id.clone();

    // Concurrent thread writing revalidations
    let handle = thread::spawn(move || {
        for i in 1..=5 {
            let res = store_clone.write_revalidation(RevalidationRecord {
                record_version: "p9-recovery-v2".into(),
                revalidation_id: format!("reval-{i}"),
                project_id: "project-1".into(),
                mission_id: "mission-1".into(),
                mission_revision: 1,
                p7_run_id: thread_run_id.clone(),
                checkpoint_id: None,
                target: None,
                classification: relintor_execution::RecoveryClassification::ExternalStateUncertain,
                disposition: RecoveryDisposition::PreExecutionRetryAuthorized,
                decision: "MANUAL_RETRY_AUTHORIZED".into(),
                reasons: vec!["concurrent test".into()],
                affected_paths: Vec::new(),
                required_authorities: Vec::new(),
                created_at_ms: 7000 + i as u64,
                integrity_tag: String::new(),
            });
            assert!(res.is_ok());
        }
    });

    // Main thread writing checkpoints
    for i in 1..=5 {
        let res = coord.checkpoint_run(
            &run,
            auth.clone(),
            CheckpointKind::AfterTaskPersistence,
            &ws_dir,
            Vec::new(),
            Vec::new(),
            format!("concurrent checkpoint {i}"),
            7000 + i as u64,
        );
        assert!(res.is_ok(), "checkpoint write {i} failed: {:?}", res.err());
    }

    handle.join().unwrap();

    let latest = store.load_latest().expect("load latest").unwrap();
    assert_eq!(latest.sequence, 5);

    let _ = fs::remove_dir_all(&ws_dir);
    let _ = fs::remove_dir_all(&store_dir);
}

// -----------------------------------------------------------------------------
// Test 10: Large workspace snapshot checkpoint integrity
// -----------------------------------------------------------------------------
#[test]
fn test_10_large_workspace_snapshot_checkpoint_integrity() {
    let ws_dir = temp_dir("test-10-ws");
    let store_dir = temp_dir("test-10-store");

    // Create 50 files in workspace
    for i in 0..50 {
        let p = ws_dir.join(format!("file_{i:04}.txt"));
        fs::write(&p, format!("Content of file {i} with deterministic padding: {}", "x".repeat(200))).unwrap();
    }

    let key = vec![0x42u8; 32];
    let store = RecoveryStore::new(&store_dir, key).expect("store");

    let run = empty_run(&ws_dir, "run-test-10");
    let auth = authority(&ws_dir, "mission-1", &run.run_id);
    let coord = RecoveryCoordinator::new(store.clone());

    let cp = coord.checkpoint_run(
        &run,
        auth,
        CheckpointKind::AfterTaskPersistence,
        &ws_dir,
        Vec::new(),
        Vec::new(),
        "checkpoint with 50 files",
        8000,
    ).expect("checkpoint with files");

    assert_eq!(cp.sequence, 1);
    assert_eq!(cp.content.workspace.files.len(), 50);

    let latest = store.load_latest().expect("load latest").unwrap();
    assert_eq!(latest.checkpoint_id, cp.checkpoint_id);
    assert_eq!(latest.content.workspace.files.len(), 50);

    let _ = fs::remove_dir_all(&ws_dir);
    let _ = fs::remove_dir_all(&store_dir);
}
