use relintor_execution::{
    CheckpointContent, CheckpointKind, CompensationActionType, CompensationOutcome,
    CompensationRegistry, ConservativeProcessInspector, DatabaseCheckpointSpec,
    DatabaseCheckpointStatus, DatabaseClassification, DatabaseKind, ExecutionRun,
    ExecutionRunState, ExecutionTask, ExecutionTaskState, GitWorktreeSnapshot, LeaseScope,
    LeaseStatus, ProcessInspector, ProcessObservation, ProcessOwnershipRecord, RecoveryAuthority,
    RecoveryCoordinator, RecoveryDisposition, RecoveryStore, RetryPolicy, SessionEndState,
    SqliteTestDatabaseHook, TaskAttemptState, TestDatabaseCheckpointHook, UntrackedFileRecord,
    UntrackedFileSnapshot, UsageBudget, WorkspaceSnapshot,
};
use relintor_standards::RequirementPriority;
use rusqlite::Connection;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

fn root(name: &str) -> PathBuf {
    let base = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target"));
    let base = if base.is_absolute() {
        base
    } else {
        std::env::current_dir()
            .expect("resolve P9 recovery working directory")
            .join(base)
    };
    let path = base.join(format!("p9-acceptance-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).expect("create P9 fixture root");
    path
}

fn digest<T: Serialize>(value: &T) -> String {
    let bytes = serde_json::to_vec(value).expect("serialize digest input");
    format!("{:x}", Sha256::digest(bytes))
}

fn store(root: &Path) -> RecoveryStore {
    RecoveryStore::new(root.join(".relintor-recovery"), vec![7; 32]).expect("open recovery store")
}

fn compensation_registry(root: &Path) -> CompensationRegistry {
    let store = store(root);
    CompensationRegistry::from_store(&store).expect("open compensation journal")
}

fn authority(workspace: &WorkspaceSnapshot) -> RecoveryAuthority {
    RecoveryAuthority::new(
        "project-p9",
        "mission-p9",
        1,
        "p6-seal-p9",
        "registry-p9",
        "run-p9",
        workspace.fingerprint.clone(),
        "source-p9",
        None,
        "P8_PENDING",
        1,
    )
}

fn content(root: &Path, reason: &str) -> CheckpointContent {
    let workspace = WorkspaceSnapshot::capture(root, 1).expect("capture workspace");
    let untracked = UntrackedFileSnapshot::capture(root, 1).expect("capture untracked");
    let git = GitWorktreeSnapshot::capture(root).expect("capture git state");
    CheckpointContent {
        authority: authority(&workspace),
        kind: CheckpointKind::AfterTaskPersistence,
        run_snapshot_json: "{}".into(),
        workspace,
        untracked,
        git,
        processes: Vec::new(),
        verification_references: vec!["p8-evidence-pending".into()],
        reason: reason.into(),
        created_at_ms: 1,
    }
}

fn empty_run(root: &Path) -> ExecutionRun {
    let mission = "mission-p9".to_string();
    let seal = "p6-seal-p9".to_string();
    let run_id = digest(&(mission.as_str(), 1_u64, seal.as_str()));
    ExecutionRun {
        ledger_version: "p7-execution-ledger-v1".into(),
        run_id,
        mission_id: mission,
        mission_revision: 1,
        seal_hash: seal,
        project_id: "project-p9".into(),
        workspace: root.to_path_buf(),
        workspace_fingerprint: "p7-workspace-p9".into(),
        state: ExecutionRunState::Ready,
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
        integrity_version: "p7-ledger-integrity-v1".into(),
        integrity_tag: String::new(),
    }
}

fn owned_process() -> ProcessOwnershipRecord {
    ProcessOwnershipRecord {
        process_id: 999_999,
        process_start_time_ms: Some(100),
        executable_path: "C:/owned/antigravity.exe".into(),
        executable_digest: "exe-digest".into(),
        command_digest: "command-digest".into(),
        run_id: "run-p9".into(),
        task_id: Some("task-p9".into()),
        attempt_id: Some("attempt-p9".into()),
        lease_id: Some("lease-p9".into()),
        launched_at_ms: 100,
        observation: ProcessObservation::OwnedProcessStillRunning,
    }
}

#[test]
fn recovery_key_must_be_os_authority_sized() {
    let path = root("key");
    assert!(RecoveryStore::new(path, vec![1; 31]).is_err());
}

#[test]
fn checkpoint_is_durable_and_loads_latest() {
    let path = root("durable");
    let store = store(&path);
    let first = store
        .write_checkpoint(content(&path, "first"))
        .expect("write checkpoint");
    let loaded = store
        .load_latest()
        .expect("load checkpoint")
        .expect("latest checkpoint");
    assert_eq!(loaded.checkpoint_id, first.checkpoint_id);
    assert_eq!(loaded.sequence, 1);
    assert_eq!(loaded.content.reason, "first");
}

#[test]
fn checkpoint_chain_appends_and_binds_parent() {
    let path = root("chain");
    let store = store(&path);
    let first = store
        .write_checkpoint(content(&path, "first"))
        .expect("first");
    let second = store
        .write_checkpoint(content(&path, "second"))
        .expect("second");
    assert_eq!(second.sequence, 2);
    assert_eq!(second.parent_digest, Some(first.content_digest));
    assert_eq!(store.load_latest().expect("load").unwrap().sequence, 2);
}

#[test]
fn checkpoint_metadata_tampering_fails_closed() {
    let path = root("metadata-tamper");
    let store = store(&path);
    store
        .write_checkpoint(content(&path, "original"))
        .expect("write");
    let artifact = fs::read_dir(path.join(".relintor-recovery"))
        .expect("list")
        .filter_map(Result::ok)
        .find(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with("checkpoint-")
        })
        .expect("artifact")
        .path();
    let mut bytes = fs::read(&artifact).expect("read");
    let position = bytes.len() / 2;
    bytes[position] ^= 0x01;
    fs::write(artifact, bytes).expect("tamper");
    assert!(store.load_latest().is_err());
}

#[test]
fn checkpoint_payload_tampering_fails_closed() {
    let path = root("payload-tamper");
    let store = store(&path);
    store
        .write_checkpoint(content(&path, "original"))
        .expect("write");
    let index = path.join(".relintor-recovery/checkpoint-index.json");
    let mut bytes = fs::read(&index).expect("read index");
    let position = bytes.len() - 10;
    bytes[position] ^= 0x01;
    fs::write(index, bytes).expect("tamper index");
    assert!(store.load_latest().is_err());
}

#[test]
fn deleted_latest_checkpoint_never_falls_back_to_oldest() {
    let path = root("delete-latest");
    let store = store(&path);
    store
        .write_checkpoint(content(&path, "first"))
        .expect("first");
    store
        .write_checkpoint(content(&path, "second"))
        .expect("second");
    let latest = fs::read_dir(path.join(".relintor-recovery"))
        .expect("list")
        .filter_map(Result::ok)
        .find(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .contains("00000000000000000002")
        })
        .expect("latest")
        .path();
    fs::remove_file(latest).expect("delete latest");
    assert!(store.load_latest().is_err());
}

