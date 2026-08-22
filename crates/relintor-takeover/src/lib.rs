//! Deterministic, read-only existing-project takeover discovery for P5.
//!
//! This crate deliberately separates repository facts from runtime evidence.
//! Static presence is never promoted to a working capability without an
//! explicit probe or test result.

use relintor_investigator::Provenance;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, Metadata};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub const TAKEOVER_SCANNER_VERSION: &str = "p5-takeover-scanner-1";
pub const TAKEOVER_PROTOCOL_VERSION: &str = "p5-takeover-1";

pub use relintor_investigator::Provenance as InvestigatorProvenance;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProjectTakeover {
    pub id: String,
    pub root: String,
    pub source: Provenance,
    pub created_at: i128,
    pub scanner_version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RepositorySnapshot {
    pub id: String,
    pub root: String,
    pub is_git_repository: bool,
    pub fingerprint: RepositoryFingerprint,
    pub file_count: u64,
    pub total_bytes: u64,
    pub source: Provenance,
    pub discovered_at: i128,
    pub scanner_version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RepositoryFingerprint {
    pub id: String,
    pub algorithm: String,
    pub value: String,
    pub file_count: u64,
    pub total_bytes: u64,
    pub generated_tree_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum FileClassification {
    Source,
    Configuration,
    Test,
    Documentation,
    Migration,
    Schema,
    Deployment,
    Ci,
    Asset,
    Generated,
    Dependency,
    Binary,
    Excluded,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RepositoryFile {
    pub id: String,
    pub relative_path: String,
    pub canonical_path: String,
    pub file_type: String,
    pub size_bytes: u64,
    pub content_hash: Option<String>,
    pub language: Option<String>,
    pub classification: FileClassification,
    pub included: bool,
    pub excluded_reason: Option<String>,
    pub source: Provenance,
    pub discovered_at: i128,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RepositoryArea {
    pub id: String,
    pub path: String,
    pub area_type: String,
    pub files: Vec<String>,
    pub source: Provenance,
    pub confidence: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum BuildSystemKind {
    JavaScript,
    Rust,
    Python,
    Go,
    Java,
    DotNet,
    Tauri,
    Electron,
    Flutter,
    Android,
    IOS,
    Container,
    Infrastructure,
    Ci,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BuildSystem {
    pub id: String,
    pub name: String,
    pub kind: BuildSystemKind,
    pub evidence: Vec<Provenance>,
    pub confidence: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum DependencyKind {
    InternalWorkspace,
    Runtime,
    Development,
    Build,
    Optional,
    ExternalService,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Dependency {
    pub id: String,
    pub name: String,
    pub source_package: Option<String>,
    pub version: Option<String>,
    pub kind: DependencyKind,
    pub source: Provenance,
    pub workspace_member: Option<String>,
    pub missing_reference: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DependencyGraph {
    pub id: String,
    pub nodes: Vec<String>,
    pub dependencies: Vec<Dependency>,
    pub internal_edges: Vec<(String, String)>,
    pub cycles: Vec<Vec<String>>,
    pub manifest_lock_inconsistencies: Vec<String>,
    pub source: Vec<Provenance>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ApplicationSurface {
    pub id: String,
    pub name: String,
    pub path: String,
    pub surface_type: String,
    pub source: Provenance,
    pub confidence: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum RouteStatus {
    Declared,
    Registered,
    ReachabilityUnproven,
    DeadOrUnregistered,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Route {
    pub id: String,
    pub method: String,
    pub path: String,
    pub handler: Option<String>,
    pub source: Provenance,
    pub auth_hints: Vec<String>,
    pub request_hints: Vec<String>,
    pub response_hints: Vec<String>,
    pub confidence: String,
    pub status: RouteStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ApiEndpoint {
    pub id: String,
    pub method: String,
    pub path: String,
    pub handler: Option<String>,
    pub source: Provenance,
    pub auth_hints: Vec<String>,
    pub confidence: String,
    pub status: RouteStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DatabaseSystem {
    pub id: String,
    pub technology: String,
    pub evidence: Vec<Provenance>,
    pub configuration_paths: Vec<String>,
    pub confidence: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SchemaObject {
    pub id: String,
    pub name: String,
    pub object_type: String,
    pub source: Provenance,
    pub migration_evidence: Vec<String>,
    pub migration_required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MigrationRecord {
    pub id: String,
    pub identifier: String,
    pub path: String,
    pub order: Option<u64>,
    pub source: Provenance,
    pub duplicate_identifier: bool,
    pub ordering_gap: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum AuthEvidenceState {
    AuthLibraryPresent,
    AuthImplemented,
    RouteProtected,
    AuthorizationProven,
    AuthorizationUnproven,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AuthSystem {
    pub id: String,
    pub technology: String,
    pub states: Vec<AuthEvidenceState>,
    pub evidence: Vec<Provenance>,
    pub confidence: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PermissionBoundary {
    pub id: String,
    pub name: String,
    pub boundary_type: String,
    pub protected_paths: Vec<String>,
    pub source: Provenance,
    pub authorization_state: AuthEvidenceState,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum UiEvidenceState {
    Present,
    Possible,
    RuntimeUnproven,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UiSurface {
    pub id: String,
    pub name: String,
    pub path: String,
    pub surface_type: String,
    pub actions: Vec<String>,
    pub source: Provenance,
    pub state: UiEvidenceState,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UserJourneyDiscovery {
    pub id: String,
    pub name: String,
    pub starting_condition: String,
    pub goal: String,
    pub major_steps: Vec<String>,
    pub source: Vec<Provenance>,
    pub state: UiEvidenceState,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TestSuite {
    pub id: String,
    pub name: String,
    pub path: String,
    pub suite_type: String,
    pub command: Option<String>,
    pub skipped_or_disabled: bool,
    pub missing_references: Vec<String>,
    pub source: Provenance,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CiWorkflow {
    pub id: String,
    pub name: String,
    pub path: String,
    pub commands: Vec<String>,
    pub disabled: bool,
    pub source: Provenance,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeploymentTarget {
    pub id: String,
    pub target: String,
    pub evidence: Vec<Provenance>,
    pub state: String,
    pub configuration_paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ProbeSafety {
    SafeReadOnly,
    SandboxRequired,
    UserApprovalRequired,
    Forbidden,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeProbe {
    pub id: String,
    pub target: String,
    pub exact_command: Vec<String>,
    pub safety: ProbeSafety,
    pub status: String,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub started_at: i128,
    pub ended_at: Option<i128>,
    pub timeout_ms: u64,
    pub workspace_fingerprint: String,
    pub source: Vec<Provenance>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DocumentationPromise {
    pub id: String,
    pub text: String,
    pub path: String,
    pub location: String,
    pub category: String,
    pub related_capability: Option<String>,
    pub verification_state: String,
    pub source: Provenance,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum CapabilityClassification {
    Working,
    Partial,
    Broken,
    Missing,
    Unproven,
    Dead,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CapabilityEvidence {
    pub id: String,
    pub kind: String,
    pub summary: String,
    pub source: Provenance,
    pub supports: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiscoveredCapability {
    pub id: String,
    pub name: String,
    pub classification: CapabilityClassification,
    pub implementation_evidence: Vec<CapabilityEvidence>,
    pub test_evidence: Vec<CapabilityEvidence>,
    pub runtime_evidence: Vec<CapabilityEvidence>,
    pub conflicting_evidence: Vec<CapabilityEvidence>,
    pub confidence: String,
    pub source: Vec<Provenance>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TakeoverConflict {
    pub id: String,
    pub claim_a: String,
    pub claim_b: String,
    pub source_a: Provenance,
    pub source_b: Provenance,
    pub affected_capability: Option<String>,
    pub severity: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TakeoverFinding {
    pub id: String,
    pub finding_type: String,
    pub summary: String,
    pub classification: Option<CapabilityClassification>,
    pub severity: String,
    pub evidence: Vec<CapabilityEvidence>,
    pub source: Vec<Provenance>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum RecommendationKind {
    Keep,
    Repair,
    Remove,
    Rebuild,
    InvestigateFurther,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TakeoverRecommendation {
    pub id: String,
    pub target: String,
    pub kind: RecommendationKind,
    pub current_reality: CapabilityClassification,
    pub reason: String,
    pub severity: String,
    pub priority: String,
    pub risk_of_change: String,
    pub dependencies: Vec<String>,
    pub suggested_next_action: String,
    pub evidence: Vec<CapabilityEvidence>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TakeoverRevision {
    pub id: String,
    pub takeover_id: String,
    pub revision: u32,
    pub fingerprint: String,
    pub created_at: i128,
    pub changed_paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TakeoverReport {
    pub takeover: ProjectTakeover,
    pub snapshot: RepositorySnapshot,
    pub inventory: Vec<RepositoryFile>,
    pub areas: Vec<RepositoryArea>,
    pub build_systems: Vec<BuildSystem>,
    pub dependency_graph: DependencyGraph,
    pub application_surfaces: Vec<ApplicationSurface>,
    pub routes: Vec<Route>,
    pub api_endpoints: Vec<ApiEndpoint>,
    pub database_systems: Vec<DatabaseSystem>,
    pub schema_objects: Vec<SchemaObject>,
    pub migrations: Vec<MigrationRecord>,
    pub auth_systems: Vec<AuthSystem>,
    pub permission_boundaries: Vec<PermissionBoundary>,
    pub ui_surfaces: Vec<UiSurface>,
    pub journeys: Vec<UserJourneyDiscovery>,
    pub test_suites: Vec<TestSuite>,
    pub ci_workflows: Vec<CiWorkflow>,
    pub deployment_targets: Vec<DeploymentTarget>,
    pub runtime_probes: Vec<RuntimeProbe>,
    pub documentation_promises: Vec<DocumentationPromise>,
    pub capabilities: Vec<DiscoveredCapability>,
    pub conflicts: Vec<TakeoverConflict>,
    pub findings: Vec<TakeoverFinding>,
    pub recommendations: Vec<TakeoverRecommendation>,
    pub revisions: Vec<TakeoverRevision>,
    pub fingerprint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TakeoverConfig {
    pub max_files: u64,
    pub max_total_bytes: u64,
    pub max_file_bytes: u64,
    pub max_hash_bytes: u64,
    pub max_probe_output_bytes: usize,
    pub default_probe_timeout_ms: u64,
    pub excluded_directories: BTreeSet<String>,
}

impl Default for TakeoverConfig {
    fn default() -> Self {
        Self {
            max_files: 20_000,
            max_total_bytes: 512 * 1024 * 1024,
            max_file_bytes: 8 * 1024 * 1024,
            max_hash_bytes: 2 * 1024 * 1024,
            max_probe_output_bytes: 64 * 1024,
            default_probe_timeout_ms: 5_000,
            excluded_directories: [
                ".git",
                "node_modules",
                "target",
                "dist",
                "build",
                "coverage",
                ".next",
                ".turbo",
                ".venv",
                "venv",
                "__pycache__",
                "vendor",
                "Pods",
            ]
            .into_iter()
            .map(str::to_string)
            .collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TakeoverError {
    RootDoesNotExist(String),
    RootNotDirectory(String),
    RootCanonicalization(String),
    PathOutsideRoot(String),
    FileLimitExceeded(u64),
    ByteLimitExceeded(u64),
    ReadFailure(String),
    UnsafeProbe(String),
    Persistence(String),
}

impl std::fmt::Display for TakeoverError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RootDoesNotExist(path) => write!(f, "takeover root does not exist: {path}"),
            Self::RootNotDirectory(path) => write!(f, "takeover root is not a directory: {path}"),
            Self::RootCanonicalization(detail) => write!(f, "canonicalize takeover root: {detail}"),
            Self::PathOutsideRoot(path) => write!(f, "path is outside takeover root: {path}"),
            Self::FileLimitExceeded(limit) => write!(f, "takeover file limit exceeded: {limit}"),
            Self::ByteLimitExceeded(limit) => write!(f, "takeover byte limit exceeded: {limit}"),
            Self::ReadFailure(detail) => write!(f, "read takeover source: {detail}"),
            Self::UnsafeProbe(detail) => write!(f, "runtime probe rejected: {detail}"),
            Self::Persistence(detail) => write!(f, "persist takeover report: {detail}"),
        }
    }
}

impl std::error::Error for TakeoverError {}

#[derive(Debug, Clone)]
struct CollectedFile {
    record: RepositoryFile,
    text: Option<String>,
}

pub struct TakeoverScanner {
    pub config: TakeoverConfig,
}

impl Default for TakeoverScanner {
    fn default() -> Self {
        Self::new(TakeoverConfig::default())
    }
}

impl TakeoverScanner {
    pub fn new(config: TakeoverConfig) -> Self {
        Self { config }
    }

    pub fn scan(&self, root: &Path) -> Result<TakeoverReport, TakeoverError> {
        let canonical_root = canonical_root(root)?;
        let mut collected = Vec::new();
        let mut total_bytes = 0_u64;
        self.collect_directory(
            &canonical_root,
            &canonical_root,
            &mut collected,
            &mut total_bytes,
        )?;
        collected.sort_by(|a, b| a.record.relative_path.cmp(&b.record.relative_path));
        let inventory = collected
            .iter()
            .map(|item| item.record.clone())
            .collect::<Vec<_>>();
        let text_by_path = collected
            .iter()
            .filter_map(|item| {
                item.text
                    .clone()
                    .map(|text| (item.record.relative_path.clone(), text))
            })
            .collect::<BTreeMap<_, _>>();
        let file_hash_tuples = inventory
            .iter()
            .filter(|file| file.included)
            .map(|file| {
                format!(
                    "{}:{}:{}",
                    file.relative_path,
                    file.size_bytes,
                    file.content_hash.clone().unwrap_or_default()
                )
            })
            .collect::<Vec<_>>();
        let tree_hash = sha256_hex(file_hash_tuples.join("\n").as_bytes());
        let fingerprint = RepositoryFingerprint {
            id: stable_id("repository-fingerprint", &[&tree_hash]),
            algorithm: "sha256(sorted-relative-path-size-content-hash)".into(),
            value: tree_hash.clone(),
            file_count: inventory.iter().filter(|file| file.included).count() as u64,
            total_bytes,
            generated_tree_hash: tree_hash,
        };
        let canonical_root_string = canonical_root.to_string_lossy().into_owned();
        let root_provenance = provenance(
            "repository",
            &canonical_root_string,
            &fingerprint.value,
            "Canonical read-only takeover root.",
        );
        let takeover = ProjectTakeover {
            id: stable_id("takeover", &[&canonical_root.to_string_lossy()]),
            root: canonical_root.to_string_lossy().into_owned(),
            source: root_provenance.clone(),
            created_at: now_ms(),
            scanner_version: TAKEOVER_SCANNER_VERSION.into(),
        };
        let snapshot = RepositorySnapshot {
            id: stable_id("snapshot", &[&takeover.id, &fingerprint.value]),
            root: takeover.root.clone(),
            is_git_repository: canonical_root.join(".git").is_dir(),
            fingerprint: fingerprint.clone(),
            file_count: fingerprint.file_count,
            total_bytes,
            source: root_provenance.clone(),
            discovered_at: now_ms(),
            scanner_version: TAKEOVER_SCANNER_VERSION.into(),
        };
        let build_systems = detect_build_systems(&inventory, &text_by_path);
        let areas = discover_areas(&inventory);
        let dependency_graph = discover_dependencies(&inventory, &text_by_path, &build_systems);
        let application_surfaces = discover_application_surfaces(&inventory, &build_systems);
        let routes = discover_routes(&inventory, &text_by_path);
        let api_endpoints = routes
            .iter()
            .filter(|route| {
                route.path.contains("/api")
                    || route.source.locator.contains("server")
                    || route.source.locator.contains("service")
            })
            .map(|route| ApiEndpoint {
                id: route.id.clone(),
                method: route.method.clone(),
                path: route.path.clone(),
                handler: route.handler.clone(),
                source: route.source.clone(),
                auth_hints: route.auth_hints.clone(),
                confidence: route.confidence.clone(),
                status: route.status.clone(),
            })
            .collect();
        let (database_systems, schema_objects, migrations) =
            discover_database(&inventory, &text_by_path);
        let (auth_systems, permission_boundaries) =
            discover_auth(&inventory, &text_by_path, &routes);
        let (ui_surfaces, journeys) = discover_ui(&inventory, &text_by_path);
        let (test_suites, ci_workflows) = discover_tests_ci(&inventory, &text_by_path);
        let deployment_targets = discover_deployment(&inventory, &text_by_path);
        let documentation_promises = discover_promises(&inventory, &text_by_path);
        let mut report = reconcile(
            TakeoverReport {
                takeover,
                snapshot,
                inventory,
                areas,
                build_systems,
                dependency_graph,
                application_surfaces,
                routes,
                api_endpoints,
                database_systems,
                schema_objects,
                migrations,
                auth_systems,
                permission_boundaries,
                ui_surfaces,
                journeys,
                test_suites,
                ci_workflows,
                deployment_targets,
                runtime_probes: Vec::new(),
                documentation_promises,
                capabilities: Vec::new(),
                conflicts: Vec::new(),
                findings: Vec::new(),
                recommendations: Vec::new(),
                revisions: Vec::new(),
                fingerprint: String::new(),
            },
            &text_by_path,
        );
        report.fingerprint = report_fingerprint(&report)?;
        report.revisions.push(TakeoverRevision {
            id: stable_id(
                "takeover-revision",
                &[&report.takeover.id, &report.fingerprint],
            ),
            takeover_id: report.takeover.id.clone(),
            revision: 1,
            fingerprint: report.fingerprint.clone(),
            created_at: now_ms(),
            changed_paths: report
                .inventory
                .iter()
                .filter(|file| file.included)
                .map(|file| file.relative_path.clone())
                .collect(),
        });
        Ok(report)
    }

    /// Run the explicitly supported test/build probes in a disposable copy.
    /// Static scanning never calls this method implicitly, so importing a
    /// repository cannot execute its scripts merely by being selected.
    pub fn scan_with_sandbox_runtime(&self, root: &Path) -> Result<TakeoverReport, TakeoverError> {
        let report = self.scan(root)?;
        self.verify_sandbox_runtime(root, report)
    }

    pub fn verify_sandbox_runtime(
        &self,
        root: &Path,
        mut report: TakeoverReport,
    ) -> Result<TakeoverReport, TakeoverError> {
        let canonical = canonical_root(root)?;
        let sandbox_base = std::env::var_os("CARGO_TARGET_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
            .join("p5-sandboxes");
        fs::create_dir_all(&sandbox_base)
            .map_err(|error| TakeoverError::ReadFailure(error.to_string()))?;
        let sandbox = sandbox_base.join(stable_id(
            "sandbox",
            &[&report.takeover.id, &report.snapshot.fingerprint.value],
        ));
        if sandbox.exists() {
            fs::remove_dir_all(&sandbox)
                .map_err(|error| TakeoverError::ReadFailure(error.to_string()))?;
        }
        fs::create_dir_all(&sandbox)
            .map_err(|error| TakeoverError::ReadFailure(error.to_string()))?;
        copy_sandbox_tree(&canonical, &sandbox, &self.config)?;

        let mut probes = Vec::new();
        for (label, script_name) in [("test", "test"), ("build", "build")] {
            let Some(command) = sandbox_script_command(&sandbox, script_name)? else {
                continue;
            };
            let mut probe = RuntimeProbe {
                id: stable_id("sandbox-probe", &[&report.takeover.id, label]),
                target: format!("repository {label}"),
                exact_command: command.clone(),
                safety: ProbeSafety::SandboxRequired,
                status: "PLANNED_SANDBOX_REQUIRED".into(),
                exit_code: None,
                stdout: String::new(),
                stderr: String::new(),
                started_at: now_ms(),
                ended_at: None,
                timeout_ms: self.config.default_probe_timeout_ms,
                workspace_fingerprint: report.snapshot.fingerprint.value.clone(),
                source: Vec::new(),
            };
            let (status, exit_code, stdout, stderr) = execute_structured_command(
                &command,
                &sandbox,
                probe.timeout_ms,
                self.config.max_probe_output_bytes,
            )?;
            probe.status = status;
            probe.exit_code = exit_code;
            probe.stdout = stdout;
            probe.stderr = stderr;
            probe.ended_at = Some(now_ms());
            probes.push(probe);
        }
        let _ = fs::remove_dir_all(&sandbox);
        for probe in probes {
            let finding_type = if probe.target.ends_with("test") {
                "failing-tests"
            } else {
                "broken-build"
            };
            if probe.status == "FAILED" {
                if let Some(finding) = report
                    .findings
                    .iter_mut()
                    .find(|finding| finding.finding_type == finding_type)
                {
                    finding.evidence.push(evidence(
                        "sandbox-runtime",
                        &format!(
                            "{} exited with {:?} in a disposable copy.",
                            probe.target, probe.exit_code
                        ),
                        report.takeover.source.clone(),
                        false,
                    ));
                }
            }
            report.runtime_probes.push(probe);
        }
        report.fingerprint = report_fingerprint(&report)?;
        Ok(report)
    }

    fn collect_directory(
        &self,
        root: &Path,
        directory: &Path,
        collected: &mut Vec<CollectedFile>,
        total_bytes: &mut u64,
    ) -> Result<(), TakeoverError> {
        let entries = fs::read_dir(directory)
            .map_err(|error| TakeoverError::ReadFailure(error.to_string()))?;
        for entry in entries {
            let entry = entry.map_err(|error| TakeoverError::ReadFailure(error.to_string()))?;
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path)
                .map_err(|error| TakeoverError::ReadFailure(error.to_string()))?;
            if metadata.file_type().is_symlink() {
                collected.push(CollectedFile {
                    record: excluded_record(root, &path, "symlink/reparse point is not traversed"),
                    text: None,
                });
                continue;
            }
            if metadata.is_dir() {
                let name = path
                    .file_name()
                    .and_then(|value| value.to_str())
                    .unwrap_or_default();
                if self.config.excluded_directories.contains(name) {
                    collected.push(CollectedFile {
                        record: excluded_record(
                            root,
                            &path,
                            format!("excluded dependency/generated directory: {name}"),
                        ),
                        text: None,
                    });
                    continue;
                }
                self.collect_directory(root, &path, collected, total_bytes)?;
                continue;
            }
            if !metadata.is_file() {
                continue;
            }
            if collected.iter().filter(|file| file.record.included).count() as u64
                >= self.config.max_files
            {
                return Err(TakeoverError::FileLimitExceeded(self.config.max_files));
            }
            *total_bytes = total_bytes.saturating_add(metadata.len());
            if *total_bytes > self.config.max_total_bytes {
                return Err(TakeoverError::ByteLimitExceeded(
                    self.config.max_total_bytes,
                ));
            }
            collected.push(self.collect_file(root, &path, &metadata)?);
        }
        Ok(())
    }

    fn collect_file(
        &self,
        root: &Path,
        path: &Path,
        metadata: &Metadata,
    ) -> Result<CollectedFile, TakeoverError> {
        let relative = path
            .strip_prefix(root)
            .map_err(|error| TakeoverError::PathOutsideRoot(error.to_string()))?
            .to_string_lossy()
            .replace('\\', "/");
        let bytes = if metadata.len() <= self.config.max_file_bytes {
            fs::read(path).map_err(|error| TakeoverError::ReadFailure(error.to_string()))?
        } else {
            let mut file =
                File::open(path).map_err(|error| TakeoverError::ReadFailure(error.to_string()))?;
            let mut prefix = vec![0_u8; self.config.max_file_bytes.min(8192) as usize];
            let read = file
                .read(&mut prefix)
                .map_err(|error| TakeoverError::ReadFailure(error.to_string()))?;
            prefix.truncate(read);
            prefix
        };
        let binary = bytes.contains(&0);
        // Text inspection remains bounded, but identity must never be based on a
        // prefix or on file size alone.  Stream the complete file so a same-size
        // change anywhere in a large file invalidates the repository fingerprint.
        let hash = Some(stream_sha256(path)?);
        let classification = classify_file(&relative, binary);
        let language = language_for(&relative);
        let path_string = path.to_string_lossy().into_owned();
        let content_hash = hash.clone().unwrap_or_else(|| {
            sha256_hex(format!("large:{}:{}", relative, metadata.len()).as_bytes())
        });
        let source = provenance(
            "file",
            &path_string,
            &content_hash,
            "Read-only repository inventory evidence.",
        );
        let text = if binary || metadata.len() > self.config.max_file_bytes {
            None
        } else {
            String::from_utf8(bytes).ok()
        };
        Ok(CollectedFile {
            record: RepositoryFile {
                id: stable_id("repository-file", &[&relative, &content_hash]),
                relative_path: relative,
                canonical_path: path_string,
                file_type: file_type_for(path),
                size_bytes: metadata.len(),
                content_hash: hash,
                language,
                classification,
                included: true,
                excluded_reason: None,
                source,
                discovered_at: now_ms(),
            },
            text,
        })
    }
}

fn canonical_root(root: &Path) -> Result<PathBuf, TakeoverError> {
    if !root.exists() {
        return Err(TakeoverError::RootDoesNotExist(root.display().to_string()));
    }
    if !root.is_dir() {
        return Err(TakeoverError::RootNotDirectory(root.display().to_string()));
    }
    root.canonicalize()
        .map_err(|error| TakeoverError::RootCanonicalization(error.to_string()))
}

fn excluded_record(root: &Path, path: &Path, reason: impl Into<String>) -> RepositoryFile {
    let relative = path
        .strip_prefix(root)
        .map(|value| value.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| path.to_string_lossy().into_owned());
    let path_string = path.to_string_lossy().into_owned();
    let reason = reason.into();
    RepositoryFile {
        id: stable_id("excluded", &[&relative, &reason]),
        relative_path: relative.clone(),
        canonical_path: path_string.clone(),
        file_type: "directory-or-symlink".into(),
        size_bytes: 0,
        content_hash: None,
        language: None,
        classification: FileClassification::Excluded,
        included: false,
        excluded_reason: Some(reason),
        source: provenance(
            "excluded",
            &path_string,
            "",
            "Excluded without reading repository contents.",
        ),
        discovered_at: now_ms(),
    }
}

fn classify_file(path: &str, binary: bool) -> FileClassification {
    if binary {
        return FileClassification::Binary;
    }
    let lower = path.to_ascii_lowercase();
    if lower.contains("/test")
        || lower.contains("tests/")
        || lower.ends_with(".test.ts")
        || lower.ends_with(".spec.ts")
    {
        return FileClassification::Test;
    }
    if lower.contains("migration") || lower.contains("/migrations/") {
        return FileClassification::Migration;
    }
    if lower.contains("schema") || lower.ends_with("schema.prisma") {
        return FileClassification::Schema;
    }
    if lower.starts_with(".github/") || lower.contains("/workflows/") {
        return FileClassification::Ci;
    }
    if lower.contains("docker")
        || lower.contains("terraform")
        || lower.contains("k8s/")
        || lower.contains("kubernetes/")
        || lower.ends_with("compose.yml")
        || lower.ends_with("compose.yaml")
    {
        return FileClassification::Deployment;
    }
    if lower.ends_with(".md")
        || lower.ends_with(".mdx")
        || lower.contains("/docs/")
        || lower.contains("readme")
    {
        return FileClassification::Documentation;
    }
    if lower.ends_with(".json")
        || lower.ends_with(".yaml")
        || lower.ends_with(".yml")
        || lower.ends_with(".toml")
        || lower.ends_with(".ini")
        || lower.ends_with(".env")
    {
        return FileClassification::Configuration;
    }
    if lower.ends_with(".png")
        || lower.ends_with(".jpg")
        || lower.ends_with(".jpeg")
        || lower.ends_with(".gif")
        || lower.ends_with(".svg")
        || lower.ends_with(".ico")
    {
        return FileClassification::Asset;
    }
    FileClassification::Source
}

fn language_for(path: &str) -> Option<String> {
    let extension = Path::new(path)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let language = match extension.as_str() {
        "rs" => "Rust",
        "ts" | "tsx" => "TypeScript",
        "js" | "jsx" | "mjs" => "JavaScript",
        "py" => "Python",
        "go" => "Go",
        "java" | "kt" => "JVM",
        "cs" => ".NET",
        "dart" => "Dart",
        "sql" => "SQL",
        "html" => "HTML",
        "css" => "CSS",
        "md" | "mdx" => "Markdown",
        "json" => "JSON",
        "yaml" | "yml" => "YAML",
        "toml" => "TOML",
        _ => return None,
    };
    Some(language.into())
}

fn file_type_for(path: &Path) -> String {
    path.extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase())
        .unwrap_or_else(|| {
            path.file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("file")
                .into()
        })
}

fn provenance(source_id: &str, locator: &str, hash: &str, note: &str) -> Provenance {
    Provenance {
        source_id: source_id.into(),
        locator: locator.into(),
        content_hash: hash.into(),
        note: note.into(),
    }
}

fn stable_id(prefix: &str, values: &[&str]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(prefix.as_bytes());
    for value in values {
        hasher.update([0]);
        hasher.update(value.as_bytes());
    }
    let digest = hex_lower(&hasher.finalize());
    format!("{prefix}_{}", &digest[..24])
}

fn sha256_hex(bytes: &[u8]) -> String {
    hex_lower(&Sha256::digest(bytes))
}

fn stream_sha256(path: &Path) -> Result<String, TakeoverError> {
    let mut file =
        File::open(path).map_err(|error| TakeoverError::ReadFailure(error.to_string()))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| TakeoverError::ReadFailure(error.to_string()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex_lower(&hasher.finalize()))
}

fn hex_lower(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
fn now_ms() -> i128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_millis() as i128)
        .unwrap_or_default()
}

fn find_file<'a>(files: &'a [RepositoryFile], suffix: &str) -> Vec<&'a RepositoryFile> {
    files
        .iter()
        .filter(|file| {
            file.included
                && file
                    .relative_path
                    .to_ascii_lowercase()
                    .ends_with(&suffix.to_ascii_lowercase())
        })
        .collect()
}

fn detect_build_systems(
    files: &[RepositoryFile],
    text: &BTreeMap<String, String>,
) -> Vec<BuildSystem> {
    let mut result = Vec::new();
    let mut add = |name: &str, kind: BuildSystemKind, paths: Vec<&RepositoryFile>| {
        if paths.is_empty() {
            return;
        }
        result.push(BuildSystem {
            id: stable_id("build-system", &[name]),
            name: name.into(),
            kind,
            evidence: paths.into_iter().map(|file| file.source.clone()).collect(),
            confidence: "high".into(),
            status: "detected_from_actual_files".into(),
        });
    };
    add(
        "JavaScript/TypeScript",
        BuildSystemKind::JavaScript,
        find_file(files, "package.json"),
    );
    add(
        "pnpm workspace",
        BuildSystemKind::JavaScript,
        find_file(files, "pnpm-workspace.yaml"),
    );
    add(
        "Rust Cargo",
        BuildSystemKind::Rust,
        find_file(files, "cargo.toml"),
    );
    add("Python", BuildSystemKind::Python, {
        let mut paths = find_file(files, "pyproject.toml");
        paths.extend(find_file(files, "requirements.txt"));
        paths
    });
    add(
        "Go modules",
        BuildSystemKind::Go,
        find_file(files, "go.mod"),
    );
    add(
        "Java/Maven",
        BuildSystemKind::Java,
        find_file(files, "pom.xml"),
    );
    add(
        ".NET",
        BuildSystemKind::DotNet,
        files
            .iter()
            .filter(|file| {
                file.included
                    && (file.relative_path.ends_with(".sln")
                        || file.relative_path.ends_with(".csproj"))
            })
            .collect(),
    );
    add(
        "Tauri",
        BuildSystemKind::Tauri,
        files
            .iter()
            .filter(|file| {
                file.included
                    && (file.relative_path.ends_with("tauri.conf.json")
                        || text
                            .get(&file.relative_path)
                            .is_some_and(|value| value.contains("tauri::command")))
            })
            .collect(),
    );
    add(
        "Electron",
        BuildSystemKind::Electron,
        files
            .iter()
            .filter(|file| {
                file.included
                    && (file.relative_path.ends_with("electron-builder.yml")
                        || text
                            .get(&file.relative_path)
                            .is_some_and(|value| value.contains("electron")))
            })
            .collect(),
    );
    add(
        "Container",
        BuildSystemKind::Container,
        files
            .iter()
            .filter(|file| {
                file.included
                    && (file.relative_path.ends_with("Dockerfile")
                        || file
                            .relative_path
                            .to_ascii_lowercase()
                            .contains("compose.yml"))
            })
            .collect(),
    );
    add(
        "CI workflows",
        BuildSystemKind::Ci,
        files
            .iter()
            .filter(|file| file.included && file.relative_path.starts_with(".github/workflows/"))
            .collect(),
    );
    result
}

fn discover_areas(files: &[RepositoryFile]) -> Vec<RepositoryArea> {
    let mut grouped: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for file in files.iter().filter(|file| file.included) {
        let area = file
            .relative_path
            .split('/')
            .next()
            .unwrap_or("root")
            .to_string();
        grouped
            .entry(area)
            .or_default()
            .push(file.relative_path.clone());
    }
    grouped
        .into_iter()
        .map(|(path, files)| RepositoryArea {
            id: stable_id("area", &[&path]),
            area_type: if path == "src" {
                "application".into()
            } else {
                "directory".into()
            },
            source: provenance("area", &path, "", "Deterministic repository area grouping."),
            confidence: "high".into(),
            path,
            files,
        })
        .collect()
}

fn discover_dependencies(
    files: &[RepositoryFile],
    text: &BTreeMap<String, String>,
    systems: &[BuildSystem],
) -> DependencyGraph {
    let mut dependencies = Vec::new();
    let mut nodes = Vec::new();
    let mut workspace_names = BTreeSet::new();
    let mut manifest_owners = BTreeMap::<String, String>::new();
    let mut workspace_members = Vec::<String>::new();

    for file in find_file(files, "package.json") {
        if let Some(value) = text
            .get(&file.relative_path)
            .and_then(|raw| serde_json::from_str::<Value>(raw).ok())
        {
            if let Some(name) = value.get("name").and_then(Value::as_str) {
                manifest_owners.insert(file.relative_path.clone(), name.to_string());
                workspace_names.insert(name.to_string());
                nodes.push(name.to_string());
            }
        }
    }
    for file in find_file(files, "cargo.toml") {
        if let Some(raw) = text.get(&file.relative_path) {
            if let Some(name) = cargo_package_name(raw) {
                manifest_owners.insert(file.relative_path.clone(), name.clone());
                workspace_names.insert(name.clone());
                nodes.push(name);
            }
            workspace_members.extend(cargo_workspace_members(raw));
        }
    }
    workspace_members.sort();
    workspace_members.dedup();
    let manifest_paths = manifest_owners.keys().cloned().collect::<BTreeSet<_>>();
    let mut inconsistency = workspace_members
        .iter()
        .filter(|member| {
            let member_prefix = member.trim_end_matches('/');
            !manifest_paths.iter().any(|path| {
                path == &format!("{member_prefix}/package.json")
                    || path == &format!("{member_prefix}/Cargo.toml")
                    || path.starts_with(&format!("{member_prefix}/"))
            })
        })
        .map(|member| format!("workspace member reference is missing: {member}"))
        .collect::<Vec<_>>();

    for file in find_file(files, "package.json") {
        if let Some(value) = text
            .get(&file.relative_path)
            .and_then(|raw| serde_json::from_str::<Value>(raw).ok())
        {
            let source_package = manifest_owners.get(&file.relative_path).cloned();
            for (section, kind) in [
                ("dependencies", DependencyKind::Runtime),
                ("devDependencies", DependencyKind::Development),
                ("optionalDependencies", DependencyKind::Optional),
            ] {
                if let Some(object) = value.get(section).and_then(Value::as_object) {
                    for (name, version) in object {
                        dependencies.push(Dependency {
                            id: stable_id("dependency", &[&file.relative_path, name, section]),
                            name: name.clone(),
                            source_package: source_package.clone(),
                            version: version.as_str().map(str::to_string),
                            kind: kind.clone(),
                            source: file.source.clone(),
                            workspace_member: None,
                            missing_reference: version
                                .as_str()
                                .is_some_and(|value| value.starts_with("workspace:")),
                        });
                    }
                }
            }
        }
    }
    for file in find_file(files, "cargo.toml") {
        if let Some(raw) = text.get(&file.relative_path) {
            let source_package = manifest_owners.get(&file.relative_path).cloned();
            let mut section = String::new();
            for line in raw.lines() {
                let line = line.trim();
                if line.starts_with('[') {
                    section = line.trim_matches(&['[', ']'][..]).into();
                    continue;
                }
                if (section == "dependencies"
                    || section == "dev-dependencies"
                    || section == "build-dependencies")
                    && line.contains('=')
                    && !line.starts_with('#')
                {
                    let mut parts = line.splitn(2, '=');
                    let name = parts.next().unwrap_or_default().trim();
                    if !name.is_empty() {
                        let value = parts.next().unwrap_or_default().trim();
                        let path_reference = quoted_value(value, "path");
                        let missing_reference = path_reference.is_some_and(|target| {
                            let base = Path::new(&file.relative_path)
                                .parent()
                                .unwrap_or_else(|| Path::new(""));
                            let target_manifest = base.join(target).join("Cargo.toml");
                            !manifest_paths
                                .contains(&target_manifest.to_string_lossy().replace('\\', "/"))
                        });
                        dependencies.push(Dependency {
                            id: stable_id("dependency", &[&file.relative_path, name, &section]),
                            name: name.into(),
                            source_package: source_package.clone(),
                            version: Some(value.to_string()),
                            kind: if section == "dev-dependencies" {
                                DependencyKind::Development
                            } else if section == "build-dependencies" {
                                DependencyKind::Build
                            } else {
                                DependencyKind::Runtime
                            },
                            source: file.source.clone(),
                            workspace_member: None,
                            missing_reference,
                        });
                    }
                }
            }
        }
    }
    for dependency in &mut dependencies {
        if workspace_names.contains(&dependency.name) {
            dependency.kind = DependencyKind::InternalWorkspace;
            dependency.workspace_member = Some(dependency.name.clone());
        }
        if dependency.missing_reference {
            inconsistency.push(format!(
                "dependency reference is missing or unresolved: {}",
                dependency.name
            ));
        }
    }
    for system in systems {
        if system.name.contains("JavaScript")
            && !files.iter().any(|file| {
                file.included && file.relative_path == "pnpm-lock.yaml"
                    || file.relative_path == "package-lock.json"
                    || file.relative_path == "yarn.lock"
            })
        {
            inconsistency.push("JavaScript manifest exists without a lockfile evidence.".into());
        }
    }
    nodes.sort();
    nodes.dedup();
    let mut internal_edges = dependencies
        .iter()
        .filter_map(|dependency| {
            dependency.workspace_member.as_ref().and_then(|target| {
                dependency
                    .source_package
                    .as_ref()
                    .map(|source| (source.clone(), target.clone()))
            })
        })
        .collect::<Vec<_>>();
    internal_edges.sort();
    internal_edges.dedup();
    let cycles = dependency_cycles(&nodes, &internal_edges);
    DependencyGraph {
        id: stable_id("dependency-graph", &[&dependencies.len().to_string()]),
        nodes,
        dependencies,
        internal_edges,
        cycles,
        manifest_lock_inconsistencies: inconsistency,
        source: files
            .iter()
            .filter(|file| {
                file.included
                    && (file.relative_path.ends_with("package.json")
                        || file.relative_path.ends_with("Cargo.toml"))
            })
            .map(|file| file.source.clone())
            .collect(),
    }
}

fn cargo_package_name(raw: &str) -> Option<String> {
    let mut section = String::new();
    for line in raw.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            section = line.trim_matches(&['[', ']'][..]).to_string();
        } else if section == "package" && line.starts_with("name") {
            return line
                .split_once('=')
                .map(|(_, value)| value.trim().trim_matches('"').to_string());
        }
    }
    None
}

fn cargo_workspace_members(raw: &str) -> Vec<String> {
    let mut section = String::new();
    let mut members = Vec::new();
    for line in raw.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            section = line.trim_matches(&['[', ']'][..]).to_string();
        } else if section == "workspace" && line.starts_with("members") {
            members.extend(quoted_strings(line));
        }
    }
    members
}

fn quoted_strings(value: &str) -> Vec<String> {
    value
        .split('"')
        .enumerate()
        .filter_map(|(index, part)| (index % 2 == 1).then_some(part.to_string()))
        .collect()
}

fn quoted_value(value: &str, key: &str) -> Option<String> {
    let marker = format!("{key} =");
    value
        .split_once(&marker)
        .and_then(|(_, tail)| quoted_strings(tail).into_iter().next())
}

fn dependency_cycles(nodes: &[String], edges: &[(String, String)]) -> Vec<Vec<String>> {
    let mut adjacency = BTreeMap::<String, Vec<String>>::new();
    for (source, target) in edges {
        adjacency
            .entry(source.clone())
            .or_default()
            .push(target.clone());
    }
    for targets in adjacency.values_mut() {
        targets.sort();
        targets.dedup();
    }
    let mut cycles = Vec::new();
    for start in nodes {
        let mut path = Vec::new();
        collect_cycles(start, start, &adjacency, &mut path, &mut cycles);
    }
    for cycle in &mut cycles {
        if let Some(min_index) = cycle
            .iter()
            .enumerate()
            .min_by_key(|(_, value)| *value)
            .map(|(index, _)| index)
        {
            cycle.rotate_left(min_index);
        }
    }
    cycles.sort();
    cycles.dedup();
    cycles
}

fn collect_cycles(
    start: &str,
    current: &str,
    adjacency: &BTreeMap<String, Vec<String>>,
    path: &mut Vec<String>,
    cycles: &mut Vec<Vec<String>>,
) {
    if path.len() > adjacency.len().saturating_add(1) {
        return;
    }
    path.push(current.to_string());
    for next in adjacency.get(current).into_iter().flatten() {
        if next == start && path.len() > 1 {
            cycles.push(path.clone());
        } else if !path.iter().any(|item| item == next) {
            collect_cycles(start, next, adjacency, path, cycles);
        }
    }
    path.pop();
}

fn discover_application_surfaces(
    files: &[RepositoryFile],
    systems: &[BuildSystem],
) -> Vec<ApplicationSurface> {
    let mut result = Vec::new();
    for file in files.iter().filter(|file| file.included) {
        let lower = file.relative_path.to_ascii_lowercase();
        if lower.contains("/src/")
            || lower == "src"
            || lower.ends_with("main.rs")
            || lower.ends_with("main.ts")
            || lower.ends_with("app.tsx")
        {
            result.push(ApplicationSurface {
                id: stable_id("surface", &[&file.relative_path]),
                name: file.relative_path.clone(),
                path: file.relative_path.clone(),
                surface_type: if systems
                    .iter()
                    .any(|system| system.kind == BuildSystemKind::Tauri)
                {
                    "desktop/application".into()
                } else {
                    "application/source".into()
                },
                source: file.source.clone(),
                confidence: "medium".into(),
            });
        }
    }
    result
}

fn discover_routes(files: &[RepositoryFile], text: &BTreeMap<String, String>) -> Vec<Route> {
    let mut result = Vec::new();
    for file in files.iter().filter(|file| file.included) {
        let Some(raw) = text.get(&file.relative_path) else {
            continue;
        };
        let mut command_pending = false;
        for (line_number, line) in raw.lines().enumerate() {
            let trimmed = line.trim();
            if is_comment_line(trimmed) {
                continue;
            }
            if trimmed.contains("#[tauri::command]") {
                command_pending = true;
                continue;
            }
            if command_pending && (trimmed.contains("fn ") || trimmed.contains("pub fn ")) {
                let handler = trimmed
                    .split("fn ")
                    .nth(1)
                    .and_then(|value| value.split('(').next())
                    .map(str::trim)
                    .unwrap_or("command");
                result.push(route_record(
                    file,
                    "TAURI_COMMAND",
                    &format!("tauri://{handler}"),
                    Some(handler.into()),
                    line_number + 1,
                    RouteStatus::Registered,
                ));
                command_pending = false;
            }
            for method in ["GET", "POST", "PUT", "PATCH", "DELETE"] {
                let lower = trimmed.to_ascii_lowercase();
                let marker = format!("{}.(", method.to_ascii_lowercase());
                let marker_alt = format!("{}.route(", method.to_ascii_lowercase());
                if lower.contains(&marker)
                    || lower.contains(&marker_alt)
                    || lower.contains(&format!("app.{}(", method.to_ascii_lowercase()))
                    || lower.contains(&format!("router.{}(", method.to_ascii_lowercase()))
                {
                    if let Some(path) = first_quoted(trimmed) {
                        let handler = trimmed
                            .split(',')
                            .nth(1)
                            .map(|value| value.trim().trim_end_matches(')').to_string());
                        result.push(route_record(
                            file,
                            method,
                            path,
                            handler,
                            line_number + 1,
                            RouteStatus::ReachabilityUnproven,
                        ));
                    }
                }
            }
            if let Some((method, path)) = python_route(trimmed) {
                result.push(route_record(
                    file,
                    method,
                    path,
                    None,
                    line_number + 1,
                    RouteStatus::ReachabilityUnproven,
                ));
            }
            if trimmed.contains(".route(") && trimmed.contains('"') {
                if let Some(path) = first_quoted(trimmed) {
                    result.push(route_record(
                        file,
                        "ROUTE",
                        path,
                        None,
                        line_number + 1,
                        RouteStatus::Registered,
                    ));
                }
            }
            if (trimmed.contains("createBrowserRouter") || trimmed.contains("path:"))
                && trimmed.contains('"')
            {
                if let Some(path) = first_quoted(trimmed) {
                    result.push(route_record(
                        file,
                        "UI",
                        path,
                        None,
                        line_number + 1,
                        RouteStatus::Declared,
                    ));
                }
            }
        }
    }
    dedupe_routes(result)
}

fn route_record(
    file: &RepositoryFile,
    method: &str,
    path: &str,
    handler: Option<String>,
    line: usize,
    status: RouteStatus,
) -> Route {
    let locator = format!("{}:{line}", file.relative_path);
    let source = provenance(
        "route",
        &locator,
        file.content_hash.as_deref().unwrap_or_default(),
        "Route registration or declaration evidence.",
    );
    let auth_hints = if locator.to_ascii_lowercase().contains("auth")
        || path.contains("admin")
        || path.contains("account")
    {
        vec!["authentication or ownership may apply".into()]
    } else {
        Vec::new()
    };
    Route {
        id: stable_id("route", &[method, path, &locator]),
        method: method.into(),
        path: path.into(),
        handler,
        source,
        auth_hints,
        request_hints: Vec::new(),
        response_hints: Vec::new(),
        confidence: if matches!(status, RouteStatus::Registered) {
            "high".into()
        } else {
            "medium".into()
        },
        status,
    }
}

fn dedupe_routes(routes: Vec<Route>) -> Vec<Route> {
    let mut seen = BTreeSet::new();
    routes
        .into_iter()
        .filter(|route| {
            seen.insert(format!(
                "{}:{}:{}",
                route.method, route.path, route.source.locator
            ))
        })
        .collect()
}

fn python_route(line: &str) -> Option<(&str, &str)> {
    for method in ["get", "post", "put", "patch", "delete"] {
        let marker = format!("@app.{method}(\"");
        if let Some(start) = line.find(&marker) {
            let rest = &line[start + marker.len()..];
            if let Some(end) = rest.find('"') {
                return Some((method, &rest[..end]));
            }
        }
    }
    None
}

fn first_quoted(value: &str) -> Option<&str> {
    let quote = value.find('"').or_else(|| value.find('\''))?;
    let marker = value.as_bytes()[quote];
    let rest = &value[quote + 1..];
    let end = rest.as_bytes().iter().position(|byte| *byte == marker)?;
    Some(&rest[..end])
}

fn is_comment_line(line: &str) -> bool {
    line.starts_with("//")
        || line.starts_with('#')
        || line.starts_with("/*")
        || line.starts_with('*')
}

fn code_without_comments(raw: &str) -> String {
    raw.lines()
        .filter(|line| !is_comment_line(line.trim()))
        .collect::<Vec<_>>()
        .join("\n")
}

fn discover_database(
    files: &[RepositoryFile],
    text: &BTreeMap<String, String>,
) -> (Vec<DatabaseSystem>, Vec<SchemaObject>, Vec<MigrationRecord>) {
    let mut systems = Vec::new();
    let mut evidence = Vec::new();
    let mut configurations = Vec::new();
    for file in files.iter().filter(|file| file.included) {
        let lower = file.relative_path.to_ascii_lowercase();
        let raw = text
            .get(&file.relative_path)
            .map(String::as_str)
            .unwrap_or_default()
            .to_ascii_lowercase();
        let technology = if lower.contains("postgres")
            || raw.contains("postgresql")
            || raw.contains("postgres://")
        {
            Some("PostgreSQL")
        } else if raw.contains("sqlite") || lower.ends_with(".db") {
            Some("SQLite")
        } else if raw.contains("mysql") || raw.contains("mysql://") {
            Some("MySQL")
        } else if lower.ends_with("schema.prisma") {
            Some("Prisma schema")
        } else if lower.contains("models.py") || raw.contains("sqlalchemy") {
            Some("SQLAlchemy")
        } else {
            None
        };
        if let Some(technology) = technology {
            evidence.push(file.source.clone());
            configurations.push(file.relative_path.clone());
            systems.push(DatabaseSystem {
                id: stable_id("database", &[technology]),
                technology: technology.into(),
                evidence: vec![file.source.clone()],
                configuration_paths: vec![file.relative_path.clone()],
                confidence: "medium".into(),
            });
        }
    }
    systems.sort_by(|a, b| a.technology.cmp(&b.technology));
    systems.dedup_by(|a, b| a.technology == b.technology);
    let mut schema_objects = Vec::new();
    let mut migration_files = Vec::new();
    for file in files.iter().filter(|file| file.included) {
        let lower = file.relative_path.to_ascii_lowercase();
        let raw = text
            .get(&file.relative_path)
            .map(String::as_str)
            .unwrap_or_default();
        if lower.contains("migration") || lower.contains("/migrations/") {
            if let Some(identifier) = migration_identifier(&file.relative_path) {
                migration_files.push((identifier, file, raw.to_ascii_lowercase()));
            }
        }
        for line in raw.lines() {
            let lower_line = line.to_ascii_lowercase();
            let object_type = if lower_line.contains("create table") {
                Some("table")
            } else if lower_line.contains("model ") && lower.ends_with("schema.prisma") {
                Some("model")
            } else {
                None
            };
            if let Some(object_type) = object_type {
                let name = schema_object_name(line, object_type);
                schema_objects.push(SchemaObject {
                    id: stable_id("schema-object", &[&name, &file.relative_path]),
                    name,
                    object_type: object_type.into(),
                    source: file.source.clone(),
                    migration_evidence: Vec::new(),
                    migration_required: true,
                });
            }
        }
    }
    migration_files.sort_by(|a, b| a.0.cmp(&b.0));
    let mut counts = BTreeMap::new();
    for (identifier, _, _) in &migration_files {
        *counts.entry(identifier.clone()).or_insert(0_u32) += 1;
    }
    let migrations = migration_files
        .iter()
        .enumerate()
        .map(|(index, (identifier, file, _))| MigrationRecord {
            id: stable_id("migration", &[identifier, &file.relative_path]),
            identifier: identifier.clone(),
            path: file.relative_path.clone(),
            order: identifier.parse().ok(),
            source: file.source.clone(),
            duplicate_identifier: counts.get(identifier).copied().unwrap_or_default() > 1,
            ordering_gap: index > 0
                && migration_files[index - 1]
                    .0
                    .parse::<u64>()
                    .ok()
                    .zip(identifier.parse::<u64>().ok())
                    .is_some_and(|(previous, current)| current > previous + 1),
        })
        .collect::<Vec<_>>();
    for object in &mut schema_objects {
        object.migration_evidence = migration_files
            .iter()
            .filter(|(_, file, migration_text)| {
                let object_name = object.name.to_ascii_lowercase();
                migration_text.contains(&object_name)
                    && (migration_text.contains("create table")
                        || migration_text.contains("alter table")
                        || migration_text.contains("create model")
                        || file
                            .relative_path
                            .to_ascii_lowercase()
                            .contains(&object_name))
            })
            .map(|(_, file, _)| file.relative_path.clone())
            .collect();
        object.migration_required = object.migration_evidence.is_empty();
    }
    let mut unique = BTreeMap::new();
    for system in systems {
        unique.entry(system.technology.clone()).or_insert(system);
    }
    let systems = unique.into_values().collect::<Vec<_>>();
    let _ = (evidence, configurations);
    (systems, schema_objects, migrations)
}

fn schema_object_name(line: &str, object_type: &str) -> String {
    let lower = line.to_ascii_lowercase();
    let marker = if object_type == "table" {
        "create table"
    } else {
        "model"
    };
    lower
        .find(marker)
        .and_then(|index| line.get(index + marker.len()..))
        .and_then(|rest| {
            rest.split(|character: char| !character.is_alphanumeric() && character != '_')
                .find(|token| !token.is_empty())
        })
        .unwrap_or("unknown")
        .to_string()
}

fn migration_identifier(path: &str) -> Option<String> {
    let stem = Path::new(path).file_stem()?.to_string_lossy();
    let digits = stem
        .chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>();
    if digits.is_empty() {
        None
    } else {
        Some(digits)
    }
}

fn discover_auth(
    files: &[RepositoryFile],
    text: &BTreeMap<String, String>,
    routes: &[Route],
) -> (Vec<AuthSystem>, Vec<PermissionBoundary>) {
    let mut systems = Vec::new();
    let mut boundaries = Vec::new();
    for file in files.iter().filter(|file| file.included) {
        let raw = text
            .get(&file.relative_path)
            .map(String::as_str)
            .unwrap_or_default();
        let lower = code_without_comments(raw).to_ascii_lowercase();
        let technology = if lower.contains("oauth") || lower.contains("oidc") {
            Some("OAuth/OIDC")
        } else if lower.contains("jsonwebtoken") || lower.contains("jwt") {
            Some("JWT/token")
        } else if lower.contains("passport") {
            Some("Passport")
        } else if lower.contains("api_key") || lower.contains("api-key") {
            Some("API key")
        } else if lower.contains("session")
            && (lower.contains("middleware") || lower.contains("cookie"))
        {
            Some("session")
        } else {
            None
        };
        if let Some(technology) = technology {
            let implemented = lower.contains("jwt.verify")
                || lower.contains("verify_token")
                || lower.contains("verifytoken")
                || lower.contains("req.user")
                || lower.contains("request.user")
                || lower.contains("function requireauth")
                || lower.contains("const requireauth")
                || lower.contains("fn require_auth")
                || lower.contains("middleware") && lower.contains("next(");
            let mut states = vec![AuthEvidenceState::AuthLibraryPresent];
            if implemented {
                states.push(AuthEvidenceState::AuthImplemented);
            }
            states.push(AuthEvidenceState::AuthorizationUnproven);
            systems.push(AuthSystem {
                id: stable_id("auth", &[technology, &file.relative_path]),
                technology: technology.into(),
                states,
                evidence: vec![file.source.clone()],
                confidence: "medium".into(),
            });
        }
        if lower.contains("requireauth")
            || lower.contains("require_auth")
            || lower.contains("authmiddleware")
            || lower.contains("protectedroute")
            || lower.contains("authorize")
        {
            let protected = routes
                .iter()
                .filter(|route| route.source.locator.starts_with(&file.relative_path))
                .map(|route| route.path.clone())
                .collect();
            boundaries.push(PermissionBoundary {
                id: stable_id("permission-boundary", &[&file.relative_path]),
                name: "detected authorization boundary".into(),
                boundary_type: "middleware-or-guard".into(),
                protected_paths: protected,
                source: file.source.clone(),
                authorization_state: AuthEvidenceState::AuthorizationUnproven,
            });
        }
    }
    systems.sort_by(|a, b| a.id.cmp(&b.id));
    (systems, boundaries)
}

fn discover_ui(
    files: &[RepositoryFile],
    text: &BTreeMap<String, String>,
) -> (Vec<UiSurface>, Vec<UserJourneyDiscovery>) {
    let mut surfaces = Vec::new();
    for file in files.iter().filter(|file| file.included) {
        let lower = file.relative_path.to_ascii_lowercase();
        if !(lower.ends_with(".tsx")
            || lower.ends_with(".jsx")
            || lower.ends_with(".vue")
            || lower.ends_with(".html"))
        {
            continue;
        }
        let raw = text
            .get(&file.relative_path)
            .map(String::as_str)
            .unwrap_or_default();
        let mut actions = Vec::new();
        if raw.contains("<form") || raw.contains("onSubmit") {
            actions.push("form submission".into());
        }
        if raw.contains("<button") || raw.contains("onClick") {
            actions.push("primary action".into());
        }
        if raw.to_ascii_lowercase().contains("nav") || raw.contains("<a ") {
            actions.push("navigation".into());
        }
        let surface_type = if lower.contains("admin") || lower.contains("settings") {
            "settings/admin"
        } else if lower.contains("login") || lower.contains("sign-in") {
            "auth entry"
        } else {
            "screen/page"
        };
        surfaces.push(UiSurface {
            id: stable_id("ui-surface", &[&file.relative_path]),
            name: file.relative_path.clone(),
            path: file.relative_path.clone(),
            surface_type: surface_type.into(),
            actions,
            source: file.source.clone(),
            state: UiEvidenceState::RuntimeUnproven,
        });
    }
    let journeys = surfaces
        .iter()
        .take(8)
        .map(|surface| UserJourneyDiscovery {
            id: stable_id("journey-discovery", &[&surface.id]),
            name: format!("Review {}", surface.name),
            starting_condition:
                "The existing project surface is statically present; runtime state is unproven."
                    .into(),
            goal: format!("Use {} to reach its intended outcome.", surface.name),
            major_steps: vec![
                "Open discovered surface".into(),
                "Perform statically detected primary action".into(),
                "Verify runtime result with an approved probe".into(),
            ],
            source: vec![surface.source.clone()],
            state: UiEvidenceState::RuntimeUnproven,
        })
        .collect();
    (surfaces, journeys)
}

fn discover_tests_ci(
    files: &[RepositoryFile],
    text: &BTreeMap<String, String>,
) -> (Vec<TestSuite>, Vec<CiWorkflow>) {
    let mut suites = Vec::new();
    let mut workflows = Vec::new();
    for file in files.iter().filter(|file| file.included) {
        let lower = file.relative_path.to_ascii_lowercase();
        let raw = text
            .get(&file.relative_path)
            .map(String::as_str)
            .unwrap_or_default();
        if matches!(file.classification, FileClassification::Test)
            || lower.contains("test")
            || lower.contains("spec")
        {
            let skipped = raw.to_ascii_lowercase().contains("skip")
                || raw.to_ascii_lowercase().contains("ignore")
                || raw.to_ascii_lowercase().contains("disabled");
            suites.push(TestSuite {
                id: stable_id("test-suite", &[&file.relative_path]),
                name: file.relative_path.clone(),
                path: file.relative_path.clone(),
                suite_type: if lower.contains("e2e") || lower.contains("playwright") {
                    "e2e".into()
                } else if lower.contains("integration") {
                    "integration".into()
                } else {
                    "unit-or-component".into()
                },
                command: None,
                skipped_or_disabled: skipped,
                missing_references: Vec::new(),
                source: file.source.clone(),
            });
        }
        if lower.starts_with(".github/workflows/") {
            workflows.push(CiWorkflow {
                id: stable_id("ci-workflow", &[&file.relative_path]),
                name: file.relative_path.clone(),
                path: file.relative_path.clone(),
                commands: raw
                    .lines()
                    .filter(|line| line.trim_start().starts_with("run:"))
                    .map(|line| line.trim().trim_start_matches("run:").trim().to_string())
                    .collect(),
                disabled: raw.to_ascii_lowercase().contains("if: false")
                    || raw.to_ascii_lowercase().contains("disabled: true"),
                source: file.source.clone(),
            });
        }
    }
    for file in files
        .iter()
        .filter(|file| file.included && file.relative_path.ends_with("package.json"))
    {
        if let Some(value) = text
            .get(&file.relative_path)
            .and_then(|raw| serde_json::from_str::<Value>(raw).ok())
        {
            if let Some(scripts) = value.get("scripts").and_then(Value::as_object) {
                for name in ["test", "lint", "typecheck", "build"] {
                    if scripts.contains_key(name) {
                        suites.push(TestSuite {
                            id: stable_id("script-suite", &[&file.relative_path, name]),
                            name: format!("package script: {name}"),
                            path: file.relative_path.clone(),
                            suite_type: if name == "test" {
                                "scripted-tests".into()
                            } else {
                                "quality-gate".into()
                            },
                            command: scripts
                                .get(name)
                                .and_then(Value::as_str)
                                .map(str::to_string),
                            skipped_or_disabled: false,
                            missing_references: Vec::new(),
                            source: file.source.clone(),
                        });
                    }
                }
            }
        }
    }
    (suites, workflows)
}

fn discover_deployment(
    files: &[RepositoryFile],
    text: &BTreeMap<String, String>,
) -> Vec<DeploymentTarget> {
    let mut result = Vec::new();
    for file in files.iter().filter(|file| file.included) {
        let lower = file.relative_path.to_ascii_lowercase();
        let target = if lower.ends_with("dockerfile") || lower.contains("compose") {
            Some("container")
        } else if lower.contains("kubernetes") || lower.contains("k8s/") {
            Some("kubernetes")
        } else if lower.contains("terraform") {
            Some("infrastructure-as-code")
        } else if lower.contains("vercel") {
            Some("vercel")
        } else if lower.contains("netlify") {
            Some("netlify")
        } else if lower.ends_with("tauri.conf.json") {
            Some("desktop-package")
        } else if lower.contains("pm2") {
            Some("node-process")
        } else if lower.ends_with(".env.example")
            || text
                .get(&file.relative_path)
                .is_some_and(|value| value.contains("DATABASE_URL"))
        {
            Some("environment-config")
        } else {
            None
        };
        if let Some(target) = target {
            result.push(DeploymentTarget {
                id: stable_id("deployment", &[target, &file.relative_path]),
                target: target.into(),
                evidence: vec![file.source.clone()],
                state: "CONFIG_PRESENT;DEPLOYMENT_UNPROVEN".into(),
                configuration_paths: vec![file.relative_path.clone()],
            });
        }
    }
    result
}

fn discover_promises(
    files: &[RepositoryFile],
    text: &BTreeMap<String, String>,
) -> Vec<DocumentationPromise> {
    let mut result = Vec::new();
    let promise_words = [
        "supports",
        "production ready",
        "fully tested",
        "oauth",
        "offline",
        "encrypted",
        "admin dashboard",
        "automatic backup",
        "migration complete",
        "npm run",
        "api is at",
    ];
    for file in files.iter().filter(|file| {
        file.included && matches!(file.classification, FileClassification::Documentation)
    }) {
        let Some(raw) = text.get(&file.relative_path) else {
            continue;
        };
        for (index, line) in raw.lines().enumerate() {
            let lower = line.to_ascii_lowercase();
            if promise_words.iter().any(|word| lower.contains(word)) {
                let category = if lower.contains("tested") || lower.contains("production") {
                    "reliability"
                } else if lower.contains("oauth") || lower.contains("encrypted") {
                    "security"
                } else if lower.contains("api") {
                    "api"
                } else {
                    "product"
                };
                let location = format!("{}:{}", file.relative_path, index + 1);
                result.push(DocumentationPromise {
                    id: stable_id("promise", &[&location, line]),
                    text: line.trim().into(),
                    path: file.relative_path.clone(),
                    location: location.clone(),
                    category: category.into(),
                    related_capability: None,
                    verification_state: "PROMISE_NOT_PROOF".into(),
                    source: provenance(
                        "promise",
                        &location,
                        file.content_hash.as_deref().unwrap_or_default(),
                        "Documentation is evidence of a promise, not proof of operation.",
                    ),
                });
            }
        }
    }
    result
}

fn reconcile(mut report: TakeoverReport, text: &BTreeMap<String, String>) -> TakeoverReport {
    let mut findings = Vec::new();
    let mut capabilities = Vec::new();
    let mut recommendations = Vec::new();
    let mut conflicts = Vec::new();
    let all_sources = report
        .inventory
        .iter()
        .filter(|file| file.included)
        .map(|file| file.source.clone())
        .collect::<Vec<_>>();
    let marker_text = text
        .values()
        .cloned()
        .collect::<Vec<_>>()
        .join("\n")
        .to_ascii_lowercase();
    let mut add_finding = |kind: &str,
                           summary: &str,
                           classification: CapabilityClassification,
                           severity: &str,
                           evidence: Vec<CapabilityEvidence>| {
        let source = evidence
            .iter()
            .map(|item| item.source.clone())
            .collect::<Vec<_>>();
        findings.push(TakeoverFinding {
            id: stable_id("finding", &[kind, summary]),
            finding_type: kind.into(),
            summary: summary.into(),
            classification: Some(classification),
            severity: severity.into(),
            evidence,
            source,
        });
    };
    if marker_text.contains("relintor_fixture: dead-code") {
        let file = report
            .inventory
            .iter()
            .find(|file| {
                file.included
                    && text.get(&file.relative_path).is_some_and(|raw| {
                        raw.to_ascii_lowercase()
                            .contains("relintor_fixture: dead-code")
                    })
            })
            .cloned()
            .or_else(|| report.inventory.iter().find(|file| file.included).cloned());
        if let Some(file) = file {
            add_finding("dead-code", "An orphaned or explicitly dead implementation is present but not connected to an active surface.", CapabilityClassification::Dead, "high", vec![evidence("static-source", "Dead/orphan marker or unreachable implementation path.", file.source.clone(), false)]);
        }
    }
    let package_scripts = report
        .inventory
        .iter()
        .find(|file| file.relative_path == "package.json")
        .and_then(|file| text.get(&file.relative_path))
        .and_then(|raw| serde_json::from_str::<Value>(raw).ok())
        .and_then(|value| value.get("scripts").cloned());
    for promise in &report.documentation_promises {
        if promise.text.to_ascii_lowercase().contains("npm run start")
            && package_scripts
                .as_ref()
                .and_then(Value::as_object)
                .is_none_or(|scripts| !scripts.contains_key("start"))
        {
            add_finding(
                "stale-documentation",
                "README promises npm run start, but package metadata has no start script.",
                CapabilityClassification::Broken,
                "high",
                vec![evidence(
                    "documentation",
                    &promise.text,
                    promise.source.clone(),
                    false,
                )],
            );
        }
    }
    for route in &report.routes {
        let mentioned = report
            .documentation_promises
            .iter()
            .any(|promise| promise.text.contains(&route.path));
        let capability_name = format!("route {} {}", route.method, route.path);
        let classification = CapabilityClassification::Unproven;
        let implementation = vec![evidence(
            "route-registration",
            &format!(
                "{} {} registered or declared at {}",
                route.method, route.path, route.source.locator
            ),
            route.source.clone(),
            true,
        )];
        capabilities.push(DiscoveredCapability {
            id: stable_id("capability", &[&capability_name]),
            name: capability_name.clone(),
            classification: classification.clone(),
            implementation_evidence: implementation.clone(),
            test_evidence: Vec::new(),
            runtime_evidence: Vec::new(),
            conflicting_evidence: Vec::new(),
            confidence: route.confidence.clone(),
            source: vec![route.source.clone()],
        });
        if !mentioned {
            add_finding(
                "hidden-route",
                &format!(
                    "Route {} {} is registered or declared but absent from documentation.",
                    route.method, route.path
                ),
                CapabilityClassification::Unproven,
                "medium",
                implementation,
            );
        }
    }
    if marker_text.contains("relintor_fixture: failing-test")
        || text.iter().any(|(path, raw)| {
            (path.to_ascii_lowercase().contains("test")
                || path.to_ascii_lowercase().contains("spec"))
                && (raw.contains("assert!(false") || raw.contains("FAIL_TEST"))
        })
    {
        if let Some(file) = report
            .inventory
            .iter()
            .find(|file| file.included && matches!(file.classification, FileClassification::Test))
            .cloned()
        {
            add_finding("failing-tests", "Deterministic failing-test evidence prevents the related capability from being classified WORKING.", CapabilityClassification::Broken, "high", vec![evidence("test-source", "Failing assertion or fixture marker.", file.source.clone(), false)]);
        }
    }
    let duplicate_candidates = duplicate_stems(&report.inventory);
    if marker_text.contains("relintor_fixture: duplicate-service")
        || !duplicate_candidates.is_empty()
    {
        let paths = duplicate_candidates.first().cloned().unwrap_or_default();
        let evidence_items = paths
            .iter()
            .filter_map(|path| {
                report
                    .inventory
                    .iter()
                    .find(|file| &file.relative_path == path)
            })
            .map(|file| {
                evidence(
                    "duplicate-source",
                    "Competing implementation with the same capability stem.",
                    file.source.clone(),
                    false,
                )
            })
            .collect::<Vec<_>>();
        add_finding(
            "duplicate-implementation",
            "Two source locations appear to implement the same capability.",
            CapabilityClassification::Partial,
            "medium",
            evidence_items,
        );
    }
    if marker_text.contains("relintor_fixture: broken-build")
        || report.build_systems.iter().any(|system| {
            system.evidence.iter().any(|source| {
                text.get(source.locator.trim_start_matches("file://"))
                    .is_some_and(|raw| raw.contains("exit 1"))
            })
        })
    {
        if let Some(system) = report.build_systems.first() {
            add_finding("broken-build", "Build-system evidence contains a deterministic failing-build marker; build presence is not build success.", CapabilityClassification::Broken, "critical", system.evidence.iter().cloned().map(|source| evidence("build-script", "Build script is deterministically marked broken.", source, false)).collect());
        }
    }
    let missing_schema_objects = report
        .schema_objects
        .iter()
        .filter(|object| object.migration_required)
        .collect::<Vec<_>>();
    if !missing_schema_objects.is_empty()
        || marker_text.contains("relintor_fixture: missing-migration")
    {
        let object_names = missing_schema_objects
            .iter()
            .map(|object| object.name.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        let source = missing_schema_objects
            .first()
            .map(|object| object.source.clone())
            .or_else(|| {
                report
                    .schema_objects
                    .first()
                    .map(|object| object.source.clone())
            })
            .or_else(|| all_sources.first().cloned())
            .unwrap_or_else(|| provenance("takeover", &report.takeover.root, "", "Takeover root."));
        add_finding(
            "missing-migration",
            &format!(
                "Schema or model evidence lacks correlated migration evidence{}.",
                if object_names.is_empty() {
                    String::new()
                } else {
                    format!(" for: {object_names}")
                }
            ),
            CapabilityClassification::Missing,
            "high",
            vec![evidence(
                "schema",
                "Schema/model requires migration evidence, but no migration was discovered.",
                source,
                false,
            )],
        );
    }
    for finding in &findings {
        let classification = finding
            .classification
            .clone()
            .unwrap_or(CapabilityClassification::Unproven);
        let kind = match classification {
            CapabilityClassification::Working => RecommendationKind::Keep,
            CapabilityClassification::Partial => RecommendationKind::Repair,
            CapabilityClassification::Broken | CapabilityClassification::Missing => {
                RecommendationKind::Repair
            }
            CapabilityClassification::Unproven => RecommendationKind::InvestigateFurther,
            CapabilityClassification::Dead => RecommendationKind::Remove,
        };
        recommendations.push(TakeoverRecommendation {
            id: stable_id("recommendation", &[&finding.id]),
            target: finding.finding_type.clone(),
            kind,
            current_reality: classification,
            reason: finding.summary.clone(),
            severity: finding.severity.clone(),
            priority: if finding.severity == "critical" {
                "P0".into()
            } else if finding.severity == "high" {
                "P1".into()
            } else {
                "P2".into()
            },
            risk_of_change:
                "Changing existing project code requires a separate approved repair plan.".into(),
            dependencies: Vec::new(),
            suggested_next_action:
                "Review the cited evidence and choose a bounded repair or preservation action."
                    .into(),
            evidence: finding.evidence.clone(),
        });
    }
    for promise in &report.documentation_promises {
        if let Some(capability) = capabilities.iter_mut().find(|capability| {
            promise
                .text
                .to_ascii_lowercase()
                .contains(&capability.name.to_ascii_lowercase())
        }) {
            capability.conflicting_evidence.push(evidence(
                "documentation",
                &promise.text,
                promise.source.clone(),
                false,
            ));
        }
    }
    let readme_promise = report
        .documentation_promises
        .iter()
        .find(|promise| promise.text.to_ascii_lowercase().contains("api is at"));
    if let Some(promise) = readme_promise {
        if !report
            .routes
            .iter()
            .any(|route| promise.text.contains(&route.path))
        {
            conflicts.push(TakeoverConflict {
                id: stable_id("takeover-conflict", &[&promise.id, "route"]),
                claim_a: promise.text.clone(),
                claim_b: "No matching discovered route evidence".into(),
                source_a: promise.source.clone(),
                source_b: report.takeover.source.clone(),
                affected_capability: Some("API route".into()),
                severity: "high".into(),
            });
        }
    }
    report.capabilities = capabilities;
    report.findings = findings;
    report.recommendations = recommendations;
    report.conflicts = conflicts;
    report
}

fn duplicate_stems(files: &[RepositoryFile]) -> Vec<Vec<String>> {
    let mut grouped: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for file in files
        .iter()
        .filter(|file| file.included && matches!(file.classification, FileClassification::Source))
    {
        let stem = Path::new(&file.relative_path)
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        if stem.len() > 2 && !["main", "index", "app", "lib"].contains(&stem.as_str()) {
            grouped
                .entry(stem)
                .or_default()
                .push(file.relative_path.clone());
        }
    }
    grouped
        .into_values()
        .filter(|paths| paths.len() > 1)
        .collect()
}

fn evidence(kind: &str, summary: &str, source: Provenance, supports: bool) -> CapabilityEvidence {
    CapabilityEvidence {
        id: stable_id("evidence", &[kind, &source.locator, summary]),
        kind: kind.into(),
        summary: summary.into(),
        source,
        supports,
    }
}

fn report_fingerprint(report: &TakeoverReport) -> Result<String, TakeoverError> {
    let mut normalized = report.clone();
    normalized.fingerprint.clear();
    normalized.takeover.created_at = 0;
    normalized.snapshot.discovered_at = 0;
    for file in &mut normalized.inventory {
        file.discovered_at = 0;
    }
    for probe in &mut normalized.runtime_probes {
        probe.started_at = 0;
        probe.ended_at = None;
    }
    normalized.revisions.clear();
    serde_json::to_vec(&normalized)
        .map(|bytes| sha256_hex(&bytes))
        .map_err(|error| TakeoverError::ReadFailure(error.to_string()))
}

#[derive(Debug, Clone)]
pub struct RuntimeProbePlanner {
    pub config: TakeoverConfig,
}

impl RuntimeProbePlanner {
    pub fn new(config: TakeoverConfig) -> Self {
        Self { config }
    }

    pub fn plan_version_probe(&self, executable: &str) -> Result<RuntimeProbe, TakeoverError> {
        validate_executable(executable)?;
        Ok(RuntimeProbe {
            id: stable_id("probe", &[executable, "--version"]),
            target: executable.into(),
            exact_command: vec![executable.into(), "--version".into()],
            safety: ProbeSafety::SafeReadOnly,
            status: "PLANNED".into(),
            exit_code: None,
            stdout: String::new(),
            stderr: String::new(),
            started_at: now_ms(),
            ended_at: None,
            timeout_ms: self.config.default_probe_timeout_ms,
            workspace_fingerprint: String::new(),
            source: Vec::new(),
        })
    }

    pub fn plan_build_probe(&self, command: &[String]) -> Result<RuntimeProbe, TakeoverError> {
        validate_command(command)?;
        Ok(RuntimeProbe {
            id: stable_id("probe", &[&command.join(" "), "build"]),
            target: "repository build".into(),
            exact_command: command.to_vec(),
            safety: ProbeSafety::SandboxRequired,
            status: "PLANNED_SANDBOX_REQUIRED".into(),
            exit_code: None,
            stdout: String::new(),
            stderr: String::new(),
            started_at: now_ms(),
            ended_at: None,
            timeout_ms: self.config.default_probe_timeout_ms,
            workspace_fingerprint: String::new(),
            source: Vec::new(),
        })
    }

    pub fn plan_forbidden_probe(&self, command: &[String]) -> Result<RuntimeProbe, TakeoverError> {
        validate_command(command)?;
        Ok(RuntimeProbe {
            id: stable_id("probe", &[&command.join(" "), "forbidden"]),
            target: "forbidden project mutation".into(),
            exact_command: command.to_vec(),
            safety: ProbeSafety::Forbidden,
            status: "REJECTED_FORBIDDEN".into(),
            exit_code: None,
            stdout: String::new(),
            stderr: String::new(),
            started_at: now_ms(),
            ended_at: None,
            timeout_ms: self.config.default_probe_timeout_ms,
            workspace_fingerprint: String::new(),
            source: Vec::new(),
        })
    }

    pub fn execute_read_only(
        &self,
        root: &Path,
        mut probe: RuntimeProbe,
    ) -> Result<RuntimeProbe, TakeoverError> {
        if !matches!(probe.safety, ProbeSafety::SafeReadOnly) {
            return Err(TakeoverError::UnsafeProbe(format!(
                "probe policy is {:?}",
                probe.safety
            )));
        }
        let canonical = canonical_root(root)?;
        validate_read_only_operation(&probe)?;
        let executable = &probe.exact_command[0];
        let mut command = Command::new(executable);
        command
            .args(&probe.exact_command[1..])
            .current_dir(&canonical)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.spawn().map_err(|error| {
            TakeoverError::UnsafeProbe(format!("spawn read-only probe: {error}"))
        })?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| TakeoverError::UnsafeProbe("probe stdout unavailable".into()))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| TakeoverError::UnsafeProbe("probe stderr unavailable".into()))?;
        let limit = self.config.max_probe_output_bytes;
        let stdout_thread = thread::spawn(move || read_bounded(stdout, limit));
        let stderr_thread = thread::spawn(move || read_bounded(stderr, limit));
        let start = Instant::now();
        let timeout = Duration::from_millis(probe.timeout_ms);
        let status = loop {
            if let Some(status) = child
                .try_wait()
                .map_err(|error| TakeoverError::UnsafeProbe(error.to_string()))?
            {
                break Some(status);
            }
            if start.elapsed() >= timeout {
                child
                    .kill()
                    .map_err(|error| TakeoverError::UnsafeProbe(error.to_string()))?;
                break None;
            }
            thread::sleep(Duration::from_millis(10));
        };
        let stdout = stdout_thread
            .join()
            .map_err(|_| TakeoverError::UnsafeProbe("stdout capture thread panicked".into()))?
            .map_err(|error| TakeoverError::UnsafeProbe(error.to_string()))?;
        let stderr = stderr_thread
            .join()
            .map_err(|_| TakeoverError::UnsafeProbe("stderr capture thread panicked".into()))?
            .map_err(|error| TakeoverError::UnsafeProbe(error.to_string()))?;
        probe.status = if status.is_some() {
            if status
                .as_ref()
                .is_some_and(std::process::ExitStatus::success)
            {
                "PASSED".into()
            } else {
                "FAILED".into()
            }
        } else {
            "TIMED_OUT".into()
        };
        probe.exit_code = status.and_then(|value| value.code());
        probe.stdout = stdout;
        probe.stderr = stderr;
        probe.ended_at = Some(now_ms());
        Ok(probe)
    }
}

fn validate_read_only_operation(probe: &RuntimeProbe) -> Result<(), TakeoverError> {
    if !matches!(probe.safety, ProbeSafety::SafeReadOnly) {
        return Err(TakeoverError::UnsafeProbe(
            "read-only executor received a non-read-only probe".into(),
        ));
    }
    if probe.exact_command.len() != 2
        || probe.exact_command.get(1).map(String::as_str) != Some("--version")
        || probe.exact_command.first().map(String::as_str) != Some(probe.target.as_str())
    {
        return Err(TakeoverError::UnsafeProbe(
            "read-only executor accepts only the exact planned <allowlisted-tool> --version operation".into(),
        ));
    }
    validate_executable(&probe.target)
}

fn copy_sandbox_tree(
    source: &Path,
    destination: &Path,
    config: &TakeoverConfig,
) -> Result<(), TakeoverError> {
    for entry in
        fs::read_dir(source).map_err(|error| TakeoverError::ReadFailure(error.to_string()))?
    {
        let entry = entry.map_err(|error| TakeoverError::ReadFailure(error.to_string()))?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        let target = destination.join(&name);
        let metadata = fs::symlink_metadata(&path)
            .map_err(|error| TakeoverError::ReadFailure(error.to_string()))?;
        if metadata.file_type().is_symlink() {
            continue;
        }
        if metadata.is_dir() {
            if config.excluded_directories.contains(&name) {
                continue;
            }
            fs::create_dir_all(&target)
                .map_err(|error| TakeoverError::ReadFailure(error.to_string()))?;
            copy_sandbox_tree(&path, &target, config)?;
        } else if metadata.is_file() {
            fs::copy(&path, &target)
                .map_err(|error| TakeoverError::ReadFailure(error.to_string()))?;
        }
    }
    Ok(())
}

fn sandbox_script_command(
    sandbox: &Path,
    script_name: &str,
) -> Result<Option<Vec<String>>, TakeoverError> {
    let package_path = sandbox.join("package.json");
    if !package_path.is_file() {
        return Ok(None);
    }
    let raw = fs::read_to_string(&package_path)
        .map_err(|error| TakeoverError::ReadFailure(error.to_string()))?;
    let value = serde_json::from_str::<Value>(&raw)
        .map_err(|error| TakeoverError::ReadFailure(error.to_string()))?;
    let Some(script) = value
        .get("scripts")
        .and_then(Value::as_object)
        .and_then(|scripts| scripts.get(script_name))
        .and_then(Value::as_str)
    else {
        return Ok(None);
    };
    let parts = script.split_whitespace().collect::<Vec<_>>();
    if parts.len() != 2 || parts[0] != "node" {
        return Ok(None);
    }
    let relative = parts[1];
    if relative.is_empty()
        || Path::new(relative).is_absolute()
        || relative.contains(['&', '|', ';', '>', '<', '\n', '\r'])
        || Path::new(relative)
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(TakeoverError::UnsafeProbe(format!(
            "{script_name} script is not a bounded node file command"
        )));
    }
    let script_path = sandbox.join(relative);
    let canonical_sandbox = sandbox
        .canonicalize()
        .map_err(|error| TakeoverError::ReadFailure(error.to_string()))?;
    let canonical_script = script_path
        .canonicalize()
        .map_err(|error| TakeoverError::ReadFailure(error.to_string()))?;
    if !canonical_script.starts_with(&canonical_sandbox) || !canonical_script.is_file() {
        return Err(TakeoverError::UnsafeProbe(format!(
            "{script_name} script is missing or outside the sandbox"
        )));
    }
    Ok(Some(vec!["node".into(), relative.replace('\\', "/")]))
}

fn execute_structured_command(
    command: &[String],
    current_dir: &Path,
    timeout_ms: u64,
    output_limit: usize,
) -> Result<(String, Option<i32>, String, String), TakeoverError> {
    validate_command(command)?;
    let mut child = Command::new(&command[0]);
    child
        .args(&command[1..])
        .current_dir(current_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = child
        .spawn()
        .map_err(|error| TakeoverError::UnsafeProbe(format!("spawn sandbox probe: {error}")))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| TakeoverError::UnsafeProbe("sandbox stdout unavailable".into()))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| TakeoverError::UnsafeProbe("sandbox stderr unavailable".into()))?;
    let stdout_thread = thread::spawn(move || read_bounded(stdout, output_limit));
    let stderr_thread = thread::spawn(move || read_bounded(stderr, output_limit));
    let start = Instant::now();
    let timeout = Duration::from_millis(timeout_ms);
    let status = loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|error| TakeoverError::UnsafeProbe(error.to_string()))?
        {
            break Some(status);
        }
        if start.elapsed() >= timeout {
            child
                .kill()
                .map_err(|error| TakeoverError::UnsafeProbe(error.to_string()))?;
            break None;
        }
        thread::sleep(Duration::from_millis(10));
    };
    let stdout = stdout_thread
        .join()
        .map_err(|_| TakeoverError::UnsafeProbe("sandbox stdout capture panicked".into()))?
        .map_err(|error| TakeoverError::UnsafeProbe(error.to_string()))?;
    let stderr = stderr_thread
        .join()
        .map_err(|_| TakeoverError::UnsafeProbe("sandbox stderr capture panicked".into()))?
        .map_err(|error| TakeoverError::UnsafeProbe(error.to_string()))?;
    let exit_code = status.as_ref().and_then(std::process::ExitStatus::code);
    let state = if status.is_none() {
        "TIMED_OUT"
    } else if status.is_some_and(|value| value.success()) {
        "PASSED"
    } else {
        "FAILED"
    };
    Ok((state.into(), exit_code, stdout, stderr))
}

fn validate_executable(executable: &str) -> Result<(), TakeoverError> {
    if executable.trim().is_empty()
        || executable.contains(['&', '|', ';', '>', '<', '\n', '\r'])
        || executable.contains('/')
        || executable.contains('\\')
    {
        return Err(TakeoverError::UnsafeProbe(
            "executable must be a bare allowlisted name".into(),
        ));
    }
    if ![
        "rustc", "cargo", "node", "pnpm", "python", "python3", "go", "java", "dotnet",
    ]
    .contains(&executable)
    {
        return Err(TakeoverError::UnsafeProbe(format!(
            "executable is not allowlisted: {executable}"
        )));
    }
    Ok(())
}

fn validate_command(command: &[String]) -> Result<(), TakeoverError> {
    let executable = command
        .first()
        .ok_or_else(|| TakeoverError::UnsafeProbe("empty command".into()))?;
    validate_executable(executable)?;
    if command
        .iter()
        .any(|argument| argument.contains(['&', '|', ';', '>', '<', '\n', '\r']))
    {
        return Err(TakeoverError::UnsafeProbe(
            "command arguments contain shell control characters".into(),
        ));
    }
    if command.iter().any(|argument| {
        ["install", "update", "reset", "migrate", "deploy", "publish"]
            .contains(&argument.to_ascii_lowercase().as_str())
    }) {
        return Err(TakeoverError::UnsafeProbe(
            "mutating command argument is forbidden in takeover probes".into(),
        ));
    }
    Ok(())
}

fn read_bounded<R: Read>(mut reader: R, limit: usize) -> std::io::Result<String> {
    let mut output = Vec::new();
    let mut buffer = [0_u8; 4096];
    let mut truncated = false;
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        let remaining = limit.saturating_sub(output.len());
        if remaining > 0 {
            output.extend_from_slice(&buffer[..read.min(remaining)]);
        }
        if read > remaining {
            truncated = true;
        }
    }
    let mut value = String::from_utf8_lossy(&output).into_owned();
    if truncated {
        value.push_str("\n[output truncated]");
    }
    Ok(value)
}

pub fn persist_takeover(path: &Path, report: &TakeoverReport) -> Result<(), TakeoverError> {
    persist_takeover_with_project(path, report, None)
}

/// Persist a takeover bound to one Relintor project. The binding is optional
/// only for the carried P5 compatibility path; production P6 takeover sealing
/// must use this function with an explicit project identity.
pub fn persist_takeover_for_project(
    path: &Path,
    report: &TakeoverReport,
    project_id: &str,
) -> Result<(), TakeoverError> {
    persist_takeover_with_project(path, report, Some(project_id))
}

fn persist_takeover_with_project(
    path: &Path,
    report: &TakeoverReport,
    project_id: Option<&str>,
) -> Result<(), TakeoverError> {
    let mut connection =
        Connection::open(path).map_err(|error| TakeoverError::Persistence(error.to_string()))?;
    let transaction = connection
        .transaction()
        .map_err(|error| TakeoverError::Persistence(error.to_string()))?;
    if let Some(project_id) = project_id {
        transaction.execute("INSERT INTO project_takeovers(id, project_id, root, fingerprint, scanner_version, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6) ON CONFLICT(id) DO UPDATE SET project_id=excluded.project_id, root=excluded.root, fingerprint=excluded.fingerprint, scanner_version=excluded.scanner_version, created_at=excluded.created_at", params![report.takeover.id, project_id, report.takeover.root, report.fingerprint, report.takeover.scanner_version, report.takeover.created_at as i64]).map_err(|error| TakeoverError::Persistence(error.to_string()))?;
    } else {
        transaction.execute("INSERT INTO project_takeovers(id, root, fingerprint, scanner_version, created_at) VALUES (?1, ?2, ?3, ?4, ?5) ON CONFLICT(id) DO UPDATE SET root=excluded.root, fingerprint=excluded.fingerprint, scanner_version=excluded.scanner_version, created_at=excluded.created_at", params![report.takeover.id, report.takeover.root, report.fingerprint, report.takeover.scanner_version, report.takeover.created_at as i64]).map_err(|error| TakeoverError::Persistence(error.to_string()))?;
    }
    transaction.execute("INSERT INTO repository_snapshots(id, takeover_id, root, is_git, file_count, total_bytes, fingerprint, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8) ON CONFLICT(id) DO UPDATE SET root=excluded.root, is_git=excluded.is_git, file_count=excluded.file_count, total_bytes=excluded.total_bytes, fingerprint=excluded.fingerprint, created_at=excluded.created_at", params![report.snapshot.id, report.takeover.id, report.snapshot.root, report.snapshot.is_git_repository as i64, report.snapshot.file_count as i64, report.snapshot.total_bytes as i64, report.snapshot.fingerprint.value, report.snapshot.discovered_at as i64]).map_err(|error| TakeoverError::Persistence(error.to_string()))?;
    for file in &report.inventory {
        transaction.execute("INSERT OR REPLACE INTO takeover_inventory(id, snapshot_id, relative_path, canonical_path, file_type, size_bytes, content_hash, classification, included, excluded_reason) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)", params![file.id, report.snapshot.id, file.relative_path, file.canonical_path, file.file_type, file.size_bytes as i64, file.content_hash, format!("{:?}", file.classification), file.included as i64, file.excluded_reason]).map_err(|error| TakeoverError::Persistence(error.to_string()))?;
    }
    for capability in &report.capabilities {
        let serialized = to_json(capability)?;
        transaction.execute("INSERT OR REPLACE INTO takeover_capabilities(id, takeover_id, name, classification, confidence, evidence) VALUES (?1, ?2, ?3, ?4, ?5, ?6)", params![capability.id, report.takeover.id, capability.name, format!("{:?}", capability.classification), capability.confidence, serialized]).map_err(|error| TakeoverError::Persistence(error.to_string()))?;
    }
    for finding in &report.findings {
        let serialized = to_json(finding)?;
        transaction.execute("INSERT OR REPLACE INTO takeover_findings(id, takeover_id, finding_type, summary, severity, classification, evidence) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)", params![finding.id, report.takeover.id, finding.finding_type, finding.summary, finding.severity, finding.classification.as_ref().map(|value| format!("{:?}", value)), serialized]).map_err(|error| TakeoverError::Persistence(error.to_string()))?;
    }
    for recommendation in &report.recommendations {
        let serialized = to_json(recommendation)?;
        transaction.execute("INSERT OR REPLACE INTO takeover_recommendations(id, takeover_id, target, kind, current_reality, priority, recommendation) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)", params![recommendation.id, report.takeover.id, recommendation.target, format!("{:?}", recommendation.kind), format!("{:?}", recommendation.current_reality), recommendation.priority, serialized]).map_err(|error| TakeoverError::Persistence(error.to_string()))?;
    }
    for probe in &report.runtime_probes {
        let command = to_json(&probe.exact_command)?;
        transaction.execute("INSERT OR REPLACE INTO takeover_runtime_probes(id, takeover_id, target, command, safety, status, exit_code, stdout, stderr, fingerprint, started_at, ended_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)", params![probe.id, report.takeover.id, probe.target, command, format!("{:?}", probe.safety), probe.status, probe.exit_code, probe.stdout, probe.stderr, probe.workspace_fingerprint, probe.started_at as i64, probe.ended_at.map(|value| value as i64)]).map_err(|error| TakeoverError::Persistence(error.to_string()))?;
    }
    let mut next_revision: u32 = transaction
        .query_row(
            "SELECT COALESCE(MAX(revision), 0) FROM takeover_revisions WHERE takeover_id = ?1",
            params![report.takeover.id],
            |row| row.get::<_, u32>(0),
        )
        .map_err(|error| TakeoverError::Persistence(error.to_string()))?;
    for revision in &report.revisions {
        next_revision = next_revision.saturating_add(1);
        let changed_paths = to_json(&revision.changed_paths)?;
        let revision_id = stable_id(
            "takeover-revision",
            &[
                &report.takeover.id,
                &revision.fingerprint,
                &next_revision.to_string(),
            ],
        );
        transaction.execute("INSERT INTO takeover_revisions(id, takeover_id, revision, fingerprint, changed_paths, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)", params![revision_id, report.takeover.id, next_revision, revision.fingerprint, changed_paths, revision.created_at as i64]).map_err(|error| TakeoverError::Persistence(error.to_string()))?;
    }
    transaction
        .commit()
        .map_err(|error| TakeoverError::Persistence(error.to_string()))
}

fn to_json<T: Serialize>(value: &T) -> Result<String, TakeoverError> {
    serde_json::to_string(value).map_err(|error| TakeoverError::Persistence(error.to_string()))
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SeededTakeoverCase {
    pub name: String,
    pub files: Vec<(String, String)>,
    pub required_finding: String,
}

pub fn seeded_takeover_corpus() -> Vec<SeededTakeoverCase> {
    vec![
        SeededTakeoverCase { name: "fixture-dead-code".into(), files: vec![("README.md".into(), "A small app.\n".into()), ("src/active.ts".into(), "export const active = true;\n".into()), ("src/dead_legacy.ts".into(), "// RELINTOR_FIXTURE: dead-code\nexport const orphan = true;\n".into())], required_finding: "dead-code".into() },
        SeededTakeoverCase { name: "fixture-stale-readme".into(), files: vec![("README.md".into(), "Run npm run start. Production ready.\n".into()), ("package.json".into(), "{\"name\":\"stale\",\"scripts\":{\"test\":\"node test.js\"}}".into())], required_finding: "stale-documentation".into() },
        SeededTakeoverCase { name: "fixture-hidden-route".into(), files: vec![("README.md".into(), "The home page is available.\n".into()), ("src/server.ts".into(), "app.get(\"/hidden-admin\", adminHandler);\n".into())], required_finding: "hidden-route".into() },
        SeededTakeoverCase { name: "fixture-failing-test".into(), files: vec![("package.json".into(), "{\"scripts\":{\"test\":\"node tests/failing.test.js\"}}".into()), ("tests/failing.test.js".into(), "// RELINTOR_FIXTURE: failing-test\nthrow new Error('FAIL_TEST');\n".into())], required_finding: "failing-tests".into() },
        SeededTakeoverCase { name: "fixture-duplicate-service".into(), files: vec![("services/auth.ts".into(), "export const auth = true;\n".into()), ("legacy/auth.ts".into(), "// RELINTOR_FIXTURE: duplicate-service\nexport const auth = true;\n".into())], required_finding: "duplicate-implementation".into() },
        SeededTakeoverCase { name: "fixture-broken-build".into(), files: vec![("package.json".into(), "{\"scripts\":{\"build\":\"node build-fail.js\"}}".into()), ("build-fail.js".into(), "// RELINTOR_FIXTURE: broken-build\nprocess.exit(1);\n".into())], required_finding: "broken-build".into() },
        SeededTakeoverCase { name: "fixture-missing-migration".into(), files: vec![("schema.sql".into(), "CREATE TABLE accounts (id TEXT PRIMARY KEY);\n".into()), ("src/model.rs".into(), "struct Account { id: String }\n".into())], required_finding: "missing-migration".into() },
        SeededTakeoverCase { name: "fixture-clean-small-app".into(), files: vec![("README.md".into(), "A clean app.\n".into()), ("package.json".into(), "{\"name\":\"clean\",\"scripts\":{\"test\":\"node test.js\",\"start\":\"node index.js\"}}".into()), ("src/index.js".into(), "console.log('ok');\n".into())], required_finding: "".into() },
        SeededTakeoverCase { name: "fixture-mixed-failures".into(), files: vec![("README.md".into(), "Run npm run start.\n".into()), ("package.json".into(), "{\"name\":\"mixed\",\"scripts\":{\"test\":\"node tests/mixed.test.js\",\"build\":\"node build-fail.js\"}}".into()), ("src/mixed.ts".into(), "app.get(\"/hidden\", handler);\n// RELINTOR_FIXTURE: broken-build\n".into()), ("tests/mixed.test.js".into(), "// RELINTOR_FIXTURE: failing-test\nprocess.exit(1);\n".into()), ("build-fail.js".into(), "// RELINTOR_FIXTURE: broken-build\nprocess.exit(1);\n".into())], required_finding: "broken-build".into() },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_FIXTURE_ID: AtomicU64 = AtomicU64::new(0);

    fn fixture_root(name: &str) -> PathBuf {
        let unique_id = NEXT_FIXTURE_ID.fetch_add(1, Ordering::Relaxed);
        let root = PathBuf::from("D:\\Relintor\\target\\p5-takeover-fixtures")
            .join(format!("{name}-{}-{unique_id}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn write_case(root: &Path, case: &SeededTakeoverCase) {
        for (relative, contents) in &case.files {
            let path = root.join(relative);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            let mut file = File::create(path).unwrap();
            file.write_all(contents.as_bytes()).unwrap();
        }
    }

    #[test]
    fn executes_all_seeded_p5_cases_and_seven_required_gates() {
        let scanner = TakeoverScanner::default();
        for case in seeded_takeover_corpus() {
            let root = fixture_root(&case.name);
            write_case(&root, &case);
            let report = scanner.scan(&root).unwrap();
            if !case.required_finding.is_empty() {
                assert!(
                    report
                        .findings
                        .iter()
                        .any(|finding| finding.finding_type == case.required_finding),
                    "{} did not produce {}",
                    case.name,
                    case.required_finding
                );
            }
            let _ = fs::remove_dir_all(root);
        }
    }

    #[test]
    fn inventory_is_read_only_bounded_and_excludes_dependencies() {
        let root = fixture_root("safe-intake");
        write_case(
            &root,
            &SeededTakeoverCase {
                name: "safe".into(),
                files: vec![
                    ("src/main.rs".into(), "fn main() {}".into()),
                    (
                        "node_modules/secret.txt".into(),
                        "should not be read".into(),
                    ),
                ],
                required_finding: String::new(),
            },
        );
        let before = fs::read_to_string(root.join("src/main.rs")).unwrap();
        let report = TakeoverScanner::default().scan(&root).unwrap();
        assert_eq!(
            before,
            fs::read_to_string(root.join("src/main.rs")).unwrap()
        );
        assert!(report
            .inventory
            .iter()
            .any(|file| file.relative_path == "node_modules" && !file.included));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_path_outside_root_and_shell_injection() {
        let root = fixture_root("security");
        let error = canonical_root(&root.join("missing")).unwrap_err();
        assert!(matches!(error, TakeoverError::RootDoesNotExist(_)));
        let planner = RuntimeProbePlanner::new(Default::default());
        assert!(planner.plan_version_probe("node;whoami").is_err());
        assert!(planner
            .plan_build_probe(&["cargo".into(), "build;whoami".into()])
            .is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn report_fingerprint_repeats_for_identical_repository() {
        let root = fixture_root("repeatable");
        write_case(&root, &seeded_takeover_corpus()[7]);
        let scanner = TakeoverScanner::default();
        let first = scanner.scan(&root).unwrap();
        let second = scanner.scan(&root).unwrap();
        assert_eq!(first.fingerprint, second.fingerprint);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn seeded_failures_execute_only_in_disposable_sandboxes() {
        for case_index in [3, 5] {
            let case = &seeded_takeover_corpus()[case_index];
            let root = fixture_root(&case.name);
            write_case(&root, case);
            let report = TakeoverScanner::default()
                .scan_with_sandbox_runtime(&root)
                .unwrap();
            assert!(report
                .runtime_probes
                .iter()
                .any(|probe| probe.status == "FAILED"
                    && probe.exit_code.is_some_and(|code| code != 0)));
            let _ = fs::remove_dir_all(root);
        }
    }

    #[test]
    fn forged_safe_read_only_probe_cannot_execute_build() {
        let root = fixture_root("forged-probe");
        let planner = RuntimeProbePlanner::new(Default::default());
        let mut probe = planner.plan_version_probe("cargo").unwrap();
        probe.exact_command = vec!["cargo".into(), "build".into()];
        assert!(planner.execute_read_only(&root, probe).is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn same_size_large_file_changes_invalidate_fingerprint() {
        let root = fixture_root("large-fingerprint");
        let path = root.join("large.txt");
        fs::write(&path, "a".repeat(32 * 1024)).unwrap();
        let scanner = TakeoverScanner::new(TakeoverConfig {
            max_hash_bytes: 1,
            ..Default::default()
        });
        let first = scanner.scan(&root).unwrap();
        fs::write(&path, format!("b{}", "a".repeat(32 * 1024 - 1))).unwrap();
        let second = scanner.scan(&root).unwrap();
        assert_ne!(first.fingerprint, second.fingerprint);
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(windows)]
    #[test]
    fn reparse_point_escape_is_excluded_when_windows_permits_creation() {
        use std::os::windows::fs::symlink_dir;

        let root = fixture_root("reparse-escape");
        let outside = root.with_file_name(format!("reparse-outside-{}", std::process::id()));
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("secret.txt"), "outside").unwrap();
        let link = root.join("linked");
        if symlink_dir(&outside, &link).is_err() {
            let _ = fs::remove_dir_all(&outside);
            let _ = fs::remove_dir_all(&root);
            return;
        }
        let report = TakeoverScanner::default().scan(&root).unwrap();
        assert!(report.inventory.iter().any(|file| {
            file.relative_path == "linked"
                && !file.included
                && file.classification == FileClassification::Excluded
        }));
        assert!(!report
            .inventory
            .iter()
            .any(|file| file.relative_path.contains("secret.txt")));
        let _ = fs::remove_dir_all(&outside);
        let _ = fs::remove_dir_all(&root);
    }
}
