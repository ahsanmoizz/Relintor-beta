//! Universal Duplication Policy — Product-Wide Authority and Invariants.
//!
//! Relintor guarantees explicit duplication semantics anywhere duplication can
//! affect authority, execution, evidence, discovery, or the governed project:
//! - Class A: Relintor Authority Artifacts (resolve via authenticated lineage/index; fail closed if ambiguous; preserve non-authoritative candidates)
//! - Class B: Duplicate Execution / Retry / Lease (exactly-once authority; redundant triggers are no-ops / consumed)
//! - Class C: Duplicate Requirements / Tasks (semantic identity + provenance preservation; distinct obligations remain separate)
//! - Class D: Duplicate Evidence (counted once; cross-requirement support requires explicit applicability semantics)
//! - Class E: Duplicate Project Files / Implementations (detect & classify without destructive deletion; report with provenance)
//! - Class F: Duplicate Discovery Generations (canonical identity bound to workspace + fingerprint + version + generation; stale generations sealed historical)
//! - Class G: Duplicate Cache / Derived State (caches are never authority; recompute from canonical authority)

use crate::{Requirement, RequirementSource};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

fn sha256_hex(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    format!("{:x}", hasher.finalize())
}

/// Canonical classification of duplication across Relintor's domain boundaries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DuplicateClass {
    /// Class A: Checkpoints, recovery journals, ledgers, leases, certificates, human decisions.
    AuthorityArtifacts,
    /// Class B: Concurrent auto/manual task dispatches, retries, lease acquisitions.
    ExecutionAndLeases,
    /// Class C: Discovery requirements and task obligations observed from multiple sources.
    RequirementsAndTasks,
    /// Class D: Raw evidence artifacts, repeated observations, cross-requirement claims.
    EvidenceArtifacts,
    /// Class E: Governed project source files, vendor trees, generated files, duplicate modules.
    ProjectFilesAndImplementations,
    /// Class F: Historical discovery scans and outdated scanner generation findings.
    DiscoveryGenerations,
    /// Class G: Status caches, derived indexes, memory projections.
    DerivedCaches,
}

/// Classification of duplicate files observed in the governed workspace (Duplicate Class E).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ProjectDuplicateClass {
    /// Legitimate intentional duplication (e.g. minimal re-exports, multi-target stubs, identical boilerplate configs).
    LegitimateDuplicate,
    /// Generated artifacts (e.g. code generator outputs, protobufs, compiler outputs with @generated markers).
    GeneratedDuplicate,
    /// Vendored third-party code (e.g. node_modules, vendor/, third_party/ dependencies).
    VendoredDuplicate,
    /// Mirror or transient cache directories (e.g. target/, dist/, .cache/, build/).
    MirrorCache,
    /// Suspicious duplicate business logic in application source needing developer review.
    SuspiciousDuplicateImplementation,
    /// Actual defect causing duplicate symbol collision, diverging bugfixes, or violation of single source of truth.
    ActualDefect,
}

/// A classified finding of duplication in the governed project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectDuplicateFinding {
    pub primary_path: String,
    pub duplicate_path: String,
    pub content_hash: String,
    pub classification: ProjectDuplicateClass,
    pub provenance_notes: String,
    pub requires_executor_remediation: bool,
}

/// Non-destructive classifier for duplicate files in the governed project (Duplicate Class E).
pub struct ProjectDuplicateClassifier;

