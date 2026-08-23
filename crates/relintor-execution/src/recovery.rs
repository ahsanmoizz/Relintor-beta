//! P9 recovery and resume authority.
//!
//! This module deliberately sits below the desktop renderer and beside the
//! P7 scheduler.  It persists authenticated, chained checkpoints; captures
//! bounded workspace/process state; and returns typed resume decisions.  It
//! never grants execution success or verification/completion authority.

use super::{
    AttemptExecutionBoundary, ExecutionError, ExecutionRun, ExecutionRunState,
    RecoveryAttemptTarget,
};
#[cfg(windows)]
use relintor_antigravity::{observable_process_command_digest, process_creation_time_ms};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub const RECOVERY_VERSION: &str = "p9-recovery-v2";
const LEGACY_RECOVERY_VERSION: &str = "p9-recovery-v1";
pub const MAX_CHECKPOINT_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_SNAPSHOT_FILES: usize = 4096;
pub const MAX_SNAPSHOT_BYTES: u64 = 64 * 1024 * 1024;
pub const MAX_UNTRACKED_FILE_BYTES: u64 = 2 * 1024 * 1024;
#[cfg(windows)]
const REPARSE_POINT: u32 = 0x400;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CheckpointKind {
    BeforeMutableAction,
    AfterAtomicAction,
    AfterTaskPersistence,
    BeforeVerification,
    BeforeCompensation,
    EmergencyStop,
    RestartRecovery,
}

