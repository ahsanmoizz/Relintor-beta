//! Structured New Project Investigator for Relintor P4.
//!
//! This crate deliberately keeps investigation evidence, questions, decisions,
//! and blueprint state as typed records. A deterministic provider is included
//! for local verification; it is never presented as production AI quality.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub const INVESTIGATOR_PROTOCOL_VERSION: &str = "p4-investigator-1";
pub const INVESTIGATOR_POLICY_VERSION: &str = "p4-policy-1";
pub const DEFAULT_MAX_SOURCE_BYTES: u64 = 2 * 1024 * 1024;
pub const DEFAULT_CONTEXT_CHARS: usize = 24_000;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Provenance {
    pub source_id: String,
    pub locator: String,
    pub content_hash: String,
    pub note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum InputKind {
    Idea,
    Document,
    Sketch,
    Screenshot,
    RepositoryReference,
    DeploymentExpectation,
    UserAnswer,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SourceKind {
    TypedText,
    TextDocument,
    ImageSketch,
    ImageScreenshot,
    RepositoryTemplate,
    Unsupported,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ExtractionStatus {
    Complete,
    MetadataOnly,
    Unsupported,
    Failed,
    NotAttempted,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InvestigationInput {
    pub id: String,
    pub kind: InputKind,
    pub value: String,
    pub provenance: Provenance,
    pub created_at: i128,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SourceDocument {
    pub id: String,
    pub display_name: String,
    pub canonical_path: Option<String>,
    pub mime_type: String,
    pub size_bytes: u64,
    pub content_hash: String,
    pub source_kind: SourceKind,
    pub extraction_status: ExtractionStatus,
    pub extracted_text: Option<String>,
    pub provenance: Provenance,
    pub created_at: i128,
    pub updated_at: i128,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProjectDraft {
    pub id: String,
    pub name: String,
    pub idea: String,
    pub provenance: Provenance,
    pub created_at: i128,
    pub updated_at: i128,
    pub version: u32,
}

impl ProjectDraft {
    pub fn from_idea(idea: &str) -> Result<Self, IntakeError> {
        let idea = idea.trim();
        if idea.is_empty() {
            return Err(IntakeError::EmptyIdea);
        }
        let hash = sha256_hex(idea.as_bytes());
        let now = now_ms();
        Ok(Self {
            id: stable_id("project", &[idea]),
            name: project_name(idea),
            idea: idea.into(),
            provenance: Provenance {
                source_id: stable_id("idea", &[idea]),
                locator: "user://typed-idea".into(),
                content_hash: hash,
                note: "Typed by the project owner; not independently verified.".into(),
            },
            created_at: now,
            updated_at: now,
            version: 1,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum InvestigationStatus {
    Draft,
    NeedsAnswers,
    NeedsConflictResolution,
    ReadyForReview,
    Approved,
    Superseded,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ClaimClassification {
    ExplicitFact,
    DerivedInference,
    UserConfirmed,
    Assumption,
    Unknown,
    Conflict,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IntentClaim {
    pub id: String,
    pub field: String,
    pub value: String,
    pub classification: ClaimClassification,
    pub provenance: Vec<Provenance>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IntentModel {
    pub product_purpose: Option<IntentClaim>,
    pub user_problem: Option<IntentClaim>,
    pub product_type: Option<IntentClaim>,
    pub deployment_expectation: Option<IntentClaim>,
    pub operational_expectation: Option<IntentClaim>,
    pub data_involved: Vec<IntentClaim>,
    pub external_integrations: Vec<IntentClaim>,
    pub security_sensitivity: Option<IntentClaim>,
    pub privacy_sensitivity: Option<IntentClaim>,
    pub financial_exposure: Option<IntentClaim>,
    pub compliance_hints: Vec<IntentClaim>,
    pub platform_targets: Vec<IntentClaim>,
    pub explicit_constraints: Vec<IntentClaim>,
    pub unknowns: Vec<IntentClaim>,
    pub contradictions: Vec<IntentClaim>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Persona {
    pub id: String,
    pub name: String,
    pub functional_role: String,
    pub needs: Vec<String>,
    pub provenance: Vec<Provenance>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UserJourney {
    pub id: String,
    pub persona_id: String,
    pub starting_condition: String,
    pub goal: String,
    pub major_steps: Vec<String>,
    pub success_state: String,
    pub failure_states: Vec<String>,
    pub provenance: Vec<Provenance>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum NfrClassification {
    Explicit,
    InferredRequired,
    Recommended,
    NotApplicable,
    Unresolved,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NonFunctionalRequirement {
    pub id: String,
    pub domain: String,
    pub statement: String,
    pub classification: NfrClassification,
    pub rationale: String,
    pub provenance: Vec<Provenance>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct QuestionOption {
    pub id: String,
    pub label: String,
    pub consequence: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum QuestionDisposition {
    Asked,
    Suppressed,
    Deferred,
    AutoDefaulted,
    Answered,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InvestigatorQuestion {
    pub id: String,
    pub question: String,
    pub why_it_matters: String,
    pub recommended_default: String,
    pub options: Vec<QuestionOption>,
    pub can_defer: bool,
    pub affects: Vec<String>,
    pub source_ambiguity: String,
    pub disposition: QuestionDisposition,
    pub provenance: Vec<Provenance>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum AnswerChoice {
    Selected(String),
    NotSure,
    Deferred,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UserAnswer {
    pub id: String,
    pub question_id: String,
    pub choice: AnswerChoice,
    pub explanation: String,
    pub confirmed: bool,
    pub provenance: Provenance,
    pub created_at: i128,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum RegisterStatus {
    Open,
    Resolved,
    Superseded,
    Accepted,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Assumption {
    pub id: String,
    pub statement: String,
    pub source_reason: String,
    pub affected_decisions: Vec<String>,
    pub confidence: String,
    pub consequence_if_wrong: String,
    pub validation_trigger: String,
    pub status: RegisterStatus,
    pub provenance: Vec<Provenance>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Risk {
    pub id: String,
    pub title: String,
    pub description: String,
    pub category: String,
    pub source: String,
    pub likelihood: String,
    pub impact: String,
    pub severity: String,
    pub affected_area: String,
    pub mitigation: String,
    pub owner: Option<String>,
    pub status: RegisterStatus,
    pub provenance: Vec<Provenance>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum DecisionStatus {
    Proposed,
    UserConfirmed,
    DefaultAccepted,
    Deferred,
    Superseded,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArchitectureDecision {
    pub id: String,
    pub context: String,
    pub decision_question: String,
    pub considered_options: Vec<String>,
    pub selected_option: Option<String>,
    pub reason: String,
    pub tradeoffs: Vec<String>,
    pub assumptions: Vec<String>,
    pub source: Vec<Provenance>,
    pub status: DecisionStatus,
    pub future_reconsideration_trigger: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ConflictSeverity {
    Informational,
    Material,
    Critical,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ConflictResolutionState {
    Unresolved,
    Resolved,
    Deferred,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Conflict {
    pub id: String,
    pub claim_a: String,
    pub claim_b: String,
    pub source_a: Provenance,
    pub source_b: Provenance,
    pub severity: ConflictSeverity,
    pub affected_blueprint_areas: Vec<String>,
    pub recommended_resolution: String,
    pub resolution_state: ConflictResolutionState,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CandidateStandardDomain {
    pub id: String,
    pub domain: String,
    pub rationale: String,
    pub provenance: Vec<Provenance>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CandidateRequirement {
    pub id: String,
    pub statement: String,
    pub domain: String,
    pub rationale: String,
    pub provenance: Vec<Provenance>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CandidateEvidenceNeed {
    pub id: String,
    pub evidence_type: String,
    pub purpose: String,
    pub related_requirement: Option<String>,
    pub provenance: Vec<Provenance>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Blueprint {
    pub id: String,
    pub revision: u32,
    pub status: BlueprintStatus,
    pub product_definition: String,
    pub problem_outcome: String,
    pub product_ontology: Vec<String>,
    pub intent: IntentModel,
    pub personas: Vec<Persona>,
    pub journeys: Vec<UserJourney>,
    pub capabilities: Vec<String>,
    pub non_functional_requirements: Vec<NonFunctionalRequirement>,
    pub data_outline: Vec<String>,
    pub integration_expectations: Vec<String>,
    pub deployment_expectations: Vec<String>,
    pub architecture_decisions: Vec<ArchitectureDecision>,
    pub assumptions: Vec<Assumption>,
    pub risks: Vec<Risk>,
    pub conflicts: Vec<Conflict>,
    pub questions: Vec<InvestigatorQuestion>,
    pub candidate_standard_domains: Vec<CandidateStandardDomain>,
    pub candidate_requirements: Vec<CandidateRequirement>,
    pub candidate_evidence_needs: Vec<CandidateEvidenceNeed>,
    pub exclusions: Vec<String>,
    pub provenance: Vec<Provenance>,
    pub fingerprint: String,
    pub created_at: i128,
    pub updated_at: i128,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum BlueprintStatus {
    Draft,
    NeedsAnswers,
    NeedsConflictResolution,
    ReadyForReview,
    Approved,
    Superseded,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BlueprintRevision {
    pub id: String,
    pub blueprint_id: String,
    pub revision: u32,
    pub changed_answer_ids: Vec<String>,
    pub invalidated_decision_ids: Vec<String>,
    pub blueprint: Blueprint,
    pub created_at: i128,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BlueprintApproval {
    pub id: String,
    pub blueprint_revision_id: String,
    pub approved_by: String,
    pub approved_at: i128,
    pub note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderMetadata {
    pub adapter: String,
    pub model: String,
    pub live: bool,
    pub availability: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Investigation {
    pub id: String,
    pub project_id: String,
    pub protocol_version: String,
    pub policy_version: String,
    pub status: InvestigationStatus,
    pub input_hashes: Vec<String>,
    pub provider: ProviderMetadata,
    pub created_at: i128,
    pub updated_at: i128,
    pub version: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InvestigationResult {
    pub project: ProjectDraft,
    pub investigation: Investigation,
    pub inputs: Vec<InvestigationInput>,
    pub sources: Vec<SourceDocument>,
    pub answers: Vec<UserAnswer>,
    pub blueprint: Blueprint,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InvestigationView {
    pub investigation: Investigation,
    pub blueprint: Blueprint,
}

impl From<&InvestigationResult> for InvestigationView {
    fn from(result: &InvestigationResult) -> Self {
        Self {
            investigation: result.investigation.clone(),
            blueprint: result.blueprint.clone(),
        }
    }
}

/// Persist structured investigator records without storing extracted source
/// contents. Source metadata, hashes, and provenance remain available for
/// review; raw project material stays at its local source boundary.
pub fn persist_result(path: &Path, result: &InvestigationResult) -> Result<(), String> {
    let mut connection = rusqlite::Connection::open(path)
        .map_err(|error| format!("open investigator database: {error}"))?;
    let transaction = connection
        .transaction()
        .map_err(|error| format!("begin investigator persistence: {error}"))?;
    transaction
        .execute(
            "INSERT INTO projects(id, name, root_path, created_at) VALUES (?1, ?2, NULL, ?3)
             ON CONFLICT(id) DO UPDATE SET name=excluded.name",
            rusqlite::params![
                result.project.id,
                result.project.name,
                result.project.created_at as i64
            ],
        )
        .map_err(|error| format!("persist canonical project: {error}"))?;
    transaction
        .execute(
            "INSERT OR REPLACE INTO project_drafts(id, name, idea, created_at, updated_at, version) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![result.project.id, result.project.name, result.project.idea, result.project.created_at as i64, result.project.updated_at as i64, result.project.version],
        )
        .map_err(|error| format!("persist project draft: {error}"))?;
    let investigation_json = serde_json::to_string(&InvestigationView::from(result))
        .map_err(|error| format!("serialize investigation view: {error}"))?;
    transaction
        .execute(
            "INSERT INTO project_workflow_state(project_id, investigation_json, updated_at)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(project_id) DO UPDATE SET investigation_json=excluded.investigation_json,
             updated_at=excluded.updated_at",
            rusqlite::params![result.project.id, investigation_json, now_ms() as i64],
        )
        .map_err(|error| format!("persist project workflow state: {error}"))?;
    transaction
        .execute(
            "INSERT OR REPLACE INTO investigations(id, project_id, protocol_version, policy_version, status, fingerprint, created_at, updated_at, version) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            rusqlite::params![result.investigation.id, result.investigation.project_id, result.investigation.protocol_version, result.investigation.policy_version, format!("{:?}", result.investigation.status), result.blueprint.fingerprint, result.investigation.created_at as i64, result.investigation.updated_at as i64, result.investigation.version],
        )
        .map_err(|error| format!("persist investigation: {error}"))?;
    for input in &result.inputs {
        transaction.execute(
            "INSERT OR REPLACE INTO investigation_inputs(id, investigation_id, kind, value, provenance, content_hash, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![input.id, result.investigation.id, format!("{:?}", input.kind), input.value, to_json(&input.provenance)?, input.provenance.content_hash, input.created_at as i64],
        ).map_err(|error| format!("persist investigation input: {error}"))?;
    }
    for source in &result.sources {
        transaction.execute(
            "INSERT OR REPLACE INTO source_documents(id, investigation_id, display_name, canonical_path, mime_type, size_bytes, content_hash, extraction_status, source_kind, provenance, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            rusqlite::params![source.id, result.investigation.id, source.display_name, source.canonical_path, source.mime_type, source.size_bytes as i64, source.content_hash, format!("{:?}", source.extraction_status), format!("{:?}", source.source_kind), to_json(&source.provenance)?, source.created_at as i64, source.updated_at as i64],
        ).map_err(|error| format!("persist source document: {error}"))?;
    }
    for question in &result.blueprint.questions {
        transaction.execute(
            "INSERT OR REPLACE INTO investigator_questions(id, investigation_id, question, why_it_matters, recommended_default, disposition, source_ambiguity, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            rusqlite::params![question.id, result.investigation.id, question.question, question.why_it_matters, question.recommended_default, format!("{:?}", question.disposition), question.source_ambiguity, now_ms() as i64],
        ).map_err(|error| format!("persist investigator question: {error}"))?;
    }
    for answer in &result.answers {
        transaction.execute(
            "INSERT OR REPLACE INTO user_answers(id, question_id, answer, confirmed, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![answer.id, answer.question_id, to_json(&answer.choice)?, answer.confirmed as i64, answer.created_at as i64],
        ).map_err(|error| format!("persist user answer: {error}"))?;
    }
    for decision in &result.blueprint.architecture_decisions {
        transaction.execute(
            "INSERT OR REPLACE INTO architecture_decisions(id, investigation_id, decision, selected_option, status, source, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![decision.id, result.investigation.id, decision.decision_question, decision.selected_option, format!("{:?}", decision.status), to_json(&decision.source)?, now_ms() as i64],
        ).map_err(|error| format!("persist architecture decision: {error}"))?;
    }
    for risk in &result.blueprint.risks {
        transaction.execute(
            "INSERT OR REPLACE INTO risks(id, investigation_id, title, category, severity, status, source, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            rusqlite::params![risk.id, result.investigation.id, risk.title, risk.category, risk.severity, format!("{:?}", risk.status), risk.source, now_ms() as i64],
        ).map_err(|error| format!("persist risk: {error}"))?;
    }
    for assumption in &result.blueprint.assumptions {
        transaction.execute(
            "INSERT OR REPLACE INTO assumptions(id, investigation_id, statement, confidence, status, source, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![assumption.id, result.investigation.id, assumption.statement, assumption.confidence, format!("{:?}", assumption.status), assumption.source_reason, now_ms() as i64],
        ).map_err(|error| format!("persist assumption: {error}"))?;
    }
    for conflict in &result.blueprint.conflicts {
        transaction.execute(
            "INSERT OR REPLACE INTO conflicts(id, investigation_id, claim_a, claim_b, severity, resolution_state, source_a, source_b, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            rusqlite::params![conflict.id, result.investigation.id, conflict.claim_a, conflict.claim_b, format!("{:?}", conflict.severity), format!("{:?}", conflict.resolution_state), to_json(&conflict.source_a)?, to_json(&conflict.source_b)?, now_ms() as i64],
        ).map_err(|error| format!("persist conflict: {error}"))?;
    }
    let normalized = normalized_blueprint(&result.blueprint)?;
    transaction.execute(
        "INSERT OR REPLACE INTO blueprint_revisions(id, investigation_id, revision, status, fingerprint, normalized_blueprint, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        rusqlite::params![result.blueprint.id, result.investigation.id, result.blueprint.revision, format!("{:?}", result.blueprint.status), result.blueprint.fingerprint, normalized, result.blueprint.created_at as i64],
    ).map_err(|error| format!("persist blueprint revision: {error}"))?;
    for candidate in result
        .blueprint
        .candidate_standard_domains
        .iter()
        .map(|candidate| {
            (
                &candidate.id,
                "standard_domain",
                &candidate.domain,
                &candidate.rationale,
            )
        })
        .chain(
            result
                .blueprint
                .candidate_requirements
                .iter()
                .map(|candidate| {
                    (
                        &candidate.id,
                        "requirement",
                        &candidate.statement,
                        &candidate.rationale,
                    )
                }),
        )
        .chain(
            result
                .blueprint
                .candidate_evidence_needs
                .iter()
                .map(|candidate| {
                    (
                        &candidate.id,
                        "evidence_need",
                        &candidate.evidence_type,
                        &candidate.purpose,
                    )
                }),
        )
    {
        transaction.execute(
            "INSERT OR REPLACE INTO blueprint_candidates(id, blueprint_revision_id, candidate_type, title, rationale, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![candidate.0, result.blueprint.id, candidate.1, candidate.2, candidate.3, now_ms() as i64],
        ).map_err(|error| format!("persist blueprint candidate: {error}"))?;
    }
    transaction
        .commit()
        .map_err(|error| format!("commit investigator persistence: {error}"))
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InvestigatorConfig {
    pub max_source_bytes: u64,
    pub max_context_chars: usize,
    pub allowed_root: Option<PathBuf>,
}

impl Default for InvestigatorConfig {
    fn default() -> Self {
        Self {
            max_source_bytes: DEFAULT_MAX_SOURCE_BYTES,
            max_context_chars: DEFAULT_CONTEXT_CHARS,
            allowed_root: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IntakeError {
    EmptyIdea,
    FileNotFound(String),
    PathOutsideAllowedRoot(String),
    FileTooLarge { bytes: u64, limit: u64 },
    UnsupportedFileType(String),
    FailedExtraction(String),
    DuplicateDocument(String),
}

impl std::fmt::Display for IntakeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyIdea => f.write_str("project idea cannot be empty"),
            Self::FileNotFound(path) => write!(f, "source file does not exist: {path}"),
            Self::PathOutsideAllowedRoot(path) => {
                write!(f, "source path is outside the allowed root: {path}")
            }
            Self::FileTooLarge { bytes, limit } => {
                write!(f, "source file is {bytes} bytes; maximum is {limit}")
            }
            Self::UnsupportedFileType(extension) => {
                write!(f, "unsupported source file type: {extension}")
            }
            Self::FailedExtraction(detail) => write!(f, "source extraction failed: {detail}"),
            Self::DuplicateDocument(hash) => write!(f, "duplicate source document: {hash}"),
        }
    }
}

impl std::error::Error for IntakeError {}

pub struct IntakeService {
    pub config: InvestigatorConfig,
}

impl IntakeService {
    pub fn new(config: InvestigatorConfig) -> Self {
        Self { config }
    }

    pub fn intake_text(
        &self,
        display_name: &str,
        text: &str,
    ) -> Result<SourceDocument, IntakeError> {
        let bytes = text.as_bytes();
        self.check_size(bytes.len() as u64)?;
        if text.trim().is_empty() {
            return Err(IntakeError::FailedExtraction("text source is empty".into()));
        }
        let hash = sha256_hex(bytes);
        let id = stable_id("source", &[display_name, &hash]);
        let provenance = Provenance {
            source_id: id.clone(),
            locator: format!("user://text/{display_name}"),
            content_hash: hash.clone(),
            note: "Typed or pasted by the project owner; not independently verified.".into(),
        };
        Ok(SourceDocument {
            id,
            display_name: display_name.into(),
            canonical_path: None,
            mime_type: "text/plain".into(),
            size_bytes: bytes.len() as u64,
            content_hash: hash,
            source_kind: SourceKind::TypedText,
            extraction_status: ExtractionStatus::Complete,
            extracted_text: Some(text.into()),
            provenance,
            created_at: now_ms(),
            updated_at: now_ms(),
        })
    }

    pub fn intake_file(&self, path: &Path) -> Result<SourceDocument, IntakeError> {
        let canonical = path
            .canonicalize()
            .map_err(|_| IntakeError::FileNotFound(path.display().to_string()))?;
        if let Some(root) = &self.config.allowed_root {
            let root = root
                .canonicalize()
                .map_err(|_| IntakeError::PathOutsideAllowedRoot(root.display().to_string()))?;
            if !canonical.starts_with(&root) {
                return Err(IntakeError::PathOutsideAllowedRoot(
                    canonical.display().to_string(),
                ));
            }
        }
        let metadata = fs::metadata(&canonical)
            .map_err(|_| IntakeError::FileNotFound(canonical.display().to_string()))?;
        self.check_size(metadata.len())?;
        let bytes =
            fs::read(&canonical).map_err(|e| IntakeError::FailedExtraction(e.to_string()))?;
        let hash = sha256_hex(&bytes);
        let display_name = canonical
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("source")
            .to_string();
        let extension = canonical
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let id = stable_id("source", &[&canonical.to_string_lossy(), &hash]);
        let (source_kind, extraction_status, mime_type, extracted_text) =
            if is_image_extension(&extension) {
                let kind = if extension == "svg" {
                    SourceKind::ImageSketch
                } else {
                    SourceKind::ImageScreenshot
                };
                (
                    kind,
                    ExtractionStatus::MetadataOnly,
                    mime_for(&extension),
                    None,
                )
            } else if is_text_extension(&extension) {
                match String::from_utf8(bytes) {
                    Ok(text) => (
                        SourceKind::TextDocument,
                        ExtractionStatus::Complete,
                        mime_for(&extension),
                        Some(text),
                    ),
                    Err(_) => (
                        SourceKind::TextDocument,
                        ExtractionStatus::Failed,
                        mime_for(&extension),
                        None,
                    ),
                }
            } else {
                (
                    SourceKind::Unsupported,
                    ExtractionStatus::Unsupported,
                    "application/octet-stream".into(),
                    None,
                )
            };
        Ok(SourceDocument {
            id: id.clone(),
            display_name,
            canonical_path: Some(canonical.display().to_string()),
            mime_type,
            size_bytes: metadata.len(),
            content_hash: hash.clone(),
            source_kind,
            extraction_status,
            extracted_text,
            provenance: Provenance {
                source_id: id,
                locator: format!("file://{}", canonical.display()),
                content_hash: hash,
                note: "User-selected local source; selection is not proof of truth.".into(),
            },
            created_at: now_ms(),
            updated_at: now_ms(),
        })
    }

    pub fn reject_duplicate(
        &self,
        existing: &[SourceDocument],
        candidate: &SourceDocument,
    ) -> Result<(), IntakeError> {
        if existing
            .iter()
            .any(|source| source.content_hash == candidate.content_hash)
        {
            return Err(IntakeError::DuplicateDocument(
                candidate.content_hash.clone(),
            ));
        }
        Ok(())
    }

    pub fn intake_files(&self, paths: &[PathBuf]) -> Result<Vec<SourceDocument>, IntakeError> {
        let mut documents = Vec::with_capacity(paths.len());
        for path in paths {
            let document = self.intake_file(path)?;
            self.reject_duplicate(&documents, &document)?;
            documents.push(document);
        }
        Ok(documents)
    }

    fn check_size(&self, bytes: u64) -> Result<(), IntakeError> {
        if bytes > self.config.max_source_bytes {
            Err(IntakeError::FileTooLarge {
                bytes,
                limit: self.config.max_source_bytes,
            })
        } else {
            Ok(())
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GatewayContextItem {
    pub source_id: String,
    pub text: String,
    pub redacted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GatewayPreparation {
    pub items: Vec<GatewayContextItem>,
    pub excluded_count: usize,
    pub redacted_count: usize,
}

pub fn prepare_minimal_gateway_context(
    sources: &[SourceDocument],
    max_chars: usize,
) -> GatewayPreparation {
    let mut ordered = sources.to_vec();
    ordered.sort_by(|a, b| a.id.cmp(&b.id));
    let mut items = Vec::new();
    let mut excluded_count = 0;
    let mut redacted_count = 0;
    let mut used = 0;
    for source in ordered {
        let Some(text) = source.extracted_text else {
            excluded_count += 1;
            continue;
        };
        if source.extraction_status != ExtractionStatus::Complete
            || contains_secret(&text)
            || contains_secret(&source.display_name)
        {
            redacted_count += 1;
            continue;
        }
        let remaining = max_chars.saturating_sub(used);
        if remaining == 0 {
            excluded_count += 1;
            continue;
        }
        let text = text.chars().take(remaining).collect::<String>();
        used += text.chars().count();
        items.push(GatewayContextItem {
            source_id: source.id,
            text,
            redacted: false,
        });
    }
    GatewayPreparation {
        items,
        excluded_count,
        redacted_count,
    }
}

pub trait InvestigatorProvider: Send + Sync {
    fn metadata(&self) -> ProviderMetadata;
    fn analyze(&self, request: ProviderRequest) -> Result<ProviderResponse, String>;
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderRequest {
    pub idea: String,
    pub context: GatewayPreparation,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderResponse {
    pub advisory_summary: String,
    pub provenance: Vec<String>,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct DeterministicProvider;

impl InvestigatorProvider for DeterministicProvider {
    fn metadata(&self) -> ProviderMetadata {
        ProviderMetadata {
            adapter: "deterministic-development".into(),
            model: "relintor-investigator-fixture-v1".into(),
            live: false,
            availability: "available_development_adapter".into(),
        }
    }

    fn analyze(&self, request: ProviderRequest) -> Result<ProviderResponse, String> {
        if request.idea.trim().is_empty() {
            return Err("idea is empty".into());
        }
        Ok(ProviderResponse {
            advisory_summary: format!(
                "Deterministic advisory analysis for: {}",
                request.idea.trim()
            ),
            provenance: request
                .context
                .items
                .into_iter()
                .map(|item| item.source_id)
                .collect(),
        })
    }
}

pub struct Investigator<P: InvestigatorProvider> {
    pub provider: P,
    pub config: InvestigatorConfig,
}

impl Default for Investigator<DeterministicProvider> {
    fn default() -> Self {
        Self {
            provider: DeterministicProvider,
            config: InvestigatorConfig::default(),
        }
    }
}

impl<P: InvestigatorProvider> Investigator<P> {
    pub fn investigate(
        &self,
        project: ProjectDraft,
        sources: Vec<SourceDocument>,
        answers: &[UserAnswer],
    ) -> Result<InvestigationResult, String> {
        let preparation = prepare_minimal_gateway_context(&sources, self.config.max_context_chars);
        let _advisory = self.provider.analyze(ProviderRequest {
            idea: project.idea.clone(),
            context: preparation,
        })?;
        let source_provenance = sources
            .iter()
            .map(|source| source.provenance.clone())
            .collect::<Vec<_>>();
        let mut blueprint_provenance = vec![project.provenance.clone()];
        blueprint_provenance.extend(source_provenance.iter().cloned());
        let content = combined_content(&project.idea, &sources);
        let flags = classify(&content);
        let available_questions = build_questions(&project, &flags, &blueprint_provenance, &[]);
        validate_answers(&available_questions, answers)?;
        let inputs = build_inputs(&project, &sources, answers);
        let questions = build_questions(&project, &flags, &blueprint_provenance, answers);
        let assumptions = build_assumptions(&questions, answers, &blueprint_provenance);
        let conflicts = detect_conflicts(&project, &sources, answers);
        let intent = build_intent(
            &project,
            &flags,
            &blueprint_provenance,
            &conflicts,
            answers,
            &questions,
        );
        let personas = build_personas(&project, &flags, &blueprint_provenance);
        let journeys = build_journeys(&personas, &project, &blueprint_provenance);
        let nfrs = build_nfrs(&flags, &blueprint_provenance);
        let decisions = build_decisions(
            &flags,
            &questions,
            &assumptions,
            &blueprint_provenance,
            answers,
        );
        let risks = build_risks(&flags, &conflicts, &blueprint_provenance);
        let candidate_domains = build_candidate_domains(&flags, &blueprint_provenance);
        let candidate_requirements = build_candidate_requirements(&flags, &blueprint_provenance);
        let candidate_evidence =
            build_candidate_evidence(&flags, &candidate_requirements, &blueprint_provenance);
        let capabilities = build_capabilities(&flags);
        let data_outline = build_data_outline(&flags);
        let integrations = build_integrations(&flags);
        let deployment = build_deployment(&flags);
        let status = if conflicts
            .iter()
            .any(|c| c.resolution_state == ConflictResolutionState::Unresolved)
        {
            InvestigationStatus::NeedsConflictResolution
        } else if questions
            .iter()
            .any(|q| q.disposition == QuestionDisposition::Asked)
        {
            InvestigationStatus::NeedsAnswers
        } else {
            InvestigationStatus::ReadyForReview
        };
        let blueprint_status = match status {
            InvestigationStatus::NeedsConflictResolution => {
                BlueprintStatus::NeedsConflictResolution
            }
            InvestigationStatus::NeedsAnswers => BlueprintStatus::NeedsAnswers,
            _ => BlueprintStatus::ReadyForReview,
        };
        let now = now_ms();
        let mut blueprint = Blueprint {
            id: stable_id("blueprint", &[&project.id, &content]),
            revision: 1,
            status: blueprint_status,
            product_definition: project.idea.clone(),
            problem_outcome: infer_problem(&project.idea),
            product_ontology: ontology(&flags),
            intent,
            personas,
            journeys,
            capabilities,
            non_functional_requirements: nfrs,
            data_outline,
            integration_expectations: integrations,
            deployment_expectations: deployment,
            architecture_decisions: decisions,
            assumptions,
            risks,
            conflicts,
            questions,
            candidate_standard_domains: candidate_domains,
            candidate_requirements,
            candidate_evidence_needs: candidate_evidence,
            exclusions: vec![
                "P4 does not seal standards, requirements, task graphs, or completion authority."
                    .into(),
            ],
            provenance: blueprint_provenance,
            fingerprint: String::new(),
            created_at: now,
            updated_at: now,
        };
        blueprint.fingerprint = blueprint_fingerprint(&blueprint)?;
        let input_hashes = inputs
            .iter()
            .map(|input| input.provenance.content_hash.clone())
            .chain(sources.iter().map(|source| source.content_hash.clone()))
            .collect::<Vec<_>>();
        let investigation = Investigation {
            id: stable_id("investigation", &[&project.id, &blueprint.fingerprint]),
            project_id: project.id.clone(),
            protocol_version: INVESTIGATOR_PROTOCOL_VERSION.into(),
            policy_version: INVESTIGATOR_POLICY_VERSION.into(),
            status,
            input_hashes,
            provider: self.provider.metadata(),
            created_at: now,
            updated_at: now,
            version: 1,
        };
        Ok(InvestigationResult {
            project,
            investigation,
            inputs,
            sources,
            answers: answers.to_vec(),
            blueprint,
        })
    }

    pub fn apply_answer(
        &self,
        result: &InvestigationResult,
        answer: UserAnswer,
    ) -> Result<BlueprintRevision, String> {
        let mut answers = result.answers.clone();
        if let Some(existing) = answers
            .iter_mut()
            .find(|existing| existing.question_id == answer.question_id)
        {
            *existing = answer.clone();
        } else {
            answers.push(answer.clone());
        }
        let project = result.project.clone();
        let next = self.investigate(project, result.sources.clone(), &answers)?;
        let invalidated = result
            .blueprint
            .architecture_decisions
            .iter()
            .filter(|decision| {
                next.blueprint
                    .architecture_decisions
                    .iter()
                    .all(|candidate| candidate.id != decision.id)
            })
            .map(|decision| decision.id.clone())
            .collect();
        Ok(BlueprintRevision {
            id: stable_id(
                "revision",
                &[
                    &result.blueprint.id,
                    &answer.id,
                    &next.blueprint.fingerprint,
                ],
            ),
            blueprint_id: result.blueprint.id.clone(),
            revision: result.blueprint.revision + 1,
            changed_answer_ids: vec![answer.id],
            invalidated_decision_ids: invalidated,
            blueprint: next.blueprint,
            created_at: now_ms(),
        })
    }

    pub fn approve(
        &self,
        revision: &BlueprintRevision,
        approved_by: &str,
        note: &str,
    ) -> Result<BlueprintApproval, String> {
        if approved_by.trim().is_empty()
            || revision.blueprint.status == BlueprintStatus::NeedsConflictResolution
            || revision.blueprint.status == BlueprintStatus::NeedsAnswers
        {
            return Err("blueprint is not ready for approval".into());
        }
        Ok(BlueprintApproval {
            id: stable_id("approval", &[&revision.id, approved_by]),
            blueprint_revision_id: revision.id.clone(),
            approved_by: approved_by.into(),
            approved_at: now_ms(),
            note: note.into(),
        })
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct Flags {
    marketing: bool,
    authenticated: bool,
    internal: bool,
    payment: bool,
    ai: bool,
    local: bool,
    mobile: bool,
    web3: bool,
    api: bool,
    public: bool,
}

fn classify(value: &str) -> Flags {
    let value = value.to_ascii_lowercase();
    let has = |words: &[&str]| words.iter().any(|word| value.contains(word));
    Flags {
        marketing: has(&["marketing", "landing page", "public website", "campaign"]),
        authenticated: has(&["login", "log in", "account", "authenticated", "team"]),
        internal: has(&["internal", "company utility", "employee", "back office"]),
        payment: has(&[
            "payment",
            "checkout",
            "billing",
            "financial",
            "settlement",
            "stripe",
        ]),
        ai: has(&[" ai ", "llm", "chatbot", "model", "prompt"]),
        local: has(&[
            "local-only",
            "local only",
            "offline",
            "desktop",
            "local-first",
        ]),
        mobile: has(&["mobile", "ios", "android"]),
        web3: has(&["blockchain", "web3", "wallet", "smart contract", "crypto"]),
        api: has(&["api", "backend service", "backend", "endpoint"]),
        public: has(&["public", "customers", "users", "website"]),
    }
}

fn build_questions(
    project: &ProjectDraft,
    flags: &Flags,
    provenance: &[Provenance],
    answers: &[UserAnswer],
) -> Vec<InvestigatorQuestion> {
    let mut questions = Vec::new();
    let add = |questions: &mut Vec<InvestigatorQuestion>,
               key: &str,
               question: &str,
               why: &str,
               default: &str,
               options: Vec<(&str, &str)>,
               can_defer: bool,
               affects: Vec<&str>,
               ambiguity: &str| {
        questions.push(InvestigatorQuestion {
            id: stable_id("question", &[&project.id, key]),
            question: question.into(),
            why_it_matters: why.into(),
            recommended_default: default.into(),
            options: options
                .into_iter()
                .map(|(label, consequence)| QuestionOption {
                    id: stable_id("option", &[key, label]),
                    label: label.into(),
                    consequence: consequence.into(),
                })
                .collect(),
            can_defer,
            affects: affects.into_iter().map(str::to_string).collect(),
            source_ambiguity: ambiguity.into(),
            disposition: QuestionDisposition::Asked,
            provenance: provenance.to_vec(),
        });
    };
    if !flags.authenticated && !flags.local && !flags.marketing {
        add(
            &mut questions,
            "audience",
            "Who is the first functional user or role?",
            "The primary actor changes permissions, journeys, and data ownership.",
            "Start with one clearly named functional role.",
            vec![
                ("Single role", "Keeps scope and permissions narrow."),
                (
                    "Several roles",
                    "Adds role boundaries and collaboration behavior.",
                ),
            ],
            true,
            vec!["personas", "permissions", "journeys"],
            "The idea names an outcome but not the actor who owns it.",
        );
    }
    if !flags.local {
        add(
            &mut questions,
            "deployment",
            "Where should the product run first?",
            "Deployment changes architecture, data residency, operations, and cost.",
            "Use a managed cloud deployment with a documented region and recovery plan.",
            vec![
                (
                    "Managed cloud",
                    "Requires hosted operations and data-residency decisions.",
                ),
                (
                    "Private/self-hosted",
                    "Adds installation, upgrade, and support responsibilities.",
                ),
                (
                    "Local only",
                    "Avoids hosted data transfer but limits collaboration.",
                ),
            ],
            true,
            vec!["deployment", "architecture", "privacy", "cost"],
            "No deployment expectation is explicit.",
        );
    }
    if flags.authenticated {
        add(&mut questions, "identity", "What identity and session behavior is required?", "Authentication affects account data, session security, recovery, and compliance exposure.", "Use an established identity provider with short-lived sessions and explicit recovery.", vec![("Hosted identity provider", "Reduces credential handling but adds provider dependency."), ("Own identity service", "Adds credential, recovery, and audit obligations."), ("No account", "Removes account flows but limits personalization and collaboration.")], false, vec!["security", "data", "architecture"], "The idea implies users or teams but does not define identity behavior.");
    }
    if flags.payment {
        add(&mut questions, "money", "Does the product move or reconcile real money?", "Settlement, refunds, disputes, and auditability change integrity and security requirements.", "Treat money movement as a reconciled server-side workflow with an auditable ledger.", vec![("Yes", "Requires reconciliation, idempotency, and stronger controls."), ("No, just pricing", "Keeps payment settlement out of the product scope."), ("Not sure", "Record a financial-exposure assumption until validated.")], false, vec!["data integrity", "security", "compliance"], "Financial language is present but settlement responsibility is unclear.");
    }
    if flags.marketing || flags.public {
        add(&mut questions, "discoverability", "Is public discoverability a product outcome?", "SEO and accessibility investment matters for public pages but is irrelevant to private utilities.", "Treat accessibility as required and SEO as relevant for public pages.", vec![("Public acquisition", "Adds crawlability, metadata, performance, and content requirements."), ("Private/internal", "Suppresses SEO-specific scope while retaining accessibility.")], true, vec!["seo", "accessibility", "performance"], "The product appears public; discoverability is not explicit.");
    }
    if flags.local {
        add(&mut questions, "sync", "Should local work synchronize across devices or remain local-only?", "Sync changes trust boundaries, conflict resolution, identity, and cloud cost.", "Keep data local-first and defer synchronization until a concrete multi-device need exists.", vec![("Local only", "Avoids cloud transfer and sync conflicts."), ("Optional sync", "Requires identity, encryption, and conflict policy."), ("Required sync", "Makes hosted data and availability core architecture.")], true, vec!["data", "deployment", "privacy"], "Local operation is explicit but synchronization scope is not.");
    }
    if flags.ai {
        add(&mut questions, "ai_authority", "What may AI recommend or decide?", "AI authority changes safety, review, privacy, cost, and verification design.", "Keep AI advisory with structured output validation and human approval for material decisions.", vec![("Advisory", "Requires review and provenance before action."), ("Automated low-risk actions", "Requires action boundaries and rollback."), ("Automated material decisions", "Requires stronger controls and explicit governance.")], false, vec!["security", "verification", "cost"], "AI is named but its authority boundary is unspecified.");
    }
    let suppressed = |questions: &mut Vec<InvestigatorQuestion>,
                      key: &str,
                      question: &str,
                      reason: &str,
                      affects: Vec<&str>| {
        questions.push(InvestigatorQuestion {
            id: stable_id("question", &[&project.id, key]),
            question: question.into(),
            why_it_matters: "This domain can materially change the blueprint when it is relevant."
                .into(),
            recommended_default: "Keep this domain out of scope until evidence makes it relevant."
                .into(),
            options: Vec::new(),
            can_defer: true,
            affects: affects.into_iter().map(str::to_string).collect(),
            source_ambiguity: reason.into(),
            disposition: QuestionDisposition::Suppressed,
            provenance: provenance.to_vec(),
        });
    };
    if !flags.public && !flags.marketing {
        suppressed(
            &mut questions,
            "discoverability-suppressed",
            "Is public discoverability a product outcome?",
            "The available evidence describes a private, internal, or non-public product.",
            vec!["seo"],
        );
    }
    if !flags.payment {
        suppressed(
            &mut questions,
            "money-suppressed",
            "Does the product move or reconcile real money?",
            "No payment or financial workflow evidence was found.",
            vec!["financial integrity"],
        );
    }
    if !flags.authenticated {
        suppressed(
            &mut questions,
            "identity-suppressed",
            "What identity and session behavior is required?",
            "No account, login, or team-access evidence was found.",
            vec!["security", "identity"],
        );
    }
    if flags.local {
        suppressed(&mut questions, "scale-suppressed", "Does this require cloud-scale architecture?", "Local-first evidence makes cloud-scale architecture irrelevant until synchronization or hosted scale is confirmed.", vec!["architecture", "cost"]);
    }
    for question in &mut questions {
        if let Some(answer) = answers
            .iter()
            .find(|answer| answer.question_id == question.id)
        {
            question.disposition = match answer.choice {
                AnswerChoice::NotSure | AnswerChoice::Deferred => {
                    QuestionDisposition::AutoDefaulted
                }
                AnswerChoice::Selected(_) => QuestionDisposition::Answered,
            };
        }
    }
    questions
}

fn validate_answers(
    questions: &[InvestigatorQuestion],
    answers: &[UserAnswer],
) -> Result<(), String> {
    for answer in answers {
        let question = questions
            .iter()
            .find(|question| question.id == answer.question_id)
            .ok_or_else(|| format!("unknown investigator question id: {}", answer.question_id))?;
        if let AnswerChoice::Selected(option_id) = &answer.choice {
            if !question
                .options
                .iter()
                .any(|option| option.id == *option_id)
            {
                return Err(format!(
                    "unknown option id '{}' for question '{}'",
                    option_id, answer.question_id
                ));
            }
        }
        if matches!(answer.choice, AnswerChoice::Selected(_)) && !answer.confirmed {
            return Err(format!(
                "selected answer '{}' must be confirmed",
                answer.question_id
            ));
        }
    }
    Ok(())
}

fn build_assumptions(
    questions: &[InvestigatorQuestion],
    answers: &[UserAnswer],
    provenance: &[Provenance],
) -> Vec<Assumption> {
    let mut assumptions = Vec::new();
    for question in questions {
        if let Some(answer) = answers
            .iter()
            .find(|answer| answer.question_id == question.id)
        {
            if matches!(
                answer.choice,
                AnswerChoice::NotSure | AnswerChoice::Deferred
            ) {
                assumptions.push(Assumption {
                    id: stable_id("assumption", &[&question.id, "default"]),
                    statement: format!("Default: {}", question.recommended_default),
                    source_reason: format!(
                        "The user selected {} for: {}",
                        if matches!(answer.choice, AnswerChoice::NotSure) {
                            "NOT SURE"
                        } else {
                            "DEFERRED"
                        },
                        question.question
                    ),
                    affected_decisions: question.affects.clone(),
                    confidence: "medium".into(),
                    consequence_if_wrong: format!(
                        "Revisit {} before implementation begins.",
                        question.affects.join(", ")
                    ),
                    validation_trigger:
                        "User confirms the question or new source evidence resolves it.".into(),
                    status: RegisterStatus::Open,
                    provenance: provenance.to_vec(),
                });
            }
        }
    }
    assumptions
}

fn detect_conflicts(
    project: &ProjectDraft,
    sources: &[SourceDocument],
    answers: &[UserAnswer],
) -> Vec<Conflict> {
    let mut conflicts = Vec::new();
    let mut no_account: Option<Provenance> = None;
    let mut login: Option<Provenance> = None;
    let mut local: Option<Provenance> = None;
    let mut cloud: Option<Provenance> = None;
    let mut inspect = |content: &str, provenance: Provenance| {
        let content = content.to_ascii_lowercase();
        if content.contains("no account") || content.contains("without login") {
            no_account = Some(provenance.clone());
        }
        if content.contains("login") || content.contains("sign in") {
            login = Some(provenance.clone());
        }
        if content.contains("local-only") || content.contains("local only") {
            local = Some(provenance.clone());
        }
        if content.contains("cloud sync") || content.contains("cloud synchronization") {
            cloud = Some(provenance);
        }
    };

    inspect(&project.idea, project.provenance.clone());
    for source in sources {
        inspect(
            source.extracted_text.as_deref().unwrap_or(""),
            source.provenance.clone(),
        );
    }
    for answer in answers {
        let choice = match &answer.choice {
            AnswerChoice::Selected(option) => option.as_str(),
            AnswerChoice::NotSure => "not sure",
            AnswerChoice::Deferred => "deferred",
        };
        inspect(choice, answer.provenance.clone());
    }

    if let (Some(a), Some(b)) = (no_account, login) {
        conflicts.push(Conflict { id: stable_id("conflict", &[&project.id, "identity"]), claim_a: "No account required".into(), claim_b: "Users sign in".into(), source_a: a, source_b: b, severity: ConflictSeverity::Material, affected_blueprint_areas: vec!["personas".into(), "identity".into(), "data ownership".into()], recommended_resolution: "Confirm whether authentication is optional, required, or limited to an administrative surface.".into(), resolution_state: ConflictResolutionState::Unresolved });
    }
    if let (Some(a), Some(b)) = (local, cloud) {
        conflicts.push(Conflict { id: stable_id("conflict", &[&project.id, "deployment"]), claim_a: "Local-only".into(), claim_b: "Cloud synchronization required".into(), source_a: a, source_b: b, severity: ConflictSeverity::Material, affected_blueprint_areas: vec!["deployment".into(), "data".into(), "privacy".into()], recommended_resolution: "Confirm whether synchronization is required and which source of truth owns conflicts.".into(), resolution_state: ConflictResolutionState::Unresolved });
    }
    conflicts
}

fn build_intent(
    project: &ProjectDraft,
    flags: &Flags,
    provenance: &[Provenance],
    conflicts: &[Conflict],
    answers: &[UserAnswer],
    questions: &[InvestigatorQuestion],
) -> IntentModel {
    let claim = |field: &str, value: String, classification: ClaimClassification| IntentClaim {
        id: stable_id("claim", &[field, &value]),
        field: field.into(),
        value,
        classification,
        provenance: provenance.to_vec(),
    };
    let mut result = IntentModel {
        product_purpose: Some(claim(
            "product_purpose",
            project.idea.clone(),
            ClaimClassification::ExplicitFact,
        )),
        user_problem: Some(claim(
            "user_problem",
            infer_problem(&project.idea),
            ClaimClassification::DerivedInference,
        )),
        product_type: Some(claim(
            "product_type",
            product_type(flags, conflicts),
            ClaimClassification::DerivedInference,
        )),
        deployment_expectation: if flags.local {
            Some(claim(
                "deployment",
                "local-first".into(),
                ClaimClassification::ExplicitFact,
            ))
        } else {
            None
        },
        operational_expectation: Some(claim(
            "operations",
            if flags.local {
                "user-managed local operation"
            } else {
                "hosted operations need confirmation"
            }
            .into(),
            if flags.local {
                ClaimClassification::DerivedInference
            } else {
                ClaimClassification::Unknown
            },
        )),
        data_involved: build_data_claims(flags, &claim),
        external_integrations: if flags.payment {
            vec![claim(
                "integration",
                "payment provider".into(),
                ClaimClassification::DerivedInference,
            )]
        } else {
            Vec::new()
        },
        security_sensitivity: if flags.authenticated || flags.payment || flags.ai {
            Some(claim(
                "security",
                "elevated".into(),
                ClaimClassification::DerivedInference,
            ))
        } else {
            None
        },
        privacy_sensitivity: if flags.local || flags.authenticated {
            Some(claim(
                "privacy",
                "material".into(),
                ClaimClassification::DerivedInference,
            ))
        } else {
            None
        },
        financial_exposure: if flags.payment {
            Some(claim(
                "financial",
                "possible payment or settlement exposure".into(),
                ClaimClassification::DerivedInference,
            ))
        } else {
            None
        },
        compliance_hints: if flags.payment {
            vec![claim(
                "compliance",
                "financial record and audit obligations may apply".into(),
                ClaimClassification::DerivedInference,
            )]
        } else {
            Vec::new()
        },
        platform_targets: platform_claims(flags, &claim),
        explicit_constraints: if flags.local {
            vec![claim(
                "constraint",
                "local-first operation".into(),
                ClaimClassification::ExplicitFact,
            )]
        } else {
            Vec::new()
        },
        unknowns: vec![claim(
            "unknown",
            "deployment and operating model require confirmation".into(),
            ClaimClassification::Unknown,
        )],
        contradictions: conflicts
            .iter()
            .map(|conflict| {
                claim(
                    "conflict",
                    conflict.claim_a.clone(),
                    ClaimClassification::Conflict,
                )
            })
            .collect(),
    };
    for answer in answers {
        let AnswerChoice::Selected(option_id) = &answer.choice else {
            continue;
        };
        let Some(question) = questions
            .iter()
            .find(|question| question.id == answer.question_id)
        else {
            continue;
        };
        let Some(option) = question
            .options
            .iter()
            .find(|option| option.id == *option_id)
        else {
            continue;
        };
        let claim = IntentClaim {
            id: stable_id("claim", &["answer", &answer.id]),
            field: format!("answer:{}", question.id),
            value: option.label.clone(),
            classification: ClaimClassification::UserConfirmed,
            provenance: vec![answer.provenance.clone()],
        };
        result.explicit_constraints.push(claim.clone());
        if question.question.contains("Where should the product run") {
            result.deployment_expectation = Some(claim);
        }
    }
    result
}

fn build_personas(
    project: &ProjectDraft,
    flags: &Flags,
    provenance: &[Provenance],
) -> Vec<Persona> {
    let mut personas = vec![Persona {
        id: stable_id("persona", &[&project.id, "primary"]),
        name: "Primary operator".into(),
        functional_role: if flags.internal {
            "internal staff member".into()
        } else {
            "person who needs the stated outcome".into()
        },
        needs: vec![
            "complete the primary outcome with understandable feedback".into(),
            "know what is confirmed versus assumed".into(),
        ],
        provenance: provenance.to_vec(),
    }];
    if flags.authenticated || flags.payment {
        personas.push(Persona {
            id: stable_id("persona", &[&project.id, "administrator"]),
            name: "Administrator or account owner".into(),
            functional_role: "controls access, policy, or financial/account state".into(),
            needs: vec![
                "review activity and recover from failure".into(),
                "understand responsibility and audit history".into(),
            ],
            provenance: provenance.to_vec(),
        });
    }
    personas
}

fn build_journeys(
    personas: &[Persona],
    project: &ProjectDraft,
    provenance: &[Provenance],
) -> Vec<UserJourney> {
    personas
        .iter()
        .take(2)
        .map(|persona| UserJourney {
            id: stable_id("journey", &[&project.id, &persona.id]),
            persona_id: persona.id.clone(),
            starting_condition: "The user has a project need but incomplete engineering decisions."
                .into(),
            goal: project.idea.clone(),
            major_steps: vec![
                "Describe the desired outcome".into(),
                "Review important ambiguities and sources".into(),
                "Choose, defer, or accept a recommendation".into(),
                "Review the blueprint before approval".into(),
            ],
            success_state: "A reviewable blueprint exists with provenance and visible assumptions."
                .into(),
            failure_states: vec![
                "Material conflict remains unresolved".into(),
                "Required input cannot be extracted".into(),
                "Provider output is malformed or unavailable".into(),
            ],
            provenance: provenance.to_vec(),
        })
        .collect()
}

fn build_nfrs(flags: &Flags, provenance: &[Provenance]) -> Vec<NonFunctionalRequirement> {
    let mut result = Vec::new();
    let add = |result: &mut Vec<NonFunctionalRequirement>,
               domain: &str,
               statement: &str,
               class: NfrClassification,
               rationale: &str| {
        result.push(NonFunctionalRequirement {
            id: stable_id("nfr", &[domain, statement]),
            domain: domain.into(),
            statement: statement.into(),
            classification: class,
            rationale: rationale.into(),
            provenance: provenance.to_vec(),
        })
    };
    if ui_surface_relevant(flags) {
        add(
            &mut result,
            "accessibility",
            "Primary flows must be keyboard navigable and communicate status without color alone.",
            NfrClassification::Recommended,
            "The evidence indicates an ordinary-user interface.",
        );
    }
    if flags.public || flags.marketing {
        add(
            &mut result,
            "seo",
            "Public pages should expose crawlable metadata and meaningful performance budgets.",
            NfrClassification::InferredRequired,
            "Public acquisition is part of the inferred product outcome.",
        );
    }
    if flags.authenticated {
        add(
            &mut result,
            "security",
            "Sessions and identity recovery must use explicit, bounded security controls.",
            NfrClassification::InferredRequired,
            "Authentication is in scope.",
        );
    }
    if flags.payment {
        add(
            &mut result,
            "data_integrity",
            "Money-affecting operations require idempotency, reconciliation, and audit evidence.",
            NfrClassification::InferredRequired,
            "Financial exposure was detected.",
        );
    }
    if flags.local {
        add(
            &mut result,
            "portability",
            "The local-first path must remain usable without an internet connection.",
            NfrClassification::Explicit,
            "Local/offline operation was stated.",
        );
    }
    if flags.ai {
        add(
            &mut result,
            "safety",
            "AI recommendations must remain advisory until reviewed and structurally validated.",
            NfrClassification::InferredRequired,
            "AI authority is material and unspecified.",
        );
    }
    result
}

fn build_decisions(
    flags: &Flags,
    questions: &[InvestigatorQuestion],
    assumptions: &[Assumption],
    provenance: &[Provenance],
    answers: &[UserAnswer],
) -> Vec<ArchitectureDecision> {
    let mut result = Vec::new();
    let selected = if flags.local {
        Some("local-first storage with explicit future sync decision".into())
    } else {
        None
    };
    result.push(ArchitectureDecision { id: stable_id("adr", &["context", "storage"]), context: "The project needs a trustworthy source of project intent and review state.".into(), decision_question: "Where should investigation state live?".into(), considered_options: vec!["local structured database".into(), "browser-only state".into(), "cloud-only state".into()], selected_option: Some("local structured database".into()), reason: "Local authority keeps provenance and review state available without sending whole projects to a provider.".into(), tradeoffs: vec!["requires local migrations".into(), "multi-device collaboration is deferred".into()], assumptions: assumptions.iter().map(|a| a.id.clone()).collect(), source: provenance.to_vec(), status: DecisionStatus::DefaultAccepted, future_reconsideration_trigger: "A real multi-device collaboration requirement is confirmed.".into() });
    if flags.local {
        result.push(ArchitectureDecision { id: stable_id("adr", &["context", "local-first"]), context: "The idea names local or offline operation.".into(), decision_question: "Should synchronization be part of the first release?".into(), considered_options: vec!["local only".into(), "optional sync".into(), "required sync".into()], selected_option: selected, reason: "Local-first reduces privacy and availability dependencies until synchronization is justified.".into(), tradeoffs: vec!["cross-device continuity is deferred".into(), "local recovery must be explicit".into()], assumptions: questions.iter().filter(|q| q.id.contains("sync")).map(|q| q.id.clone()).collect(), source: provenance.to_vec(), status: DecisionStatus::Proposed, future_reconsideration_trigger: "User confirms a multi-device or team workflow.".into() });
    }
    for answer in answers {
        let AnswerChoice::Selected(option_id) = &answer.choice else {
            continue;
        };
        let Some(question) = questions
            .iter()
            .find(|question| question.id == answer.question_id)
        else {
            continue;
        };
        let Some(option) = question
            .options
            .iter()
            .find(|option| option.id == *option_id)
        else {
            continue;
        };
        result.push(ArchitectureDecision {
            id: stable_id("adr", &["answer", &question.id, option_id]),
            context: question.why_it_matters.clone(),
            decision_question: question.question.clone(),
            considered_options: question
                .options
                .iter()
                .map(|item| item.label.clone())
                .collect(),
            selected_option: Some(option.label.clone()),
            reason: option.consequence.clone(),
            tradeoffs: vec![format!(
                "Revisit if {} changes.",
                question.affects.join(", ")
            )],
            assumptions: Vec::new(),
            source: vec![answer.provenance.clone()],
            status: DecisionStatus::UserConfirmed,
            future_reconsideration_trigger: format!(
                "Reopen when evidence changes the {} decision.",
                question.affects.join(", ")
            ),
        });
    }
    result
}

fn build_risks(flags: &Flags, conflicts: &[Conflict], provenance: &[Provenance]) -> Vec<Risk> {
    let mut result = Vec::new();
    if !conflicts.is_empty() {
        result.push(Risk { id: stable_id("risk", &["conflict"]), title: "Conflicting source claims".into(), description: "Material documents disagree about product behavior.".into(), category: "product".into(), source: "conflict detector".into(), likelihood: "medium".into(), impact: "high".into(), severity: "high".into(), affected_area: "blueprint scope and architecture".into(), mitigation: "Resolve the conflict before representing the affected section as confirmed.".into(), owner: Some("project owner".into()), status: RegisterStatus::Open, provenance: provenance.to_vec() });
    }
    if flags.payment {
        result.push(Risk { id: stable_id("risk", &["financial"]), title: "Financial integrity exposure".into(), description: "Payment or settlement behavior may create reconciliation and compliance obligations.".into(), category: "security".into(), source: "intent inference".into(), likelihood: "medium".into(), impact: "high".into(), severity: "high".into(), affected_area: "payments and ledger".into(), mitigation: "Confirm money movement scope and require server-side idempotent reconciliation.".into(), owner: Some("product owner".into()), status: RegisterStatus::Open, provenance: provenance.to_vec() });
    }
    if flags.ai {
        result.push(Risk { id: stable_id("risk", &["ai"]), title: "Advisory AI mistaken for authority".into(), description: "Users may treat an AI recommendation as a confirmed requirement.".into(), category: "technical".into(), source: "intent inference".into(), likelihood: "medium".into(), impact: "high".into(), severity: "high".into(), affected_area: "investigation decisions".into(), mitigation: "Label provider status, preserve provenance, validate structure, and require review.".into(), owner: None, status: RegisterStatus::Open, provenance: provenance.to_vec() });
    }
    result
}

fn build_candidate_domains(
    flags: &Flags,
    provenance: &[Provenance],
) -> Vec<CandidateStandardDomain> {
    let mut domains = Vec::new();
    if ui_surface_relevant(flags) {
        domains.push(CandidateStandardDomain {
            id: stable_id("standard-domain", &["accessibility"]),
            domain: "accessibility".into(),
            rationale: "Applicable to ordinary-user flows.".into(),
            provenance: provenance.to_vec(),
        });
    }
    if flags.authenticated {
        domains.push(CandidateStandardDomain {
            id: stable_id("standard-domain", &["security"]),
            domain: "security and privacy".into(),
            rationale: "Identity and session behavior are in scope.".into(),
            provenance: provenance.to_vec(),
        });
    }
    if flags.payment {
        domains.push(CandidateStandardDomain {
            id: stable_id("standard-domain", &["financial-integrity"]),
            domain: "financial integrity and audit".into(),
            rationale: "Payment or settlement language was detected.".into(),
            provenance: provenance.to_vec(),
        });
    }
    if flags.public || flags.marketing {
        domains.push(CandidateStandardDomain {
            id: stable_id("standard-domain", &["public-web"]),
            domain: "public web discoverability".into(),
            rationale: "Public acquisition is relevant.".into(),
            provenance: provenance.to_vec(),
        });
    }
    domains
}

fn ui_surface_relevant(flags: &Flags) -> bool {
    !flags.api || flags.public || flags.marketing || flags.mobile
}

fn build_candidate_requirements(
    flags: &Flags,
    provenance: &[Provenance],
) -> Vec<CandidateRequirement> {
    let mut result = vec![CandidateRequirement {
        id: stable_id("candidate-requirement", &["provenance"]),
        statement: "Important blueprint claims must retain source provenance and classification."
            .into(),
        domain: "traceability".into(),
        rationale: "Investigation output must remain reviewable.".into(),
        provenance: provenance.to_vec(),
    }];
    if flags.authenticated {
        result.push(CandidateRequirement {
            id: stable_id("candidate-requirement", &["session"]),
            statement: "Session behavior and recovery must be explicit before implementation."
                .into(),
            domain: "security".into(),
            rationale: "Authentication was detected.".into(),
            provenance: provenance.to_vec(),
        });
    }
    result
}

fn build_candidate_evidence(
    flags: &Flags,
    requirements: &[CandidateRequirement],
    provenance: &[Provenance],
) -> Vec<CandidateEvidenceNeed> {
    let mut result = vec![CandidateEvidenceNeed {
        id: stable_id("evidence-need", &["blueprint-review"]),
        evidence_type: "blueprint-review-record".into(),
        purpose: "Show the owner reviewed assumptions, questions, and conflicts.".into(),
        related_requirement: requirements.first().map(|r| r.id.clone()),
        provenance: provenance.to_vec(),
    }];
    if flags.payment {
        result.push(CandidateEvidenceNeed {
            id: stable_id("evidence-need", &["reconciliation"]),
            evidence_type: "reconciliation-test".into(),
            purpose: "Verify payment state cannot silently diverge from the ledger.".into(),
            related_requirement: None,
            provenance: provenance.to_vec(),
        });
    }
    result
}

fn build_capabilities(flags: &Flags) -> Vec<String> {
    let mut result = vec![
        "capture project intent".into(),
        "review assumptions and important questions".into(),
        "approve or revise a blueprint".into(),
    ];
    if flags.authenticated {
        result.push("manage identity-bound access".into());
    }
    if flags.payment {
        result.push("handle payment-related workflow".into());
    }
    if flags.ai {
        result.push("provide advisory AI assistance".into());
    }
    result
}

fn build_data_outline(flags: &Flags) -> Vec<String> {
    let mut result = vec![
        "project draft".into(),
        "source metadata and hashes".into(),
        "investigation questions and answers".into(),
        "blueprint revisions".into(),
    ];
    if flags.authenticated {
        result.push("account and session references".into());
    }
    if flags.payment {
        result.push("payment and reconciliation records".into());
    }
    result
}

fn build_integrations(flags: &Flags) -> Vec<String> {
    let mut result = Vec::new();
    if flags.ai {
        result.push("Relintor AI gateway through the minimal-context broker".into());
    }
    if flags.payment {
        result.push("payment provider, pending scope confirmation".into());
    }
    result
}
fn build_deployment(flags: &Flags) -> Vec<String> {
    if flags.local {
        vec![
            "local-first desktop operation".into(),
            "synchronization deferred until confirmed".into(),
        ]
    } else {
        vec!["deployment target unresolved; ask before architecture is fixed".into()]
    }
}
fn ontology(flags: &Flags) -> Vec<String> {
    let mut result = vec![
        "project".into(),
        "investigation".into(),
        "source".into(),
        "question".into(),
        "answer".into(),
        "blueprint".into(),
    ];
    if flags.payment {
        result.push("financial event".into());
    }
    result
}
fn product_type(flags: &Flags, conflicts: &[Conflict]) -> String {
    if !conflicts.is_empty() {
        "application requiring clarification".into()
    } else if flags.payment {
        "payment or financial workflow".into()
    } else if flags.ai {
        "AI-enabled application".into()
    } else if flags.mobile {
        "mobile application".into()
    } else if flags.web3 {
        "blockchain/Web3 application".into()
    } else if flags.api {
        "API/backend service".into()
    } else if flags.internal {
        "internal company utility".into()
    } else if flags.local {
        "desktop/local-first application".into()
    } else if flags.marketing {
        "public marketing website".into()
    } else {
        "application requiring clarification".into()
    }
}
fn build_data_claims(
    flags: &Flags,
    claim: &impl Fn(&str, String, ClaimClassification) -> IntentClaim,
) -> Vec<IntentClaim> {
    let mut result = vec![claim(
        "data",
        "project intent and review records".into(),
        ClaimClassification::DerivedInference,
    )];
    if flags.authenticated {
        result.push(claim(
            "data",
            "identity and session data".into(),
            ClaimClassification::DerivedInference,
        ));
    }
    result
}
fn platform_claims(
    flags: &Flags,
    claim: &impl Fn(&str, String, ClaimClassification) -> IntentClaim,
) -> Vec<IntentClaim> {
    if flags.mobile {
        vec![claim(
            "platform",
            "mobile".into(),
            ClaimClassification::ExplicitFact,
        )]
    } else if flags.local {
        vec![claim(
            "platform",
            "desktop".into(),
            ClaimClassification::ExplicitFact,
        )]
    } else {
        vec![claim(
            "platform",
            "platform target unresolved".into(),
            ClaimClassification::Unknown,
        )]
    }
}
fn infer_problem(idea: &str) -> String {
    format!("The owner needs a reliable way to turn this outcome into an agreed, reviewable product plan: {}", idea.trim())
}
fn project_name(idea: &str) -> String {
    idea.split_whitespace()
        .take(5)
        .collect::<Vec<_>>()
        .join(" ")
}
fn combined_content(idea: &str, sources: &[SourceDocument]) -> String {
    let mut result = idea.to_string();
    let mut ordered = sources.to_vec();
    ordered.sort_by(|a, b| a.id.cmp(&b.id));
    for source in ordered {
        if let Some(text) = source.extracted_text {
            result.push('\n');
            result.push_str(&text);
        }
    }
    result
}
fn build_inputs(
    project: &ProjectDraft,
    sources: &[SourceDocument],
    answers: &[UserAnswer],
) -> Vec<InvestigationInput> {
    let mut result = vec![InvestigationInput {
        id: stable_id("input", &[&project.id, "idea"]),
        kind: InputKind::Idea,
        value: project.idea.clone(),
        provenance: project.provenance.clone(),
        created_at: project.created_at,
    }];
    result.extend(sources.iter().map(|source| InvestigationInput {
        id: stable_id("input", &[&source.id]),
        kind: match source.source_kind {
            SourceKind::ImageSketch => InputKind::Sketch,
            SourceKind::ImageScreenshot => InputKind::Screenshot,
            _ => InputKind::Document,
        },
        value: source.display_name.clone(),
        provenance: source.provenance.clone(),
        created_at: source.created_at,
    }));
    result.extend(answers.iter().map(|answer| InvestigationInput {
        id: stable_id("input", &["answer", &answer.id]),
        kind: InputKind::UserAnswer,
        value: answer_payload(answer),
        provenance: answer.provenance.clone(),
        created_at: answer.created_at,
    }));
    result
}

fn answer_payload(answer: &UserAnswer) -> String {
    let choice = match &answer.choice {
        AnswerChoice::Selected(option_id) => format!("selected:{option_id}"),
        AnswerChoice::NotSure => "not-sure".into(),
        AnswerChoice::Deferred => "deferred".into(),
    };
    format!(
        "question={};choice={choice};explanation={}",
        answer.question_id, answer.explanation
    )
}
pub fn normalized_blueprint(blueprint: &Blueprint) -> Result<String, String> {
    let mut normalized = blueprint.clone();
    normalized.fingerprint.clear();
    normalized.created_at = 0;
    normalized.updated_at = 0;
    for source in &mut normalized.provenance {
        source.locator = source.locator.replace('\\', "/");
    }
    serde_json::to_string(&normalized).map_err(|e| e.to_string())
}

fn blueprint_fingerprint(blueprint: &Blueprint) -> Result<String, String> {
    Ok(sha256_hex(normalized_blueprint(blueprint)?.as_bytes()))
}
fn stable_id(prefix: &str, values: &[&str]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(prefix.as_bytes());
    for value in values {
        hasher.update([0]);
        hasher.update(value.as_bytes());
    }
    format!("{prefix}_{}", &hex_lower(&hasher.finalize())[..24])
}
fn to_json(value: &impl Serialize) -> Result<String, String> {
    serde_json::to_string(value).map_err(|error| error.to_string())
}
fn sha256_hex(bytes: &[u8]) -> String {
    hex_lower(&Sha256::digest(bytes))
}
fn hex_lower(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
fn now_ms() -> i128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i128)
        .unwrap_or_default()
}
fn contains_secret(value: &str) -> bool {
    let value = value.to_ascii_lowercase();
    [
        "sk-",
        "akia",
        "-----begin ",
        "postgres://",
        "postgresql://",
        "password=",
        "api_key=",
        "apikey=",
        "authorization: bearer",
        "bearer ",
        "private_key",
        "client_secret",
    ]
    .iter()
    .any(|marker| value.contains(marker))
}
fn is_text_extension(extension: &str) -> bool {
    [
        "txt", "md", "markdown", "json", "csv", "yaml", "yml", "toml", "rs", "ts", "tsx", "js",
        "jsx", "html", "css", "sql", "xml", "svg",
    ]
    .contains(&extension)
}
fn is_image_extension(extension: &str) -> bool {
    ["png", "jpg", "jpeg", "gif", "webp", "bmp", "svg"].contains(&extension)
}
fn mime_for(extension: &str) -> String {
    match extension {
        "md" | "markdown" => "text/markdown",
        "json" => "application/json",
        "yaml" | "yml" => "application/yaml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "svg" => "image/svg+xml",
        _ if is_image_extension(extension) => "image/*",
        _ => "text/plain",
    }
    .into()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CorpusCase {
    pub name: String,
    pub idea: String,
    pub documents: Vec<String>,
    pub expected_type: String,
}

pub fn diverse_corpus() -> Vec<CorpusCase> {
    vec![
        CorpusCase {
            name: "public-marketing".into(),
            idea: "A public marketing website for a new studio".into(),
            documents: vec!["Launch page should be discoverable".into()],
            expected_type: "public marketing website".into(),
        },
        CorpusCase {
            name: "authenticated-saas".into(),
            idea: "An authenticated SaaS application where teams log in".into(),
            documents: vec![],
            expected_type: "application requiring clarification".into(),
        },
        CorpusCase {
            name: "internal-utility".into(),
            idea: "An internal company utility for employees".into(),
            documents: vec![],
            expected_type: "internal company utility".into(),
        },
        CorpusCase {
            name: "payment-workflow".into(),
            idea: "A payment and financial settlement workflow".into(),
            documents: vec![],
            expected_type: "payment or financial workflow".into(),
        },
        CorpusCase {
            name: "ai-application".into(),
            idea: "An AI assistant using an LLM".into(),
            documents: vec![],
            expected_type: "AI-enabled application".into(),
        },
        CorpusCase {
            name: "desktop-local".into(),
            idea: "A local-first offline desktop utility".into(),
            documents: vec![],
            expected_type: "desktop/local-first application".into(),
        },
        CorpusCase {
            name: "mobile".into(),
            idea: "A mobile app for iOS and Android".into(),
            documents: vec![],
            expected_type: "mobile application".into(),
        },
        CorpusCase {
            name: "web3".into(),
            idea: "A blockchain wallet and Web3 application".into(),
            documents: vec![],
            expected_type: "blockchain/Web3 application".into(),
        },
        CorpusCase {
            name: "api".into(),
            idea: "An API backend service with endpoints".into(),
            documents: vec![],
            expected_type: "API/backend service".into(),
        },
        CorpusCase {
            name: "contradictory".into(),
            idea: "A local-only project".into(),
            documents: vec![
                "No account required".into(),
                "Users login with email and cloud synchronization".into(),
            ],
            expected_type: "application requiring clarification".into(),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn source(service: &IntakeService, name: &str, text: &str) -> SourceDocument {
        service.intake_text(name, text).unwrap()
    }
    fn result(idea: &str, docs: &[(&str, &str)]) -> InvestigationResult {
        let service = IntakeService::new(InvestigatorConfig::default());
        let project = ProjectDraft::from_idea(idea).unwrap();
        let sources = docs
            .iter()
            .map(|(name, text)| source(&service, name, text))
            .collect();
        Investigator::default()
            .investigate(project, sources, &[])
            .unwrap()
    }

    #[test]
    fn important_ambiguity_is_asked_and_irrelevant_questions_are_suppressed() {
        let web = result("A public marketing website", &[]);
        assert!(web
            .blueprint
            .questions
            .iter()
            .any(|q| q.question.contains("discoverability")));
        let local = result("A private internal desktop utility, local-only", &[]);
        assert!(!local
            .blueprint
            .questions
            .iter()
            .filter(|q| q.disposition != QuestionDisposition::Suppressed)
            .any(|q| q.question.contains("discoverability")));
        assert!(local
            .blueprint
            .questions
            .iter()
            .any(|q| q.disposition == QuestionDisposition::Suppressed));
        assert!(!local
            .blueprint
            .questions
            .iter()
            .filter(|q| q.disposition != QuestionDisposition::Suppressed)
            .any(|q| q.question.contains("cloud-scale")));
    }

    #[test]
    fn not_sure_yields_recommendation_and_visible_assumption() {
        let initial = result("An authenticated application for a small team", &[]);
        let question = initial
            .blueprint
            .questions
            .iter()
            .find(|q| q.question.contains("identity"))
            .unwrap();
        let answer = UserAnswer {
            id: stable_id("answer", &[&question.id, "not-sure"]),
            question_id: question.id.clone(),
            choice: AnswerChoice::NotSure,
            explanation: "I am not sure".into(),
            confirmed: false,
            provenance: Provenance {
                source_id: "answer".into(),
                locator: "user://answer".into(),
                content_hash: sha256_hex(b"not-sure"),
                note: "User explicitly deferred expertise.".into(),
            },
            created_at: now_ms(),
        };
        let next = Investigator::default()
            .investigate(initial.project, initial.sources, &[answer])
            .unwrap();
        assert!(next
            .blueprint
            .assumptions
            .iter()
            .any(|a| a.statement.contains("Default:")));
    }

    #[test]
    fn contradictory_documents_are_flagged_and_block_confirmation() {
        let output = result(
            "A project",
            &[
                ("a.md", "No account required"),
                ("b.md", "Users login with email"),
            ],
        );
        assert_eq!(output.blueprint.conflicts.len(), 1);
        assert_eq!(
            output.blueprint.status,
            BlueprintStatus::NeedsConflictResolution
        );
        assert_eq!(
            output.investigation.status,
            InvestigationStatus::NeedsConflictResolution
        );
    }

    #[test]
    fn identical_normalized_inputs_reproduce_fingerprint() {
        let left = result("An offline desktop utility", &[("brief.md", "local-first")]);
        let right = result("An offline desktop utility", &[("brief.md", "local-first")]);
        assert_eq!(left.blueprint.fingerprint, right.blueprint.fingerprint);
        assert_eq!(
            normalized_blueprint(&left.blueprint).unwrap(),
            normalized_blueprint(&right.blueprint).unwrap()
        );
    }

    #[test]
    fn intake_handles_duplicates_types_size_paths_and_truthful_unsupported_state() {
        let service = IntakeService::new(InvestigatorConfig {
            max_source_bytes: 32,
            ..Default::default()
        });
        let first = source(&service, "brief.md", "small brief");
        assert!(matches!(
            service.reject_duplicate(std::slice::from_ref(&first), &first),
            Err(IntakeError::DuplicateDocument(_))
        ));
        let mut file = tempfile_path("source.pdf");
        let mut handle = fs::File::create(&file).unwrap();
        handle.write_all(b"not analyzed").unwrap();
        let unsupported = service.intake_file(&file).unwrap();
        assert_eq!(unsupported.extraction_status, ExtractionStatus::Unsupported);
        file.set_extension("md");
        let mut handle = fs::File::create(&file).unwrap();
        handle
            .write_all(b"this is longer than the configured limit and fails")
            .unwrap();
        assert!(matches!(
            service.intake_file(&file),
            Err(IntakeError::FileTooLarge { .. })
        ));
        let _ = fs::remove_file(file);
    }

    #[test]
    fn secret_context_is_redacted_before_provider_boundary() {
        let service = IntakeService::new(Default::default());
        let safe = source(&service, "safe.md", "safe context");
        let secret = source(&service, "secret.env", "OPENAI_API_KEY=sk-secret");
        let prepared = prepare_minimal_gateway_context(&[safe, secret], 1000);
        assert_eq!(prepared.items.len(), 1);
        assert_eq!(prepared.redacted_count, 1);
        assert!(!prepared.items[0].text.contains("sk-"));
    }

    #[test]
    fn malformed_provider_output_is_not_authority() {
        struct BadProvider;
        impl InvestigatorProvider for BadProvider {
            fn metadata(&self) -> ProviderMetadata {
                ProviderMetadata {
                    adapter: "bad".into(),
                    model: "bad".into(),
                    live: true,
                    availability: "available".into(),
                }
            }
            fn analyze(&self, _request: ProviderRequest) -> Result<ProviderResponse, String> {
                Err("malformed structured response".into())
            }
        }
        let project = ProjectDraft::from_idea("idea").unwrap();
        let error = Investigator {
            provider: BadProvider,
            config: Default::default(),
        }
        .investigate(project, vec![], &[])
        .unwrap_err();
        assert!(error.contains("malformed"));
    }

    #[test]
    fn personas_journeys_nfrs_adrs_risks_and_candidates_are_structured() {
        let output = result("An AI payment application where users login", &[]);
        assert!(!output.blueprint.personas.is_empty());
        assert!(!output.blueprint.journeys.is_empty());
        assert!(!output.blueprint.non_functional_requirements.is_empty());
        assert!(!output.blueprint.architecture_decisions.is_empty());
        assert!(!output.blueprint.risks.is_empty());
        assert!(!output.blueprint.candidate_standard_domains.is_empty());
        assert!(!output.blueprint.candidate_evidence_needs.is_empty());
    }

    #[test]
    fn approval_is_not_verified_completion_and_conflicts_require_resolution() {
        let mut ready = result("A private internal utility", &[]);
        let mut answers = Vec::new();
        for question in ready
            .blueprint
            .questions
            .clone()
            .into_iter()
            .filter(|question| !question.options.is_empty())
        {
            answers.push(UserAnswer {
                id: stable_id("answer", &[&question.id, "selected"]),
                question_id: question.id,
                choice: AnswerChoice::Selected(
                    question
                        .options
                        .first()
                        .map(|option| option.id.clone())
                        .unwrap_or_default(),
                ),
                explanation: "Selected during review.".into(),
                confirmed: true,
                provenance: Provenance {
                    source_id: "answer".into(),
                    locator: "user://answer".into(),
                    content_hash: sha256_hex(b"selected"),
                    note: "User-confirmed answer.".into(),
                },
                created_at: now_ms(),
            });
        }
        ready = Investigator::default()
            .investigate(ready.project.clone(), ready.sources.clone(), &answers)
            .unwrap();
        let revision = BlueprintRevision {
            id: "revision".into(),
            blueprint_id: ready.blueprint.id.clone(),
            revision: 1,
            changed_answer_ids: vec![],
            invalidated_decision_ids: vec![],
            blueprint: ready.blueprint,
            created_at: now_ms(),
        };
        assert!(Investigator::default()
            .approve(&revision, "owner", "reviewed")
            .is_ok());
        let blocked = result(
            "project",
            &[("a", "No account required"), ("b", "Users login")],
        );
        let blocked_revision = BlueprintRevision {
            id: "revision".into(),
            blueprint_id: blocked.blueprint.id.clone(),
            revision: 1,
            changed_answer_ids: vec![],
            invalidated_decision_ids: vec![],
            blueprint: blocked.blueprint,
            created_at: now_ms(),
        };
        assert!(Investigator::default()
            .approve(&blocked_revision, "owner", "reviewed")
            .is_err());
    }

    #[test]
    fn corpus_covers_ten_meaningfully_different_projects() {
        assert_eq!(diverse_corpus().len(), 10);
        let fixtures: Vec<serde_json::Value> =
            serde_json::from_str(include_str!("../tests/corpus.json")).unwrap();
        assert_eq!(fixtures.len(), 10);
    }

    fn tempfile_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("relintor-p4-{}-{name}", std::process::id()))
    }
}