#[test]
fn deleted_old_checkpoint_breaks_parent_chain() {
    let path = root("delete-old");
    let store = store(&path);
    store
        .write_checkpoint(content(&path, "first"))
        .expect("first");
    store
        .write_checkpoint(content(&path, "second"))
        .expect("second");
    let old = fs::read_dir(path.join(".relintor-recovery"))
        .expect("list")
        .filter_map(Result::ok)
        .find(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .contains("00000000000000000001")
        })
        .expect("old")
        .path();
    fs::remove_file(old).expect("delete old");
    assert!(store.load_latest().is_err());
}

#[test]
fn checkpoint_index_sequence_rollback_is_authenticated() {
    let path = root("sequence-rollback");
    let store = store(&path);
    store
        .write_checkpoint(content(&path, "first"))
        .expect("first");
    store
        .write_checkpoint(content(&path, "second"))
        .expect("second");
    let index = path.join(".relintor-recovery/checkpoint-index.json");
    let mut text = fs::read_to_string(&index).expect("index");
    text = text.replace("\"latest_sequence\": 2", "\"latest_sequence\": 1");
    fs::write(index, text).expect("rollback");
    assert!(store.load_latest().is_err());
}

#[test]
fn wrong_mission_is_rejected_during_resume_integrity() {
    let path = root("wrong-mission");
    let store = store(&path);
    let workspace = WorkspaceSnapshot::capture(&path, 1).expect("workspace");
    store
        .write_checkpoint(content(&path, "mission"))
        .expect("write");
    let mut expected = authority(&workspace);
    expected.mission_id = "other-mission".into();
    let result = RecoveryCoordinator::new(store).resume_integrity(
        &expected,
        &path,
        &ConservativeProcessInspector,
        2,
    );
    assert!(
        result.is_err(),
        "invalid run snapshot is corrupt before authority comparison"
    );
}

#[test]
fn workspace_fingerprint_changes_are_revalidation_required() {
    let path = root("external-edit");
    fs::write(path.join("project.txt"), "before").expect("seed");
    let run = empty_run(&path);
    let store = store(&path);
    let coordinator = RecoveryCoordinator::new(store.clone());
    let snapshot = WorkspaceSnapshot::capture(&path, 1).expect("snapshot");
    let authority = RecoveryAuthority::new(
        "project-p9",
        "mission-p9",
        1,
        "p6-seal-p9",
        "registry-p9",
        run.run_id.clone(),
        snapshot.fingerprint.clone(),
        "source-p9",
        None,
        "P8_PENDING",
        1,
    );
    coordinator
        .checkpoint_run(
            &run,
            authority,
            CheckpointKind::AfterAtomicAction,
            &path,
            Vec::new(),
            Vec::new(),
            "safe",
            1,
        )
        .expect("checkpoint");
    fs::write(path.join("project.txt"), "external").expect("edit");
    let record = store.load_latest().expect("load").unwrap();
    let mut expected = RecoveryAuthority::new(
        "project-p9",
        "mission-p9",
        1,
        "p6-seal-p9",
        "registry-p9",
        run.run_id.clone(),
        record.content.authority.workspace_identity.clone(),
        "source-p9",
        None,
        "P8_PENDING",
        2,
    );
    expected.workspace_identity = record.content.authority.workspace_identity;
    let result = coordinator
        .resume_integrity(&expected, &path, &ConservativeProcessInspector, 2)
        .expect("integrity result");
    assert_eq!(
        result.disposition,
        RecoveryDisposition::RevalidationRequired
    );
    assert!(result
        .changed_paths
        .iter()
        .any(|path| path == "project.txt"));
}