impl CheckpointKind {
    fn can_be_automatic_resume_boundary(self) -> bool {
        matches!(
            self,
            Self::AfterAtomicAction
                | Self::AfterTaskPersistence
                | Self::EmergencyStop
                | Self::RestartRecovery
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RecoveryDisposition {
    SafeToResume,
    SafeToResumeAfterProcessReconciliation,
    PreExecutionRetryAuthorized,
    RevalidationRequired,
    BlockedExternal,
    StoppedIncomplete,
    RecoveryRecordCorrupt,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RecoveryClassification {
    PreExecutionPrevented,
    InterruptedAtSafeCheckpoint,
    InterruptedWithoutSafeCheckpoint,
    #[default]
    ExternalStateUncertain,
    RecoveryNotAllowed,
}

fn default_revalidation_disposition() -> RecoveryDisposition {
    RecoveryDisposition::RevalidationRequired
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SessionEndState {
    Active,
    CleanShutdown,
    EmergencyStop,
    CrashDetected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ProcessObservation {
    ProcessGone,
    OwnedProcessStillRunning,
    PidReusedNotOurs,
    ProcessCompletedResultAvailable,
    ProcessStateUnknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ExternalChangeKind {
    NoChange,
    ExpectedOwnChange,
    UserOrExternalChange,
    AuthorityOrSealChange,
    UnknownChange,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DatabaseClassification {
    LocalTest,
    Production,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DatabaseKind {
    Sqlite,
    PostgreSql,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DatabaseCheckpointStatus {
    Success,
    Failed,
    NotAvailable,
    NotApplicable,
    Blocked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CompensationOutcome {
    Applied,
    BlockedExternal,
    HumanApprovalRequired,
    NotAutomaticallyReversible,
    NotEligible,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CompensationActionType {
    FileWrite,
    FileCreation,
    DirectoryCreation,
    PackageOperation,
    LocalTestDatabase,
    ProcessLaunch,
    ExternalApiMutation,
    Deployment,
    UnknownDestructive,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecoveryAuthority {
    pub project_id: String,
    pub mission_id: String,
    pub mission_revision: u64,
    pub p6_seal_hash: String,
    pub standards_registry_id: String,
    pub p7_run_id: String,
    pub checkpoint_sequence: u64,
    pub workspace_identity: String,
    pub source_identity: String,
    pub current_task: Option<String>,
    pub p8_evidence_state: String,
    pub created_at_ms: u64,
    pub integrity_proof: String,
}

impl RecoveryAuthority {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        project_id: impl Into<String>,
        mission_id: impl Into<String>,
        mission_revision: u64,
        p6_seal_hash: impl Into<String>,
        standards_registry_id: impl Into<String>,
        p7_run_id: impl Into<String>,
        workspace_identity: impl Into<String>,
        source_identity: impl Into<String>,
        current_task: Option<String>,
        p8_evidence_state: impl Into<String>,
        created_at_ms: u64,
    ) -> Self {
        Self {
            project_id: project_id.into(),
            mission_id: mission_id.into(),
            mission_revision,
            p6_seal_hash: p6_seal_hash.into(),
            standards_registry_id: standards_registry_id.into(),
            p7_run_id: p7_run_id.into(),
            checkpoint_sequence: 0,
            workspace_identity: workspace_identity.into(),
            source_identity: source_identity.into(),
            current_task,
            p8_evidence_state: p8_evidence_state.into(),
            created_at_ms,
            integrity_proof: String::new(),
        }
    }

    fn identity_tuple(&self) -> (&str, &str, u64, &str, &str, &str, &str, &str, &str) {
        (
            &self.project_id,
            &self.mission_id,
            self.mission_revision,
            &self.p6_seal_hash,
            &self.standards_registry_id,
            &self.p7_run_id,
            &self.workspace_identity,
            &self.source_identity,
            &self.p8_evidence_state,
        )
    }

    fn validate_nonempty(&self) -> Result<(), RecoveryError> {
        if self.project_id.trim().is_empty()
            || self.mission_id.trim().is_empty()
            || self.p6_seal_hash.trim().is_empty()
            || self.standards_registry_id.trim().is_empty()
            || self.p7_run_id.trim().is_empty()
            || self.workspace_identity.trim().is_empty()
            || self.source_identity.trim().is_empty()
        {
            return Err(RecoveryError::Authority(
                "recovery authority identity is incomplete".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceFileRecord {
    pub relative_path: String,
    pub digest: String,
    pub size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceSnapshot {
    pub root: String,
    pub fingerprint: String,
    pub files: Vec<WorkspaceFileRecord>,
    pub file_count: usize,
    pub total_bytes: u64,
    pub captured_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UntrackedFileRecord {
    pub relative_path: String,
    pub digest: String,
    pub size: u64,
    pub content: Option<Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UntrackedFileSnapshot {
    pub root: String,
    pub files: Vec<UntrackedFileRecord>,
    pub omitted_paths: Vec<String>,
    pub total_bytes: u64,
    pub captured_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitWorktreeSnapshot {
    pub available: bool,
    pub root: String,
    pub head: Option<String>,
    pub branch: Option<String>,
    pub dirty: bool,
    pub changed_paths: Vec<String>,
    pub diff_digest: Option<String>,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessOwnershipRecord {
    pub process_id: u32,
    pub process_start_time_ms: Option<u64>,
    pub executable_path: String,
    pub executable_digest: String,
    pub command_digest: String,
    pub run_id: String,
    pub task_id: Option<String>,
    pub attempt_id: Option<String>,
    pub lease_id: Option<String>,
    pub launched_at_ms: u64,
    pub observation: ProcessObservation,
}

impl ProcessOwnershipRecord {
    pub fn identity_digest(&self) -> String {
        sha256_json(&(
            self.process_id,
            self.process_start_time_ms,
            self.executable_path.to_ascii_lowercase(),
            self.executable_digest.as_str(),
            self.command_digest.as_str(),
            self.run_id.as_str(),
            self.task_id.as_deref(),
            self.attempt_id.as_deref(),
            self.lease_id.as_deref(),
        ))
    }

    pub fn binds_to_running_attempt(&self, run: &ExecutionRun) -> bool {
        if self.run_id != run.run_id {
            return false;
        }
        run.attempts.iter().any(|attempt| {
            attempt.state == super::TaskAttemptState::Running
                && self.task_id.as_deref() == Some(attempt.task_id.as_str())
                && self.attempt_id.as_deref() == Some(attempt.attempt_id.as_str())
                && self.lease_id.as_deref() == Some(attempt.lease_id.as_str())
                && run.leases.iter().any(|lease| {
                    lease.lease_id == attempt.lease_id
                        && lease.task_id == attempt.task_id
                        && lease.status == super::LeaseStatus::Active
                })
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionMarker {
    #[serde(default)]
    pub record_version: String,
    pub session_id: String,
    pub run_id: String,
    pub process_id: u32,
    pub process_start_time_ms: Option<u64>,
    pub executable_digest: String,
    pub command_digest: String,
    pub started_at_ms: u64,
    pub ended_at_ms: Option<u64>,
    pub state: SessionEndState,
    pub integrity_tag: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckpointContent {
    pub authority: RecoveryAuthority,
    pub kind: CheckpointKind,
    pub run_snapshot_json: String,
    pub workspace: WorkspaceSnapshot,
    pub untracked: UntrackedFileSnapshot,
    pub git: GitWorktreeSnapshot,
    pub processes: Vec<ProcessOwnershipRecord>,
    pub verification_references: Vec<String>,
    pub reason: String,
    pub created_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckpointRecord {
    pub recovery_version: String,
    pub checkpoint_id: String,
    pub sequence: u64,
    pub parent_digest: Option<String>,
    pub content_digest: String,
    pub content: CheckpointContent,
    pub integrity_tag: String,
}

impl CheckpointRecord {
    pub fn is_safe_to_resume(&self) -> bool {
        if !self.content.kind.can_be_automatic_resume_boundary() {
            return false;
        }
        let Ok(run) = ExecutionRun::restore_json(&self.content.run_snapshot_json) else {
            return false;
        };
        if run
            .attempts
            .iter()
            .any(|attempt| attempt.state == super::TaskAttemptState::Running)
            || run
                .leases
                .iter()
                .any(|lease| lease.status == super::LeaseStatus::Active)
        {
            return false;
        }
        matches!(
            run.state,
            ExecutionRunState::Ready
                | ExecutionRunState::WaitingRetry
                | ExecutionRunState::WaitingDependency
                | ExecutionRunState::TurnEndedIncomplete
                | ExecutionRunState::SafeBoundaryReached
                | ExecutionRunState::StoppedIncomplete
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct CheckpointIndex {
    recovery_version: String,
    mission_id: String,
    mission_revision: u64,
    run_id: String,
    latest_sequence: u64,
    latest_checkpoint_id: String,
    latest_digest: String,
    integrity_tag: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResumeIntegrityResult {
    pub disposition: RecoveryDisposition,
    #[serde(default)]
    pub classification: RecoveryClassification,
    pub reasons: Vec<String>,
    pub changed_paths: Vec<String>,
    pub checkpoint_id: Option<String>,
    pub checkpoint_sequence: Option<u64>,
    pub process_observations: Vec<ProcessObservation>,
    #[serde(default)]
    pub target: Option<RecoveryAttemptTarget>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CrashRecord {
    #[serde(default)]
    pub record_version: String,
    pub session_id: String,
    pub run_id: String,
    pub detected_at_ms: u64,
    pub prior_started_at_ms: u64,
    pub detail: String,
    pub integrity_tag: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RevalidationRecord {
    #[serde(default)]
    pub record_version: String,
    pub revalidation_id: String,
    pub project_id: String,
    pub mission_id: String,
    pub mission_revision: u64,
    pub p7_run_id: String,
    pub checkpoint_id: Option<String>,
    #[serde(default)]
    pub target: Option<RecoveryAttemptTarget>,
    #[serde(default)]
    pub classification: RecoveryClassification,
    #[serde(default = "default_revalidation_disposition")]
    pub disposition: RecoveryDisposition,
    #[serde(default)]
    pub decision: String,
    pub reasons: Vec<String>,
    pub affected_paths: Vec<String>,
    pub required_authorities: Vec<String>,
    pub created_at_ms: u64,
    pub integrity_tag: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DatabaseCheckpointSpec {
    pub identity: String,
    pub kind: DatabaseKind,
    pub classification: DatabaseClassification,
    pub adapter_id: String,
    pub endpoint_redacted: String,
    pub max_duration_ms: u64,
    pub restore_capability: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DatabaseCheckpointResult {
    pub status: DatabaseCheckpointStatus,
    pub database_identity: String,
    pub adapter_id: String,
    pub artifact_path: Option<String>,
    pub artifact_digest: Option<String>,
    pub restore_capability: bool,
    pub detail: String,
}

pub trait TestDatabaseCheckpointHook {
    fn checkpoint(
        &self,
        spec: &DatabaseCheckpointSpec,
        destination: &Path,
    ) -> DatabaseCheckpointResult;

    fn restore(
        &self,
        spec: &DatabaseCheckpointSpec,
        artifact: &Path,
        destination: &Path,
    ) -> DatabaseCheckpointResult;
}

/// SQLite's `VACUUM INTO` is a consistent backup boundary, unlike copying a
/// live database file while a write transaction is in progress.
#[derive(Debug, Default, Clone, Copy)]
pub struct SqliteTestDatabaseHook;

impl TestDatabaseCheckpointHook for SqliteTestDatabaseHook {
    fn checkpoint(
        &self,
        spec: &DatabaseCheckpointSpec,
        destination: &Path,
    ) -> DatabaseCheckpointResult {
        if spec.classification != DatabaseClassification::LocalTest {
            return database_result(
                spec,
                DatabaseCheckpointStatus::Blocked,
                "production/unknown database refused",
            );
        }
        if spec.identity.trim().is_empty()
            || spec.adapter_id != "sqlite.vacuum-into"
            || spec.max_duration_ms == 0
        {
            return database_result(
                spec,
                DatabaseCheckpointStatus::Blocked,
                "SQLite checkpoint requires a bounded registered local-test adapter",
            );
        }
        if spec.kind != DatabaseKind::Sqlite {
            return database_result(
                spec,
                DatabaseCheckpointStatus::NotApplicable,
                "SQLite hook received a non-SQLite database",
            );
        }
        if destination.exists() {
            return database_result(
                spec,
                DatabaseCheckpointStatus::Failed,
                "backup destination already exists",
            );
        }
        let source = PathBuf::from(&spec.endpoint_redacted);
        if !valid_local_database_path(&source, false)
            || !valid_local_database_path(destination, true)
        {
            return database_result(
                spec,
                DatabaseCheckpointStatus::Blocked,
                "SQLite checkpoint path is not a safe local-test file",
            );
        }
        let Ok(connection) = rusqlite::Connection::open(&source) else {
            return database_result(
                spec,
                DatabaseCheckpointStatus::Failed,
                "SQLite test database could not be opened",
            );
        };
        if let Some(parent) = destination.parent() {
            if fs::create_dir_all(parent).is_err() {
                return database_result(
                    spec,
                    DatabaseCheckpointStatus::Failed,
                    "backup directory could not be created",
                );
            }
        }
        let timeout = Duration::from_millis(spec.max_duration_ms);
        let started = Instant::now();
        connection.progress_handler(1_000, Some(move || started.elapsed() >= timeout));
        let destination_text = destination.to_string_lossy().to_string();
        let result = connection.execute("VACUUM INTO ?1", rusqlite::params![destination_text]);
        connection.progress_handler(0, None::<fn() -> bool>);
        if result.is_err() {
            return database_result(
                spec,
                DatabaseCheckpointStatus::Failed,
                "SQLite consistent backup failed",
            );
        }
        let Some(digest) = hash_file(destination).ok().map(|(digest, _)| digest) else {
            return database_result(
                spec,
                DatabaseCheckpointStatus::Failed,
                "SQLite backup artifact could not be hashed",
            );
        };
        DatabaseCheckpointResult {
            status: DatabaseCheckpointStatus::Success,
            database_identity: spec.identity.clone(),
            adapter_id: spec.adapter_id.clone(),
            artifact_path: Some(destination.display().to_string()),
            artifact_digest: Some(digest),
            restore_capability: spec.restore_capability,
            detail: "SQLite VACUUM INTO checkpoint completed".into(),
        }
    }

    fn restore(
        &self,
        spec: &DatabaseCheckpointSpec,
        artifact: &Path,
        destination: &Path,
    ) -> DatabaseCheckpointResult {
        if spec.classification != DatabaseClassification::LocalTest
            || spec.kind != DatabaseKind::Sqlite
            || !spec.restore_capability
            || spec.adapter_id != "sqlite.vacuum-into"
            || spec.max_duration_ms == 0
        {
            return database_result(
                spec,
                DatabaseCheckpointStatus::Blocked,
                "SQLite restore is not authorized for this database",
            );
        }
        if !valid_local_database_path(artifact, false)
            || !valid_local_database_path(destination, true)
        {
            return database_result(
                spec,
                DatabaseCheckpointStatus::Failed,
                "SQLite restore paths are invalid",
            );
        }
        let Ok(connection) = rusqlite::Connection::open_with_flags(
            artifact,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        ) else {
            return database_result(
                spec,
                DatabaseCheckpointStatus::Failed,
                "SQLite restore artifact is not a readable database",
            );
        };
        let integrity = connection
            .query_row("PRAGMA integrity_check", [], |row| row.get::<_, String>(0))
            .ok();
        drop(connection);
        if integrity.as_deref() != Some("ok") {
            return database_result(
                spec,
                DatabaseCheckpointStatus::Failed,
                "SQLite restore artifact failed integrity_check",
            );
        }
        match fs::copy(artifact, destination) {
            Ok(_) => {
                let digest = hash_file(destination).ok().map(|(digest, _)| digest);
                match digest {
                    Some(digest) => DatabaseCheckpointResult {
                        status: DatabaseCheckpointStatus::Success,
                        database_identity: spec.identity.clone(),
                        adapter_id: spec.adapter_id.clone(),
                        artifact_path: Some(destination.display().to_string()),
                        artifact_digest: Some(digest),
                        restore_capability: true,
                        detail: "SQLite test database restored from checkpoint artifact".into(),
                    },
                    None => database_result(
                        spec,
                        DatabaseCheckpointStatus::Failed,
                        "SQLite restored database could not be hashed",
                    ),
                }
            }
            Err(error) => database_result(
                spec,
                DatabaseCheckpointStatus::Failed,
                &format!("SQLite restore failed: {error}"),
            ),
        }
    }
}

fn database_result(
    spec: &DatabaseCheckpointSpec,
    status: DatabaseCheckpointStatus,
    detail: &str,
) -> DatabaseCheckpointResult {
    DatabaseCheckpointResult {
        status,
        database_identity: spec.identity.clone(),
        adapter_id: spec.adapter_id.clone(),
        artifact_path: None,
        artifact_digest: None,
        restore_capability: false,
        detail: detail.into(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompensationPreconditions {
    pub workspace_root: String,
    pub expected_current_digest: Option<String>,
    pub required_snapshot_digest: Option<String>,
    pub external_edit_block: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompensationRegistration {
    pub compensation_id: String,
    pub action_type: CompensationActionType,
    pub relative_path: Option<String>,
    pub target: String,
    pub preconditions: CompensationPreconditions,
    pub snapshot_requirement: String,
    pub reversible: bool,
    pub handler_identity: String,
    pub risk: String,
    pub human_approval_required: bool,
    pub owner_run_id: String,
    pub registered_at_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CompensationAttemptState {
    Planned,
    Started,
    Succeeded,
    Failed,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompensationAttempt {
    pub compensation_id: String,
    pub state: CompensationAttemptState,
    pub outcome: CompensationOutcome,
    pub detail: String,
    pub attempted_at_ms: u64,
}

#[derive(Debug, Clone)]
pub struct CompensationRegistry {
    entries: BTreeMap<String, CompensationRegistration>,
    attempts: Vec<CompensationAttempt>,
    journal: Option<CompensationJournalContext>,
}

#[derive(Debug, Clone)]
struct CompensationJournalContext {
    path: PathBuf,
    key: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct CompensationJournal {
    recovery_version: String,
    entries: BTreeMap<String, CompensationRegistration>,
    attempts: Vec<CompensationAttempt>,
    integrity_tag: String,
}

impl CompensationRegistry {
    /// Open the authenticated compensation journal used by production
    /// recovery.  There is no public in-memory production constructor: the
    /// journal is always bound to the authenticated recovery store.
    pub fn from_store(store: &RecoveryStore) -> Result<Self, RecoveryError> {
        let path = store.root.join("compensation-journal.json");
        let journal = if path.is_file() {
            let mut journal: CompensationJournal = read_json(&path)?;
            let expected = hmac_json(&store.key, &compensation_journal_without_tag(&journal))?;
            if !matches!(
                journal.recovery_version.as_str(),
                RECOVERY_VERSION | LEGACY_RECOVERY_VERSION
            ) || journal.integrity_tag != expected
            {
                return Err(RecoveryError::Corrupt(
                    "compensation journal authentication failed".into(),
                ));
            }
            if journal.recovery_version == LEGACY_RECOVERY_VERSION {
                journal.recovery_version = RECOVERY_VERSION.into();
                let tag = hmac_json(&store.key, &compensation_journal_without_tag(&journal))?;
                journal.integrity_tag = tag;
                if let Err(error) = atomic_write_json(&path, &journal) {
                    if !is_low_storage_error(&error) {
                        return Err(error);
                    }
                }
            }
            journal
        } else {
            CompensationJournal {
                recovery_version: RECOVERY_VERSION.into(),
                entries: BTreeMap::new(),
                attempts: Vec::new(),
                integrity_tag: String::new(),
            }
        };
        Ok(Self {
            entries: journal.entries,
            attempts: journal.attempts,
            journal: Some(CompensationJournalContext {
                path,
                key: store.key.clone(),
            }),
        })
    }

    pub fn entries(&self) -> &BTreeMap<String, CompensationRegistration> {
        &self.entries
    }

    pub fn attempts(&self) -> &[CompensationAttempt] {
        &self.attempts
    }

    pub fn register(
        &mut self,
        registration: CompensationRegistration,
    ) -> Result<(), RecoveryError> {
        if registration.compensation_id.trim().is_empty()
            || registration.owner_run_id.trim().is_empty()
            || registration.handler_identity.trim().is_empty()
        {
            return Err(RecoveryError::Compensation(
                "compensation registration identity is incomplete".into(),
            ));
        }
        if registration.action_type == CompensationActionType::UnknownDestructive
            && (registration.reversible
                || registration.handler_identity != "NOT_AUTOMATICALLY_REVERSIBLE")
        {
            return Err(RecoveryError::Compensation(
                "unknown/destructive actions cannot be auto-reversible".into(),
            ));
        }
        if registration.action_type == CompensationActionType::FileWrite {
            if registration.handler_identity != "p9.file-write.restore-v1"
                || registration
                    .relative_path
                    .as_deref()
                    .is_none_or(str::is_empty)
                || registration.preconditions.workspace_root.trim().is_empty()
            {
                return Err(RecoveryError::Compensation(
                    "file-write compensation must use the trusted Rust handler and path binding"
                        .into(),
                ));
            }
            let root = Path::new(&registration.preconditions.workspace_root);
            let relative = registration.relative_path.as_deref().unwrap_or_default();
            let target = safe_join(root, relative)?;
            if target.display().to_string() != registration.target {
                return Err(RecoveryError::Compensation(
                    "file-write target is not bound to its workspace-relative path".into(),
                ));
            }
        }
        let compensation_id = registration.compensation_id.clone();
        let previous = self.entries.insert(compensation_id.clone(), registration);
        if let Err(error) = self.persist() {
            if let Some(previous) = previous {
                self.entries.insert(compensation_id, previous);
            } else {
                self.entries.remove(&compensation_id);
            }
            return Err(error);
        }
        Ok(())
    }

    pub fn register_file_write(
        &mut self,
        owner_run_id: &str,
        root: &Path,
        relative_path: &str,
        prior: &UntrackedFileRecord,
        post_action_digest: &str,
        now_ms: u64,
    ) -> Result<String, RecoveryError> {
        if prior
            .content
            .as_ref()
            .is_some_and(|content| sha256_bytes(content) != prior.digest)
        {
            return Err(RecoveryError::Compensation(
                "prior file snapshot digest does not match its content".into(),
            ));
        }
        let target = safe_join(root, relative_path)?;
        if !target.is_file()
            || hash_file(&target)
                .ok()
                .is_none_or(|(digest, _)| digest != post_action_digest)
        {
            return Err(RecoveryError::Compensation(
                "post-action file is not present at the registered digest".into(),
            ));
        }
        let id = sha256_json(&(
            owner_run_id,
            relative_path,
            prior.digest.as_str(),
            post_action_digest,
            now_ms,
        ));
        self.register(CompensationRegistration {
            compensation_id: id.clone(),
            action_type: CompensationActionType::FileWrite,
            relative_path: Some(relative_path.replace('\\', "/")),
            target: target.display().to_string(),
            preconditions: CompensationPreconditions {
                workspace_root: root.display().to_string(),
                expected_current_digest: Some(post_action_digest.into()),
                required_snapshot_digest: Some(prior.digest.clone()),
                external_edit_block: true,
            },
            snapshot_requirement: "prior-file-content-or-digest".into(),
            reversible: prior.content.is_some(),
            handler_identity: "p9.file-write.restore-v1".into(),
            risk: "restores only when current content still equals Relintor post-action content"
                .into(),
            human_approval_required: false,
            owner_run_id: owner_run_id.into(),
            registered_at_ms: now_ms,
        })?;
        Ok(id)
    }

    pub fn compensate_file_write(
        &mut self,
        compensation_id: &str,
        prior: &UntrackedFileRecord,
        now_ms: u64,
    ) -> Result<CompensationAttempt, RecoveryError> {
        let registration = self
            .entries
            .get(compensation_id)
            .ok_or_else(|| RecoveryError::Compensation("unknown compensation registration".into()))?
            .clone();
        if registration.action_type != CompensationActionType::FileWrite
            || registration.handler_identity != "p9.file-write.restore-v1"
        {
            return Err(RecoveryError::Compensation(
                "registration handler/type mismatch".into(),
            ));
        }
        let relative = registration.relative_path.as_deref().ok_or_else(|| {
            RecoveryError::Compensation("file-write relative path is missing".into())
        })?;
        if prior.relative_path != relative
            || registration
                .preconditions
                .required_snapshot_digest
                .as_deref()
                != Some(prior.digest.as_str())
            || prior.size
                != prior
                    .content
                    .as_ref()
                    .map_or(prior.size, |content| content.len() as u64)
            || prior
                .content
                .as_ref()
                .is_some_and(|content| sha256_bytes(content) != prior.digest)
        {
            return Err(RecoveryError::Compensation(
                "caller-supplied prior snapshot is not bound to the registration".into(),
            ));
        }
        let target = safe_join(
            Path::new(&registration.preconditions.workspace_root),
            relative,
        )?;
        if target.display().to_string() != registration.target {
            return Err(RecoveryError::Compensation(
                "registered file target changed".into(),
            ));
        }
        if let Some(previous) = self
            .attempts
            .iter()
            .rev()
            .find(|attempt| attempt.compensation_id == compensation_id)
        {
            if matches!(
                previous.state,
                CompensationAttemptState::Planned
                    | CompensationAttemptState::Started
                    | CompensationAttemptState::Unknown
                    | CompensationAttemptState::Succeeded
            ) {
                return Ok(previous.clone());
            }
        }
        let current = hash_file(&target).ok().map(|(digest, _)| digest);
        let planned = CompensationAttempt {
            compensation_id: compensation_id.into(),
            state: CompensationAttemptState::Planned,
            outcome: CompensationOutcome::NotEligible,
            detail: "compensation planned; no mutation has started".into(),
            attempted_at_ms: now_ms,
        };
        self.attempts.push(planned);
        self.persist()?;
        let started = CompensationAttempt {
            compensation_id: compensation_id.into(),
            state: CompensationAttemptState::Started,
            outcome: CompensationOutcome::NotEligible,
            detail: "compensation mutation started under trusted handler".into(),
            attempted_at_ms: now_ms,
        };
        self.attempts.push(started);
        self.persist()?;
        let outcome = if current.as_deref()
            != registration
                .preconditions
                .expected_current_digest
                .as_deref()
        {
            CompensationOutcome::BlockedExternal
        } else if prior.content.is_none() || !registration.reversible {
            CompensationOutcome::NotAutomaticallyReversible
        } else {
            let content = prior.content.clone().expect("checked above");
            let write_result = atomic_write(&target, &content);
            if write_result.is_ok()
                && hash_file(&target)
                    .ok()
                    .is_some_and(|(digest, size)| digest == prior.digest && size == prior.size)
            {
                CompensationOutcome::Applied
            } else {
                CompensationOutcome::NotAutomaticallyReversible
            }
        };
        let detail = match outcome {
            CompensationOutcome::Applied => "prior file content restored".into(),
            CompensationOutcome::BlockedExternal => {
                "current file changed externally; rollback refused".into()
            }
            CompensationOutcome::NotAutomaticallyReversible => {
                "no safe prior content is available".into()
            }
            CompensationOutcome::NotEligible => "file restore failed or target was unsafe".into(),
            CompensationOutcome::HumanApprovalRequired => "human approval is required".into(),
        };
        let attempt = CompensationAttempt {
            compensation_id: compensation_id.into(),
            state: match outcome {
                CompensationOutcome::Applied => CompensationAttemptState::Succeeded,
                CompensationOutcome::BlockedExternal
                | CompensationOutcome::NotAutomaticallyReversible
                | CompensationOutcome::NotEligible
                | CompensationOutcome::HumanApprovalRequired => CompensationAttemptState::Failed,
            },
            outcome,
            detail,
            attempted_at_ms: now_ms,
        };
        self.attempts.push(attempt.clone());
        self.persist()?;
        Ok(attempt)
    }

    pub fn record_non_reversible(
        &mut self,
        owner_run_id: &str,
        action_type: CompensationActionType,
        target: &str,
        now_ms: u64,
    ) -> Result<String, RecoveryError> {
        let id = sha256_json(&(owner_run_id, action_type, target, now_ms));
        self.register(CompensationRegistration {
            compensation_id: id.clone(),
            action_type,
            relative_path: None,
            target: target.into(),
            preconditions: CompensationPreconditions {
                workspace_root: String::new(),
                expected_current_digest: None,
                required_snapshot_digest: None,
                external_edit_block: true,
            },
            snapshot_requirement: "explicit-compensating-action-required".into(),
            reversible: false,
            handler_identity: "NOT_AUTOMATICALLY_REVERSIBLE".into(),
            risk: "unknown or externally governed action".into(),
            human_approval_required: true,
            owner_run_id: owner_run_id.into(),
            registered_at_ms: now_ms,
        })?;
        Ok(id)
    }

    fn persist(&self) -> Result<(), RecoveryError> {
        let Some(context) = &self.journal else {
            return Ok(());
        };
        let mut journal = CompensationJournal {
            recovery_version: RECOVERY_VERSION.into(),
            entries: self.entries.clone(),
            attempts: self.attempts.clone(),
            integrity_tag: String::new(),
        };
        let integrity_tag = hmac_json(&context.key, &compensation_journal_without_tag(&journal))?;
        journal.integrity_tag = integrity_tag;
        atomic_write_json(&context.path, &journal)
    }
}

pub trait ProcessInspector {
    fn observe(&self, record: &ProcessOwnershipRecord) -> ProcessObservation;

    fn terminate(
        &self,
        record: &ProcessOwnershipRecord,
    ) -> Result<ProcessObservation, RecoveryError>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct ConservativeProcessInspector;

impl ProcessInspector for ConservativeProcessInspector {
    fn observe(&self, record: &ProcessOwnershipRecord) -> ProcessObservation {
        if record.process_id == std::process::id() {
            return ProcessObservation::PidReusedNotOurs;
        }
        #[cfg(windows)]
        {
            inspect_windows_process(record)
        }
        #[cfg(not(windows))]
        {
            inspect_non_windows_process(record)
        }
    }

    fn terminate(
        &self,
        record: &ProcessOwnershipRecord,
    ) -> Result<ProcessObservation, RecoveryError> {
        match self.observe(record) {
            ProcessObservation::OwnedProcessStillRunning => {
                #[cfg(windows)]
                {
                    let status = hidden_command("taskkill.exe")
                        .args(["/PID", &record.process_id.to_string(), "/T", "/F"])
                        .status()
                        .map_err(|error| {
                            RecoveryError::Process(format!("stop owned process: {error}"))
                        })?;
                    if status.success() {
                        Ok(ProcessObservation::ProcessGone)
                    } else {
                        Err(RecoveryError::Process(
                            "verified owned process could not be stopped".into(),
                        ))
                    }
                }
                #[cfg(not(windows))]
                {
                    Err(RecoveryError::Process(
                        "verified process termination is unavailable on this host".into(),
                    ))
                }
            }
            ProcessObservation::ProcessStateUnknown => Err(RecoveryError::Process(
                "process identity is incomplete; termination refused".into(),
            )),
            ProcessObservation::PidReusedNotOurs => Ok(ProcessObservation::PidReusedNotOurs),
            ProcessObservation::ProcessGone => Ok(ProcessObservation::ProcessGone),
            ProcessObservation::ProcessCompletedResultAvailable => {
                Ok(ProcessObservation::ProcessCompletedResultAvailable)
            }
        }
    }
}

#[cfg(not(windows))]
fn inspect_non_windows_process(record: &ProcessOwnershipRecord) -> ProcessObservation {
    let Some(image) = process_image_name(record.process_id) else {
        return ProcessObservation::ProcessGone;
    };
    let expected = Path::new(&record.executable_path)
        .file_name()
        .map(|value| value.to_string_lossy().to_ascii_lowercase());
    if expected.as_deref() != Some(image.to_ascii_lowercase().as_str()) {
        return ProcessObservation::PidReusedNotOurs;
    }
    // A PID/image match is not enough to authorize a kill or adoption. The
    // portable fallback deliberately reports UNKNOWN until the host provides
    // verifiable process creation and argv identity.
    ProcessObservation::ProcessStateUnknown
}

#[cfg(windows)]
fn inspect_windows_process(record: &ProcessOwnershipRecord) -> ProcessObservation {
    use std::ffi::c_void;

    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
    const PROCESS_VM_READ: u32 = 0x0010;
    const ERROR_ACCESS_DENIED: u32 = 5;

    #[link(name = "kernel32")]
    extern "system" {
        fn OpenProcess(desired_access: u32, inherit_handle: i32, process_id: u32) -> *mut c_void;
        fn QueryFullProcessImageNameW(
            process: *mut c_void,
            flags: u32,
            name: *mut u16,
            size: *mut u32,
        ) -> i32;
        fn CloseHandle(object: *mut c_void) -> i32;
        fn GetLastError() -> u32;
    }

    let handle = unsafe {
        OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ,
            0,
            record.process_id,
        )
    };
    if handle.is_null() {
        return if unsafe { GetLastError() } == ERROR_ACCESS_DENIED {
            ProcessObservation::ProcessStateUnknown
        } else {
            ProcessObservation::ProcessGone
        };
    }

    let close = |handle: *mut c_void| unsafe {
        CloseHandle(handle);
    };
    let result = (|| {
        let mut image_buffer = vec![0_u16; 32_768];
        let mut image_length = image_buffer.len() as u32;
        if unsafe {
            QueryFullProcessImageNameW(handle, 0, image_buffer.as_mut_ptr(), &mut image_length)
        } == 0
        {
            return ProcessObservation::ProcessStateUnknown;
        }
        let image = String::from_utf16(&image_buffer[..image_length as usize]).ok();
        let Some(image) = image else {
            return ProcessObservation::ProcessStateUnknown;
        };
        if normalize_windows_path(&image) != normalize_windows_path(&record.executable_path) {
            return ProcessObservation::PidReusedNotOurs;
        }
        let Ok(bytes) = fs::read(&image) else {
            return ProcessObservation::ProcessStateUnknown;
        };
        if sha256_bytes(&bytes) != record.executable_digest {
            return ProcessObservation::PidReusedNotOurs;
        }
        let Some(expected_start) = record.process_start_time_ms else {
            return ProcessObservation::ProcessStateUnknown;
        };
        let Some(actual_start) = process_creation_time_ms(record.process_id) else {
            return ProcessObservation::ProcessStateUnknown;
        };
        if actual_start != i128::from(expected_start) {
            return ProcessObservation::PidReusedNotOurs;
        }
        if !record.command_digest.starts_with("p9-command-v2:") {
            return ProcessObservation::ProcessStateUnknown;
        }
        let Some(command_line) = read_windows_command_line(handle) else {
            return ProcessObservation::ProcessStateUnknown;
        };
        let Some(argv) = parse_windows_command_line(&command_line) else {
            return ProcessObservation::ProcessStateUnknown;
        };
        if argv.is_empty() {
            return ProcessObservation::ProcessStateUnknown;
        }
        let args = argv
            .into_iter()
            .skip(1)
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        let digest = observable_process_command_digest(Path::new(&record.executable_path), &args);
        if digest != record.command_digest {
            return ProcessObservation::PidReusedNotOurs;
        }
        ProcessObservation::OwnedProcessStillRunning
    })();
    close(handle);
    result
}

#[cfg(windows)]
fn read_windows_command_line(handle: *mut std::ffi::c_void) -> Option<Vec<u16>> {
    use std::mem::{size_of, MaybeUninit};

    #[repr(C)]
    struct ProcessBasicInformation {
        reserved: *mut std::ffi::c_void,
        peb_base_address: *mut std::ffi::c_void,
        reserved2: [*mut std::ffi::c_void; 2],
        unique_process_id: usize,
        reserved3: *mut std::ffi::c_void,
    }
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct UnicodeString {
        length: u16,
        maximum_length: u16,
        buffer: *mut u16,
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn ReadProcessMemory(
            process: *mut std::ffi::c_void,
            address: *const std::ffi::c_void,
            buffer: *mut std::ffi::c_void,
            size: usize,
            read: *mut usize,
        ) -> i32;
    }
    #[link(name = "ntdll")]
    extern "system" {
        fn NtQueryInformationProcess(
            process: *mut std::ffi::c_void,
            information_class: u32,
            information: *mut std::ffi::c_void,
            information_length: u32,
            return_length: *mut u32,
        ) -> i32;
    }

    let mut basic = MaybeUninit::<ProcessBasicInformation>::uninit();
    let mut returned = 0_u32;
    let status = unsafe {
        NtQueryInformationProcess(
            handle,
            0,
            basic.as_mut_ptr().cast(),
            size_of::<ProcessBasicInformation>() as u32,
            &mut returned,
        )
    };
    if status != 0 {
        return None;
    }
    let basic = unsafe { basic.assume_init() };
    let parameters_address = basic.peb_base_address as usize
        + if cfg!(target_pointer_width = "64") {
            0x20
        } else {
            0x10
        };
    let mut parameters = MaybeUninit::<*mut std::ffi::c_void>::uninit();
    let mut read = 0_usize;
    if unsafe {
        ReadProcessMemory(
            handle,
            parameters_address as *const std::ffi::c_void,
            parameters.as_mut_ptr().cast(),
            size_of::<*mut std::ffi::c_void>(),
            &mut read,
        )
    } == 0
        || read != size_of::<*mut std::ffi::c_void>()
    {
        return None;
    }
    let parameters = unsafe { parameters.assume_init() } as usize;
    let command_line_address = parameters
        + if cfg!(target_pointer_width = "64") {
            0x70
        } else {
            0x40
        };
    let mut command_line = MaybeUninit::<UnicodeString>::uninit();
    read = 0;
    if unsafe {
        ReadProcessMemory(
            handle,
            command_line_address as *const std::ffi::c_void,
            command_line.as_mut_ptr().cast(),
            size_of::<UnicodeString>(),
            &mut read,
        )
    } == 0
        || read != size_of::<UnicodeString>()
    {
        return None;
    }
    let command_line = unsafe { command_line.assume_init() };
    if command_line.length == 0
        || command_line.length > 60 * 1024
        || command_line.length % 2 != 0
        || command_line.buffer.is_null()
    {
        return None;
    }
    let mut buffer = vec![0_u16; command_line.length as usize / 2];
    read = 0;
    if unsafe {
        ReadProcessMemory(
            handle,
            command_line.buffer.cast(),
            buffer.as_mut_ptr().cast(),
            command_line.length as usize,
            &mut read,
        )
    } == 0
        || read != command_line.length as usize
    {
        return None;
    }
    Some(buffer)
}

#[cfg(windows)]
fn parse_windows_command_line(command_line: &[u16]) -> Option<Vec<std::ffi::OsString>> {
    use std::ffi::{c_void, OsString};
    use std::os::windows::ffi::OsStringExt;
    use std::slice;
    #[link(name = "shell32")]
    extern "system" {
        fn CommandLineToArgvW(command_line: *const u16, argc: *mut i32) -> *mut *mut u16;
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn LocalFree(memory: *mut c_void) -> *mut c_void;
    }
    let mut input = command_line.to_vec();
    input.push(0);
    let mut argc = 0_i32;
    let argv = unsafe { CommandLineToArgvW(input.as_ptr(), &mut argc) };
    if argv.is_null() || argc < 0 {
        return None;
    }
    let pointers = unsafe { slice::from_raw_parts(argv, argc as usize) };
    let result = pointers
        .iter()
        .map(|pointer| {
            if pointer.is_null() {
                return None;
            }
            let mut length = 0_usize;
            while unsafe { *pointer.add(length) } != 0 {
                length += 1;
                if length > 32_768 {
                    return None;
                }
            }
            Some(OsString::from_wide(unsafe {
                slice::from_raw_parts(*pointer, length)
            }))
        })
        .collect::<Option<Vec<_>>>();
    unsafe {
        LocalFree(argv.cast());
    }
    result
}

#[cfg(windows)]
fn normalize_windows_path(value: &str) -> String {
    let mut normalized = value.replace('/', "\\");
    if let Some(stripped) = normalized.strip_prefix(r"\\?\UNC\") {
        normalized = format!(r"\\{stripped}");
    } else if let Some(stripped) = normalized.strip_prefix(r"\\?\") {
        normalized = stripped.to_string();
    }
    normalized.to_ascii_lowercase()
}

#[cfg(not(windows))]
fn process_image_name(pid: u32) -> Option<String> {
    let path = PathBuf::from(format!("/proc/{pid}/comm"));
    fs::read_to_string(path)
        .ok()
        .map(|value| value.trim().into())
}

fn hidden_command(program: &str) -> Command {
    let command = Command::new(program);
    #[cfg(windows)]
    let mut command = command;
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    command
}

#[derive(Debug, Clone)]
pub struct RecoveryStore {
    root: PathBuf,
    key: Vec<u8>,
}

impl RecoveryStore {
    /// Production callers must obtain `key` from the existing OS keychain
    /// authority before constructing this store.  This constructor refuses
    /// short keys and never derives a key from a path or timestamp.
    pub fn new(root: impl Into<PathBuf>, key: Vec<u8>) -> Result<Self, RecoveryError> {
        if key.len() != 32 {
            return Err(RecoveryError::Storage(
                "recovery key must be exactly 32 bytes".into(),
            ));
        }
        let root = root.into();
        fs::create_dir_all(&root).map_err(io_error)?;
        Ok(Self { root, key })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn write_checkpoint(
        &self,
        mut content: CheckpointContent,
    ) -> Result<CheckpointRecord, RecoveryError> {
        content.authority.validate_nonempty()?;
        if content.run_snapshot_json.len() > MAX_CHECKPOINT_BYTES {
            return Err(RecoveryError::Storage(
                "execution snapshot exceeds checkpoint size limit".into(),
            ));
        }
        let serialized_content = serde_json::to_vec(&content)
            .map_err(|error| RecoveryError::Corrupt(error.to_string()))?;
        if contains_secret_material(&serialized_content)
            || contains_secret_material(content.run_snapshot_json.as_bytes())
        {
            return Err(RecoveryError::Storage(
                "checkpoint content contains secret-like material and was refused".into(),
            ));
        }
        let index = self.load_index()?;
        let next_sequence = index
            .as_ref()
            .map_or(1, |item| item.latest_sequence.saturating_add(1));
        if content.authority.checkpoint_sequence != 0
            && content.authority.checkpoint_sequence != next_sequence
        {
            return Err(RecoveryError::Chain(
                "checkpoint sequence does not extend the trusted chain".into(),
            ));
        }
        content.authority.checkpoint_sequence = next_sequence;
        let parent_digest = index.as_ref().map(|item| item.latest_digest.clone());
        let checkpoint_id = sha256_json(&(
            RECOVERY_VERSION,
            content.authority.identity_tuple(),
            next_sequence,
            parent_digest.as_deref(),
            content.kind,
            content.created_at_ms,
        ));
        content.authority.integrity_proof = sha256_json(&(
            content.authority.identity_tuple(),
            next_sequence,
            parent_digest.as_deref(),
        ));
        let content_digest = sha256_json(&content);
        let mut record = CheckpointRecord {
            recovery_version: RECOVERY_VERSION.into(),
            checkpoint_id: checkpoint_id.clone(),
            sequence: next_sequence,
            parent_digest,
            content_digest: content_digest.clone(),
            content,
            integrity_tag: String::new(),
        };
        let integrity_tag = hmac_json(&self.key, &record_without_tag(&record))?;
        record.integrity_tag = integrity_tag;
        let path = self.checkpoint_path(next_sequence, &checkpoint_id);
        atomic_write_json(&path, &record)?;
        let mut new_index = CheckpointIndex {
            recovery_version: RECOVERY_VERSION.into(),
            mission_id: record.content.authority.mission_id.clone(),
            mission_revision: record.content.authority.mission_revision,
            run_id: record.content.authority.p7_run_id.clone(),
            latest_sequence: next_sequence,
            latest_checkpoint_id: checkpoint_id,
            latest_digest: content_digest,
            integrity_tag: String::new(),
        };
        let integrity_tag = hmac_json(&self.key, &index_without_tag(&new_index))?;
        new_index.integrity_tag = integrity_tag;
        atomic_write_json(&self.root.join("checkpoint-index.json"), &new_index)?;
        Ok(record)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn write_run_checkpoint(
        &self,
        run: &ExecutionRun,
        mut authority: RecoveryAuthority,
        kind: CheckpointKind,
        workspace: WorkspaceSnapshot,
        untracked: UntrackedFileSnapshot,
        git: GitWorktreeSnapshot,
        processes: Vec<ProcessOwnershipRecord>,
        verification_references: Vec<String>,
        reason: impl Into<String>,
        created_at_ms: u64,
    ) -> Result<CheckpointRecord, RecoveryError> {
        authority.p7_run_id = run.run_id.clone();
        authority.workspace_identity = workspace.fingerprint.clone();
        authority.current_task = run
            .attempts
            .iter()
            .rev()
            .find(|attempt| matches!(attempt.state, super::TaskAttemptState::Running))
            .map(|attempt| attempt.task_id.clone());
        let run_snapshot_json = run
            .snapshot_json()
            .map_err(|error| RecoveryError::Storage(error.to_string()))?;
        self.write_checkpoint(CheckpointContent {
            authority,
            kind,
            run_snapshot_json,
            workspace,
            untracked,
            git,
            processes,
            verification_references,
            reason: reason.into(),
            created_at_ms,
        })
    }

    pub fn load_latest(&self) -> Result<Option<CheckpointRecord>, RecoveryError> {
        let Some(index) = self.load_index()? else {
            if self.has_checkpoint_files()? {
                return Err(RecoveryError::Chain(
                    "checkpoint index is missing while checkpoint artifacts remain".into(),
                ));
            }
            return Ok(None);
        };
        let mut previous: Option<String> = None;
        let mut validated_records = Vec::with_capacity(index.latest_sequence as usize);
        for sequence in 1..=index.latest_sequence {
            let path = self.find_checkpoint(sequence)?;
            let record: CheckpointRecord = read_json(&path)?;
            self.validate_record(&record)?;
            if record.sequence != sequence || record.parent_digest != previous {
                return Err(RecoveryError::Chain(
                    "checkpoint parent chain is broken".into(),
                ));
            }
            previous = Some(record.content_digest.clone());
            validated_records.push((path, record));
            if sequence == index.latest_sequence {
                let record = &validated_records
                    .last()
                    .expect("the newest checkpoint was just validated")
                    .1;
                if record.checkpoint_id != index.latest_checkpoint_id
                    || record.content_digest != index.latest_digest
                {
                    return Err(RecoveryError::Chain(
                        "checkpoint index does not match the newest artifact".into(),
                    ));
                }
            }
        }
        let mut current_index = index;
        let mut migration_deferred = false;
        for (path, record) in &mut validated_records {
            if record.recovery_version == LEGACY_RECOVERY_VERSION {
                let legacy_version = record.recovery_version.clone();
                let legacy_tag = record.integrity_tag.clone();
                record.recovery_version = RECOVERY_VERSION.into();
                let tag = hmac_json(&self.key, &record_without_tag(record))?;
                record.integrity_tag = tag;
                if let Err(error) = atomic_write_json(path, record) {
                    if !is_low_storage_error(&error) {
                        return Err(error);
                    }
                    record.recovery_version = legacy_version;
                    record.integrity_tag = legacy_tag;
                    migration_deferred = true;
                    break;
                }
            }
        }
        if !migration_deferred && current_index.recovery_version == LEGACY_RECOVERY_VERSION {
            let legacy_version = current_index.recovery_version.clone();
            let legacy_tag = current_index.integrity_tag.clone();
            current_index.recovery_version = RECOVERY_VERSION.into();
            let tag = hmac_json(&self.key, &index_without_tag(&current_index))?;
            current_index.integrity_tag = tag;
            if let Err(error) =
                atomic_write_json(&self.root.join("checkpoint-index.json"), &current_index)
            {
                if !is_low_storage_error(&error) {
                    return Err(error);
                }
                current_index.recovery_version = legacy_version;
                current_index.integrity_tag = legacy_tag;
            }
        }
        validated_records
            .pop()
            .map(|(_, record)| Some(record))
            .ok_or_else(|| {
                RecoveryError::Chain("checkpoint index has no valid newest checkpoint".into())
            })
    }

    pub fn load_run(&self) -> Result<Option<ExecutionRun>, RecoveryError> {
        let Some(record) = self.load_latest()? else {
            return Ok(None);
        };
        ExecutionRun::restore_json(&record.content.run_snapshot_json)
            .map(Some)
            .map_err(|error| RecoveryError::Corrupt(error.to_string()))
    }

    pub fn begin_session(
        &self,
        run_id: &str,
        executable_digest: &str,
        command_digest: &str,
        now_ms: u64,
    ) -> Result<Option<CrashRecord>, RecoveryError> {
        let prior = self.load_session()?;
        let process_start_time_ms = current_process_start_time_ms();
        let crash = match prior.as_ref() {
            Some(marker)
                if marker.state == SessionEndState::Active
                    && marker.run_id == run_id
                    && (marker.process_id != std::process::id()
                        || marker.process_start_time_ms != process_start_time_ms
                        || marker.executable_digest != executable_digest
                        || marker.command_digest != command_digest) =>
            {
                let crash = CrashRecord {
                    record_version: RECOVERY_VERSION.into(),
                    session_id: marker.session_id.clone(),
                    run_id: marker.run_id.clone(),
                    detected_at_ms: now_ms,
                    prior_started_at_ms: marker.started_at_ms,
                    detail: "previous session remained ACTIVE across restart; unclean termination detected".into(),
                    integrity_tag: String::new(),
                };
                self.write_crash_record(crash.clone())?;
                Some(crash)
            }
            _ => None,
        };
        let mut marker = SessionMarker {
            record_version: RECOVERY_VERSION.into(),
            session_id: sha256_json(&(run_id, now_ms, std::process::id())),
            run_id: run_id.into(),
            process_id: std::process::id(),
            process_start_time_ms,
            executable_digest: executable_digest.into(),
            command_digest: command_digest.into(),
            started_at_ms: now_ms,
            ended_at_ms: None,
            state: SessionEndState::Active,
            integrity_tag: String::new(),
        };
        let integrity_tag = hmac_json(&self.key, &session_without_tag(&marker))?;
        marker.integrity_tag = integrity_tag;
        atomic_write_json(&self.root.join("session-marker.json"), &marker)?;
        Ok(crash)
    }

    pub fn end_session(&self, state: SessionEndState, now_ms: u64) -> Result<(), RecoveryError> {
        let Some(mut marker) = self.load_session()? else {
            return Err(RecoveryError::Storage(
                "cannot close a missing recovery session".into(),
            ));
        };
        if state == SessionEndState::Active {
            return Err(RecoveryError::Storage(
                "session cannot be closed as ACTIVE".into(),
            ));
        }
        marker.state = state;
        marker.ended_at_ms = Some(now_ms);
        let integrity_tag = hmac_json(&self.key, &session_without_tag(&marker))?;
        marker.integrity_tag = integrity_tag;
        atomic_write_json(&self.root.join("session-marker.json"), &marker)
    }

    pub fn load_session(&self) -> Result<Option<SessionMarker>, RecoveryError> {
        let path = self.root.join("session-marker.json");
        if !path.is_file() {
            return Ok(None);
        }
        let mut marker: SessionMarker = read_json(&path)?;
        let authenticated = if marker.record_version.is_empty() {
            marker.integrity_tag == hmac_json(&self.key, &legacy_session_without_tag(&marker))?
        } else if marker.record_version == RECOVERY_VERSION {
            marker.integrity_tag == hmac_json(&self.key, &session_without_tag(&marker))?
        } else {
            false
        };
        if !authenticated {
            return Err(RecoveryError::Corrupt(
                "session marker authentication failed".into(),
            ));
        }
        if marker.record_version.is_empty() {
            marker.record_version = RECOVERY_VERSION.into();
            let tag = hmac_json(&self.key, &session_without_tag(&marker))?;
            marker.integrity_tag = tag;
            if let Err(error) = atomic_write_json(&path, &marker) {
                if !is_low_storage_error(&error) {
                    return Err(error);
                }
            }
        }
        Ok(Some(marker))
    }

    pub fn write_revalidation(
        &self,
        mut record: RevalidationRecord,
    ) -> Result<RevalidationRecord, RecoveryError> {
        if record.project_id.is_empty()
            || record.mission_id.is_empty()
            || record.p7_run_id.is_empty()
        {
            return Err(RecoveryError::Authority(
                "revalidation authority identity is incomplete".into(),
            ));
        }
        record.record_version = RECOVERY_VERSION.into();
        record.integrity_tag.clear();
        let integrity_tag = hmac_json(&self.key, &revalidation_without_tag(&record))?;
        record.integrity_tag = integrity_tag;
        let path = self
            .root
            .join(format!("revalidation-{}.json", record.revalidation_id));
        atomic_write_json(&path, &record)?;
        Ok(record)
    }

    pub fn load_latest_revalidation(&self) -> Result<Option<RevalidationRecord>, RecoveryError> {
        let mut records = fs::read_dir(&self.root)
            .map_err(io_error)?
            .filter_map(Result::ok)
            .map(|entry| (entry.path(), entry.file_name()))
            .filter(|(path, _)| {
                path.file_name().is_some_and(|name| {
                    name.to_string_lossy().starts_with("revalidation-")
                        && name.to_string_lossy().ends_with(".json")
                })
            })
            .map(|(path, _)| read_json::<RevalidationRecord>(&path).map(|record| (path, record)))
            .collect::<Result<Vec<_>, _>>()?;
        for (path, record) in &mut records {
            self.validate_revalidation(record)?;
            let legacy_current_tag = record.record_version == RECOVERY_VERSION
                && record.integrity_tag
                    == hmac_json(&self.key, &revalidation_without_target_tag(record))?;
            if record.record_version.is_empty() || legacy_current_tag {
                record.record_version = RECOVERY_VERSION.into();
                let tag = hmac_json(&self.key, &revalidation_without_tag(record))?;
                record.integrity_tag = tag;
                if let Err(error) = atomic_write_json(path, record) {
                    if !is_low_storage_error(&error) {
                        return Err(error);
                    }
                }
            }
        }
        records.sort_by(|left, right| {
            left.1
                .created_at_ms
                .cmp(&right.1.created_at_ms)
                .then_with(|| left.1.revalidation_id.cmp(&right.1.revalidation_id))
        });
        Ok(records.pop().map(|(_, record)| record))
    }

    pub fn compensation_registry(&self) -> Result<CompensationRegistry, RecoveryError> {
        CompensationRegistry::from_store(self)
    }

    fn load_index(&self) -> Result<Option<CheckpointIndex>, RecoveryError> {
        let path = self.root.join("checkpoint-index.json");
        if !path.is_file() {
            return Ok(None);
        }
        let index: CheckpointIndex = read_json(&path)?;
        let expected = hmac_json(&self.key, &index_without_tag(&index))?;
        if index.integrity_tag != expected {
            return Err(RecoveryError::Corrupt(
                "checkpoint index authentication failed".into(),
            ));
        }
        if !matches!(
            index.recovery_version.as_str(),
            RECOVERY_VERSION | LEGACY_RECOVERY_VERSION
        ) || index.latest_sequence == 0
        {
            return Err(RecoveryError::Corrupt(
                "checkpoint index version/sequence is invalid".into(),
            ));
        }
        Ok(Some(index))
    }

    fn validate_record(&self, record: &CheckpointRecord) -> Result<(), RecoveryError> {
        let version_supported = matches!(
            record.recovery_version.as_str(),
            RECOVERY_VERSION | LEGACY_RECOVERY_VERSION
        );
        let authenticated =
            record.integrity_tag == hmac_json(&self.key, &record_without_tag(record))?;
        if !version_supported
            || record.sequence == 0
            || record.content.authority.checkpoint_sequence != record.sequence
            || record.content_digest != sha256_json(&record.content)
            || !authenticated
        {
            return Err(RecoveryError::Corrupt(
                "checkpoint authentication or content digest failed".into(),
            ));
        }
        let expected_authority_proof = sha256_json(&(
            record.content.authority.identity_tuple(),
            record.sequence,
            record.parent_digest.as_deref(),
        ));
        if record.content.authority.integrity_proof != expected_authority_proof {
            return Err(RecoveryError::Corrupt(
                "checkpoint authority proof does not match its chain position".into(),
            ));
        }
        record.content.authority.validate_nonempty()?;
        if record.content.run_snapshot_json.len() > MAX_CHECKPOINT_BYTES {
            return Err(RecoveryError::Corrupt(
                "checkpoint payload exceeds size limit".into(),
            ));
        }
        Ok(())
    }

    fn validate_revalidation(&self, record: &RevalidationRecord) -> Result<(), RecoveryError> {
        let expected = hmac_json(&self.key, &revalidation_without_tag(record))?;
        let current_without_target_expected =
            hmac_json(&self.key, &revalidation_without_target_tag(record))?;
        let legacy_expected = hmac_json(&self.key, &legacy_revalidation_without_tag(record))?;
        let legacy_with_decision_expected = hmac_json(
            &self.key,
            &legacy_revalidation_with_decision_without_tag(record),
        )?;
        if record.revalidation_id.trim().is_empty()
            || record.project_id.trim().is_empty()
            || record.mission_id.trim().is_empty()
            || record.p7_run_id.trim().is_empty()
            || match record.record_version.as_str() {
                value if value == RECOVERY_VERSION => {
                    record.integrity_tag != expected
                        && record.integrity_tag != current_without_target_expected
                }
                "" => {
                    record.integrity_tag != legacy_expected
                        && record.integrity_tag != legacy_with_decision_expected
                }
                _ => true,
            }
        {
            return Err(RecoveryError::Corrupt(
                "revalidation authority authentication failed".into(),
            ));
        }
        Ok(())
    }

    fn checkpoint_path(&self, sequence: u64, id: &str) -> PathBuf {
        self.root
            .join(format!("checkpoint-{sequence:020}-{id}.json"))
    }

    fn find_checkpoint(&self, sequence: u64) -> Result<PathBuf, RecoveryError> {
        let mut matches = fs::read_dir(&self.root)
            .map_err(io_error)?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name().is_some_and(|name| {
                    name.to_string_lossy()
                        .starts_with(&format!("checkpoint-{sequence:020}-"))
                        && name.to_string_lossy().ends_with(".json")
                })
            })
            .collect::<Vec<_>>();
        matches.sort();
        match matches.as_slice() {
            [path] => Ok(path.clone()),
            [] => Err(RecoveryError::Chain(format!(
                "checkpoint sequence {sequence} is missing"
            ))),
            _ => Err(RecoveryError::Chain(format!(
                "checkpoint sequence {sequence} has duplicate artifacts"
            ))),
        }
    }

    fn has_checkpoint_files(&self) -> Result<bool, RecoveryError> {
        Ok(fs::read_dir(&self.root)
            .map_err(io_error)?
            .filter_map(Result::ok)
            .any(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("checkpoint-")
            }))
    }

    fn write_crash_record(&self, mut record: CrashRecord) -> Result<(), RecoveryError> {
        record.integrity_tag.clear();
        let integrity_tag = hmac_json(&self.key, &crash_without_tag(&record))?;
        record.integrity_tag = integrity_tag;
        atomic_write_json(&self.root.join("crash-record.json"), &record)
    }
}

#[derive(Debug, Clone)]
pub struct RecoveryCoordinator {
    pub store: RecoveryStore,
}

impl RecoveryCoordinator {
    pub fn new(store: RecoveryStore) -> Self {
        Self { store }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn checkpoint_run(
        &self,
        run: &ExecutionRun,
        authority: RecoveryAuthority,
        kind: CheckpointKind,
        root: &Path,
        processes: Vec<ProcessOwnershipRecord>,
        verification_references: Vec<String>,
        reason: impl Into<String>,
        now_ms: u64,
    ) -> Result<CheckpointRecord, RecoveryError> {
        let workspace = WorkspaceSnapshot::capture(root, now_ms)?;
        let untracked = UntrackedFileSnapshot::capture(root, now_ms)?;
        let git = GitWorktreeSnapshot::capture(root)?;
        self.store.write_run_checkpoint(
            run,
            authority,
            kind,
            workspace,
            untracked,
            git,
            processes,
            verification_references,
            reason,
            now_ms,
        )
    }

    pub fn reconcile_processes(
        &self,
        records: &[ProcessOwnershipRecord],
        inspector: &dyn ProcessInspector,
    ) -> Vec<ProcessObservation> {
        records
            .iter()
            .map(|record| inspector.observe(record))
            .collect()
    }

    pub fn resume_run(
        &self,
        run: &mut ExecutionRun,
        expected: &RecoveryAuthority,
        root: &Path,
        inspector: &dyn ProcessInspector,
        now_ms: u64,
        continue_turn: bool,
    ) -> Result<ResumeIntegrityResult, RecoveryError> {
        let result = self.resume_integrity_for_run(run, expected, root, inspector, now_ms)?;
        if matches!(
            result.disposition,
            RecoveryDisposition::SafeToResume
                | RecoveryDisposition::SafeToResumeAfterProcessReconciliation
                | RecoveryDisposition::PreExecutionRetryAuthorized
                | RecoveryDisposition::StoppedIncomplete
        ) {
            if continue_turn {
                run.start_next_turn(now_ms)
                    .map_err(RecoveryError::Execution)?;
            } else if run.state == ExecutionRunState::Running
                && result.disposition == RecoveryDisposition::SafeToResumeAfterProcessReconciliation
            {
                // The exact owned process is already the live execution
                // boundary. Reconciliation must not reset it to READY or
                // mint a second continuation event.
            } else {
                run.resume_from_recovery(result.disposition, now_ms)
                    .map_err(RecoveryError::Execution)?;
            }
        }
        Ok(result)
    }

    pub fn emergency_stop(
        &self,
        run: &mut ExecutionRun,
        authority: RecoveryAuthority,
        root: &Path,
        reason: &str,
        now_ms: u64,
    ) -> Result<CheckpointRecord, RecoveryError> {
        run.request_stop(None, reason, now_ms)
            .map_err(RecoveryError::Execution)?;
        run.reach_safe_boundary(now_ms)
            .map_err(RecoveryError::Execution)?;
        run.mark_stopped_incomplete(reason, now_ms)
            .map_err(RecoveryError::Execution)?;
        self.checkpoint_run(
            run,
            authority,
            CheckpointKind::EmergencyStop,
            root,
            Vec::new(),
            Vec::new(),
            reason,
            now_ms,
        )
    }

    pub fn resume_integrity(
        &self,
        expected: &RecoveryAuthority,
        root: &Path,
        inspector: &dyn ProcessInspector,
        now_ms: u64,
    ) -> Result<ResumeIntegrityResult, RecoveryError> {
        let Some(record) = self.store.load_latest()? else {
            return Ok(ResumeIntegrityResult {
                disposition: RecoveryDisposition::BlockedExternal,
                classification: RecoveryClassification::RecoveryNotAllowed,
                reasons: vec!["no trusted recovery checkpoint is available".into()],
                changed_paths: Vec::new(),
                checkpoint_id: None,
                checkpoint_sequence: None,
                process_observations: Vec::new(),
                target: None,
            });
        };
        let mut reasons = Vec::new();
        if record.content.authority.identity_tuple() != expected.identity_tuple()
            || record.content.authority.p6_seal_hash != expected.p6_seal_hash
            || record.content.authority.standards_registry_id != expected.standards_registry_id
        {
            reasons.push(
                "P6, registry, mission, revision, P7 run, or workspace authority identity changed"
                    .into(),
            );
        }
        let current_workspace = WorkspaceSnapshot::capture(root, now_ms)?;
        let changed_paths = diff_workspace(&record.content.workspace, &current_workspace);
        let current_git = GitWorktreeSnapshot::capture(root)?;
        let git_changed_paths = diff_git_worktree(&record.content.git, &current_git);
        let mut changed_paths = changed_paths;
        changed_paths.extend(git_changed_paths);
        changed_paths.sort();
        changed_paths.dedup();
        if current_workspace.fingerprint != record.content.workspace.fingerprint {
            reasons.push(
                "workspace/source fingerprint changed after the last trusted checkpoint".into(),
            );
        }
        if git_worktree_changed(&record.content.git, &current_git) {
            reasons
                .push("Git HEAD, branch, index, or worktree state changed after checkpoint".into());
        }
        let process_observations = record
            .content
            .processes
            .iter()
            .map(|process| inspector.observe(process))
            .collect::<Vec<_>>();
        if process_observations.contains(&ProcessObservation::PidReusedNotOurs) {
            reasons.push("a recorded PID now identifies a different executable/process".into());
        }
        if process_observations.contains(&ProcessObservation::ProcessStateUnknown) {
            reasons.push("owned process identity could not be proven after restart".into());
        }
        let run = ExecutionRun::restore_json(&record.content.run_snapshot_json)
            .map_err(|error| RecoveryError::Corrupt(error.to_string()))?;
        if run.run_id != expected.p7_run_id {
            reasons.push("checkpoint P7 run identity differs from the expected run".into());
        }
        let live_owned_process = run.state == ExecutionRunState::Running
            && !record.content.processes.is_empty()
            && record.content.processes.len() == process_observations.len()
            && record
                .content
                .processes
                .iter()
                .zip(&process_observations)
                .all(|(process, observation)| {
                    *observation == ProcessObservation::OwnedProcessStillRunning
                        && process.binds_to_running_attempt(&run)
                });
        let disposition = if record.content.authority.identity_tuple() != expected.identity_tuple()
            || current_workspace.fingerprint != record.content.workspace.fingerprint
            || git_worktree_changed(&record.content.git, &current_git)
        {
            RecoveryDisposition::RevalidationRequired
        } else if live_owned_process {
            RecoveryDisposition::SafeToResumeAfterProcessReconciliation
        } else if !record.is_safe_to_resume() {
            reasons.push(
                "checkpoint records durable state but not a safe automatic-resume boundary".into(),
            );
            RecoveryDisposition::RevalidationRequired
        } else if process_observations.iter().any(|observation| {
            matches!(
                observation,
                ProcessObservation::PidReusedNotOurs | ProcessObservation::ProcessStateUnknown
            )
        }) {
            RecoveryDisposition::RevalidationRequired
        } else if run.state == ExecutionRunState::StoppedIncomplete {
            RecoveryDisposition::StoppedIncomplete
        } else if self
            .store
            .load_session()?
            .is_some_and(|session| session.state == SessionEndState::Active)
        {
            RecoveryDisposition::SafeToResumeAfterProcessReconciliation
        } else {
            RecoveryDisposition::SafeToResume
        };
        Ok(ResumeIntegrityResult {
            disposition,
            classification: if matches!(
                disposition,
                RecoveryDisposition::SafeToResume
                    | RecoveryDisposition::SafeToResumeAfterProcessReconciliation
                    | RecoveryDisposition::StoppedIncomplete
            ) {
                RecoveryClassification::InterruptedAtSafeCheckpoint
            } else if !changed_paths.is_empty() {
                RecoveryClassification::ExternalStateUncertain
            } else {
                RecoveryClassification::InterruptedWithoutSafeCheckpoint
            },
            reasons,
            changed_paths,
            checkpoint_id: Some(record.checkpoint_id),
            checkpoint_sequence: Some(record.sequence),
            process_observations,
            target: run.current_recovery_attempt(),
        })
    }

    pub fn resume_integrity_for_run(
        &self,
        run: &ExecutionRun,
        expected: &RecoveryAuthority,
        root: &Path,
        inspector: &dyn ProcessInspector,
        now_ms: u64,
    ) -> Result<ResumeIntegrityResult, RecoveryError> {
        if run.run_id != expected.p7_run_id
            || run.mission_id != expected.mission_id
            || run.mission_revision != expected.mission_revision
        {
            return Ok(ResumeIntegrityResult {
                disposition: RecoveryDisposition::RevalidationRequired,
                classification: RecoveryClassification::RecoveryNotAllowed,
                reasons: vec!["execution and recovery authority identities differ".into()],
                changed_paths: Vec::new(),
                checkpoint_id: None,
                checkpoint_sequence: None,
                process_observations: Vec::new(),
                target: run.current_recovery_attempt(),
            });
        }
        let current_target = run.current_recovery_attempt();
        if run.current_recovery_attempt_is_pre_execution() {
            if self.store.load_latest()?.is_none() {
                return Ok(Self::pre_execution_retry_result(run, None));
            }
            let mut checkpoint_result = self.resume_integrity(expected, root, inspector, now_ms)?;
            if checkpoint_result.target.is_none() {
                checkpoint_result.target = current_target.clone();
            }
            let checkpoint_is_unchanged = checkpoint_result.changed_paths.is_empty()
                && checkpoint_result.process_observations.iter().all(|observation| {
                    !matches!(
                        observation,
                        ProcessObservation::PidReusedNotOurs | ProcessObservation::ProcessStateUnknown
                    )
                })
                && checkpoint_result.reasons.iter().all(|reason| {
                    reason == "checkpoint records durable state but not a safe automatic-resume boundary"
                });
            if checkpoint_is_unchanged {
                return Ok(Self::pre_execution_retry_result(
                    run,
                    Some(checkpoint_result),
                ));
            }
            return Ok(checkpoint_result);
        }
        let mut result = self.resume_integrity(expected, root, inspector, now_ms)?;
        result.target = current_target;
        if result.target.as_ref().is_some_and(|target| {
            target.execution_boundary == AttemptExecutionBoundary::ExternalProcessStarted
        }) {
            if let Some(target) = result.target.as_ref() {
                if let Some(attempt) = run
                    .attempts
                    .iter()
                    .find(|attempt| attempt.attempt_id == target.attempt_id)
                {
                    if let Some(before) = attempt.workspace_before.as_ref() {
                        let current = WorkspaceSnapshot::capture(root, now_ms)?;
                        let after = current
                            .files
                            .iter()
                            .map(|file| (file.relative_path.clone(), file.digest.clone()))
                            .collect::<BTreeMap<_, _>>();
                        let attempt_changes = before
                            .keys()
                            .chain(after.keys())
                            .cloned()
                            .collect::<BTreeSet<_>>()
                            .into_iter()
                            .filter(|path| before.get(path) != after.get(path))
                            .collect::<Vec<_>>();
                        result.changed_paths = result
                            .changed_paths
                            .into_iter()
                            .chain(attempt_changes)
                            .collect::<BTreeSet<_>>()
                            .into_iter()
                            .collect();
                    }
                }
            }
            result.disposition = RecoveryDisposition::RevalidationRequired;
            result.classification = RecoveryClassification::ExternalStateUncertain;
            let reason = "an external process started for the selected attempt; Relintor cannot prove that it left no workspace or external side effects";
            if !result.reasons.iter().any(|existing| existing == reason) {
                result.reasons.push(reason.into());
            }
        }
        Ok(result)
    }

    fn pre_execution_retry_result(
        run: &ExecutionRun,
        checkpoint: Option<ResumeIntegrityResult>,
    ) -> ResumeIntegrityResult {
        ResumeIntegrityResult {
            disposition: RecoveryDisposition::PreExecutionRetryAuthorized,
            classification: RecoveryClassification::PreExecutionPrevented,
            reasons: vec![
                "the failed attempt stopped before external process execution; no tool calls, steps, or workspace side effects are recorded".into(),
            ],
            changed_paths: Vec::new(),
            checkpoint_id: checkpoint.as_ref().and_then(|result| result.checkpoint_id.clone()),
            checkpoint_sequence: checkpoint.as_ref().and_then(|result| result.checkpoint_sequence),
            process_observations: checkpoint
                .map_or_else(Vec::new, |result| result.process_observations),
            target: run.current_recovery_attempt(),
        }
    }

    pub fn begin_revalidation(
        &self,
        expected: &RecoveryAuthority,
        result: &ResumeIntegrityResult,
        now_ms: u64,
    ) -> Result<RevalidationRecord, RecoveryError> {
        if let Some(existing) = self.store.load_latest_revalidation()? {
            if existing.project_id == expected.project_id
                && existing.mission_id == expected.mission_id
                && existing.mission_revision == expected.mission_revision
                && existing.p7_run_id == expected.p7_run_id
                && existing.classification == result.classification
                && existing.disposition == result.disposition
                && existing.reasons == result.reasons
                && existing.affected_paths == result.changed_paths
                && existing.target == result.target
            {
                return Ok(existing);
            }
        }
        let record = RevalidationRecord {
            record_version: RECOVERY_VERSION.into(),
            revalidation_id: sha256_json(&(
                expected.p7_run_id.as_str(),
                result.checkpoint_id.as_deref(),
                result.target.as_ref(),
                result.classification,
                result.disposition,
                &result.reasons,
                &result.changed_paths,
            )),
            project_id: expected.project_id.clone(),
            mission_id: expected.mission_id.clone(),
            mission_revision: expected.mission_revision,
            p7_run_id: expected.p7_run_id.clone(),
            checkpoint_id: result.checkpoint_id.clone(),
            target: result.target.clone(),
            classification: result.classification,
            disposition: result.disposition,
            decision: if result.disposition == RecoveryDisposition::PreExecutionRetryAuthorized {
                "RETRY_AUTHORIZED".into()
            } else {
                "REVALIDATION_REQUIRED".into()
            },
            reasons: result.reasons.clone(),
            affected_paths: result.changed_paths.clone(),
            required_authorities: vec![
                "P8 evidence invalidation/revalidation".into(),
                "P7 execution admission".into(),
            ],
            created_at_ms: now_ms,
            integrity_tag: String::new(),
        };
        self.store.write_revalidation(record)
    }

    /// Persist an explicit manual retry decision for an interrupted external
    /// attempt. The decision is authenticated and bound to the exact P7
    /// identities, attempt, checkpoint, and reviewed affected paths.
    pub fn authorize_manual_retry(
        &self,
        expected: &RecoveryAuthority,
        result: &ResumeIntegrityResult,
        now_ms: u64,
    ) -> Result<RevalidationRecord, RecoveryError> {
        let Some(target) = result.target.as_ref() else {
            return Err(RecoveryError::Authority(
                "manual retry has no exact interrupted attempt target".into(),
            ));
        };
        if result.disposition != RecoveryDisposition::RevalidationRequired
            || target.execution_boundary != AttemptExecutionBoundary::ExternalProcessStarted
            || result.process_observations.is_empty()
            || result
                .process_observations
                .iter()
                .any(|observation| *observation != ProcessObservation::ProcessGone)
        {
            return Err(RecoveryError::Authority(
                "manual retry requires a revalidated external attempt with every owned process gone"
                    .into(),
            ));
        }
        let record = RevalidationRecord {
            record_version: RECOVERY_VERSION.into(),
            revalidation_id: sha256_json(&(
                expected.p7_run_id.as_str(),
                result.checkpoint_id.as_deref(),
                target,
                result.classification,
                result.disposition,
                &result.reasons,
                &result.changed_paths,
                "MANUAL_RETRY_AUTHORIZED",
            )),
            project_id: expected.project_id.clone(),
            mission_id: expected.mission_id.clone(),
            mission_revision: expected.mission_revision,
            p7_run_id: expected.p7_run_id.clone(),
            checkpoint_id: result.checkpoint_id.clone(),
            target: result.target.clone(),
            classification: result.classification,
            disposition: result.disposition,
            decision: "MANUAL_RETRY_AUTHORIZED".into(),
            reasons: result.reasons.clone(),
            affected_paths: result.changed_paths.clone(),
            required_authorities: vec![
                "P8 evidence invalidation/revalidation".into(),
                "P7 execution admission".into(),
                "explicit user recovery review".into(),
            ],
            created_at_ms: now_ms,
            integrity_tag: String::new(),
        };
        self.store.write_revalidation(record)
    }
}

impl WorkspaceSnapshot {
    pub fn capture(root: &Path, now_ms: u64) -> Result<Self, RecoveryError> {
        let root = canonical_root(root)?;
        let mut files = Vec::new();
        let mut total_bytes = 0_u64;
        collect_files(&root, &root, &mut files, &mut total_bytes)?;
        files.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
        let fingerprint = sha256_json(&files);
        Ok(Self {
            root: root.display().to_string(),
            fingerprint,
            file_count: files.len(),
            total_bytes,
            files,
            captured_at_ms: now_ms,
        })
    }
}

impl UntrackedFileSnapshot {
    pub fn capture(root: &Path, now_ms: u64) -> Result<Self, RecoveryError> {
        let root = canonical_root(root)?;
        let workspace = WorkspaceSnapshot::capture(&root, now_ms)?;
        let mut files = Vec::new();
        let mut omitted_paths = Vec::new();
        let mut total_bytes = 0_u64;
        for item in workspace.files {
            let relative = item.relative_path.clone();
            if is_sensitive_path(&relative) {
                omitted_paths.push(relative);
                continue;
            }
            let path = root.join(&relative);
            let metadata = fs::symlink_metadata(&path).map_err(io_error)?;
            if is_link_or_reparse(&metadata) {
                omitted_paths.push(relative);
                continue;
            }
            let path = safe_join(&root, &relative)?;
            let content = if item.size <= MAX_UNTRACKED_FILE_BYTES
                && total_bytes.saturating_add(item.size) <= MAX_SNAPSHOT_BYTES
            {
                let bytes = fs::read(&path).map_err(io_error)?;
                if sha256_bytes(&bytes) != item.digest || contains_secret_material(&bytes) {
                    omitted_paths.push(relative.clone());
                    None
                } else {
                    total_bytes = total_bytes.saturating_add(item.size);
                    Some(bytes)
                }
            } else {
                omitted_paths.push(relative.clone());
                None
            };
            files.push(UntrackedFileRecord {
                relative_path: relative,
                digest: item.digest,
                size: item.size,
                content,
            });
            if files.len() > MAX_SNAPSHOT_FILES {
                return Err(RecoveryError::Storage(
                    "untracked snapshot file-count limit exceeded".into(),
                ));
            }
        }
        Ok(Self {
            root: root.display().to_string(),
            files,
            omitted_paths,
            total_bytes,
            captured_at_ms: now_ms,
        })
    }
}

impl GitWorktreeSnapshot {
    pub fn capture(root: &Path) -> Result<Self, RecoveryError> {
        let root = canonical_root(root)?;
        let root_text = root.display().to_string();
        let probe = hidden_command("git")
            .args(["-C", &root_text, "rev-parse", "--show-toplevel"])
            .output();
        let Ok(probe) = probe else {
            return Ok(Self {
                available: false,
                root: root_text,
                head: None,
                branch: None,
                dirty: false,
                changed_paths: Vec::new(),
                diff_digest: None,
                detail: "Git is not installed; non-Git workspace snapshot remains authoritative"
                    .into(),
            });
        };
        if !probe.status.success() {
            return Ok(Self {
                available: false,
                root: root_text,
                head: None,
                branch: None,
                dirty: false,
                changed_paths: Vec::new(),
                diff_digest: None,
                detail: "workspace is not a Git worktree".into(),
            });
        }
        let head = git_output(&root, &["rev-parse", "HEAD"])?;
        let branch = git_output(&root, &["symbolic-ref", "--short", "-q", "HEAD"])?;
        let status = git_output(&root, &["status", "--porcelain=v1", "--untracked-files=no"])?
            .unwrap_or_default();
        let diff =
            git_output(&root, &["diff", "--name-only", "--no-ext-diff"])?.unwrap_or_default();
        let cached = git_output(&root, &["diff", "--cached", "--name-only", "--no-ext-diff"])?
            .unwrap_or_default();
        let paths = parse_git_changed_paths(&status, &diff, &cached);
        let diff_material = format!("{status}\n{diff}\n{cached}");
        Ok(Self {
            available: true,
            root: root_text,
            head,
            branch,
            dirty: !paths.is_empty(),
            changed_paths: paths.into_iter().collect(),
            diff_digest: Some(sha256_bytes(diff_material.as_bytes())),
            detail: "read-only Git HEAD/branch/status snapshot captured".into(),
        })
    }
}

fn git_output(root: &Path, args: &[&str]) -> Result<Option<String>, RecoveryError> {
    let root_text = root.display().to_string();
    let output = hidden_command("git")
        .arg("-C")
        .arg(&root_text)
        .args(args)
        .output()
        .map_err(io_error)?;
    if !output.status.success() {
        return Ok(None);
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if value.len() > 128 * 1024 {
        return Err(RecoveryError::Storage(
            "Git snapshot output exceeded bound".into(),
        ));
    }
    Ok(Some(value))
}

fn parse_git_changed_paths(status: &str, diff: &str, cached: &str) -> BTreeSet<String> {
    let mut paths = BTreeSet::new();
    for line in status.lines() {
        let line = line.trim_end();
        let value = line.get(3..).unwrap_or(line).trim();
        insert_git_path_and_rename(&mut paths, value);
    }
    for line in diff.lines().chain(cached.lines()) {
        insert_git_path_and_rename(&mut paths, line.trim());
    }
    paths
}

fn insert_git_path_and_rename(paths: &mut BTreeSet<String>, value: &str) {
    if value.is_empty() {
        return;
    }
    for path in value.split(" -> ") {
        let path = path.trim().trim_matches('"').replace('\\', "/");
        if !path.is_empty() {
            paths.insert(path);
        }
    }
}

fn diff_workspace(before: &WorkspaceSnapshot, after: &WorkspaceSnapshot) -> Vec<String> {
    let old = before
        .files
        .iter()
        .map(|file| (file.relative_path.as_str(), file.digest.as_str()))
        .collect::<BTreeMap<_, _>>();
    let new = after
        .files
        .iter()
        .map(|file| (file.relative_path.as_str(), file.digest.as_str()))
        .collect::<BTreeMap<_, _>>();
    old.keys()
        .chain(new.keys())
        .copied()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter(|path| old.get(path) != new.get(path))
        .map(str::to_owned)
        .collect()
}

fn git_worktree_changed(before: &GitWorktreeSnapshot, after: &GitWorktreeSnapshot) -> bool {
    before.available != after.available
        || before.head != after.head
        || before.branch != after.branch
        || before.dirty != after.dirty
        || before.changed_paths != after.changed_paths
        || before.diff_digest != after.diff_digest
}

fn diff_git_worktree(before: &GitWorktreeSnapshot, after: &GitWorktreeSnapshot) -> Vec<String> {
    if !git_worktree_changed(before, after) {
        return Vec::new();
    }
    let mut paths = before
        .changed_paths
        .iter()
        .chain(after.changed_paths.iter())
        .cloned()
        .collect::<BTreeSet<_>>();
    if before.head != after.head || before.branch != after.branch {
        paths.insert(".git/HEAD".into());
    }
    if before.diff_digest != after.diff_digest {
        paths.insert(".git/index".into());
    }
    paths.into_iter().collect()
}

fn collect_files(
    root: &Path,
    current: &Path,
    files: &mut Vec<WorkspaceFileRecord>,
    total: &mut u64,
) -> Result<(), RecoveryError> {
    let entries = fs::read_dir(current).map_err(io_error)?;
    for entry in entries {
        let entry = entry.map_err(io_error)?;
        let path = entry.path();
        let relative = path
            .strip_prefix(root)
            .map_err(|_| RecoveryError::Path("workspace relative path failed".into()))?;
        let relative_text = relative.to_string_lossy().replace('\\', "/");
        if should_exclude(&relative_text) {
            continue;
        }
        let metadata = fs::symlink_metadata(&path).map_err(io_error)?;
        if is_link_or_reparse(&metadata) {
            let target = fs::read_link(&path)
                .map(|target| target.to_string_lossy().into_owned())
                .unwrap_or_else(|_| "REPARSE_POINT".into());
            let digest = sha256_bytes(format!("P9_LINK\0{relative_text}\0{target}").as_bytes());
            files.push(WorkspaceFileRecord {
                relative_path: relative_text,
                digest,
                size: 0,
            });
            if files.len() > MAX_SNAPSHOT_FILES {
                return Err(RecoveryError::Storage(
                    "workspace snapshot file-count limit exceeded".into(),
                ));
            }
            continue;
        }
        if metadata.is_dir() {
            collect_files(root, &path, files, total)?;
        } else if metadata.is_file() {
            let (digest, size) = hash_file(&path)?;
            *total = total.saturating_add(size);
            files.push(WorkspaceFileRecord {
                relative_path: relative_text,
                digest,
                size,
            });
            if files.len() > MAX_SNAPSHOT_FILES || *total > MAX_SNAPSHOT_BYTES.saturating_mul(8) {
                return Err(RecoveryError::Storage(
                    "workspace snapshot quota exceeded".into(),
                ));
            }
        }
    }
    Ok(())
}

fn should_exclude(relative: &str) -> bool {
    relative.split('/').any(|part| {
        let lower = part.to_ascii_lowercase();
        matches!(
            lower.as_str(),
            ".git" | "node_modules" | "target" | ".relintor-recovery"
        )
    })
}

fn is_sensitive_path(relative: &str) -> bool {
    let lower = relative.to_ascii_lowercase();
    lower.split('/').any(|part| {
        part == ".env"
            || part.starts_with(".env.")
            || part.contains("credential")
            || part.contains("secret")
            || part.contains("browser")
            || part.contains("chrome")
            || part.contains("chromium")
            || part.contains("firefox")
            || part.contains("edge")
            || part.contains("cookies")
            || part.contains("keychain")
            || part == ".npmrc"
            || part == ".netrc"
            || part == ".pypirc"
            || part == ".ssh"
            || part == ".aws"
    }) || lower.ends_with(".pem")
        || lower.ends_with(".key")
        || lower.ends_with("id_rsa")
        || lower.ends_with("id_ed25519")
}

fn contains_secret_material(bytes: &[u8]) -> bool {
    let lower = String::from_utf8_lossy(bytes).to_ascii_lowercase();
    [
        "api_key=",
        "\"api_key\"",
        "apikey=",
        "access_token=",
        "refresh_token=",
        "password=",
        "\"password\"",
        "client_secret=",
        "private_key",
        "\"private_key\"",
        "authorization: bearer ",
        "authorization:bearer ",
        "aws_access_key_id=",
        "aws_secret_access_key=",
        "-----begin",
        "-----begin ",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
}

fn is_link_or_reparse(metadata: &fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & REPARSE_POINT != 0
    }
    #[cfg(not(windows))]
    {
        false
    }
}

fn valid_local_database_path(path: &Path, destination: bool) -> bool {
    if !path.is_absolute() {
        return false;
    }
    if path.exists() && (destination || !path.is_file()) {
        return false;
    }
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return destination && !path.exists();
    };
    if is_link_or_reparse(&metadata) {
        return false;
    }
    let Some(parent) = path.parent() else {
        return false;
    };
    fs::canonicalize(parent).is_ok_and(|parent| !is_link_or_reparse_parent(&parent))
}

fn is_link_or_reparse_parent(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .map(|metadata| is_link_or_reparse(&metadata))
        .unwrap_or(true)
}

fn canonical_root(root: &Path) -> Result<PathBuf, RecoveryError> {
    if !root.is_absolute() {
        return Err(RecoveryError::Path(
            "workspace root must be absolute".into(),
        ));
    }
    fs::canonicalize(root).map_err(io_error)
}

fn safe_join(root: &Path, relative: &str) -> Result<PathBuf, RecoveryError> {
    let root = canonical_root(root)?;
    let relative_path = Path::new(relative);
    if relative_path.is_absolute()
        || relative_path
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(RecoveryError::Path(
            "relative recovery path escapes workspace".into(),
        ));
    }
    let candidate = root.join(relative_path);
    if let Some(parent) = candidate.parent() {
        let parent = fs::canonicalize(parent).map_err(io_error)?;
        if !parent.starts_with(&root) {
            return Err(RecoveryError::Path(
                "recovery path escapes workspace".into(),
            ));
        }
    }
    if candidate.exists() {
        let metadata = fs::symlink_metadata(&candidate).map_err(io_error)?;
        if is_link_or_reparse(&metadata) {
            return Err(RecoveryError::Path(
                "symlink/reparse recovery target is refused".into(),
            ));
        }
        if !fs::canonicalize(&candidate)
            .map_err(io_error)?
            .starts_with(&root)
        {
            return Err(RecoveryError::Path(
                "recovery target escapes workspace".into(),
            ));
        }
    }
    Ok(candidate)
}

fn hash_file(path: &Path) -> Result<(String, u64), RecoveryError> {
    let mut file = File::open(path).map_err(io_error)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    let mut size = 0_u64;
    loop {
        let read = file.read(&mut buffer).map_err(io_error)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        size = size.saturating_add(read as u64);
    }
    Ok((format!("{:x}", hasher.finalize()), size))
}

fn sha256_bytes(value: &[u8]) -> String {
    format!("{:x}", Sha256::digest(value))
}

fn sha256_json<T: Serialize>(value: &T) -> String {
    serde_json::to_vec(value).map_or_else(|_| String::new(), |bytes| sha256_bytes(&bytes))
}

fn hmac_json<T: Serialize>(key: &[u8], value: &T) -> Result<String, RecoveryError> {
    if key.len() != 32 {
        return Err(RecoveryError::Storage("recovery key length invalid".into()));
    }
    let bytes =
        serde_json::to_vec(value).map_err(|error| RecoveryError::Corrupt(error.to_string()))?;
    let mut ipad = [0x36_u8; 64];
    let mut opad = [0x5c_u8; 64];
    for (index, value) in key.iter().enumerate() {
        ipad[index] ^= value;
        opad[index] ^= value;
    }
    let mut inner = Sha256::new();
    inner.update(ipad);
    inner.update(&bytes);
    let mut outer = Sha256::new();
    outer.update(opad);
    outer.update(inner.finalize());
    Ok(format!("{:x}", outer.finalize()))
}

fn crash_without_tag(record: &CrashRecord) -> impl Serialize + '_ {
    (
        &record.record_version,
        &record.session_id,
        &record.run_id,
        record.detected_at_ms,
        record.prior_started_at_ms,
        &record.detail,
    )
}

fn compensation_journal_without_tag(journal: &CompensationJournal) -> impl Serialize + '_ {
    (
        &journal.recovery_version,
        &journal.entries,
        &journal.attempts,
    )
}

fn record_without_tag(record: &CheckpointRecord) -> impl Serialize + '_ {
    (
        &record.recovery_version,
        &record.checkpoint_id,
        record.sequence,
        &record.parent_digest,
        &record.content_digest,
        &record.content,
    )
}

fn index_without_tag(index: &CheckpointIndex) -> impl Serialize + '_ {
    (
        &index.recovery_version,
        &index.mission_id,
        index.mission_revision,
        &index.run_id,
        index.latest_sequence,
        &index.latest_checkpoint_id,
        &index.latest_digest,
    )
}

fn session_without_tag(marker: &SessionMarker) -> impl Serialize + '_ {
    (
        &marker.record_version,
        &marker.session_id,
        &marker.run_id,
        marker.process_id,
        marker.process_start_time_ms,
        &marker.executable_digest,
        &marker.command_digest,
        marker.started_at_ms,
        marker.ended_at_ms,
        marker.state,
    )
}

fn legacy_session_without_tag(marker: &SessionMarker) -> impl Serialize + '_ {
    (
        &marker.session_id,
        &marker.run_id,
        marker.process_id,
        marker.process_start_time_ms,
        &marker.executable_digest,
        &marker.command_digest,
        marker.started_at_ms,
        marker.ended_at_ms,
        marker.state,
    )
}

fn revalidation_without_tag(record: &RevalidationRecord) -> impl Serialize + '_ {
    (
        &record.record_version,
        &record.revalidation_id,
        &record.project_id,
        &record.mission_id,
        record.mission_revision,
        &record.p7_run_id,
        &record.checkpoint_id,
        &record.target,
        record.classification,
        record.disposition,
        &record.decision,
        &record.reasons,
        &record.affected_paths,
        &record.required_authorities,
        record.created_at_ms,
    )
}

fn revalidation_without_target_tag(record: &RevalidationRecord) -> impl Serialize + '_ {
    (
        &record.record_version,
        &record.revalidation_id,
        &record.project_id,
        &record.mission_id,
        record.mission_revision,
        &record.p7_run_id,
        &record.checkpoint_id,
        record.classification,
        record.disposition,
        &record.decision,
        &record.reasons,
        &record.affected_paths,
        &record.required_authorities,
        record.created_at_ms,
    )
}

fn legacy_revalidation_without_tag(record: &RevalidationRecord) -> impl Serialize + '_ {
    (
        &record.revalidation_id,
        &record.project_id,
        &record.mission_id,
        record.mission_revision,
        &record.p7_run_id,
        &record.checkpoint_id,
        &record.reasons,
        &record.affected_paths,
        &record.required_authorities,
        record.created_at_ms,
    )
}

fn legacy_revalidation_with_decision_without_tag(
    record: &RevalidationRecord,
) -> impl Serialize + '_ {
    (
        &record.revalidation_id,
        &record.project_id,
        &record.mission_id,
        record.mission_revision,
        &record.p7_run_id,
        &record.checkpoint_id,
        record.classification,
        record.disposition,
        &record.decision,
        &record.reasons,
        &record.affected_paths,
        &record.required_authorities,
        record.created_at_ms,
    )
}

fn atomic_write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), RecoveryError> {
    let bytes = serde_json::to_vec_pretty(value)
        .map_err(|error| RecoveryError::Corrupt(error.to_string()))?;
    atomic_write(path, &bytes)
}

fn is_low_storage_error(error: &RecoveryError) -> bool {
    let detail = error.to_string().to_ascii_lowercase();
    detail.contains("os error 112")
        || detail.contains("not enough space")
        || detail.contains("disk full")
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), RecoveryError> {
    if bytes.len() > MAX_CHECKPOINT_BYTES {
        return Err(RecoveryError::Storage(
            "recovery artifact exceeds size limit".into(),
        ));
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(io_error)?;
    }
    let temp = path.with_file_name(format!(
        ".{}.tmp",
        path.file_name().unwrap_or_default().to_string_lossy()
    ));
    let _ = fs::remove_file(&temp);
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temp)
        .map_err(io_error)?;
    file.write_all(bytes).map_err(io_error)?;
    file.sync_all().map_err(io_error)?;
    drop(file);
    atomic_replace(&temp, path)?;
    #[cfg(not(windows))]
    if let Some(parent) = path.parent() {
        if let Ok(directory) = File::open(parent) {
            let _ = directory.sync_all();
        }
    }
    Ok(())
}

#[cfg(windows)]
fn atomic_replace(temp: &Path, destination: &Path) -> Result<(), RecoveryError> {
    use std::os::windows::ffi::OsStrExt;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn MoveFileExW(existing: *const u16, replacement: *const u16, flags: u32) -> i32;
    }

    const MOVEFILE_REPLACE_EXISTING: u32 = 0x0000_0001;
    const MOVEFILE_WRITE_THROUGH: u32 = 0x0000_0008;
    let existing = temp
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let replacement = destination
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let success = unsafe {
        MoveFileExW(
            existing.as_ptr(),
            replacement.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    } != 0;
    if success {
        Ok(())
    } else {
        Err(io_error(std::io::Error::last_os_error()))
    }
}

#[cfg(not(windows))]
fn atomic_replace(temp: &Path, destination: &Path) -> Result<(), RecoveryError> {
    fs::rename(temp, destination).map_err(io_error)
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, RecoveryError> {
    let metadata = fs::metadata(path).map_err(io_error)?;
    if metadata.len() > MAX_CHECKPOINT_BYTES as u64 {
        return Err(RecoveryError::Corrupt(
            "recovery artifact exceeds size limit".into(),
        ));
    }
    let bytes = fs::read(path).map_err(io_error)?;
    serde_json::from_slice(&bytes).map_err(|error| RecoveryError::Corrupt(error.to_string()))
}

fn io_error(error: std::io::Error) -> RecoveryError {
    RecoveryError::Storage(error.to_string())
}

#[derive(Debug)]
pub enum RecoveryError {
    Authority(String),
    Chain(String),
    Corrupt(String),
    Storage(String),
    Path(String),
    Process(String),
    Compensation(String),
    Execution(ExecutionError),
}

impl fmt::Display for RecoveryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Authority(value) => write!(formatter, "recovery authority: {value}"),
            Self::Chain(value) => write!(formatter, "recovery checkpoint chain: {value}"),
            Self::Corrupt(value) => write!(formatter, "recovery record corrupt: {value}"),
            Self::Storage(value) => write!(formatter, "recovery storage: {value}"),
            Self::Path(value) => write!(formatter, "recovery path: {value}"),
            Self::Process(value) => write!(formatter, "recovery process ownership: {value}"),
            Self::Compensation(value) => write!(formatter, "recovery compensation: {value}"),
            Self::Execution(value) => write!(formatter, "recovery execution state: {value}"),
        }
    }
}

impl std::error::Error for RecoveryError {}

impl WorkspaceSnapshot {
    pub fn current_fingerprint(root: &Path) -> Result<String, RecoveryError> {
        Ok(Self::capture(root, now_ms())?.fingerprint)
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis() as u64)
}

#[cfg(windows)]
fn current_process_start_time_ms() -> Option<u64> {
    #[repr(C)]
    struct FileTime {
        low: u32,
        high: u32,
    }

    extern "system" {
        fn GetCurrentProcess() -> *mut std::ffi::c_void;
        fn GetProcessTimes(
            process: *mut std::ffi::c_void,
            creation: *mut FileTime,
            exit: *mut FileTime,
            kernel: *mut FileTime,
            user: *mut FileTime,
        ) -> i32;
    }

    let mut creation = FileTime { low: 0, high: 0 };
    let mut exit = FileTime { low: 0, high: 0 };
    let mut kernel = FileTime { low: 0, high: 0 };
    let mut user = FileTime { low: 0, high: 0 };
    let success = unsafe {
        GetProcessTimes(
            GetCurrentProcess(),
            &mut creation,
            &mut exit,
            &mut kernel,
            &mut user,
        )
    } != 0;
    success.then(|| (((creation.high as u64) << 32) | u64::from(creation.low)) / 10_000)
}

#[cfg(unix)]
fn current_process_start_time_ms() -> Option<u64> {
    let stat = fs::read_to_string("/proc/self/stat").ok()?;
    let fields = stat
        .rsplit_once(") ")?
        .1
        .split_whitespace()
        .collect::<Vec<_>>();
    fields.get(19)?.parse().ok()
}

#[cfg(not(any(windows, unix)))]
fn current_process_start_time_ms() -> Option<u64> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_revalidation_without_new_recovery_fields_loads_with_safe_defaults() {
        let root = std::env::temp_dir().join(format!(
            "relintor-legacy-revalidation-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        let key = vec![7_u8; 32];
        let store = RecoveryStore::new(root.clone(), key.clone()).expect("create store");
        let record = RevalidationRecord {
            record_version: String::new(),
            revalidation_id: "legacy-revalidation".into(),
            project_id: "legacy-project".into(),
            mission_id: "mission-legacy-project".into(),
            mission_revision: 1,
            p7_run_id: "legacy-run".into(),
            checkpoint_id: Some("legacy-checkpoint".into()),
            target: None,
            classification: RecoveryClassification::ExternalStateUncertain,
            disposition: RecoveryDisposition::RevalidationRequired,
            decision: String::new(),
            reasons: vec!["legacy recovery record".into()],
            affected_paths: Vec::new(),
            required_authorities: vec!["P7 execution admission".into()],
            created_at_ms: 1,
            integrity_tag: String::new(),
        };
        let tag = hmac_json(&key, &legacy_revalidation_without_tag(&record)).expect("legacy tag");
        let legacy = serde_json::json!({
            "revalidation_id": record.revalidation_id,
            "project_id": record.project_id,
            "mission_id": record.mission_id,
            "mission_revision": record.mission_revision,
            "p7_run_id": record.p7_run_id,
            "checkpoint_id": record.checkpoint_id,
            "reasons": record.reasons,
            "affected_paths": record.affected_paths,
            "required_authorities": record.required_authorities,
            "created_at_ms": record.created_at_ms,
            "integrity_tag": tag,
        });
        let parsed: RevalidationRecord =
            serde_json::from_value(legacy.clone()).expect("parse legacy record");
        assert_eq!(
            parsed.integrity_tag,
            hmac_json(&key, &legacy_revalidation_without_tag(&parsed))
                .expect("recompute legacy tag")
        );
        fs::write(
            root.join("revalidation-legacy-revalidation.json"),
            serde_json::to_vec_pretty(&legacy).expect("serialize legacy record"),
        )
        .expect("write legacy record");

        let loaded = store
            .load_latest_revalidation()
            .expect("load legacy record")
            .expect("legacy record exists");
        assert_eq!(
            loaded.classification,
            RecoveryClassification::ExternalStateUncertain
        );
        assert_eq!(
            loaded.disposition,
            RecoveryDisposition::RevalidationRequired
        );
        assert!(loaded.decision.is_empty());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn legacy_revalidation_with_decision_fields_loads_and_migrates() {
        let root = std::env::temp_dir().join(format!(
            "relintor-legacy-revalidation-decision-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        let key = vec![8_u8; 32];
        let store = RecoveryStore::new(root.clone(), key.clone()).expect("create store");
        let record = RevalidationRecord {
            record_version: String::new(),
            revalidation_id: "legacy-revalidation-decision".into(),
            project_id: "legacy-project".into(),
            mission_id: "mission-legacy-project".into(),
            mission_revision: 1,
            p7_run_id: "legacy-run".into(),
            checkpoint_id: Some("legacy-checkpoint".into()),
            target: None,
            classification: RecoveryClassification::PreExecutionPrevented,
            disposition: RecoveryDisposition::PreExecutionRetryAuthorized,
            decision: "RETRY_AUTHORIZED".into(),
            reasons: vec!["historical pre-execution prevention".into()],
            affected_paths: Vec::new(),
            required_authorities: vec!["P7 execution admission".into()],
            created_at_ms: 2,
            integrity_tag: String::new(),
        };
        let tag = hmac_json(
            &key,
            &legacy_revalidation_with_decision_without_tag(&record),
        )
        .expect("legacy decision tag");
        let legacy = serde_json::json!({
            "revalidation_id": record.revalidation_id,
            "project_id": record.project_id,
            "mission_id": record.mission_id,
            "mission_revision": record.mission_revision,
            "p7_run_id": record.p7_run_id,
            "checkpoint_id": record.checkpoint_id,
            "classification": record.classification,
            "disposition": record.disposition,
            "decision": record.decision,
            "reasons": record.reasons,
            "affected_paths": record.affected_paths,
            "required_authorities": record.required_authorities,
            "created_at_ms": record.created_at_ms,
            "integrity_tag": tag,
        });
        fs::write(
            root.join("revalidation-legacy-revalidation-decision.json"),
            serde_json::to_vec_pretty(&legacy).expect("serialize legacy decision record"),
        )
        .expect("write legacy decision record");

        let loaded = store
            .load_latest_revalidation()
            .expect("load legacy decision record")
            .expect("legacy decision record exists");
        assert_eq!(loaded.decision, "RETRY_AUTHORIZED");
        assert_eq!(
            loaded.disposition,
            RecoveryDisposition::PreExecutionRetryAuthorized
        );
        assert_eq!(loaded.record_version, RECOVERY_VERSION);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn tampered_legacy_revalidation_with_decision_fields_is_rejected() {
        let root = std::env::temp_dir().join(format!(
            "relintor-tampered-legacy-revalidation-decision-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        let key = vec![9_u8; 32];
        let store = RecoveryStore::new(root.clone(), key.clone()).expect("create store");
        let record = RevalidationRecord {
            record_version: String::new(),
            revalidation_id: "tampered-legacy-revalidation-decision".into(),
            project_id: "legacy-project".into(),
            mission_id: "mission-legacy-project".into(),
            mission_revision: 1,
            p7_run_id: "legacy-run".into(),
            checkpoint_id: None,
            target: None,
            classification: RecoveryClassification::PreExecutionPrevented,
            disposition: RecoveryDisposition::PreExecutionRetryAuthorized,
            decision: "RETRY_AUTHORIZED".into(),
            reasons: vec!["historical pre-execution prevention".into()],
            affected_paths: Vec::new(),
            required_authorities: Vec::new(),
            created_at_ms: 3,
            integrity_tag: String::new(),
        };
        let tag = hmac_json(
            &key,
            &legacy_revalidation_with_decision_without_tag(&record),
        )
        .expect("legacy decision tag");
        let mut legacy = serde_json::to_value(record).expect("serialize record");
        legacy["integrity_tag"] = serde_json::Value::String(tag);
        legacy["decision"] = serde_json::Value::String("FORGED".into());
        fs::write(
            root.join("revalidation-tampered-legacy-revalidation-decision.json"),
            serde_json::to_vec_pretty(&legacy).expect("serialize tampered record"),
        )
        .expect("write tampered record");

        let error = store
            .load_latest_revalidation()
            .expect_err("tampered legacy revalidation must fail closed");
        assert!(error
            .to_string()
            .contains("revalidation authority authentication failed"));

        let _ = fs::remove_dir_all(root);
    }
}
