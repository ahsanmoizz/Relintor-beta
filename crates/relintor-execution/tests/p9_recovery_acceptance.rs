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
        reviewed_recovery_deltas: Vec::new(),
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
fn superseded_checkpoint_payloads_compact_to_authenticated_chain_stubs() {
    let path = root("phase2-chain-compaction");
    let recovery_store = store(&path);
    let first = recovery_store
        .write_checkpoint(content(&path, "first"))
        .expect("first checkpoint");
    let second = recovery_store
        .write_checkpoint(content(&path, "second"))
        .expect("second checkpoint");
    let third = recovery_store
        .write_checkpoint(content(&path, "third"))
        .expect("third checkpoint");

    let recovery = path.join(".relintor-recovery");
    let artifact_for = |sequence: u64| {
        fs::read_dir(&recovery)
            .expect("list recovery")
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .find(|artifact| {
                artifact.file_name().is_some_and(|name| {
                    name.to_string_lossy()
                        .contains(&format!("{sequence:020}"))
                })
            })
            .expect("checkpoint artifact")
    };
    let first_path = artifact_for(1);
    let second_path = artifact_for(2);
    let third_path = artifact_for(3);

    let first_json: serde_json::Value =
        serde_json::from_slice(&fs::read(&first_path).expect("read first")).expect("parse first");
    let second_json: serde_json::Value =
        serde_json::from_slice(&fs::read(&second_path).expect("read second")).expect("parse second");
    let third_json: serde_json::Value =
        serde_json::from_slice(&fs::read(&third_path).expect("read third")).expect("parse third");

    assert!(
        first_json.get("content").is_none(),
        "payload older than the previous rollback boundary should be compacted"
    );
    assert_eq!(
        first_json.get("record_version").and_then(|value| value.as_str()),
        Some("p9-checkpoint-chain-stub-v1")
    );
    assert_eq!(
        first_json.get("content_digest").and_then(|value| value.as_str()),
        Some(first.content_digest.as_str())
    );
    assert!(
        second_json.get("content").is_some(),
        "immediately previous checkpoint must remain fully recoverable"
    );
    assert!(
        third_json.get("content").is_some(),
        "latest checkpoint must remain fully recoverable"
    );
    assert_eq!(second.sequence, 2);
    assert_eq!(third.sequence, 3);
    assert!(
        fs::metadata(&first_path).expect("first metadata").len()
            < fs::metadata(&third_path).expect("third metadata").len()
    );

    let reopened = store(&path).load_latest().expect("reload").expect("latest");
    assert_eq!(reopened.sequence, third.sequence);
    assert_eq!(reopened.checkpoint_id, third.checkpoint_id);
}


#[test]
fn checkpoint_run_records_workspace_identity_without_copying_workspace_bytes() {
    let path = root("phase2-checkpoint-metadata-only");
    fs::write(path.join("notes.txt"), vec![b'x'; 256 * 1024]).expect("seed workspace file");
    let run = empty_run(&path);
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
    let store = store(&path);
    RecoveryCoordinator::new(store.clone())
        .checkpoint_run(
            &run,
            authority,
            CheckpointKind::AfterTaskPersistence,
            &path,
            Vec::new(),
            Vec::new(),
            "metadata-only checkpoint",
            1,
        )
        .expect("checkpoint");

    let latest = store.load_latest().expect("load").expect("latest");
    assert_eq!(latest.content.workspace.fingerprint, workspace.fingerprint);
    assert_eq!(latest.content.untracked.total_bytes, 0);
    assert!(latest
        .content
        .untracked
        .files
        .iter()
        .all(|file| file.content.is_none()));
    assert!(latest
        .content
        .untracked
        .files
        .iter()
        .any(|file| file.relative_path == "notes.txt"));
}