impl ProjectDuplicateClassifier {
    /// Detects and classifies duplicates without destructive file operations.
    pub fn classify(
        primary_path: &str,
        duplicate_path: &str,
        content_hash: &str,
        file_content: Option<&str>,
    ) -> ProjectDuplicateFinding {
        let norm_dup = duplicate_path.replace('\\', "/").to_lowercase();
        let norm_prim = primary_path.replace('\\', "/").to_lowercase();

        // 1. Vendor directories
        if norm_dup.contains("/node_modules/")
            || norm_dup.starts_with("node_modules/")
            || norm_dup.contains("/vendor/")
            || norm_dup.starts_with("vendor/")
            || norm_dup.contains("/third_party/")
            || norm_dup.starts_with("third_party/")
            || norm_dup.contains("/extern/")
        {
            return ProjectDuplicateFinding {
                primary_path: primary_path.into(),
                duplicate_path: duplicate_path.into(),
                content_hash: content_hash.into(),
                classification: ProjectDuplicateClass::VendoredDuplicate,
                provenance_notes: "Duplicate resides in third-party or vendored dependency directory".into(),
                requires_executor_remediation: false,
            };
        }

        // 2. Mirror/Cache/Build artifacts
        if norm_dup.contains("/target/")
            || norm_dup.starts_with("target/")
            || norm_dup.contains("/dist/")
            || norm_dup.starts_with("dist/")
            || norm_dup.contains("/build/")
            || norm_dup.starts_with("build/")
            || norm_dup.contains("/.cache/")
            || norm_dup.starts_with(".cache/")
            || norm_dup.ends_with(".bak")
            || norm_dup.ends_with(".tmp")
        {
            return ProjectDuplicateFinding {
                primary_path: primary_path.into(),
                duplicate_path: duplicate_path.into(),
                content_hash: content_hash.into(),
                classification: ProjectDuplicateClass::MirrorCache,
                provenance_notes: "Duplicate resides in build output, package distribution, or transient cache".into(),
                requires_executor_remediation: false,
            };
        }

        // 3. Generated files
        let has_generated_header = file_content.is_some_and(|text| {
            text.contains("@generated")
                || text.contains("DO NOT EDIT")
                || text.contains("Code generated by")
                || text.contains("This file is automatically generated")
        });
        if has_generated_header
            || norm_dup.contains("/gen/")
            || norm_dup.contains("/generated/")
            || norm_dup.ends_with(".g.dart")
            || norm_dup.ends_with(".pb.go")
        {
            return ProjectDuplicateFinding {
                primary_path: primary_path.into(),
                duplicate_path: duplicate_path.into(),
                content_hash: content_hash.into(),
                classification: ProjectDuplicateClass::GeneratedDuplicate,
                provenance_notes: "Duplicate is machine-generated code from generator source".into(),
                requires_executor_remediation: false,
            };
        }

        // 4. Legitimate boilerplate (e.g. short barrel re-exports like index.ts or minimal README/Cargo.toml stubs)
        let is_minimal_boilerplate = file_content.is_some_and(|text| {
            text.trim().len() < 120
                && (text.contains("export *")
                    || text.contains("pub mod")
                    || text.contains("# ")
                    || text.trim().is_empty())
        });
        if is_minimal_boilerplate {
            return ProjectDuplicateFinding {
                primary_path: primary_path.into(),
                duplicate_path: duplicate_path.into(),
                content_hash: content_hash.into(),
                classification: ProjectDuplicateClass::LegitimateDuplicate,
                provenance_notes: "Duplicate is legitimate minimal barrel export or boilerplate stub".into(),
                requires_executor_remediation: false,
            };
        }

        // 5. Duplicate implementation in source tree
        if norm_prim.starts_with("src/") && norm_dup.starts_with("src/") {
            // If paths indicate intentional copy/backup or divergence in active code
            if norm_dup.contains("_copy") || norm_dup.contains("_old") || norm_dup.contains("/duplicate") {
                return ProjectDuplicateFinding {
                    primary_path: primary_path.into(),
                    duplicate_path: duplicate_path.into(),
                    content_hash: content_hash.into(),
                    classification: ProjectDuplicateClass::ActualDefect,
                    provenance_notes: "Orphaned or divergent duplicate copy detected in active source tree".into(),
                    requires_executor_remediation: true,
                };
            }
            return ProjectDuplicateFinding {
                primary_path: primary_path.into(),
                duplicate_path: duplicate_path.into(),
                content_hash: content_hash.into(),
                classification: ProjectDuplicateClass::SuspiciousDuplicateImplementation,
                provenance_notes: "Duplicate source implementation detected across modules in source tree".into(),
                requires_executor_remediation: false,
            };
        }

        ProjectDuplicateFinding {
            primary_path: primary_path.into(),
            duplicate_path: duplicate_path.into(),
            content_hash: content_hash.into(),
            classification: ProjectDuplicateClass::SuspiciousDuplicateImplementation,
            provenance_notes: "Identical content detected across project surfaces".into(),
            requires_executor_remediation: false,
        }
    }
}

/// Canonical identity for discovery generations (Duplicate Class F).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveryGenerationIdentity {
    pub workspace: String,
    pub source_fingerprint: String,
    pub scanner_version: String,
    pub generation: u64,
}

impl DiscoveryGenerationIdentity {
    pub fn generation_digest(&self) -> String {
        sha256_hex(
            format!(
                "{}:{}:{}:{}",
                self.workspace, self.source_fingerprint, self.scanner_version, self.generation
            )
            .as_bytes(),
        )
    }
}

