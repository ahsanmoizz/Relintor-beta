//! P7 execution orchestration and watchdog authority.
//!
//! This crate consumes a validated P6 handoff. It never changes the P6 seal,
//! never grants completion authority, and never trusts renderer- or agent-
//! supplied leases. The deterministic scheduler is adapter-agnostic at its
//! policy boundary and dispatches real work through the P3 Antigravity adapter
//! contract when an adapter is supplied.

use relintor_antigravity::{
    AntigravityAdapter, AuthorizedAction, BridgeError, EventType, ProcessIdentity, ProcessResult,
    ProcessState, ProductionAdapter, StopCondition, TaskPacket as BridgeTaskPacket,
    TASK_PACKET_VERSION,
};
use relintor_core::load_or_create_keychain_authority_key;
use relintor_standards::{
    AuthorityEngine, EvidenceObligation, ExecutionHandoff, MissionRevision, RequirementPriority,
    RequirementStatus, StandardsRegistry, TrustedSignerSet,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};
use walkdir::WalkDir;

mod recovery;

pub use recovery::*;

pub const EXECUTION_STATUS: &str = "implemented_unverified";
pub const EXECUTION_PACKET_VERSION: &str = "p7-task-packet-v1";
pub const EXECUTION_LEDGER_VERSION: &str = "p7-execution-ledger-v1";
pub const EXECUTION_LEDGER_RECORD_VERSION: &str = "p7-ledger-record-v6";
/// Closed-Beta default authority for one Antigravity task. Individual tasks
/// receive a finite budget derived from sealed task complexity; this value is
/// the normal baseline, not an unconditional kill timer for every task.
pub const BETA_TASK_EXECUTION_BUDGET_MS: u64 = 25 * 60 * 1_000;
pub const BETA_MIN_TASK_EXECUTION_BUDGET_MS: u64 = 15 * 60 * 1_000;
pub const BETA_COMPLEX_TASK_EXECUTION_BUDGET_MS: u64 = 35 * 60 * 1_000;
pub const BETA_MAX_PROGRESS_RENEWALS_PER_TASK: u32 = 1;
pub const ADAPTER_PROCESS_SAFETY_TIMEOUT_MS: u64 = 45 * 60 * 1_000;
const LEGACY_EXECUTION_LEDGER_INTEGRITY_VERSION: &str = "p7-ledger-integrity-v1";
const EXECUTION_LEDGER_INTEGRITY_VERSION: &str = "p7-ledger-integrity-v2";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ExecutionRunState {
    Ready,
    Running,
    WaitingRetry,
    WaitingDependency,
    BlockedExternal,
    SafeBoundaryReached,
    TurnEndedIncomplete,
    Stopped,
    StoppedIncomplete,
    ExecutionTasksFinishedAwaitingVerification,
    Failed,
    RevalidationRequired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ExecutionTaskState {
    Pending,
    Ready,
    Running,
    WaitingRetry,
    WaitingDependency,
    BlockedExternal,
    FinishedAwaitingVerification,
    Stopped,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TaskAttemptState {
    Created,
    Running,
    Succeeded,
    Failed,
    Denied,
    WaitingRetry,
    TurnEndedIncomplete,
    SafeBoundaryStopped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AttemptExecutionBoundary {
    NotStarted,
    ExternalProcessStarted,
    #[default]
    Unknown,
}

fn is_unknown_execution_boundary(boundary: &AttemptExecutionBoundary) -> bool {
    *boundary == AttemptExecutionBoundary::Unknown
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FailureClass {
    Transient,
    Deterministic,
    PolicyDenied,
    AuthorityStale,
    ExternalUnavailable,
    Timeout,
    AgentExit,
    ProcessFailure,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecoveryAttemptTarget {
    pub task_id: String,
    pub attempt_id: String,
    pub lease_id: String,
    pub execution_boundary: AttemptExecutionBoundary,
    pub failure_class: Option<FailureClass>,
    pub termination_reason: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RetryDecision {
    Retry,
    Stop,
    RevalidationRequired,
    InjectDiagnostic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum LeaseStatus {
    Active,
    Consumed,
    Revoked,
    Expired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum WatchdogState {
    Healthy,
    Warning,
    RepeatedCommand,
    OscillationDetected,
    NoProgress,
    BudgetExhausted,
    DriftDetected,
    ExternalModification,
    SafeBoundary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum MeasurementQuality {
    Measured,
    Estimated,
    #[default]
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsageBudget {
    pub wall_clock_ms: u64,
    pub execution_steps: u64,
    pub tool_calls: u64,
    pub retry_attempts: u32,
    pub cost_micros: Option<u64>,
}

impl Default for UsageBudget {
    fn default() -> Self {
        Self {
            wall_clock_ms: BETA_TASK_EXECUTION_BUDGET_MS,
            execution_steps: 100,
            tool_calls: 50,
            retry_attempts: 2,
            cost_micros: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct UsageTelemetry {
    pub provider: Option<String>,
    pub model: Option<String>,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub tool_calls: u64,
    pub execution_steps: u64,
    pub wall_time_ms: u64,
    pub attempt_count: u32,
    pub retry_count: u32,
    pub estimated_cost_micros: Option<u64>,
    pub actual_cost_micros: Option<u64>,
    pub quality: MeasurementQuality,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetryPolicy {
    pub max_attempts: u32,
    pub retryable: BTreeSet<FailureClass>,
    pub backoff_ms: u64,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 2,
            retryable: [
                FailureClass::Transient,
                FailureClass::ExternalUnavailable,
                FailureClass::Timeout,
                FailureClass::ProcessFailure,
            ]
            .into_iter()
            .collect(),
            backoff_ms: 250,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LeaseScope {
    pub workspace: PathBuf,
    pub file_scopes: Vec<String>,
    pub directory_scopes: Vec<String>,
    pub shared_resources: Vec<String>,
    pub package_lockfiles: Vec<String>,
    pub generated_files: Vec<String>,
    pub allowed_tools: BTreeSet<String>,
    pub external_authority: BTreeSet<String>,
    pub scope_known: bool,
}

impl LeaseScope {
    pub fn conflicts(&self, other: &Self) -> bool {
        if self.workspace != other.workspace {
            return false;
        }
        if !self.scope_known || !other.scope_known {
            return true;
        }
        if self.external_authority != other.external_authority
            && (!self.external_authority.is_empty() || !other.external_authority.is_empty())
        {
            return true;
        }
        overlap(&self.file_scopes, &other.file_scopes)
            || overlap(&self.directory_scopes, &other.directory_scopes)
            || overlap(&self.shared_resources, &other.shared_resources)
            || overlap(&self.package_lockfiles, &other.package_lockfiles)
            || overlap(&self.generated_files, &other.generated_files)
    }

    fn allows_path(&self, path: &str) -> bool {
        let candidate = Path::new(path);
        if !candidate.is_absolute() {
            return false;
        }
        if !safe_path_within(candidate, &self.workspace) {
            return false;
        }
        let scopes = self
            .file_scopes
            .iter()
            .chain(self.directory_scopes.iter())
            .map(PathBuf::from)
            .collect::<Vec<_>>();
        scopes.is_empty()
            || scopes
                .iter()
                .any(|scope| safe_path_within(candidate, scope))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskPacket {
    pub contract_version: String,
    pub mission_id: String,
    pub mission_revision: u64,
    pub seal_hash: String,
    pub task_id: String,
    pub objective: String,
    pub requirement_ids: Vec<String>,
    pub dependency_ids: Vec<String>,
    pub project_id: String,
    pub workspace: PathBuf,
    pub allowed_file_scope: Vec<String>,
    pub allowed_tools: BTreeSet<String>,
    pub external_authority: BTreeSet<String>,
    pub time_budget_ms: u64,
    pub step_budget: u64,
    pub tool_call_budget: u64,
    pub usage_budget: UsageBudget,
    pub retry_policy: RetryPolicy,
    pub evidence_obligations: Vec<EvidenceObligation>,
    pub workspace_fingerprint: String,
    pub task_packet_digest: String,
    pub attempt_number: u32,
    pub lease_id: String,
    #[serde(default)]
    pub lease_expires_at_ms: u64,
    pub authorized_action: AuthorizedAction,
}

impl TaskPacket {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        mission_id: &str,
        mission_revision: u64,
        seal_hash: &str,
        task_id: &str,
        objective: &str,
        requirement_ids: Vec<String>,
        dependency_ids: Vec<String>,
        project_id: &str,
        workspace: PathBuf,
        allowed_tools: BTreeSet<String>,
        external_authority: BTreeSet<String>,
        budget: UsageBudget,
        retry_policy: RetryPolicy,
        evidence_obligations: Vec<EvidenceObligation>,
        workspace_fingerprint: &str,
        attempt_number: u32,
        lease_expires_at_ms: u64,
    ) -> Result<Self, ExecutionError> {
        let nonce = canonical_hash(&(
            mission_id,
            mission_revision,
            seal_hash,
            task_id,
            attempt_number,
            workspace.display().to_string(),
        ))?;
        let mut authorized_action = AuthorizedAction {
            sequence: 1,
            tool: "antigravity".into(),
            operation: "execute_task".into(),
            arguments: vec![task_id.into()],
            paths: vec![workspace.display().to_string()],
            working_scope: workspace.display().to_string(),
            expires_at_ms: lease_expires_at_ms,
            nonce,
            digest: String::new(),
        };
        authorized_action
            .refresh_digest()
            .map_err(|error| ExecutionError::Canonicalization(error.to_string()))?;
        let mut packet = Self {
            contract_version: EXECUTION_PACKET_VERSION.into(),
            mission_id: mission_id.into(),
            mission_revision,
            seal_hash: seal_hash.into(),
            task_id: task_id.into(),
            objective: objective.into(),
            requirement_ids,
            dependency_ids,
            project_id: project_id.into(),
            allowed_file_scope: vec![workspace.display().to_string()],
            allowed_tools,
            external_authority,
            time_budget_ms: budget.wall_clock_ms,
            step_budget: budget.execution_steps,
            tool_call_budget: budget.tool_calls,
            usage_budget: budget,
            retry_policy,
            evidence_obligations,
            workspace,
            workspace_fingerprint: workspace_fingerprint.into(),
            task_packet_digest: String::new(),
            attempt_number,
            lease_id: String::new(),
            lease_expires_at_ms,
            authorized_action,
        };
        packet.task_packet_digest = packet.compute_digest()?;
        Ok(packet)
    }

    fn digest_view(&self) -> Self {
        let mut view = self.clone();
        view.task_packet_digest.clear();
        view.lease_id.clear();
        view.lease_expires_at_ms = 0;
        view
    }

    pub fn compute_digest(&self) -> Result<String, ExecutionError> {
        canonical_hash(&self.digest_view())
    }

    pub fn verify_digest(&self) -> Result<(), ExecutionError> {
        if self.task_packet_digest != self.compute_digest()? {
            return Err(ExecutionError::StaleTaskPacket);
        }
        Ok(())
    }

    pub fn bind_lease(&mut self, lease_id: &str, expires_at_ms: u64) -> Result<(), ExecutionError> {
        self.verify_digest()?;
        self.lease_id = lease_id.into();
        self.lease_expires_at_ms = expires_at_ms;
        Ok(())
    }

    pub fn to_bridge_packet(&self) -> BridgeTaskPacket {
        let mut bridge = BridgeTaskPacket {
            contract_version: TASK_PACKET_VERSION.into(),
            mission_id: self.mission_id.clone(),
            mission_revision: self.mission_revision,
            seal_hash: self.seal_hash.clone(),
            task_id: self.task_id.clone(),
            project_id: self.project_id.clone(),
            workspace: self.workspace.clone(),
            workspace_fingerprint: self.workspace_fingerprint.clone(),
            task_packet_digest: String::new(),
            lease_id: self.lease_id.clone(),
            lease_expires_at_ms: self.lease_expires_at_ms,
            authorized_action: self.authorized_action.clone(),
            time_budget_ms: self.time_budget_ms,
            allowed_tools: self.allowed_tools.clone(),
            worktree_identity: worktree_identity(&self.workspace),
            subagent_identity: None,
            objective: self.objective.clone(),
            scope: self.allowed_file_scope.clone(),
            sealed_requirement_ids: self.requirement_ids.clone(),
            architecture_decisions: vec![format!("P6 seal {}", self.seal_hash)],
            professional_constraints: vec!["P7 implementation only; P8 verifies".into()],
            required_evidence: self
                .evidence_obligations
                .iter()
                .map(|obligation| format!("{:?}", obligation.class))
                .collect(),
            previous_failures: vec![],
            forbidden_changes: vec!["spec/locked".into(), "P6 mission seal".into()],
            stop_condition: StopCondition::AfterProcessExit,
        };
        bridge.task_packet_digest = bridge.binding_digest().unwrap_or_default();
        bridge
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionLease {
    pub lease_id: String,
    pub mission_id: String,
    pub mission_revision: u64,
    pub task_id: String,
    pub task_packet_digest: String,
    pub scope: LeaseScope,
    pub issued_at_ms: u64,
    pub expires_at_ms: u64,
    pub step_budget: u64,
    pub tool_call_budget: u64,
    pub usage_budget: UsageBudget,
    pub attempt_number: u32,
    pub status: LeaseStatus,
    pub lease_digest: String,
}

impl ExecutionLease {
    #[allow(clippy::too_many_arguments)]
    fn issue(
        mission_id: &str,
        mission_revision: u64,
        task_id: &str,
        task_packet_digest: &str,
        scope: LeaseScope,
        budget: UsageBudget,
        attempt_number: u32,
        issued_at_ms: u64,
    ) -> Result<Self, ExecutionError> {
        let expires_at_ms = issued_at_ms.saturating_add(budget.wall_clock_ms);
        let seed = (
            mission_id,
            mission_revision,
            task_id,
            task_packet_digest,
            attempt_number,
            issued_at_ms,
        );
        let lease_id = canonical_hash(&seed)?;
        let mut lease = Self {
            lease_id,
            mission_id: mission_id.into(),
            mission_revision,
            task_id: task_id.into(),
            task_packet_digest: task_packet_digest.into(),
            scope,
            issued_at_ms,
            expires_at_ms,
            step_budget: budget.execution_steps,
            tool_call_budget: budget.tool_calls,
            usage_budget: budget,
            attempt_number,
            status: LeaseStatus::Active,
            lease_digest: String::new(),
        };
        lease.lease_digest = lease.compute_digest()?;
        Ok(lease)
    }

    fn digest_view(&self) -> Self {
        let mut view = self.clone();
        view.lease_digest.clear();
        view
    }

    pub fn compute_digest(&self) -> Result<String, ExecutionError> {
        canonical_hash(&self.digest_view())
    }

    pub fn verify(&self, packet: &TaskPacket, now_ms: u64) -> Result<(), ExecutionError> {
        if self.status != LeaseStatus::Active {
            return Err(ExecutionError::LeaseInactive);
        }
        if now_ms >= self.expires_at_ms {
            return Err(ExecutionError::LeaseExpired);
        }
        if self.compute_digest()? != self.lease_digest {
            return Err(ExecutionError::LeaseTampered);
        }
        packet.verify_digest()?;
        if packet.lease_id != self.lease_id
            || packet.task_packet_digest != self.task_packet_digest
            || packet.mission_id != self.mission_id
            || packet.mission_revision != self.mission_revision
            || packet.task_id != self.task_id
            || packet.attempt_number != self.attempt_number
        {
            return Err(ExecutionError::LeaseBindingMismatch);
        }
        Ok(())
    }

    fn revoke(&mut self) -> Result<(), ExecutionError> {
        self.status = LeaseStatus::Revoked;
        self.lease_digest = self.compute_digest()?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionTask {
    pub task_id: String,
    pub objective: String,
    pub requirement_ids: Vec<String>,
    pub dependency_ids: Vec<String>,
    pub priority: RequirementPriority,
    pub state: ExecutionTaskState,
    pub scope: LeaseScope,
    pub usage_budget: UsageBudget,
    pub retry_policy: RetryPolicy,
    pub evidence_obligations: Vec<EvidenceObligation>,
    pub attempt_number: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskAttempt {
    pub attempt_id: String,
    pub task_id: String,
    pub attempt_number: u32,
    pub packet_digest: String,
    pub lease_id: String,
    pub state: TaskAttemptState,
    pub started_at_ms: u64,
    pub ended_at_ms: Option<u64>,
    pub failure_class: Option<FailureClass>,
    pub failure_fingerprint: Option<String>,
    pub usage: UsageTelemetry,
    pub termination_reason: Option<String>,
    // The field was added after the first Beta ledger format.  Omitting the
    // legacy Unknown value preserves the old canonical JSON/HMAC while still
    // persisting every explicit boundary in new ledgers.
    #[serde(default, skip_serializing_if = "is_unknown_execution_boundary")]
    pub execution_boundary: AttemptExecutionBoundary,
    #[serde(default)]
    completion_authority: Option<ExecutionCompletionAuthority>,
    /// Authenticated pre/post inventories make artifact attribution explicit.
    /// Historical ledgers omit these fields and therefore cannot authorize
    /// fresh P8 evidence for an old attempt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    workspace_before: Option<BTreeMap<String, String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    workspace_after: Option<BTreeMap<String, String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ExecutionCompletionAuthority {
    task_id: String,
    attempt_id: String,
    packet_digest: String,
    lease_id: String,
    process_digest: String,
    started_at_ms: u64,
    ended_at_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum WorkspaceArtifactChangeKind {
    New,
    Modified,
    Deleted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceArtifactChange {
    pub path: String,
    pub kind: WorkspaceArtifactChangeKind,
    pub before_digest: Option<String>,
    pub after_digest: Option<String>,
}

/// Exact successful execution identity exported by the authenticated P7
/// ledger. P8 can consume this view but cannot manufacture it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SuccessfulExecutionIdentity {
    pub mission_id: String,
    pub mission_revision: u64,
    pub seal_hash: String,
    pub run_id: String,
    pub task_id: String,
    pub attempt_id: String,
    pub attempt_number: u32,
    pub packet_digest: String,
    pub lease_id: String,
    pub lease_digest: String,
    pub process_digest: String,
    pub workspace_identity: String,
    pub workspace_before_fingerprint: String,
    pub workspace_after_fingerprint: String,
    pub artifact_changes: Vec<WorkspaceArtifactChange>,
    pub started_at_ms: u64,
    pub ended_at_ms: u64,
    pub lease_expires_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoopFingerprint {
    pub fingerprint: String,
    pub tool: String,
    pub operation: String,
    pub normalized_arguments: Vec<String>,
    pub working_scope: String,
    pub environment_identity: String,
    pub failure_fingerprint: String,
    pub occurrences: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OscillationFingerprint {
    pub fingerprint: String,
    pub snapshots: Vec<String>,
    pub changed_paths: Vec<String>,
    pub occurrences: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ProgressSnapshot {
    pub workspace_fingerprint: String,
    pub task_state_fingerprint: String,
    pub changed_paths: Vec<String>,
    pub passing_tests: u64,
    #[serde(default)]
    pub passing_tests_observed: bool,
    pub useful_artifacts: u64,
    #[serde(default)]
    pub artifacts_observed: bool,
    pub resolved_blockers: u64,
    pub dependency_completions: u64,
    pub diagnostic_information: u64,
}

impl ProgressSnapshot {
    fn progress_fingerprint(&self) -> Result<String, ExecutionError> {
        canonical_hash(self)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContinuationRecord {
    pub continuation_id: String,
    pub mission_id: String,
    pub mission_revision: u64,
    pub seal_hash: String,
    pub current_task: Option<String>,
    pub completed_tasks: Vec<String>,
    pub remaining_dependencies: BTreeMap<String, Vec<String>>,
    pub previous_attempt_summary: String,
    pub workspace_fingerprint: String,
    pub failure_summary: Vec<String>,
    pub budget_remaining: UsageBudget,
    pub attempt_number: u32,
    pub loop_history: Vec<LoopFingerprint>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiagnosticTask {
    pub diagnostic_id: String,
    pub blocked_task_id: String,
    pub provenance: String,
    pub objective: String,
    pub budget: UsageBudget,
    pub lease_id: Option<String>,
    pub state: ExecutionTaskState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalModificationRecord {
    pub detected_at_ms: u64,
    pub expected_paths: Vec<String>,
    pub changed_paths: Vec<String>,
    pub classification: String,
    pub before_fingerprint: String,
    pub after_fingerprint: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SafeBoundary {
    pub requested_at_ms: u64,
    pub timeout_ms: u64,
    pub atomic_action_allowed: bool,
    pub reason: String,
    pub reached: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionRequest {
    pub tool: String,
    pub operation: String,
    pub arguments: Vec<String>,
    pub working_scope: String,
    pub environment_identity: String,
    pub mutable: bool,
    pub paths: Vec<String>,
    pub external_authority: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionResult {
    pub success: bool,
    pub failure_class: Option<FailureClass>,
    pub failure_message: Option<String>,
    pub changed_paths: Vec<String>,
    pub workspace_fingerprint: String,
    pub passing_tests: u64,
    pub useful_artifacts: u64,
    pub wall_time_ms: u64,
    pub estimated_cost_micros: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE", tag = "kind")]
pub enum ExecutionEventKind {
    RunStarted,
    TaskScheduled,
    LeaseIssued,
    AttemptStarted,
    ActionResult,
    RetryScheduled,
    BudgetWarning,
    LoopSignal,
    NoProgressSignal,
    DiagnosticTaskInjected,
    ExternalModification,
    LeaseRevoked,
    SafeBoundaryStop,
    TurnEndedIncomplete,
    ContinuationStarted,
    ContinuationAuthorized,
    TaskImplementationFinished,
    AuthorityRevalidationRequired,
    TaskDriftDenied,
    RecoveryRevalidated,
    RetryAuthorized,
    VerificationCorrectionAuthorized,
    ProgressRenewalAuthorized,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionEvent {
    pub sequence: u64,
    pub occurred_at_ms: u64,
    pub task_id: Option<String>,
    pub kind: ExecutionEventKind,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SchedulerPolicy {
    pub default_budget: UsageBudget,
    pub default_retry_policy: RetryPolicy,
    pub repeated_failure_threshold: u32,
    pub oscillation_threshold: u32,
    pub no_progress_threshold: u32,
    pub safe_boundary_timeout_ms: u64,
    pub max_parallel_tasks: usize,
    #[serde(default)]
    pub execution_time_policy: ExecutionTimePolicy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionTimePolicy {
    pub task_wall_clock_ms: u64,
    pub lease_duration_ms: u64,
    pub adapter_safety_timeout_ms: u64,
}

impl Default for ExecutionTimePolicy {
    fn default() -> Self {
        Self {
            task_wall_clock_ms: BETA_TASK_EXECUTION_BUDGET_MS,
            lease_duration_ms: BETA_TASK_EXECUTION_BUDGET_MS,
            adapter_safety_timeout_ms: ADAPTER_PROCESS_SAFETY_TIMEOUT_MS,
        }
    }
}

impl ExecutionTimePolicy {
    pub fn validate(&self) -> Result<(), ExecutionError> {
        if self.task_wall_clock_ms == 0
            || self.lease_duration_ms != self.task_wall_clock_ms
            || self.adapter_safety_timeout_ms < self.task_wall_clock_ms
        {
            return Err(ExecutionError::PolicyDenied(
                "execution time policy must be finite, lease-aligned, and bounded by the adapter safety timeout".into(),
            ));
        }
        Ok(())
    }
}

impl Default for SchedulerPolicy {
    fn default() -> Self {
        Self {
            default_budget: UsageBudget::default(),
            default_retry_policy: RetryPolicy::default(),
            repeated_failure_threshold: 3,
            oscillation_threshold: 4,
            no_progress_threshold: 3,
            safe_boundary_timeout_ms: 2_000,
            max_parallel_tasks: 2,
            execution_time_policy: ExecutionTimePolicy::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionRun {
    pub ledger_version: String,
    pub run_id: String,
    pub mission_id: String,
    pub mission_revision: u64,
    pub seal_hash: String,
    pub project_id: String,
    pub workspace: PathBuf,
    pub workspace_fingerprint: String,
    pub state: ExecutionRunState,
    pub tasks: BTreeMap<String, ExecutionTask>,
    pub attempts: Vec<TaskAttempt>,
    pub leases: Vec<ExecutionLease>,
    pub events: Vec<ExecutionEvent>,
    pub usage: UsageTelemetry,
    pub loop_signals: Vec<LoopFingerprint>,
    pub oscillation_signals: Vec<OscillationFingerprint>,
    pub continuations: Vec<ContinuationRecord>,
    pub diagnostics: Vec<DiagnosticTask>,
    pub external_modifications: Vec<ExternalModificationRecord>,
    pub progress: Vec<ProgressSnapshot>,
    pub watchdog_state: WatchdogState,
    pub policy: SchedulerPolicy,
    pub safe_boundary: Option<SafeBoundary>,
    pub current_turn: u32,
    pub last_error: Option<String>,
    pub no_progress_occurrences: u32,
    #[serde(default)]
    pub integrity_version: String,
    #[serde(default)]
    pub integrity_tag: String,
}

impl ExecutionRun {
    fn build_from_validated_handoff(
        revision: &MissionRevision,
        handoff: &ExecutionHandoff,
        trusted: &TrustedSignerSet,
        workspace: PathBuf,
        workspace_fingerprint: &str,
        policy: SchedulerPolicy,
        now_ms: u64,
    ) -> Result<Self, ExecutionError> {
        let engine = AuthorityEngine;
        let seal_validation = engine
            .validate_seal(revision)
            .map_err(|error| ExecutionError::RevalidationRequired(error.to_string()))?;
        if !seal_validation.valid {
            return Err(ExecutionError::RevalidationRequired(
                "P6 seal validation failed".into(),
            ));
        }
        if revision.contract.registry_id.is_empty() {
            return Err(ExecutionError::RevalidationRequired(
                "registry identity missing".into(),
            ));
        }
        if revision.seal.state != "SEALED"
            || revision.seal.mission_id != revision.contract.mission_id
            || revision.seal.project_id != revision.contract.project_id
        {
            return Err(ExecutionError::RevalidationRequired(
                "sealed mission identity changed".into(),
            ));
        }
        if handoff.state != "READY_FOR_EXECUTION"
            || handoff.mission_id != revision.seal.mission_id
            || handoff.revision != revision.revision
            || handoff.contract_hash != revision.seal.contract_hash
        {
            return Err(ExecutionError::RevalidationRequired(
                "ExecutionHandoff does not match sealed mission".into(),
            ));
        }
        revision
            .contract
            .task_graph
            .validate(&revision.contract.requirement_graph)
            .map_err(|error| ExecutionError::RevalidationRequired(error.to_string()))?;
        validate_sealed_task_requirement_ids(revision)?;
        let expected_order = executable_task_order(revision)?;
        if handoff.task_order != expected_order {
            return Err(ExecutionError::RevalidationRequired(
                "handoff task order is stale or contains non-executable work".into(),
            ));
        }
        if !trusted_signers_present(trusted) {
            return Err(ExecutionError::RevalidationRequired(
                "trusted signer set is empty".into(),
            ));
        }

        let requirements = revision
            .contract
            .requirement_graph
            .requirements
            .iter()
            .map(|requirement| (&requirement.requirement_id, requirement))
            .collect::<BTreeMap<_, _>>();
        let mut tasks = BTreeMap::new();
        for task_id in &handoff.task_order {
            let task = revision
                .contract
                .task_graph
                .tasks
                .iter()
                .find(|task| &task.task_id == task_id)
                .ok_or_else(|| {
                    ExecutionError::RevalidationRequired("handoff task missing".into())
                })?;
            let priority = task
                .requirement_ids
                .iter()
                .filter_map(|id| requirements.get(id).map(|requirement| requirement.priority))
                .min()
                .unwrap_or(RequirementPriority::P3);
            let scope = LeaseScope {
                workspace: workspace.clone(),
                file_scopes: vec![workspace.display().to_string()],
                directory_scopes: vec![workspace.display().to_string()],
                shared_resources: vec!["workspace".into()],
                package_lockfiles: vec!["Cargo.lock".into(), "pnpm-lock.yaml".into()],
                generated_files: vec![],
                allowed_tools: ["antigravity".into(), "workspace".into()]
                    .into_iter()
                    .collect(),
                external_authority: BTreeSet::new(),
                scope_known: true,
            };
            let mut task_budget = policy.default_budget.clone();
            task_budget.wall_clock_ms = beta_task_wall_clock_budget_ms(
                priority,
                task.requirement_ids.len(),
                task.dependency_ids.len(),
                task.evidence_obligations.len(),
                task.objective.len(),
                policy.execution_time_policy.adapter_safety_timeout_ms,
            );
            tasks.insert(
                task_id.clone(),
                ExecutionTask {
                    task_id: task.task_id.clone(),
                    objective: task.objective.clone(),
                    requirement_ids: task.requirement_ids.clone(),
                    dependency_ids: task.dependency_ids.clone(),
                    priority,
                    state: ExecutionTaskState::Pending,
                    scope,
                    usage_budget: task_budget,
                    retry_policy: policy.default_retry_policy.clone(),
                    evidence_obligations: task.evidence_obligations.clone(),
                    attempt_number: 0,
                },
            );
        }
        let run_id = canonical_hash(&(
            revision.seal.mission_id.as_str(),
            revision.revision,
            revision.seal.contract_hash.as_str(),
        ))?;
        let mut run = Self {
            ledger_version: EXECUTION_LEDGER_VERSION.into(),
            run_id,
            mission_id: revision.seal.mission_id.clone(),
            mission_revision: revision.revision,
            seal_hash: revision.seal.contract_hash.clone(),
            project_id: revision.seal.project_id.clone(),
            workspace,
            workspace_fingerprint: workspace_fingerprint.into(),
            state: ExecutionRunState::Ready,
            tasks,
            attempts: vec![],
            leases: vec![],
            events: vec![],
            usage: UsageTelemetry::default(),
            loop_signals: vec![],
            oscillation_signals: vec![],
            continuations: vec![],
            diagnostics: vec![],
            external_modifications: vec![],
            progress: vec![],
            watchdog_state: WatchdogState::Healthy,
            policy,
            safe_boundary: None,
            current_turn: 1,
            last_error: None,
            no_progress_occurrences: 0,
            integrity_version: "p7-ledger-integrity-v1".into(),
            integrity_tag: String::new(),
        };
        run.emit(
            now_ms,
            None,
            ExecutionEventKind::RunStarted,
            "P6 handoff validated",
        )?;
        Ok(run)
    }

    /// Admit a handoff only when the exact signed registry that produced it is
    /// still verified and its identity is the one sealed into the mission.
    #[allow(clippy::too_many_arguments)]
    pub fn from_p6_handoff_with_registry(
        revision: &MissionRevision,
        handoff: &ExecutionHandoff,
        registry: &StandardsRegistry,
        trusted: &TrustedSignerSet,
        workspace: PathBuf,
        workspace_fingerprint: &str,
        policy: SchedulerPolicy,
        now_ms: u64,
    ) -> Result<Self, ExecutionError> {
        registry
            .verify(trusted, true)
            .map_err(|error| ExecutionError::RevalidationRequired(error.to_string()))?;
        if revision.contract.registry_id != registry.registry_id
            || revision.contract.registry_version != registry.registry_version
            || revision.contract.registry_digest != registry.registry_digest
        {
            return Err(ExecutionError::RevalidationRequired(
                "sealed mission registry identity does not match the verified registry".into(),
            ));
        }
        Self::build_from_validated_handoff(
            revision,
            handoff,
            trusted,
            workspace,
            workspace_fingerprint,
            policy,
            now_ms,
        )
    }

    pub fn runnable_tasks(&self) -> Vec<String> {
        let mut candidates = self
            .tasks
            .values()
            .filter(|task| {
                matches!(
                    task.state,
                    ExecutionTaskState::Pending
                        | ExecutionTaskState::Ready
                        | ExecutionTaskState::WaitingRetry
                ) && task.dependency_ids.iter().all(|dependency| {
                    self.tasks.get(dependency).is_some_and(|task| {
                        task.state == ExecutionTaskState::FinishedAwaitingVerification
                    })
                })
            })
            .map(|task| (&task.priority, task.task_id.clone()))
            .collect::<Vec<_>>();
        candidates.sort_by(|a, b| a.0.cmp(b.0).then_with(|| a.1.cmp(&b.1)));
        candidates.into_iter().map(|(_, id)| id).collect()
    }

    pub fn plan_parallel_batch(&self) -> Vec<String> {
        let mut selected: Vec<String> = Vec::new();
        for task_id in self.runnable_tasks() {
            if selected.len() >= self.policy.max_parallel_tasks {
                break;
            }
            let Some(task) = self.tasks.get(&task_id) else {
                continue;
            };
            if selected.iter().all(|other_id| {
                self.tasks
                    .get(other_id)
                    .is_some_and(|other| !task.scope.conflicts(&other.scope))
            }) {
                selected.push(task_id);
            }
        }
        selected
    }

    pub fn start_task(&mut self, task_id: &str, now_ms: u64) -> Result<TaskPacket, ExecutionError> {
        self.policy.execution_time_policy.validate()?;
        if matches!(
            self.state,
            ExecutionRunState::RevalidationRequired
                | ExecutionRunState::BlockedExternal
                | ExecutionRunState::SafeBoundaryReached
                | ExecutionRunState::Stopped
                | ExecutionRunState::StoppedIncomplete
        ) {
            return Err(ExecutionError::RevalidationRequired(
                "execution run is not in a mutable scheduling state".into(),
            ));
        }
        let runnable = self.runnable_tasks();
        if !runnable.iter().any(|id| id == task_id) {
            return Err(ExecutionError::DependencyNotReady(task_id.into()));
        }
        let task_snapshot = self
            .tasks
            .get(task_id)
            .cloned()
            .ok_or_else(|| ExecutionError::UnknownTask(task_id.into()))?;
        if task_snapshot.usage_budget.wall_clock_ms == 0 {
            return Err(ExecutionError::PolicyDenied(
                "task wall-clock authority must be finite and non-zero".into(),
            ));
        }
        let workspace_before = workspace_inventory(&self.workspace)?;
        let remaining_budget = self.remaining_budget(task_id)?;
        if remaining_budget.wall_clock_ms
            > self.policy.execution_time_policy.adapter_safety_timeout_ms
        {
            return Err(ExecutionError::PolicyDenied(
                "remaining task wall-clock authority exceeds the per-attempt safety ceiling".into(),
            ));
        }
        if budget_exhausted(&remaining_budget) {
            self.watchdog_state = WatchdogState::BudgetExhausted;
            return Err(ExecutionError::BudgetExhausted);
        }
        let active_task_ids = self
            .leases
            .iter()
            .filter(|lease| lease.status == LeaseStatus::Active)
            .map(|lease| lease.task_id.as_str())
            .collect::<BTreeSet<_>>();
        if !active_task_ids.contains(task_id)
            && active_task_ids.len() >= self.policy.max_parallel_tasks
        {
            return Err(ExecutionError::PolicyDenied(
                "maximum parallel task count is active".into(),
            ));
        }
        for lease in &mut self.leases {
            if lease.task_id == task_id && lease.status == LeaseStatus::Active {
                lease.revoke()?;
            }
        }
        if self.leases.iter().any(|lease| {
            lease.status == LeaseStatus::Active
                && lease.task_id != task_id
                && lease.scope.conflicts(&task_snapshot.scope)
        }) {
            return Err(ExecutionError::PolicyDenied(
                "task scope conflicts with an active lease".into(),
            ));
        }
        let task = self
            .tasks
            .get_mut(task_id)
            .ok_or_else(|| ExecutionError::UnknownTask(task_id.into()))?;
        for attempt in self.attempts.iter_mut().filter(|attempt| {
            attempt.task_id == task_id && attempt.state == TaskAttemptState::Running
        }) {
            attempt.state = TaskAttemptState::WaitingRetry;
        }
        task.attempt_number = task.attempt_number.saturating_add(1);
        let packet = TaskPacket::new(
            &self.mission_id,
            self.mission_revision,
            &self.seal_hash,
            &task.task_id,
            &task.objective,
            task.requirement_ids.clone(),
            task.dependency_ids.clone(),
            &self.project_id,
            self.workspace.clone(),
            task.scope.allowed_tools.clone(),
            task.scope.external_authority.clone(),
            remaining_budget.clone(),
            task.retry_policy.clone(),
            task.evidence_obligations.clone(),
            &self.workspace_fingerprint,
            task.attempt_number,
            now_ms.saturating_add(remaining_budget.wall_clock_ms),
        )?;
        let lease = ExecutionLease::issue(
            &self.mission_id,
            self.mission_revision,
            task_id,
            &packet.task_packet_digest,
            task.scope.clone(),
            remaining_budget,
            task.attempt_number,
            now_ms,
        )?;
        let mut packet = packet;
        packet.bind_lease(&lease.lease_id, lease.expires_at_ms)?;
        let attempt_id = canonical_hash(&(&self.run_id, task_id, task.attempt_number, now_ms))?;
        self.attempts.push(TaskAttempt {
            attempt_id,
            task_id: task_id.into(),
            attempt_number: task.attempt_number,
            packet_digest: packet.task_packet_digest.clone(),
            lease_id: lease.lease_id.clone(),
            state: TaskAttemptState::Running,
            started_at_ms: now_ms,
            ended_at_ms: None,
            failure_class: None,
            failure_fingerprint: None,
            usage: UsageTelemetry {
                attempt_count: task.attempt_number,
                quality: MeasurementQuality::Unavailable,
                ..UsageTelemetry::default()
            },
            termination_reason: None,
            execution_boundary: AttemptExecutionBoundary::NotStarted,
            completion_authority: None,
            workspace_before: Some(workspace_before),
            workspace_after: None,
        });
        self.usage.attempt_count = self.usage.attempt_count.saturating_add(1);
        task.state = ExecutionTaskState::Running;
        self.leases.push(lease);
        self.state = ExecutionRunState::Running;
        self.emit(
            now_ms,
            Some(task_id.into()),
            ExecutionEventKind::LeaseIssued,
            "scheduler-issued-lease",
        )?;
        self.emit(
            now_ms,
            Some(task_id.into()),
            ExecutionEventKind::AttemptStarted,
            "attempt-started",
        )?;
        Ok(packet)
    }

    pub fn authorize_action(
        &mut self,
        task_id: &str,
        lease_id: &str,
        packet: &TaskPacket,
        action: &ActionRequest,
        now_ms: u64,
    ) -> Result<(), ExecutionError> {
        let (lease_task_id, tool_call_budget, wall_clock_budget, retry_budget, scope) = {
            let lease = self
                .leases
                .iter()
                .find(|lease| lease.lease_id == lease_id)
                .ok_or(ExecutionError::LeaseInactive)?;
            lease.verify(packet, now_ms)?;
            (
                lease.task_id.clone(),
                lease.tool_call_budget,
                lease.usage_budget.wall_clock_ms,
                lease.usage_budget.retry_attempts,
                lease.scope.clone(),
            )
        };
        if lease_task_id != task_id {
            return Err(ExecutionError::LeaseBindingMismatch);
        }
        let task = self
            .tasks
            .get(task_id)
            .ok_or_else(|| ExecutionError::UnknownTask(task_id.into()))?;
        if action.mutable && task.state != ExecutionTaskState::Running {
            return Err(ExecutionError::TaskNotRunning(task_id.into()));
        }
        if action.mutable && action.paths.is_empty() {
            self.watchdog_state = WatchdogState::DriftDetected;
            self.emit(
                now_ms,
                Some(task_id.into()),
                ExecutionEventKind::TaskDriftDenied,
                "mutable action must declare bounded paths",
            )?;
            return Err(ExecutionError::TaskDrift);
        }
        if !scope.allowed_tools.contains(&action.tool) {
            return Err(ExecutionError::PolicyDenied(
                "tool is not allowed by lease".into(),
            ));
        }
        if action
            .external_authority
            .as_ref()
            .is_some_and(|authority| !scope.external_authority.contains(authority))
        {
            return Err(ExecutionError::PolicyDenied(
                "external authority is not allowed by lease".into(),
            ));
        }
        if action.paths.iter().any(|path| !scope.allows_path(path)) {
            self.watchdog_state = WatchdogState::DriftDetected;
            self.emit(
                now_ms,
                Some(task_id.into()),
                ExecutionEventKind::TaskDriftDenied,
                "action path is outside the leased scope",
            )?;
            return Err(ExecutionError::TaskDrift);
        }
        if action.mutable {
            let used = self.task_usage(task_id)?;
            if let Some(limit) = self
                .tasks
                .get(task_id)
                .and_then(|task| task.usage_budget.cost_micros)
            {
                let cost = used
                    .estimated_cost_micros
                    .unwrap_or_default()
                    .max(used.actual_cost_micros.unwrap_or_default());
                if cost >= limit {
                    self.watchdog_state = WatchdogState::BudgetExhausted;
                    return Err(ExecutionError::BudgetExhausted);
                }
            }
            let attempt = self.current_attempt_mut(task_id)?;
            if attempt.usage.tool_calls >= tool_call_budget
                || attempt.usage.execution_steps >= packet.step_budget
                || attempt.usage.wall_time_ms >= wall_clock_budget
                || attempt.usage.attempt_count > retry_budget + 1
            {
                self.watchdog_state = WatchdogState::BudgetExhausted;
                return Err(ExecutionError::BudgetExhausted);
            }
        }
        Ok(())
    }

    pub fn record_action(
        &mut self,
        task_id: &str,
        action: &ActionRequest,
        result: ActionResult,
        now_ms: u64,
    ) -> Result<RetryDecision, ExecutionError> {
        let snapshot = ProgressSnapshot {
            workspace_fingerprint: result.workspace_fingerprint.clone(),
            task_state_fingerprint: self.task_state_fingerprint()?,
            changed_paths: result.changed_paths.clone(),
            passing_tests: result.passing_tests,
            useful_artifacts: result.useful_artifacts,
            ..ProgressSnapshot::default()
        };
        let decision = self.record_action_internal(task_id, action, result, now_ms, true)?;
        self.record_workspace_observation(snapshot, now_ms)?;
        Ok(decision)
    }

    fn record_action_internal(
        &mut self,
        task_id: &str,
        action: &ActionRequest,
        result: ActionResult,
        now_ms: u64,
        allow_retry_scheduling: bool,
    ) -> Result<RetryDecision, ExecutionError> {
        let failure_class = result.failure_class;
        let failure_fingerprint = canonical_hash(&(
            action_fingerprint(action)?,
            failure_class,
            result.failure_message.clone().unwrap_or_default(),
        ))?;
        let fingerprint = LoopFingerprint {
            fingerprint: action_fingerprint(action)?,
            tool: action.tool.clone(),
            operation: action.operation.clone(),
            normalized_arguments: normalize_arguments(&action.arguments),
            working_scope: normalize_path(&action.working_scope),
            environment_identity: action.environment_identity.clone(),
            failure_fingerprint: failure_fingerprint.clone(),
            occurrences: 1,
        };
        if !result.success {
            if let Some(existing) = self.loop_signals.iter_mut().find(|existing| {
                existing.fingerprint == fingerprint.fingerprint
                    && existing.failure_fingerprint == failure_fingerprint
            }) {
                existing.occurrences = existing.occurrences.saturating_add(1);
            } else {
                self.loop_signals.push(fingerprint.clone());
            }
        }
        let occurrences = self
            .loop_signals
            .iter()
            .find(|existing| {
                existing.fingerprint == fingerprint.fingerprint
                    && existing.failure_fingerprint == failure_fingerprint
            })
            .map(|existing| existing.occurrences)
            .unwrap_or(0);
        let attempt = self.current_attempt_mut(task_id)?;
        attempt.usage.tool_calls = attempt.usage.tool_calls.saturating_add(1);
        attempt.usage.execution_steps = attempt.usage.execution_steps.saturating_add(1);
        attempt.usage.wall_time_ms = attempt
            .usage
            .wall_time_ms
            .saturating_add(result.wall_time_ms);
        attempt.usage.quality = if result.estimated_cost_micros.is_some() {
            MeasurementQuality::Estimated
        } else {
            MeasurementQuality::Unavailable
        };
        if let Some(cost) = result.estimated_cost_micros {
            attempt.usage.estimated_cost_micros = Some(
                attempt
                    .usage
                    .estimated_cost_micros
                    .unwrap_or_default()
                    .saturating_add(cost),
            );
        }
        let _ = attempt;
        self.usage.tool_calls = self.usage.tool_calls.saturating_add(1);
        self.usage.execution_steps = self.usage.execution_steps.saturating_add(1);
        self.usage.wall_time_ms = self.usage.wall_time_ms.saturating_add(result.wall_time_ms);
        if let Some(cost) = result.estimated_cost_micros {
            self.usage.estimated_cost_micros = Some(
                self.usage
                    .estimated_cost_micros
                    .unwrap_or_default()
                    .saturating_add(cost),
            );
        }
        if result.success {
            self.workspace_fingerprint = result.workspace_fingerprint;
            self.emit(
                now_ms,
                Some(task_id.into()),
                ExecutionEventKind::ActionResult,
                "action-succeeded",
            )?;
            Ok(RetryDecision::Stop)
        } else if occurrences >= self.policy.repeated_failure_threshold {
            self.watchdog_state = WatchdogState::RepeatedCommand;
            self.inject_diagnostic_task(task_id, "repeated equivalent failing action", now_ms)?;
            self.tasks
                .get_mut(task_id)
                .ok_or_else(|| ExecutionError::UnknownTask(task_id.into()))?
                .state = ExecutionTaskState::BlockedExternal;
            self.emit(
                now_ms,
                Some(task_id.into()),
                ExecutionEventKind::LoopSignal,
                "repeated-equivalent-failure-stopped",
            )?;
            Ok(RetryDecision::InjectDiagnostic)
        } else {
            let class = failure_class.unwrap_or(FailureClass::Unknown);
            let decision = if allow_retry_scheduling {
                self.retry_decision(task_id, class)?
            } else {
                RetryDecision::Stop
            };
            if decision == RetryDecision::Retry {
                self.usage.retry_count = self.usage.retry_count.saturating_add(1);
                if let Ok(attempt) = self.current_attempt_mut(task_id) {
                    attempt.usage.retry_count = attempt.usage.retry_count.saturating_add(1);
                }
                self.tasks
                    .get_mut(task_id)
                    .ok_or_else(|| ExecutionError::UnknownTask(task_id.into()))?
                    .state = ExecutionTaskState::WaitingRetry;
                self.state = ExecutionRunState::Ready;
                self.emit(
                    now_ms,
                    Some(task_id.into()),
                    ExecutionEventKind::RetryScheduled,
                    "bounded-retry-scheduled",
                )?;
            } else if decision == RetryDecision::RevalidationRequired {
                self.state = ExecutionRunState::RevalidationRequired;
            } else {
                let task = self
                    .tasks
                    .get_mut(task_id)
                    .ok_or_else(|| ExecutionError::UnknownTask(task_id.into()))?;
                task.state = if class == FailureClass::ExternalUnavailable {
                    ExecutionTaskState::BlockedExternal
                } else {
                    ExecutionTaskState::Failed
                };
            }
            if let Ok(attempt) = self.current_attempt_mut(task_id) {
                attempt.state = if decision == RetryDecision::Retry {
                    TaskAttemptState::WaitingRetry
                } else {
                    TaskAttemptState::Failed
                };
                attempt.ended_at_ms = Some(now_ms);
                attempt.failure_class = failure_class;
                attempt.termination_reason = result.failure_message.clone();
            }
            Ok(decision)
        }
    }

    pub fn finish_task(&mut self, task_id: &str, now_ms: u64) -> Result<(), ExecutionError> {
        let workspace_after = workspace_inventory(&self.workspace)?;
        self.record_completion_workspace(task_id, workspace_after)?;
        self.promote_task_from_trusted_completion(task_id, now_ms, false)
    }

    fn record_completion_workspace(
        &mut self,
        task_id: &str,
        workspace_after: BTreeMap<String, String>,
    ) -> Result<(), ExecutionError> {
        let attempt = self
            .attempts
            .iter_mut()
            .rev()
            .find(|attempt| {
                attempt.task_id == task_id && attempt.state == TaskAttemptState::Running
            })
            .ok_or_else(|| ExecutionError::AttemptMissing(task_id.into()))?;
        if attempt.completion_authority.is_none() {
            return Err(ExecutionError::PolicyDenied(
                "task has no trusted adapter completion authority".into(),
            ));
        }
        if let Some(existing) = attempt.workspace_after.as_ref() {
            if existing == &workspace_after {
                return Ok(());
            }
            return Err(ExecutionError::RevalidationRequired(
                "conflicting post-execution workspace replay for an already authenticated completion"
                    .into(),
            ));
        }
        attempt.workspace_after = Some(workspace_after);
        Ok(())
    }

    fn promote_task_from_trusted_completion(
        &mut self,
        task_id: &str,
        completion_time_ms: u64,
        require_current_workspace_match: bool,
    ) -> Result<(), ExecutionError> {
        let task_snapshot = self
            .tasks
            .get(task_id)
            .cloned()
            .ok_or_else(|| ExecutionError::UnknownTask(task_id.into()))?;

        // Replaying the exact already-promoted trusted completion is harmless
        // and must be idempotent. Any disagreement in receipt, lease, time, or
        // post-workspace is a new authority claim and therefore fails closed.
        if task_snapshot.state == ExecutionTaskState::FinishedAwaitingVerification {
            let attempt = self
                .attempts
                .iter()
                .rev()
                .find(|attempt| {
                    attempt.task_id == task_id && attempt.state == TaskAttemptState::Succeeded
                })
                .ok_or_else(|| {
                    ExecutionError::RevalidationRequired(
                        "finished task has no successful attempt for completion replay".into(),
                    )
                })?;
            let completion = attempt.completion_authority.as_ref().ok_or_else(|| {
                ExecutionError::RevalidationRequired(
                    "finished task has no trusted completion receipt for replay".into(),
                )
            })?;
            let workspace_after = attempt.workspace_after.as_ref().ok_or_else(|| {
                ExecutionError::RevalidationRequired(
                    "finished task has no authenticated post-execution workspace for replay".into(),
                )
            })?;
            let lease = self
                .leases
                .iter()
                .find(|lease| lease.lease_id == attempt.lease_id)
                .ok_or_else(|| {
                    ExecutionError::RevalidationRequired(
                        "finished task completion lease is unavailable for replay".into(),
                    )
                })?;
            let exact = lease.status == LeaseStatus::Consumed
                && lease.task_id == task_id
                && lease.task_packet_digest == attempt.packet_digest
                && lease.compute_digest()? == lease.lease_digest
                && completion.task_id == task_id
                && completion.attempt_id == attempt.attempt_id
                && completion.packet_digest == attempt.packet_digest
                && completion.lease_id == attempt.lease_id
                && completion.ended_at_ms == completion_time_ms
                && attempt.ended_at_ms == Some(completion_time_ms)
                && inventory_fingerprint(workspace_after)? == self.workspace_fingerprint;
            if !exact {
                return Err(ExecutionError::RevalidationRequired(
                    "conflicting replay of an already promoted trusted completion".into(),
                ));
            }
            if require_current_workspace_match {
                let current_workspace = workspace_inventory(&self.workspace)?;
                if &current_workspace != workspace_after {
                    return Err(ExecutionError::RevalidationRequired(
                        "the workspace changed after the trusted completion was promoted".into(),
                    ));
                }
            }
            return Ok(());
        }

        if task_snapshot.state != ExecutionTaskState::Running
            || task_snapshot.dependency_ids.iter().any(|dependency| {
                self.tasks.get(dependency).is_none_or(|task| {
                    task.state != ExecutionTaskState::FinishedAwaitingVerification
                })
            })
        {
            return Err(ExecutionError::PolicyDenied(
                "task is not execution-authorized or dependencies are incomplete".into(),
            ));
        }
        let attempt = self
            .attempts
            .iter()
            .rev()
            .find(|attempt| {
                attempt.task_id == task_id && attempt.state == TaskAttemptState::Running
            })
            .cloned()
            .ok_or_else(|| ExecutionError::AttemptMissing(task_id.into()))?;
        let completion = attempt.completion_authority.clone().ok_or_else(|| {
            ExecutionError::PolicyDenied("task has no trusted adapter completion authority".into())
        })?;
        let workspace_after = attempt.workspace_after.clone().ok_or_else(|| {
            ExecutionError::PolicyDenied(
                "task completion has no authenticated post-execution workspace inventory".into(),
            )
        })?;
        let lease = self
            .leases
            .iter()
            .find(|lease| lease.lease_id == attempt.lease_id)
            .ok_or(ExecutionError::LeaseInactive)?;
        if lease.status != LeaseStatus::Active {
            return Err(ExecutionError::LeaseInactive);
        }
        if lease.task_id != task_id
            || lease.task_packet_digest != attempt.packet_digest
            || lease.compute_digest()? != lease.lease_digest
            || completion.task_id != task_id
            || completion.attempt_id != attempt.attempt_id
            || completion.packet_digest != attempt.packet_digest
            || completion.lease_id != attempt.lease_id
            || completion.ended_at_ms >= lease.expires_at_ms
            || completion.ended_at_ms != completion_time_ms
        {
            return Err(ExecutionError::LeaseExpired);
        }
        if require_current_workspace_match {
            let current_workspace = workspace_inventory(&self.workspace)?;
            if current_workspace != workspace_after {
                return Err(ExecutionError::RevalidationRequired(
                    "the workspace changed after the trusted executor completion receipt was persisted"
                        .into(),
                ));
            }
        }
        let workspace_fingerprint = inventory_fingerprint(&workspace_after)?;
        let task = self
            .tasks
            .get_mut(task_id)
            .ok_or_else(|| ExecutionError::UnknownTask(task_id.into()))?;
        task.state = ExecutionTaskState::FinishedAwaitingVerification;
        let current = self.current_attempt_mut(task_id)?;
        current.state = TaskAttemptState::Succeeded;
        current.ended_at_ms = Some(completion_time_ms);
        if let Some(lease) = self
            .leases
            .iter_mut()
            .find(|lease| lease.lease_id == attempt.lease_id && lease.status == LeaseStatus::Active)
        {
            lease.status = LeaseStatus::Consumed;
            lease.lease_digest = lease.compute_digest()?;
        }
        self.workspace_fingerprint = workspace_fingerprint;
        self.emit(
            completion_time_ms,
            Some(task_id.into()),
            ExecutionEventKind::TaskImplementationFinished,
            "implementation-finished-awaiting-verification",
        )?;
        self.update_run_state();
        Ok(())
    }

    /// Recover only a completion that was already durably authenticated before
    /// the desktop stopped. Exit code or workspace files alone are never enough.
    pub fn recover_trusted_completion_after_restart(
        &mut self,
        now_ms: u64,
    ) -> Result<bool, ExecutionError> {
        if self.state != ExecutionRunState::Running {
            return Ok(false);
        }
        let candidates = self
            .attempts
            .iter()
            .filter(|attempt| {
                attempt.state == TaskAttemptState::Running
                    && attempt.completion_authority.is_some()
                    && attempt.workspace_after.is_some()
            })
            .map(|attempt| {
                (
                    attempt.task_id.clone(),
                    attempt
                        .completion_authority
                        .as_ref()
                        .map(|completion| completion.ended_at_ms)
                        .unwrap_or_default(),
                )
            })
            .collect::<Vec<_>>();
        if candidates.is_empty() {
            return Ok(false);
        }
        if candidates.len() != 1 {
            return Err(ExecutionError::RevalidationRequired(
                "restart found more than one unpromoted trusted completion receipt".into(),
            ));
        }
        let (task_id, ended_at_ms) = &candidates[0];
        self.promote_task_from_trusted_completion(task_id, *ended_at_ms, true)?;
        self.emit(
            now_ms,
            Some(task_id.clone()),
            ExecutionEventKind::RecoveryRevalidated,
            "trusted executor completion receipt and exact post-workspace recovered after restart",
        )?;
        Ok(true)
    }

    /// Reopen only the exact task(s) whose sealed requirements failed
    /// deterministic verification, plus downstream dependents that can no
    /// longer be trusted. Historical successful attempts remain immutable.
    /// Each reopened task receives one fresh bounded correction allowance and
    /// may not exceed its sealed retry-attempt policy.
    pub fn authorize_verification_correction(
        &mut self,
        failed_requirement_ids: &BTreeSet<String>,
        now_ms: u64,
    ) -> Result<Vec<String>, ExecutionError> {
        if self.state != ExecutionRunState::ExecutionTasksFinishedAwaitingVerification {
            return Err(ExecutionError::PolicyDenied(
                "verification correction requires a finished implementation run".into(),
            ));
        }
        if failed_requirement_ids.is_empty() {
            return Err(ExecutionError::PolicyDenied(
                "verification correction requires an exact failed requirement".into(),
            ));
        }

        let known_requirements = self
            .tasks
            .values()
            .flat_map(|task| task.requirement_ids.iter().cloned())
            .collect::<BTreeSet<_>>();
        if failed_requirement_ids
            .iter()
            .any(|requirement_id| !known_requirements.contains(requirement_id))
        {
            return Err(ExecutionError::RevalidationRequired(
                "verification failure references a requirement outside the sealed task graph"
                    .into(),
            ));
        }

        let mut affected = self
            .tasks
            .values()
            .filter(|task| {
                task.requirement_ids
                    .iter()
                    .any(|id| failed_requirement_ids.contains(id))
            })
            .map(|task| task.task_id.clone())
            .collect::<BTreeSet<_>>();
        if affected.is_empty() {
            return Err(ExecutionError::RevalidationRequired(
                "failed verification requirement has no executable sealed task".into(),
            ));
        }

        loop {
            let mut changed = false;
            for task in self.tasks.values() {
                if !affected.contains(&task.task_id)
                    && task
                        .dependency_ids
                        .iter()
                        .any(|dependency| affected.contains(dependency))
                {
                    affected.insert(task.task_id.clone());
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }

        for task_id in &affected {
            let task = self
                .tasks
                .get(task_id)
                .ok_or_else(|| ExecutionError::UnknownTask(task_id.clone()))?;
            if self.events.iter().any(|event| {
                event.task_id.as_deref() == Some(task_id.as_str())
                    && event.kind == ExecutionEventKind::VerificationCorrectionAuthorized
            }) {
                return Err(ExecutionError::PolicyDenied(format!(
                    "the single bounded verification correction was already used for {task_id}"
                )));
            }
            if task.state != ExecutionTaskState::FinishedAwaitingVerification {
                return Err(ExecutionError::RevalidationRequired(format!(
                    "verification correction target {task_id} is not at a finished task boundary"
                )));
            }
        }

        // A correction is new bounded authority under the current Beta policy.
        // Prior usage is preserved and the new allowance is added on top, so
        // historical work is never erased to manufacture budget.
        self.policy.execution_time_policy = ExecutionTimePolicy::default();
        self.policy.default_budget = UsageBudget::default();
        let mut budgets = BTreeMap::new();
        for task_id in &affected {
            let task = self
                .tasks
                .get(task_id)
                .ok_or_else(|| ExecutionError::UnknownTask(task_id.clone()))?;
            let used = self.task_usage(task_id)?;
            let fresh_wall = beta_task_wall_clock_budget_ms(
                task.priority,
                task.requirement_ids.len(),
                task.dependency_ids.len(),
                task.evidence_obligations.len(),
                task.objective.len(),
                self.policy.execution_time_policy.adapter_safety_timeout_ms,
            );
            budgets.insert(
                task_id.clone(),
                UsageBudget {
                    wall_clock_ms: used.wall_time_ms.saturating_add(fresh_wall),
                    execution_steps: used
                        .execution_steps
                        .saturating_add(self.policy.default_budget.execution_steps),
                    tool_calls: used
                        .tool_calls
                        .saturating_add(self.policy.default_budget.tool_calls),
                    // A verification correction is separate bounded authority.
                    // Preserve historical attempts while allowing exactly one
                    // fresh correction attempt even if an interrupted attempt
                    // already consumed the normal retry count.
                    retry_attempts: task.attempt_number.saturating_add(1),
                    cost_micros: self.policy.default_budget.cost_micros,
                },
            );
        }

        for task_id in &affected {
            let task = self
                .tasks
                .get_mut(task_id)
                .ok_or_else(|| ExecutionError::UnknownTask(task_id.clone()))?;
            task.state = ExecutionTaskState::Pending;
            task.usage_budget = budgets
                .remove(task_id)
                .ok_or_else(|| ExecutionError::PolicyDenied("correction budget missing".into()))?;
            self.emit(
                now_ms,
                Some(task_id.clone()),
                ExecutionEventKind::VerificationCorrectionAuthorized,
                "deterministic verification failure reopened the exact task/dependent boundary",
            )?;
        }
        self.state = ExecutionRunState::Ready;
        self.last_error = Some(format!(
            "deterministic verification failed for {}; bounded correction authorized",
            failed_requirement_ids
                .iter()
                .cloned()
                .collect::<Vec<_>>()
                .join(",")
        ));
        Ok(affected.into_iter().collect())
    }

    pub fn has_pending_retry(&self) -> bool {
        self.state == ExecutionRunState::Ready
            && self
                .tasks
                .values()
                .any(|task| task.state == ExecutionTaskState::WaitingRetry)
    }

    fn progress_renewal_count(&self, task_id: &str) -> u32 {
        self.events
            .iter()
            .filter(|event| {
                event.task_id.as_deref() == Some(task_id)
                    && event.kind == ExecutionEventKind::ProgressRenewalAuthorized
            })
            .count() as u32
    }

    fn progress_supports_renewal(&self, snapshot: &ProgressSnapshot) -> bool {
        let prior = self.progress.last();
        let prior_passing_tests = prior
            .filter(|item| item.passing_tests_observed)
            .map(|item| item.passing_tests)
            .unwrap_or_default();
        let prior_artifacts = prior
            .filter(|item| item.artifacts_observed)
            .map(|item| item.useful_artifacts)
            .unwrap_or_default();
        let prior_resolved_blockers = prior.map(|item| item.resolved_blockers).unwrap_or_default();
        let prior_dependency_completions = prior
            .map(|item| item.dependency_completions)
            .unwrap_or_default();
        let prior_diagnostics = prior
            .map(|item| item.diagnostic_information)
            .unwrap_or_default();

        // Locked spec 06 defines progress as requirement/evidence/test/diagnostic
        // advancement. Arbitrary source edits are intentionally not authority:
        // edit churn must never buy another execution lease on its own.
        let tests_advanced =
            snapshot.passing_tests_observed && snapshot.passing_tests > prior_passing_tests;
        let evidence_advanced =
            snapshot.artifacts_observed && snapshot.useful_artifacts > prior_artifacts;
        let blocker_advanced = snapshot.resolved_blockers > prior_resolved_blockers;
        let requirement_advanced = snapshot.dependency_completions > prior_dependency_completions;
        let diagnostic_advanced = snapshot.diagnostic_information != 0
            && snapshot.diagnostic_information != prior_diagnostics;

        tests_advanced
            || evidence_advanced
            || blocker_advanced
            || requirement_advanced
            || diagnostic_advanced
    }

    fn authorize_progress_renewal(
        &mut self,
        task_id: &str,
        snapshot: &ProgressSnapshot,
        now_ms: u64,
    ) -> Result<bool, ExecutionError> {
        if self.progress_renewal_count(task_id) >= BETA_MAX_PROGRESS_RENEWALS_PER_TASK {
            return Ok(false);
        }
        if matches!(
            self.watchdog_state,
            WatchdogState::RepeatedCommand
                | WatchdogState::OscillationDetected
                | WatchdogState::NoProgress
                | WatchdogState::DriftDetected
                | WatchdogState::ExternalModification
                | WatchdogState::SafeBoundary
        ) || !self.progress_supports_renewal(snapshot)
        {
            return Ok(false);
        }

        let used = self.task_usage(task_id)?;
        let task_snapshot = self
            .tasks
            .get(task_id)
            .cloned()
            .ok_or_else(|| ExecutionError::UnknownTask(task_id.into()))?;
        let fresh_wall = beta_task_wall_clock_budget_ms(
            task_snapshot.priority,
            task_snapshot.requirement_ids.len(),
            task_snapshot.dependency_ids.len(),
            task_snapshot.evidence_obligations.len(),
            task_snapshot.objective.len(),
            self.policy.execution_time_policy.adapter_safety_timeout_ms,
        );
        let default_steps = self.policy.default_budget.execution_steps;
        let default_tools = self.policy.default_budget.tool_calls;
        let task = self
            .tasks
            .get_mut(task_id)
            .ok_or_else(|| ExecutionError::UnknownTask(task_id.into()))?;
        task.usage_budget.wall_clock_ms = used.wall_time_ms.saturating_add(fresh_wall);
        task.usage_budget.execution_steps = used.execution_steps.saturating_add(default_steps);
        task.usage_budget.tool_calls = used.tool_calls.saturating_add(default_tools);
        task.usage_budget.retry_attempts = task.attempt_number.saturating_add(1);
        task.state = ExecutionTaskState::WaitingRetry;

        for lease in &mut self.leases {
            if lease.task_id == task_id && lease.status == LeaseStatus::Active {
                lease.revoke()?;
            }
        }
        self.state = ExecutionRunState::Ready;
        self.watchdog_state = WatchdogState::Healthy;
        self.last_error = Some(
            "bounded task authority expired after measurable progress; one fresh lease was authorized"
                .into(),
        );
        self.emit(
            now_ms,
            Some(task_id.into()),
            ExecutionEventKind::ProgressRenewalAuthorized,
            "measurable progress justified one bounded fresh execution lease",
        )?;
        Ok(true)
    }

    /// Return the only identities that P8 may use to attribute evidence.
    /// Failed, stopped, late, superseded, or structurally incomplete attempts
    /// are intentionally absent.
    pub fn successful_execution_identities(
        &self,
    ) -> Result<Vec<SuccessfulExecutionIdentity>, ExecutionError> {
        let mut identities = Vec::new();
        for task in self.tasks.values() {
            if task.state != ExecutionTaskState::FinishedAwaitingVerification {
                continue;
            }
            let attempt = self
                .attempts
                .iter()
                .rev()
                .find(|attempt| {
                    attempt.task_id == task.task_id
                        && attempt.state == TaskAttemptState::Succeeded
                        && attempt.ended_at_ms.is_some()
                })
                .ok_or_else(|| ExecutionError::AttemptMissing(task.task_id.clone()))?;
            let completion = attempt.completion_authority.as_ref().ok_or_else(|| {
                ExecutionError::PolicyDenied(
                    "successful attempt has no adapter completion authority".into(),
                )
            })?;
            let lease = self
                .leases
                .iter()
                .find(|lease| lease.lease_id == attempt.lease_id)
                .ok_or(ExecutionError::LeaseInactive)?;
            if lease.status != LeaseStatus::Consumed
                || lease.mission_id != self.mission_id
                || lease.mission_revision != self.mission_revision
                || lease.task_id != task.task_id
                || lease.task_packet_digest != attempt.packet_digest
                || lease.attempt_number != attempt.attempt_number
                || lease.compute_digest()? != lease.lease_digest
                || completion.task_id != task.task_id
                || completion.attempt_id != attempt.attempt_id
                || completion.packet_digest != attempt.packet_digest
                || completion.lease_id != attempt.lease_id
                || completion.ended_at_ms != attempt.ended_at_ms.unwrap_or_default()
                || completion.ended_at_ms >= lease.expires_at_ms
                || attempt.workspace_before.is_none()
                || attempt.workspace_after.is_none()
            {
                return Err(ExecutionError::PolicyDenied(
                    "successful attempt evidence authority is incomplete or inconsistent".into(),
                ));
            }
            let workspace_before = attempt.workspace_before.as_ref().expect("checked above");
            let workspace_after = attempt.workspace_after.as_ref().expect("checked above");
            let changes = workspace_changes(workspace_before, workspace_after);
            identities.push(SuccessfulExecutionIdentity {
                mission_id: self.mission_id.clone(),
                mission_revision: self.mission_revision,
                seal_hash: self.seal_hash.clone(),
                run_id: self.run_id.clone(),
                task_id: task.task_id.clone(),
                attempt_id: attempt.attempt_id.clone(),
                attempt_number: attempt.attempt_number,
                packet_digest: attempt.packet_digest.clone(),
                lease_id: attempt.lease_id.clone(),
                lease_digest: lease.lease_digest.clone(),
                process_digest: completion.process_digest.clone(),
                workspace_identity: canonical_hash(&self.workspace)?,
                workspace_before_fingerprint: inventory_fingerprint(workspace_before)?,
                workspace_after_fingerprint: inventory_fingerprint(workspace_after)?,
                artifact_changes: changes,
                started_at_ms: completion.started_at_ms,
                ended_at_ms: completion.ended_at_ms,
                lease_expires_at_ms: lease.expires_at_ms,
            });
        }
        identities.sort_by(|left, right| left.task_id.cmp(&right.task_id));
        Ok(identities)
    }

    pub fn retry_decision(
        &self,
        task_id: &str,
        failure: FailureClass,
    ) -> Result<RetryDecision, ExecutionError> {
        if failure == FailureClass::AuthorityStale {
            return Ok(RetryDecision::RevalidationRequired);
        }
        if matches!(
            failure,
            FailureClass::PolicyDenied | FailureClass::Deterministic
        ) {
            return Ok(RetryDecision::Stop);
        }
        let task = self
            .tasks
            .get(task_id)
            .ok_or_else(|| ExecutionError::UnknownTask(task_id.into()))?;
        let attempts = self
            .attempts
            .iter()
            .filter(|attempt| attempt.task_id == task_id)
            .count() as u32;
        if attempts < task.retry_policy.max_attempts
            && task.retry_policy.retryable.contains(&failure)
        {
            Ok(RetryDecision::Retry)
        } else {
            Ok(RetryDecision::Stop)
        }
    }

    pub fn record_workspace_observation(
        &mut self,
        snapshot: ProgressSnapshot,
        now_ms: u64,
    ) -> Result<(), ExecutionError> {
        let fingerprint = snapshot.progress_fingerprint()?;
        if self.progress.last().is_some_and(|previous| {
            previous.progress_fingerprint().ok().as_deref() == Some(&fingerprint)
        }) {
            self.no_progress_occurrences = self.no_progress_occurrences.saturating_add(1);
            let already_decisive = matches!(
                self.watchdog_state,
                WatchdogState::RepeatedCommand
                    | WatchdogState::OscillationDetected
                    | WatchdogState::BudgetExhausted
                    | WatchdogState::DriftDetected
                    | WatchdogState::ExternalModification
                    | WatchdogState::SafeBoundary
            );
            if !already_decisive {
                self.watchdog_state = WatchdogState::NoProgress;
            }
            self.emit(
                now_ms,
                None,
                ExecutionEventKind::NoProgressSignal,
                "identical measurable progress snapshot",
            )?;
            if self.no_progress_occurrences >= self.policy.no_progress_threshold {
                self.state = ExecutionRunState::BlockedExternal;
            }
        } else {
            self.progress.push(snapshot);
            self.no_progress_occurrences = 0;
            self.watchdog_state = WatchdogState::Healthy;
        }
        Ok(())
    }

    pub fn record_oscillation(
        &mut self,
        before_fingerprint: &str,
        after_fingerprint: &str,
        changed_paths: Vec<String>,
        now_ms: u64,
    ) -> Result<bool, ExecutionError> {
        let snapshots = vec![before_fingerprint.into(), after_fingerprint.into()];
        let mut canonical_paths = changed_paths.clone();
        canonical_paths.sort();
        let fingerprint = canonical_hash(&(&snapshots, &canonical_paths))?;
        if let Some(existing) = self
            .oscillation_signals
            .iter_mut()
            .find(|signal| signal.fingerprint == fingerprint)
        {
            existing.occurrences = existing.occurrences.saturating_add(1);
        } else {
            self.oscillation_signals.push(OscillationFingerprint {
                fingerprint,
                snapshots,
                changed_paths,
                occurrences: 1,
            });
        }
        let reverse_exists = self.oscillation_signals.iter().any(|signal| {
            signal.snapshots == vec![after_fingerprint.to_owned(), before_fingerprint.to_owned()]
                && signal.changed_paths == canonical_paths
        });
        let reciprocal_occurrences = self
            .oscillation_signals
            .iter()
            .filter(|signal| {
                signal.changed_paths == canonical_paths
                    && ((signal.snapshots
                        == vec![before_fingerprint.to_owned(), after_fingerprint.to_owned()])
                        || (signal.snapshots
                            == vec![after_fingerprint.to_owned(), before_fingerprint.to_owned()]))
            })
            .map(|signal| signal.occurrences)
            .sum::<u32>();
        if reverse_exists && reciprocal_occurrences >= self.policy.oscillation_threshold {
            self.watchdog_state = WatchdogState::OscillationDetected;
            for lease in &mut self.leases {
                if lease.status == LeaseStatus::Active {
                    lease.revoke()?;
                }
            }
            self.state = ExecutionRunState::SafeBoundaryReached;
            self.emit(
                now_ms,
                None,
                ExecutionEventKind::LoopSignal,
                "edit-revert oscillation detected; mutable leases revoked",
            )?;
            return Ok(true);
        }
        Ok(false)
    }

    pub fn inject_diagnostic_task(
        &mut self,
        blocked_task_id: &str,
        reason: &str,
        now_ms: u64,
    ) -> Result<String, ExecutionError> {
        if !self.tasks.contains_key(blocked_task_id) {
            return Err(ExecutionError::UnknownTask(blocked_task_id.into()));
        }
        let diagnostic_id = canonical_hash(&(
            &self.run_id,
            blocked_task_id,
            reason,
            self.diagnostics.len(),
        ))?;
        self.diagnostics.push(DiagnosticTask {
            diagnostic_id: diagnostic_id.clone(),
            blocked_task_id: blocked_task_id.into(),
            provenance: "SYSTEM_DIAGNOSTIC".into(),
            objective: reason.into(),
            budget: UsageBudget {
                execution_steps: 5,
                tool_calls: 3,
                wall_clock_ms: 10_000,
                retry_attempts: 0,
                cost_micros: None,
            },
            lease_id: None,
            state: ExecutionTaskState::Ready,
        });
        self.emit(
            now_ms,
            Some(blocked_task_id.into()),
            ExecutionEventKind::DiagnosticTaskInjected,
            "bounded-system-diagnostic-created",
        )?;
        Ok(diagnostic_id)
    }

    pub fn request_stop(
        &mut self,
        task_id: Option<&str>,
        reason: &str,
        now_ms: u64,
    ) -> Result<(), ExecutionError> {
        self.safe_boundary = Some(SafeBoundary {
            requested_at_ms: now_ms,
            timeout_ms: self.policy.safe_boundary_timeout_ms,
            atomic_action_allowed: true,
            reason: reason.into(),
            reached: false,
        });
        let mut revoked_tasks = Vec::new();
        for lease in &mut self.leases {
            if task_id.is_none_or(|id| id == lease.task_id) && lease.status == LeaseStatus::Active {
                revoked_tasks.push(lease.task_id.clone());
                lease.revoke()?;
            }
        }
        for revoked_task in revoked_tasks {
            self.emit(
                now_ms,
                Some(revoked_task),
                ExecutionEventKind::LeaseRevoked,
                "lease-revoked-at-safe-boundary",
            )?;
        }
        if let Some(task_id) = task_id {
            if let Some(task) = self.tasks.get_mut(task_id) {
                task.state = ExecutionTaskState::Stopped;
            }
            if let Ok(attempt) = self.current_attempt_mut(task_id) {
                attempt.state = TaskAttemptState::SafeBoundaryStopped;
                attempt.ended_at_ms = Some(now_ms);
                attempt.termination_reason = Some(reason.into());
            }
        }
        self.state = ExecutionRunState::SafeBoundaryReached;
        self.watchdog_state = WatchdogState::SafeBoundary;
        self.emit(
            now_ms,
            task_id.map(str::to_owned),
            ExecutionEventKind::SafeBoundaryStop,
            reason,
        )?;
        Ok(())
    }

    pub fn reach_safe_boundary(&mut self, now_ms: u64) -> Result<(), ExecutionError> {
        let Some(boundary) = self.safe_boundary.as_mut() else {
            return Err(ExecutionError::SafeBoundaryRequired);
        };
        if now_ms > boundary.requested_at_ms.saturating_add(boundary.timeout_ms) {
            boundary.atomic_action_allowed = false;
        }
        boundary.reached = true;
        self.state = ExecutionRunState::Stopped;
        Ok(())
    }

    /// P9 owns the durable recovery meaning of an emergency stop.  The P7
    /// safe-boundary state remains available for its existing pause semantics;
    /// this explicit transition prevents a stopped run with remaining work
    /// from looking like completion.
    pub fn mark_stopped_incomplete(
        &mut self,
        reason: &str,
        now_ms: u64,
    ) -> Result<(), ExecutionError> {
        for task in self.tasks.values_mut() {
            if task.state == ExecutionTaskState::Running {
                task.state = ExecutionTaskState::Pending;
            }
        }
        for attempt in self
            .attempts
            .iter_mut()
            .filter(|attempt| attempt.state == TaskAttemptState::Running)
        {
            attempt.state = TaskAttemptState::SafeBoundaryStopped;
            attempt.ended_at_ms = Some(now_ms);
            attempt.termination_reason = Some(reason.into());
        }
        let revoked_tasks = self
            .leases
            .iter()
            .filter(|lease| lease.status == LeaseStatus::Active)
            .map(|lease| lease.task_id.clone())
            .collect::<Vec<_>>();
        for lease in &mut self.leases {
            if lease.status == LeaseStatus::Active {
                lease.revoke()?;
            }
        }
        for task_id in revoked_tasks {
            self.emit(
                now_ms,
                Some(task_id),
                ExecutionEventKind::LeaseRevoked,
                "lease-revoked-during-restart-reconciliation",
            )?;
        }
        if self
            .tasks
            .values()
            .all(|task| task.state == ExecutionTaskState::FinishedAwaitingVerification)
        {
            self.state = ExecutionRunState::Stopped;
        } else {
            self.state = ExecutionRunState::StoppedIncomplete;
        }
        self.last_error = Some(reason.into());
        self.emit(
            now_ms,
            None,
            ExecutionEventKind::SafeBoundaryStop,
            format!("STOPPED_INCOMPLETE: {reason}"),
        )?;
        Ok(())
    }

    pub fn resume_from_recovery(
        &mut self,
        disposition: RecoveryDisposition,
        now_ms: u64,
    ) -> Result<(), ExecutionError> {
        if !matches!(
            disposition,
            RecoveryDisposition::SafeToResume
                | RecoveryDisposition::SafeToResumeAfterProcessReconciliation
                | RecoveryDisposition::StoppedIncomplete
                | RecoveryDisposition::PreExecutionRetryAuthorized
        ) {
            return Err(ExecutionError::RevalidationRequired(
                "recovery integrity did not authorize resume".into(),
            ));
        }
        if self.state == ExecutionRunState::StoppedIncomplete {
            for task in self.tasks.values_mut() {
                if task.state == ExecutionTaskState::Stopped {
                    task.state = ExecutionTaskState::Pending;
                }
            }
        }
        if disposition == RecoveryDisposition::PreExecutionRetryAuthorized {
            let task_id = self.pre_execution_retry_task().ok_or_else(|| {
                ExecutionError::RevalidationRequired(
                    "recovery could not prove the prior attempt stopped before external execution"
                        .into(),
                )
            })?;
            if let Some(task) = self.tasks.get_mut(&task_id) {
                task.state = ExecutionTaskState::WaitingRetry;
            }
            self.emit(
                now_ms,
                Some(task_id.clone()),
                ExecutionEventKind::RecoveryRevalidated,
                "pre-execution attempt had no external side-effect evidence",
            )?;
            self.emit(
                now_ms,
                Some(task_id),
                ExecutionEventKind::RetryAuthorized,
                "retry authorized for the original task",
            )?;
        }
        self.safe_boundary = None;
        self.state = ExecutionRunState::Ready;
        if disposition != RecoveryDisposition::PreExecutionRetryAuthorized {
            let already_authorized = self.events.last().is_some_and(|event| {
                event.kind == ExecutionEventKind::ContinuationAuthorized
                    && event.detail == "P9 resume integrity authorized execution continuation"
            });
            if !already_authorized {
                self.emit(
                    now_ms,
                    None,
                    ExecutionEventKind::ContinuationAuthorized,
                    "P9 resume integrity authorized execution continuation",
                )?;
            }
        }
        Ok(())
    }

    /// Authorize a retry only after P9 has revalidated the exact interrupted
    /// external attempt and a user has explicitly reviewed the resulting
    /// workspace changes. This is deliberately separate from automatic
    /// resume: the stopped attempt remains part of the authenticated history,
    /// while the task becomes runnable for a fresh attempt.
    pub fn authorize_manual_recovery_retry(
        &mut self,
        target: &RecoveryAttemptTarget,
        now_ms: u64,
    ) -> Result<(), ExecutionError> {
        if self.state != ExecutionRunState::RevalidationRequired {
            return Err(ExecutionError::RevalidationRequired(
                "manual recovery retry requires an explicit revalidation state".into(),
            ));
        }
        if target.execution_boundary != AttemptExecutionBoundary::ExternalProcessStarted {
            return Err(ExecutionError::RevalidationRequired(
                "manual recovery retry is only available for an interrupted external attempt"
                    .into(),
            ));
        }
        let attempt = self
            .attempts
            .iter_mut()
            .find(|attempt| attempt.attempt_id == target.attempt_id)
            .ok_or_else(|| ExecutionError::AttemptMissing(target.attempt_id.clone()))?;
        if attempt.task_id != target.task_id
            || attempt.lease_id != target.lease_id
            || attempt.execution_boundary != target.execution_boundary
            || attempt.completion_authority.is_some()
            || attempt.ended_at_ms.is_none()
            || !matches!(
                attempt.state,
                TaskAttemptState::Failed
                    | TaskAttemptState::TurnEndedIncomplete
                    | TaskAttemptState::SafeBoundaryStopped
            )
        {
            return Err(ExecutionError::RevalidationRequired(
                "the selected recovery attempt is no longer an eligible interrupted attempt".into(),
            ));
        }
        if self
            .leases
            .iter()
            .any(|lease| lease.lease_id == target.lease_id && lease.status == LeaseStatus::Active)
        {
            return Err(ExecutionError::RevalidationRequired(
                "the interrupted attempt still has an active lease".into(),
            ));
        }
        attempt.state = TaskAttemptState::WaitingRetry;
        let task = self
            .tasks
            .get_mut(&target.task_id)
            .ok_or_else(|| ExecutionError::UnknownTask(target.task_id.clone()))?;
        if task.state == ExecutionTaskState::FinishedAwaitingVerification {
            return Err(ExecutionError::PolicyDenied(
                "a completed task cannot be retried".into(),
            ));
        }
        task.state = ExecutionTaskState::WaitingRetry;
        self.safe_boundary = None;
        self.last_error = None;
        self.watchdog_state = WatchdogState::Healthy;
        self.state = ExecutionRunState::Ready;
        self.emit(
            now_ms,
            Some(target.task_id.clone()),
            ExecutionEventKind::RecoveryRevalidated,
            "explicit recovery review authorized a fresh attempt for the interrupted task",
        )?;
        self.emit(
            now_ms,
            Some(target.task_id.clone()),
            ExecutionEventKind::RetryAuthorized,
            "manual retry authorized for the exact interrupted task",
        )?;
        Ok(())
    }

    pub fn agent_stopped(
        &mut self,
        task_id: &str,
        summary: &str,
        now_ms: u64,
    ) -> Result<(), ExecutionError> {
        let task = self
            .tasks
            .get_mut(task_id)
            .ok_or_else(|| ExecutionError::UnknownTask(task_id.into()))?;
        task.state = ExecutionTaskState::Pending;
        let attempt = self.current_attempt_mut(task_id)?;
        attempt.state = TaskAttemptState::TurnEndedIncomplete;
        attempt.ended_at_ms = Some(now_ms);
        attempt.termination_reason = Some(summary.into());
        let remaining_dependencies = self
            .tasks
            .iter()
            .filter(|(_, task)| task.state != ExecutionTaskState::FinishedAwaitingVerification)
            .map(|(id, task)| (id.clone(), task.dependency_ids.clone()))
            .collect();
        for lease in &mut self.leases {
            if lease.task_id == task_id && lease.status == LeaseStatus::Active {
                lease.revoke()?;
            }
        }
        let continuation_id = canonical_hash(&(&self.run_id, self.current_turn, task_id, summary))?;
        let budget_remaining = self
            .remaining_budget(task_id)
            .unwrap_or_else(|_| self.policy.default_budget.clone());
        self.continuations.push(ContinuationRecord {
            continuation_id,
            mission_id: self.mission_id.clone(),
            mission_revision: self.mission_revision,
            seal_hash: self.seal_hash.clone(),
            current_task: Some(task_id.into()),
            completed_tasks: self.completed_tasks(),
            remaining_dependencies,
            previous_attempt_summary: summary.into(),
            workspace_fingerprint: self.workspace_fingerprint.clone(),
            failure_summary: vec![summary.into()],
            budget_remaining,
            attempt_number: self
                .attempts
                .iter()
                .filter(|attempt| attempt.task_id == task_id)
                .count() as u32,
            loop_history: self.loop_signals.clone(),
        });
        self.state = ExecutionRunState::TurnEndedIncomplete;
        self.emit(
            now_ms,
            Some(task_id.into()),
            ExecutionEventKind::TurnEndedIncomplete,
            summary,
        )?;
        Ok(())
    }

    pub fn start_next_turn(&mut self, now_ms: u64) -> Result<(), ExecutionError> {
        if self.state != ExecutionRunState::TurnEndedIncomplete {
            return Err(ExecutionError::ContinuationNotAvailable);
        }
        let record = self
            .continuations
            .last()
            .ok_or(ExecutionError::ContinuationNotAvailable)?;
        if record.mission_id != self.mission_id
            || record.mission_revision != self.mission_revision
            || record.seal_hash != self.seal_hash
        {
            return Err(ExecutionError::RevalidationRequired(
                "continuation authority changed".into(),
            ));
        }
        self.current_turn = self.current_turn.saturating_add(1);
        self.state = ExecutionRunState::Ready;
        self.emit(
            now_ms,
            record.current_task.clone(),
            ExecutionEventKind::ContinuationStarted,
            "clean-context-continuation-started",
        )?;
        Ok(())
    }

    /// Authorize the next controlled task turn after a clean terminal task
    /// boundary.  This is deliberately separate from `start_task`: the
    /// scheduler must prove that the prior attempt completed, its lease was
    /// consumed, the sealed identity is unchanged, and no recovery/stop
    /// condition is active before issuing another bounded attempt.
    pub fn authorize_next_task_continuation(
        &mut self,
        revision: &MissionRevision,
        handoff: &ExecutionHandoff,
        now_ms: u64,
    ) -> Result<Option<String>, ExecutionError> {
        self.validate_authority_identity(revision, handoff, now_ms)?;
        if self.state != ExecutionRunState::Ready {
            return Err(ExecutionError::ContinuationNotAvailable);
        }
        if self
            .attempts
            .iter()
            .any(|attempt| attempt.state == TaskAttemptState::Running)
            || self
                .leases
                .iter()
                .any(|lease| lease.status == LeaseStatus::Active)
        {
            return Err(ExecutionError::PolicyDenied(
                "continuation requires a terminal attempt and no active lease".into(),
            ));
        }
        let previous_task = self.events.last().and_then(|event| {
            (event.kind == ExecutionEventKind::TaskImplementationFinished)
                .then(|| event.task_id.clone())
                .flatten()
        });
        if previous_task.is_none() {
            return Err(ExecutionError::ContinuationNotAvailable);
        }
        let Some(next_task) = self.runnable_tasks().into_iter().next() else {
            self.update_run_state();
            return Ok(None);
        };
        self.current_turn = self.current_turn.saturating_add(1);
        self.emit(
            now_ms,
            Some(next_task.clone()),
            ExecutionEventKind::ContinuationStarted,
            "automatic continuation reached a clean task boundary",
        )?;
        self.emit(
            now_ms,
            Some(next_task.clone()),
            ExecutionEventKind::ContinuationAuthorized,
            "sealed identity, evidence boundary, dependencies, and executor lease state revalidated",
        )?;
        Ok(Some(next_task))
    }

    pub fn detect_external_modification(
        &mut self,
        before_fingerprint: &str,
        after_fingerprint: &str,
        expected_paths: &[String],
        changed_paths: Vec<String>,
        now_ms: u64,
    ) -> Result<(), ExecutionError> {
        let unexpected = changed_paths
            .iter()
            .filter(|path| {
                !expected_paths
                    .iter()
                    .any(|expected| safe_path_within(Path::new(path), Path::new(expected)))
            })
            .cloned()
            .collect::<Vec<_>>();
        if unexpected.is_empty() && before_fingerprint == after_fingerprint {
            return Ok(());
        }
        let classification = if unexpected
            .iter()
            .any(|path| path.contains("mission") || path.contains("seal"))
        {
            "AUTHORITY_OR_SEAL_EXTERNAL_MODIFICATION"
        } else if unexpected.is_empty() {
            "EXPECTED_LEASED_MODIFICATION"
        } else {
            "EXTERNAL_MODIFICATION_DETECTED"
        };
        self.external_modifications
            .push(ExternalModificationRecord {
                detected_at_ms: now_ms,
                expected_paths: expected_paths.to_vec(),
                changed_paths: changed_paths.clone(),
                classification: classification.into(),
                before_fingerprint: before_fingerprint.into(),
                after_fingerprint: after_fingerprint.into(),
            });
        if classification != "EXPECTED_LEASED_MODIFICATION" {
            for lease in &mut self.leases {
                if lease.status == LeaseStatus::Active {
                    lease.revoke()?;
                }
            }
            self.watchdog_state = WatchdogState::ExternalModification;
            self.state = if classification.contains("AUTHORITY") {
                ExecutionRunState::RevalidationRequired
            } else {
                ExecutionRunState::BlockedExternal
            };
            self.emit(
                now_ms,
                None,
                ExecutionEventKind::ExternalModification,
                classification,
            )?;
        }
        Ok(())
    }

    pub fn validate_authority_identity(
        &mut self,
        revision: &MissionRevision,
        handoff: &ExecutionHandoff,
        now_ms: u64,
    ) -> Result<(), ExecutionError> {
        if let Err(error) = validate_sealed_task_requirement_ids(revision) {
            self.state = ExecutionRunState::RevalidationRequired;
            self.emit(
                now_ms,
                None,
                ExecutionEventKind::AuthorityRevalidationRequired,
                "sealed task requirement identity is not executable",
            )?;
            return Err(error);
        }
        if revision.seal.state != "SEALED"
            || revision.seal.contract_hash != self.seal_hash
            || handoff.contract_hash != self.seal_hash
            || handoff.mission_id != self.mission_id
            || handoff.revision != self.mission_revision
        {
            self.state = ExecutionRunState::RevalidationRequired;
            self.emit(
                now_ms,
                None,
                ExecutionEventKind::AuthorityRevalidationRequired,
                "P6 authority identity changed",
            )?;
            return Err(ExecutionError::RevalidationRequired(
                "P6 seal or handoff identity changed".into(),
            ));
        }
        for task in &revision.contract.task_graph.tasks {
            let Some(runtime_task) = self.tasks.get(&task.task_id) else {
                self.state = ExecutionRunState::RevalidationRequired;
                return Err(ExecutionError::RevalidationRequired(format!(
                    "sealed task {} is missing from the execution ledger",
                    task.task_id
                )));
            };
            if runtime_task.objective != task.objective
                || runtime_task.requirement_ids != task.requirement_ids
                || runtime_task.dependency_ids != task.dependency_ids
            {
                self.state = ExecutionRunState::RevalidationRequired;
                return Err(ExecutionError::RevalidationRequired(format!(
                    "sealed task {} changed between mission authority and execution ledger",
                    task.task_id
                )));
            }
        }
        Ok(())
    }

    pub fn dispatch_with_adapter<A: AntigravityAdapter>(
        &mut self,
        adapter: &mut A,
        task_id: &str,
        packet: &TaskPacket,
        root: &Path,
        now_ms: u64,
    ) -> Result<ProcessResult, ExecutionError> {
        self.dispatch_with_adapter_and_started_callback(
            adapter,
            task_id,
            packet,
            root,
            now_ms,
            |_, _| Ok(()),
        )
    }

    fn dispatch_with_adapter_and_started_callback<
        A: AntigravityAdapter,
        F: FnMut(&ExecutionRun, &ProcessIdentity) -> Result<(), ExecutionError>,
    >(
        &mut self,
        adapter: &mut A,
        task_id: &str,
        packet: &TaskPacket,
        root: &Path,
        now_ms: u64,
        mut on_executor_started: F,
    ) -> Result<ProcessResult, ExecutionError> {
        packet.verify_digest()?;
        let report = adapter.detect();
        adapter
            .validate_version(&report)
            .map_err(ExecutionError::Adapter)?;
        let bridge_packet = packet.to_bridge_packet();
        let session = adapter
            .create_execution(bridge_packet.clone(), root)
            .map_err(ExecutionError::Adapter)?;
        adapter
            .send_task(&session.execution_id, &bridge_packet)
            .map_err(ExecutionError::Adapter)?;
        let process_identity = adapter
            .process_identity(&session.execution_id)
            .map_err(ExecutionError::Adapter)?;
        self.mark_external_process_started(task_id)?;
        on_executor_started(self, &process_identity)?;
        let (_effective_deadline_ms, timeout, idle_timeout) =
            self.effective_execution_deadline(task_id, packet.lease_id.as_str())?;
        let result = adapter
            .wait_for_exit_with_timeouts(&session.execution_id, timeout, idle_timeout)
            .map_err(ExecutionError::Adapter)?;
        adapter
            .reconcile_exit(&session.execution_id, result.clone())
            .map_err(ExecutionError::Adapter)?;
        let events = adapter
            .stream_events(&session.execution_id)
            .map_err(ExecutionError::Adapter)?;
        if events
            .iter()
            .any(|event| event.event_type == EventType::StopRequested)
        {
            self.agent_stopped(task_id, "adapter stop event", now_ms)?;
            return Err(ExecutionError::SafeBoundaryRequired);
        }
        let artifact_paths = adapter
            .collect_artifacts(&session.execution_id)
            .map_err(ExecutionError::Adapter)?;
        let mut result = result;
        result.artifact_paths = artifact_paths;
        Ok(result)
    }

    /// Derive the one deadline that both Relintor and the production adapter
    /// use. Setup/spawn time is included because this is evaluated immediately
    /// before supervision begins, and the lease remains the hard authority.
    fn effective_execution_deadline(
        &self,
        task_id: &str,
        lease_id: &str,
    ) -> Result<(u64, Duration, Duration), ExecutionError> {
        let lease = self
            .leases
            .iter()
            .find(|lease| lease.lease_id == lease_id)
            .ok_or(ExecutionError::LeaseInactive)?;
        if lease.task_id != task_id {
            return Err(ExecutionError::LeaseBindingMismatch);
        }
        let attempt_started_at_ms = self
            .attempts
            .iter()
            .rev()
            .find(|attempt| attempt.task_id == task_id && attempt.lease_id == lease_id)
            .map(|attempt| attempt.started_at_ms)
            .ok_or_else(|| ExecutionError::AttemptMissing(task_id.into()))?;
        let now_ms = execution_now_ms();
        let deadline_ms = effective_execution_deadline_ms(
            lease.expires_at_ms,
            attempt_started_at_ms,
            lease.usage_budget.wall_clock_ms,
            now_ms,
            self.policy
                .execution_time_policy
                .adapter_safety_timeout_ms
                .min(relintor_antigravity::PRODUCTION_PROCESS_TIMEOUT.as_millis() as u64),
        );
        let remaining_ms = deadline_ms.saturating_sub(now_ms);
        let timeout = Duration::from_millis(remaining_ms);
        let idle_timeout = relintor_antigravity::PRODUCTION_IDLE_TIMEOUT.min(timeout);
        Ok((deadline_ms, timeout, idle_timeout))
    }

    /// Execute one runnable task through a caller-supplied, validated
    /// Antigravity adapter. The scheduler owns the lease, action accounting,
    /// completion transition, and failure transition; an adapter cannot mark
    /// a task complete by returning a fabricated status.
    pub fn execute_next_with_adapter<A: AntigravityAdapter>(
        &mut self,
        adapter: &mut A,
        revision: &MissionRevision,
        handoff: &ExecutionHandoff,
        root: &Path,
        now_ms: u64,
    ) -> Result<RetryDecision, ExecutionError> {
        self.execute_next_with_adapter_with_callbacks(
            adapter,
            revision,
            handoff,
            root,
            now_ms,
            (|_, _| Ok(()), |_| Ok(())),
        )
    }

    pub fn execute_next_with_adapter_with_started_callback<
        A: AntigravityAdapter,
        F: FnMut(&ExecutionRun, &ProcessIdentity) -> Result<(), ExecutionError>,
    >(
        &mut self,
        adapter: &mut A,
        revision: &MissionRevision,
        handoff: &ExecutionHandoff,
        root: &Path,
        now_ms: u64,
        on_attempt_started: F,
    ) -> Result<RetryDecision, ExecutionError> {
        self.execute_next_with_adapter_with_callbacks(
            adapter,
            revision,
            handoff,
            root,
            now_ms,
            (on_attempt_started, |_| Ok(())),
        )
    }

    pub fn execute_next_with_adapter_with_callbacks<
        A: AntigravityAdapter,
        F: FnMut(&ExecutionRun, &ProcessIdentity) -> Result<(), ExecutionError>,
        G: FnMut(&ExecutionRun) -> Result<(), ExecutionError>,
    >(
        &mut self,
        adapter: &mut A,
        revision: &MissionRevision,
        handoff: &ExecutionHandoff,
        root: &Path,
        now_ms: u64,
        callbacks: (F, G),
    ) -> Result<RetryDecision, ExecutionError> {
        let (mut on_attempt_started, mut on_completion_recorded) = callbacks;
        self.validate_authority_identity(revision, handoff, now_ms)?;
        let task_id = self
            .runnable_tasks()
            .into_iter()
            .next()
            .ok_or_else(|| ExecutionError::DependencyNotReady("no runnable task".into()))?;
        let packet = self.start_task(&task_id, now_ms)?;
        let action = ActionRequest {
            tool: "antigravity".into(),
            operation: "execute_task".into(),
            arguments: vec![task_id.clone()],
            working_scope: root.display().to_string(),
            environment_identity: "rust-owned-antigravity-dispatch".into(),
            mutable: true,
            paths: vec![root.display().to_string()],
            external_authority: None,
        };
        let authorized = &packet.authorized_action;
        if action.tool != authorized.tool
            || action.operation != authorized.operation
            || action.arguments != authorized.arguments
            || action.paths != authorized.paths
            || action.working_scope != authorized.working_scope
        {
            return Err(ExecutionError::PolicyDenied(
                "dispatch action is not the exact P7-authorized operation".into(),
            ));
        }
        self.authorize_action(&task_id, &packet.lease_id, &packet, &action, now_ms)?;
        let result = match self.dispatch_with_adapter_and_started_callback(
            adapter,
            &task_id,
            &packet,
            root,
            now_ms,
            |snapshot, identity| on_attempt_started(snapshot, identity),
        ) {
            Ok(result) => result,
            Err(error @ ExecutionError::SafeBoundaryRequired) => return Err(error),
            Err(error) => {
                self.fail_adapter_attempt(&task_id, &error.to_string(), now_ms)?;
                return Err(error);
            }
        };
        let ended_at_ms = u64::try_from(result.ended_at_ms).unwrap_or(u64::MAX);
        if result.state == ProcessState::Cancelled {
            self.mark_stopped_incomplete("execution cancelled by user", ended_at_ms)?;
            return Ok(RetryDecision::Stop);
        }
        let started_at_ms = u64::try_from(result.started_at_ms).unwrap_or(now_ms);
        let wall_time_ms = ended_at_ms.saturating_sub(started_at_ms);
        let lease_expired = self
            .leases
            .iter()
            .find(|lease| lease.lease_id == packet.lease_id)
            .is_some_and(|lease| ended_at_ms >= lease.expires_at_ms);
        let wall_budget_exceeded = self
            .tasks
            .get(&task_id)
            .zip(self.task_usage(&task_id).ok())
            .is_some_and(|(task, used)| {
                used.wall_time_ms.saturating_add(wall_time_ms) > task.usage_budget.wall_clock_ms
            });
        let process_succeeded = result.state == ProcessState::ExitedSuccess
            && result.exit_code == Some(0)
            && !result.forced
            && !result.output_overflow;
        let success = process_succeeded && !lease_expired && !wall_budget_exceeded;
        let timeout_or_budget =
            lease_expired || wall_budget_exceeded || result.state == ProcessState::TimedOut;
        let workspace_before_fingerprint = self.workspace_fingerprint.clone();
        let workspace_after = workspace_inventory(&self.workspace)?;
        let workspace_before = self
            .attempts
            .iter()
            .rev()
            .find(|attempt| {
                attempt.task_id == task_id && attempt.state == TaskAttemptState::Running
            })
            .and_then(|attempt| attempt.workspace_before.as_ref());
        let changed_paths = workspace_before
            .map(|before| workspace_changes(before, &workspace_after))
            .unwrap_or_default()
            .into_iter()
            .map(|change| change.path)
            .collect::<Vec<_>>();
        let workspace_fingerprint = inventory_fingerprint(&workspace_after)?;
        let passing_test_count = observed_passing_test_count(&result.stdout, &result.stderr);
        let action_result = ActionResult {
            success,
            failure_class: (!success).then_some(if timeout_or_budget {
                FailureClass::Timeout
            } else if result.forced {
                FailureClass::AgentExit
            } else {
                FailureClass::ProcessFailure
            }),
            failure_message: (!success).then(|| {
                format!(
                    "adapter state={:?}, exit={:?}, forced={}, output_overflow={}, lease_expired={}, wall_budget_exceeded={}",
                    result.state,
                    result.exit_code,
                    result.forced,
                    result.output_overflow,
                    lease_expired,
                    wall_budget_exceeded
                )
            }),
            changed_paths,
            workspace_fingerprint,
            passing_tests: passing_test_count.unwrap_or_default(),
            useful_artifacts: result.artifact_paths.len() as u64,
            wall_time_ms,
            estimated_cost_micros: None,
        };
        let mut progress_snapshot = ProgressSnapshot {
            workspace_fingerprint: action_result.workspace_fingerprint.clone(),
            task_state_fingerprint: self.task_state_fingerprint()?,
            changed_paths: action_result.changed_paths.clone(),
            passing_tests: action_result.passing_tests,
            passing_tests_observed: passing_test_count.is_some(),
            useful_artifacts: action_result.useful_artifacts,
            artifacts_observed: true,
            diagnostic_information: observed_diagnostic_information(&result.stdout, &result.stderr),
            ..ProgressSnapshot::default()
        };
        // A process that crossed the authoritative deadline is not eligible
        // for an automatic retry. It may have changed the workspace, so the
        // exact attempt must enter recovery before any new lease is issued.
        let decision = self.record_action_internal(
            &task_id,
            &action,
            action_result,
            now_ms,
            !timeout_or_budget,
        )?;
        if success {
            self.record_adapter_completion(&task_id, &packet, &result)?;
            self.record_completion_workspace(&task_id, workspace_after.clone())?;
            // This callback is the crash-consistency boundary: the exact clean
            // process receipt and post-workspace inventory exist before the task
            // is promoted to finished. Desktop persistence occurs here.
            on_completion_recorded(self)?;
            self.promote_task_from_trusted_completion(&task_id, ended_at_ms, false)?;
            progress_snapshot.task_state_fingerprint = self.task_state_fingerprint()?;
            progress_snapshot.dependency_completions = self
                .tasks
                .values()
                .filter(|task| {
                    task.dependency_ids
                        .iter()
                        .any(|dependency| dependency == &task_id)
                })
                .count() as u64;
        } else if timeout_or_budget {
            if self.authorize_progress_renewal(&task_id, &progress_snapshot, ended_at_ms)? {
                // The expired attempt remains terminal history. The task may
                // continue only under a newly issued bounded lease.
            } else {
                self.watchdog_state = WatchdogState::BudgetExhausted;
                self.state = ExecutionRunState::RevalidationRequired;
                if let Some(task) = self.tasks.get_mut(&task_id) {
                    task.state = ExecutionTaskState::BlockedExternal;
                }
                self.last_error = Some(
                    "adapter execution exceeded lease or wall-clock budget without safe renewable progress"
                        .into(),
                );
                self.emit(
                    ended_at_ms,
                    Some(task_id.clone()),
                    ExecutionEventKind::BudgetWarning,
                    "adapter completion rejected after lease or wall-clock budget boundary",
                )?;
                if let Some(lease) = self
                    .leases
                    .iter_mut()
                    .find(|lease| lease.task_id == task_id && lease.status == LeaseStatus::Active)
                {
                    lease.revoke()?;
                }
            }
        } else if let Some(lease) = self
            .leases
            .iter_mut()
            .find(|lease| lease.task_id == task_id && lease.status == LeaseStatus::Active)
        {
            lease.revoke()?;
        }
        let workspace_returned_to_prior_state =
            self.progress.iter().rev().skip(1).any(|snapshot| {
                snapshot.workspace_fingerprint == progress_snapshot.workspace_fingerprint
            });
        if workspace_returned_to_prior_state {
            let _oscillation_detected = self.record_oscillation(
                &workspace_before_fingerprint,
                &progress_snapshot.workspace_fingerprint,
                progress_snapshot.changed_paths.clone(),
                ended_at_ms,
            )?;
        }
        self.workspace_fingerprint = progress_snapshot.workspace_fingerprint.clone();
        self.record_workspace_observation(progress_snapshot, ended_at_ms)?;
        Ok(decision)
    }

    fn record_adapter_completion(
        &mut self,
        task_id: &str,
        packet: &TaskPacket,
        result: &ProcessResult,
    ) -> Result<(), ExecutionError> {
        packet.verify_digest()?;
        if result.exit_code != Some(0) || result.forced || result.output_overflow {
            return Err(ExecutionError::PolicyDenied(
                "only a clean adapter-owned process result can authorize completion".into(),
            ));
        }
        let started_at_ms = u64::try_from(result.started_at_ms)
            .map_err(|_| ExecutionError::PolicyDenied("adapter start time is invalid".into()))?;
        let ended_at_ms = u64::try_from(result.ended_at_ms)
            .map_err(|_| ExecutionError::PolicyDenied("adapter end time is invalid".into()))?;
        if ended_at_ms < started_at_ms {
            return Err(ExecutionError::PolicyDenied(
                "adapter process timing is invalid".into(),
            ));
        }
        let attempt = self
            .attempts
            .iter()
            .rev()
            .find(|attempt| {
                attempt.task_id == task_id && attempt.state == TaskAttemptState::Running
            })
            .cloned()
            .ok_or_else(|| ExecutionError::AttemptMissing(task_id.into()))?;
        let lease = self
            .leases
            .iter()
            .find(|lease| lease.lease_id == attempt.lease_id)
            .ok_or(ExecutionError::LeaseInactive)?;
        lease.verify(packet, ended_at_ms)?;
        if lease.task_id != task_id || lease.task_packet_digest != packet.task_packet_digest {
            return Err(ExecutionError::LeaseBindingMismatch);
        }
        let process_digest = canonical_hash(result)?;
        let proposed = ExecutionCompletionAuthority {
            task_id: task_id.into(),
            attempt_id: attempt.attempt_id,
            packet_digest: packet.task_packet_digest.clone(),
            lease_id: lease.lease_id.clone(),
            process_digest,
            started_at_ms,
            ended_at_ms,
        };
        let current = self.current_attempt_mut(task_id)?;
        if let Some(existing) = current.completion_authority.as_ref() {
            if existing == &proposed {
                return Ok(());
            }
            return Err(ExecutionError::RevalidationRequired(
                "conflicting trusted executor completion receipt replay".into(),
            ));
        }
        current.completion_authority = Some(proposed);
        Ok(())
    }

    fn fail_adapter_attempt(
        &mut self,
        task_id: &str,
        reason: &str,
        now_ms: u64,
    ) -> Result<(), ExecutionError> {
        if let Ok(attempt) = self.current_attempt_mut(task_id) {
            attempt.state = TaskAttemptState::Failed;
            attempt.ended_at_ms = Some(now_ms);
            attempt.failure_class = Some(FailureClass::ExternalUnavailable);
            attempt.failure_fingerprint = Some(canonical_hash(&reason)?);
            attempt.termination_reason = Some(reason.into());
        }
        if let Some(task) = self.tasks.get_mut(task_id) {
            task.state = ExecutionTaskState::BlockedExternal;
        }
        for lease in &mut self.leases {
            if lease.task_id == task_id && lease.status == LeaseStatus::Active {
                lease.revoke()?;
            }
        }
        self.state = ExecutionRunState::BlockedExternal;
        self.last_error = Some(reason.into());
        self.emit(
            now_ms,
            Some(task_id.into()),
            ExecutionEventKind::TaskDriftDenied,
            if self
                .attempts
                .iter()
                .rev()
                .find(|attempt| attempt.task_id == task_id)
                .is_some_and(|attempt| {
                    attempt.execution_boundary == AttemptExecutionBoundary::NotStarted
                })
            {
                "execution prevented before external process boundary"
            } else {
                "adapter execution failed closed"
            },
        )?;
        Ok(())
    }

    fn mark_external_process_started(&mut self, task_id: &str) -> Result<(), ExecutionError> {
        self.current_attempt_mut(task_id)?.execution_boundary =
            AttemptExecutionBoundary::ExternalProcessStarted;
        Ok(())
    }

    fn legacy_pre_execution_attempt(attempt: &TaskAttempt) -> bool {
        attempt.execution_boundary == AttemptExecutionBoundary::Unknown
            && attempt.failure_class == Some(FailureClass::ExternalUnavailable)
            && attempt.usage.tool_calls == 0
            && attempt.usage.execution_steps == 0
            && attempt.usage.wall_time_ms == 0
            && attempt.completion_authority.is_none()
            && attempt.termination_reason.as_deref()
                == Some("Antigravity adapter: Antigravity version is incompatible")
    }

    fn current_recovery_attempt_record(&self) -> Option<&TaskAttempt> {
        self.attempts.iter().rev().find(|attempt| {
            matches!(
                attempt.state,
                TaskAttemptState::Failed
                    | TaskAttemptState::TurnEndedIncomplete
                    | TaskAttemptState::SafeBoundaryStopped
            ) && attempt.ended_at_ms.is_some()
                && attempt.completion_authority.is_none()
                && self.tasks.contains_key(&attempt.task_id)
        })
    }

    /// Return the newest durable terminal attempt that requires recovery.
    /// This prevents an older pre-execution failure or Safe Stop from masking
    /// a later attempt that crossed the external-process boundary.
    pub fn current_recovery_attempt(&self) -> Option<RecoveryAttemptTarget> {
        self.current_recovery_attempt_record()
            .map(|attempt| RecoveryAttemptTarget {
                task_id: attempt.task_id.clone(),
                attempt_id: attempt.attempt_id.clone(),
                lease_id: attempt.lease_id.clone(),
                execution_boundary: attempt.execution_boundary,
                failure_class: attempt.failure_class,
                termination_reason: attempt.termination_reason.clone(),
            })
    }

    /// Return whether the status surface should evaluate or expose recovery
    /// authority for this run. A newly admitted mission has no retry target
    /// and must not be represented as an interrupted run merely because no
    /// recovery checkpoint exists yet.
    pub fn recovery_status_requires_attention(&self) -> bool {
        self.current_recovery_attempt().is_some()
            || matches!(
                self.state,
                ExecutionRunState::BlockedExternal
                    | ExecutionRunState::SafeBoundaryReached
                    | ExecutionRunState::TurnEndedIncomplete
                    | ExecutionRunState::StoppedIncomplete
                    | ExecutionRunState::Failed
                    | ExecutionRunState::RevalidationRequired
            )
    }

    fn current_recovery_attempt_is_pre_execution(&self) -> bool {
        let Some(attempt) = self.current_recovery_attempt_record() else {
            return false;
        };
        attempt.state == TaskAttemptState::Failed
            && (attempt.execution_boundary == AttemptExecutionBoundary::NotStarted
                || Self::legacy_pre_execution_attempt(attempt))
            && attempt.usage.tool_calls == 0
            && attempt.usage.execution_steps == 0
            && attempt.usage.wall_time_ms == 0
            && attempt.completion_authority.is_none()
            && self
                .tasks
                .get(&attempt.task_id)
                .is_some_and(|task| task.state == ExecutionTaskState::BlockedExternal)
            && !self.leases.iter().any(|lease| {
                lease.task_id == attempt.task_id && lease.status == LeaseStatus::Active
            })
    }

    pub fn pre_execution_retry_task(&self) -> Option<String> {
        if self.current_recovery_attempt_is_pre_execution() {
            self.current_recovery_attempt_record()
                .map(|attempt| attempt.task_id.clone())
        } else {
            None
        }
    }

    /// Persist a visible, idempotent ledger event for a recovery decision that
    /// did not authorize a resume. The authenticated recovery record remains
    /// the decision authority; this event makes the completed check visible in
    /// Activity without duplicating it on repeated clicks.
    pub fn record_recovery_decision(
        &mut self,
        target: Option<&RecoveryAttemptTarget>,
        disposition: &str,
        now_ms: u64,
    ) -> Result<(), ExecutionError> {
        let detail = format!(
            "recovery decision {disposition} for {}",
            target
                .map(|value| value.attempt_id.as_str())
                .unwrap_or("unbound-attempt")
        );
        if self.events.iter().any(|event| {
            event.kind == ExecutionEventKind::AuthorityRevalidationRequired
                && event.detail == detail
        }) {
            return Ok(());
        }
        self.emit(
            now_ms,
            target.map(|value| value.task_id.clone()),
            ExecutionEventKind::AuthorityRevalidationRequired,
            detail,
        )
    }

    /// Production command boundary. This path always selects the real
    /// adapter. If the executable, plugin, or compatibility contract is not
    /// available, the scheduler records a truthful external blocker and never
    /// creates synthetic process success.
    pub fn dispatch_next_with_production_adapter(
        &mut self,
        revision: &MissionRevision,
        handoff: &ExecutionHandoff,
        now_ms: u64,
    ) -> Result<(), ExecutionError> {
        self.dispatch_next_with_production_adapter_at_path(revision, handoff, None, now_ms)
    }

    pub fn dispatch_next_with_production_adapter_at_path(
        &mut self,
        revision: &MissionRevision,
        handoff: &ExecutionHandoff,
        cli_path: Option<&Path>,
        now_ms: u64,
    ) -> Result<(), ExecutionError> {
        let mut adapter = ProductionAdapter::with_cli_path(cli_path.map(Path::to_path_buf));
        self.dispatch_next_with_adapter_with_callback(
            &mut adapter,
            revision,
            handoff,
            now_ms,
            |_, _| Ok(()),
        )
    }

    pub fn dispatch_next_with_adapter_with_callback<
        A: AntigravityAdapter,
        F: FnMut(&ExecutionRun, &ProcessIdentity) -> Result<(), ExecutionError>,
    >(
        &mut self,
        adapter: &mut A,
        revision: &MissionRevision,
        handoff: &ExecutionHandoff,
        now_ms: u64,
        on_attempt_started: F,
    ) -> Result<(), ExecutionError> {
        self.dispatch_next_with_adapter_with_callbacks(
            adapter,
            revision,
            handoff,
            now_ms,
            on_attempt_started,
            |_| Ok(()),
        )
    }

    pub fn dispatch_next_with_adapter_with_callbacks<
        A: AntigravityAdapter,
        F: FnMut(&ExecutionRun, &ProcessIdentity) -> Result<(), ExecutionError>,
        G: FnMut(&ExecutionRun) -> Result<(), ExecutionError>,
    >(
        &mut self,
        adapter: &mut A,
        revision: &MissionRevision,
        handoff: &ExecutionHandoff,
        now_ms: u64,
        on_attempt_started: F,
        on_completion_recorded: G,
    ) -> Result<(), ExecutionError> {
        self.validate_authority_identity(revision, handoff, now_ms)?;
        if self.runnable_tasks().is_empty() {
            return Err(ExecutionError::DependencyNotReady(
                "no runnable task".into(),
            ));
        }
        let root = self.workspace.clone();
        match self.execute_next_with_adapter_with_callbacks(
            adapter,
            revision,
            handoff,
            &root,
            now_ms,
            (on_attempt_started, on_completion_recorded),
        ) {
            Ok(_) => {}
            Err(_error)
                if matches!(
                    self.state,
                    ExecutionRunState::BlockedExternal | ExecutionRunState::RevalidationRequired
                ) => {}
            Err(error) => return Err(error),
        }
        Ok(())
    }

    pub fn snapshot_json(&self) -> Result<String, ExecutionError> {
        let mut snapshot = self.clone();
        snapshot.integrity_version = EXECUTION_LEDGER_INTEGRITY_VERSION.into();
        snapshot.integrity_tag.clear();
        snapshot.integrity_tag = ledger_tag(&snapshot)?;
        serde_json::to_string(&CurrentLedgerEnvelope {
            record_version: EXECUTION_LEDGER_RECORD_VERSION,
            run: &snapshot,
        })
        .map_err(|error| ExecutionError::Ledger(error.to_string()))
    }

    pub fn persist_snapshot(&self, path: &Path) -> Result<(), ExecutionError> {
        let json = self.snapshot_json()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| ExecutionError::Ledger(error.to_string()))?;
        }
        let temporary = path.with_file_name(format!(".{}.tmp", self.run_id));
        let _ = fs::remove_file(&temporary);
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)
            .map_err(|error| ExecutionError::Ledger(error.to_string()))?;
        file.write_all(json.as_bytes())
            .map_err(|error| ExecutionError::Ledger(error.to_string()))?;
        file.sync_all()
            .map_err(|error| ExecutionError::Ledger(error.to_string()))?;
        drop(file);
        atomic_replace(&temporary, path)?;
        Ok(())
    }

    pub fn restore_json(json: &str) -> Result<Self, ExecutionError> {
        let record_version = persisted_ledger_record_version(json)?;
        let mut run: Self = serde_json::from_str(json)
            .map_err(|error| ExecutionError::Ledger(error.to_string()))?;
        if run.ledger_version != EXECUTION_LEDGER_VERSION || run.integrity_tag.is_empty() {
            return Err(ExecutionError::Ledger(
                "unsupported or incomplete execution ledger envelope".into(),
            ));
        }
        let expected = run.integrity_tag.clone();
        match record_version.as_deref() {
            Some(EXECUTION_LEDGER_RECORD_VERSION) => {
                if run.integrity_version != EXECUTION_LEDGER_INTEGRITY_VERSION {
                    return Err(ExecutionError::Ledger(
                        "execution ledger integrity version is unsupported".into(),
                    ));
                }
                if !current_record_authenticates(&run, &expected)? {
                    return Err(ExecutionError::Ledger(
                        "execution ledger integrity verification failed".into(),
                    ));
                }
            }
            None => {
                if run.integrity_version != LEGACY_EXECUTION_LEDGER_INTEGRITY_VERSION
                    || !legacy_record_authenticates(json, &expected)?
                {
                    return Err(ExecutionError::Ledger(
                        "execution ledger legacy integrity verification failed".into(),
                    ));
                }
            }
            Some(_) => {
                return Err(ExecutionError::Ledger(
                    "unsupported execution ledger record version".into(),
                ));
            }
        }
        run.normalize_historical_budget_boundary();
        run.validate_invariants()?;
        Ok(run)
    }

    pub fn restore_snapshot(path: &Path) -> Result<Self, ExecutionError> {
        let json =
            fs::read_to_string(path).map_err(|error| ExecutionError::Ledger(error.to_string()))?;
        let legacy = persisted_ledger_record_version(&json)?.is_none();
        let run = Self::restore_json(&json)?;
        let current_json = run.snapshot_json()?;
        if legacy || current_json != json {
            run.persist_snapshot(path)?;
        }
        Ok(run)
    }

    fn task_usage(&self, task_id: &str) -> Result<UsageTelemetry, ExecutionError> {
        if !self.tasks.contains_key(task_id) {
            return Err(ExecutionError::UnknownTask(task_id.into()));
        }
        let mut usage = UsageTelemetry::default();
        for attempt in self
            .attempts
            .iter()
            .filter(|attempt| attempt.task_id == task_id)
        {
            usage.tool_calls = usage.tool_calls.saturating_add(attempt.usage.tool_calls);
            usage.execution_steps = usage
                .execution_steps
                .saturating_add(attempt.usage.execution_steps);
            usage.wall_time_ms = usage
                .wall_time_ms
                .saturating_add(attempt.usage.wall_time_ms);
            usage.retry_count = usage.retry_count.saturating_add(attempt.usage.retry_count);
            usage.estimated_cost_micros = match (
                usage.estimated_cost_micros,
                attempt.usage.estimated_cost_micros,
            ) {
                (Some(left), Some(right)) => Some(left.saturating_add(right)),
                (Some(left), None) => Some(left),
                (None, Some(right)) => Some(right),
                (None, None) => None,
            };
            usage.actual_cost_micros =
                match (usage.actual_cost_micros, attempt.usage.actual_cost_micros) {
                    (Some(left), Some(right)) => Some(left.saturating_add(right)),
                    (Some(left), None) => Some(left),
                    (None, Some(right)) => Some(right),
                    (None, None) => None,
                };
        }
        Ok(usage)
    }

    fn remaining_budget(&self, task_id: &str) -> Result<UsageBudget, ExecutionError> {
        let task = self
            .tasks
            .get(task_id)
            .ok_or_else(|| ExecutionError::UnknownTask(task_id.into()))?;
        let used = self.task_usage(task_id)?;
        let mut budget = task.usage_budget.clone();
        budget.tool_calls = budget.tool_calls.saturating_sub(used.tool_calls);
        budget.execution_steps = budget.execution_steps.saturating_sub(used.execution_steps);
        budget.wall_clock_ms = budget.wall_clock_ms.saturating_sub(used.wall_time_ms);
        let retries = self
            .attempts
            .iter()
            .filter(|attempt| attempt.task_id == task_id)
            .count()
            .saturating_sub(1) as u32;
        budget.retry_attempts = budget.retry_attempts.saturating_sub(retries);
        if let Some(limit) = budget.cost_micros {
            let used_cost = used
                .estimated_cost_micros
                .unwrap_or_default()
                .max(used.actual_cost_micros.unwrap_or_default());
            budget.cost_micros = Some(limit.saturating_sub(used_cost));
        }
        Ok(budget)
    }

    fn validate_invariants(&self) -> Result<(), ExecutionError> {
        let expected_run_id = canonical_hash(&(
            self.mission_id.as_str(),
            self.mission_revision,
            self.seal_hash.as_str(),
        ))?;
        if expected_run_id != self.run_id
            || self.mission_id.is_empty()
            || self.project_id.is_empty()
        {
            return Err(ExecutionError::Ledger(
                "execution identity invariant failed".into(),
            ));
        }
        for (id, task) in &self.tasks {
            if id != &task.task_id
                || task
                    .dependency_ids
                    .iter()
                    .any(|dep| dep == id || !self.tasks.contains_key(dep))
            {
                return Err(ExecutionError::Ledger("task graph invariant failed".into()));
            }
            let used = self.task_usage(id)?;
            let usage_exceeded = used.tool_calls > task.usage_budget.tool_calls
                || used.execution_steps > task.usage_budget.execution_steps
                || used.wall_time_ms > task.usage_budget.wall_clock_ms;
            if usage_exceeded && !self.has_durable_budget_boundary(&task.task_id) {
                return Err(ExecutionError::Ledger("task usage invariant failed".into()));
            }
        }
        for event in self.events.iter().enumerate() {
            if event.1.sequence != event.0 as u64 + 1 {
                return Err(ExecutionError::Ledger(
                    "event sequence invariant failed".into(),
                ));
            }
        }
        for lease in &self.leases {
            if !self.tasks.contains_key(&lease.task_id)
                || lease.compute_digest()? != lease.lease_digest
            {
                return Err(ExecutionError::Ledger("lease invariant failed".into()));
            }
        }
        Ok(())
    }

    fn has_durable_budget_boundary(&self, task_id: &str) -> bool {
        let task_blocked = self.tasks.get(task_id).is_some_and(|task| {
            matches!(
                task.state,
                ExecutionTaskState::BlockedExternal
                    | ExecutionTaskState::Failed
                    | ExecutionTaskState::Stopped
            )
        });
        let no_active_lease = self
            .leases
            .iter()
            .filter(|lease| lease.task_id == task_id)
            .all(|lease| lease.status != LeaseStatus::Active);
        task_blocked
            && no_active_lease
            && matches!(
                self.state,
                ExecutionRunState::BlockedExternal
                    | ExecutionRunState::Failed
                    | ExecutionRunState::StoppedIncomplete
                    | ExecutionRunState::RevalidationRequired
            )
            && self.events.iter().any(|event| {
                event.kind == ExecutionEventKind::BudgetWarning
                    && event.task_id.as_deref() == Some(task_id)
            })
    }

    fn normalize_historical_budget_boundary(&mut self) {
        let budget_events = self
            .events
            .iter()
            .filter(|event| event.kind == ExecutionEventKind::BudgetWarning)
            .filter_map(|event| {
                event
                    .task_id
                    .as_ref()
                    .map(|task_id| (task_id.clone(), event.occurred_at_ms))
            })
            .collect::<BTreeMap<_, _>>();
        let active_leases = self
            .leases
            .iter()
            .filter(|lease| lease.status == LeaseStatus::Active)
            .map(|lease| lease.lease_id.clone())
            .collect::<BTreeSet<_>>();
        for attempt in &mut self.attempts {
            if attempt.state != TaskAttemptState::Running {
                continue;
            }
            let Some(ended_at_ms) = budget_events.get(&attempt.task_id) else {
                continue;
            };
            let lease_revoked = !active_leases.contains(&attempt.lease_id);
            if lease_revoked {
                attempt.state = TaskAttemptState::Failed;
                attempt.ended_at_ms = Some(*ended_at_ms);
                attempt.failure_class = Some(FailureClass::Timeout);
                attempt.termination_reason = Some(
                    "historical budget boundary was durably recorded; execution requires recovery"
                        .into(),
                );
            }
        }
    }

    fn current_attempt_mut(&mut self, task_id: &str) -> Result<&mut TaskAttempt, ExecutionError> {
        self.attempts
            .iter_mut()
            .rev()
            .find(|attempt| {
                attempt.task_id == task_id && attempt.state == TaskAttemptState::Running
            })
            .ok_or_else(|| ExecutionError::AttemptMissing(task_id.into()))
    }

    fn completed_tasks(&self) -> Vec<String> {
        self.tasks
            .values()
            .filter(|task| task.state == ExecutionTaskState::FinishedAwaitingVerification)
            .map(|task| task.task_id.clone())
            .collect()
    }

    fn task_state_fingerprint(&self) -> Result<String, ExecutionError> {
        canonical_hash(
            &self
                .tasks
                .iter()
                .map(|(id, task)| (id.clone(), task.state))
                .collect::<BTreeMap<_, _>>(),
        )
    }

    fn update_run_state(&mut self) {
        if self
            .tasks
            .values()
            .all(|task| task.state == ExecutionTaskState::FinishedAwaitingVerification)
        {
            self.state = ExecutionRunState::ExecutionTasksFinishedAwaitingVerification;
        } else if self.runnable_tasks().is_empty() {
            self.state = ExecutionRunState::WaitingDependency;
        } else {
            // A completed task releases its dependencies into a new explicit
            // manual dispatch boundary. No task is auto-started and the prior
            // RUNNING state must not block the desktop's Run next task action.
            self.state = ExecutionRunState::Ready;
        }
    }

    fn emit(
        &mut self,
        occurred_at_ms: u64,
        task_id: Option<String>,
        kind: ExecutionEventKind,
        detail: impl Into<String>,
    ) -> Result<(), ExecutionError> {
        self.events.push(ExecutionEvent {
            sequence: self.events.len() as u64 + 1,
            occurred_at_ms,
            task_id,
            kind,
            detail: detail.into(),
        });
        Ok(())
    }
}

fn validate_sealed_task_requirement_ids(revision: &MissionRevision) -> Result<(), ExecutionError> {
    revision
        .contract
        .task_graph
        .validate(&revision.contract.requirement_graph)
        .map_err(|error| ExecutionError::RevalidationRequired(error.to_string()))?;
    for task in &revision.contract.task_graph.tasks {
        for requirement_id in &task.requirement_ids {
            if !valid_sealed_requirement_id(requirement_id) {
                return Err(ExecutionError::RevalidationRequired(format!(
                    "task {} contains non-canonical sealed requirement ID {}",
                    task.task_id, requirement_id
                )));
            }
        }
    }
    Ok(())
}

fn valid_sealed_requirement_id(id: &str) -> bool {
    let bytes = id.as_bytes();
    let compact_spec_id = bytes.len() == 4
        && (b'A'..=b'L').contains(&bytes[0])
        && bytes[1] == b'-'
        && bytes[2].is_ascii_digit()
        && bytes[3].is_ascii_digit()
        && (1..=12).contains(&id[2..].parse::<u8>().unwrap_or(0));
    let stable_requirement_id = id.strip_prefix("requirement_").is_some_and(|digest| {
        (16..=64).contains(&digest.len()) && digest.bytes().all(|byte| byte.is_ascii_hexdigit())
    });
    compact_spec_id || stable_requirement_id
}

fn executable_task_order(revision: &MissionRevision) -> Result<Vec<String>, ExecutionError> {
    let order = revision
        .contract
        .task_graph
        .topological_order()
        .map_err(|error| ExecutionError::RevalidationRequired(error.to_string()))?;
    let executable = revision
        .contract
        .task_graph
        .tasks
        .iter()
        .filter(|task| {
            !matches!(
                task.status,
                RequirementStatus::DeferredByExplicitDecision | RequirementStatus::NotApplicable
            )
        })
        .map(|task| task.task_id.as_str())
        .collect::<BTreeSet<_>>();
    Ok(order
        .into_iter()
        .filter(|task_id| executable.contains(task_id.as_str()))
        .collect())
}

fn overlap(left: &[String], right: &[String]) -> bool {
    left.iter().any(|a| {
        let a = normalize_path(a);
        right.iter().any(|b| {
            let b = normalize_path(b);
            a.starts_with(&b) || b.starts_with(&a)
        })
    })
}

fn beta_task_wall_clock_budget_ms(
    priority: RequirementPriority,
    requirement_count: usize,
    dependency_count: usize,
    evidence_count: usize,
    objective_len: usize,
    safety_cap_ms: u64,
) -> u64 {
    let mut complexity = match priority {
        RequirementPriority::P0 => 4_u32,
        RequirementPriority::P1 => 4,
        RequirementPriority::P2 => 2,
        RequirementPriority::P3 => 1,
    };
    complexity = complexity
        .saturating_add(requirement_count.saturating_sub(1).min(4) as u32)
        .saturating_add(dependency_count.min(3) as u32)
        .saturating_add(evidence_count.saturating_sub(1).min(3) as u32);
    if objective_len >= 240 {
        complexity = complexity.saturating_add(2);
    } else if objective_len >= 120 {
        complexity = complexity.saturating_add(1);
    }

    let selected = if complexity >= 8 {
        BETA_COMPLEX_TASK_EXECUTION_BUDGET_MS
    } else if complexity >= 4 {
        BETA_TASK_EXECUTION_BUDGET_MS
    } else {
        BETA_MIN_TASK_EXECUTION_BUDGET_MS
    };
    selected.min(safety_cap_ms)
}

fn budget_exhausted(budget: &UsageBudget) -> bool {
    budget.wall_clock_ms == 0
        && budget.execution_steps == 0
        && budget.tool_calls == 0
        && budget.retry_attempts == 0
        && budget.cost_micros.is_none_or(|cost| cost == 0)
}

fn execution_now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_millis() as u64)
        .unwrap_or_default()
}

fn excluded_workspace_path(path: &Path) -> bool {
    path.components().any(|component| {
        matches!(
            component.as_os_str().to_string_lossy().as_ref(),
            ".git" | "target" | "node_modules" | ".cargo-home"
        )
    })
}

fn workspace_inventory(root: &Path) -> Result<BTreeMap<String, String>, ExecutionError> {
    if !root.is_dir() {
        return Err(ExecutionError::PolicyDenied(format!(
            "workspace is not a directory: {}",
            root.display()
        )));
    }
    let canonical_root =
        fs::canonicalize(root).map_err(|error| ExecutionError::Ledger(error.to_string()))?;
    let mut inventory = BTreeMap::new();
    for entry in WalkDir::new(&canonical_root)
        .follow_links(false)
        .max_depth(128)
    {
        let entry = entry.map_err(|error| ExecutionError::Ledger(error.to_string()))?;
        if !entry.file_type().is_file() {
            continue;
        }
        let relative_path = entry
            .path()
            .strip_prefix(&canonical_root)
            .map_err(|error| ExecutionError::Ledger(error.to_string()))?;
        if excluded_workspace_path(relative_path) {
            continue;
        }
        let relative = relative_path.to_string_lossy().replace('\\', "/");
        let bytes =
            fs::read(entry.path()).map_err(|error| ExecutionError::Ledger(error.to_string()))?;
        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        inventory.insert(relative, format!("{:x}", hasher.finalize()));
    }
    Ok(inventory)
}

fn inventory_fingerprint(inventory: &BTreeMap<String, String>) -> Result<String, ExecutionError> {
    canonical_hash(
        &inventory
            .iter()
            .map(|(path, digest)| (path.clone(), digest.clone()))
            .collect::<Vec<_>>(),
    )
}

/// Count only structured test-runner summaries emitted by the owned process.
/// Free-form agent claims such as "all tests pass" are intentionally ignored.
fn observed_passing_test_count(stdout: &[u8], stderr: &[u8]) -> Option<u64> {
    let mut total = 0_u64;
    let mut observed = false;
    for line in String::from_utf8_lossy(stdout)
        .lines()
        .chain(String::from_utf8_lossy(stderr).lines())
    {
        let lower = line.trim().to_ascii_lowercase();
        if lower.contains("test result: ok.") {
            observed = true;
            total = total.saturating_add(parse_count_before(&lower, "passed"));
        } else if lower.starts_with("# pass ") {
            observed = true;
            total = total.saturating_add(
                lower
                    .strip_prefix("# pass ")
                    .and_then(|value| value.trim().parse::<u64>().ok())
                    .unwrap_or_default(),
            );
        } else if lower.contains("tests") && lower.contains("passed") {
            observed = true;
            total = total.saturating_add(parse_count_before(&lower, "passed"));
        }
    }
    observed.then_some(total)
}

/// Fingerprint only structured compiler/test/runtime diagnostics. Free-form
/// executor narration is not progress authority and therefore hashes to zero.
fn observed_diagnostic_information(stdout: &[u8], stderr: &[u8]) -> u64 {
    let mut diagnostics = String::from_utf8_lossy(stdout)
        .lines()
        .chain(String::from_utf8_lossy(stderr).lines())
        .filter_map(|line| {
            let trimmed = line.trim();
            let lower = trimmed.to_ascii_lowercase();
            let structured = lower.starts_with("error:")
                || lower.starts_with("error[")
                || lower.contains(": error:")
                || lower.contains(": error ts")
                || lower.starts_with("warning:")
                || lower.contains(": warning:")
                || lower.contains("test result: failed")
                || lower.starts_with("fail ")
                || lower.starts_with("failed ")
                || lower.contains("panicked at")
                || lower.starts_with("thread '") && lower.contains("panicked")
                || lower.starts_with("traceback (")
                || lower.contains("exception:")
                || lower.contains("assertionerror")
                || lower.starts_with("npm error")
                || lower.starts_with("npm err!")
                || lower.starts_with("caused by:");
            structured.then(|| trimmed.to_string())
        })
        .collect::<Vec<_>>();
    diagnostics.sort();
    diagnostics.dedup();
    if diagnostics.is_empty() {
        return 0;
    }

    let mut hasher = Sha256::new();
    for line in diagnostics {
        hasher.update(line.as_bytes());
        hasher.update(b"\n");
    }
    let digest = hasher.finalize();
    digest
        .iter()
        .take(8)
        .fold(0_u64, |value, byte| (value << 8) | u64::from(*byte))
        .max(1)
}

fn parse_count_before(line: &str, marker: &str) -> u64 {
    line.split(marker)
        .next()
        .and_then(|prefix| {
            prefix.split_whitespace().rev().find_map(|value| {
                value
                    .trim_matches(|ch: char| !ch.is_ascii_digit())
                    .parse()
                    .ok()
            })
        })
        .unwrap_or_default()
}

fn workspace_changes(
    before: &BTreeMap<String, String>,
    after: &BTreeMap<String, String>,
) -> Vec<WorkspaceArtifactChange> {
    let paths = before
        .keys()
        .chain(after.keys())
        .cloned()
        .collect::<BTreeSet<_>>();
    paths
        .into_iter()
        .filter_map(|path| {
            let before_digest = before.get(&path).cloned();
            let after_digest = after.get(&path).cloned();
            let kind = match (&before_digest, &after_digest) {
                (None, Some(_)) => WorkspaceArtifactChangeKind::New,
                (Some(_), None) => WorkspaceArtifactChangeKind::Deleted,
                (Some(left), Some(right)) if left != right => WorkspaceArtifactChangeKind::Modified,
                _ => return None,
            };
            Some(WorkspaceArtifactChange {
                path,
                kind,
                before_digest,
                after_digest,
            })
        })
        .collect()
}

/// Return the shared authority boundary for an owned executor. Keeping this
/// pure makes the policy deterministic and independently testable.
pub fn effective_execution_deadline_ms(
    lease_expires_at_ms: u64,
    attempt_started_at_ms: u64,
    task_budget_ms: u64,
    now_ms: u64,
    safety_cap_ms: u64,
) -> u64 {
    lease_expires_at_ms
        .min(attempt_started_at_ms.saturating_add(task_budget_ms))
        .min(now_ms.saturating_add(safety_cap_ms))
}

fn trusted_signers_present(trusted: &TrustedSignerSet) -> bool {
    !trusted.signers.is_empty()
}

fn safe_path_within(candidate: &Path, root: &Path) -> bool {
    let Some(candidate) = resolve_path(candidate) else {
        return false;
    };
    let Some(root) = resolve_path(root) else {
        return false;
    };
    let candidate = path_components(&candidate);
    let root = path_components(&root);
    candidate.len() >= root.len()
        && candidate
            .iter()
            .zip(root.iter())
            .all(|(left, right)| left == right)
}

fn resolve_path(path: &Path) -> Option<PathBuf> {
    if !path.is_absolute() {
        return None;
    }
    let mut current = path.to_path_buf();
    let mut suffix = Vec::new();
    while !current.exists() {
        let name = current.file_name()?.to_owned();
        suffix.push(name);
        current = current.parent()?.to_path_buf();
    }
    let mut resolved = fs::canonicalize(current).ok()?;
    for component in suffix.iter().rev() {
        resolved.push(component);
    }
    Some(resolved)
}

fn path_components(path: &Path) -> Vec<String> {
    path.components()
        .filter_map(|component| match component {
            std::path::Component::Prefix(prefix) => {
                Some(prefix.as_os_str().to_string_lossy().to_ascii_lowercase())
            }
            std::path::Component::RootDir => Some("/".into()),
            std::path::Component::Normal(value) => {
                Some(value.to_string_lossy().to_ascii_lowercase())
            }
            std::path::Component::CurDir => None,
            std::path::Component::ParentDir => Some("..".into()),
        })
        .collect()
}

fn normalize_path(value: &str) -> String {
    value
        .replace('\\', "/")
        .trim_end_matches('/')
        .to_ascii_lowercase()
}

fn normalize_arguments(arguments: &[String]) -> Vec<String> {
    arguments
        .iter()
        .map(|argument| argument.split_whitespace().collect::<Vec<_>>().join(" "))
        .collect()
}

pub fn action_fingerprint(action: &ActionRequest) -> Result<String, ExecutionError> {
    canonical_hash(&(
        action.tool.to_ascii_lowercase(),
        action.operation.to_ascii_lowercase(),
        normalize_arguments(&action.arguments),
        normalize_path(&action.working_scope),
        action.environment_identity.to_ascii_lowercase(),
    ))
}

#[cfg(test)]
mod recovery_boundary_tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};

    static FIXTURE_SEQUENCE: AtomicU64 = AtomicU64::new(1);

    fn fixture_root(label: &str) -> PathBuf {
        let sequence = FIXTURE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "relintor-recovery-boundary-{label}-{}-{sequence}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("create recovery fixture");
        root
    }

    fn fixture(
        label: &str,
        boundary: AttemptExecutionBoundary,
    ) -> (PathBuf, ExecutionRun, RecoveryAuthority, RecoveryStore) {
        let root = fixture_root(label);
        let task_id = "task-recovery".to_string();
        let run_id = canonical_hash(&("mission-recovery", 1_u64, "seal-recovery"))
            .expect("hash recovery run identity");
        let scope = LeaseScope {
            workspace: root.clone(),
            file_scopes: vec![root.display().to_string()],
            directory_scopes: vec![root.display().to_string()],
            shared_resources: Vec::new(),
            package_lockfiles: Vec::new(),
            generated_files: Vec::new(),
            allowed_tools: BTreeSet::new(),
            external_authority: BTreeSet::new(),
            scope_known: true,
        };
        let mut tasks = BTreeMap::new();
        tasks.insert(
            task_id.clone(),
            ExecutionTask {
                task_id: task_id.clone(),
                objective: "recover the failed task".into(),
                requirement_ids: Vec::new(),
                dependency_ids: Vec::new(),
                priority: RequirementPriority::P1,
                state: ExecutionTaskState::BlockedExternal,
                scope,
                usage_budget: UsageBudget::default(),
                retry_policy: RetryPolicy::default(),
                evidence_obligations: Vec::new(),
                attempt_number: 1,
            },
        );
        let run = ExecutionRun {
            ledger_version: EXECUTION_LEDGER_VERSION.into(),
            run_id: run_id.clone(),
            mission_id: "mission-recovery".into(),
            mission_revision: 1,
            seal_hash: "seal-recovery".into(),
            project_id: "project-recovery".into(),
            workspace: root.clone(),
            workspace_fingerprint: "fixture-workspace".into(),
            state: ExecutionRunState::BlockedExternal,
            tasks,
            attempts: vec![TaskAttempt {
                attempt_id: "attempt-1".into(),
                task_id,
                attempt_number: 1,
                packet_digest: "packet-1".into(),
                lease_id: "lease-1".into(),
                state: TaskAttemptState::Failed,
                started_at_ms: 10,
                ended_at_ms: Some(11),
                failure_class: Some(FailureClass::ExternalUnavailable),
                failure_fingerprint: Some("failure-1".into()),
                usage: UsageTelemetry::default(),
                termination_reason: Some(
                    "Antigravity adapter: Antigravity version is incompatible".into(),
                ),
                execution_boundary: boundary,
                completion_authority: None,
                workspace_before: None,
                workspace_after: None,
            }],
            leases: Vec::new(),
            events: Vec::new(),
            usage: UsageTelemetry::default(),
            loop_signals: Vec::new(),
            oscillation_signals: Vec::new(),
            continuations: Vec::new(),
            diagnostics: Vec::new(),
            external_modifications: Vec::new(),
            progress: Vec::new(),
            watchdog_state: WatchdogState::Healthy,
            policy: SchedulerPolicy::default(),
            safe_boundary: None,
            current_turn: 1,
            last_error: Some("adapter failed before execution".into()),
            no_progress_occurrences: 0,
            integrity_version: "p7-ledger-integrity-v1".into(),
            integrity_tag: String::new(),
        };
        let workspace = WorkspaceSnapshot::capture(&root, 1).expect("capture fixture workspace");
        let authority = RecoveryAuthority::new(
            &run.project_id,
            &run.mission_id,
            run.mission_revision,
            &run.seal_hash,
            "registry-recovery",
            &run.run_id,
            workspace.fingerprint,
            "source-recovery",
            None,
            "P8_PENDING",
            1,
        );
        let recovery_root = root.with_file_name(format!(".relintor-recovery-store-{label}"));
        let _ = fs::remove_dir_all(&recovery_root);
        let store =
            RecoveryStore::new(recovery_root, vec![9; 32]).expect("open recovery fixture store");
        (root, run, authority, store)
    }

    fn prepare_persisted_completion(run: &mut ExecutionRun, root: &Path) {
        fs::write(root.join("completed.txt"), b"trusted-completion")
            .expect("write completed workspace");
        let after = workspace_inventory(root).expect("inventory completed workspace");
        let scope = run.tasks["task-recovery"].scope.clone();
        let budget = UsageBudget {
            wall_clock_ms: 1_000,
            ..UsageBudget::default()
        };
        let lease = ExecutionLease::issue(
            &run.mission_id,
            run.mission_revision,
            "task-recovery",
            "packet-complete",
            scope,
            budget,
            1,
            100,
        )
        .expect("issue completion lease");
        let lease_id = lease.lease_id.clone();
        run.leases = vec![lease];
        run.state = ExecutionRunState::Running;
        run.tasks.get_mut("task-recovery").expect("task").state = ExecutionTaskState::Running;
        let attempt = run.attempts.get_mut(0).expect("attempt");
        attempt.state = TaskAttemptState::Running;
        attempt.packet_digest = "packet-complete".into();
        attempt.lease_id = lease_id.clone();
        attempt.started_at_ms = 100;
        attempt.ended_at_ms = None;
        attempt.failure_class = None;
        attempt.failure_fingerprint = None;
        attempt.termination_reason = None;
        attempt.execution_boundary = AttemptExecutionBoundary::ExternalProcessStarted;
        attempt.workspace_before = Some(BTreeMap::new());
        attempt.workspace_after = Some(after);
        attempt.completion_authority = Some(ExecutionCompletionAuthority {
            task_id: "task-recovery".into(),
            attempt_id: attempt.attempt_id.clone(),
            packet_digest: "packet-complete".into(),
            lease_id,
            process_digest: "trusted-process-digest".into(),
            started_at_ms: 100,
            ended_at_ms: 200,
        });
    }

    #[test]
    fn persisted_trusted_completion_recovers_only_when_workspace_still_matches() {
        let (root, mut run, _, _) = fixture(
            "trusted-completion-recovery",
            AttemptExecutionBoundary::ExternalProcessStarted,
        );
        prepare_persisted_completion(&mut run, &root);
        assert!(run
            .recover_trusted_completion_after_restart(250)
            .expect("recover trusted completion"));
        assert_eq!(
            run.tasks["task-recovery"].state,
            ExecutionTaskState::FinishedAwaitingVerification
        );
        assert_eq!(run.attempts[0].state, TaskAttemptState::Succeeded);
        assert_eq!(run.leases[0].status, LeaseStatus::Consumed);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn persisted_trusted_completion_fails_closed_after_external_workspace_change() {
        let (root, mut run, _, _) = fixture(
            "trusted-completion-mutation",
            AttemptExecutionBoundary::ExternalProcessStarted,
        );
        prepare_persisted_completion(&mut run, &root);
        fs::write(root.join("completed.txt"), b"changed-after-receipt")
            .expect("mutate completed workspace");
        assert!(matches!(
            run.recover_trusted_completion_after_restart(250),
            Err(ExecutionError::RevalidationRequired(_))
        ));
        assert_eq!(
            run.tasks["task-recovery"].state,
            ExecutionTaskState::Running
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn trusted_completion_workspace_record_is_idempotent_but_conflicting_replay_is_rejected() {
        let (root, mut run, _, _) = fixture(
            "trusted-workspace-replay",
            AttemptExecutionBoundary::ExternalProcessStarted,
        );
        prepare_persisted_completion(&mut run, &root);
        let exact = run.attempts[0]
            .workspace_after
            .clone()
            .expect("persisted post workspace");
        run.record_completion_workspace("task-recovery", exact.clone())
            .expect("exact replay is idempotent");

        let mut conflicting = exact;
        conflicting.insert(
            "conflict.txt".into(),
            "not-the-authenticated-workspace".into(),
        );
        assert!(matches!(
            run.record_completion_workspace("task-recovery", conflicting),
            Err(ExecutionError::RevalidationRequired(_))
        ));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn exact_trusted_completion_promotion_replay_is_idempotent_but_conflicting_time_is_rejected() {
        let (root, mut run, _, _) = fixture(
            "trusted-promotion-replay",
            AttemptExecutionBoundary::ExternalProcessStarted,
        );
        prepare_persisted_completion(&mut run, &root);
        run.promote_task_from_trusted_completion("task-recovery", 200, true)
            .expect("first promotion");
        run.promote_task_from_trusted_completion("task-recovery", 200, true)
            .expect("exact replay is idempotent");
        assert!(matches!(
            run.promote_task_from_trusted_completion("task-recovery", 201, true),
            Err(ExecutionError::RevalidationRequired(_))
        ));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn beta_task_budget_is_complexity_sized_and_bounded_by_safety_cap() {
        assert_eq!(
            beta_task_wall_clock_budget_ms(
                RequirementPriority::P3,
                1,
                0,
                1,
                20,
                ADAPTER_PROCESS_SAFETY_TIMEOUT_MS,
            ),
            BETA_MIN_TASK_EXECUTION_BUDGET_MS
        );
        assert_eq!(
            beta_task_wall_clock_budget_ms(
                RequirementPriority::P1,
                1,
                0,
                1,
                20,
                ADAPTER_PROCESS_SAFETY_TIMEOUT_MS,
            ),
            BETA_TASK_EXECUTION_BUDGET_MS
        );
        assert_eq!(
            beta_task_wall_clock_budget_ms(
                RequirementPriority::P0,
                4,
                3,
                3,
                300,
                ADAPTER_PROCESS_SAFETY_TIMEOUT_MS,
            ),
            BETA_COMPLEX_TASK_EXECUTION_BUDGET_MS
        );
        assert_eq!(
            beta_task_wall_clock_budget_ms(RequirementPriority::P0, 4, 3, 3, 300, 5 * 60 * 1_000,),
            5 * 60 * 1_000
        );
    }

    #[test]
    fn measurable_progress_can_authorize_only_one_fresh_lease_cycle() {
        let (root, mut run, _, _) = fixture(
            "progress-renewal",
            AttemptExecutionBoundary::ExternalProcessStarted,
        );
        run.workspace_fingerprint = "before".into();
        let snapshot = ProgressSnapshot {
            workspace_fingerprint: "after".into(),
            changed_paths: vec!["src/main.rs".into()],
            diagnostic_information: 1,
            ..ProgressSnapshot::default()
        };
        assert!(run
            .authorize_progress_renewal("task-recovery", &snapshot, 100)
            .expect("authorize progress renewal"));
        assert!(run.has_pending_retry());
        assert_eq!(
            run.tasks["task-recovery"].state,
            ExecutionTaskState::WaitingRetry
        );
        assert!(!run
            .authorize_progress_renewal("task-recovery", &snapshot, 101)
            .expect("deny second progress renewal"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn diagnostic_progress_requires_structured_new_information() {
        assert_eq!(
            observed_diagnostic_information(
                b"still working; changed several files; everything looks good",
                b"",
            ),
            0
        );

        let first = observed_diagnostic_information(
            b"",
            b"error[E0308]: mismatched types\n  --> src/lib.rs:10:5",
        );
        let same = observed_diagnostic_information(
            b"",
            b"error[E0308]: mismatched types\n  --> src/lib.rs:10:5",
        );
        let changed = observed_diagnostic_information(
            b"",
            b"error[E0425]: cannot find value `authority` in this scope",
        );
        assert_ne!(first, 0);
        assert_eq!(first, same);
        assert_ne!(first, changed);
    }

    #[test]
    fn arbitrary_workspace_edits_do_not_renew_execution_authority() {
        let (root, mut run, _, _) = fixture(
            "edit-churn-no-renewal",
            AttemptExecutionBoundary::ExternalProcessStarted,
        );
        run.workspace_fingerprint = "before".into();
        let snapshot = ProgressSnapshot {
            workspace_fingerprint: "after".into(),
            changed_paths: vec!["src/main.rs".into()],
            ..ProgressSnapshot::default()
        };
        assert!(!run
            .authorize_progress_renewal("task-recovery", &snapshot, 100)
            .expect("edit churn must not renew authority"));
        assert!(!run.has_pending_retry());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn verification_correction_is_separate_bounded_authority_and_not_an_endless_loop() {
        let (root, mut run, _, _) = fixture(
            "verification-correction",
            AttemptExecutionBoundary::ExternalProcessStarted,
        );
        run.state = ExecutionRunState::ExecutionTasksFinishedAwaitingVerification;
        {
            let task = run.tasks.get_mut("task-recovery").expect("task");
            task.state = ExecutionTaskState::FinishedAwaitingVerification;
            task.requirement_ids = vec!["req-a".into()];
            task.attempt_number = task.retry_policy.max_attempts;
        }
        let failed = ["req-a".to_string()].into_iter().collect::<BTreeSet<_>>();
        let affected = run
            .authorize_verification_correction(&failed, 100)
            .expect("authorize single correction");
        assert_eq!(affected, vec!["task-recovery".to_string()]);
        assert_eq!(run.state, ExecutionRunState::Ready);
        assert_eq!(
            run.tasks["task-recovery"].state,
            ExecutionTaskState::Pending
        );
        assert!(
            run.tasks["task-recovery"].usage_budget.retry_attempts
                > run.tasks["task-recovery"].attempt_number
        );

        run.state = ExecutionRunState::ExecutionTasksFinishedAwaitingVerification;
        run.tasks.get_mut("task-recovery").expect("task").state =
            ExecutionTaskState::FinishedAwaitingVerification;
        assert!(matches!(
            run.authorize_verification_correction(&failed, 101),
            Err(ExecutionError::PolicyDenied(_))
        ));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn effective_deadline_is_the_minimum_of_task_lease_and_safety_cap() {
        assert_eq!(
            effective_execution_deadline_ms(1_000, 100, 700, 200, 600),
            800
        );
        assert_eq!(
            effective_execution_deadline_ms(9_000, 1_000, 8_000, 2_000, 500),
            2_500
        );
    }

    #[test]
    fn deadline_never_moves_forward_when_authority_has_expired() {
        assert_eq!(
            effective_execution_deadline_ms(1_000, 100, 900, 2_000, 900_000),
            1_000
        );
    }

    #[test]
    fn pre_execution_failure_is_authorized_without_side_effects() {
        let (root, mut run, authority, store) =
            fixture("pre-execution", AttemptExecutionBoundary::NotStarted);
        let coordinator = RecoveryCoordinator::new(store.clone());
        let result = coordinator
            .resume_integrity_for_run(&run, &authority, &root, &ConservativeProcessInspector, 20)
            .expect("evaluate pre-execution recovery");
        assert_eq!(
            result.disposition,
            RecoveryDisposition::PreExecutionRetryAuthorized
        );
        assert_eq!(
            result.classification,
            RecoveryClassification::PreExecutionPrevented
        );
        assert_eq!(
            run.pre_execution_retry_task().as_deref(),
            Some("task-recovery")
        );
        let record = coordinator
            .begin_revalidation(&authority, &result, 20)
            .expect("persist retry authority");
        assert_eq!(record.decision, "RETRY_AUTHORIZED");
        assert_eq!(record.mission_revision, 1);
        run.resume_from_recovery(result.disposition, 21)
            .expect("apply retry authority");
        assert_eq!(run.state, ExecutionRunState::Ready);
        assert_eq!(
            run.tasks["task-recovery"].state,
            ExecutionTaskState::WaitingRetry
        );
        assert_eq!(run.attempts.len(), 1);
        assert_eq!(run.mission_revision, 1);
    }

    #[test]
    fn pre_execution_failure_with_a_non_boundary_checkpoint_is_still_classified_safely() {
        let (root, run, authority, store) = fixture(
            "pre-execution-checkpoint",
            AttemptExecutionBoundary::NotStarted,
        );
        let coordinator = RecoveryCoordinator::new(store);
        coordinator
            .checkpoint_run(
                &run,
                authority.clone(),
                CheckpointKind::AfterAtomicAction,
                &root,
                Vec::new(),
                Vec::new(),
                "failed adapter attempt persisted",
                12,
            )
            .expect("persist failed attempt checkpoint");
        let result = coordinator
            .resume_integrity_for_run(&run, &authority, &root, &ConservativeProcessInspector, 20)
            .expect("evaluate checkpointed pre-execution recovery");
        assert_eq!(
            result.disposition,
            RecoveryDisposition::PreExecutionRetryAuthorized
        );
        assert_eq!(
            result.classification,
            RecoveryClassification::PreExecutionPrevented
        );
        assert!(result.checkpoint_id.is_some());
        assert_eq!(result.changed_paths, Vec::<String>::new());
    }

    #[test]
    fn side_effect_boundary_without_checkpoint_remains_blocked() {
        let (root, run, authority, store) = fixture(
            "side-effect",
            AttemptExecutionBoundary::ExternalProcessStarted,
        );
        let result = RecoveryCoordinator::new(store)
            .resume_integrity_for_run(&run, &authority, &root, &ConservativeProcessInspector, 20)
            .expect("evaluate uncertain recovery");
        assert_eq!(
            result.disposition,
            RecoveryDisposition::RevalidationRequired
        );
        assert_eq!(
            result.classification,
            RecoveryClassification::ExternalStateUncertain
        );
        assert!(result
            .reasons
            .iter()
            .any(|reason| reason.contains("external process started")));
        assert!(run.pre_execution_retry_task().is_none());
    }

    #[test]
    fn fresh_run_without_attempt_has_no_recovery_attention() {
        let (_root, mut run, _authority, _store) =
            fixture("fresh-run", AttemptExecutionBoundary::Unknown);
        run.state = ExecutionRunState::Ready;
        run.attempts.clear();

        assert!(run.current_recovery_attempt().is_none());
        assert!(!run.recovery_status_requires_attention());
    }

    #[test]
    fn newest_external_attempt_masks_older_pre_execution_retry_target() {
        let (_root, mut run, _authority, _store) = fixture(
            "current-recovery-target",
            AttemptExecutionBoundary::NotStarted,
        );
        run.attempts.push(TaskAttempt {
            attempt_id: "attempt-external-timeout".into(),
            task_id: "task-recovery".into(),
            attempt_number: 2,
            packet_digest: "packet-2".into(),
            lease_id: "lease-2".into(),
            state: TaskAttemptState::Failed,
            started_at_ms: 20,
            ended_at_ms: Some(40),
            failure_class: Some(FailureClass::Timeout),
            failure_fingerprint: Some("timeout".into()),
            usage: UsageTelemetry {
                wall_time_ms: 300_001,
                ..UsageTelemetry::default()
            },
            termination_reason: Some("historical budget boundary".into()),
            execution_boundary: AttemptExecutionBoundary::ExternalProcessStarted,
            completion_authority: None,
            workspace_before: None,
            workspace_after: None,
        });

        let target = run.current_recovery_attempt().expect("current target");
        assert_eq!(target.task_id, "task-recovery");
        assert_eq!(target.attempt_id, "attempt-external-timeout");
        assert_eq!(target.lease_id, "lease-2");
        assert_eq!(target.failure_class, Some(FailureClass::Timeout));
        assert_eq!(
            target.execution_boundary,
            AttemptExecutionBoundary::ExternalProcessStarted
        );
        assert!(run.pre_execution_retry_task().is_none());
    }

    #[test]
    fn external_attempt_decision_is_durable_and_idempotent_per_exact_target() {
        let (root, run, authority, store) = fixture(
            "external-decision",
            AttemptExecutionBoundary::ExternalProcessStarted,
        );
        let coordinator = RecoveryCoordinator::new(store.clone());
        let result = coordinator
            .resume_integrity_for_run(&run, &authority, &root, &ConservativeProcessInspector, 20)
            .expect("evaluate external recovery");
        let first = coordinator
            .begin_revalidation(&authority, &result, 20)
            .expect("persist external recovery decision");
        let second = coordinator
            .begin_revalidation(&authority, &result, 21)
            .expect("reuse exact external recovery decision");
        assert_eq!(first.revalidation_id, second.revalidation_id);
        assert_eq!(first.target, result.target);
        assert_eq!(first.disposition, RecoveryDisposition::RevalidationRequired);

        let mut later = result.clone();
        later.target = Some(RecoveryAttemptTarget {
            task_id: "task-recovery".into(),
            attempt_id: "attempt-later".into(),
            lease_id: "lease-later".into(),
            execution_boundary: AttemptExecutionBoundary::ExternalProcessStarted,
            failure_class: Some(FailureClass::Timeout),
            termination_reason: Some("later timeout".into()),
        });
        let later_record = coordinator
            .begin_revalidation(&authority, &later, 22)
            .expect("persist later attempt decision");
        assert_ne!(first.revalidation_id, later_record.revalidation_id);
        assert_eq!(
            coordinator
                .store
                .load_latest_revalidation()
                .expect("load latest decision")
                .expect("latest decision exists")
                .target,
            later.target
        );
    }

    #[test]
    fn reviewed_external_recovery_authorizes_only_a_fresh_attempt() {
        let (root, mut run, authority, store) = fixture(
            "manual-external-retry",
            AttemptExecutionBoundary::ExternalProcessStarted,
        );
        run.state = ExecutionRunState::RevalidationRequired;
        let coordinator = RecoveryCoordinator::new(store);
        coordinator
            .checkpoint_run(
                &run,
                authority.clone(),
                CheckpointKind::RestartRecovery,
                &root,
                vec![ProcessOwnershipRecord {
                    process_id: u32::MAX,
                    process_start_time_ms: Some(10),
                    executable_path: "agy.exe".into(),
                    executable_digest: "executor-digest".into(),
                    command_digest: "command-digest".into(),
                    run_id: run.run_id.clone(),
                    task_id: Some("task-recovery".into()),
                    attempt_id: Some("attempt-1".into()),
                    lease_id: Some("lease-1".into()),
                    launched_at_ms: 10,
                    observation: ProcessObservation::ProcessGone,
                }],
                Vec::new(),
                "interrupted external attempt reconciled",
                12,
            )
            .expect("persist reconciled external attempt");
        let authority = RecoveryAuthority::new(
            &run.project_id,
            &run.mission_id,
            run.mission_revision,
            &run.seal_hash,
            "registry-recovery",
            &run.run_id,
            WorkspaceSnapshot::capture(&root, 13)
                .expect("capture workspace")
                .fingerprint,
            "source-recovery",
            None,
            "P8_PENDING",
            13,
        );
        let result = coordinator
            .resume_integrity_for_run(&run, &authority, &root, &ConservativeProcessInspector, 20)
            .expect("evaluate reconciled external attempt");
        assert_eq!(
            result.disposition,
            RecoveryDisposition::RevalidationRequired
        );
        assert_eq!(
            result.process_observations,
            vec![ProcessObservation::ProcessGone]
        );
        let target = result.target.clone().expect("exact retry target");
        coordinator
            .authorize_manual_retry(&authority, &result, 21)
            .expect("persist manual retry authority");
        run.authorize_manual_recovery_retry(&target, 21)
            .expect("apply manual retry authority");
        assert_eq!(run.state, ExecutionRunState::Ready);
        assert_eq!(
            run.tasks["task-recovery"].state,
            ExecutionTaskState::WaitingRetry
        );
        assert_eq!(run.attempts[0].state, TaskAttemptState::WaitingRetry);
        assert!(run.current_recovery_attempt().is_none());
    }

    #[test]
    fn blocked_recovery_decision_event_is_visible_and_idempotent() {
        let (_root, mut run, _authority, _store) = fixture(
            "recovery-decision-event",
            AttemptExecutionBoundary::ExternalProcessStarted,
        );
        let target = run.current_recovery_attempt().expect("target");
        run.record_recovery_decision(Some(&target), "REVALIDATION_REQUIRED", 1)
            .expect("record blocked decision");
        run.record_recovery_decision(Some(&target), "REVALIDATION_REQUIRED", 2)
            .expect("reuse blocked decision event");
        assert_eq!(run.events.len(), 1);
        assert_eq!(
            run.events[0].kind,
            ExecutionEventKind::AuthorityRevalidationRequired
        );
    }

    #[test]
    fn retry_preserves_failed_attempt_and_revokes_stale_lease_on_new_attempt() {
        let (root, mut run, authority, store) =
            fixture("retry-attempts", AttemptExecutionBoundary::NotStarted);
        let coordinator = RecoveryCoordinator::new(store);
        let result = coordinator
            .resume_integrity_for_run(&run, &authority, &root, &ConservativeProcessInspector, 20)
            .expect("evaluate retry");
        run.resume_from_recovery(result.disposition, 21)
            .expect("authorize retry");
        let _first_retry_packet = run.start_task("task-recovery", 22).expect("start retry");
        run.tasks
            .get_mut("task-recovery")
            .expect("recovery task")
            .state = ExecutionTaskState::WaitingRetry;
        let _second_retry_packet = run
            .start_task("task-recovery", 23)
            .expect("start bounded replacement attempt");
        assert_eq!(run.attempts.len(), 3);
        assert_eq!(run.attempts[0].state, TaskAttemptState::Failed);
        assert_eq!(run.attempts[1].state, TaskAttemptState::WaitingRetry);
        assert_eq!(run.attempts[2].state, TaskAttemptState::Running);
        assert_eq!(run.leases.len(), 2);
        assert!(run
            .leases
            .iter()
            .any(|lease| lease.status == LeaseStatus::Revoked));
        assert_eq!(run.tasks["task-recovery"].attempt_number, 3);
    }

    #[test]
    fn recovery_decision_is_authenticated_idempotent_and_reloadable() {
        let (root, run, authority, store) =
            fixture("durable-decision", AttemptExecutionBoundary::NotStarted);
        let coordinator = RecoveryCoordinator::new(store.clone());
        let result = coordinator
            .resume_integrity_for_run(&run, &authority, &root, &ConservativeProcessInspector, 20)
            .expect("evaluate durable retry");
        let first = coordinator
            .begin_revalidation(&authority, &result, 20)
            .expect("write durable decision");
        let second = coordinator
            .begin_revalidation(&authority, &result, 21)
            .expect("reuse durable decision");
        assert_eq!(first.revalidation_id, second.revalidation_id);
        let reloaded = RecoveryStore::new(store.root().to_path_buf(), vec![9; 32])
            .expect("reload recovery store")
            .load_latest_revalidation()
            .expect("load durable decision")
            .expect("decision exists");
        assert_eq!(reloaded.decision, "RETRY_AUTHORIZED");
        assert_eq!(
            reloaded.disposition,
            RecoveryDisposition::PreExecutionRetryAuthorized
        );
    }

    #[test]
    fn legacy_ledger_without_execution_boundary_remains_authenticated_and_readable() {
        let (_root, run, _authority, _store) =
            fixture("legacy-ledger-shape", AttemptExecutionBoundary::Unknown);
        let mut legacy = run.clone();
        legacy.integrity_version = LEGACY_EXECUTION_LEDGER_INTEGRITY_VERSION.into();
        legacy.integrity_tag.clear();
        let mut json = serde_json::to_string(&legacy).expect("serialize legacy ledger shape");
        json = json.replace(",\"completion_authority\":null", "");
        let tag = ledger_hmac(json.as_bytes()).expect("legacy ledger tag");
        json = json.replace(
            "\"integrity_tag\":\"\"",
            &format!("\"integrity_tag\":\"{tag}\""),
        );
        assert!(!json.contains("record_version"));
        assert!(!json.contains("execution_boundary"));
        let restored = ExecutionRun::restore_json(&json).expect("restore legacy ledger shape");
        assert_eq!(restored.mission_id, run.mission_id);
        assert_eq!(restored.mission_revision, 1);
        assert_eq!(
            restored.attempts[0].execution_boundary,
            AttemptExecutionBoundary::Unknown
        );
        assert_eq!(
            restored.pre_execution_retry_task().as_deref(),
            Some("task-recovery")
        );
    }

    #[test]
    fn current_ledger_uses_explicit_record_version_and_rejects_unknown_versions() {
        let (_root, run, _authority, _store) = fixture(
            "versioned-ledger-shape",
            AttemptExecutionBoundary::NotStarted,
        );
        let json = run.snapshot_json().expect("serialize current ledger");
        assert!(json.contains(EXECUTION_LEDGER_RECORD_VERSION));
        let restored = ExecutionRun::restore_json(&json).expect("restore current ledger");
        assert_eq!(restored.run_id, run.run_id);

        let unknown = json.replace(EXECUTION_LEDGER_RECORD_VERSION, "p7-ledger-record-future");
        let error = ExecutionRun::restore_json(&unknown).expect_err("reject future ledger");
        assert!(error
            .to_string()
            .contains("unsupported execution ledger record version"));
    }

    #[test]
    fn current_record_accepts_a_matching_legacy_key_but_not_an_unrelated_key() {
        let (_root, run, _authority, _store) = fixture(
            "legacy-build-key-current-record",
            AttemptExecutionBoundary::NotStarted,
        );
        let mut unsigned = run.clone();
        unsigned.integrity_version = EXECUTION_LEDGER_INTEGRITY_VERSION.into();
        unsigned.integrity_tag.clear();
        let legacy_key = vec![0x41; 32];
        let expected = current_record_tag_with_key(&unsigned, &legacy_key)
            .expect("sign current-format record with old Beta key");

        assert!(current_record_authenticates_with_keys(
            &unsigned,
            &expected,
            &[0x99; 32],
            std::slice::from_ref(&legacy_key),
        )
        .expect("legacy key authenticates"));
        assert!(!current_record_authenticates_with_keys(
            &unsigned,
            &expected,
            &[0x99; 32],
            &[vec![0x55; 32]],
        )
        .expect("unrelated key rejected"));
    }

    #[test]
    fn legacy_record_accepts_a_matching_legacy_key_but_not_an_unrelated_key() {
        let (_root, run, _authority, _store) = fixture(
            "legacy-build-key-legacy-record",
            AttemptExecutionBoundary::Unknown,
        );
        let mut legacy = run.clone();
        legacy.integrity_version = LEGACY_EXECUTION_LEDGER_INTEGRITY_VERSION.into();
        legacy.integrity_tag.clear();
        let mut json = serde_json::to_string(&legacy).expect("serialize old record");
        json = json.replace(",\"completion_authority\":null", "");
        let legacy_key = vec![0x42; 32];
        let tag = ledger_hmac_with_key(json.as_bytes(), &legacy_key);
        json = json.replace(
            "\"integrity_tag\":\"\"",
            &format!("\"integrity_tag\":\"{tag}\""),
        );

        assert!(legacy_record_authenticates_with_keys(
            &json,
            &tag,
            &[0x98; 32],
            std::slice::from_ref(&legacy_key),
        )
        .expect("legacy key authenticates old record"));
        assert!(!legacy_record_authenticates_with_keys(
            &json,
            &tag,
            &[0x98; 32],
            &[vec![0x54; 32]],
        )
        .expect("unrelated key rejected"));
    }

    #[test]
    fn authenticated_historical_budget_boundary_is_recovered_without_fabricating_success() {
        let (_root, mut run, _authority, _store) = fixture(
            "historical-budget-boundary",
            AttemptExecutionBoundary::ExternalProcessStarted,
        );
        let task_id = "task-recovery";
        run.state = ExecutionRunState::BlockedExternal;
        run.tasks.get_mut(task_id).expect("fixture task").state =
            ExecutionTaskState::BlockedExternal;
        run.attempts[0].state = TaskAttemptState::Running;
        run.attempts[0].usage.wall_time_ms = run.tasks[task_id]
            .usage_budget
            .wall_clock_ms
            .saturating_add(1);
        run.leases.clear();
        run.emit(
            99,
            Some(task_id.into()),
            ExecutionEventKind::BudgetWarning,
            "historical budget boundary",
        )
        .expect("record budget boundary");
        let restored =
            ExecutionRun::restore_json(&run.snapshot_json().expect("serialize budget boundary"))
                .expect("restore authenticated historical boundary");
        assert_eq!(restored.state, ExecutionRunState::BlockedExternal);
        assert_eq!(restored.attempts[0].state, TaskAttemptState::Failed);
        assert!(restored.attempts[0].ended_at_ms.is_some());
        assert!(restored.attempts[0].termination_reason.is_some());
        assert!(restored.pre_execution_retry_task().is_none());
    }
}

fn canonical_hash<T: Serialize>(value: &T) -> Result<String, ExecutionError> {
    let bytes = serde_json::to_vec(value)
        .map_err(|error| ExecutionError::Canonicalization(error.to_string()))?;
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    Ok(format!("{:x}", hasher.finalize()))
}

fn worktree_identity(workspace: &Path) -> String {
    let canonical_path =
        std::fs::canonicalize(workspace).unwrap_or_else(|_| workspace.to_path_buf());
    let canonical = canonical_path.display().to_string();
    let git_marker = canonical_path.join(".git").display().to_string();
    let mut hasher = Sha256::new();
    hasher.update(format!("{canonical}\0{git_marker}").as_bytes());
    format!("{:x}", hasher.finalize())
}

#[derive(Debug, Deserialize)]
struct PersistedLedgerMarker {
    #[serde(default)]
    record_version: Option<String>,
}

#[derive(Serialize)]
struct CurrentLedgerEnvelope<'a> {
    record_version: &'static str,
    #[serde(flatten)]
    run: &'a ExecutionRun,
}

fn persisted_ledger_record_version(json: &str) -> Result<Option<String>, ExecutionError> {
    serde_json::from_str::<PersistedLedgerMarker>(json)
        .map(|marker| marker.record_version)
        .map_err(|error| ExecutionError::Ledger(error.to_string()))
}

fn ledger_tag(run: &ExecutionRun) -> Result<String, ExecutionError> {
    let bytes = serde_json::to_vec(&CurrentLedgerEnvelope {
        record_version: EXECUTION_LEDGER_RECORD_VERSION,
        run,
    })
    .map_err(|error| ExecutionError::Ledger(error.to_string()))?;
    ledger_hmac(&bytes)
}

fn ledger_hmac(bytes: &[u8]) -> Result<String, ExecutionError> {
    let key = ledger_key()?;
    Ok(ledger_hmac_with_key(bytes, &key))
}

fn ledger_hmac_with_key(bytes: &[u8], key: &[u8]) -> String {
    let mut ipad = [0x36_u8; 64];
    let mut opad = [0x5c_u8; 64];
    for (index, value) in key.iter().take(64).enumerate() {
        ipad[index] ^= value;
        opad[index] ^= value;
    }
    let mut inner = Sha256::new();
    inner.update(ipad);
    inner.update(bytes);
    let inner_digest = inner.finalize();
    let mut outer = Sha256::new();
    outer.update(opad);
    outer.update(inner_digest);
    format!("{:x}", outer.finalize())
}

fn atomic_replace(temporary: &Path, destination: &Path) -> Result<(), ExecutionError> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;

        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn MoveFileExW(existing: *const u16, replacement: *const u16, flags: u32) -> i32;
        }

        const MOVEFILE_REPLACE_EXISTING: u32 = 0x0000_0001;
        const MOVEFILE_WRITE_THROUGH: u32 = 0x0000_0008;
        let existing = temporary
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
        };
        if success == 0 {
            return Err(ExecutionError::Ledger(
                std::io::Error::last_os_error().to_string(),
            ));
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        fs::rename(temporary, destination)
            .map_err(|error| ExecutionError::Ledger(error.to_string()))
    }
}

const P7_KEYCHAIN_SERVICE: &str = "Relintor.P7.Execution";
const P7_KEYCHAIN_ACCOUNT: &str = "desktop-authority-v1";
const LEGACY_P7_KEY_FILENAME: &str = ".relintor-execution-ledger.key";
static P7_LEDGER_KEY: OnceLock<Result<Vec<u8>, String>> = OnceLock::new();

fn ledger_key() -> Result<Vec<u8>, ExecutionError> {
    match P7_LEDGER_KEY.get_or_init(|| {
        load_or_create_keychain_authority_key(P7_KEYCHAIN_SERVICE, P7_KEYCHAIN_ACCOUNT)
    }) {
        Ok(key) => Ok(key.clone()),
        Err(error) => Err(ExecutionError::Ledger(error.clone())),
    }
}

fn legacy_ledger_key_paths() -> Vec<PathBuf> {
    let mut candidates = Vec::new();

    // Explicit override is for controlled migration/recovery only.  It never
    // replaces the stable OS-keychain authority and is not used for writes.
    if let Some(value) = std::env::var_os("RELINTOR_P7_LEGACY_LEDGER_KEY_PATHS") {
        candidates.extend(std::env::split_paths(&value));
    }

    if let Some(target) = std::env::var_os("CARGO_TARGET_DIR") {
        candidates.push(PathBuf::from(target).join(LEGACY_P7_KEY_FILENAME));
    }

    if let Ok(current) = std::env::current_dir() {
        candidates.push(current.join("target").join(LEGACY_P7_KEY_FILENAME));
    }

    // Old Beta builds derived the P7 key from a relative `target` directory.
    // Walk executable ancestors so a newer binary can authenticate an older
    // ledger without guessing or replacing the new stable key.  This is a
    // read-only compatibility path; successful restore is immediately
    // re-signed by `restore_snapshot` with the OS-keychain authority.
    if let Ok(executable) = std::env::current_exe() {
        for ancestor in executable.ancestors().skip(1).take(8) {
            candidates.push(ancestor.join("target").join(LEGACY_P7_KEY_FILENAME));
        }
    }

    let mut seen = BTreeSet::new();
    candidates
        .into_iter()
        .filter(|path| seen.insert(path.clone()))
        .collect()
}

fn legacy_ledger_keys() -> Vec<Vec<u8>> {
    let mut seen = BTreeSet::new();
    legacy_ledger_key_paths()
        .into_iter()
        .filter_map(|path| fs::read(path).ok())
        .filter(|bytes| bytes.len() == 32)
        .filter(|bytes| seen.insert(bytes.clone()))
        .collect()
}

fn current_record_tag_with_key(run: &ExecutionRun, key: &[u8]) -> Result<String, ExecutionError> {
    let bytes = serde_json::to_vec(&CurrentLedgerEnvelope {
        record_version: EXECUTION_LEDGER_RECORD_VERSION,
        run,
    })
    .map_err(|error| ExecutionError::Ledger(error.to_string()))?;
    Ok(ledger_hmac_with_key(&bytes, key))
}

fn legacy_record_tag_with_key(
    json: &str,
    supplied: &str,
    key: &[u8],
) -> Result<String, ExecutionError> {
    let marker = format!("\"integrity_tag\":\"{supplied}\"");
    if supplied.is_empty() || json.matches(&marker).count() != 1 {
        return Err(ExecutionError::Ledger(
            "legacy execution ledger integrity field is malformed".into(),
        ));
    }
    let unsigned = json.replacen(&marker, "\"integrity_tag\":\"\"", 1);
    Ok(ledger_hmac_with_key(unsigned.as_bytes(), key))
}

fn current_record_authenticates_with_keys(
    run: &ExecutionRun,
    expected: &str,
    current_key: &[u8],
    legacy_keys: &[Vec<u8>],
) -> Result<bool, ExecutionError> {
    let mut unsigned = run.clone();
    unsigned.integrity_tag.clear();

    if current_record_tag_with_key(&unsigned, current_key)? == expected {
        return Ok(true);
    }

    for key in legacy_keys {
        if current_record_tag_with_key(&unsigned, key)? == expected {
            return Ok(true);
        }
    }
    Ok(false)
}

fn current_record_authenticates(
    run: &ExecutionRun,
    expected: &str,
) -> Result<bool, ExecutionError> {
    let current = ledger_key()?;
    let legacy = legacy_ledger_keys();
    current_record_authenticates_with_keys(run, expected, &current, &legacy)
}

fn legacy_record_authenticates_with_keys(
    json: &str,
    expected: &str,
    current_key: &[u8],
    legacy_keys: &[Vec<u8>],
) -> Result<bool, ExecutionError> {
    if legacy_record_tag_with_key(json, expected, current_key)? == expected {
        return Ok(true);
    }

    for key in legacy_keys {
        if legacy_record_tag_with_key(json, expected, key)? == expected {
            return Ok(true);
        }
    }
    Ok(false)
}

fn legacy_record_authenticates(json: &str, expected: &str) -> Result<bool, ExecutionError> {
    let current = ledger_key()?;
    let legacy = legacy_ledger_keys();
    legacy_record_authenticates_with_keys(json, expected, &current, &legacy)
}

#[derive(Debug)]
pub enum ExecutionError {
    RevalidationRequired(String),
    AuthorityStale,
    UnknownTask(String),
    DependencyNotReady(String),
    TaskNotRunning(String),
    TaskDrift,
    BudgetExhausted,
    LeaseInactive,
    LeaseExpired,
    LeaseTampered,
    LeaseBindingMismatch,
    StaleTaskPacket,
    AttemptMissing(String),
    PolicyDenied(String),
    SafeBoundaryRequired,
    ContinuationNotAvailable,
    Adapter(BridgeError),
    Ledger(String),
    Canonicalization(String),
}

impl fmt::Display for ExecutionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RevalidationRequired(reason) => write!(f, "REVALIDATION_REQUIRED: {reason}"),
            Self::AuthorityStale => write!(f, "REVALIDATION_REQUIRED: authority is stale"),
            Self::UnknownTask(task) => write!(f, "unknown execution task: {task}"),
            Self::DependencyNotReady(task) => write!(f, "dependency not ready for task: {task}"),
            Self::TaskNotRunning(task) => write!(f, "task is not running: {task}"),
            Self::TaskDrift => write!(f, "task drift denied before mutation"),
            Self::BudgetExhausted => write!(f, "BUDGET_EXHAUSTED"),
            Self::LeaseInactive => write!(f, "lease is not active"),
            Self::LeaseExpired => write!(f, "lease expired"),
            Self::LeaseTampered => write!(f, "lease integrity check failed"),
            Self::LeaseBindingMismatch => write!(f, "lease/task packet binding mismatch"),
            Self::StaleTaskPacket => write!(f, "stale task packet"),
            Self::AttemptMissing(task) => write!(f, "running attempt missing for task: {task}"),
            Self::PolicyDenied(reason) => write!(f, "POLICY_DENIED: {reason}"),
            Self::SafeBoundaryRequired => write!(f, "safe boundary was not requested"),
            Self::ContinuationNotAvailable => write!(f, "clean continuation is not available"),
            Self::Adapter(error) => write!(f, "Antigravity adapter: {error}"),
            Self::Ledger(reason) => write!(f, "execution ledger: {reason}"),
            Self::Canonicalization(reason) => write!(f, "canonicalization: {reason}"),
        }
    }
}

impl std::error::Error for ExecutionError {}

impl From<BridgeError> for ExecutionError {
    fn from(error: BridgeError) -> Self {
        Self::Adapter(error)
    }
}