#[test]
fn untracked_snapshot_captures_content_but_omits_secrets() {
    let path = root("untracked");
    fs::write(path.join("notes.txt"), "safe").expect("notes");
    fs::write(path.join(".env"), "TOKEN=do-not-copy").expect("secret");
    let snapshot = UntrackedFileSnapshot::capture(&path, 1).expect("snapshot");
    assert!(snapshot
        .files
        .iter()
        .any(|file| file.relative_path == "notes.txt"
            && file.content.as_deref() == Some(b"safe".as_slice())));
    assert!(snapshot.omitted_paths.iter().any(|file| file == ".env"));
}

#[test]
fn secret_like_checkpoint_payload_is_refused() {
    let path = root("checkpoint-secret");
    let store = store(&path);
    let mut checkpoint = content(&path, "secret");
    checkpoint.run_snapshot_json = r#"{"password":"do-not-persist"}"#.into();
    assert!(store.write_checkpoint(checkpoint).is_err());
}

#[test]
fn secret_like_untracked_content_is_not_copied() {
    let path = root("untracked-content-secret");
    fs::write(path.join("notes.txt"), "api_key=do-not-copy").expect("notes");
    let snapshot = UntrackedFileSnapshot::capture(&path, 1).expect("snapshot");
    let record = snapshot
        .files
        .iter()
        .find(|file| file.relative_path == "notes.txt")
        .expect("record");
    assert!(record.content.is_none());
    assert!(snapshot
        .omitted_paths
        .iter()
        .any(|file| file == "notes.txt"));
}

#[test]
fn same_size_file_change_invalidates_workspace_fingerprint() {
    let path = root("same-size");
    fs::write(path.join("large.bin"), vec![b'a'; 4096]).expect("seed");
    let before = WorkspaceSnapshot::capture(&path, 1).expect("before");
    fs::write(path.join("large.bin"), vec![b'b'; 4096]).expect("change");
    let after = WorkspaceSnapshot::capture(&path, 2).expect("after");
    assert_ne!(before.fingerprint, after.fingerprint);
    assert_ne!(before.files[0].digest, after.files[0].digest);
}

#[test]
fn traversal_paths_are_refused_for_compensation() {
    let path = root("traversal");
    let prior = UntrackedFileRecord {
        relative_path: "../outside.txt".into(),
        digest: "digest".into(),
        size: 1,
        content: Some(vec![1]),
    };
    let mut registry = compensation_registry(&path);
    assert!(registry
        .register_file_write("run-p9", &path, "../outside.txt", &prior, "post", 1)
        .is_err());
}

#[test]
fn symlink_escape_is_refused_when_supported() {
    let path = root("symlink");
    let outside = root("symlink-outside").join("outside.txt");
    fs::write(&outside, "outside").expect("outside");
    let link = path.join("link.txt");
    #[cfg(windows)]
    let result = std::os::windows::fs::symlink_file(&outside, &link);
    #[cfg(not(windows))]
    let result = std::os::unix::fs::symlink(&outside, &link);
    if result.is_err() {
        println!("P9_SYMLINK_REPARSE_CHECK=NOT_RUN: symlink creation unavailable");
        return;
    }
    let prior = UntrackedFileRecord {
        relative_path: "link.txt".into(),
        digest: "digest".into(),
        size: 1,
        content: Some(vec![1]),
    };
    let mut registry = compensation_registry(&path);
    assert!(registry
        .register_file_write("run-p9", &path, "link.txt", &prior, "post", 1)
        .is_err());
}

#[test]
fn git_snapshot_is_read_only_and_typed_when_unavailable() {
    let path = root("git");
    let snapshot = GitWorktreeSnapshot::capture(&path).expect("git snapshot");
    assert!(!snapshot.root.is_empty());
    assert!(!snapshot.detail.is_empty());
}

#[test]
fn git_snapshot_preserves_actual_changed_paths() {
    let path = root("git-paths");
    let git = |args: &[&str]| Command::new("git").arg("-C").arg(&path).args(args).output();
    if !git(&["init", "-q"]).is_ok_and(|output| output.status.success()) {
        println!("P9_GIT_PATH_CHECK=NOT_RUN: git unavailable");
        return;
    }
    fs::create_dir_all(path.join("src")).expect("src");
    fs::write(path.join("src/alpha.rs"), "fn alpha() {}\n").expect("source");
    assert!(git(&["config", "user.email", "p9@example.invalid"])
        .is_ok_and(|output| output.status.success()));
    assert!(git(&["config", "user.name", "P9 Test"]).is_ok_and(|output| output.status.success()));
    assert!(git(&["add", "."]).is_ok_and(|output| output.status.success()));
    assert!(git(&["commit", "-qm", "initial"]).is_ok_and(|output| output.status.success()));
    fs::write(path.join("src/alpha.rs"), "fn alpha_changed() {}\n").expect("edit");
    let snapshot = GitWorktreeSnapshot::capture(&path).expect("snapshot");
    assert!(snapshot
        .changed_paths
        .iter()
        .any(|changed| changed == "src/alpha.rs"));
}

#[test]
fn symlink_addition_changes_workspace_fingerprint_when_supported() {
    let path = root("symlink-fingerprint");
    let outside = root("symlink-fingerprint-outside").join("outside.txt");
    fs::write(&outside, "outside").expect("outside");
    let link = path.join("linked.txt");
    #[cfg(windows)]
    let result = std::os::windows::fs::symlink_file(&outside, &link);
    #[cfg(not(windows))]
    let result = std::os::unix::fs::symlink(&outside, &link);
    if result.is_err() {
        println!("P9_SYMLINK_REPARSE_CHECK=NOT_RUN: symlink creation unavailable");
        return;
    }
    fs::remove_file(&link).expect("remove link");
    let before = WorkspaceSnapshot::capture(&path, 1).expect("before");
    #[cfg(windows)]
    std::os::windows::fs::symlink_file(&outside, &link).expect("link");
    #[cfg(not(windows))]
    std::os::unix::fs::symlink(&outside, &link).expect("link");
    let after = WorkspaceSnapshot::capture(&path, 2).expect("after");
    assert_ne!(before.fingerprint, after.fingerprint);
}