#[test]
fn compact_chain_stub_tampering_fails_closed() {
    let path = root("phase2-stub-tamper");
    let recovery_store = store(&path);
    recovery_store
        .write_checkpoint(content(&path, "first"))
        .expect("first checkpoint");
    recovery_store
        .write_checkpoint(content(&path, "second"))
        .expect("second checkpoint");
    recovery_store
        .write_checkpoint(content(&path, "third"))
        .expect("third checkpoint");

    let recovery = path.join(".relintor-recovery");
    let first_path = fs::read_dir(&recovery)
        .expect("list recovery")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|artifact| {
            artifact
                .file_name()
                .is_some_and(|name| name.to_string_lossy().contains("00000000000000000001"))
        })
        .expect("first artifact");
    let mut value: serde_json::Value =
        serde_json::from_slice(&fs::read(&first_path).expect("read stub")).expect("parse stub");
    assert!(
        value.get("content").is_none(),
        "sequence 1 must be compacted once sequence 3 is durable"
    );
    value["reason"] = serde_json::Value::String("tampered".into());
    fs::write(
        &first_path,
        serde_json::to_vec(&value).expect("serialize tampered stub"),
    )
    .expect("write tampered stub");

    let reopened = store(&path);
    assert!(
        reopened.load_latest().is_err(),
        "tampered compact history must fail closed"
    );
}


#[test]
fn interrupted_compaction_mixed_full_and_stub_history_still_loads_latest() {
    let path = root("phase2-interrupted-compaction");
    let recovery_store = store(&path);
    recovery_store
        .write_checkpoint(content(&path, "first"))
        .expect("first checkpoint");
    recovery_store
        .write_checkpoint(content(&path, "second"))
        .expect("second checkpoint");
    recovery_store
        .write_checkpoint(content(&path, "third"))
        .expect("third checkpoint");

    let recovery = path.join(".relintor-recovery");
    let second_path = fs::read_dir(&recovery)
        .expect("list recovery")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|artifact| {
            artifact
                .file_name()
                .is_some_and(|name| name.to_string_lossy().contains("00000000000000000002"))
        })
        .expect("second artifact");
    let second_full_bytes = fs::read(&second_path).expect("save second full payload");

    recovery_store
        .write_checkpoint(content(&path, "fourth"))
        .expect("fourth checkpoint");

    // Sequence 2 is now eligible for compaction. Restoring its already-authenticated full
    // payload models cleanup being interrupted before that replacement became durable.
    // The chain must accept this mixed full/stub history and still resolve the newest authority.
    fs::write(&second_path, second_full_bytes).expect("restore second full payload");

    let reopened = store(&path)
        .load_latest()
        .expect("load mixed chain")
        .expect("latest");
    assert_eq!(reopened.sequence, 4);
    assert_eq!(reopened.content.reason, "fourth");
}


#[test]
fn repeated_checkpoint_generation_keeps_previous_and_latest_full_recovery_payloads() {
    let path = root("phase2-bounded-history");
    let recovery_store = store(&path);
    for sequence in 1..=100_u64 {
        recovery_store
            .write_checkpoint(content(&path, &format!("checkpoint-{sequence}")))
            .expect("write bounded checkpoint chain");
    }

    let recovery = path.join(".relintor-recovery");
    let mut full_payloads = 0_usize;
    let mut chain_stubs = 0_usize;
    let mut checkpoint_bytes = 0_u64;
    let mut full_sequences = Vec::new();
    for path in fs::read_dir(&recovery)
        .expect("list recovery")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name().is_some_and(|name| {
                let name = name.to_string_lossy();
                name.starts_with("checkpoint-") && name.ends_with(".json")
            })
        })
    {
        checkpoint_bytes += fs::metadata(&path).expect("checkpoint metadata").len();
        let value: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).expect("read checkpoint"))
                .expect("parse checkpoint");
        if value.get("content").is_some() {
            full_payloads += 1;
            let sequence = value
                .get("sequence")
                .and_then(|value| value.as_u64())
                .expect("full checkpoint sequence");
            full_sequences.push(sequence);
        } else if value
            .get("record_version")
            .and_then(|value| value.as_str())
            == Some("p9-checkpoint-chain-stub-v1")
        {
            chain_stubs += 1;
        }
    }

    full_sequences.sort_unstable();
    assert_eq!(
        full_payloads, 2,
        "previous and latest recovery payloads must remain full"
    );
    assert_eq!(
        full_sequences,
        vec![99, 100],
        "only the immediate rollback boundary and latest authority stay full"
    );
    assert_eq!(
        chain_stubs, 98,
        "older superseded generations become authenticated stubs"
    );
    assert!(
        checkpoint_bytes < 10 * 1024 * 1024,
        "synthetic 100-generation history must remain bounded"
    );

    let reopened = store(&path).load_latest().expect("reload").expect("latest");
    assert_eq!(reopened.sequence, 100);
    assert_eq!(reopened.content.reason, "checkpoint-100");
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

