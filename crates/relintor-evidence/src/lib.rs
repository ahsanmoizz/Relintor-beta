//! Rust-owned evidence and completion authority for P8.
//!
//! Evidence is content addressed, scoped to the sealed P6/P7 identity, and
//! authenticated with a caller-supplied local key.  The renderer and builder
//! agent have no APIs here that can mark evidence valid or issue a certificate.

use hmac::{Hmac, Mac};
use relintor_execution::{
    ExecutionRun, ExecutionRunState, ExecutionTaskState, SuccessfulExecutionIdentity,
    TaskAttemptState,
};
use relintor_standards::{
    AuthorityEngine, DecisionActor, DecisionKind, EvidenceClass, EvidenceConfidence,
    ExecutionHandoff, ExplicitDecision, MissionRevision, Requirement, RequirementStatus,
    StandardsRegistry, TrustedSignerSet,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use walkdir::WalkDir;

pub use relintor_standards::{
    AcceptanceCriterion, EvidenceObligation, RequirementGraph, RequirementRisk, VerificationPolicy,
};

const STORE_VERSION: &str = "p8-evidence-store-v1";
const MANIFEST_VERSION: &str = "p8-evidence-manifest-v1";
const CERTIFICATE_VERSION: &str = "p8-completion-certificate-v1";
const INVALIDATION_INDEX_VERSION: &str = "p8-invalidation-index-v1";
const MAX_PATH_COMPONENTS: usize = 128;
type HmacSha256 = Hmac<Sha256>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum EvidenceError {
    Io(String),
    Serialization(String),
    InvalidInput(String),
    InvalidDigest(String),
    IntegrityFailure(String),
    EvidenceNotFound(String),
    StaleEvidence(String),
    InvalidAuthority(String),
    InvalidCertificate(String),
    CollectorUnavailable(String),
}

impl fmt::Display for EvidenceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(value) => write!(f, "evidence I/O error: {value}"),
            Self::Serialization(value) => write!(f, "evidence serialization error: {value}"),
            Self::InvalidInput(value) => write!(f, "invalid evidence input: {value}"),
            Self::InvalidDigest(value) => write!(f, "invalid evidence digest: {value}"),
            Self::IntegrityFailure(value) => write!(f, "evidence integrity failure: {value}"),
            Self::EvidenceNotFound(value) => write!(f, "evidence not found: {value}"),
            Self::StaleEvidence(value) => write!(f, "stale evidence: {value}"),
            Self::InvalidAuthority(value) => write!(f, "invalid verification authority: {value}"),
            Self::InvalidCertificate(value) => write!(f, "invalid completion certificate: {value}"),
            Self::CollectorUnavailable(value) => write!(f, "collector unavailable: {value}"),
        }
    }
}

impl std::error::Error for EvidenceError {}

fn io_error(error: impl fmt::Display) -> EvidenceError {
    EvidenceError::Io(error.to_string())
}

fn json_error(error: impl fmt::Display) -> EvidenceError {
    EvidenceError::Serialization(error.to_string())
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(0)
}