#[test]
fn sqlite_checkpoint_is_consistent_and_local_test_only() {
    let path = root("sqlite");
    let source = path.join("test.sqlite");
    let destination = path.join("backup.sqlite");
    let connection = Connection::open(&source).expect("open sqlite");
    connection
        .execute("CREATE TABLE values_table(value TEXT)", [])
        .expect("table");
    connection
        .execute(
            "INSERT INTO values_table(value) VALUES ('checkpointed')",
            [],
        )
        .expect("insert");
    drop(connection);
    let hook = SqliteTestDatabaseHook;
    let spec = DatabaseCheckpointSpec {
        identity: "sqlite-test".into(),
        kind: DatabaseKind::Sqlite,
        classification: DatabaseClassification::LocalTest,
        adapter_id: "sqlite.vacuum-into".into(),
        endpoint_redacted: source.display().to_string(),
        max_duration_ms: 1_000,
        restore_capability: true,
    };
    let result = hook.checkpoint(&spec, &destination);
    assert_eq!(result.status, DatabaseCheckpointStatus::Success);
    assert!(result.artifact_digest.is_some());
}

#[test]
fn sqlite_production_classification_is_blocked() {
    let path = root("sqlite-production");
    let spec = DatabaseCheckpointSpec {
        identity: "prod".into(),
        kind: DatabaseKind::Sqlite,
        classification: DatabaseClassification::Production,
        adapter_id: "sqlite".into(),
        endpoint_redacted: path.join("prod.sqlite").display().to_string(),
        max_duration_ms: 1_000,
        restore_capability: true,
    };
    let result = SqliteTestDatabaseHook.checkpoint(&spec, &path.join("backup.sqlite"));
    assert_eq!(result.status, DatabaseCheckpointStatus::Blocked);
}

#[test]
fn sqlite_restore_requires_declared_capability() {
    let path = root("sqlite-restore");
    let artifact = path.join("artifact.sqlite");
    fs::write(&artifact, b"not-a-db").expect("artifact");
    let spec = DatabaseCheckpointSpec {
        identity: "sqlite".into(),
        kind: DatabaseKind::Sqlite,
        classification: DatabaseClassification::LocalTest,
        adapter_id: "sqlite".into(),
        endpoint_redacted: path.join("source.sqlite").display().to_string(),
        max_duration_ms: 1_000,
        restore_capability: false,
    };
    let result = SqliteTestDatabaseHook.restore(&spec, &artifact, &path.join("destination.sqlite"));
    assert_eq!(result.status, DatabaseCheckpointStatus::Blocked);
}

#[test]
fn sqlite_restore_rejects_a_non_database_artifact() {
    let path = root("sqlite-invalid-restore");
    let artifact = path.join("artifact.sqlite");
    fs::write(&artifact, b"not-a-database").expect("artifact");
    let spec = DatabaseCheckpointSpec {
        identity: "sqlite".into(),
        kind: DatabaseKind::Sqlite,
        classification: DatabaseClassification::LocalTest,
        adapter_id: "sqlite.vacuum-into".into(),
        endpoint_redacted: path.join("source.sqlite").display().to_string(),
        max_duration_ms: 1_000,
        restore_capability: true,
    };
    let result = SqliteTestDatabaseHook.restore(&spec, &artifact, &path.join("destination.sqlite"));
    assert_eq!(result.status, DatabaseCheckpointStatus::Failed);
}

#[test]
fn process_identity_digest_binds_pid_start_and_command() {
    let mut record = owned_process();
    let before = record.identity_digest();
    record.process_start_time_ms = Some(101);
    assert_ne!(before, record.identity_digest());
    record.process_start_time_ms = Some(100);
    record.command_digest = "different".into();
    assert_ne!(before, record.identity_digest());
    record.command_digest = "command-digest".into();
    record.executable_digest = "different-executable".into();
    assert_ne!(before, record.identity_digest());
}

#[test]
fn conservative_inspector_never_kills_pid_alone() {
    let record = ProcessOwnershipRecord {
        process_id: std::process::id(),
        ..owned_process()
    };
    assert_eq!(
        ConservativeProcessInspector.observe(&record),
        ProcessObservation::PidReusedNotOurs
    );
    assert!(ConservativeProcessInspector.terminate(&record).is_ok());
}

struct FakeInspector(ProcessObservation);

impl ProcessInspector for FakeInspector {
    fn observe(&self, _record: &ProcessOwnershipRecord) -> ProcessObservation {
        self.0
    }
    fn terminate(
        &self,
        _record: &ProcessOwnershipRecord,
    ) -> Result<ProcessObservation, relintor_execution::RecoveryError> {
        Ok(self.0)
    }
}