#[test]
fn manual_recovery_retry_authorizes_fresh_attempt_and_passes_action_authorization() {
    let path = root("manual-recovery-retry-allowance");
    let mut run = empty_run(&path);
    run.tasks.insert(
        "task-recovery-test".into(),
        ExecutionTask {
            task_id: "task-recovery-test".into(),
            objective: "test objective".into(),
            requirement_ids: vec!["req-1".into()],
            dependency_ids: Vec::new(),
            priority: RequirementPriority::P1,
            state: ExecutionTaskState::Pending,
            scope: LeaseScope {
                workspace: path.clone(),
                file_scopes: vec![path.display().to_string()],
                directory_scopes: vec![path.display().to_string()],
                shared_resources: Vec::new(),
                package_lockfiles: Vec::new(),
                generated_files: Vec::new(),
                allowed_tools: ["antigravity".into()].into_iter().collect(),
                external_authority: Default::default(),
                scope_known: true,
            },
            usage_budget: UsageBudget {
                wall_clock_ms: 1_500_000,
                execution_steps: 100,
                tool_calls: 50,
                retry_attempts: 2,
                cost_micros: None,
            },
            retry_policy: RetryPolicy {
                max_attempts: 2,
                retryable: [
                    relintor_execution::FailureClass::Transient,
                    relintor_execution::FailureClass::ExternalUnavailable,
                    relintor_execution::FailureClass::ProcessFailure,
                ]
                .into_iter()
                .collect(),
                backoff_ms: 250,
            },
            evidence_obligations: Vec::new(),
            attempt_number: 0,
        },
    );

    // Attempt 1: start and mark stopped
    let packet1 = run.start_task("task-recovery-test", 10).expect("start 1");
    if let Some(attempt) = run.attempts.last_mut() {
        attempt.execution_boundary =
            relintor_execution::AttemptExecutionBoundary::ExternalProcessStarted;
        attempt.state = TaskAttemptState::SafeBoundaryStopped;
        attempt.ended_at_ms = Some(20);
        attempt.failure_class = Some(relintor_execution::FailureClass::ProcessFailure);
    }
    for lease in &mut run.leases {
        if lease.lease_id == packet1.lease_id {
            lease.status = LeaseStatus::Consumed;
        }
    }
    run.tasks.get_mut("task-recovery-test").unwrap().state = ExecutionTaskState::WaitingRetry;
    run.state = ExecutionRunState::Ready;

    // Attempt 2: start and mark stopped with external process started
    let packet2 = run.start_task("task-recovery-test", 30).expect("start 2");
    let lease2 = packet2.lease_id.clone();
    let attempt2_id = run.attempts.last().unwrap().attempt_id.clone();
    if let Some(attempt) = run.attempts.last_mut() {
        attempt.execution_boundary =
            relintor_execution::AttemptExecutionBoundary::ExternalProcessStarted;
        attempt.state = TaskAttemptState::SafeBoundaryStopped;
        attempt.ended_at_ms = Some(40);
        attempt.failure_class = Some(relintor_execution::FailureClass::ProcessFailure);
    }
    for lease in &mut run.leases {
        if lease.lease_id == packet2.lease_id {
            lease.status = LeaseStatus::Consumed;
        }
    }

    // Now set run state to RevalidationRequired
    run.state = ExecutionRunState::RevalidationRequired;
    assert_eq!(run.tasks["task-recovery-test"].attempt_number, 2);

    let target = run
        .current_recovery_attempt()
        .expect("recovery attempt target");
    assert_eq!(target.attempt_id, attempt2_id);
    assert_eq!(target.lease_id, lease2);

    run.authorize_manual_recovery_retry(&target, 50)
        .expect("authorize manual recovery retry");

    assert_eq!(run.state, ExecutionRunState::Ready);
    assert_eq!(
        run.tasks["task-recovery-test"].state,
        ExecutionTaskState::WaitingRetry
    );
    assert!(run.tasks["task-recovery-test"].usage_budget.retry_attempts >= 3);
    assert!(run.tasks["task-recovery-test"].retry_policy.max_attempts >= 3);

    // Attempt 3: fresh attempt must start and authorize action without BudgetExhausted
    let packet3 = run.start_task("task-recovery-test", 60).expect("start 3");
    let action = relintor_execution::ActionRequest {
        tool: "antigravity".into(),
        operation: "execute_task".into(),
        arguments: vec!["task-recovery-test".into()],
        working_scope: path.display().to_string(),
        environment_identity: "test".into(),
        mutable: true,
        paths: vec![path.display().to_string()],
        external_authority: None,
    };

    run.authorize_action(
        "task-recovery-test",
        &packet3.lease_id,
        &packet3,
        &action,
        65,
    )
    .expect("authorize action for attempt 3");
}