/// Discovery Generation Policy ensuring old scanner generations never contaminate current generation.
pub struct DiscoveryGenerationPolicy;

impl DiscoveryGenerationPolicy {
    /// Validates whether a candidate finding belongs to the canonical active generation.
    pub fn is_current_generation(
        current: &DiscoveryGenerationIdentity,
        candidate: &DiscoveryGenerationIdentity,
    ) -> bool {
        current.workspace == candidate.workspace
            && current.source_fingerprint == candidate.source_fingerprint
            && current.scanner_version == candidate.scanner_version
            && current.generation == candidate.generation
    }

    /// Filters findings to strictly retain the active canonical generation while sealing older ones as historical.
    pub fn select_active_findings<'a, T>(
        current: &DiscoveryGenerationIdentity,
        findings: &'a [(DiscoveryGenerationIdentity, T)],
    ) -> (Vec<&'a T>, Vec<&'a T>) {
        let mut active = Vec::new();
        let mut historical = Vec::new();
        for (id, item) in findings {
            if Self::is_current_generation(current, id) {
                active.push(item);
            } else {
                historical.push(item);
            }
        }
        (active, historical)
    }
}

/// Deduplicator for discovery requirements (Duplicate Class C).
pub struct RequirementDeduplicator;

fn source_reference_uri(source: &RequirementSource) -> String {
    match source {
        RequirementSource::User { reference } => format!("user://{reference}"),
        RequirementSource::Document { reference } => format!("document://{reference}"),
        RequirementSource::Blueprint { reference } => format!("blueprint://{reference}"),
        RequirementSource::TakeoverFinding { reference } => format!("takeover://{reference}"),
        RequirementSource::Standard { pack_id, rule_id, .. } => format!("standards://{pack_id}/{rule_id}"),
        RequirementSource::Inference { reference } => format!("inference://{reference}"),
        RequirementSource::ExplicitDecision { reference } => format!("decision://{reference}"),
    }
}

impl RequirementDeduplicator {
    /// Deduplicates discovery requirements based on semantic identity, origin rule, and applicable surface.
    /// Preserves provenance from all discovery sources while preventing duplicate task generation.
    pub fn deduplicate(requirements: &[Requirement]) -> Vec<Requirement> {
        let mut canonical_map: BTreeMap<String, Requirement> = BTreeMap::new();

        for req in requirements {
            // Semantic key incorporates origin rule ID (if present) or requirement ID plus applicability target.
            let semantic_key = if let Some(origin) = &req.origin_rule_id {
                format!("rule:{}", origin)
            } else {
                format!("semantic:{}:{}", req.requirement_type, req.title.trim().to_lowercase())
            };

            let source_ref = source_reference_uri(&req.source);

            if let Some(existing) = canonical_map.get_mut(&semantic_key) {
                // Same authoritative obligation from multiple sources: merge provenance into implementation_links
                if !existing.implementation_links.contains(&source_ref) {
                    existing.implementation_links.push(source_ref);
                }
                // Merge acceptance criteria if missing
                for crit in &req.acceptance_criteria {
                    if !existing.acceptance_criteria.iter().any(|c| c.criterion_id == crit.criterion_id) {
                        existing.acceptance_criteria.push(crit.clone());
                    }
                }
            } else {
                let mut canon = req.clone();
                if !canon.implementation_links.contains(&source_ref) {
                    canon.implementation_links.push(source_ref);
                }
                canonical_map.insert(semantic_key, canon);
            }
        }

        canonical_map.into_values().collect()
    }
}

/// Policy governing Cache vs Authority conflict resolution (Duplicate Class G).
pub struct CacheAuthorityPolicy;

impl CacheAuthorityPolicy {
    /// Asserts that persisted authenticated authority strictly supersedes any derived cache.
    /// If cache disagrees with canonical authority, the cache must be discarded and recomputed.
    pub fn resolve<T: PartialEq + Clone>(
        canonical_authority: &T,
        cached_entry: Option<&T>,
    ) -> (T, bool) {
        match cached_entry {
            Some(cached) if cached == canonical_authority => {
                (canonical_authority.clone(), false) // Cache hit, valid
            }
            Some(_) => {
                // Cache diverged from authority: invalidate cache and recompute from authority
                (canonical_authority.clone(), true) // Invalidation occurred
            }
            None => {
                (canonical_authority.clone(), false) // Cache miss
            }
        }
    }
}