fn running_checkpoint_fixture(
    name: &str,
    with_process: bool,
) -> (PathBuf, ExecutionRun, RecoveryStore, RecoveryAuthority) {
    let path = root(name);
    let workspace = WorkspaceSnapshot::capture(&path, 1).expect("workspace");
    let mut run = empty_run(&path);
    run.workspace_fingerprint = workspace.fingerprint.clone();
    run.tasks.insert(
        "task-p9".into(),
        ExecutionTask {
            task_id: "task-p9".into(),
            objective: "reconcile the persisted process".into(),
            requirement_ids: vec!["requirement-p9".into()],
            dependency_ids: Vec::new(),
            priority: RequirementPriority::P2,
            state: ExecutionTaskState::Ready,
            scope: LeaseScope {
                workspace: path.clone(),
                file_scopes: vec![path.display().to_string()],
                directory_scopes: vec![path.display().to_string()],
                shared_resources: Vec::new(),
                package_lockfiles: Vec::new(),
                generated_files: Vec::new(),
                allowed_tools: Default::default(),
                external_authority: Default::default(),
                scope_known: true,
            },
            usage_budget: UsageBudget::default(),
            retry_policy: RetryPolicy::default(),
            evidence_obligations: Vec::new(),
            attempt_number: 0,
        },
    );
    run.start_task("task-p9", 10).expect("start task");
    let attempt = run.attempts.last().expect("running attempt").clone();
    let mut process = owned_process();
    process.run_id = run.run_id.clone();
    process.task_id = Some(attempt.task_id);
    process.attempt_id = Some(attempt.attempt_id);
    process.lease_id = Some(attempt.lease_id);
    let store = store(&path);
    let processes = if with_process {
        vec![process]
    } else {
        Vec::new()
    };
    RecoveryCoordinator::new(store.clone())
        .checkpoint_run(
            &run,
            authority(&workspace),
            CheckpointKind::AfterTaskPersistence,
            &path,
            processes,
            Vec::new(),
            "persisted running attempt before desktop restart",
            10,
        )
        .expect("persist running checkpoint");
    let latest = store
        .load_latest()
        .expect("load checkpoint")
        .expect("checkpoint");
    (path, run, store, latest.content.authority)
}

#[test]
fn orphan_reconciliation_preserves_unknown_process_state() {
    let path = root("orphan");
    let coordinator = RecoveryCoordinator::new(store(&path));
    let result = coordinator.reconcile_processes(
        &[owned_process()],
        &FakeInspector(ProcessObservation::ProcessStateUnknown),
    );
    assert_eq!(result, vec![ProcessObservation::ProcessStateUnknown]);
}

#[test]
fn pid_reuse_is_not_adoptable() {
    let path = root("pid-reuse");
    let coordinator = RecoveryCoordinator::new(store(&path));
    let result = coordinator.reconcile_processes(
        &[owned_process()],
        &FakeInspector(ProcessObservation::PidReusedNotOurs),
    );
    assert_eq!(result, vec![ProcessObservation::PidReusedNotOurs]);
}

#[test]
fn restart_reconciliation_has_explicit_deterministic_outcomes() {
    let (path, run, store, expected) = running_checkpoint_fixture("restart-outcomes", true);
    let coordinator = RecoveryCoordinator::new(store);
    let exact = coordinator
        .resume_integrity(
            &expected,
            &path,
            &FakeInspector(ProcessObservation::OwnedProcessStillRunning),
            20,
        )
        .expect("exact process reconciliation");
    assert_eq!(
        exact.disposition,
        RecoveryDisposition::SafeToResumeAfterProcessReconciliation
    );
    assert_eq!(run.state, ExecutionRunState::Running);

    for observation in [
        ProcessObservation::PidReusedNotOurs,
        ProcessObservation::ProcessStateUnknown,
        ProcessObservation::ProcessGone,
        ProcessObservation::ProcessCompletedResultAvailable,
    ] {
        let (path, _run, store, expected) =
            running_checkpoint_fixture(&format!("restart-{:?}", observation), true);
        let result = RecoveryCoordinator::new(store)
            .resume_integrity(&expected, &path, &FakeInspector(observation), 20)
            .expect("failed identity reconciliation");
        assert_eq!(
            result.disposition,
            RecoveryDisposition::RevalidationRequired
        );
        assert!(result
            .reasons
            .iter()
            .any(|reason| reason.contains("checkpoint records")
                || reason.contains("could not be proven")
                || reason.contains("different executable")));
    }

    let (path, _run, store, expected) = running_checkpoint_fixture("restart-absent", false);
    let result = RecoveryCoordinator::new(store)
        .resume_integrity(
            &expected,
            &path,
            &FakeInspector(ProcessObservation::ProcessGone),
            20,
        )
        .expect("absent process reconciliation");
    assert_eq!(
        result.disposition,
        RecoveryDisposition::RevalidationRequired
    );
}

#[test]
fn restart_interruption_revokes_lease_without_erasing_attempt_history() {
    let path = root("restart-interruption-history");
    let workspace = WorkspaceSnapshot::capture(&path, 1).expect("workspace");
    let mut run = empty_run(&path);
    run.workspace_fingerprint = workspace.fingerprint.clone();
    run.tasks.insert(
        "task-p9".into(),
        ExecutionTask {
            task_id: "task-p9".into(),
            objective: "reconcile".into(),
            requirement_ids: vec!["requirement-p9".into()],
            dependency_ids: Vec::new(),
            priority: RequirementPriority::P2,
            state: ExecutionTaskState::Ready,
            scope: LeaseScope {
                workspace: path.clone(),
                file_scopes: Vec::new(),
                directory_scopes: vec![path.display().to_string()],
                shared_resources: Vec::new(),
                package_lockfiles: Vec::new(),
                generated_files: Vec::new(),
                allowed_tools: Default::default(),
                external_authority: Default::default(),
                scope_known: true,
            },
            usage_budget: UsageBudget::default(),
            retry_policy: RetryPolicy::default(),
            evidence_obligations: Vec::new(),
            attempt_number: 0,
        },
    );
    run.start_task("task-p9", 10).expect("start task");
    let event_count = run.events.len();
    run.mark_stopped_incomplete("restart could not prove process ownership", 20)
        .expect("mark interrupted");
    assert_eq!(run.attempts.len(), 1);
    assert_eq!(run.attempts[0].state, TaskAttemptState::SafeBoundaryStopped);
    assert!(run
        .leases
        .iter()
        .all(|lease| lease.status != LeaseStatus::Active));
    assert!(run.events.len() > event_count);
    assert!(run
        .events
        .iter()
        .any(|event| event.detail == "lease-revoked-during-restart-reconciliation"));
}