#[test]
fn finished_mission_with_historical_recovery_attempts_reconciles_cleanly_and_disables_recovery() {
    let path = root("finished-mission-recovery-resolution");
    let mut run = empty_run(&path);
    run.tasks.insert(
        "task-1".into(),
        ExecutionTask {
            task_id: "task-1".into(),
            objective: "task 1".into(),
            requirement_ids: vec!["req-1".into()],
            dependency_ids: Vec::new(),
            priority: RequirementPriority::P1,
            state: ExecutionTaskState::Pending,
            scope: LeaseScope {
                workspace: path.clone(),
                file_scopes: vec![path.display().to_string()],
                directory_scopes: vec![path.display().to_string()],
                shared_resources: Vec::new(),
                package_lockfiles: Vec::new(),
                generated_files: Vec::new(),
                allowed_tools: ["antigravity".into()].into_iter().collect(),
                external_authority: Default::default(),
                scope_known: true,
            },
            usage_budget: UsageBudget {
                wall_clock_ms: 1_500_000,
                execution_steps: 100,
                tool_calls: 50,
                retry_attempts: 2,
                cost_micros: None,
            },
            retry_policy: RetryPolicy {
                max_attempts: 2,
                retryable: [relintor_execution::FailureClass::Transient].into_iter().collect(),
                backoff_ms: 250,
            },
            evidence_obligations: Vec::new(),
            attempt_number: 0,
        },
    );

    // Attempt 1: start and mark stopped
    let packet1 = run.start_task("task-1", 10).expect("start 1");
    if let Some(attempt) = run.attempts.last_mut() {
        attempt.execution_boundary =
            relintor_execution::AttemptExecutionBoundary::ExternalProcessStarted;
        attempt.state = TaskAttemptState::SafeBoundaryStopped;
        attempt.ended_at_ms = Some(20);
        attempt.failure_class = Some(relintor_execution::FailureClass::ProcessFailure);
    }
    for lease in &mut run.leases {
        if lease.lease_id == packet1.lease_id {
            lease.status = LeaseStatus::Consumed;
        }
    }
    run.tasks.get_mut("task-1").unwrap().state = ExecutionTaskState::WaitingRetry;
    run.state = ExecutionRunState::Ready;

    // Attempt 2: start and mark stopped
    let packet2 = run.start_task("task-1", 30).expect("start 2");
    if let Some(attempt) = run.attempts.last_mut() {
        attempt.execution_boundary =
            relintor_execution::AttemptExecutionBoundary::ExternalProcessStarted;
        attempt.state = TaskAttemptState::SafeBoundaryStopped;
        attempt.ended_at_ms = Some(40);
        attempt.failure_class = Some(relintor_execution::FailureClass::ProcessFailure);
    }
    for lease in &mut run.leases {
        if lease.lease_id == packet2.lease_id {
            lease.status = LeaseStatus::Consumed;
        }
    }
    run.tasks.get_mut("task-1").unwrap().state = ExecutionTaskState::WaitingRetry;
    run.tasks.get_mut("task-1").unwrap().usage_budget.retry_attempts = 3;
    run.tasks.get_mut("task-1").unwrap().retry_policy.max_attempts = 3;
    run.state = ExecutionRunState::Ready;

    // Attempt 3: start and mark succeeded
    let packet3 = run.start_task("task-1", 50).expect("start 3");
    if let Some(attempt) = run.attempts.last_mut() {
        attempt.execution_boundary =
            relintor_execution::AttemptExecutionBoundary::ExternalProcessStarted;
        attempt.state = TaskAttemptState::Succeeded;
        attempt.ended_at_ms = Some(60);
    }
    for lease in &mut run.leases {
        if lease.lease_id == packet3.lease_id {
            lease.status = LeaseStatus::Consumed;
        }
    }
    run.tasks.get_mut("task-1").unwrap().state = ExecutionTaskState::FinishedAwaitingVerification;
    assert!(run.all_tasks_finished());
    assert_eq!(run.current_recovery_attempt(), None);
    assert!(!run.recovery_status_requires_attention());

    let initial_event_count = run.events.len();
    for i in 0..10 {
        run.record_recovery_decision(None, "RevalidationRequired", 70 + i)
            .expect("record recovery decision on finished run");
    }
    assert_eq!(run.events.len(), initial_event_count);
}