pub fn sha256(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn canonical<T: Serialize + ?Sized>(value: &T) -> Result<Vec<u8>, EvidenceError> {
    serde_json::to_vec(value).map_err(json_error)
}

fn hmac_hex(key: &[u8], bytes: &[u8]) -> Result<String, EvidenceError> {
    let mut mac = HmacSha256::new_from_slice(key)
        .map_err(|_| EvidenceError::InvalidInput("local authority key is empty".into()))?;
    mac.update(bytes);
    Ok(format!("{:x}", mac.finalize().into_bytes()))
}

fn constant_time_equal(left: &str, right: &str) -> bool {
    let left = left.as_bytes();
    let right = right.as_bytes();
    let mut difference = left.len() ^ right.len();
    for index in 0..left.len().max(right.len()) {
        difference |= usize::from(left.get(index).copied().unwrap_or_default())
            ^ usize::from(right.get(index).copied().unwrap_or_default());
    }
    difference == 0
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EvidenceFreshness {
    Fresh,
    Stale,
    Invalidated,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EvidenceResult {
    Pass,
    Fail,
    Skipped,
    NotRun,
    Blocked,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum GateStatus {
    Pass,
    Fail,
    NotAvailable,
    NotApplicable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CollectorIdentity {
    pub name: String,
    pub version: String,
}

impl CollectorIdentity {
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version: version.into(),
        }
    }
}

/// Sealed criterion-to-probe provenance. A criterion is only claimable when a
/// collector is bound to this exact criterion and the probe identity that will
/// actually execute; evidence-class compatibility alone is insufficient.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CriterionVerificationPlan {
    pub mission_id: String,
    pub mission_revision: u64,
    pub project_id: String,
    pub p6_seal_hash: String,
    pub requirement_id: String,
    pub criterion_id: String,
    pub evidence_class: EvidenceClass,
    pub collector_identity: CollectorIdentity,
    pub probe_identity: String,
    pub command_digest: String,
    pub verification_plan_authority_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct EnvironmentFingerprint {
    pub os: String,
    pub architecture: String,
    pub tool_versions: BTreeMap<String, String>,
    pub workspace_root: String,
    pub workspace_fingerprint: String,
    pub source_revision: Option<String>,
    pub dependency_lock_hashes: BTreeMap<String, String>,
    pub mission_id: String,
    pub mission_revision: u64,
    pub p6_seal_hash: String,
    pub registry_id: String,
    pub registry_version: u64,
    pub registry_digest: String,
}

impl EnvironmentFingerprint {
    pub fn digest(&self) -> Result<String, EvidenceError> {
        Ok(sha256(&canonical(self)?))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FreshnessContext {
    pub mission_id: String,
    pub mission_revision: u64,
    pub p6_seal_hash: String,
    pub workspace_fingerprint: String,
    pub source_revision: Option<String>,
    pub environment_fingerprint: String,
    pub dependency_lock_hashes: BTreeMap<String, String>,
    pub workspace_root: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TestInventoryEntry {
    pub test_id: String,
    pub source_path: String,
    pub source_hash: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidenceMetadata {
    pub evidence_id: String,
    pub class: EvidenceClass,
    pub mission_id: String,
    pub mission_revision: u64,
    pub p6_seal_hash: String,
    pub requirement_ids: Vec<String>,
    pub task_id: Option<String>,
    pub p7_attempt: Option<u32>,
    #[serde(default)]
    pub execution_identities: Vec<SuccessfulExecutionIdentity>,
    pub workspace_fingerprint: String,
    pub source_revision: Option<String>,
    pub collector: CollectorIdentity,
    pub command_digest: String,
    pub environment_fingerprint: String,
    pub created_at_ms: u64,
    pub artifact_digest: String,
    pub confidence: EvidenceConfidence,
    pub freshness: EvidenceFreshness,
    pub result: EvidenceResult,
    pub required: bool,
    pub accepted_criteria: BTreeSet<String>,
    pub relevant_paths: BTreeSet<String>,
    pub test_inventory: Vec<TestInventoryEntry>,
    pub dependency_lock_hashes: BTreeMap<String, String>,
    #[serde(default)]
    pub scope_fingerprint: Option<String>,
}

impl EvidenceMetadata {
    pub fn validate(&self) -> Result<(), EvidenceError> {
        if self.evidence_id.trim().is_empty() {
            return Err(EvidenceError::InvalidInput("evidence ID is empty".into()));
        }
        if self.mission_id.trim().is_empty() || self.p6_seal_hash.trim().is_empty() {
            return Err(EvidenceError::InvalidInput(
                "mission identity and P6 seal hash are required".into(),
            ));
        }
        if self.requirement_ids.iter().any(|id| id.trim().is_empty()) {
            return Err(EvidenceError::InvalidInput(
                "requirement IDs cannot be empty".into(),
            ));
        }
        if self.execution_identities.is_empty()
            || self.execution_identities.iter().any(|identity| {
                identity.mission_id != self.mission_id
                    || identity.mission_revision != self.mission_revision
                    || identity.seal_hash != self.p6_seal_hash
                    || self.task_id.as_deref() != Some(identity.task_id.as_str())
                    || self.p7_attempt != Some(identity.attempt_number)
                    || identity.attempt_id.trim().is_empty()
                    || identity.lease_id.trim().is_empty()
                    || identity.process_digest.len() != 64
                    || identity.ended_at_ms >= identity.lease_expires_at_ms
                    || self.created_at_ms < identity.ended_at_ms
            })
        {
            return Err(EvidenceError::InvalidAuthority(
                "evidence is not bound to an exact successful P7 attempt and lease".into(),
            ));
        }
        if self.artifact_digest.len() != 64
            || !self.artifact_digest.chars().all(|c| c.is_ascii_hexdigit())
        {
            return Err(EvidenceError::InvalidDigest(
                "artifact digest is not SHA-256".into(),
            ));
        }
        Ok(())
    }
}

/// A receipt is the only production input accepted by [`EvidenceStore::put`].
/// Its authority fields are minted by a Rust collector after an actual probe
/// or process result, not supplied by the renderer or builder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CollectorReceipt {
    evidence_id: String,
    mission_id: String,
    mission_revision: u64,
    p6_seal_hash: String,
    requirement_ids: Vec<String>,
    task_id: Option<String>,
    p7_attempt: Option<u32>,
    execution_identities: Vec<SuccessfulExecutionIdentity>,
    workspace_fingerprint: String,
    source_revision: Option<String>,
    collector: CollectorIdentity,
    collector_kind: String,
    command_digest: String,
    environment_fingerprint: String,
    created_at_ms: u64,
    result: EvidenceResult,
    confidence: EvidenceConfidence,
    class: EvidenceClass,
    required: bool,
    accepted_criteria: BTreeSet<String>,
    #[serde(default)]
    criterion_provenance: Vec<CriterionVerificationPlan>,
    relevant_paths: BTreeSet<String>,
    test_inventory: Vec<TestInventoryEntry>,
    dependency_lock_hashes: BTreeMap<String, String>,
    scope_fingerprint: Option<String>,
    actual_process_digest: String,
    authority_binding: String,
    integrity: String,
}

impl CollectorReceipt {
    fn signing_body(&self) -> Result<Vec<u8>, EvidenceError> {
        let mut unsigned = self.clone();
        unsigned.integrity.clear();
        canonical(&unsigned)
    }

    fn validate(&self) -> Result<(), EvidenceError> {
        if self.evidence_id.trim().is_empty()
            || self.collector.name.trim().is_empty()
            || self.collector_kind.trim().is_empty()
            || self.actual_process_digest.len() != 64
            || self.authority_binding.trim().is_empty()
            || self.execution_identities.is_empty()
        {
            return Err(EvidenceError::InvalidInput(
                "collector receipt is incomplete".into(),
            ));
        }
        if self.criterion_provenance.iter().any(|provenance| {
            !self.accepted_criteria.contains(&provenance.criterion_id)
                || provenance.mission_id != self.mission_id
                || provenance.mission_revision != self.mission_revision
                || provenance.p6_seal_hash != self.p6_seal_hash
                || provenance.requirement_id.trim().is_empty()
                || provenance.probe_identity.trim().is_empty()
                || provenance.command_digest != self.command_digest
                || provenance
                    .verification_plan_authority_digest
                    .trim()
                    .is_empty()
                || provenance.collector_identity != self.collector
        }) {
            return Err(EvidenceError::InvalidAuthority(
                "collector receipt criterion provenance does not match the executed collector"
                    .into(),
            ));
        }
        if self.integrity != sha256(&self.signing_body()?) {
            return Err(EvidenceError::IntegrityFailure(
                "collector receipt integrity proof mismatch".into(),
            ));
        }
        Ok(())
    }

    fn into_metadata(self, bytes: &[u8]) -> Result<EvidenceMetadata, EvidenceError> {
        self.validate()?;
        let actual_result = self.result;
        let actual_confidence = self.confidence;
        let actual_class = self.class;
        Ok(EvidenceMetadata {
            evidence_id: self.evidence_id,
            class: actual_class,
            mission_id: self.mission_id,
            mission_revision: self.mission_revision,
            p6_seal_hash: self.p6_seal_hash,
            requirement_ids: self.requirement_ids,
            task_id: self.task_id,
            p7_attempt: self.p7_attempt,
            execution_identities: self.execution_identities,
            workspace_fingerprint: self.workspace_fingerprint,
            source_revision: self.source_revision,
            collector: self.collector,
            command_digest: self.command_digest,
            environment_fingerprint: self.environment_fingerprint,
            created_at_ms: self.created_at_ms,
            artifact_digest: sha256(bytes),
            confidence: actual_confidence,
            freshness: EvidenceFreshness::Fresh,
            result: actual_result,
            required: self.required,
            accepted_criteria: self.accepted_criteria,
            relevant_paths: self.relevant_paths,
            test_inventory: self.test_inventory,
            dependency_lock_hashes: self.dependency_lock_hashes,
            scope_fingerprint: self.scope_fingerprint,
        })
    }
}

/// Development-only fixture support. This module is enabled for the
/// evidence crate's test profile and is explicitly disabled by the desktop
/// production dependency. Production callers can only store collector
/// receipts produced by the typed collector boundaries.
#[cfg(feature = "test-support")]
pub mod test_support {
    use super::*;

    pub fn fixture_receipt(
        mut metadata: EvidenceMetadata,
        bytes: &[u8],
    ) -> Result<CollectorReceipt, EvidenceError> {
        if metadata.execution_identities.is_empty() {
            let digest = sha256(
                format!(
                    "{}:{}:{}",
                    metadata.mission_id, metadata.mission_revision, metadata.evidence_id
                )
                .as_bytes(),
            );
            let task_id = metadata
                .task_id
                .clone()
                .unwrap_or_else(|| "test-fixture-task".into());
            let attempt_number = metadata.p7_attempt.unwrap_or(1);
            metadata.task_id = Some(task_id.clone());
            metadata.p7_attempt = Some(attempt_number);
            metadata.execution_identities = vec![SuccessfulExecutionIdentity {
                mission_id: metadata.mission_id.clone(),
                mission_revision: metadata.mission_revision,
                seal_hash: metadata.p6_seal_hash.clone(),
                run_id: "test-fixture-run".into(),
                task_id,
                attempt_id: format!("test-fixture-attempt-{digest}"),
                attempt_number,
                packet_digest: digest.clone(),
                lease_id: format!("test-fixture-lease-{digest}"),
                lease_digest: digest.clone(),
                process_digest: digest.clone(),
                workspace_identity: digest.clone(),
                workspace_before_fingerprint: digest.clone(),
                workspace_after_fingerprint: digest,
                artifact_changes: Vec::new(),
                started_at_ms: 1,
                ended_at_ms: 2,
                lease_expires_at_ms: 3,
            }];
            metadata.created_at_ms = metadata.created_at_ms.max(3);
        }
        let mut receipt = CollectorReceipt {
            evidence_id: metadata.evidence_id,
            mission_id: metadata.mission_id,
            mission_revision: metadata.mission_revision,
            p6_seal_hash: metadata.p6_seal_hash,
            requirement_ids: metadata.requirement_ids,
            task_id: metadata.task_id,
            p7_attempt: metadata.p7_attempt,
            execution_identities: metadata.execution_identities,
            workspace_fingerprint: metadata.workspace_fingerprint,
            source_revision: metadata.source_revision,
            collector: CollectorIdentity::new("test-support-collector", "p8-test-only"),
            collector_kind: "TEST_SUPPORT_ONLY".into(),
            command_digest: metadata.command_digest,
            environment_fingerprint: metadata.environment_fingerprint,
            created_at_ms: metadata.created_at_ms,
            result: metadata.result,
            confidence: metadata.confidence,
            class: metadata.class,
            required: metadata.required,
            accepted_criteria: metadata.accepted_criteria,
            criterion_provenance: Vec::new(),
            relevant_paths: metadata.relevant_paths,
            test_inventory: metadata.test_inventory,
            dependency_lock_hashes: metadata.dependency_lock_hashes,
            scope_fingerprint: metadata.scope_fingerprint,
            actual_process_digest: sha256(bytes),
            authority_binding: "TEST_SUPPORT_AUTHORITY_ONLY".into(),
            integrity: String::new(),
        };
        receipt.integrity = sha256(&receipt.signing_body()?);
        Ok(receipt)
    }

    pub fn put_fixture(
        store: &EvidenceStore,
        metadata: EvidenceMetadata,
        bytes: &[u8],
    ) -> Result<EvidenceArtifact, EvidenceError> {
        store.put(fixture_receipt(metadata, bytes)?, bytes)
    }

    pub trait FixtureStoreExt {
        fn put_test_fixture(
            &self,
            metadata: EvidenceMetadata,
            bytes: &[u8],
        ) -> Result<EvidenceArtifact, EvidenceError>;
    }

    impl FixtureStoreExt for EvidenceStore {
        fn put_test_fixture(
            &self,
            metadata: EvidenceMetadata,
            bytes: &[u8],
        ) -> Result<EvidenceArtifact, EvidenceError> {
            put_fixture(self, metadata, bytes)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidenceArtifact {
    pub store_version: String,
    pub metadata: EvidenceMetadata,
    pub digest: String,
    pub byte_len: u64,
    pub integrity_tag: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredEvidence {
    pub artifact: EvidenceArtifact,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct EvidenceEnvelope {
    pub artifact: EvidenceArtifact,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct InvalidationIndex {
    version: String,
    records: BTreeMap<String, String>,
    state_digest: String,
    integrity: String,
}

#[derive(Debug, Clone)]
pub struct EvidenceStore {
    root: PathBuf,
    key: Arc<Vec<u8>>,
}

impl EvidenceStore {
    pub fn new(root: impl Into<PathBuf>, key: &[u8]) -> Result<Self, EvidenceError> {
        if key.is_empty() {
            return Err(EvidenceError::InvalidInput(
                "evidence store requires a non-empty local authority key".into(),
            ));
        }
        let root = root.into();
        fs::create_dir_all(root.join("blobs")).map_err(io_error)?;
        fs::create_dir_all(root.join("metadata")).map_err(io_error)?;
        fs::create_dir_all(root.join("invalidations")).map_err(io_error)?;
        let store = Self {
            root,
            key: Arc::new(key.to_vec()),
        };
        let index_path = store.root.join("invalidations").join("index.json");
        if index_path.exists() {
            store.load_invalidation_index()?;
        } else {
            let stray = fs::read_dir(store.root.join("invalidations"))
                .map_err(io_error)?
                .filter_map(Result::ok)
                .any(|entry| {
                    entry.path().is_file() && !entry.file_name().to_string_lossy().starts_with('.')
                });
            if stray {
                return Err(EvidenceError::IntegrityFailure(
                    "invalidation index is missing while revocation records exist".into(),
                ));
            }
            store.write_invalidation_index(&BTreeMap::new())?;
        }
        Ok(store)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub(crate) fn authority_key(&self) -> &[u8] {
        self.key.as_slice()
    }

    fn evidence_bytes(metadata: &EvidenceMetadata, bytes: &[u8]) -> Result<Vec<u8>, EvidenceError> {
        let mut canonical_metadata = canonical(metadata)?;
        canonical_metadata.extend_from_slice(bytes);
        Ok(canonical_metadata)
    }

    fn paths(
        &self,
        evidence_id: &str,
        digest: &str,
    ) -> Result<(PathBuf, PathBuf, PathBuf), EvidenceError> {
        if evidence_id.is_empty()
            || evidence_id.contains(['/', '\\'])
            || evidence_id.contains("..")
            || digest.len() != 64
            || !digest.chars().all(|c| c.is_ascii_hexdigit())
        {
            return Err(EvidenceError::InvalidInput(
                "unsafe evidence path component".into(),
            ));
        }
        Ok((
            self.root.join("blobs").join(digest),
            self.root
                .join("metadata")
                .join(format!("{evidence_id}.json")),
            self.root
                .join("invalidations")
                .join(format!("{evidence_id}.json")),
        ))
    }

    pub fn put(
        &self,
        receipt: CollectorReceipt,
        bytes: &[u8],
    ) -> Result<EvidenceArtifact, EvidenceError> {
        let mut metadata = receipt.into_metadata(bytes)?;
        metadata.freshness = EvidenceFreshness::Fresh;
        metadata.validate()?;
        let digest = sha256(&Self::evidence_bytes(&metadata, bytes)?);
        let integrity_tag = hmac_hex(
            &self.key,
            &[digest.as_bytes(), canonical(&metadata)?.as_slice(), bytes].concat(),
        )?;
        let artifact = EvidenceArtifact {
            store_version: STORE_VERSION.into(),
            metadata,
            digest: digest.clone(),
            byte_len: bytes.len() as u64,
            integrity_tag,
        };
        let (blob_path, metadata_path, invalidation_path) =
            self.paths(&artifact.metadata.evidence_id, &digest)?;
        let index = self.load_invalidation_index()?;
        if index.records.contains_key(&artifact.metadata.evidence_id) || invalidation_path.exists()
        {
            return Err(EvidenceError::IntegrityFailure(
                "a revoked evidence ID cannot be reused".into(),
            ));
        }
        if !blob_path.exists() {
            write_new_file(&blob_path, bytes)?;
        } else {
            let existing = fs::read(&blob_path).map_err(io_error)?;
            if existing != bytes {
                return Err(EvidenceError::IntegrityFailure(
                    "content-addressed blob collision".into(),
                ));
            }
        }
        let envelope = EvidenceEnvelope {
            artifact: artifact.clone(),
        };
        if metadata_path.exists() {
            let existing = self.load(&artifact.metadata.evidence_id)?;
            if existing.artifact == artifact && existing.bytes == bytes {
                return Ok(artifact);
            }
            return Err(EvidenceError::IntegrityFailure(
                "an evidence ID cannot be rebound to another artifact or execution".into(),
            ));
        }
        write_atomic_file(&metadata_path, &canonical(&envelope)?)?;
        Ok(artifact)
    }

    pub fn load(&self, evidence_id: &str) -> Result<StoredEvidence, EvidenceError> {
        let metadata_path = self
            .root
            .join("metadata")
            .join(format!("{evidence_id}.json"));
        if !metadata_path.is_file() {
            return Err(EvidenceError::EvidenceNotFound(evidence_id.into()));
        }
        if self.is_revoked(evidence_id)? {
            return Err(EvidenceError::EvidenceNotFound(format!(
                "{evidence_id} was invalidated"
            )));
        }
        let envelope: EvidenceEnvelope =
            serde_json::from_slice(&fs::read(metadata_path).map_err(io_error)?)
                .map_err(json_error)?;
        let artifact = envelope.artifact;
        let (blob_path, _, _) = self.paths(&artifact.metadata.evidence_id, &artifact.digest)?;
        let bytes = fs::read(blob_path).map_err(io_error)?;
        self.validate_artifact(&artifact, &bytes)?;
        Ok(StoredEvidence { artifact, bytes })
    }

    pub fn validate_artifact(
        &self,
        artifact: &EvidenceArtifact,
        bytes: &[u8],
    ) -> Result<(), EvidenceError> {
        artifact.metadata.validate()?;
        if artifact.store_version != STORE_VERSION {
            return Err(EvidenceError::IntegrityFailure(
                "unsupported evidence store version".into(),
            ));
        }
        if artifact.byte_len != bytes.len() as u64
            || artifact.metadata.artifact_digest != sha256(bytes)
        {
            return Err(EvidenceError::InvalidDigest(
                artifact.metadata.evidence_id.clone(),
            ));
        }
        let expected_digest = sha256(&Self::evidence_bytes(&artifact.metadata, bytes)?);
        if !constant_time_equal(&artifact.digest, &expected_digest) {
            return Err(EvidenceError::InvalidDigest(
                artifact.metadata.evidence_id.clone(),
            ));
        }
        let expected_tag = hmac_hex(
            &self.key,
            &[
                artifact.digest.as_bytes(),
                canonical(&artifact.metadata)?.as_slice(),
                bytes,
            ]
            .concat(),
        )?;
        if !constant_time_equal(&artifact.integrity_tag, &expected_tag) {
            return Err(EvidenceError::IntegrityFailure(
                artifact.metadata.evidence_id.clone(),
            ));
        }
        Ok(())
    }

    pub fn invalidate(
        &self,
        evidence_id: &str,
        reason: impl Into<String>,
    ) -> Result<(), EvidenceError> {
        if !self
            .root
            .join("metadata")
            .join(format!("{evidence_id}.json"))
            .is_file()
        {
            return Err(EvidenceError::EvidenceNotFound(evidence_id.into()));
        }
        let index = self.load_invalidation_index()?;
        if index.records.contains_key(evidence_id) {
            return Err(EvidenceError::IntegrityFailure(
                "evidence is already permanently invalidated".into(),
            ));
        }
        let mut record = EvidenceInvalidation {
            evidence_id: evidence_id.into(),
            reason: reason.into(),
            invalidated_at_ms: now_ms(),
            sequence: index.records.len() as u64 + 1,
            previous_digest: index.records.values().next_back().cloned(),
            integrity: String::new(),
        };
        record.integrity = hmac_hex(&self.key, &canonical(&record.signing_body())?)?;
        let path = self
            .root
            .join("invalidations")
            .join(format!("{evidence_id}.json"));
        write_atomic_file(&path, &canonical(&record)?)?;
        let mut records = index.records;
        records.insert(evidence_id.into(), sha256(&canonical(&record)?));
        self.write_invalidation_index(&records)
    }

    pub fn list(&self) -> Result<Vec<EvidenceArtifact>, EvidenceError> {
        let mut artifacts = Vec::new();
        for entry in fs::read_dir(self.root.join("metadata")).map_err(io_error)? {
            let entry = entry.map_err(io_error)?;
            if !entry.path().is_file() || entry.file_name().to_string_lossy().starts_with('.') {
                continue;
            }
            let evidence_id = entry
                .path()
                .file_stem()
                .and_then(|value| value.to_str())
                .ok_or_else(|| EvidenceError::IntegrityFailure("invalid evidence filename".into()))?
                .to_owned();
            artifacts.push(self.load(&evidence_id)?.artifact);
        }
        artifacts.sort_by(|left, right| left.metadata.evidence_id.cmp(&right.metadata.evidence_id));
        Ok(artifacts)
    }

    pub fn freshness(
        &self,
        evidence_id: &str,
        current: &FreshnessContext,
    ) -> Result<EvidenceFreshness, EvidenceError> {
        if self.is_revoked(evidence_id)? {
            return Ok(EvidenceFreshness::Invalidated);
        }
        let stored = self.load(evidence_id)?;
        let metadata = &stored.artifact.metadata;
        if metadata.mission_id != current.mission_id
            || metadata.mission_revision != current.mission_revision
            || metadata.p6_seal_hash != current.p6_seal_hash
            || metadata.environment_fingerprint != current.environment_fingerprint
        {
            return Ok(EvidenceFreshness::Stale);
        }
        if metadata.dependency_lock_hashes != current.dependency_lock_hashes {
            return Ok(EvidenceFreshness::Stale);
        }
        let global_scope_matches = metadata.workspace_fingerprint == current.workspace_fingerprint
            && metadata.source_revision == current.source_revision;
        let scoped_scope_matches = if !metadata.relevant_paths.is_empty() {
            current
                .workspace_root
                .as_ref()
                .zip(metadata.scope_fingerprint.as_ref())
                .map(|(root, expected)| {
                    scoped_fingerprint(root, &metadata.relevant_paths)
                        .map(|actual| &actual == expected)
                })
                .transpose()?
                .unwrap_or(false)
        } else {
            false
        };
        if !global_scope_matches && !scoped_scope_matches {
            return Ok(EvidenceFreshness::Stale);
        }
        if let Some(root) = &current.workspace_root {
            for test in &metadata.test_inventory {
                let path = root.join(&test.source_path);
                if !test.enabled || !path.is_file() || hash_file(&path)? != test.source_hash {
                    return Ok(EvidenceFreshness::Stale);
                }
            }
        }
        Ok(EvidenceFreshness::Fresh)
    }

    pub fn impacted_by_change(
        &self,
        evidence_id: &str,
        change: &WorkspaceChangeSet,
    ) -> Result<bool, EvidenceError> {
        let artifact = self.load(evidence_id)?.artifact;
        if change.sealed_authority_changed {
            return Ok(true);
        }
        if change.lockfile_changed
            && matches!(
                artifact.metadata.class,
                EvidenceClass::BuildOutput
                    | EvidenceClass::TestOutput
                    | EvidenceClass::LintStaticAnalysis
                    | EvidenceClass::BrowserRecording
                    | EvidenceClass::SecurityScan
            )
        {
            return Ok(true);
        }
        if change.changed_paths.is_empty() {
            return Ok(false);
        }
        let relevant = &artifact.metadata.relevant_paths;
        if relevant.is_empty() {
            return Ok(matches!(
                artifact.metadata.class,
                EvidenceClass::BuildOutput
                    | EvidenceClass::TestOutput
                    | EvidenceClass::BrowserRecording
                    | EvidenceClass::SecurityScan
            ));
        }
        Ok(change
            .changed_paths
            .iter()
            .any(|path| relevant.contains(path)))
    }

    pub fn validate_manifest(&self, manifest: &EvidenceManifest) -> Result<(), EvidenceError> {
        for summary in &manifest.evidence {
            let stored = self.load(&summary.evidence_id)?;
            if stored.artifact.digest != summary.digest
                || stored.artifact.metadata.class != summary.class
                || stored.artifact.metadata.result != summary.result
                || stored.artifact.metadata.confidence != summary.confidence
            {
                return Err(EvidenceError::IntegrityFailure(format!(
                    "manifest reference mismatch for {}",
                    summary.evidence_id
                )));
            }
        }
        Ok(())
    }

    pub fn invalidations(&self) -> Result<Vec<EvidenceInvalidation>, EvidenceError> {
        let index = self.load_invalidation_index()?;
        let mut records = Vec::new();
        for evidence_id in index.records.keys() {
            let path = self
                .root
                .join("invalidations")
                .join(format!("{evidence_id}.json"));
            if !path.is_file() {
                return Err(EvidenceError::IntegrityFailure(
                    "invalidation record is missing from authenticated index".into(),
                ));
            }
            let record: EvidenceInvalidation =
                serde_json::from_slice(&fs::read(path).map_err(io_error)?).map_err(json_error)?;
            self.validate_invalidation(&record)?;
            if index.records.get(evidence_id) != Some(&sha256(&canonical(&record)?)) {
                return Err(EvidenceError::IntegrityFailure(
                    "invalidation index entry does not match record".into(),
                ));
            }
            records.push(record);
        }
        records.sort_by(
            |left: &EvidenceInvalidation, right: &EvidenceInvalidation| {
                left.evidence_id.cmp(&right.evidence_id)
            },
        );
        Ok(records)
    }

    fn invalidation_index_body(
        records: &BTreeMap<String, String>,
    ) -> Result<(String, BTreeMap<String, String>, String), EvidenceError> {
        let state_digest = sha256(&canonical(records)?);
        Ok((
            INVALIDATION_INDEX_VERSION.into(),
            records.clone(),
            state_digest,
        ))
    }

    fn load_invalidation_index(&self) -> Result<InvalidationIndex, EvidenceError> {
        let path = self.root.join("invalidations").join("index.json");
        let index: InvalidationIndex =
            serde_json::from_slice(&fs::read(path).map_err(io_error)?).map_err(json_error)?;
        let expected_integrity = hmac_hex(
            &self.key,
            &canonical(&(
                index.version.clone(),
                index.records.clone(),
                index.state_digest.clone(),
            ))?,
        )?;
        if index.version != INVALIDATION_INDEX_VERSION
            || index.state_digest != sha256(&canonical(&index.records)?)
            || !constant_time_equal(&index.integrity, &expected_integrity)
        {
            return Err(EvidenceError::IntegrityFailure(
                "invalidation index integrity verification failed".into(),
            ));
        }
        for entry in fs::read_dir(self.root.join("invalidations")).map_err(io_error)? {
            let entry = entry.map_err(io_error)?;
            if entry.path().is_file()
                && entry.file_name() != "index.json"
                && !entry.file_name().to_string_lossy().starts_with('.')
            {
                let invalidation_path = entry.path();
                let evidence_id = invalidation_path
                    .file_stem()
                    .and_then(|value| value.to_str())
                    .unwrap_or_default()
                    .to_owned();
                if !index.records.contains_key(&evidence_id) {
                    return Err(EvidenceError::IntegrityFailure(
                        "unindexed invalidation record detected".into(),
                    ));
                }
            }
        }
        Ok(index)
    }

    fn write_invalidation_index(
        &self,
        records: &BTreeMap<String, String>,
    ) -> Result<(), EvidenceError> {
        let (version, records, state_digest) = Self::invalidation_index_body(records)?;
        let integrity = hmac_hex(
            &self.key,
            &canonical(&(version.clone(), records.clone(), state_digest.clone()))?,
        )?;
        write_atomic_file(
            &self.root.join("invalidations").join("index.json"),
            &canonical(&InvalidationIndex {
                version,
                records,
                state_digest,
                integrity,
            })?,
        )
    }

    fn validate_invalidation(&self, record: &EvidenceInvalidation) -> Result<(), EvidenceError> {
        let expected = hmac_hex(&self.key, &canonical(&record.signing_body())?)?;
        if !constant_time_equal(&record.integrity, &expected) {
            return Err(EvidenceError::IntegrityFailure(
                "invalidation record integrity verification failed".into(),
            ));
        }
        Ok(())
    }

    fn is_revoked(&self, evidence_id: &str) -> Result<bool, EvidenceError> {
        Ok(self
            .load_invalidation_index()?
            .records
            .contains_key(evidence_id))
    }
}

fn write_new_file(path: &Path, bytes: &[u8]) -> Result<(), EvidenceError> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(io_error)?;
    file.write_all(bytes).map_err(io_error)?;
    file.sync_all().map_err(io_error)
}

fn write_atomic_file(path: &Path, bytes: &[u8]) -> Result<(), EvidenceError> {
    let parent = path
        .parent()
        .ok_or_else(|| EvidenceError::Io("atomic evidence path has no parent".into()))?;
    fs::create_dir_all(parent).map_err(io_error)?;
    let temporary = parent.join(format!(
        ".{}.{}.tmp",
        path.file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("evidence"),
        now_ms()
    ));
    let _ = fs::remove_file(&temporary);
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary)
        .map_err(io_error)?;
    file.write_all(bytes).map_err(io_error)?;
    file.sync_all().map_err(io_error)?;
    drop(file);
    atomic_replace(&temporary, path)
}

fn atomic_replace(temporary: &Path, destination: &Path) -> Result<(), EvidenceError> {
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
            return Err(io_error(std::io::Error::last_os_error()));
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        fs::rename(temporary, destination).map_err(io_error)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidenceInvalidation {
    pub evidence_id: String,
    pub reason: String,
    pub invalidated_at_ms: u64,
    pub sequence: u64,
    pub previous_digest: Option<String>,
    pub integrity: String,
}

impl EvidenceInvalidation {
    fn signing_body(&self) -> (&str, &str, u64, u64, &Option<String>) {
        (
            &self.evidence_id,
            &self.reason,
            self.invalidated_at_ms,
            self.sequence,
            &self.previous_digest,
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct WorkspaceChangeSet {
    pub changed_paths: BTreeSet<String>,
    pub lockfile_changed: bool,
    pub sealed_authority_changed: bool,
}

pub fn hash_file(path: &Path) -> Result<String, EvidenceError> {
    let mut file = File::open(path).map_err(io_error)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer).map_err(io_error)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn excluded_workspace_path(path: &Path) -> bool {
    path.components().any(|component| {
        let text = component.as_os_str().to_string_lossy();
        matches!(
            text.as_ref(),
            ".git" | "target" | "node_modules" | ".cargo-home"
        )
    })
}

pub fn fingerprint_workspace(root: &Path) -> Result<String, EvidenceError> {
    if !root.is_dir() {
        return Err(EvidenceError::InvalidInput(format!(
            "workspace is not a directory: {}",
            root.display()
        )));
    }
    let canonical_root = fs::canonicalize(root).map_err(io_error)?;
    let mut entries = Vec::new();
    for entry in WalkDir::new(&canonical_root)
        .follow_links(false)
        .max_depth(MAX_PATH_COMPONENTS)
    {
        let entry = entry.map_err(io_error)?;
        let path = entry.path();
        if !entry.file_type().is_file() {
            continue;
        }
        let relative_path = path
            .strip_prefix(&canonical_root)
            .map_err(|error| EvidenceError::InvalidInput(error.to_string()))?;
        if excluded_workspace_path(relative_path) {
            continue;
        }
        let relative = relative_path.to_string_lossy().replace('\\', "/");
        entries.push((relative, hash_file(path)?));
    }
    entries.sort();
    Ok(sha256(&canonical(&entries)?))
}

fn scoped_fingerprint(
    root: &Path,
    relevant_paths: &BTreeSet<String>,
) -> Result<String, EvidenceError> {
    let mut entries = BTreeMap::new();
    for relative in relevant_paths {
        let path = root.join(relative);
        let digest = if path.is_file() {
            hash_file(&path)?
        } else {
            "MISSING".into()
        };
        entries.insert(relative.clone(), digest);
    }
    Ok(sha256(&canonical(&entries)?))
}

#[allow(clippy::too_many_arguments)]
pub fn environment_fingerprint(
    root: &Path,
    mission_id: &str,
    mission_revision: u64,
    p6_seal_hash: &str,
    registry: &StandardsRegistry,
    source_revision: Option<String>,
    tool_versions: BTreeMap<String, String>,
    dependency_lock_hashes: BTreeMap<String, String>,
) -> Result<EnvironmentFingerprint, EvidenceError> {
    Ok(EnvironmentFingerprint {
        os: std::env::consts::OS.into(),
        architecture: std::env::consts::ARCH.into(),
        tool_versions,
        workspace_root: fs::canonicalize(root)
            .map_err(io_error)?
            .display()
            .to_string(),
        workspace_fingerprint: fingerprint_workspace(root)?,
        source_revision,
        dependency_lock_hashes,
        mission_id: mission_id.into(),
        mission_revision,
        p6_seal_hash: p6_seal_hash.into(),
        registry_id: registry.registry_id.clone(),
        registry_version: registry.registry_version,
        registry_digest: registry.registry_digest.clone(),
    })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandSpec {
    pub program: String,
    pub args: Vec<String>,
    pub working_directory: PathBuf,
    pub environment: BTreeMap<String, String>,
}

impl CommandSpec {
    pub fn new(program: impl Into<String>, working_directory: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            working_directory: working_directory.into(),
            environment: BTreeMap::new(),
        }
    }

    pub fn digest(&self) -> Result<String, EvidenceError> {
        Ok(sha256(&canonical(self)?))
    }
}

/// A Rust-owned candidate execution plan. Workspace configuration can describe
/// how to run tooling, but it cannot authorize what a probe proves. Criterion
/// authority is supplied separately by [`ProtectedCriterionVerificationPlan`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VerificationCollectorSpec {
    pub evidence_class: EvidenceClass,
    pub command: Option<CommandSpec>,
    pub adapter: Option<String>,
    pub route: Option<String>,
    pub threshold: Option<f64>,
    pub provenance: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct VerificationCollectorPlan {
    pub project_kind: String,
    pub source_fingerprint: String,
    pub collectors: Vec<VerificationCollectorSpec>,
}

/// A criterion-to-probe authority is separate from mutable workspace tooling.
/// Production can obtain an empty authority for a sealed mission; non-empty
/// mappings must arrive through a protected Rust-owned authority path.
#[derive(Debug, Clone)]
pub struct ProtectedCriterionVerificationPlan {
    authority_digest: String,
    bindings: Vec<AuthorizedCriterionProbe>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AuthorizedCriterionProbe {
    mission_id: String,
    mission_revision: u64,
    project_id: String,
    p6_seal_hash: String,
    requirement_id: String,
    criterion_id: String,
    evidence_class: EvidenceClass,
    collector_identity: CollectorIdentity,
    probe_identity: String,
    command_digest: String,
    verification_plan_authority_digest: String,
}

struct CriterionProbeLookup<'a> {
    requirement_id: &'a str,
    criterion_id: &'a str,
    evidence_class: EvidenceClass,
    collector_identity: &'a CollectorIdentity,
    probe_identity: &'a str,
    command_digest: &'a str,
}

#[cfg(feature = "test-support")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CriterionProbeAuthorizationInput {
    pub requirement_id: String,
    pub criterion_id: String,
    pub evidence_class: EvidenceClass,
    pub collector_identity: CollectorIdentity,
    pub probe_identity: String,
    pub command_digest: String,
}

impl ProtectedCriterionVerificationPlan {
    pub fn empty_for_authority(authority: &VerificationAuthority) -> Result<Self, EvidenceError> {
        authority.validate()?;
        Ok(Self {
            authority_digest: authority.identity_digest()?,
            bindings: Vec::new(),
        })
    }

    fn from_sealed_single_machine_criteria(
        authority: &VerificationAuthority,
        plan: &VerificationCollectorPlan,
    ) -> Result<Self, EvidenceError> {
        authority.validate()?;
        let authority_digest = authority.identity_digest()?;
        let mut candidates = Vec::new();
        for requirement in &authority.revision.contract.requirement_graph.requirements {
            let machine = requirement
                .acceptance_criteria
                .iter()
                .filter(|criterion| criterion.machine_checkable)
                .collect::<Vec<_>>();
            if machine.len() != 1 {
                continue;
            }
            for obligation in requirement
                .verification_policy
                .obligations
                .iter()
                .filter(|obligation| obligation.required)
            {
                let Some(spec) = plan.for_class(obligation.class) else {
                    continue;
                };
                if spec
                    .provenance
                    .starts_with("mutable workspace candidate verification config")
                {
                    continue;
                }
                let Some(command) = spec.command.as_ref() else {
                    continue;
                };
                let Some(collector_identity) = trusted_collector_identity(obligation.class) else {
                    continue;
                };
                candidates.push((
                    requirement.requirement_id.clone(),
                    machine[0].criterion_id.clone(),
                    obligation.class,
                    collector_identity,
                    format!(
                        "sealed-single-machine-criterion:{}:{}",
                        machine[0].criterion_id, plan.source_fingerprint
                    ),
                    command.digest()?,
                ));
            }
        }
        let plan_digest = sha256(&canonical(&(&authority_digest, &candidates))?);
        let bindings = candidates
            .into_iter()
            .map(
                |(
                    requirement_id,
                    criterion_id,
                    evidence_class,
                    collector_identity,
                    probe_identity,
                    command_digest,
                )| AuthorizedCriterionProbe {
                    mission_id: authority.revision.seal.mission_id.clone(),
                    mission_revision: authority.revision.revision,
                    project_id: authority.revision.seal.project_id.clone(),
                    p6_seal_hash: authority.revision.seal.contract_hash.clone(),
                    requirement_id,
                    criterion_id,
                    evidence_class,
                    collector_identity,
                    probe_identity,
                    command_digest,
                    verification_plan_authority_digest: plan_digest.clone(),
                },
            )
            .collect();
        Ok(Self {
            authority_digest,
            bindings,
        })
    }

    #[cfg(feature = "test-support")]
    pub fn from_test_support(
        authority: &VerificationAuthority,
        inputs: Vec<CriterionProbeAuthorizationInput>,
    ) -> Result<Self, EvidenceError> {
        authority.validate()?;
        let authority_digest = authority.identity_digest()?;
        for input in &inputs {
            let requirement = authority
                .revision
                .contract
                .requirement_graph
                .requirements
                .iter()
                .find(|item| item.requirement_id == input.requirement_id)
                .ok_or_else(|| {
                    EvidenceError::InvalidAuthority(format!(
                        "protected criterion references unknown requirement: {}",
                        input.requirement_id
                    ))
                })?;
            if !requirement
                .acceptance_criteria
                .iter()
                .any(|criterion| criterion.criterion_id == input.criterion_id)
            {
                return Err(EvidenceError::InvalidAuthority(format!(
                    "protected criterion is not sealed in requirement: {}",
                    input.criterion_id
                )));
            }
            if !requirement
                .verification_policy
                .obligations
                .iter()
                .any(|obligation| obligation.required && obligation.class == input.evidence_class)
            {
                return Err(EvidenceError::InvalidAuthority(
                    "protected criterion evidence class is not required by the sealed plan".into(),
                ));
            }
            if input.collector_identity.name.trim().is_empty()
                || input.probe_identity.trim().is_empty()
                || input.command_digest.len() != 64
                || !input
                    .command_digest
                    .chars()
                    .all(|item| item.is_ascii_hexdigit())
            {
                return Err(EvidenceError::InvalidInput(
                    "protected criterion probe identity or command digest is invalid".into(),
                ));
            }
        }
        let plan_digest = sha256(&canonical(&(
            &authority_digest,
            inputs
                .iter()
                .map(|input| {
                    (
                        &input.requirement_id,
                        &input.criterion_id,
                        input.evidence_class,
                        &input.collector_identity,
                        &input.probe_identity,
                        &input.command_digest,
                    )
                })
                .collect::<Vec<_>>(),
        ))?);
        let bindings = inputs
            .into_iter()
            .map(|input| AuthorizedCriterionProbe {
                mission_id: authority.revision.seal.mission_id.clone(),
                mission_revision: authority.revision.revision,
                project_id: authority.revision.seal.project_id.clone(),
                p6_seal_hash: authority.revision.seal.contract_hash.clone(),
                requirement_id: input.requirement_id,
                criterion_id: input.criterion_id,
                evidence_class: input.evidence_class,
                collector_identity: input.collector_identity,
                probe_identity: input.probe_identity,
                command_digest: input.command_digest,
                verification_plan_authority_digest: plan_digest.clone(),
            })
            .collect();
        Ok(Self {
            authority_digest,
            bindings,
        })
    }

    fn authorized_probe(
        &self,
        authority: &VerificationAuthority,
        current: &FreshnessContext,
        lookup: &CriterionProbeLookup<'_>,
    ) -> Result<AuthorizedCriterionProbe, EvidenceError> {
        self.validate_against(authority, current)?;
        self.bindings
            .iter()
            .find(|binding| {
                binding.requirement_id == lookup.requirement_id
                    && binding.criterion_id == lookup.criterion_id
                    && binding.evidence_class == lookup.evidence_class
                    && binding.collector_identity == *lookup.collector_identity
                    && binding.probe_identity == lookup.probe_identity
                    && binding.command_digest == lookup.command_digest
                    && binding.mission_id == authority.revision.seal.mission_id
                    && binding.mission_revision == authority.revision.revision
                    && binding.project_id == authority.revision.seal.project_id
                    && binding.p6_seal_hash == authority.revision.seal.contract_hash
            })
            .cloned()
            .ok_or_else(|| {
                EvidenceError::InvalidAuthority(
                    "candidate criterion probe is not authorized by protected P8 authority".into(),
                )
            })
    }

    fn validate_against(
        &self,
        authority: &VerificationAuthority,
        current: &FreshnessContext,
    ) -> Result<(), EvidenceError> {
        if self.authority_digest != authority.identity_digest()?
            || current.mission_id != authority.revision.seal.mission_id
            || current.mission_revision != authority.revision.revision
            || current.p6_seal_hash != authority.revision.seal.contract_hash
        {
            return Err(EvidenceError::InvalidAuthority(
                "protected criterion authority does not match sealed mission".into(),
            ));
        }
        if self.bindings.iter().any(|binding| {
            binding.verification_plan_authority_digest.trim().is_empty()
                || binding.mission_id != authority.revision.seal.mission_id
                || binding.mission_revision != authority.revision.revision
                || binding.project_id != authority.revision.seal.project_id
                || binding.p6_seal_hash != authority.revision.seal.contract_hash
        }) {
            return Err(EvidenceError::InvalidAuthority(
                "protected criterion mapping identity is not bound to sealed authority".into(),
            ));
        }
        Ok(())
    }

    fn authorized_probes_for(
        &self,
        authority: &VerificationAuthority,
        current: &FreshnessContext,
        requirement_id: &str,
        evidence_class: EvidenceClass,
    ) -> Result<Vec<AuthorizedCriterionProbe>, EvidenceError> {
        self.validate_against(authority, current)?;
        Ok(self
            .bindings
            .iter()
            .filter(|binding| {
                binding.requirement_id == requirement_id && binding.evidence_class == evidence_class
            })
            .cloned()
            .collect())
    }
}

impl VerificationCollectorPlan {
    pub fn discover(workspace_root: &Path) -> Result<Self, EvidenceError> {
        if !workspace_root.is_dir() {
            return Err(EvidenceError::CollectorUnavailable(
                "verification workspace is unavailable".into(),
            ));
        }
        let mut collectors = Vec::new();
        let mut project_kind = "unknown".to_string();
        let config_path = workspace_root
            .join(".relintor")
            .join("verification-plan.json");
        if config_path.is_file() {
            let bytes = fs::read(&config_path).map_err(io_error)?;
            let config: VerificationPlanFile =
                serde_json::from_slice(&bytes).map_err(json_error)?;
            for item in config.collectors {
                let class = item.evidence_class.ok_or_else(|| {
                    EvidenceError::InvalidInput("verification collector class is missing".into())
                })?;
                let command = item.program.map(|program| CommandSpec {
                    program,
                    args: item.args,
                    working_directory: workspace_root.to_path_buf(),
                    environment: item.environment,
                });
                collectors.push(VerificationCollectorSpec {
                    evidence_class: class,
                    command,
                    adapter: item.adapter,
                    route: item.route,
                    threshold: item.threshold,
                    provenance: format!(
                        "mutable workspace candidate verification config: {}",
                        config_path.display()
                    ),
                });
            }
        }

        let has_cargo = workspace_root.join("Cargo.toml").is_file();
        let package_json = workspace_root.join("package.json");
        let has_node = package_json.is_file();
        if has_cargo {
            project_kind = "cargo".into();
            add_discovered_command(
                &mut collectors,
                EvidenceClass::BuildOutput,
                CommandSpec {
                    program: "cargo".into(),
                    args: vec!["build".into(), "--workspace".into(), "--locked".into()],
                    working_directory: workspace_root.to_path_buf(),
                    environment: BTreeMap::new(),
                },
                "Cargo.toml build target",
            );
            add_discovered_command(
                &mut collectors,
                EvidenceClass::TestOutput,
                CommandSpec {
                    program: "cargo".into(),
                    args: vec!["test".into(), "--workspace".into(), "--locked".into()],
                    working_directory: workspace_root.to_path_buf(),
                    environment: BTreeMap::new(),
                },
                "Cargo.toml test target",
            );
            add_discovered_command(
                &mut collectors,
                EvidenceClass::LintStaticAnalysis,
                CommandSpec {
                    program: "cargo".into(),
                    args: vec![
                        "clippy".into(),
                        "--workspace".into(),
                        "--all-targets".into(),
                        "--locked".into(),
                        "--".into(),
                        "-D".into(),
                        "warnings".into(),
                    ],
                    working_directory: workspace_root.to_path_buf(),
                    environment: BTreeMap::new(),
                },
                "Cargo.toml clippy target",
            );
        }
        if has_node {
            let bytes = fs::read(&package_json).map_err(io_error)?;
            let package: serde_json::Value = serde_json::from_slice(&bytes).map_err(json_error)?;
            let scripts = package
                .get("scripts")
                .and_then(serde_json::Value::as_object);
            let manager = if workspace_root.join("pnpm-lock.yaml").is_file() {
                "pnpm"
            } else if workspace_root.join("yarn.lock").is_file() {
                "yarn"
            } else {
                "npm"
            };
            for (script, class) in [
                ("build", EvidenceClass::BuildOutput),
                ("test", EvidenceClass::TestOutput),
                ("lint", EvidenceClass::LintStaticAnalysis),
                ("security-scan", EvidenceClass::SecurityScan),
                ("accessibility-audit", EvidenceClass::AccessibilityResult),
            ] {
                if scripts.is_some_and(|items| items.contains_key(script)) {
                    add_discovered_command(
                        &mut collectors,
                        class,
                        CommandSpec {
                            program: manager.into(),
                            args: vec!["run".into(), script.into()],
                            working_directory: workspace_root.to_path_buf(),
                            environment: BTreeMap::new(),
                        },
                        &format!("package.json scripts.{script}"),
                    );
                }
            }
            if project_kind == "unknown" {
                project_kind = "node".into();
            } else {
                project_kind.push_str("+node");
            }
        }
        let pyproject = workspace_root.join("pyproject.toml");
        if pyproject.is_file() {
            project_kind = if project_kind == "unknown" {
                "python".into()
            } else {
                format!("{project_kind}+python")
            };
            let text = fs::read_to_string(&pyproject).map_err(io_error)?;
            if text.contains("pytest") {
                add_discovered_command(
                    &mut collectors,
                    EvidenceClass::TestOutput,
                    CommandSpec {
                        program: "python".into(),
                        args: vec!["-m".into(), "pytest".into()],
                        working_directory: workspace_root.to_path_buf(),
                        environment: BTreeMap::new(),
                    },
                    "pyproject.toml pytest configuration",
                );
            }
            if text.contains("ruff") {
                add_discovered_command(
                    &mut collectors,
                    EvidenceClass::LintStaticAnalysis,
                    CommandSpec {
                        program: "ruff".into(),
                        args: vec!["check".into(), ".".into()],
                        working_directory: workspace_root.to_path_buf(),
                        environment: BTreeMap::new(),
                    },
                    "pyproject.toml ruff configuration",
                );
            }
        }
        let gradle_wrapper = if cfg!(windows) {
            workspace_root.join("gradlew.bat")
        } else {
            workspace_root.join("gradlew")
        };
        if gradle_wrapper.is_file() {
            project_kind = if project_kind == "unknown" {
                "gradle".into()
            } else {
                format!("{project_kind}+gradle")
            };
            let program = gradle_wrapper.to_string_lossy().into_owned();
            for (task, class) in [
                ("build", EvidenceClass::BuildOutput),
                ("test", EvidenceClass::TestOutput),
                ("check", EvidenceClass::LintStaticAnalysis),
            ] {
                add_discovered_command(
                    &mut collectors,
                    class,
                    CommandSpec {
                        program: program.clone(),
                        args: vec![task.into()],
                        working_directory: workspace_root.to_path_buf(),
                        environment: BTreeMap::new(),
                    },
                    &format!("Gradle wrapper task {task}"),
                );
            }
        }
        collectors.sort_by_key(|item| format!("{:?}:{}", item.evidence_class, item.provenance));
        let source_fingerprint = sha256(&canonical(&collectors)?);
        Ok(Self {
            project_kind,
            source_fingerprint,
            collectors,
        })
    }

    pub fn for_class(&self, class: EvidenceClass) -> Option<&VerificationCollectorSpec> {
        self.collectors
            .iter()
            .find(|item| item.evidence_class == class)
    }
}

#[derive(Debug, Deserialize)]
struct VerificationPlanFile {
    #[serde(default)]
    collectors: Vec<VerificationPlanFileEntry>,
}

#[derive(Debug, Deserialize)]
struct VerificationPlanFileEntry {
    #[serde(rename = "class")]
    evidence_class: Option<EvidenceClass>,
    program: Option<String>,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    environment: BTreeMap<String, String>,
    adapter: Option<String>,
    route: Option<String>,
    threshold: Option<f64>,
}

fn add_discovered_command(
    collectors: &mut Vec<VerificationCollectorSpec>,
    evidence_class: EvidenceClass,
    command: CommandSpec,
    provenance: &str,
) {
    if collectors
        .iter()
        .any(|item| item.evidence_class == evidence_class)
    {
        return;
    }
    collectors.push(VerificationCollectorSpec {
        evidence_class,
        command: Some(command),
        adapter: Some("process".into()),
        route: None,
        threshold: None,
        provenance: provenance.into(),
    });
}

fn trusted_collector_identity(class: EvidenceClass) -> Option<CollectorIdentity> {
    let name = match class {
        EvidenceClass::BuildOutput => "build-collector",
        EvidenceClass::TestOutput => "test-collector",
        EvidenceClass::LintStaticAnalysis => "static-analysis-collector",
        EvidenceClass::ApiResponse | EvidenceClass::DatabaseQuery => "api-database-collector",
        EvidenceClass::BrowserRecording => "browser-runtime-collector",
        EvidenceClass::Screenshot => "screenshot-collector",
        EvidenceClass::AccessibilityResult => "accessibility-collector",
        EvidenceClass::PerformanceResult => "performance-collector",
        EvidenceClass::SecurityScan => "security-collector",
        _ => return None,
    };
    Some(CollectorIdentity::new(name, "p8-v1"))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessEvidence {
    pub command: CommandSpec,
    pub collector: CollectorIdentity,
    pub started_at_ms: u64,
    pub ended_at_ms: u64,
    pub exit_code: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub output_overflow: bool,
    pub status: GateStatus,
    pub result: EvidenceResult,
}

#[derive(Debug, Clone)]
pub struct ProcessCollector {
    pub identity: CollectorIdentity,
    pub max_output_bytes: usize,
    pub timeout: Duration,
}

impl Default for ProcessCollector {
    fn default() -> Self {
        Self {
            identity: CollectorIdentity::new("relintor-process-collector", "1"),
            max_output_bytes: 256 * 1024,
            timeout: Duration::from_secs(15 * 60),
        }
    }
}

impl ProcessCollector {
    pub fn run(&self, command: &CommandSpec) -> Result<ProcessEvidence, EvidenceError> {
        if command.program.trim().is_empty() || !command.working_directory.is_dir() {
            return Err(EvidenceError::CollectorUnavailable(
                "process command or working directory is invalid".into(),
            ));
        }
        let started_at_ms = now_ms();
        let mut process = Command::new(&command.program);
        process
            .args(&command.args)
            .current_dir(&command.working_directory)
            .envs(&command.environment)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = process.spawn().map_err(io_error)?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| EvidenceError::CollectorUnavailable("stdout pipe unavailable".into()))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| EvidenceError::CollectorUnavailable("stderr pipe unavailable".into()))?;
        let max = self.max_output_bytes;
        let stdout_thread = thread::spawn(move || capture_bounded(stdout, max));
        let stderr_thread = thread::spawn(move || capture_bounded(stderr, max));
        let deadline = Instant::now() + self.timeout;
        let status = loop {
            match child.try_wait().map_err(io_error)? {
                Some(status) => break status,
                None if Instant::now() >= deadline => {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = stdout_thread.join();
                    let _ = stderr_thread.join();
                    return Err(EvidenceError::CollectorUnavailable(
                        "verification collector exceeded its time limit".into(),
                    ));
                }
                None => thread::sleep(Duration::from_millis(50)),
            }
        };
        let (stdout, stdout_overflow) = stdout_thread.join().map_err(|_| {
            EvidenceError::CollectorUnavailable("stdout collector panicked".into())
        })??;
        let (stderr, stderr_overflow) = stderr_thread.join().map_err(|_| {
            EvidenceError::CollectorUnavailable("stderr collector panicked".into())
        })??;
        let ended_at_ms = now_ms();
        let exit_code = status.code();
        let passed = status.success();
        Ok(ProcessEvidence {
            command: command.clone(),
            collector: self.identity.clone(),
            started_at_ms,
            ended_at_ms,
            exit_code,
            stdout,
            stderr,
            output_overflow: stdout_overflow || stderr_overflow,
            status: if passed {
                GateStatus::Pass
            } else {
                GateStatus::Fail
            },
            result: if passed {
                EvidenceResult::Pass
            } else {
                EvidenceResult::Fail
            },
        })
    }
}

fn capture_bounded<R: Read>(mut reader: R, limit: usize) -> Result<(Vec<u8>, bool), EvidenceError> {
    let mut output = Vec::with_capacity(limit.min(8 * 1024));
    let mut buffer = [0_u8; 16 * 1024];
    let mut overflow = false;
    loop {
        let count = reader.read(&mut buffer).map_err(io_error)?;
        if count == 0 {
            break;
        }
        if output.len() < limit {
            let take = (limit - output.len()).min(count);
            output.extend_from_slice(&buffer[..take]);
            if take < count {
                overflow = true;
            }
        } else {
            overflow = true;
        }
    }
    Ok((output, overflow))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TestEvidenceSummary {
    pub process: ProcessEvidence,
    pub framework: String,
    pub inventory: Vec<TestInventoryEntry>,
    pub passed: u32,
    pub failed: u32,
    pub skipped: u32,
    pub ignored: u32,
    pub not_discovered: u32,
    pub required_test_ids: BTreeSet<String>,
}

impl TestEvidenceSummary {
    pub fn result(&self) -> EvidenceResult {
        if self.failed > 0 || self.process.result == EvidenceResult::Fail {
            EvidenceResult::Fail
        } else if self.skipped > 0 || self.ignored > 0 || self.not_discovered > 0 {
            EvidenceResult::Skipped
        } else if self.passed > 0 && self.process.result == EvidenceResult::Pass {
            EvidenceResult::Pass
        } else {
            EvidenceResult::Unknown
        }
    }

    pub fn required_tests_present(&self) -> bool {
        self.required_test_ids.iter().all(|required| {
            self.inventory
                .iter()
                .any(|entry| entry.test_id == *required && entry.enabled)
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuildOutput {
    pub process: ProcessEvidence,
    pub tool_identity: CollectorIdentity,
    pub artifact_hashes: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LintStaticAnalysis {
    pub process: ProcessEvidence,
    pub tool_identity: CollectorIdentity,
    pub result_digest: String,
}

#[derive(Default)]
pub struct BuildCollector {
    process: ProcessCollector,
}

impl BuildCollector {
    pub fn collect(
        &self,
        binding: &CollectorBinding,
        command: &CommandSpec,
    ) -> Result<CollectedEvidence<BuildOutput>, EvidenceError> {
        let process = self.process.run(command)?;
        let bytes = process_output_bytes(&process)?;
        let tool_identity = CollectorIdentity::new("build-collector", "p8-v1");
        let artifact_hashes = BTreeMap::from([("process-output".into(), sha256(&bytes))]);
        let observation = BuildOutput {
            process: process.clone(),
            tool_identity: tool_identity.clone(),
            artifact_hashes,
        };
        let receipt = binding.receipt(
            tool_identity,
            "REAL_BUILD_PROCESS",
            command.digest()?,
            process.result,
            EvidenceClass::BuildOutput,
            EvidenceConfidence::StrongDeterministic,
            &bytes,
        )?;
        Ok(CollectedEvidence {
            receipt,
            observation,
            artifact_bytes: bytes,
        })
    }
}

#[derive(Default)]
pub struct TestCollector {
    process: ProcessCollector,
}

impl TestCollector {
    pub fn collect(
        &self,
        binding: &CollectorBinding,
        command: &CommandSpec,
        framework: &str,
        required_test_ids: BTreeSet<String>,
    ) -> Result<CollectedEvidence<TestEvidenceSummary>, EvidenceError> {
        let process = self.process.run(command)?;
        let bytes = process_output_bytes(&process)?;
        let text = format!(
            "{}\n{}",
            String::from_utf8_lossy(&process.stdout),
            String::from_utf8_lossy(&process.stderr)
        );
        let inventory = discover_test_inventory(&command.working_directory)?;
        let mut passed = 0_u32;
        let mut failed = 0_u32;
        let mut skipped = 0_u32;
        let mut ignored = 0_u32;
        for line in text.lines() {
            let trimmed = line.trim();
            if line.contains("... ok") || trimmed.starts_with("ok ") {
                passed += 1;
            } else if line.contains("... FAILED") || trimmed.starts_with("not ok ") {
                failed += 1;
            } else if line.contains("... ignored") {
                ignored += 1;
            } else if trimmed.contains("# SKIP") || trimmed.contains("# TODO") {
                skipped += 1;
            }
        }
        if passed == 0
            && failed == 0
            && process.result == EvidenceResult::Pass
            && !inventory.is_empty()
        {
            passed = inventory.len().min(u32::MAX as usize) as u32;
        }
        let not_discovered =
            if inventory.is_empty() && passed == 0 && process.result == EvidenceResult::Pass {
                1
            } else {
                0
            };
        let observation = TestEvidenceSummary {
            process: process.clone(),
            framework: framework.into(),
            inventory: inventory.clone(),
            passed,
            failed,
            skipped,
            ignored,
            not_discovered,
            required_test_ids,
        };
        let result = observation.result();
        let mut receipt = binding.receipt(
            CollectorIdentity::new("test-collector", "p8-v1"),
            "REAL_TEST_PROCESS",
            command.digest()?,
            result,
            EvidenceClass::TestOutput,
            EvidenceConfidence::StrongDeterministic,
            &bytes,
        )?;
        receipt.test_inventory = inventory;
        receipt.integrity = sha256(&receipt.signing_body()?);
        Ok(CollectedEvidence {
            receipt,
            observation,
            artifact_bytes: bytes,
        })
    }
}

fn discover_test_inventory(root: &Path) -> Result<Vec<TestInventoryEntry>, EvidenceError> {
    let canonical_root = fs::canonicalize(root).map_err(io_error)?;
    let mut inventory = Vec::new();
    for entry in WalkDir::new(&canonical_root)
        .follow_links(false)
        .max_depth(128)
    {
        let entry = entry.map_err(io_error)?;
        if !entry.file_type().is_file() {
            continue;
        }
        let relative_path = entry
            .path()
            .strip_prefix(&canonical_root)
            .map_err(|error| EvidenceError::InvalidInput(error.to_string()))?;
        if excluded_workspace_path(relative_path) {
            continue;
        }
        let relative = relative_path.to_string_lossy().replace('\\', "/");
        let lower = relative.to_ascii_lowercase();
        let test_source = lower.starts_with("tests/")
            || lower.starts_with("test/")
            || lower.contains(".test.")
            || lower.contains(".spec.")
            || lower.ends_with("_test.py")
            || lower
                .rsplit('/')
                .next()
                .is_some_and(|name| name.starts_with("test_") && name.ends_with(".py"));
        if test_source {
            inventory.push(TestInventoryEntry {
                test_id: relative.clone(),
                source_path: relative,
                source_hash: hash_file(entry.path())?,
                enabled: true,
            });
        }
    }
    inventory.sort_by(|left, right| left.test_id.cmp(&right.test_id));
    Ok(inventory)
}

#[derive(Default)]
pub struct StaticAnalysisCollector {
    process: ProcessCollector,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct CollectorOrchestrationResult {
    pub executed: Vec<String>,
    pub reused_fresh: Vec<String>,
    pub blocked_external: Vec<String>,
}

/// Rust-owned P8 collector planning and execution. The sealed requirement
/// graph is the sole source of requirement IDs, evidence classes, and
/// acceptance criteria. Missing live dependencies are left incomplete rather
/// than represented by synthetic PASS receipts.
pub struct VerificationCollectorOrchestrator {
    workspace_root: PathBuf,
}

impl VerificationCollectorOrchestrator {
    pub fn new(workspace_root: impl Into<PathBuf>) -> Self {
        Self {
            workspace_root: workspace_root.into(),
        }
    }

    fn execute_binding(
        &self,
        binding: &CollectorBinding,
        class: EvidenceClass,
        command: &CommandSpec,
        spec: &VerificationCollectorSpec,
        project_kind: &str,
        store: &EvidenceStore,
    ) -> Result<(), EvidenceError> {
        match class {
            EvidenceClass::BuildOutput => {
                let collected = BuildCollector::default().collect(binding, command)?;
                store.put(collected.receipt, &collected.artifact_bytes)?;
            }
            EvidenceClass::TestOutput => {
                let collected = TestCollector::default().collect(
                    binding,
                    command,
                    project_kind,
                    BTreeSet::new(),
                )?;
                store.put(collected.receipt, &collected.artifact_bytes)?;
            }
            EvidenceClass::LintStaticAnalysis => {
                let collected = StaticAnalysisCollector::default().collect(binding, command)?;
                store.put(collected.receipt, &collected.artifact_bytes)?;
            }
            EvidenceClass::ApiResponse | EvidenceClass::DatabaseQuery => {
                let collected =
                    ApiDatabaseCollector::default().collect_for_class(binding, command, class)?;
                store.put(collected.receipt, &collected.artifact_bytes)?;
            }
            EvidenceClass::BrowserRecording => {
                let mut adapter = CommandBrowserAdapter {
                    process: ProcessCollector::default(),
                    command: command.clone(),
                };
                let collected = BrowserRuntimeCollector.collect(
                    binding,
                    &mut adapter,
                    spec.route.as_deref().unwrap_or("/"),
                    &[],
                )?;
                store.put(collected.receipt, &collected.artifact_bytes)?;
            }
            EvidenceClass::Screenshot => {
                let mut adapter = CommandScreenshotAdapter {
                    process: ProcessCollector::default(),
                    command: command.clone(),
                };
                let collected = ScreenshotCollector.capture(
                    binding,
                    &mut adapter,
                    spec.route.as_deref().unwrap_or("/"),
                    0,
                    0,
                )?;
                store.put(collected.receipt, &collected.artifact_bytes)?;
            }
            EvidenceClass::AccessibilityResult => {
                let collected = AccessibilityCollector::default().collect_process(
                    binding,
                    command,
                    spec.route.as_deref().unwrap_or("workspace"),
                )?;
                store.put(collected.receipt, &collected.artifact_bytes)?;
            }
            EvidenceClass::SecurityScan => {
                let tool = CollectorIdentity::new(
                    spec.adapter.as_deref().unwrap_or("configured-security"),
                    "discovered",
                );
                let collected = SecurityCollector::default().collect_process(
                    binding,
                    command,
                    tool,
                    spec.route.as_deref().unwrap_or("workspace"),
                )?;
                store.put(collected.receipt, &collected.artifact_bytes)?;
            }
            EvidenceClass::PerformanceResult => {
                return Err(EvidenceError::CollectorUnavailable(
                    "performance collector requires a measured value and sealed threshold".into(),
                ));
            }
            _ => {
                return Err(EvidenceError::CollectorUnavailable(
                    "configured collector boundary is unavailable".into(),
                ));
            }
        }
        Ok(())
    }

    pub fn collect_required_evidence_with_p7(
        &self,
        authority: &VerificationAuthority,
        current: &FreshnessContext,
        store: &EvidenceStore,
        p7_execution: &AuthenticatedP7Execution,
    ) -> Result<CollectorOrchestrationResult, EvidenceError> {
        let plan = VerificationCollectorPlan::discover(&self.workspace_root)?;
        let protected = ProtectedCriterionVerificationPlan::from_sealed_single_machine_criteria(
            authority, &plan,
        )?;
        self.collect_required_evidence_with_p7_and_protected_plan(
            authority,
            current,
            store,
            Some(p7_execution),
            &protected,
        )
    }

    fn collect_required_evidence_with_p7_and_protected_plan(
        &self,
        authority: &VerificationAuthority,
        current: &FreshnessContext,
        store: &EvidenceStore,
        p7_execution: Option<&AuthenticatedP7Execution>,
        protected: &ProtectedCriterionVerificationPlan,
    ) -> Result<CollectorOrchestrationResult, EvidenceError> {
        authority.validate()?;
        if let Some(p7_execution) = p7_execution {
            p7_execution.validate_against(authority)?;
        }
        protected.validate_against(authority, current)?;
        if !self.workspace_root.is_dir() {
            return Err(EvidenceError::CollectorUnavailable(
                "verification workspace is unavailable".into(),
            ));
        }
        let plan = VerificationCollectorPlan::discover(&self.workspace_root)?;
        let existing = store.list()?;
        let mut result = CollectorOrchestrationResult::default();
        for requirement in &authority.revision.contract.requirement_graph.requirements {
            if matches!(
                requirement.status,
                RequirementStatus::NotApplicable | RequirementStatus::DeferredByExplicitDecision
            ) {
                continue;
            }
            for obligation in requirement
                .verification_policy
                .obligations
                .iter()
                .filter(|item| item.required)
            {
                let operation = format!("{}:{:?}", requirement.requirement_id, obligation.class);
                let Some(spec) = plan.for_class(obligation.class) else {
                    result.blocked_external.push(format!(
                        "{operation}: no trusted collector was discovered for {:?}",
                        obligation.class
                    ));
                    continue;
                };
                let Some(command) = spec.command.clone() else {
                    result.blocked_external.push(format!(
                        "{operation}: configured collector has no executable dependency"
                    ));
                    continue;
                };
                let candidate_command_digest = command.digest()?;
                let probes = protected.authorized_probes_for(
                    authority,
                    current,
                    &requirement.requirement_id,
                    obligation.class,
                )?;
                if probes.is_empty() {
                    let fresh = existing.iter().any(|artifact| {
                        artifact.metadata.requirement_ids
                            == vec![requirement.requirement_id.clone()]
                            && artifact.metadata.class == obligation.class
                            && artifact.metadata.result == EvidenceResult::Pass
                            && confidence_meets(
                                artifact.metadata.confidence,
                                obligation.minimum_confidence,
                            )
                            && p7_execution.is_none_or(|p7| {
                                p7.validates_evidence_metadata(authority, &artifact.metadata)
                                    .is_ok_and(|valid| valid)
                            })
                            && store
                                .freshness(&artifact.metadata.evidence_id, current)
                                .is_ok_and(|value| value == EvidenceFreshness::Fresh)
                    });
                    if fresh {
                        result.reused_fresh.push(operation);
                        continue;
                    }
                    let binding = match CollectorBinding::for_requirement(
                        authority,
                        current,
                        format!(
                            "p8-collector-{}-{:?}",
                            requirement.requirement_id, obligation.class
                        ),
                        &requirement.requirement_id,
                        obligation.class,
                    )
                    .and_then(|binding| match p7_execution {
                        Some(p7) => binding.bind_successful_execution(
                            p7,
                            authority,
                            &requirement.requirement_id,
                        ),
                        None => binding
                            .bind_test_successful_execution(authority, &requirement.requirement_id),
                    }) {
                        Ok(binding) => binding,
                        Err(error) => {
                            result
                                .blocked_external
                                .push(format!("{operation}: {error}"));
                            continue;
                        }
                    };
                    if let Err(error) = self.execute_binding(
                        &binding,
                        obligation.class,
                        &command,
                        spec,
                        &plan.project_kind,
                        store,
                    ) {
                        result
                            .blocked_external
                            .push(format!("{operation}: {error}"));
                        continue;
                    }
                    result.executed.push(format!(
                        "{operation} via {} ({})",
                        command.program, spec.provenance
                    ));
                    continue;
                }
                for probe in probes {
                    let criterion_operation = format!("{operation}:{}", probe.criterion_id);
                    let fresh = existing.iter().any(|artifact| {
                        artifact.metadata.requirement_ids
                            == vec![requirement.requirement_id.clone()]
                            && artifact.metadata.class == obligation.class
                            && artifact.metadata.result == EvidenceResult::Pass
                            && artifact
                                .metadata
                                .accepted_criteria
                                .contains(&probe.criterion_id)
                            && confidence_meets(
                                artifact.metadata.confidence,
                                obligation.minimum_confidence,
                            )
                            && p7_execution.is_none_or(|p7| {
                                p7.validates_evidence_metadata(authority, &artifact.metadata)
                                    .is_ok_and(|valid| valid)
                            })
                            && store
                                .freshness(&artifact.metadata.evidence_id, current)
                                .is_ok_and(|value| value == EvidenceFreshness::Fresh)
                    });
                    if fresh {
                        result.reused_fresh.push(criterion_operation);
                        continue;
                    }
                    let binding = match CollectorBinding::for_criterion(
                        authority,
                        current,
                        format!(
                            "p8-collector-{}-{}",
                            requirement.requirement_id, probe.criterion_id
                        ),
                        &requirement.requirement_id,
                        &probe.criterion_id,
                        obligation.class,
                        probe.collector_identity.clone(),
                        probe.probe_identity.clone(),
                        candidate_command_digest.clone(),
                        protected,
                    )
                    .and_then(|binding| match p7_execution {
                        Some(p7) => binding.bind_successful_execution(
                            p7,
                            authority,
                            &requirement.requirement_id,
                        ),
                        None => binding
                            .bind_test_successful_execution(authority, &requirement.requirement_id),
                    }) {
                        Ok(binding) => binding,
                        Err(error) => {
                            result
                                .blocked_external
                                .push(format!("{criterion_operation}: {error}"));
                            continue;
                        }
                    };
                    if let Err(error) = self.execute_binding(
                        &binding,
                        obligation.class,
                        &command,
                        spec,
                        &plan.project_kind,
                        store,
                    ) {
                        result
                            .blocked_external
                            .push(format!("{criterion_operation}: {error}"));
                        continue;
                    }
                    result.executed.push(format!(
                        "{criterion_operation} via {} ({}) probe={}",
                        command.program, spec.provenance, probe.probe_identity
                    ));
                }
            }
        }
        Ok(result)
    }

    pub fn run_required_collectors(
        &self,
        authority: &VerificationAuthority,
        current: &FreshnessContext,
        store: &EvidenceStore,
        p7_execution: &AuthenticatedP7Execution,
    ) -> Result<CollectorOrchestrationResult, EvidenceError> {
        self.collect_required_evidence_with_p7(authority, current, store, p7_execution)
    }

    #[cfg(feature = "test-support")]
    pub fn collect_required_evidence(
        &self,
        authority: &VerificationAuthority,
        current: &FreshnessContext,
        store: &EvidenceStore,
    ) -> Result<CollectorOrchestrationResult, EvidenceError> {
        let protected = ProtectedCriterionVerificationPlan::empty_for_authority(authority)?;
        self.collect_required_evidence_with_p7_and_protected_plan(
            authority, current, store, None, &protected,
        )
    }

    #[cfg(feature = "test-support")]
    pub fn collect_required_evidence_with_protected_plan(
        &self,
        authority: &VerificationAuthority,
        current: &FreshnessContext,
        store: &EvidenceStore,
        protected: &ProtectedCriterionVerificationPlan,
    ) -> Result<CollectorOrchestrationResult, EvidenceError> {
        self.collect_required_evidence_with_p7_and_protected_plan(
            authority, current, store, None, protected,
        )
    }
}

impl StaticAnalysisCollector {
    pub fn collect(
        &self,
        binding: &CollectorBinding,
        command: &CommandSpec,
    ) -> Result<CollectedEvidence<LintStaticAnalysis>, EvidenceError> {
        let process = self.process.run(command)?;
        let bytes = process_output_bytes(&process)?;
        let observation = LintStaticAnalysis {
            process: process.clone(),
            tool_identity: CollectorIdentity::new("static-analysis-collector", "p8-v1"),
            result_digest: sha256(&bytes),
        };
        let receipt = binding.receipt(
            observation.tool_identity.clone(),
            "REAL_STATIC_ANALYSIS_PROCESS",
            command.digest()?,
            process.result,
            EvidenceClass::LintStaticAnalysis,
            EvidenceConfidence::StrongDeterministic,
            &bytes,
        )?;
        Ok(CollectedEvidence {
            receipt,
            observation,
            artifact_bytes: bytes,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApiDatabaseObservation {
    pub target_identity: String,
    pub probe_digest: String,
    pub response_digest: String,
    pub status: GateStatus,
    pub schema_metadata: BTreeMap<String, String>,
    pub result: EvidenceResult,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserRuntimeObservation {
    pub route: String,
    pub runtime_environment: String,
    pub steps_digest: String,
    pub result: EvidenceResult,
    pub artifact_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScreenshotObservation {
    pub capture_source: String,
    pub route_or_state: String,
    pub runtime_identity: String,
    pub width: u32,
    pub height: u32,
    pub artifact_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccessibilityObservation {
    pub route_or_component: String,
    pub violations: Vec<String>,
    pub severity_counts: BTreeMap<String, u32>,
    pub result: EvidenceResult,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PerformanceObservation {
    pub metric_name: String,
    pub measured_value: f64,
    pub threshold: f64,
    pub unit: String,
    pub probe_definition: String,
    pub result: EvidenceResult,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecurityObservation {
    pub tool: CollectorIdentity,
    pub scope: String,
    pub command_digest: String,
    pub finding_counts: BTreeMap<String, u32>,
    pub result: EvidenceResult,
    pub redacted_output_digest: String,
}

#[derive(Debug, Clone)]
pub struct CollectorBinding {
    evidence_id: String,
    mission_id: String,
    mission_revision: u64,
    p6_seal_hash: String,
    requirement_ids: Vec<String>,
    task_id: Option<String>,
    p7_attempt: Option<u32>,
    execution_identities: Vec<SuccessfulExecutionIdentity>,
    workspace_fingerprint: String,
    source_revision: Option<String>,
    environment_fingerprint: String,
    dependency_lock_hashes: BTreeMap<String, String>,
    required: bool,
    accepted_criteria: BTreeSet<String>,
    criterion_provenance: Vec<CriterionVerificationPlan>,
    relevant_paths: BTreeSet<String>,
    scope_fingerprint: Option<String>,
}

impl CollectorBinding {
    pub fn for_requirement(
        authority: &VerificationAuthority,
        current: &FreshnessContext,
        evidence_id: impl Into<String>,
        requirement_id: &str,
        evidence_class: EvidenceClass,
    ) -> Result<Self, EvidenceError> {
        let requirement = authority
            .revision
            .contract
            .requirement_graph
            .requirements
            .iter()
            .find(|item| item.requirement_id == requirement_id)
            .ok_or_else(|| {
                EvidenceError::InvalidAuthority(format!(
                    "collector requirement is not present in the sealed plan: {requirement_id}"
                ))
            })?;
        let obligation = requirement
            .verification_policy
            .obligations
            .iter()
            .find(|item| item.required && item.class == evidence_class)
            .ok_or_else(|| {
                EvidenceError::InvalidAuthority(format!(
                    "evidence class {evidence_class:?} is not required by sealed requirement {requirement_id}"
                ))
            })?;
        let mut binding = Self::from_authority_internal(
            authority,
            current,
            evidence_id,
            vec![requirement.requirement_id.clone()],
            obligation.required,
            BTreeSet::new(),
            BTreeSet::new(),
        )?;
        binding.required = obligation.required;
        #[cfg(feature = "test-support")]
        {
            binding = binding.bind_test_successful_execution(authority, requirement_id)?;
        }
        Ok(binding)
    }

    /// Bind one criterion to one collector/probe. Generic requirement
    /// bindings intentionally carry no acceptance criteria.
    #[allow(clippy::too_many_arguments)]
    pub fn for_criterion(
        authority: &VerificationAuthority,
        current: &FreshnessContext,
        evidence_id: impl Into<String>,
        requirement_id: &str,
        criterion_id: &str,
        evidence_class: EvidenceClass,
        collector_identity: CollectorIdentity,
        probe_identity: impl Into<String>,
        command_digest: impl Into<String>,
        protected: &ProtectedCriterionVerificationPlan,
    ) -> Result<Self, EvidenceError> {
        let probe_identity = probe_identity.into();
        let command_digest = command_digest.into();
        if collector_identity.name.trim().is_empty() || probe_identity.trim().is_empty() {
            return Err(EvidenceError::InvalidInput(
                "criterion collector and probe identities are required".into(),
            ));
        }
        let authorized = protected.authorized_probe(
            authority,
            current,
            &CriterionProbeLookup {
                requirement_id,
                criterion_id,
                evidence_class,
                collector_identity: &collector_identity,
                probe_identity: &probe_identity,
                command_digest: &command_digest,
            },
        )?;
        let mut binding = Self::from_authority_internal(
            authority,
            current,
            evidence_id,
            vec![requirement_id.into()],
            true,
            BTreeSet::from([criterion_id.into()]),
            BTreeSet::new(),
        )?;
        binding.criterion_provenance = vec![CriterionVerificationPlan {
            mission_id: authorized.mission_id,
            mission_revision: authorized.mission_revision,
            project_id: authorized.project_id,
            p6_seal_hash: authorized.p6_seal_hash,
            requirement_id: authorized.requirement_id,
            criterion_id: authorized.criterion_id,
            evidence_class: authorized.evidence_class,
            collector_identity: authorized.collector_identity,
            probe_identity: authorized.probe_identity,
            command_digest: authorized.command_digest,
            verification_plan_authority_digest: authorized.verification_plan_authority_digest,
        }];
        #[cfg(feature = "test-support")]
        {
            binding = binding.bind_test_successful_execution(authority, requirement_id)?;
        }
        Ok(binding)
    }

    fn from_authority_internal(
        authority: &VerificationAuthority,
        current: &FreshnessContext,
        evidence_id: impl Into<String>,
        requirement_ids: Vec<String>,
        required: bool,
        accepted_criteria: BTreeSet<String>,
        relevant_paths: BTreeSet<String>,
    ) -> Result<Self, EvidenceError> {
        authority.validate()?;
        if current.mission_id != authority.revision.seal.mission_id
            || current.mission_revision != authority.revision.revision
            || current.p6_seal_hash != authority.revision.seal.contract_hash
        {
            return Err(EvidenceError::InvalidAuthority(
                "collector binding does not match sealed authority".into(),
            ));
        }
        let graph = &authority.revision.contract.requirement_graph;
        for requirement_id in &requirement_ids {
            if !graph
                .requirements
                .iter()
                .any(|item| item.requirement_id == *requirement_id)
            {
                return Err(EvidenceError::InvalidAuthority(format!(
                    "collector binding references unknown sealed requirement: {requirement_id}"
                )));
            }
        }
        for criterion_id in &accepted_criteria {
            if !graph.requirements.iter().any(|requirement| {
                requirement_ids.contains(&requirement.requirement_id)
                    && requirement
                        .acceptance_criteria
                        .iter()
                        .any(|criterion| criterion.criterion_id == *criterion_id)
            }) {
                return Err(EvidenceError::InvalidAuthority(format!(
                    "collector binding criterion is not owned by its sealed requirement: {criterion_id}"
                )));
            }
        }
        let scope_fingerprint = current
            .workspace_root
            .as_ref()
            .filter(|_| !relevant_paths.is_empty())
            .map(|root| scoped_fingerprint(root, &relevant_paths))
            .transpose()?;
        let criterion_provenance = accepted_criteria
            .iter()
            .map(|criterion_id| CriterionVerificationPlan {
                mission_id: authority.revision.seal.mission_id.clone(),
                mission_revision: authority.revision.revision,
                project_id: authority.revision.seal.project_id.clone(),
                p6_seal_hash: authority.revision.seal.contract_hash.clone(),
                requirement_id: requirement_ids.first().cloned().unwrap_or_default(),
                criterion_id: criterion_id.clone(),
                evidence_class: EvidenceClass::TestOutput,
                collector_identity: CollectorIdentity::new(
                    "test-support-collector",
                    "p8-test-only",
                ),
                probe_identity: "TEST_SUPPORT_FIXTURE".into(),
                command_digest: "TEST_SUPPORT_COMMAND".into(),
                verification_plan_authority_digest: "TEST_SUPPORT_AUTHORITY".into(),
            })
            .collect();
        Ok(Self {
            evidence_id: evidence_id.into(),
            mission_id: authority.revision.seal.mission_id.clone(),
            mission_revision: authority.revision.revision,
            p6_seal_hash: authority.revision.seal.contract_hash.clone(),
            requirement_ids,
            task_id: None,
            p7_attempt: None,
            execution_identities: Vec::new(),
            workspace_fingerprint: current.workspace_fingerprint.clone(),
            source_revision: current.source_revision.clone(),
            environment_fingerprint: current.environment_fingerprint.clone(),
            dependency_lock_hashes: current.dependency_lock_hashes.clone(),
            required,
            accepted_criteria,
            criterion_provenance,
            relevant_paths,
            scope_fingerprint,
        })
    }

    fn bind_successful_execution(
        mut self,
        p7_execution: &AuthenticatedP7Execution,
        authority: &VerificationAuthority,
        requirement_id: &str,
    ) -> Result<Self, EvidenceError> {
        let mut identities = p7_execution.identities_for_requirement(authority, requirement_id)?;
        if identities.len() != 1 {
            return Err(EvidenceError::InvalidAuthority(format!(
                "requirement {requirement_id} must map to exactly one successful P7 task for collector attribution"
            )));
        }
        let identity = identities.remove(0);
        let execution_suffix = sha256(&canonical(&(
            &identity.attempt_id,
            &identity.lease_id,
            &identity.process_digest,
        ))?);
        self.evidence_id = format!("{}-{}", self.evidence_id, &execution_suffix[..16]);
        self.task_id = Some(identity.task_id.clone());
        self.p7_attempt = Some(identity.attempt_number);
        self.execution_identities = vec![identity];
        Ok(self)
    }

    fn bind_test_successful_execution(
        self,
        authority: &VerificationAuthority,
        requirement_id: &str,
    ) -> Result<Self, EvidenceError> {
        #[cfg(feature = "test-support")]
        {
            let task = authority
                .revision
                .contract
                .task_graph
                .tasks
                .iter()
                .find(|task| task.requirement_ids.iter().any(|id| id == requirement_id))
                .ok_or_else(|| {
                    EvidenceError::InvalidAuthority(
                        "test fixture requirement has no sealed task".into(),
                    )
                })?;
            let digest = sha256(
                format!(
                    "{}:{}:{}",
                    authority.revision.seal.mission_id, authority.revision.revision, task.task_id
                )
                .as_bytes(),
            );
            let mut bound = self;
            bound.task_id = Some(task.task_id.clone());
            bound.p7_attempt = Some(1);
            bound.execution_identities = vec![SuccessfulExecutionIdentity {
                mission_id: authority.revision.seal.mission_id.clone(),
                mission_revision: authority.revision.revision,
                seal_hash: authority.revision.seal.contract_hash.clone(),
                run_id: authority.p7_run_id.clone(),
                task_id: task.task_id.clone(),
                attempt_id: format!("test-attempt-{digest}"),
                attempt_number: 1,
                packet_digest: digest.clone(),
                lease_id: format!("test-lease-{digest}"),
                lease_digest: digest.clone(),
                process_digest: digest.clone(),
                workspace_identity: digest.clone(),
                workspace_before_fingerprint: digest.clone(),
                workspace_after_fingerprint: digest,
                artifact_changes: Vec::new(),
                started_at_ms: 1,
                ended_at_ms: 2,
                lease_expires_at_ms: 3,
            }];
            Ok(bound)
        }
        #[cfg(not(feature = "test-support"))]
        {
            let _ = (self, authority, requirement_id);
            Err(EvidenceError::InvalidAuthority(
                "P7 execution identity is required".into(),
            ))
        }
    }

    #[cfg(feature = "test-support")]
    pub fn from_authority(
        authority: &VerificationAuthority,
        current: &FreshnessContext,
        evidence_id: impl Into<String>,
        requirement_ids: Vec<String>,
        required: bool,
        accepted_criteria: BTreeSet<String>,
        relevant_paths: BTreeSet<String>,
    ) -> Result<Self, EvidenceError> {
        Self::from_authority_internal(
            authority,
            current,
            evidence_id,
            requirement_ids,
            required,
            accepted_criteria,
            relevant_paths,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn receipt(
        &self,
        collector: CollectorIdentity,
        collector_kind: &str,
        command_digest: String,
        result: EvidenceResult,
        class: EvidenceClass,
        confidence: EvidenceConfidence,
        process_bytes: &[u8],
    ) -> Result<CollectorReceipt, EvidenceError> {
        let criterion_provenance = self
            .criterion_provenance
            .iter()
            .map(|provenance| CriterionVerificationPlan {
                mission_id: provenance.mission_id.clone(),
                mission_revision: provenance.mission_revision,
                project_id: provenance.project_id.clone(),
                p6_seal_hash: provenance.p6_seal_hash.clone(),
                requirement_id: provenance.requirement_id.clone(),
                criterion_id: provenance.criterion_id.clone(),
                evidence_class: provenance.evidence_class,
                collector_identity: provenance.collector_identity.clone(),
                probe_identity: provenance.probe_identity.clone(),
                command_digest: command_digest.clone(),
                verification_plan_authority_digest: provenance
                    .verification_plan_authority_digest
                    .clone(),
            })
            .collect();
        let mut receipt = CollectorReceipt {
            evidence_id: self.evidence_id.clone(),
            mission_id: self.mission_id.clone(),
            mission_revision: self.mission_revision,
            p6_seal_hash: self.p6_seal_hash.clone(),
            requirement_ids: self.requirement_ids.clone(),
            task_id: self.task_id.clone(),
            p7_attempt: self.p7_attempt,
            execution_identities: self.execution_identities.clone(),
            workspace_fingerprint: self.workspace_fingerprint.clone(),
            source_revision: self.source_revision.clone(),
            collector,
            collector_kind: collector_kind.into(),
            command_digest,
            environment_fingerprint: self.environment_fingerprint.clone(),
            created_at_ms: now_ms(),
            result,
            confidence,
            class,
            required: self.required,
            accepted_criteria: self.accepted_criteria.clone(),
            criterion_provenance,
            relevant_paths: self.relevant_paths.clone(),
            test_inventory: Vec::new(),
            dependency_lock_hashes: self.dependency_lock_hashes.clone(),
            scope_fingerprint: self.scope_fingerprint.clone(),
            actual_process_digest: sha256(process_bytes),
            authority_binding: sha256(&canonical(&(
                &self.mission_id,
                self.mission_revision,
                &self.p6_seal_hash,
                &self.workspace_fingerprint,
                &self.execution_identities,
            ))?),
            integrity: String::new(),
        };
        receipt.integrity = sha256(&receipt.signing_body()?);
        Ok(receipt)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CollectedEvidence<T> {
    pub receipt: CollectorReceipt,
    pub observation: T,
    pub artifact_bytes: Vec<u8>,
}

fn process_output_bytes(process: &ProcessEvidence) -> Result<Vec<u8>, EvidenceError> {
    canonical(process)
}

/// API and database probes execute a bounded structured process and derive
/// their result from the observed exit status, never from a renderer DTO.
#[derive(Default)]
pub struct ApiDatabaseCollector {
    process: ProcessCollector,
}

impl ApiDatabaseCollector {
    pub fn collect(
        &self,
        binding: &CollectorBinding,
        command: &CommandSpec,
    ) -> Result<CollectedEvidence<ApiDatabaseObservation>, EvidenceError> {
        self.collect_for_class(binding, command, EvidenceClass::DatabaseQuery)
    }

    pub fn collect_for_class(
        &self,
        binding: &CollectorBinding,
        command: &CommandSpec,
        evidence_class: EvidenceClass,
    ) -> Result<CollectedEvidence<ApiDatabaseObservation>, EvidenceError> {
        if !matches!(
            evidence_class,
            EvidenceClass::ApiResponse | EvidenceClass::DatabaseQuery
        ) {
            return Err(EvidenceError::InvalidInput(
                "API/database collector received an unrelated evidence class".into(),
            ));
        }
        let process = self.process.run(command)?;
        let bytes = process_output_bytes(&process)?;
        let observation = ApiDatabaseObservation {
            target_identity: command.program.clone(),
            probe_digest: command.digest()?,
            response_digest: sha256(&bytes),
            status: if process.result == EvidenceResult::Pass {
                GateStatus::Pass
            } else {
                GateStatus::Fail
            },
            schema_metadata: BTreeMap::new(),
            result: process.result,
        };
        let receipt = binding.receipt(
            CollectorIdentity::new("api-database-collector", "p8-v1"),
            "BOUNDED_API_DATABASE_PROCESS",
            command.digest()?,
            process.result,
            evidence_class,
            EvidenceConfidence::StrongDeterministic,
            &bytes,
        )?;
        Ok(CollectedEvidence {
            receipt,
            observation,
            artifact_bytes: bytes,
        })
    }
}

pub trait BrowserRuntimeAdapter {
    fn run_route(&mut self, route: &str, steps: &[String]) -> Result<Vec<u8>, String>;
}

pub struct BrowserRuntimeCollector;

struct CommandBrowserAdapter {
    process: ProcessCollector,
    command: CommandSpec,
}

impl BrowserRuntimeAdapter for CommandBrowserAdapter {
    fn run_route(&mut self, _route: &str, _steps: &[String]) -> Result<Vec<u8>, String> {
        let output = self
            .process
            .run(&self.command)
            .map_err(|error| error.to_string())?;
        if output.result == EvidenceResult::Pass {
            process_output_bytes(&output).map_err(|error| error.to_string())
        } else {
            Err(format!(
                "browser runtime probe exited with {:?}",
                output.exit_code
            ))
        }
    }
}

impl BrowserRuntimeCollector {
    pub fn collect<A: BrowserRuntimeAdapter>(
        &self,
        binding: &CollectorBinding,
        adapter: &mut A,
        route: &str,
        steps: &[String],
    ) -> Result<CollectedEvidence<BrowserRuntimeObservation>, EvidenceError> {
        let bytes = adapter
            .run_route(route, steps)
            .map_err(EvidenceError::CollectorUnavailable)?;
        let observation = BrowserRuntimeObservation {
            route: route.into(),
            runtime_environment: "configured-browser-adapter".into(),
            steps_digest: sha256(&canonical(steps)?),
            result: EvidenceResult::Pass,
            artifact_digest: sha256(&bytes),
        };
        let receipt = binding.receipt(
            CollectorIdentity::new("browser-runtime-collector", "p8-v1"),
            "BROWSER_RUNTIME_ADAPTER",
            observation.steps_digest.clone(),
            observation.result,
            EvidenceClass::BrowserRecording,
            EvidenceConfidence::StrongRuntime,
            &bytes,
        )?;
        Ok(CollectedEvidence {
            receipt,
            observation,
            artifact_bytes: bytes,
        })
    }
}

pub trait ScreenshotAdapter {
    fn capture(&mut self, route_or_state: &str) -> Result<Vec<u8>, String>;
}

pub struct ScreenshotCollector;

struct CommandScreenshotAdapter {
    process: ProcessCollector,
    command: CommandSpec,
}

impl ScreenshotAdapter for CommandScreenshotAdapter {
    fn capture(&mut self, _route_or_state: &str) -> Result<Vec<u8>, String> {
        let output = self
            .process
            .run(&self.command)
            .map_err(|error| error.to_string())?;
        if output.result == EvidenceResult::Pass {
            process_output_bytes(&output).map_err(|error| error.to_string())
        } else {
            Err(format!(
                "screenshot adapter exited with {:?}",
                output.exit_code
            ))
        }
    }
}

impl ScreenshotCollector {
    pub fn capture<A: ScreenshotAdapter>(
        &self,
        binding: &CollectorBinding,
        adapter: &mut A,
        route_or_state: &str,
        width: u32,
        height: u32,
    ) -> Result<CollectedEvidence<ScreenshotObservation>, EvidenceError> {
        let bytes = adapter
            .capture(route_or_state)
            .map_err(EvidenceError::CollectorUnavailable)?;
        let observation = ScreenshotObservation {
            capture_source: "configured-screenshot-adapter".into(),
            route_or_state: route_or_state.into(),
            runtime_identity: "rust-owned-capture".into(),
            width,
            height,
            artifact_digest: sha256(&bytes),
        };
        let receipt = binding.receipt(
            CollectorIdentity::new("screenshot-collector", "p8-v1"),
            "SCREENSHOT_CAPTURE_ADAPTER",
            sha256(route_or_state.as_bytes()),
            EvidenceResult::Pass,
            EvidenceClass::Screenshot,
            EvidenceConfidence::StrongRuntime,
            &bytes,
        )?;
        Ok(CollectedEvidence {
            receipt,
            observation,
            artifact_bytes: bytes,
        })
    }
}

#[derive(Default)]
pub struct AccessibilityCollector {
    process: ProcessCollector,
}

impl AccessibilityCollector {
    pub fn collect_process(
        &self,
        binding: &CollectorBinding,
        command: &CommandSpec,
        route_or_component: &str,
    ) -> Result<CollectedEvidence<AccessibilityObservation>, EvidenceError> {
        let process = self.process.run(command)?;
        let bytes = process_output_bytes(&process)?;
        let observation = AccessibilityObservation {
            route_or_component: route_or_component.into(),
            violations: Vec::new(),
            severity_counts: BTreeMap::new(),
            result: process.result,
        };
        let receipt = binding.receipt(
            CollectorIdentity::new("accessibility-collector", "p8-v1"),
            "CONFIGURED_ACCESSIBILITY_PROCESS",
            command.digest()?,
            process.result,
            EvidenceClass::AccessibilityResult,
            EvidenceConfidence::StrongRuntime,
            &bytes,
        )?;
        Ok(CollectedEvidence {
            receipt,
            observation,
            artifact_bytes: bytes,
        })
    }
}

pub struct PerformanceCollector;

impl PerformanceCollector {
    pub fn measure(
        &self,
        binding: &CollectorBinding,
        metric_name: &str,
        measured_value: f64,
        threshold: f64,
        unit: &str,
        probe_definition: &str,
    ) -> Result<CollectedEvidence<PerformanceObservation>, EvidenceError> {
        let result = if measured_value <= threshold {
            EvidenceResult::Pass
        } else {
            EvidenceResult::Fail
        };
        let observation = PerformanceObservation {
            metric_name: metric_name.into(),
            measured_value,
            threshold,
            unit: unit.into(),
            probe_definition: probe_definition.into(),
            result,
        };
        let bytes = canonical(&observation)?;
        let receipt = binding.receipt(
            CollectorIdentity::new("performance-collector", "p8-v1"),
            "MEASURED_PERFORMANCE_THRESHOLD",
            sha256(probe_definition.as_bytes()),
            result,
            EvidenceClass::PerformanceResult,
            EvidenceConfidence::StrongRuntime,
            &bytes,
        )?;
        Ok(CollectedEvidence {
            receipt,
            observation,
            artifact_bytes: bytes,
        })
    }
}

#[derive(Default)]
pub struct SecurityCollector {
    process: ProcessCollector,
}

impl SecurityCollector {
    pub fn collect_process(
        &self,
        binding: &CollectorBinding,
        command: &CommandSpec,
        tool: CollectorIdentity,
        scope: &str,
    ) -> Result<CollectedEvidence<SecurityObservation>, EvidenceError> {
        let process = self.process.run(command)?;
        let bytes = process_output_bytes(&process)?;
        let observation = SecurityObservation {
            tool,
            scope: scope.into(),
            command_digest: command.digest()?,
            finding_counts: BTreeMap::new(),
            result: process.result,
            redacted_output_digest: sha256(&bytes),
        };
        let receipt = binding.receipt(
            CollectorIdentity::new("security-collector", "p8-v1"),
            "CONFIGURED_SECURITY_PROCESS",
            command.digest()?,
            process.result,
            EvidenceClass::SecurityScan,
            EvidenceConfidence::StrongDeterministic,
            &bytes,
        )?;
        Ok(CollectedEvidence {
            receipt,
            observation,
            artifact_bytes: bytes,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidenceInvalidationRecord {
    pub evidence_id: String,
    pub reason: String,
    pub impacted_requirements: BTreeSet<String>,
    pub invalidated_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerificationAuthority {
    pub revision: MissionRevision,
    pub handoff: ExecutionHandoff,
    pub registry: StandardsRegistry,
    pub trusted_signers: TrustedSignerSet,
    pub p7_run_id: String,
    pub p7_state: String,
    pub workspace_fingerprint: String,
    pub source_revision: Option<String>,
    pub environment_fingerprint: String,
}

impl VerificationAuthority {
    pub fn validate(&self) -> Result<(), EvidenceError> {
        self.registry
            .verify(&self.trusted_signers, true)
            .map_err(|error| EvidenceError::InvalidAuthority(error.to_string()))?;
        let seal = AuthorityEngine
            .validate_seal(&self.revision)
            .map_err(|error| EvidenceError::InvalidAuthority(error.to_string()))?;
        if !seal.valid {
            return Err(EvidenceError::InvalidAuthority(
                "P6 seal is not valid".into(),
            ));
        }
        let contract = &self.revision.contract;
        let mission = &self.revision.seal;
        if self.handoff.state != "READY_FOR_EXECUTION"
            || self.p7_state != "EXECUTION_TASKS_FINISHED_AWAITING_VERIFICATION"
            || self.handoff.mission_id != mission.mission_id
            || self.handoff.revision != self.revision.revision
            || self.handoff.contract_hash != mission.contract_hash
            || contract.registry_id != self.registry.registry_id
            || contract.registry_version != self.registry.registry_version
            || contract.registry_digest != self.registry.registry_digest
            || self.source_revision.as_deref() != Some(contract.project_source_revision.as_str())
        {
            return Err(EvidenceError::InvalidAuthority(
                "P6/P7/registry/source identity mismatch".into(),
            ));
        }
        if self.p7_run_id.trim().is_empty() || self.workspace_fingerprint.trim().is_empty() {
            return Err(EvidenceError::InvalidAuthority(
                "P7 run or workspace identity missing".into(),
            ));
        }
        Ok(())
    }

    pub fn identity_digest(&self) -> Result<String, EvidenceError> {
        Ok(sha256(&canonical(&(
            self.revision.seal.mission_id.clone(),
            self.revision.revision,
            self.revision.seal.contract_hash.clone(),
            self.registry.registry_id.clone(),
            self.registry.registry_version,
            self.registry.registry_digest.clone(),
            self.p7_run_id.clone(),
            self.p7_state.clone(),
            self.workspace_fingerprint.clone(),
            self.source_revision.clone(),
            self.environment_fingerprint.clone(),
        ))?))
    }
}

/// An authenticated P7 execution result admitted from the real Rust ledger.
/// `ExecutionRun::restore_snapshot` verifies the ledger integrity envelope;
/// this wrapper additionally binds the terminal task outcomes to the sealed
/// P6 mission before P8 evaluation can proceed.
#[derive(Debug, Clone)]
pub struct AuthenticatedP7Execution {
    pub run: ExecutionRun,
    pub ledger_digest: String,
}

impl AuthenticatedP7Execution {
    pub fn from_snapshot(path: &Path) -> Result<Self, EvidenceError> {
        let run = ExecutionRun::restore_snapshot(path)
            .map_err(|error| EvidenceError::InvalidAuthority(error.to_string()))?;
        Self::from_run(run)
    }

    pub fn from_run(run: ExecutionRun) -> Result<Self, EvidenceError> {
        let snapshot = run
            .snapshot_json()
            .map_err(|error| EvidenceError::InvalidAuthority(error.to_string()))?;
        Ok(Self {
            ledger_digest: sha256(snapshot.as_bytes()),
            run,
        })
    }

    pub fn validate_against(&self, authority: &VerificationAuthority) -> Result<(), EvidenceError> {
        authority.validate()?;
        let run = &self.run;
        if run.ledger_version != relintor_execution::EXECUTION_LEDGER_VERSION
            || run.run_id != authority.p7_run_id
            || run.mission_id != authority.revision.seal.mission_id
            || run.mission_revision != authority.revision.revision
            || run.seal_hash != authority.revision.seal.contract_hash
            || run.workspace_fingerprint != authority.workspace_fingerprint
            || run.state != ExecutionRunState::ExecutionTasksFinishedAwaitingVerification
            || run.tasks.is_empty()
            || run.tasks.values().any(|task| {
                task.state != ExecutionTaskState::FinishedAwaitingVerification
                    || !run.attempts.iter().any(|attempt| {
                        attempt.task_id == task.task_id
                            && attempt.state == TaskAttemptState::Succeeded
                    })
            })
        {
            return Err(EvidenceError::InvalidAuthority(
                "authenticated P7 execution ledger is not a matching finished run".into(),
            ));
        }
        let identities = run
            .successful_execution_identities()
            .map_err(|error| EvidenceError::InvalidAuthority(error.to_string()))?;
        if identities.len() != run.tasks.len() {
            return Err(EvidenceError::InvalidAuthority(
                "authenticated P7 ledger does not contain one exact successful execution identity per task"
                    .into(),
            ));
        }
        Ok(())
    }

    pub fn identities_for_requirement(
        &self,
        authority: &VerificationAuthority,
        requirement_id: &str,
    ) -> Result<Vec<SuccessfulExecutionIdentity>, EvidenceError> {
        self.validate_against(authority)?;
        let task_ids = self
            .run
            .tasks
            .values()
            .filter(|task| task.requirement_ids.iter().any(|id| id == requirement_id))
            .map(|task| task.task_id.clone())
            .collect::<BTreeSet<_>>();
        if task_ids.is_empty() {
            return Err(EvidenceError::InvalidAuthority(format!(
                "sealed requirement {requirement_id} has no P7 task"
            )));
        }
        Ok(self
            .run
            .successful_execution_identities()
            .map_err(|error| EvidenceError::InvalidAuthority(error.to_string()))?
            .into_iter()
            .filter(|identity| task_ids.contains(&identity.task_id))
            .collect())
    }

    pub fn validates_evidence_metadata(
        &self,
        authority: &VerificationAuthority,
        metadata: &EvidenceMetadata,
    ) -> Result<bool, EvidenceError> {
        if metadata.mission_id != authority.revision.seal.mission_id
            || metadata.mission_revision != authority.revision.revision
            || metadata.p6_seal_hash != authority.revision.seal.contract_hash
            || metadata.requirement_ids.len() != 1
        {
            return Ok(false);
        }
        let expected = self.identities_for_requirement(
            authority,
            metadata
                .requirement_ids
                .first()
                .map(String::as_str)
                .unwrap_or_default(),
        )?;
        Ok(exact_execution_binding_matches(metadata, &expected))
    }
}

fn exact_execution_binding_matches(
    metadata: &EvidenceMetadata,
    expected: &[SuccessfulExecutionIdentity],
) -> bool {
    expected.len() == 1
        && metadata.execution_identities == expected
        && metadata.task_id.as_deref() == Some(expected[0].task_id.as_str())
        && metadata.p7_attempt == Some(expected[0].attempt_number)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CompletionState {
    VerifiedComplete,
    CompleteWithAcceptedRisks,
    StoppedIncomplete,
    BlockedExternal,
    FailedVerification,
    RevalidationRequired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequirementVerification {
    pub requirement_id: String,
    pub status: RequirementStatus,
    pub evidence_ids: Vec<String>,
    pub missing_obligations: Vec<EvidenceClass>,
    pub missing_acceptance_criteria: Vec<String>,
    pub stale_evidence: Vec<String>,
    pub failed_evidence: Vec<String>,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeterministicGateResult {
    pub gate_id: String,
    pub status: GateStatus,
    pub evidence_id: Option<String>,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompletionDecision {
    pub state: CompletionState,
    pub reason: String,
    pub deterministic_gates: Vec<DeterministicGateResult>,
    pub accepted_risks: Vec<AcceptedRiskRecord>,
    pub blocked_external: Vec<BlockedExternalRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerificationReport {
    pub verification_run_id: String,
    pub authority_digest: String,
    pub requirement_statuses: Vec<RequirementVerification>,
    pub decision: CompletionDecision,
    pub builder_claim: Option<String>,
    pub coverage_total: usize,
    pub coverage_accounted: usize,
    pub evidence_manifest_hash: String,
    #[serde(default)]
    pub p7_ledger_digest: Option<String>,
    #[serde(default)]
    pub ai_judgements: Vec<AiVerifierJudgement>,
    #[serde(default)]
    pub integrity_tag: String,
}

impl VerificationReport {
    fn signing_body(&self) -> Result<Vec<u8>, EvidenceError> {
        canonical(&(
            self.verification_run_id.clone(),
            self.authority_digest.clone(),
            self.requirement_statuses.clone(),
            self.decision.clone(),
            self.builder_claim.clone(),
            self.coverage_total,
            self.coverage_accounted,
            self.evidence_manifest_hash.clone(),
            self.p7_ledger_digest.clone(),
            self.ai_judgements.clone(),
        ))
    }

    fn authenticate(&mut self, key: &[u8]) -> Result<(), EvidenceError> {
        self.integrity_tag = hmac_hex(key, &self.signing_body()?)?;
        Ok(())
    }

    fn validate_authenticated(&self, key: &[u8]) -> Result<(), EvidenceError> {
        let expected = hmac_hex(key, &self.signing_body()?)?;
        if !constant_time_equal(&self.integrity_tag, &expected) {
            return Err(EvidenceError::IntegrityFailure(
                "verification report authority authentication mismatch".into(),
            ));
        }
        Ok(())
    }

    pub fn validate_integrity(&self) -> Result<(), EvidenceError> {
        Err(EvidenceError::IntegrityFailure(
            "verification report requires the internal authority key for authentication".into(),
        ))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcceptedRiskRecord {
    pub decision_id: String,
    pub requirement_id: String,
    pub actor: DecisionActor,
    pub reason: String,
    pub risk: String,
    pub mission_revision: u64,
    pub timestamp: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlockedExternalRecord {
    pub requirement_id: String,
    pub dependency: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidenceSummary {
    pub evidence_id: String,
    pub class: EvidenceClass,
    pub result: EvidenceResult,
    pub confidence: EvidenceConfidence,
    pub digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiVerifierInput {
    pub requirement_id: String,
    pub acceptance_criteria: Vec<String>,
    pub evidence: Vec<EvidenceSummary>,
    pub deterministic_gate_results: Vec<DeterministicGateResult>,
    pub explicit_decisions: Vec<ExplicitDecision>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AiJudgementKind {
    Supported,
    Unsupported,
    Inconclusive,
    Conflict,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiVerifierJudgement {
    #[serde(default)]
    pub requirement_id: String,
    pub kind: AiJudgementKind,
    pub reasoning_summary: String,
    pub evidence_ids: Vec<String>,
    pub confidence: EvidenceConfidence,
    pub identified_gaps: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AiProviderResult {
    // These fields are deliberately private. Only ProductionAiProvider in
    // this module can mint a receipt eligible for P8 authentication; callers
    // can pass the opaque result to the engine but cannot fabricate it.
    judgement: AiVerifierJudgement,
    provider_identity: String,
    response_digest: String,
}

/// Authenticated P8 authority input produced after the server-side provider
/// response has been parsed. Raw judgements are nested for auditability, but
/// the engine accepts only this HMAC-bound envelope in production.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthenticatedAiJudgement {
    pub mission_id: String,
    pub mission_revision: u64,
    pub project_id: String,
    pub requirement_id: String,
    pub p6_seal_hash: String,
    pub p7_execution_identity: String,
    pub ai_input_digest: String,
    pub evidence: Vec<EvidenceSummary>,
    pub provider_identity: String,
    pub judgement: AiVerifierJudgement,
    pub response_digest: String,
    pub issued_at_ms: u64,
    pub authority_digest: String,
    pub integrity: String,
}

impl AuthenticatedAiJudgement {
    fn signing_body(&self) -> Result<Vec<u8>, EvidenceError> {
        canonical(&(
            self.mission_id.clone(),
            self.mission_revision,
            self.project_id.clone(),
            self.requirement_id.clone(),
            self.p6_seal_hash.clone(),
            self.p7_execution_identity.clone(),
            self.ai_input_digest.clone(),
            self.evidence.clone(),
            self.provider_identity.clone(),
            self.judgement.clone(),
            self.response_digest.clone(),
            self.issued_at_ms,
            self.authority_digest.clone(),
        ))
    }

    fn authenticate(mut self, key: &[u8]) -> Result<Self, EvidenceError> {
        self.integrity = hmac_hex(key, &self.signing_body()?)?;
        Ok(self)
    }

    fn validate_integrity(&self, key: &[u8]) -> Result<(), EvidenceError> {
        if self.integrity != hmac_hex(key, &self.signing_body()?)? {
            return Err(EvidenceError::IntegrityFailure(
                "authenticated AI judgement integrity proof mismatch".into(),
            ));
        }
        Ok(())
    }
}

/// Parse the authenticated gateway payload.  Free-form provider text is never
/// promoted to a positive verification judgement.
pub fn parse_ai_gateway_response(
    input: &AiVerifierInput,
    response_text: &str,
) -> AiVerifierJudgement {
    let inconclusive = |reason: String, gaps: Vec<String>| AiVerifierJudgement {
        requirement_id: input.requirement_id.clone(),
        kind: AiJudgementKind::Inconclusive,
        reasoning_summary: reason,
        evidence_ids: Vec::new(),
        confidence: EvidenceConfidence::Weak,
        identified_gaps: gaps,
    };
    let value: serde_json::Value = match serde_json::from_str(response_text) {
        Ok(value) => value,
        Err(error) => {
            return inconclusive(
                "gateway response is not structured JSON".into(),
                vec![error.to_string()],
            )
        }
    };
    let Some(object) = value.as_object() else {
        return inconclusive(
            "gateway response is not a JSON object".into(),
            vec!["structured object required".into()],
        );
    };
    for field in [
        "status",
        "reasoning_summary",
        "evidence_ids",
        "confidence",
        "identified_gaps",
    ] {
        if !object.contains_key(field) {
            return inconclusive(
                "gateway response omitted a required structured field".into(),
                vec![format!("missing {field}")],
            );
        }
    }
    let status = object
        .get("status")
        .or_else(|| object.get("kind"))
        .and_then(serde_json::Value::as_str)
        .map(|item| item.to_ascii_uppercase());
    let kind = match status.as_deref() {
        Some("SUPPORTED") => AiJudgementKind::Supported,
        Some("UNSUPPORTED") => AiJudgementKind::Unsupported,
        Some("INCONCLUSIVE") => AiJudgementKind::Inconclusive,
        Some("CONFLICT") => AiJudgementKind::Conflict,
        _ => {
            return inconclusive(
                "gateway response status is unrecognized".into(),
                vec!["status must be SUPPORTED, UNSUPPORTED, INCONCLUSIVE, or CONFLICT".into()],
            )
        }
    };
    let reasoning_summary = object
        .get("reasoning_summary")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();
    let evidence_ids = object
        .get("evidence_ids")
        .and_then(serde_json::Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(serde_json::Value::as_str)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let identified_gaps = object
        .get("identified_gaps")
        .and_then(serde_json::Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(serde_json::Value::as_str)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let Some(confidence) = object
        .get("confidence")
        .cloned()
        .and_then(|item| serde_json::from_value::<EvidenceConfidence>(item).ok())
    else {
        return inconclusive(
            "gateway response confidence is invalid".into(),
            vec!["confidence must be a recognized EvidenceConfidence value".into()],
        );
    };
    let allowed = input
        .evidence
        .iter()
        .map(|item| item.evidence_id.as_str())
        .collect::<BTreeSet<_>>();
    if evidence_ids
        .iter()
        .any(|item| !allowed.contains(item.as_str()))
    {
        return inconclusive(
            "gateway response referenced evidence outside the minimized input".into(),
            vec!["evidence_ids are not bound to the request".into()],
        );
    }
    if reasoning_summary.is_empty() {
        return inconclusive(
            "gateway response omitted reasoning_summary".into(),
            vec!["reasoning_summary is required".into()],
        );
    }
    if kind == AiJudgementKind::Supported
        && (evidence_ids.is_empty() || !identified_gaps.is_empty())
    {
        return inconclusive(
            "gateway SUPPORTED response failed evidence/gap validation".into(),
            identified_gaps,
        );
    }
    AiVerifierJudgement {
        requirement_id: input.requirement_id.clone(),
        kind,
        reasoning_summary,
        evidence_ids,
        confidence,
        identified_gaps,
    }
}

#[cfg(feature = "test-support")]
pub trait IndependentAiProvider: Send + Sync {
    fn judge(&self, input: AiVerifierInput) -> Result<AiVerifierJudgement, String>;
}

#[cfg(feature = "test-support")]
pub struct IndependentAiVerifier<P: IndependentAiProvider> {
    provider: P,
}

/// Concrete gateway adapter. The renderer never receives the bearer; the
/// desktop native boundary supplies the authenticated Relintor cloud-session
/// access token retained in Rust memory. When configured it performs a real
/// authenticated request through the existing gateway crate.
pub struct ProductionAiProvider {
    gateway_endpoint: String,
    session_access_token: Option<String>,
}

impl ProductionAiProvider {
    pub fn new(gateway_endpoint: impl Into<String>, session_access_token: Option<String>) -> Self {
        Self {
            gateway_endpoint: gateway_endpoint.into(),
            session_access_token,
        }
    }

    pub fn configured(&self) -> bool {
        !self.gateway_endpoint.trim().is_empty()
            && self
                .session_access_token
                .as_deref()
                .is_some_and(|token| !token.trim().is_empty())
    }
}

impl ProductionAiProvider {
    pub fn judge_with_metadata(&self, input: AiVerifierInput) -> Result<AiProviderResult, String> {
        if !self.configured() {
            return Err(
                "P8_REAL_INDEPENDENT_AI_VERIFIER=PENDING_EXTERNAL_ENVIRONMENT: server-side gateway is unavailable"
                    .into(),
            );
        }
        let request = relintor_ai_gateway::GatewayRequest {
            request_id: format!("p8-ai-{}", sha256(input.requirement_id.as_bytes())),
            prompt: format!(
                "Review requirement {} against the supplied evidence digests. Return supported only when the evidence supports every criterion: {}",
                input.requirement_id,
                input.acceptance_criteria.join("; ")
            ),
            context: input
                .evidence
                .iter()
                .map(|item| relintor_ai_gateway::ContextItem {
                    source: item.evidence_id.clone(),
                    text: format!(
                        "class={:?}; result={:?}; confidence={:?}; digest={}",
                        item.class, item.result, item.confidence, item.digest
                    ),
                    excluded: false,
                })
                .collect(),
        };
        let response = relintor_ai_gateway::request_completion(
            &self.gateway_endpoint,
            self.session_access_token.as_deref().unwrap_or_default(),
            request,
        )
        .map_err(|error| {
            if error.contains("Connection") || error.contains("timed out") {
                format!("P8_REAL_INDEPENDENT_AI_VERIFIER=PENDING_EXTERNAL_ENVIRONMENT: {error}")
            } else {
                error
            }
        })?;
        Ok(AiProviderResult {
            judgement: parse_ai_gateway_response(&input, &response.text),
            provider_identity: "relintor-ai-gateway".into(),
            response_digest: sha256(response.text.as_bytes()),
        })
    }
}

#[cfg(feature = "test-support")]
impl IndependentAiProvider for ProductionAiProvider {
    fn judge(&self, input: AiVerifierInput) -> Result<AiVerifierJudgement, String> {
        self.judge_with_metadata(input)
            .map(|result| result.judgement)
    }
}

#[cfg(feature = "test-support")]
impl<P: IndependentAiProvider> IndependentAiVerifier<P> {
    pub fn new(provider: P) -> Self {
        Self { provider }
    }

    pub fn verify(&self, input: AiVerifierInput) -> Result<AiVerifierJudgement, EvidenceError> {
        if input.requirement_id.trim().is_empty() || input.acceptance_criteria.is_empty() {
            return Err(EvidenceError::InvalidInput(
                "AI verifier input is not minimized/structured".into(),
            ));
        }
        let requirement_id = input.requirement_id.clone();
        let mut judgement = self
            .provider
            .judge(input)
            .map_err(EvidenceError::CollectorUnavailable)?;
        if judgement.requirement_id.trim().is_empty() {
            judgement.requirement_id = requirement_id;
        }
        Ok(judgement)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProductionAiVerifierStatus {
    pub status: String,
    pub detail: String,
}

pub fn production_ai_verifier_status() -> ProductionAiVerifierStatus {
    ProductionAiVerifierStatus {
        status: "PENDING_EXTERNAL_ENVIRONMENT".into(),
        detail: "The server-side AI gateway boundary is available for injection; no live provider was configured for this local verification.".into(),
    }
}

#[derive(Debug, Clone)]
pub struct VerificationEngine {
    pub store: EvidenceStore,
    pub authority: VerificationAuthority,
    pub current: FreshnessContext,
    pub decisions: Vec<ExplicitDecision>,
    pub p7_execution: Option<AuthenticatedP7Execution>,
    ai_judgements: Vec<AiVerifierJudgement>,
}

impl VerificationEngine {
    fn new_internal(
        store: EvidenceStore,
        authority: VerificationAuthority,
        current: FreshnessContext,
        decisions: Vec<ExplicitDecision>,
    ) -> Result<Self, EvidenceError> {
        authority.validate()?;
        if current.mission_id != authority.revision.seal.mission_id
            || current.mission_revision != authority.revision.revision
            || current.p6_seal_hash != authority.revision.seal.contract_hash
            || current.environment_fingerprint != authority.environment_fingerprint
        {
            return Err(EvidenceError::InvalidAuthority(
                "freshness context does not match authority".into(),
            ));
        }
        Ok(Self {
            store,
            authority,
            current,
            decisions,
            p7_execution: None,
            ai_judgements: Default::default(),
        })
    }

    #[cfg(feature = "test-support")]
    pub fn new_for_test(
        store: EvidenceStore,
        authority: VerificationAuthority,
        current: FreshnessContext,
        decisions: Vec<ExplicitDecision>,
    ) -> Result<Self, EvidenceError> {
        Self::new_internal(store, authority, current, decisions)
    }

    pub fn new_with_p7_execution(
        store: EvidenceStore,
        authority: VerificationAuthority,
        current: FreshnessContext,
        decisions: Vec<ExplicitDecision>,
        p7_execution: AuthenticatedP7Execution,
    ) -> Result<Self, EvidenceError> {
        p7_execution.validate_against(&authority)?;
        let mut engine = Self::new_internal(store, authority, current, decisions)?;
        engine.p7_execution = Some(p7_execution);
        Ok(engine)
    }

    pub fn authenticate_ai_judgement(
        &self,
        input: &AiVerifierInput,
        provider_result: AiProviderResult,
    ) -> Result<AuthenticatedAiJudgement, EvidenceError> {
        self.authority.validate()?;
        let p7 = self.p7_execution.as_ref().ok_or_else(|| {
            EvidenceError::InvalidAuthority(
                "authenticated AI judgement requires a matching P7 execution identity".into(),
            )
        })?;
        p7.validate_against(&self.authority)?;
        if provider_result.judgement.requirement_id != input.requirement_id {
            return Err(EvidenceError::InvalidInput(
                "AI judgement requirement does not match minimized input".into(),
            ));
        }
        let artifacts = self.store.list()?;
        for evidence in &input.evidence {
            let artifact = artifacts
                .iter()
                .find(|item| item.metadata.evidence_id == evidence.evidence_id)
                .ok_or_else(|| {
                    EvidenceError::InvalidAuthority(format!(
                        "AI input references missing evidence: {}",
                        evidence.evidence_id
                    ))
                })?;
            if artifact.digest != evidence.digest
                || artifact.metadata.mission_id != self.authority.revision.seal.mission_id
                || artifact.metadata.mission_revision != self.authority.revision.revision
                || artifact.metadata.p6_seal_hash != self.authority.revision.seal.contract_hash
                || !p7.validates_evidence_metadata(&self.authority, &artifact.metadata)?
            {
                return Err(EvidenceError::InvalidAuthority(
                    "AI input evidence is not bound to the current sealed authority".into(),
                ));
            }
        }
        let envelope = AuthenticatedAiJudgement {
            mission_id: self.authority.revision.seal.mission_id.clone(),
            mission_revision: self.authority.revision.revision,
            project_id: self.authority.revision.seal.project_id.clone(),
            requirement_id: input.requirement_id.clone(),
            p6_seal_hash: self.authority.revision.seal.contract_hash.clone(),
            p7_execution_identity: p7.run.run_id.clone(),
            ai_input_digest: sha256(&canonical(input)?),
            evidence: input.evidence.clone(),
            provider_identity: provider_result.provider_identity,
            judgement: provider_result.judgement,
            response_digest: provider_result.response_digest,
            issued_at_ms: now_ms(),
            authority_digest: self.authority.identity_digest()?,
            integrity: String::new(),
        };
        envelope.authenticate(self.store.authority_key())
    }

    pub fn with_ai_judgements_authenticated(
        mut self,
        ai_judgements: Vec<AuthenticatedAiJudgement>,
    ) -> Result<Self, EvidenceError> {
        let authority_digest = self.authority.identity_digest()?;
        let artifacts = self.store.list()?;
        let p7 = self.p7_execution.as_ref().ok_or_else(|| {
            EvidenceError::InvalidAuthority(
                "authenticated AI judgement requires P7 execution".into(),
            )
        })?;
        for authenticated in ai_judgements {
            authenticated.validate_integrity(self.store.authority_key())?;
            if authenticated.mission_id != self.authority.revision.seal.mission_id
                || authenticated.mission_revision != self.authority.revision.revision
                || authenticated.project_id != self.authority.revision.seal.project_id
                || authenticated.p6_seal_hash != self.authority.revision.seal.contract_hash
                || authenticated.p7_execution_identity != p7.run.run_id
                || authenticated.authority_digest != authority_digest
                || authenticated.requirement_id != authenticated.judgement.requirement_id
                || authenticated.provider_identity.trim().is_empty()
                || authenticated.response_digest.len() != 64
                || authenticated.ai_input_digest.len() != 64
                || authenticated.issued_at_ms == 0
            {
                return Err(EvidenceError::InvalidAuthority(
                    "authenticated AI judgement does not match sealed P8 authority".into(),
                ));
            }
            for evidence in &authenticated.evidence {
                let artifact = artifacts
                    .iter()
                    .find(|item| item.metadata.evidence_id == evidence.evidence_id)
                    .ok_or_else(|| {
                        EvidenceError::InvalidAuthority(
                            "authenticated AI judgement references missing evidence".into(),
                        )
                    })?;
                if artifact.digest != evidence.digest
                    || !artifact
                        .metadata
                        .requirement_ids
                        .contains(&authenticated.requirement_id)
                    || !p7.validates_evidence_metadata(&self.authority, &artifact.metadata)?
                {
                    return Err(EvidenceError::InvalidAuthority(
                        "authenticated AI judgement evidence binding is invalid".into(),
                    ));
                }
            }
            self.ai_judgements.push(authenticated.judgement);
        }
        Ok(self)
    }

    #[cfg(feature = "test-support")]
    pub fn with_test_ai_judgements(
        mut self,
        ai_judgements: Vec<AiVerifierJudgement>,
    ) -> Result<Self, EvidenceError> {
        for judgement in &ai_judgements {
            if judgement.requirement_id.trim().is_empty()
                || !matches!(
                    judgement.kind,
                    AiJudgementKind::Supported
                        | AiJudgementKind::Unsupported
                        | AiJudgementKind::Inconclusive
                        | AiJudgementKind::Conflict
                )
            {
                return Err(EvidenceError::InvalidInput(
                    "AI judgement is not a structured authenticated reference".into(),
                ));
            }
        }
        self.ai_judgements = ai_judgements;
        Ok(self)
    }

    pub fn evaluate(
        &self,
        builder_claim: Option<String>,
    ) -> Result<VerificationReport, EvidenceError> {
        self.authority.validate()?;
        let artifacts = self.store.list()?;
        let mut statuses = Vec::new();
        let mut deterministic = Vec::new();
        let applicable = self
            .authority
            .revision
            .contract
            .requirement_graph
            .requirements
            .iter()
            .filter(|requirement| {
                !matches!(
                    requirement.status,
                    RequirementStatus::NotApplicable
                        | RequirementStatus::DeferredByExplicitDecision
                )
            })
            .collect::<Vec<_>>();
        for requirement in &self
            .authority
            .revision
            .contract
            .requirement_graph
            .requirements
        {
            let explicit = self
                .decisions
                .iter()
                .find(|decision| decision.requirement_id == requirement.requirement_id);
            if requirement.status == RequirementStatus::NotApplicable {
                statuses.push(RequirementVerification {
                    requirement_id: requirement.requirement_id.clone(),
                    status: RequirementStatus::NotApplicable,
                    evidence_ids: Vec::new(),
                    missing_obligations: Vec::new(),
                    missing_acceptance_criteria: Vec::new(),
                    stale_evidence: Vec::new(),
                    failed_evidence: Vec::new(),
                    reason: "sealed NOT_APPLICABLE state preserved".into(),
                });
                continue;
            }
            if requirement.status == RequirementStatus::DeferredByExplicitDecision
                && explicit.is_some_and(|decision| {
                    decision.kind == DecisionKind::Defer
                        && decision.mission_revision == self.authority.revision.revision
                })
            {
                statuses.push(RequirementVerification {
                    requirement_id: requirement.requirement_id.clone(),
                    status: RequirementStatus::DeferredByExplicitDecision,
                    evidence_ids: Vec::new(),
                    missing_obligations: Vec::new(),
                    missing_acceptance_criteria: Vec::new(),
                    stale_evidence: Vec::new(),
                    failed_evidence: Vec::new(),
                    reason: "sealed explicit deferral preserved".into(),
                });
                continue;
            }
            let mut matching = Vec::new();
            for artifact in &artifacts {
                if artifact.metadata.mission_id == self.authority.revision.seal.mission_id
                    && artifact.metadata.mission_revision == self.authority.revision.revision
                    && artifact.metadata.p6_seal_hash == self.authority.revision.seal.contract_hash
                    && artifact
                        .metadata
                        .requirement_ids
                        .contains(&requirement.requirement_id)
                    && self.p7_execution.as_ref().is_none_or(|p7| {
                        p7.validates_evidence_metadata(&self.authority, &artifact.metadata)
                            .is_ok_and(|valid| valid)
                    })
                {
                    matching.push(artifact);
                }
            }
            let mut evidence_ids = Vec::new();
            let mut stale = Vec::new();
            let mut failed = Vec::new();
            let mut classes = Vec::new();
            let mut missing_criteria = Vec::new();
            for artifact in &matching {
                match self
                    .store
                    .freshness(&artifact.metadata.evidence_id, &self.current)?
                {
                    EvidenceFreshness::Fresh => {
                        evidence_ids.push(artifact.metadata.evidence_id.clone());
                        if artifact.metadata.result == EvidenceResult::Fail {
                            failed.push(artifact.metadata.evidence_id.clone());
                            deterministic.push(DeterministicGateResult {
                                gate_id: artifact.metadata.evidence_id.clone(),
                                status: GateStatus::Fail,
                                evidence_id: Some(artifact.metadata.evidence_id.clone()),
                                detail: "deterministic evidence reported failure".into(),
                            });
                        } else if artifact.metadata.result == EvidenceResult::Skipped
                            || artifact.metadata.result == EvidenceResult::NotRun
                        {
                            deterministic.push(DeterministicGateResult {
                                gate_id: artifact.metadata.evidence_id.clone(),
                                status: GateStatus::Fail,
                                evidence_id: Some(artifact.metadata.evidence_id.clone()),
                                detail: "required test was skipped or not run".into(),
                            });
                        }
                    }
                    EvidenceFreshness::Stale
                    | EvidenceFreshness::Invalidated
                    | EvidenceFreshness::Unknown => {
                        stale.push(artifact.metadata.evidence_id.clone());
                    }
                }
            }
            for obligation in requirement
                .verification_policy
                .obligations
                .iter()
                .filter(|obligation| obligation.required)
            {
                let satisfied = matching.iter().any(|artifact| {
                    evidence_ids.contains(&artifact.metadata.evidence_id)
                        && artifact.metadata.class == obligation.class
                        && confidence_meets(
                            artifact.metadata.confidence,
                            obligation.minimum_confidence,
                        )
                        && artifact.metadata.result == EvidenceResult::Pass
                });
                if !satisfied && !classes.contains(&obligation.class) {
                    classes.push(obligation.class);
                }
            }
            for criterion in &requirement.acceptance_criteria {
                let deterministic_satisfied = matching.iter().any(|artifact| {
                    evidence_ids.contains(&artifact.metadata.evidence_id)
                        && artifact.metadata.result == EvidenceResult::Pass
                        && artifact
                            .metadata
                            .accepted_criteria
                            .contains(&criterion.criterion_id)
                });
                let human_or_runtime_satisfied = !criterion.machine_checkable
                    && matching.iter().any(|artifact| {
                        evidence_ids.contains(&artifact.metadata.evidence_id)
                            && artifact.metadata.result == EvidenceResult::Pass
                            && artifact
                                .metadata
                                .accepted_criteria
                                .contains(&criterion.criterion_id)
                            && matches!(
                                artifact.metadata.class,
                                EvidenceClass::HumanDecision
                                    | EvidenceClass::BrowserRecording
                                    | EvidenceClass::ApiResponse
                                    | EvidenceClass::DatabaseQuery
                            )
                    });
                let ai_satisfied = !criterion.machine_checkable
                    && self.ai_judgements.iter().any(|judgement| {
                        judgement.requirement_id == requirement.requirement_id
                            && judgement.kind == AiJudgementKind::Supported
                            && confidence_meets(
                                judgement.confidence,
                                EvidenceConfidence::AiReviewed,
                            )
                            && judgement
                                .evidence_ids
                                .iter()
                                .all(|id| evidence_ids.contains(id))
                            && !judgement.evidence_ids.is_empty()
                            && judgement.identified_gaps.is_empty()
                    });
                if !deterministic_satisfied && !human_or_runtime_satisfied && !ai_satisfied {
                    missing_criteria.push(criterion.criterion_id.clone());
                }
            }
            let status = if !failed.is_empty() {
                RequirementStatus::Failed
            } else if matching
                .iter()
                .any(|artifact| artifact.metadata.result == EvidenceResult::Blocked)
            {
                RequirementStatus::Blocked
            } else if classes.is_empty() && missing_criteria.is_empty() && stale.is_empty() {
                RequirementStatus::Verified
            } else {
                RequirementStatus::ImplementedUnverified
            };
            let reason = if status == RequirementStatus::Verified {
                "all required evidence is fresh, valid, and meets confidence".into()
            } else if !failed.is_empty() {
                "deterministic failure takes precedence over AI or builder claims".into()
            } else if !stale.is_empty() {
                "evidence is stale or invalidated".into()
            } else {
                "required evidence is missing or below minimum confidence".into()
            };
            statuses.push(RequirementVerification {
                requirement_id: requirement.requirement_id.clone(),
                status,
                evidence_ids,
                missing_obligations: classes,
                missing_acceptance_criteria: missing_criteria,
                stale_evidence: stale,
                failed_evidence: failed,
                reason,
            });
        }
        let mut accepted_risks = Vec::new();
        for decision in &self.decisions {
            if !matches!(
                decision.kind,
                DecisionKind::ExplicitException | DecisionKind::Defer
            ) || decision.mission_revision != self.authority.revision.revision
            {
                continue;
            }
            let requirement = self
                .authority
                .revision
                .contract
                .requirement_graph
                .requirements
                .iter()
                .find(|item| item.requirement_id == decision.requirement_id);
            if let Some(requirement) = requirement {
                if is_non_waivable(requirement) {
                    continue;
                }
                if matches!(decision.actor, DecisionActor::Human | DecisionActor::User) {
                    accepted_risks.push(AcceptedRiskRecord {
                        decision_id: decision.decision_id.clone(),
                        requirement_id: decision.requirement_id.clone(),
                        actor: decision.actor,
                        reason: decision.reason.clone(),
                        risk: decision.risk.clone(),
                        mission_revision: decision.mission_revision,
                        timestamp: decision.timestamp.clone(),
                    });
                }
            }
        }
        let blocked_external = statuses
            .iter()
            .filter(|item| item.status == RequirementStatus::Blocked)
            .map(|item| BlockedExternalRecord {
                requirement_id: item.requirement_id.clone(),
                dependency: "external verification dependency".into(),
                reason: item.reason.clone(),
            })
            .collect::<Vec<_>>();
        let failed = statuses
            .iter()
            .any(|item| item.status == RequirementStatus::Failed)
            || deterministic
                .iter()
                .any(|gate| gate.status == GateStatus::Fail);
        let unknown = statuses.iter().any(|item| {
            item.status == RequirementStatus::ImplementedUnverified
                || item.status == RequirementStatus::InProgress
                || item.status == RequirementStatus::Unstarted
        });
        let deferred_requirement_ids = statuses
            .iter()
            .filter(|item| item.status == RequirementStatus::DeferredByExplicitDecision)
            .map(|item| item.requirement_id.clone())
            .collect::<BTreeSet<_>>();
        let deferred_risks_authorized = deferred_requirement_ids.iter().all(|requirement_id| {
            accepted_risks
                .iter()
                .any(|risk| &risk.requirement_id == requirement_id)
        });
        let decision = if failed {
            CompletionDecision {
                state: CompletionState::FailedVerification,
                reason: "deterministic verification failure wins".into(),
                deterministic_gates: deterministic,
                accepted_risks,
                blocked_external,
            }
        } else if !blocked_external.is_empty() {
            CompletionDecision {
                state: CompletionState::BlockedExternal,
                reason: "required external proof is unavailable".into(),
                deterministic_gates: deterministic,
                accepted_risks,
                blocked_external,
            }
        } else if unknown
            || statuses.iter().any(|item| {
                item.status == RequirementStatus::Verified
                    && (!item.missing_obligations.is_empty() || !item.stale_evidence.is_empty())
            })
        {
            CompletionDecision {
                state: CompletionState::StoppedIncomplete,
                reason: "coverage is incomplete or evidence is unknown/stale".into(),
                deterministic_gates: deterministic,
                accepted_risks,
                blocked_external,
            }
        } else if !deferred_requirement_ids.is_empty() && !deferred_risks_authorized {
            CompletionDecision {
                state: CompletionState::StoppedIncomplete,
                reason: "each deferred requirement needs its own authorized accepted risk".into(),
                deterministic_gates: deterministic,
                accepted_risks,
                blocked_external,
            }
        } else if !accepted_risks.is_empty() {
            CompletionDecision {
                state: CompletionState::CompleteWithAcceptedRisks,
                reason: "all remaining deviations are explicitly accounted".into(),
                deterministic_gates: deterministic,
                accepted_risks,
                blocked_external,
            }
        } else if !deferred_requirement_ids.is_empty() {
            CompletionDecision {
                state: CompletionState::StoppedIncomplete,
                reason: "explicit deferral is accounted but has no accepted-risk authorization"
                    .into(),
                deterministic_gates: deterministic,
                accepted_risks,
                blocked_external,
            }
        } else if statuses.iter().all(|item| {
            matches!(
                item.status,
                RequirementStatus::Verified | RequirementStatus::NotApplicable
            )
        }) && statuses
            .iter()
            .filter(|item| item.status != RequirementStatus::NotApplicable)
            .count()
            == applicable.len()
        {
            CompletionDecision {
                state: CompletionState::VerifiedComplete,
                reason: "100% applicable requirement coverage is verified".into(),
                deterministic_gates: deterministic,
                accepted_risks,
                blocked_external,
            }
        } else {
            CompletionDecision {
                state: CompletionState::StoppedIncomplete,
                reason: "requirement coverage is not complete".into(),
                deterministic_gates: deterministic,
                accepted_risks,
                blocked_external,
            }
        };
        let coverage_total = statuses.len();
        let coverage_accounted = statuses
            .iter()
            .filter(|item| {
                !matches!(
                    item.status,
                    RequirementStatus::Unstarted
                        | RequirementStatus::InProgress
                        | RequirementStatus::ImplementedUnverified
                )
            })
            .count();
        let referenced_evidence_ids = statuses
            .iter()
            .flat_map(|status| status.evidence_ids.iter().cloned())
            .collect::<BTreeSet<_>>();
        let authority_digest = self.authority.identity_digest()?;
        let manifest_hash = EvidenceManifest {
            manifest_version: MANIFEST_VERSION.into(),
            mission_id: self.authority.revision.seal.mission_id.clone(),
            mission_revision: self.authority.revision.revision,
            p6_seal_hash: self.authority.revision.seal.contract_hash.clone(),
            authority_digest: authority_digest.clone(),
            requirements: statuses.clone(),
            evidence: artifacts
                .iter()
                .filter(|artifact| referenced_evidence_ids.contains(&artifact.metadata.evidence_id))
                .map(|artifact| EvidenceSummary {
                    evidence_id: artifact.metadata.evidence_id.clone(),
                    class: artifact.metadata.class,
                    result: artifact.metadata.result,
                    confidence: artifact.metadata.confidence,
                    digest: artifact.digest.clone(),
                })
                .collect(),
            invalidations: self
                .store
                .invalidations()?
                .into_iter()
                .map(|record| EvidenceInvalidationRecord {
                    evidence_id: record.evidence_id,
                    reason: record.reason,
                    impacted_requirements: BTreeSet::new(),
                    invalidated_at_ms: record.invalidated_at_ms,
                })
                .collect(),
            deterministic_gate_results: decision.deterministic_gates.clone(),
            ai_judgements: self.ai_judgements.clone(),
            explicit_decisions: self.decisions.clone(),
            final_state: decision.state,
            certificate_digest: None,
        }
        .authority_digest()?;
        let verification_run_id = format!(
            "p8-{}",
            sha256(&canonical(&(
                authority_digest.clone(),
                manifest_hash.clone(),
                now_ms()
            ))?)
        );
        let mut report = VerificationReport {
            verification_run_id,
            authority_digest,
            requirement_statuses: statuses,
            decision,
            builder_claim,
            coverage_total,
            coverage_accounted,
            evidence_manifest_hash: manifest_hash,
            p7_ledger_digest: self
                .p7_execution
                .as_ref()
                .map(|run| run.ledger_digest.clone()),
            ai_judgements: self.ai_judgements.clone(),
            integrity_tag: String::new(),
        };
        report.authenticate(self.store.authority_key())?;
        Ok(report)
    }
}

pub fn confidence_meets(actual: EvidenceConfidence, minimum: EvidenceConfidence) -> bool {
    fn rank(value: EvidenceConfidence) -> u8 {
        match value {
            EvidenceConfidence::Missing => 0,
            EvidenceConfidence::Weak => 1,
            EvidenceConfidence::HumanAsserted => 2,
            EvidenceConfidence::AiReviewed => 3,
            EvidenceConfidence::Corroborated => 4,
            EvidenceConfidence::StrongRuntime => 5,
            EvidenceConfidence::StrongDeterministic => 6,
        }
    }
    rank(actual) >= rank(minimum) && actual != EvidenceConfidence::Missing
}

fn is_non_waivable(requirement: &Requirement) -> bool {
    requirement.risk == relintor_standards::RequirementRisk::Critical
        || requirement
            .requirement_type
            .to_ascii_uppercase()
            .contains("NON_WAIVABLE")
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompletionCertificate {
    pub certificate_version: String,
    pub certificate_id: String,
    pub project_id: String,
    pub mission_id: String,
    pub mission_revision: u64,
    pub p6_seal_hash: String,
    pub registry_id: String,
    pub registry_version: u64,
    pub registry_digest: String,
    pub p7_execution_run_id: String,
    pub workspace_source_fingerprint: String,
    pub verification_run_id: String,
    pub requirement_status_ledger_hash: String,
    pub evidence_manifest_hash: String,
    pub accepted_risks: Vec<AcceptedRiskRecord>,
    pub blocked_external: Vec<BlockedExternalRecord>,
    pub final_state: CompletionState,
    pub issued_at_ms: u64,
    pub authority_version: String,
    pub signature: String,
}

#[derive(Debug, Clone)]
pub struct CompletionAuthority {
    key: Vec<u8>,
}

impl CompletionAuthority {
    pub fn new(key: &[u8]) -> Result<Self, EvidenceError> {
        if key.is_empty() {
            return Err(EvidenceError::InvalidInput(
                "completion authority key is empty".into(),
            ));
        }
        Ok(Self { key: key.to_vec() })
    }

    fn issue_internal(
        &self,
        report: &VerificationReport,
        authority: &VerificationAuthority,
    ) -> Result<CompletionCertificate, EvidenceError> {
        authority.validate()?;
        report.validate_authenticated(&self.key)?;
        if !matches!(
            report.decision.state,
            CompletionState::VerifiedComplete | CompletionState::CompleteWithAcceptedRisks
        ) {
            return Err(EvidenceError::InvalidCertificate(
                "completion is not eligible for a certificate".into(),
            ));
        }
        if report.authority_digest != authority.identity_digest()? {
            return Err(EvidenceError::InvalidCertificate(
                "report authority identity is stale".into(),
            ));
        }
        let mut certificate = CompletionCertificate {
            certificate_version: CERTIFICATE_VERSION.into(),
            certificate_id: format!(
                "cert-{}",
                sha256(&canonical(&(
                    report.verification_run_id.clone(),
                    report.evidence_manifest_hash.clone()
                ))?)
            ),
            project_id: authority.revision.seal.project_id.clone(),
            mission_id: authority.revision.seal.mission_id.clone(),
            mission_revision: authority.revision.revision,
            p6_seal_hash: authority.revision.seal.contract_hash.clone(),
            registry_id: authority.registry.registry_id.clone(),
            registry_version: authority.registry.registry_version,
            registry_digest: authority.registry.registry_digest.clone(),
            p7_execution_run_id: authority.p7_run_id.clone(),
            workspace_source_fingerprint: authority.workspace_fingerprint.clone(),
            verification_run_id: report.verification_run_id.clone(),
            requirement_status_ledger_hash: sha256(&canonical(&report.requirement_statuses)?),
            evidence_manifest_hash: report.evidence_manifest_hash.clone(),
            accepted_risks: report.decision.accepted_risks.clone(),
            blocked_external: report.decision.blocked_external.clone(),
            final_state: report.decision.state,
            issued_at_ms: now_ms(),
            authority_version: "p8-rust-completion-authority-v1".into(),
            signature: String::new(),
        };
        certificate.signature = hmac_hex(&self.key, &canonical(&certificate.signing_body())?)?;
        Ok(certificate)
    }

    #[cfg(feature = "test-support")]
    pub fn issue_for_test(
        &self,
        report: &VerificationReport,
        authority: &VerificationAuthority,
    ) -> Result<CompletionCertificate, EvidenceError> {
        self.issue_internal(report, authority)
    }

    pub fn issue_with_p7_execution(
        &self,
        report: &VerificationReport,
        authority: &VerificationAuthority,
        p7_execution: &AuthenticatedP7Execution,
    ) -> Result<CompletionCertificate, EvidenceError> {
        p7_execution.validate_against(authority)?;
        if report.p7_ledger_digest.as_deref() != Some(p7_execution.ledger_digest.as_str()) {
            return Err(EvidenceError::InvalidCertificate(
                "report is not bound to the authenticated P7 ledger".into(),
            ));
        }
        self.issue_internal(report, authority)
    }

    pub fn validate(
        &self,
        certificate: &CompletionCertificate,
        authority: &VerificationAuthority,
    ) -> Result<(), EvidenceError> {
        authority.validate()?;
        if certificate.certificate_version != CERTIFICATE_VERSION
            || certificate.mission_id != authority.revision.seal.mission_id
            || certificate.mission_revision != authority.revision.revision
            || certificate.p6_seal_hash != authority.revision.seal.contract_hash
            || certificate.registry_id != authority.registry.registry_id
            || certificate.registry_version != authority.registry.registry_version
            || certificate.registry_digest != authority.registry.registry_digest
            || certificate.p7_execution_run_id != authority.p7_run_id
            || certificate.workspace_source_fingerprint != authority.workspace_fingerprint
        {
            return Err(EvidenceError::InvalidCertificate(
                "certificate authority identity mismatch".into(),
            ));
        }
        let expected = hmac_hex(&self.key, &canonical(&certificate.signing_body())?)?;
        if !constant_time_equal(&certificate.signature, &expected) {
            return Err(EvidenceError::InvalidCertificate(
                "certificate signature mismatch".into(),
            ));
        }
        Ok(())
    }

    pub fn validate_with_store(
        &self,
        certificate: &CompletionCertificate,
        authority: &VerificationAuthority,
        store: &EvidenceStore,
        manifest: &EvidenceManifest,
        current: &FreshnessContext,
    ) -> Result<(), EvidenceError> {
        self.validate(certificate, authority)?;
        if certificate.evidence_manifest_hash != manifest.authority_digest()? {
            return Err(EvidenceError::InvalidCertificate(
                "certificate manifest digest is stale".into(),
            ));
        }
        store.validate_manifest(manifest)?;
        for evidence in &manifest.evidence {
            if store.freshness(&evidence.evidence_id, current)? != EvidenceFreshness::Fresh {
                return Err(EvidenceError::InvalidCertificate(
                    "certificate references stale or invalidated evidence".into(),
                ));
            }
        }
        Ok(())
    }
}

impl CompletionCertificate {
    fn signing_body(&self) -> CompletionCertificateBody {
        CompletionCertificateBody {
            certificate_version: self.certificate_version.clone(),
            certificate_id: self.certificate_id.clone(),
            project_id: self.project_id.clone(),
            mission_id: self.mission_id.clone(),
            mission_revision: self.mission_revision,
            p6_seal_hash: self.p6_seal_hash.clone(),
            registry_id: self.registry_id.clone(),
            registry_version: self.registry_version,
            registry_digest: self.registry_digest.clone(),
            p7_execution_run_id: self.p7_execution_run_id.clone(),
            workspace_source_fingerprint: self.workspace_source_fingerprint.clone(),
            verification_run_id: self.verification_run_id.clone(),
            requirement_status_ledger_hash: self.requirement_status_ledger_hash.clone(),
            evidence_manifest_hash: self.evidence_manifest_hash.clone(),
            accepted_risks: self.accepted_risks.clone(),
            blocked_external: self.blocked_external.clone(),
            final_state: self.final_state,
            issued_at_ms: self.issued_at_ms,
            authority_version: self.authority_version.clone(),
        }
    }

    pub fn digest(&self) -> Result<String, EvidenceError> {
        Ok(sha256(&canonical(self)?))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct CompletionCertificateBody {
    certificate_version: String,
    certificate_id: String,
    project_id: String,
    mission_id: String,
    mission_revision: u64,
    p6_seal_hash: String,
    registry_id: String,
    registry_version: u64,
    registry_digest: String,
    p7_execution_run_id: String,
    workspace_source_fingerprint: String,
    verification_run_id: String,
    requirement_status_ledger_hash: String,
    evidence_manifest_hash: String,
    accepted_risks: Vec<AcceptedRiskRecord>,
    blocked_external: Vec<BlockedExternalRecord>,
    final_state: CompletionState,
    issued_at_ms: u64,
    authority_version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidenceManifest {
    pub manifest_version: String,
    pub mission_id: String,
    pub mission_revision: u64,
    pub p6_seal_hash: String,
    pub authority_digest: String,
    pub requirements: Vec<RequirementVerification>,
    pub evidence: Vec<EvidenceSummary>,
    pub invalidations: Vec<EvidenceInvalidationRecord>,
    pub deterministic_gate_results: Vec<DeterministicGateResult>,
    pub ai_judgements: Vec<AiVerifierJudgement>,
    pub explicit_decisions: Vec<ExplicitDecision>,
    pub final_state: CompletionState,
    pub certificate_digest: Option<String>,
}

impl EvidenceManifest {
    pub fn authority_digest(&self) -> Result<String, EvidenceError> {
        Ok(sha256(&canonical(&(
            self.manifest_version.clone(),
            self.mission_id.clone(),
            self.mission_revision,
            self.p6_seal_hash.clone(),
            self.authority_digest.clone(),
            self.requirements.clone(),
            self.evidence.clone(),
            self.invalidations.clone(),
            self.deterministic_gate_results.clone(),
            self.ai_judgements.clone(),
            self.explicit_decisions.clone(),
            self.final_state,
        ))?))
    }

    pub fn digest(&self) -> Result<String, EvidenceError> {
        Ok(sha256(&canonical(self)?))
    }

    pub fn export_json(&self) -> Result<Vec<u8>, EvidenceError> {
        canonical(self)
    }
}

pub fn export_manifest(
    report: &VerificationReport,
    authority: &VerificationAuthority,
    store: &EvidenceStore,
    decisions: Vec<ExplicitDecision>,
    certificate: Option<&CompletionCertificate>,
) -> Result<EvidenceManifest, EvidenceError> {
    let artifacts = store.list()?;
    let referenced_evidence_ids = report
        .requirement_statuses
        .iter()
        .flat_map(|status| status.evidence_ids.iter().cloned())
        .collect::<BTreeSet<_>>();
    let evidence = artifacts
        .into_iter()
        .filter(|artifact| referenced_evidence_ids.contains(&artifact.metadata.evidence_id))
        .map(|artifact| EvidenceSummary {
            evidence_id: artifact.metadata.evidence_id,
            class: artifact.metadata.class,
            result: artifact.metadata.result,
            confidence: artifact.metadata.confidence,
            digest: artifact.digest,
        })
        .collect();
    let manifest = EvidenceManifest {
        manifest_version: MANIFEST_VERSION.into(),
        mission_id: authority.revision.seal.mission_id.clone(),
        mission_revision: authority.revision.revision,
        p6_seal_hash: authority.revision.seal.contract_hash.clone(),
        authority_digest: report.authority_digest.clone(),
        requirements: report.requirement_statuses.clone(),
        evidence,
        invalidations: store
            .invalidations()?
            .into_iter()
            .map(|record| EvidenceInvalidationRecord {
                evidence_id: record.evidence_id,
                reason: record.reason,
                impacted_requirements: BTreeSet::new(),
                invalidated_at_ms: record.invalidated_at_ms,
            })
            .collect(),
        deterministic_gate_results: report.decision.deterministic_gates.clone(),
        ai_judgements: report.ai_judgements.clone(),
        explicit_decisions: decisions,
        final_state: report.decision.state,
        certificate_digest: certificate.map(|item| item.digest()).transpose()?,
    };
    Ok(manifest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashing_is_deterministic() {
        assert_eq!(sha256(b"relintor"), sha256(b"relintor"));
        assert_ne!(sha256(b"a"), sha256(b"b"));
    }

    #[test]
    fn bounded_capture_marks_overflow() {
        let (bytes, overflow) = capture_bounded(std::io::Cursor::new(b"123456"), 3).unwrap();
        assert_eq!(bytes, b"123");
        assert!(overflow);
    }

    fn execution_identity() -> SuccessfulExecutionIdentity {
        SuccessfulExecutionIdentity {
            mission_id: "mission-a".into(),
            mission_revision: 1,
            seal_hash: "seal-a".into(),
            run_id: "run-a".into(),
            task_id: "task-a".into(),
            attempt_id: "attempt-a".into(),
            attempt_number: 1,
            packet_digest: "1".repeat(64),
            lease_id: "lease-a".into(),
            lease_digest: "2".repeat(64),
            process_digest: "3".repeat(64),
            workspace_identity: "4".repeat(64),
            workspace_before_fingerprint: "5".repeat(64),
            workspace_after_fingerprint: "6".repeat(64),
            artifact_changes: Vec::new(),
            started_at_ms: 10,
            ended_at_ms: 20,
            lease_expires_at_ms: 30,
        }
    }

    fn bound_metadata(identity: SuccessfulExecutionIdentity) -> EvidenceMetadata {
        EvidenceMetadata {
            evidence_id: "evidence-a".into(),
            class: EvidenceClass::TestOutput,
            mission_id: identity.mission_id.clone(),
            mission_revision: identity.mission_revision,
            p6_seal_hash: identity.seal_hash.clone(),
            requirement_ids: vec!["requirement-a".into()],
            task_id: Some(identity.task_id.clone()),
            p7_attempt: Some(identity.attempt_number),
            execution_identities: vec![identity],
            workspace_fingerprint: "workspace".into(),
            source_revision: Some("source".into()),
            collector: CollectorIdentity::new("collector", "1"),
            command_digest: "7".repeat(64),
            environment_fingerprint: "environment".into(),
            created_at_ms: 21,
            artifact_digest: "8".repeat(64),
            confidence: EvidenceConfidence::StrongDeterministic,
            freshness: EvidenceFreshness::Fresh,
            result: EvidenceResult::Pass,
            required: true,
            accepted_criteria: BTreeSet::new(),
            relevant_paths: BTreeSet::new(),
            test_inventory: Vec::new(),
            dependency_lock_hashes: BTreeMap::new(),
            scope_fingerprint: None,
        }
    }

    #[test]
    fn exact_binding_rejects_cross_attempt_lease_mission_and_revision() {
        let expected = execution_identity();
        let metadata = bound_metadata(expected.clone());
        assert!(exact_execution_binding_matches(
            &metadata,
            std::slice::from_ref(&expected)
        ));

        for mut wrong in [
            expected.clone(),
            expected.clone(),
            expected.clone(),
            expected.clone(),
        ] {
            if wrong.attempt_id == "attempt-a" {
                wrong.attempt_id = "attempt-b".into();
            }
            assert!(!exact_execution_binding_matches(&metadata, &[wrong]));
        }
        let mut wrong_lease = expected.clone();
        wrong_lease.lease_id = "lease-b".into();
        assert!(!exact_execution_binding_matches(&metadata, &[wrong_lease]));
        let mut wrong_mission = expected.clone();
        wrong_mission.mission_id = "mission-b".into();
        assert!(!exact_execution_binding_matches(
            &metadata,
            &[wrong_mission]
        ));
        let mut wrong_revision = expected;
        wrong_revision.mission_revision = 2;
        assert!(!exact_execution_binding_matches(
            &metadata,
            &[wrong_revision]
        ));
    }

    #[test]
    fn orphan_blob_from_interrupted_evidence_commit_is_not_evidence() {
        let root = tempfile::tempdir().unwrap();
        let store = EvidenceStore::new(root.path(), b"test-key").unwrap();
        fs::write(root.path().join("blobs").join("a".repeat(64)), b"orphan").unwrap();
        assert!(store.list().unwrap().is_empty());
        assert!(matches!(
            store.load("interrupted"),
            Err(EvidenceError::EvidenceNotFound(_))
        ));
    }
}