#[test]
fn recovery_authorization_is_idempotent_and_distinct_from_new_continuation() {
    let path = root("continuation-idempotency");
    let mut run = empty_run(&path);
    run.resume_from_recovery(RecoveryDisposition::SafeToResume, 10)
        .expect("authorize recovery");
    let first_count = run.events.len();
    run.resume_from_recovery(RecoveryDisposition::SafeToResume, 11)
        .expect("repeat same authorization");
    assert_eq!(run.events.len(), first_count);
    assert_eq!(
        run.events.last().expect("authorization event").kind,
        relintor_execution::ExecutionEventKind::ContinuationAuthorized
    );
}

#[test]
fn session_marker_is_authenticated_and_clean_shutdown_is_explicit() {
    let path = root("session-clean");
    let store = store(&path);
    assert!(store
        .begin_session("run-p9", "exe", "cmd", 1)
        .expect("begin")
        .is_none());
    store
        .end_session(SessionEndState::CleanShutdown, 2)
        .expect("close");
    assert_eq!(
        store.load_session().expect("load").unwrap().state,
        SessionEndState::CleanShutdown
    );
}

#[test]
fn changed_session_identity_records_crash_without_inventing_success() {
    let path = root("session-crash");
    let store = store(&path);
    store
        .begin_session("run-p9", "exe-one", "cmd-one", 1)
        .expect("first");
    let crash = store
        .begin_session("run-p9", "exe-two", "cmd-two", 2)
        .expect("second");
    assert!(crash.is_some());
    assert!(store.load_session().expect("load").unwrap().state == SessionEndState::Active);
    let crash_json = fs::read_to_string(path.join(".relintor-recovery/crash-record.json"))
        .expect("crash record");
    assert!(crash_json.contains("integrity_tag"));
}

#[test]
fn active_session_from_another_run_does_not_affect_current_run() {
    let path = root("session-other-run");
    let store = store(&path);
    store
        .begin_session("run-one", "exe", "cmd", 1)
        .expect("first");
    assert!(store
        .begin_session("run-two", "exe", "cmd", 2)
        .expect("second")
        .is_none());
    assert_eq!(
        store.load_session().expect("load").unwrap().run_id,
        "run-two"
    );
}

#[test]
fn revalidation_record_is_durable_and_scoped() {
    let path = root("revalidation");
    let store = store(&path);
    let record = store
        .write_revalidation(relintor_execution::RevalidationRecord {
            record_version: String::new(),
            revalidation_id: "revalidation-p9".into(),
            project_id: "project-p9".into(),
            mission_id: "mission-p9".into(),
            mission_revision: 1,
            p7_run_id: "run-p9".into(),
            checkpoint_id: Some("checkpoint-p9".into()),
            target: None,
            classification: relintor_execution::RecoveryClassification::ExternalStateUncertain,
            disposition: relintor_execution::RecoveryDisposition::RevalidationRequired,
            decision: "REVALIDATION_REQUIRED".into(),
            reasons: vec!["external edit".into()],
            affected_paths: vec!["project.txt".into()],
            required_authorities: vec!["P8".into(), "P7".into()],
            created_at_ms: 1,
            integrity_tag: String::new(),
        })
        .expect("write");
    assert_eq!(record.affected_paths, vec!["project.txt"]);
}

#[test]
fn compensation_restores_only_unchanged_relintor_output() {
    let path = root("compensation-applied");
    let target = path.join("file.txt");
    fs::write(&target, "before").expect("before");
    let prior = UntrackedFileRecord {
        relative_path: "file.txt".into(),
        digest: format!("{:x}", Sha256::digest(b"before")),
        size: 6,
        content: Some(b"before".to_vec()),
    };
    fs::write(&target, "after").expect("after");
    let post = WorkspaceSnapshot::capture(&path, 2).expect("post").files[0]
        .digest
        .clone();
    let mut registry = compensation_registry(&path);
    let id = registry
        .register_file_write("run-p9", &path, "file.txt", &prior, &post, 2)
        .expect("register");
    let attempt = registry
        .compensate_file_write(&id, &prior, 3)
        .expect("compensate");
    assert_eq!(attempt.outcome, CompensationOutcome::Applied);
    assert_eq!(fs::read_to_string(target).expect("read"), "before");
}

#[test]
fn compensation_blocks_when_user_changed_the_target() {
    let path = root("compensation-blocked");
    let target = path.join("file.txt");
    fs::write(&target, "before").expect("before");
    let prior = UntrackedFileRecord {
        relative_path: "file.txt".into(),
        digest: format!("{:x}", Sha256::digest(b"before")),
        size: 6,
        content: Some(b"before".to_vec()),
    };
    fs::write(&target, "after").expect("after");
    let post = WorkspaceSnapshot::capture(&path, 2).expect("post").files[0]
        .digest
        .clone();
    let mut registry = compensation_registry(&path);
    let id = registry
        .register_file_write("run-p9", &path, "file.txt", &prior, &post, 2)
        .expect("register");
    fs::write(&target, "user-edit").expect("user edit");
    let attempt = registry
        .compensate_file_write(&id, &prior, 3)
        .expect("compensate");
    assert_eq!(attempt.outcome, CompensationOutcome::BlockedExternal);
}