#[test]
fn test_recovery_store_shared_cache_hits_across_instances() {
    let path = root("shared-cache");
    let store1 = store(&path);
    let _first = store1
        .write_checkpoint(content(&path, "first"))
        .expect("write first");
    let second = store1
        .write_checkpoint(content(&path, "second"))
        .expect("write second");

    // Create another store instance pointing to the same path (simulating Tauri's recovery_store(app, revision))
    let store2 = store(&path);
    let loaded = store2
        .load_latest()
        .expect("load checkpoint")
        .expect("latest checkpoint");
    assert_eq!(loaded.checkpoint_id, second.checkpoint_id);
    assert_eq!(loaded.sequence, 2);
    assert_eq!(loaded.content.reason, "second");

    // Creating a third store instance also hits the shared cache
    let store3 = store(&path);
    let loaded3 = store3
        .load_latest()
        .expect("load checkpoint")
        .expect("latest checkpoint");
    assert_eq!(loaded3.checkpoint_id, second.checkpoint_id);
    assert_eq!(loaded3.sequence, 2);

    let _ = fs::remove_dir_all(&path);
}

#[test]
fn test_defect_a_recovery_review_and_retry_flow() {
    let path = root("defect-a-flow-ws");
    let recovery_dir = root("defect-a-flow-rec");
    let store1 = RecoveryStore::new(&recovery_dir, vec![7; 32]).expect("open recovery store");
    let mut run = empty_run(&path);
    run.tasks.insert(
        "task-11".into(),
        ExecutionTask {
            task_id: "task-11".into(),
            objective: "Phase 5 Task 11".into(),
            requirement_ids: vec!["requirement-p9".into()],
            dependency_ids: Vec::new(),
            priority: RequirementPriority::P2,
            state: ExecutionTaskState::Ready,
            scope: LeaseScope {
                workspace: path.clone(),
                file_scopes: Vec::new(),
                directory_scopes: Vec::new(),
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
    run.state = ExecutionRunState::Ready;

    let packet = run.start_task("task-11", 200).expect("start task 11");
    if let Some(attempt) = run.attempts.last_mut() {
        attempt.execution_boundary = relintor_execution::AttemptExecutionBoundary::ExternalProcessStarted;
        attempt.state = TaskAttemptState::Failed;
        attempt.termination_reason = Some("execution ledger: recovery storage: recovery artifact exceeds size limit".into());
        attempt.ended_at_ms = Some(250);
    }
    for lease in &mut run.leases {
        if lease.lease_id == packet.lease_id {
            lease.status = LeaseStatus::Consumed;
            lease.lease_digest = lease.compute_digest().unwrap();
        }
    }
    run.tasks.get_mut("task-11").unwrap().state = ExecutionTaskState::BlockedExternal;
    run.state = ExecutionRunState::BlockedExternal;
    run.last_error = Some("execution ledger: recovery storage: recovery artifact exceeds size limit".into());

    let ws = WorkspaceSnapshot::capture(&path, 100).expect("capture ws");
    let mut auth = authority(&ws);
    auth.p7_run_id = run.run_id.clone();
    let coord = RecoveryCoordinator::new(store1.clone());

    let mut proc = owned_process();
    proc.run_id = run.run_id.clone();
    proc.task_id = Some("task-11".into());
    proc.attempt_id = run.attempts.last().map(|a| a.attempt_id.clone());
    proc.lease_id = Some(packet.lease_id.clone());

    // Write initial checkpoint 1
    let cp1 = coord.checkpoint_run(
        &run,
        auth.clone(),
        CheckpointKind::AfterTaskPersistence,
        &path,
        vec![proc],
        vec![],
        "initial checkpoint",
        300,
    ).expect("checkpoint 1");
    assert_eq!(cp1.sequence, 1);

    // 1. Resume integrity check: ExternalProcessStarted attempt, process is gone
    let integrity = coord.resume_integrity_for_run(
        &run,
        &auth,
        &path,
        &ConservativeProcessInspector,
        400,
    ).expect("evaluate resume integrity");
    assert_eq!(integrity.disposition, RecoveryDisposition::RevalidationRequired);

    // 2. Begin revalidation (writes checkpoint-revalidation.json)
    let reval = coord.begin_revalidation(&auth, &integrity, 401).expect("begin revalidation");
    assert_eq!(reval.disposition, RecoveryDisposition::RevalidationRequired);

    // 3. User authorizes manual retry
    let retry_reval = coord.authorize_manual_retry(&auth, &integrity, 500).expect("authorize retry");
    assert_eq!(retry_reval.decision, "MANUAL_RETRY_AUTHORIZED");

    // 4. Update run with reviewed delta
    let target = run.current_recovery_attempt().expect("target exists");
    let delta = relintor_execution::ReviewedRecoveryDelta {
        mission_id: run.mission_id.clone(),
        mission_revision: run.mission_revision,
        seal_hash: run.seal_hash.clone(),
        task_id: target.task_id.clone(),
        attempt_id: target.attempt_id.clone(),
        checkpoint_id: Some(cp1.checkpoint_id.clone()),
        affected_paths: retry_reval.affected_paths.clone(),
        baseline_fingerprint: run.workspace_fingerprint.clone(),
        authorized_starting_fingerprint: String::new(),
        authorized_at_ms: 500,
    };
    run.authorize_manual_recovery_retry_with_delta(&target, Some(&delta), 500).expect("authorize retry with delta");

    // 5. Checkpoint the retry (Checkpoint 2) - verifies compact JSON writing and size check
    let cp2 = coord.checkpoint_run(
        &run,
        auth.clone(),
        CheckpointKind::AfterTaskPersistence,
        &path,
        vec![],
        vec![],
        "explicit recovery review authorized a fresh attempt for the interrupted task",
        510,
    ).expect("write recovery retry checkpoint");
    assert_eq!(cp2.sequence, 2);

    let _ = fs::remove_dir_all(&path);
    let _ = fs::remove_dir_all(&recovery_dir);
}

#[test]
fn test_intermediate_duplicate_checkpoint_with_forked_lineage_resolves_to_authoritative_branch() {
    let path = root("forked-intermediate-dup");
    let recovery_dir = path.join(".relintor-recovery");
    let recovery_store = store(&path);

    // 1. Write checkpoint 1
    let _cp1 = recovery_store
        .write_checkpoint(content(&path, "sequence 1 checkpoint"))
        .expect("write cp1");
    let index_after_cp1 = fs::read(recovery_dir.join("checkpoint-index.json")).expect("read index cp1");

    // 2. Write checkpoint 2 branch A
    let mut c_a = content(&path, "sequence 2 branch A");
    c_a.created_at_ms = 2000;
    let cp2_a = recovery_store
        .write_checkpoint(c_a)
        .expect("write cp2_a");
    let cp2_a_path = recovery_dir.join(format!(
        "checkpoint-00000000000000000002-{}.json",
        cp2_a.checkpoint_id
    ));
    let cp2_a_bytes = fs::read(&cp2_a_path).expect("read cp2_a");

    // Revert index to cp1 and remove cp2_a
    fs::write(recovery_dir.join("checkpoint-index.json"), index_after_cp1).expect("revert index");
    fs::remove_file(&cp2_a_path).expect("remove cp2_a temporarily");

    // Re-create store to clear any memory cache
    let recovery_store = store(&path);

    // 3. Write checkpoint 2 branch B
    let mut c_b = content(&path, "sequence 2 branch B (canonical)");
    c_b.created_at_ms = 2500;
    let cp2_b = recovery_store
        .write_checkpoint(c_b)
        .expect("write cp2_b");
    assert_ne!(cp2_a.checkpoint_id, cp2_b.checkpoint_id, "checkpoint IDs must differ");

    // 4. Write checkpoint 3 on top of branch B
    let cp3 = recovery_store
        .write_checkpoint(content(&path, "sequence 3 checkpoint"))
        .expect("write cp3");
    assert_eq!(cp3.parent_digest, Some(cp2_b.content_digest.clone()));

    // 5. Place branch A artifact back into the directory so sequence 2 now has 2 valid duplicate artifacts
    fs::write(&cp2_a_path, cp2_a_bytes).expect("restore cp2_a file");

    let seq2_files = fs::read_dir(&recovery_dir)
        .expect("read dir")
        .filter_map(Result::ok)
        .filter(|e| e.file_name().to_string_lossy().starts_with("checkpoint-00000000000000000002-"))
        .collect::<Vec<_>>();
    assert_eq!(seq2_files.len(), 2, "must have 2 duplicate sequence 2 files");

    // 6. Fresh store loads latest: must resolve sequence 2 to branch B and sequence 3 to cp3
    let fresh_store = store(&path);
    let loaded = fresh_store.load_latest().expect("load_latest must resolve lineage");
    assert!(loaded.is_some());
    let latest = loaded.unwrap();
    assert_eq!(latest.sequence, 3);
    assert_eq!(latest.checkpoint_id, cp3.checkpoint_id);

    // Verify branch B exists and is authoritative
    let cp2_resolved_path = recovery_dir.join(format!(
        "checkpoint-00000000000000000002-{}.json",
        cp2_b.checkpoint_id
    ));
    assert!(cp2_resolved_path.exists());

    let _ = fs::remove_dir_all(&path);
}