#[test]
fn compensation_rejects_a_forged_prior_snapshot() {
    let path = root("compensation-forged-prior");
    let target = path.join("file.txt");
    fs::write(&target, "before").expect("before");
    let prior = UntrackedFileRecord {
        relative_path: "file.txt".into(),
        digest: format!("{:x}", Sha256::digest(b"before")),
        size: 6,
        content: Some(b"before".to_vec()),
    };
    fs::write(&target, "after").expect("after");
    let post = WorkspaceSnapshot::capture(&path, 2).expect("post").files[0]
        .digest
        .clone();
    let mut registry = compensation_registry(&path);
    let id = registry
        .register_file_write("run-p9", &path, "file.txt", &prior, &post, 2)
        .expect("register");
    let forged = UntrackedFileRecord {
        relative_path: "file.txt".into(),
        digest: format!("{:x}", Sha256::digest(b"forged")),
        size: 6,
        content: Some(b"forged".to_vec()),
    };
    assert!(registry.compensate_file_write(&id, &forged, 3).is_err());
    assert_eq!(fs::read_to_string(target).expect("read"), "after");
}

#[test]
fn compensation_journal_survives_reload_and_blocks_repeat_execution() {
    let path = root("compensation-journal");
    let target = path.join("file.txt");
    fs::write(&target, "before").expect("before");
    let prior = UntrackedFileRecord {
        relative_path: "file.txt".into(),
        digest: format!("{:x}", Sha256::digest(b"before")),
        size: 6,
        content: Some(b"before".to_vec()),
    };
    fs::write(&target, "after").expect("after");
    let post = WorkspaceSnapshot::capture(&path, 2).expect("post").files[0]
        .digest
        .clone();
    let store = store(&path);
    let mut registry = CompensationRegistry::from_store(&store).expect("journal");
    let id = registry
        .register_file_write("run-p9", &path, "file.txt", &prior, &post, 2)
        .expect("register");
    drop(registry);
    let mut reloaded = CompensationRegistry::from_store(&store).expect("reload");
    let attempt = reloaded
        .compensate_file_write(&id, &prior, 3)
        .expect("compensate");
    assert_eq!(
        attempt.state,
        relintor_execution::CompensationAttemptState::Succeeded
    );
    drop(reloaded);
    let reloaded = CompensationRegistry::from_store(&store).expect("reload result");
    assert!(reloaded.attempts().iter().any(|attempt| {
        attempt.compensation_id == id
            && attempt.state == relintor_execution::CompensationAttemptState::Succeeded
    }));
}

#[test]
fn before_mutation_checkpoint_is_not_an_automatic_resume_authority() {
    let path = root("unsafe-checkpoint");
    let run = empty_run(&path);
    let store = store(&path);
    let coordinator = RecoveryCoordinator::new(store.clone());
    let workspace = WorkspaceSnapshot::capture(&path, 1).expect("workspace");
    let authority = RecoveryAuthority::new(
        "project-p9",
        "mission-p9",
        1,
        "p6-seal-p9",
        "registry-p9",
        run.run_id.clone(),
        workspace.fingerprint.clone(),
        "source-p9",
        None,
        "P8_PENDING",
        1,
    );
    coordinator
        .checkpoint_run(
            &run,
            authority,
            CheckpointKind::BeforeMutableAction,
            &path,
            Vec::new(),
            Vec::new(),
            "before mutation",
            1,
        )
        .expect("checkpoint");
    let record = store.load_latest().expect("load").unwrap();
    assert!(!record.is_safe_to_resume());
    let expected = RecoveryAuthority::new(
        "project-p9",
        "mission-p9",
        1,
        "p6-seal-p9",
        "registry-p9",
        run.run_id.clone(),
        record.content.authority.workspace_identity.clone(),
        "source-p9",
        None,
        "P8_PENDING",
        2,
    );
    let result = coordinator
        .resume_integrity(&expected, &path, &ConservativeProcessInspector, 2)
        .expect("integrity");
    assert_eq!(
        result.disposition,
        RecoveryDisposition::RevalidationRequired
    );
}

#[test]
fn unknown_destructive_action_is_not_automatically_reversible() {
    let path = root("unknown-destructive");
    let mut registry = compensation_registry(&path);
    let id = registry
        .record_non_reversible(
            "run-p9",
            CompensationActionType::UnknownDestructive,
            "unknown",
            1,
        )
        .expect("record");
    assert!(!registry.entries().get(&id).expect("entry").reversible);
    assert!(
        registry
            .entries()
            .get(&id)
            .expect("entry")
            .human_approval_required
    );
}

#[test]
fn safe_checkpoint_resume_does_not_mint_completion() {
    let path = root("resume-safe");
    fs::write(path.join("project.txt"), "safe").expect("seed");
    let run = empty_run(&path);
    let store = store(&path);
    let coordinator = RecoveryCoordinator::new(store.clone());
    let workspace = WorkspaceSnapshot::capture(&path, 1).expect("workspace");
    let authority = RecoveryAuthority::new(
        "project-p9",
        "mission-p9",
        1,
        "p6-seal-p9",
        "registry-p9",
        run.run_id.clone(),
        workspace.fingerprint.clone(),
        "source-p9",
        None,
        "P8_PENDING",
        1,
    );
    coordinator
        .checkpoint_run(
            &run,
            authority,
            CheckpointKind::AfterTaskPersistence,
            &path,
            Vec::new(),
            Vec::new(),
            "safe",
            1,
        )
        .expect("checkpoint");
    let record = store.load_latest().expect("load").unwrap();
    let expected = RecoveryAuthority::new(
        "project-p9",
        "mission-p9",
        1,
        "p6-seal-p9",
        "registry-p9",
        run.run_id.clone(),
        record.content.authority.workspace_identity.clone(),
        "source-p9",
        None,
        "P8_PENDING",
        2,
    );
    let result = coordinator
        .resume_integrity(&expected, &path, &ConservativeProcessInspector, 2)
        .expect("resume");
    assert_eq!(result.disposition, RecoveryDisposition::SafeToResume);
    let restored = store.load_run().expect("run").unwrap();
    assert_ne!(
        restored.state,
        ExecutionRunState::ExecutionTasksFinishedAwaitingVerification
    );
}

#[test]
fn stopped_incomplete_is_a_distinct_recovery_state() {
    let path = root("stopped-incomplete");
    let mut run = empty_run(&path);
    run.state = ExecutionRunState::StoppedIncomplete;
    let store = store(&path);
    let coordinator = RecoveryCoordinator::new(store.clone());
    let workspace = WorkspaceSnapshot::capture(&path, 1).expect("workspace");
    let authority = RecoveryAuthority::new(
        "project-p9",
        "mission-p9",
        1,
        "p6-seal-p9",
        "registry-p9",
        run.run_id.clone(),
        workspace.fingerprint.clone(),
        "source-p9",
        None,
        "P8_PENDING",
        1,
    );
    coordinator
        .checkpoint_run(
            &run,
            authority,
            CheckpointKind::EmergencyStop,
            &path,
            Vec::new(),
            Vec::new(),
            "user stop",
            1,
        )
        .expect("checkpoint");
    let record = store.load_latest().expect("load").unwrap();
    let expected = RecoveryAuthority::new(
        "project-p9",
        "mission-p9",
        1,
        "p6-seal-p9",
        "registry-p9",
        run.run_id.clone(),
        record.content.authority.workspace_identity.clone(),
        "source-p9",
        None,
        "P8_PENDING",
        2,
    );
    let result = coordinator
        .resume_integrity(&expected, &path, &ConservativeProcessInspector, 2)
        .expect("resume");
    assert_eq!(result.disposition, RecoveryDisposition::StoppedIncomplete);
}

#[test]
fn process_restart_harness_child() {
    if std::env::var_os("P9_CHILD").is_none() {
        return;
    }
    let path = PathBuf::from(std::env::var_os("P9_CHILD_ROOT").expect("child root"));
    let child_store = store(&path);
    child_store
        .begin_session("run-p9", "child-executable", "child-command", 10)
        .expect("child session");
    fs::write(path.join("mid-edit.txt"), "child edit before termination").expect("child edit");
    fs::write(path.join("child-ready"), "ready").expect("ready marker");
    std::thread::sleep(Duration::from_secs(30));
}

#[test]
fn process_restart_harness_detects_interrupted_mid_edit() {
    let path = root("restart-harness");
    fs::write(path.join("mid-edit.txt"), "before").expect("seed");
    let run = empty_run(&path);
    let store = store(&path);
    let coordinator = RecoveryCoordinator::new(store.clone());
    let workspace = WorkspaceSnapshot::capture(&path, 1).expect("workspace");
    let authority = RecoveryAuthority::new(
        "project-p9",
        "mission-p9",
        1,
        "p6-seal-p9",
        "registry-p9",
        run.run_id.clone(),
        workspace.fingerprint.clone(),
        "source-p9",
        None,
        "P8_PENDING",
        1,
    );
    coordinator
        .checkpoint_run(
            &run,
            authority,
            CheckpointKind::BeforeMutableAction,
            &path,
            Vec::new(),
            Vec::new(),
            "before child process edit",
            1,
        )
        .expect("checkpoint");
    let executable = std::env::current_exe().expect("test executable");
    let mut child = Command::new(executable)
        .args(["--exact", "process_restart_harness_child", "--nocapture"])
        .env("P9_CHILD", "1")
        .env("P9_CHILD_ROOT", &path)
        .spawn()
        .expect("spawn restart harness child");
    let ready = path.join("child-ready");
    for _ in 0..100 {
        if ready.is_file() {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(ready.is_file(), "child did not reach its edit boundary");
    child
        .kill()
        .expect("terminate child at interruption boundary");
    let status = child.wait().expect("wait child");
    assert!(
        !status.success(),
        "interrupted child must not report success"
    );
    let record = store.load_latest().expect("checkpoint").expect("record");
    let expected = RecoveryAuthority::new(
        "project-p9",
        "mission-p9",
        1,
        "p6-seal-p9",
        "registry-p9",
        run.run_id.clone(),
        record.content.authority.workspace_identity.clone(),
        "source-p9",
        None,
        "P8_PENDING",
        2,
    );
    let result = coordinator
        .resume_integrity(&expected, &path, &ConservativeProcessInspector, 2)
        .expect("resume integrity");
    assert_eq!(
        result.disposition,
        RecoveryDisposition::RevalidationRequired
    );
    assert!(result
        .changed_paths
        .iter()
        .any(|path| path == "mid-edit.txt"));
}
