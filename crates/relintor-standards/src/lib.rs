//! P6 deterministic standards, requirement-graph, and mission-sealing authority.
//!
//! This crate is deliberately independent of the renderer and of the future P7
//! scheduler. Standards are inspectable data, predicates are a closed typed AST,
//! and every authority-bearing transition fails closed with specific reasons.

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

pub const P6_SCHEMA_VERSION: u16 = 1;
pub const STANDARDS_STATUS: &str = "implemented_unverified";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DomainPack {
    WebFrontend,
    BackendApi,
    Databases,
    AuthenticationAuthorization,
    ApplicationSecurity,
    Accessibility,
    SeoDiscoverability,
    Performance,
    DevopsReleaseEngineering,
    ObservabilityOperations,
    DataPrivacy,
    PaymentsFinancialWorkflows,
    AiMlApplications,
    BlockchainWeb3,
    Mobile,
    Desktop,
    DataEngineering,
    ThirdPartyIntegrations,
}

impl DomainPack {
    pub const ALL: [Self; 18] = [
        Self::WebFrontend,
        Self::BackendApi,
        Self::Databases,
        Self::AuthenticationAuthorization,
        Self::ApplicationSecurity,
        Self::Accessibility,
        Self::SeoDiscoverability,
        Self::Performance,
        Self::DevopsReleaseEngineering,
        Self::ObservabilityOperations,
        Self::DataPrivacy,
        Self::PaymentsFinancialWorkflows,
        Self::AiMlApplications,
        Self::BlockchainWeb3,
        Self::Mobile,
        Self::Desktop,
        Self::DataEngineering,
        Self::ThirdPartyIntegrations,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Self::WebFrontend => "web-frontend",
            Self::BackendApi => "backend-api",
            Self::Databases => "databases",
            Self::AuthenticationAuthorization => "authentication-authorization",
            Self::ApplicationSecurity => "application-security",
            Self::Accessibility => "accessibility",
            Self::SeoDiscoverability => "seo-discoverability",
            Self::Performance => "performance",
            Self::DevopsReleaseEngineering => "devops-release-engineering",
            Self::ObservabilityOperations => "observability-operations",
            Self::DataPrivacy => "data-privacy",
            Self::PaymentsFinancialWorkflows => "payments-financial-workflows",
            Self::AiMlApplications => "ai-ml-applications",
            Self::BlockchainWeb3 => "blockchain-web3",
            Self::Mobile => "mobile",
            Self::Desktop => "desktop",
            Self::DataEngineering => "data-engineering",
            Self::ThirdPartyIntegrations => "third-party-integrations",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::WebFrontend => "Web frontend",
            Self::BackendApi => "Backend/API",
            Self::Databases => "Databases",
            Self::AuthenticationAuthorization => "Authentication/authorization",
            Self::ApplicationSecurity => "Application security",
            Self::Accessibility => "Accessibility",
            Self::SeoDiscoverability => "SEO/discoverability",
            Self::Performance => "Performance",
            Self::DevopsReleaseEngineering => "DevOps/release engineering",
            Self::ObservabilityOperations => "Observability/operations",
            Self::DataPrivacy => "Data/privacy",
            Self::PaymentsFinancialWorkflows => "Payments/financial workflows",
            Self::AiMlApplications => "AI/ML applications",
            Self::BlockchainWeb3 => "Blockchain/Web3",
            Self::Mobile => "Mobile",
            Self::Desktop => "Desktop",
            Self::DataEngineering => "Data engineering",
            Self::ThirdPartyIntegrations => "Third-party integrations",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SourceVerification {
    SourceVerified,
    SourceMetadataOnly,
    InternalRelintorRule,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StandardSource {
    pub source_id: String,
    pub title: String,
    pub uri: Option<String>,
    pub version: String,
    pub published_or_reviewed: String,
    pub verification: SourceVerification,
    pub rationale: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FactValue {
    Text(String),
    Bool(bool),
    Number(i64),
    Set(Vec<String>),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ApplicabilityContext {
    pub facts: BTreeMap<String, FactValue>,
    pub revision: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ProjectAuthorityInput {
    pub requirements: Vec<ProjectRequirementSeed>,
    pub architecture_decisions: Vec<String>,
    pub assumptions: Vec<String>,
    pub risks: Vec<String>,
    pub decisions: Vec<ExplicitDecision>,
    pub source_revision: String,
    pub source_fingerprint: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectRequirementSeed {
    pub requirement_id: Option<String>,
    pub title: String,
    pub intent: String,
    pub source: RequirementSource,
    pub priority: RequirementPriority,
    pub acceptance_criteria: Vec<AcceptanceCriterion>,
    pub verification_policy: VerificationPolicy,
    pub dependencies: Vec<String>,
    pub risk: RequirementRisk,
    pub requirement_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ApplicabilityPredicate {
    Always,
    Equals { field: String, value: FactValue },
    NotEquals { field: String, value: FactValue },
    Contains { field: String, value: String },
    Exists { field: String },
    GreaterThan { field: String, value: i64 },
    LessThan { field: String, value: i64 },
    InSet { field: String, values: Vec<String> },
    And(Vec<Self>),
    Or(Vec<Self>),
    Not(Box<Self>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PredicateValue {
    True,
    False,
    Unknown,
}

impl ApplicabilityPredicate {
    fn evaluate(&self, context: &ApplicabilityContext) -> PredicateValue {
        match self {
            Self::Always => PredicateValue::True,
            Self::Equals { field, value } => match context.facts.get(field) {
                Some(actual) => PredicateValue::from(actual == value),
                None => PredicateValue::Unknown,
            },
            Self::NotEquals { field, value } => match context.facts.get(field) {
                Some(actual) => PredicateValue::from(actual != value),
                None => PredicateValue::Unknown,
            },
            Self::Contains { field, value } => match context.facts.get(field) {
                Some(FactValue::Text(actual)) => PredicateValue::from(actual.contains(value)),
                Some(FactValue::Set(actual)) => PredicateValue::from(actual.contains(value)),
                Some(_) => PredicateValue::False,
                None => PredicateValue::Unknown,
            },
            Self::Exists { field } => PredicateValue::from(context.facts.contains_key(field)),
            Self::GreaterThan { field, value } => match context.facts.get(field) {
                Some(FactValue::Number(actual)) => PredicateValue::from(actual > value),
                Some(_) => PredicateValue::False,
                None => PredicateValue::Unknown,
            },
            Self::LessThan { field, value } => match context.facts.get(field) {
                Some(FactValue::Number(actual)) => PredicateValue::from(actual < value),
                Some(_) => PredicateValue::False,
                None => PredicateValue::Unknown,
            },
            Self::InSet { field, values } => match context.facts.get(field) {
                Some(FactValue::Text(actual)) => PredicateValue::from(values.contains(actual)),
                Some(FactValue::Set(actual)) => {
                    PredicateValue::from(actual.iter().any(|item| values.contains(item)))
                }
                Some(_) => PredicateValue::False,
                None => PredicateValue::Unknown,
            },
            Self::And(predicates) => {
                let mut unknown = false;
                for predicate in predicates {
                    match predicate.evaluate(context) {
                        PredicateValue::False => return PredicateValue::False,
                        PredicateValue::Unknown => unknown = true,
                        PredicateValue::True => {}
                    }
                }
                if unknown {
                    PredicateValue::Unknown
                } else {
                    PredicateValue::True
                }
            }
            Self::Or(predicates) => {
                let mut unknown = false;
                for predicate in predicates {
                    match predicate.evaluate(context) {
                        PredicateValue::True => return PredicateValue::True,
                        PredicateValue::Unknown => unknown = true,
                        PredicateValue::False => {}
                    }
                }
                if unknown {
                    PredicateValue::Unknown
                } else {
                    PredicateValue::False
                }
            }
            Self::Not(predicate) => match predicate.evaluate(context) {
                PredicateValue::True => PredicateValue::False,
                PredicateValue::False => PredicateValue::True,
                PredicateValue::Unknown => PredicateValue::Unknown,
            },
        }
    }

    fn validate(&self) -> Result<(), AuthorityError> {
        match self {
            Self::And(items) | Self::Or(items) => {
                if items.is_empty() {
                    return Err(AuthorityError::InvalidPredicate(
                        "logical predicate cannot be empty".into(),
                    ));
                }
                for item in items {
                    item.validate()?;
                }
            }
            Self::Not(item) => item.validate()?,
            Self::InSet { values, .. } if values.is_empty() => {
                return Err(AuthorityError::InvalidPredicate(
                    "in_set predicate cannot be empty".into(),
                ));
            }
            _ => {}
        }
        Ok(())
    }
}

impl From<bool> for PredicateValue {
    fn from(value: bool) -> Self {
        if value {
            Self::True
        } else {
            Self::False
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RuleSeverity {
    Critical,
    High,
    Medium,
    Low,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ExceptionPolicy {
    NonWaivable,
    ExplicitExceptionAllowed,
    ProjectNotApplicable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StandardRule {
    pub rule_id: String,
    pub title: String,
    pub domain: DomainPack,
    pub rule_version: String,
    pub source: StandardSource,
    pub applicability: ApplicabilityPredicate,
    pub severity: RuleSeverity,
    pub rationale: String,
    pub expected_implementation_patterns: Vec<String>,
    pub deterministic_check_hints: Vec<String>,
    pub evidence_requirements: Vec<EvidenceClass>,
    pub exception_policy: ExceptionPolicy,
    pub lifecycle: String,
    pub acceptance_template: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StandardsPack {
    pub pack_id: String,
    pub pack_version: String,
    pub domain: DomainPack,
    pub title: String,
    pub source: StandardSource,
    pub rules: Vec<StandardRule>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignatureEnvelope {
    pub signer_key_id: String,
    pub algorithm: String,
    pub signed_digest: String,
    pub signature: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrustedSigner {
    pub key_id: String,
    pub algorithm: String,
    pub public_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct TrustedSignerSet {
    pub signers: Vec<TrustedSigner>,
}

/// Public production registry metadata. The corresponding signing key is
/// release-only input and is never part of the desktop binary or database.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProductionRegistryArtifact {
    pub registry_id: String,
    pub registry_version: u64,
    pub registry_digest: String,
    pub signer_key_id: String,
    pub algorithm: String,
    pub public_key: String,
    pub signature: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StandardsRegistry {
    pub registry_id: String,
    pub registry_version: u64,
    pub created_at: String,
    pub schema_version: u16,
    pub packs: Vec<StandardsPack>,
    pub pack_counts: BTreeMap<String, usize>,
    pub pack_digests: BTreeMap<String, String>,
    pub registry_digest: String,
    pub signature: Option<SignatureEnvelope>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegistryDiff {
    pub from_version: Option<u64>,
    pub to_version: u64,
    pub added_rules: Vec<String>,
    pub removed_rules: Vec<String>,
    pub changed_rules: Vec<String>,
    pub added_packs: Vec<String>,
    pub removed_packs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StandardsUpdate {
    pub registry: StandardsRegistry,
    pub diff: RegistryDiff,
}

/// P11 signed distribution metadata binds a published artifact to the
/// standards registry it contains. The artifact itself remains a signed
/// `StandardsRegistry`; this outer envelope also authenticates the release
/// channel, supported-version floor, and exact bytes that were distributed.
pub const P11_DISTRIBUTION_SCHEMA_VERSION: u16 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StandardsDistributionMetadata {
    pub schema_version: u16,
    pub channel: String,
    pub registry_id: String,
    pub registry_version: u64,
    pub registry_digest: String,
    pub artifact_digest: String,
    pub artifact_uri: String,
    pub minimum_supported_version: String,
    pub metadata_digest: String,
    pub signature: SignatureEnvelope,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedStandardsDistribution {
    pub metadata: StandardsDistributionMetadata,
    pub update: StandardsUpdate,
}

fn validate_distribution_fields(
    channel: &str,
    artifact_uri: &str,
    minimum_supported_version: &str,
) -> Result<(), AuthorityError> {
    if !matches!(channel, "stable" | "beta" | "canary") {
        return Err(AuthorityError::InvalidDistribution(
            "distribution channel must be stable, beta, or canary".into(),
        ));
    }
    let local_http = artifact_uri.starts_with("http://localhost/")
        || artifact_uri.starts_with("http://127.0.0.1/")
        || artifact_uri.starts_with("http://[::1]/");
    if !(artifact_uri.starts_with("https://") || local_http)
        || artifact_uri.len() <= if local_http { 18 } else { 8 }
    {
        return Err(AuthorityError::InvalidDistribution(
            "distribution artifact URI must be HTTPS (or local HTTP in tests)".into(),
        ));
    }
    if minimum_supported_version.trim().is_empty()
        || minimum_supported_version.len() > 64
        || minimum_supported_version.chars().any(char::is_whitespace)
    {
        return Err(AuthorityError::InvalidDistribution(
            "minimum supported version is invalid".into(),
        ));
    }
    Ok(())
}

impl StandardsDistributionMetadata {
    fn unsigned_canonical_bytes(&self) -> Result<Vec<u8>, AuthorityError> {
        let mut unsigned = self.clone();
        unsigned.metadata_digest.clear();
        unsigned.signature = SignatureEnvelope {
            signer_key_id: String::new(),
            algorithm: String::new(),
            signed_digest: String::new(),
            signature: String::new(),
        };
        serde_json::to_vec(&unsigned)
            .map_err(|error| AuthorityError::Canonicalization(error.to_string()))
    }

    pub fn calculate_digest(&self) -> Result<String, AuthorityError> {
        Ok(sha256_hex(&self.unsigned_canonical_bytes()?))
    }

    pub fn validate(&self) -> Result<(), AuthorityError> {
        if self.schema_version != P11_DISTRIBUTION_SCHEMA_VERSION
            || self.registry_id.trim().is_empty()
            || self.registry_version == 0
            || self.registry_digest.trim().is_empty()
            || self.artifact_digest.trim().is_empty()
        {
            return Err(AuthorityError::InvalidDistribution(
                "distribution metadata identity or digest is invalid".into(),
            ));
        }
        validate_distribution_fields(
            &self.channel,
            &self.artifact_uri,
            &self.minimum_supported_version,
        )?;
        if self.signature.algorithm != "Ed25519"
            || self.signature.signed_digest != self.metadata_digest
            || self.signature.signer_key_id.trim().is_empty()
            || self.signature.signature.trim().is_empty()
        {
            return Err(AuthorityError::MalformedSignature);
        }
        Ok(())
    }
}

/// Create the signed release metadata that accompanies an exact registry
/// artifact. Release signing keys are supplied by the release environment and
/// are intentionally not embedded in this crate or in a renderer.
pub fn create_signed_distribution_metadata(
    registry: &StandardsRegistry,
    artifact_bytes: &[u8],
    channel: &str,
    artifact_uri: &str,
    minimum_supported_version: &str,
    signer_key_id: &str,
    signing_key: &SigningKey,
) -> Result<StandardsDistributionMetadata, AuthorityError> {
    validate_distribution_fields(channel, artifact_uri, minimum_supported_version)?;
    registry.validate_schema()?;
    let calculated_registry_digest = registry.calculate_digest()?;
    if registry.registry_digest != calculated_registry_digest {
        return Err(AuthorityError::RegistryDigestMismatch);
    }
    let registry_signature = registry
        .signature
        .as_ref()
        .ok_or(AuthorityError::UnsignedRegistry)?;
    if registry_signature.signer_key_id != signer_key_id {
        return Err(AuthorityError::WrongSigner);
    }
    let artifact_registry: StandardsRegistry = serde_json::from_slice(artifact_bytes)
        .map_err(|error| AuthorityError::InvalidDistribution(error.to_string()))?;
    if artifact_registry != *registry {
        return Err(AuthorityError::InvalidDistribution(
            "artifact bytes do not equal the signed registry".into(),
        ));
    }
    let mut metadata = StandardsDistributionMetadata {
        schema_version: P11_DISTRIBUTION_SCHEMA_VERSION,
        channel: channel.into(),
        registry_id: registry.registry_id.clone(),
        registry_version: registry.registry_version,
        registry_digest: registry.registry_digest.clone(),
        artifact_digest: sha256_hex(artifact_bytes),
        artifact_uri: artifact_uri.into(),
        minimum_supported_version: minimum_supported_version.into(),
        metadata_digest: String::new(),
        signature: SignatureEnvelope {
            signer_key_id: signer_key_id.into(),
            algorithm: "Ed25519".into(),
            signed_digest: String::new(),
            signature: String::new(),
        },
    };
    metadata.metadata_digest = metadata.calculate_digest()?;
    let signature = signing_key.sign(metadata.metadata_digest.as_bytes());
    metadata.signature.signed_digest = metadata.metadata_digest.clone();
    metadata.signature.signature = BASE64.encode(signature.to_bytes());
    metadata.validate()?;
    Ok(metadata)
}

/// Verify a signed distribution, including its exact artifact bytes, and
/// enforce the monotonic registry-version rule before returning an update.
pub fn verify_signed_distribution(
    current: Option<&StandardsRegistry>,
    metadata: &StandardsDistributionMetadata,
    artifact_bytes: &[u8],
    trusted: &TrustedSignerSet,
) -> Result<VerifiedStandardsDistribution, AuthorityError> {
    metadata.validate()?;
    if metadata.metadata_digest != metadata.calculate_digest()? {
        return Err(AuthorityError::DistributionDigestMismatch);
    }
    let signer = trusted
        .signers
        .iter()
        .find(|signer| signer.key_id == metadata.signature.signer_key_id)
        .ok_or_else(|| AuthorityError::UnknownSigner(metadata.signature.signer_key_id.clone()))?;
    if signer.algorithm != metadata.signature.algorithm {
        return Err(AuthorityError::WrongSigner);
    }
    let public_key = BASE64
        .decode(&signer.public_key)
        .map_err(|_| AuthorityError::MalformedSignature)?;
    let public_key: [u8; 32] = public_key
        .try_into()
        .map_err(|_| AuthorityError::MalformedSignature)?;
    let verifying_key =
        VerifyingKey::from_bytes(&public_key).map_err(|_| AuthorityError::MalformedSignature)?;
    let signature_bytes = BASE64
        .decode(&metadata.signature.signature)
        .map_err(|_| AuthorityError::MalformedSignature)?;
    let signature =
        Signature::from_slice(&signature_bytes).map_err(|_| AuthorityError::MalformedSignature)?;
    verifying_key
        .verify(metadata.metadata_digest.as_bytes(), &signature)
        .map_err(|_| AuthorityError::InvalidSignature)?;

    if sha256_hex(artifact_bytes) != metadata.artifact_digest {
        return Err(AuthorityError::DistributionArtifactDigestMismatch);
    }
    let registry: StandardsRegistry = serde_json::from_slice(artifact_bytes)
        .map_err(|error| AuthorityError::InvalidDistribution(error.to_string()))?;
    if registry.registry_id != metadata.registry_id
        || registry.registry_version != metadata.registry_version
        || registry.registry_digest != metadata.registry_digest
    {
        return Err(AuthorityError::DistributionIdentityMismatch);
    }
    let update = apply_signed_update(current, registry, trusted)?;
    Ok(VerifiedStandardsDistribution {
        metadata: metadata.clone(),
        update,
    })
}

impl StandardsRegistry {
    pub fn validate_schema(&self) -> Result<(), AuthorityError> {
        if self.schema_version != P6_SCHEMA_VERSION {
            return Err(AuthorityError::UnsupportedSchema(self.schema_version));
        }
        if self.registry_id.trim().is_empty() || self.registry_version == 0 {
            return Err(AuthorityError::InvalidRegistry(
                "registry identity is invalid".into(),
            ));
        }
        if self.packs.len() != DomainPack::ALL.len() {
            return Err(AuthorityError::InvalidRegistry(format!(
                "expected 18 domain packs, found {}",
                self.packs.len()
            )));
        }
        let mut pack_ids = BTreeSet::new();
        let mut domains = BTreeSet::new();
        let mut rule_ids = BTreeSet::new();
        for pack in &self.packs {
            if !pack_ids.insert(pack.pack_id.clone()) {
                return Err(AuthorityError::DuplicatePack(pack.pack_id.clone()));
            }
            if !domains.insert(pack.domain) {
                return Err(AuthorityError::InvalidRegistry(format!(
                    "duplicate domain pack for {:?}",
                    pack.domain
                )));
            }
            if pack.rules.is_empty() || pack.pack_id != pack.domain.id() {
                return Err(AuthorityError::InvalidRegistry(format!(
                    "pack {} is empty or has an invalid domain identity",
                    pack.pack_id
                )));
            }
            for rule in &pack.rules {
                if !rule_ids.insert(rule.rule_id.clone()) {
                    return Err(AuthorityError::DuplicateRule(rule.rule_id.clone()));
                }
                if rule.domain != pack.domain {
                    return Err(AuthorityError::InvalidRegistry(format!(
                        "rule {} is in the wrong pack",
                        rule.rule_id
                    )));
                }
                rule.applicability.validate()?;
                if rule.evidence_requirements.is_empty()
                    || rule.acceptance_template.trim().is_empty()
                {
                    return Err(AuthorityError::InvalidRegistry(format!(
                        "rule {} lacks evidence or acceptance metadata",
                        rule.rule_id
                    )));
                }
            }
        }
        let expected_domains = DomainPack::ALL.into_iter().collect::<BTreeSet<_>>();
        if domains != expected_domains {
            return Err(AuthorityError::InvalidRegistry(
                "registry does not contain exactly the sealed 18 domain packs".into(),
            ));
        }
        for pack in &self.packs {
            let expected_count = pack.rules.len();
            if self.pack_counts.get(&pack.pack_id) != Some(&expected_count) {
                return Err(AuthorityError::InvalidRegistry(format!(
                    "pack count metadata is stale for {}",
                    pack.pack_id
                )));
            }
            let expected_digest = canonical_pack_digest(pack)?;
            if self.pack_digests.get(&pack.pack_id) != Some(&expected_digest) {
                return Err(AuthorityError::InvalidRegistry(format!(
                    "pack digest metadata is stale for {}",
                    pack.pack_id
                )));
            }
        }
        Ok(())
    }

    fn unsigned_canonical_bytes(&self) -> Result<Vec<u8>, AuthorityError> {
        let mut normalized = self.clone();
        normalized.registry_digest.clear();
        normalized.signature = None;
        normalized.packs.sort_by(|a, b| a.pack_id.cmp(&b.pack_id));
        for pack in &mut normalized.packs {
            pack.rules.sort_by(|a, b| a.rule_id.cmp(&b.rule_id));
        }
        serde_json::to_vec(&normalized)
            .map_err(|error| AuthorityError::Canonicalization(error.to_string()))
    }

    pub fn calculate_digest(&self) -> Result<String, AuthorityError> {
        Ok(sha256_hex(&self.unsigned_canonical_bytes()?))
    }

    pub fn sign(
        &mut self,
        signer_key_id: &str,
        signing_key: &SigningKey,
    ) -> Result<(), AuthorityError> {
        self.refresh_pack_metadata()?;
        self.validate_schema()?;
        self.registry_digest = self.calculate_digest()?;
        let signature = signing_key.sign(self.registry_digest.as_bytes());
        self.signature = Some(SignatureEnvelope {
            signer_key_id: signer_key_id.into(),
            algorithm: "Ed25519".into(),
            signed_digest: self.registry_digest.clone(),
            signature: BASE64.encode(signature.to_bytes()),
        });
        Ok(())
    }

    pub fn refresh_pack_metadata(&mut self) -> Result<(), AuthorityError> {
        self.pack_counts.clear();
        self.pack_digests.clear();
        for pack in &self.packs {
            self.pack_counts
                .insert(pack.pack_id.clone(), pack.rules.len());
            self.pack_digests
                .insert(pack.pack_id.clone(), canonical_pack_digest(pack)?);
        }
        Ok(())
    }

    pub fn verify(&self, trusted: &TrustedSignerSet, strict: bool) -> Result<(), AuthorityError> {
        if let Err(error) = self.validate_schema() {
            if matches!(&error, AuthorityError::InvalidRegistry(message) if message.contains("metadata is stale"))
            {
                return Err(AuthorityError::RegistryDigestMismatch);
            }
            return Err(error);
        }
        let calculated = self.calculate_digest()?;
        if self.registry_digest != calculated {
            return Err(AuthorityError::RegistryDigestMismatch);
        }
        let Some(envelope) = &self.signature else {
            if strict {
                return Err(AuthorityError::UnsignedRegistry);
            }
            return Ok(());
        };
        if envelope.algorithm != "Ed25519" || envelope.signed_digest != calculated {
            return Err(AuthorityError::MalformedSignature);
        }
        let signer = trusted
            .signers
            .iter()
            .find(|signer| signer.key_id == envelope.signer_key_id)
            .ok_or_else(|| AuthorityError::UnknownSigner(envelope.signer_key_id.clone()))?;
        if signer.algorithm != envelope.algorithm {
            return Err(AuthorityError::WrongSigner);
        }
        let public_key = BASE64
            .decode(&signer.public_key)
            .map_err(|_| AuthorityError::MalformedSignature)?;
        let public_key: [u8; 32] = public_key
            .try_into()
            .map_err(|_| AuthorityError::MalformedSignature)?;
        let verifying_key = VerifyingKey::from_bytes(&public_key)
            .map_err(|_| AuthorityError::MalformedSignature)?;
        let signature_bytes = BASE64
            .decode(&envelope.signature)
            .map_err(|_| AuthorityError::MalformedSignature)?;
        let signature = Signature::from_slice(&signature_bytes)
            .map_err(|_| AuthorityError::MalformedSignature)?;
        verifying_key
            .verify(calculated.as_bytes(), &signature)
            .map_err(|_| AuthorityError::InvalidSignature)
    }
}

pub fn apply_signed_update(
    current: Option<&StandardsRegistry>,
    proposed: StandardsRegistry,
    trusted: &TrustedSignerSet,
) -> Result<StandardsUpdate, AuthorityError> {
    proposed.verify(trusted, true)?;
    if current.is_some_and(|current| proposed.registry_version <= current.registry_version) {
        return Err(AuthorityError::RegistryReplay);
    }
    let diff = registry_diff(current, &proposed);
    Ok(StandardsUpdate {
        registry: proposed,
        diff,
    })
}

fn canonical_pack_digest(pack: &StandardsPack) -> Result<String, AuthorityError> {
    let mut normalized = pack.clone();
    normalized.rules.sort_by(|a, b| a.rule_id.cmp(&b.rule_id));
    serde_json::to_vec(&normalized)
        .map(|bytes| sha256_hex(&bytes))
        .map_err(|error| AuthorityError::Canonicalization(error.to_string()))
}

fn registry_diff(
    current: Option<&StandardsRegistry>,
    proposed: &StandardsRegistry,
) -> RegistryDiff {
    let current_rules = current
        .into_iter()
        .flat_map(|registry| registry.packs.iter().flat_map(|pack| pack.rules.iter()))
        .map(|rule| {
            (
                rule.rule_id.clone(),
                serde_json::to_string(rule).unwrap_or_default(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let proposed_rules = proposed
        .packs
        .iter()
        .flat_map(|pack| pack.rules.iter())
        .map(|rule| {
            (
                rule.rule_id.clone(),
                serde_json::to_string(rule).unwrap_or_default(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let added_rules = proposed_rules
        .keys()
        .filter(|id| !current_rules.contains_key(*id))
        .cloned()
        .collect();
    let removed_rules = current_rules
        .keys()
        .filter(|id| !proposed_rules.contains_key(*id))
        .cloned()
        .collect();
    let changed_rules = proposed_rules
        .iter()
        .filter(|(id, value)| current_rules.get(*id).is_some_and(|old| old != *value))
        .map(|(id, _)| id.clone())
        .collect();
    let current_packs = current
        .into_iter()
        .flat_map(|registry| registry.packs.iter().map(|pack| pack.pack_id.clone()))
        .collect::<BTreeSet<_>>();
    let proposed_packs = proposed
        .packs
        .iter()
        .map(|pack| pack.pack_id.clone())
        .collect::<BTreeSet<_>>();
    RegistryDiff {
        from_version: current.map(|registry| registry.registry_version),
        to_version: proposed.registry_version,
        added_rules,
        removed_rules,
        changed_rules,
        added_packs: proposed_packs.difference(&current_packs).cloned().collect(),
        removed_packs: current_packs.difference(&proposed_packs).cloned().collect(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ApplicabilityOutcome {
    Applicable,
    NotApplicable,
    NeedsDecision,
    BlockedByUnknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApplicabilityResult {
    pub rule_id: String,
    pub rule_digest: String,
    pub outcome: ApplicabilityOutcome,
    pub reason: String,
    pub facts_used: BTreeMap<String, FactValue>,
    pub source: StandardSource,
    pub predicate_version: String,
    pub evaluated_at_revision: String,
    pub requirement_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RequirementStatus {
    Unstarted,
    InProgress,
    ImplementedUnverified,
    Verified,
    Failed,
    Blocked,
    NotApplicable,
    DeferredByExplicitDecision,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RequirementPriority {
    P0,
    P1,
    P2,
    P3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RequirementRisk {
    Critical,
    High,
    Medium,
    Low,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RequirementSource {
    User {
        reference: String,
    },
    Document {
        reference: String,
    },
    Blueprint {
        reference: String,
    },
    TakeoverFinding {
        reference: String,
    },
    Standard {
        registry_id: String,
        registry_version: u64,
        pack_id: String,
        rule_id: String,
        rule_digest: String,
        applicability_id: String,
    },
    Inference {
        reference: String,
    },
    ExplicitDecision {
        reference: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EvidenceClass {
    SourceDiff,
    FileHash,
    BuildOutput,
    TestOutput,
    LintStaticAnalysis,
    ApiResponse,
    DatabaseQuery,
    BrowserRecording,
    Screenshot,
    AccessibilityResult,
    PerformanceResult,
    SecurityScan,
    DeploymentProbe,
    ExternalServiceReceipt,
    HumanDecision,
    AiVerifierJudgement,
    EnvironmentFingerprint,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EvidenceConfidence {
    StrongDeterministic,
    StrongRuntime,
    Corroborated,
    AiReviewed,
    HumanAsserted,
    Weak,
    Missing,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcceptanceCriterion {
    pub criterion_id: String,
    pub statement: String,
    pub criterion_type: String,
    pub machine_checkable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidenceObligation {
    pub class: EvidenceClass,
    pub minimum_confidence: EvidenceConfidence,
    pub rationale: String,
    pub required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerificationPolicy {
    pub obligations: Vec<EvidenceObligation>,
    pub p8_collector_required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequirementDependency {
    pub requirement_id: String,
    pub depends_on: String,
    pub reason: String,
    pub dependency_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Requirement {
    pub requirement_id: String,
    pub title: String,
    pub intent: String,
    pub source: RequirementSource,
    pub priority: RequirementPriority,
    pub applicability: ApplicabilityOutcome,
    pub acceptance_criteria: Vec<AcceptanceCriterion>,
    pub verification_policy: VerificationPolicy,
    pub dependencies: Vec<String>,
    pub risk: RequirementRisk,
    pub status: RequirementStatus,
    pub implementation_links: Vec<String>,
    pub evidence_links: Vec<String>,
    pub explicit_exceptions: Vec<String>,
    pub sealed_hash: Option<String>,
    pub requirement_type: String,
    pub origin_rule_id: Option<String>,
    pub revision: u64,
    pub schema_version: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct RequirementGraph {
    pub requirements: Vec<Requirement>,
    pub dependencies: Vec<RequirementDependency>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Task {
    pub task_id: String,
    pub title: String,
    pub objective: String,
    pub requirement_ids: Vec<String>,
    pub dependency_ids: Vec<String>,
    pub suggested_scope: String,
    pub risk: RequirementRisk,
    pub evidence_obligations: Vec<EvidenceObligation>,
    pub status: RequirementStatus,
    pub provenance: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskDependency {
    pub task_id: String,
    pub depends_on: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskRequirementLink {
    pub task_id: String,
    pub requirement_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct TaskGraph {
    pub tasks: Vec<Task>,
    pub dependencies: Vec<TaskDependency>,
    pub links: Vec<TaskRequirementLink>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DecisionActor {
    Human,
    User,
    Ai,
    System,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DecisionKind {
    Defer,
    NotApplicable,
    ExplicitException,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExplicitDecision {
    pub decision_id: String,
    pub kind: DecisionKind,
    pub actor: DecisionActor,
    pub requirement_id: String,
    pub reason: String,
    pub risk: String,
    pub timestamp: String,
    pub mission_revision: u64,
    pub provenance: String,
}

pub type ExceptionRecord = ExplicitDecision;
pub type DeferralDecision = ExplicitDecision;
pub type NotApplicableDecision = ExplicitDecision;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScopeFingerprint {
    pub algorithm: String,
    pub value: String,
    pub inputs: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MissionDraft {
    pub mission_id: String,
    pub project_id: String,
    pub project_source_revision: String,
    pub blueprint_or_takeover_fingerprint: String,
    pub registry: StandardsRegistry,
    pub applicability: Vec<ApplicabilityResult>,
    pub applicability_context: ApplicabilityContext,
    pub applicability_context_digest: String,
    pub project_authority_fingerprint: String,
    pub project_authority: ProjectAuthorityInput,
    pub project_authority_digest: String,
    pub requirement_graph: RequirementGraph,
    pub task_graph: TaskGraph,
    pub decisions: Vec<ExplicitDecision>,
    pub scope: ScopeFingerprint,
    pub architecture_decisions: Vec<String>,
    pub revision: u64,
    pub schema_version: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MissionContract {
    pub mission_id: String,
    pub project_id: String,
    pub project_source_revision: String,
    pub blueprint_or_takeover_fingerprint: String,
    pub registry_id: String,
    pub registry_version: u64,
    pub registry_digest: String,
    pub authority_manifest: MissionSealManifest,
    pub applicability: Vec<ApplicabilityResult>,
    pub applicability_context: ApplicabilityContext,
    pub applicability_context_digest: String,
    pub project_authority_fingerprint: String,
    pub project_authority: ProjectAuthorityInput,
    pub project_authority_digest: String,
    pub requirement_graph: RequirementGraph,
    pub task_graph: TaskGraph,
    pub decisions: Vec<ExplicitDecision>,
    pub scope: ScopeFingerprint,
    pub architecture_decisions: Vec<String>,
    pub revision: u64,
    pub schema_version: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MissionSealManifest {
    pub registry_id: String,
    pub registry_version: u64,
    pub registry_digest: String,
    pub pack_versions: BTreeMap<String, String>,
    pub rule_ids: Vec<String>,
    pub rule_digests: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MissionSeal {
    pub mission_id: String,
    pub revision: u64,
    pub contract_hash: String,
    pub manifest: MissionSealManifest,
    pub manifest_digest: String,
    pub project_id: String,
    pub project_source_revision: String,
    pub blueprint_or_takeover_fingerprint: String,
    pub requirement_hashes: BTreeMap<String, String>,
    pub scope_fingerprint: String,
    pub sealed_at: String,
    pub schema_version: u16,
    pub state: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MissionRevision {
    pub revision: u64,
    pub contract: MissionContract,
    pub seal: MissionSeal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionHandoff {
    pub mission_id: String,
    pub revision: u64,
    pub contract_hash: String,
    pub state: String,
    pub task_order: Vec<String>,
    pub scheduler_owner: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RevalidationReason {
    RequirementChanged(String),
    RequirementRemoved(String),
    AcceptanceChanged(String),
    EvidencePolicyChanged(String),
    DependencyChanged(String),
    TaskCoverageChanged(String),
    DecisionChanged(String),
    RegistryChanged,
    ScopeChanged,
    ContractHashChanged,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SealValidationResult {
    pub valid: bool,
    pub state: String,
    pub reasons: Vec<RevalidationReason>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RevalidationResult {
    pub status: String,
    pub reasons: Vec<RevalidationReason>,
    pub new_revision_required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthorityError {
    UnsupportedSchema(u16),
    InvalidRegistry(String),
    DuplicatePack(String),
    DuplicateRule(String),
    InvalidPredicate(String),
    UnsignedRegistry,
    UnknownSigner(String),
    WrongSigner,
    MalformedSignature,
    InvalidSignature,
    RegistryDigestMismatch,
    RegistryReplay,
    InvalidDistribution(String),
    DistributionDigestMismatch,
    DistributionArtifactDigestMismatch,
    DistributionIdentityMismatch,
    Canonicalization(String),
    DuplicateRequirement(String),
    MissingRequirement(String),
    SelfDependency(String),
    DuplicateDependency(String),
    DependencyCycle(Vec<String>),
    DuplicateTask(String),
    MissingTask(String),
    TaskCycle(Vec<String>),
    OrphanTask(String),
    MissingTaskCoverage(String),
    MissingAcceptance(String),
    MissingEvidencePolicy(String),
    UnknownApplicability(String),
    CriticalRuleMissing(String),
    InvalidDecision(String),
    ApplicabilityMismatch(String),
    RevalidationRequired(Vec<RevalidationReason>),
    Persistence(String),
}

impl std::fmt::Display for AuthorityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedSchema(value) => write!(f, "unsupported P6 schema version {value}"),
            Self::InvalidRegistry(value) => write!(f, "invalid standards registry: {value}"),
            Self::DuplicatePack(value) => write!(f, "duplicate standards pack: {value}"),
            Self::DuplicateRule(value) => write!(f, "duplicate standards rule: {value}"),
            Self::InvalidPredicate(value) => write!(f, "invalid applicability predicate: {value}"),
            Self::UnsignedRegistry => f.write_str("strict verification rejects unsigned registry"),
            Self::UnknownSigner(value) => write!(f, "unknown standards signer: {value}"),
            Self::WrongSigner => f.write_str("standards signature signer is not trusted"),
            Self::MalformedSignature => f.write_str("malformed standards signature"),
            Self::InvalidSignature => f.write_str("invalid standards signature"),
            Self::RegistryDigestMismatch => f.write_str("standards registry digest mismatch"),
            Self::RegistryReplay => {
                f.write_str("standards registry update is a downgrade or replay")
            }
            Self::InvalidDistribution(value) => {
                write!(f, "invalid standards distribution: {value}")
            }
            Self::DistributionDigestMismatch => {
                f.write_str("standards distribution metadata digest mismatch")
            }
            Self::DistributionArtifactDigestMismatch => {
                f.write_str("standards distribution artifact digest mismatch")
            }
            Self::DistributionIdentityMismatch => {
                f.write_str("standards distribution identity mismatch")
            }
            Self::Canonicalization(value) => write!(f, "canonicalization failed: {value}"),
            Self::DuplicateRequirement(value) => write!(f, "duplicate requirement: {value}"),
            Self::MissingRequirement(value) => write!(f, "missing requirement: {value}"),
            Self::SelfDependency(value) => write!(f, "self dependency: {value}"),
            Self::DuplicateDependency(value) => write!(f, "duplicate dependency: {value}"),
            Self::DependencyCycle(value) => write!(f, "requirement dependency cycle: {value:?}"),
            Self::DuplicateTask(value) => write!(f, "duplicate task: {value}"),
            Self::MissingTask(value) => write!(f, "missing task: {value}"),
            Self::TaskCycle(value) => write!(f, "task dependency cycle: {value:?}"),
            Self::OrphanTask(value) => write!(f, "orphan task: {value}"),
            Self::MissingTaskCoverage(value) => write!(f, "missing task coverage: {value}"),
            Self::MissingAcceptance(value) => write!(f, "missing acceptance criteria: {value}"),
            Self::MissingEvidencePolicy(value) => write!(f, "missing evidence policy: {value}"),
            Self::UnknownApplicability(value) => write!(f, "unresolved applicability: {value}"),
            Self::CriticalRuleMissing(value) => {
                write!(f, "critical rule has no requirement: {value}")
            }
            Self::InvalidDecision(value) => write!(f, "invalid explicit decision: {value}"),
            Self::ApplicabilityMismatch(value) => {
                write!(f, "applicability authority mismatch: {value}")
            }
            Self::RevalidationRequired(value) => write!(f, "revalidation required: {value:?}"),
            Self::Persistence(value) => write!(f, "authority persistence failed: {value}"),
        }
    }
}

impl std::error::Error for AuthorityError {}

#[derive(Debug, Default, Clone, Copy)]
pub struct AuthorityEngine;

impl AuthorityEngine {
    pub fn evaluate(
        &self,
        registry: &StandardsRegistry,
        context: &ApplicabilityContext,
    ) -> Result<(Vec<ApplicabilityResult>, RequirementGraph), AuthorityError> {
        registry.validate_schema()?;
        let mut ledger = Vec::new();
        let mut requirements = Vec::new();
        for pack in &registry.packs {
            for rule in &pack.rules {
                let outcome = match rule.applicability.evaluate(context) {
                    PredicateValue::True => ApplicabilityOutcome::Applicable,
                    PredicateValue::False => ApplicabilityOutcome::NotApplicable,
                    PredicateValue::Unknown => ApplicabilityOutcome::BlockedByUnknown,
                };
                let requirement_id = if outcome == ApplicabilityOutcome::Applicable {
                    let requirement_id =
                        stable_id("requirement", &[&registry.registry_id, &rule.rule_id]);
                    requirements.push(requirement_from_rule(registry, pack, rule, &requirement_id));
                    Some(requirement_id)
                } else {
                    None
                };
                let reason = match outcome {
                    ApplicabilityOutcome::Applicable => {
                        "structured project facts satisfy the rule predicate"
                    }
                    ApplicabilityOutcome::NotApplicable => {
                        "structured project facts do not satisfy the rule predicate"
                    }
                    ApplicabilityOutcome::BlockedByUnknown => {
                        "required project fact is unavailable"
                    }
                    ApplicabilityOutcome::NeedsDecision => {
                        "explicit user applicability decision is required"
                    }
                };
                ledger.push(ApplicabilityResult {
                    rule_id: rule.rule_id.clone(),
                    rule_digest: sha256_hex(
                        &serde_json::to_vec(rule)
                            .map_err(|error| AuthorityError::Canonicalization(error.to_string()))?,
                    ),
                    outcome,
                    reason: reason.into(),
                    facts_used: context.facts.clone(),
                    source: rule.source.clone(),
                    predicate_version: "p6-predicate-v1".into(),
                    evaluated_at_revision: context.revision.clone(),
                    requirement_id,
                });
            }
        }
        requirements.sort_by(|a, b| a.requirement_id.cmp(&b.requirement_id));
        Ok((
            ledger,
            RequirementGraph {
                requirements,
                dependencies: Vec::new(),
            },
        ))
    }

    pub fn decompose_tasks(&self, graph: &RequirementGraph) -> Result<TaskGraph, AuthorityError> {
        graph.validate()?;
        let mut tasks = Vec::new();
        let mut links = Vec::new();
        for requirement in &graph.requirements {
            if requirement.applicability != ApplicabilityOutcome::Applicable {
                continue;
            }
            let task_id = stable_id("task", &[&requirement.requirement_id]);
            tasks.push(Task {
                task_id: task_id.clone(),
                title: format!("Implement: {}", requirement.title),
                objective: requirement.intent.clone(),
                requirement_ids: vec![requirement.requirement_id.clone()],
                dependency_ids: requirement
                    .dependencies
                    .iter()
                    .map(|dependency| stable_id("task", &[dependency]))
                    .collect(),
                suggested_scope: requirement.requirement_type.clone(),
                risk: requirement.risk,
                evidence_obligations: requirement.verification_policy.obligations.clone(),
                status: requirement.status,
                provenance: format!(
                    "P6 requirement decomposition for {}",
                    requirement.requirement_id
                ),
            });
            links.push(TaskRequirementLink {
                task_id,
                requirement_id: requirement.requirement_id.clone(),
            });
        }
        let mut dependencies = Vec::new();
        for dependency in &graph.dependencies {
            let from = stable_id("task", &[&dependency.requirement_id]);
            let to = stable_id("task", &[&dependency.depends_on]);
            if from != to {
                dependencies.push(TaskDependency {
                    task_id: from,
                    depends_on: to,
                    reason: dependency.reason.clone(),
                });
            }
        }
        Ok(TaskGraph {
            tasks,
            dependencies,
            links,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn build_draft(
        &self,
        mission_id: &str,
        project_id: &str,
        project_source_revision: &str,
        blueprint_or_takeover_fingerprint: &str,
        registry: StandardsRegistry,
        context: &ApplicabilityContext,
        scope: ScopeFingerprint,
        decisions: Vec<ExplicitDecision>,
    ) -> Result<MissionDraft, AuthorityError> {
        self.build_draft_with_project_authority(
            mission_id,
            project_id,
            project_source_revision,
            blueprint_or_takeover_fingerprint,
            registry,
            context,
            scope,
            ProjectAuthorityInput::default(),
            decisions,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn build_draft_with_project_authority(
        &self,
        mission_id: &str,
        project_id: &str,
        project_source_revision: &str,
        blueprint_or_takeover_fingerprint: &str,
        registry: StandardsRegistry,
        context: &ApplicabilityContext,
        scope: ScopeFingerprint,
        project_authority: ProjectAuthorityInput,
        decisions: Vec<ExplicitDecision>,
    ) -> Result<MissionDraft, AuthorityError> {
        let (applicability, mut requirement_graph) = self.evaluate(&registry, context)?;
        let standard_requirement_ids = requirement_graph
            .requirements
            .iter()
            .map(|requirement| requirement.requirement_id.clone())
            .collect::<BTreeSet<_>>();
        let (project_authority, project_requirement_aliases) = canonicalize_project_authority(
            project_id,
            project_authority,
            &standard_requirement_ids,
        )?;
        for seed in &project_authority.requirements {
            let requirement_id = project_requirement_id(project_id, seed);
            if requirement_graph
                .requirements
                .iter()
                .any(|requirement| requirement.requirement_id == requirement_id)
            {
                return Err(AuthorityError::DuplicateRequirement(requirement_id));
            }
            requirement_graph
                .requirements
                .push(requirement_from_project_seed(seed, &requirement_id));
            for depends_on in &seed.dependencies {
                requirement_graph.dependencies.push(RequirementDependency {
                    requirement_id: requirement_id.clone(),
                    depends_on: depends_on.clone(),
                    reason: "project authority dependency".into(),
                    dependency_type: "project-authority".into(),
                });
            }
        }
        requirement_graph
            .requirements
            .sort_by(|a, b| a.requirement_id.cmp(&b.requirement_id));
        let mut all_decisions = project_authority.decisions.clone();
        all_decisions.extend(
            decisions
                .into_iter()
                .map(|mut decision| {
                    decision.requirement_id = resolve_decision_reference(
                        &decision.requirement_id,
                        &project_requirement_aliases,
                        &standard_requirement_ids,
                    );
                    Ok(decision)
                })
                .collect::<Result<Vec<_>, AuthorityError>>()?,
        );
        validate_decisions(&registry, &requirement_graph, &all_decisions)?;
        // Explicit decisions are represented in the graph rather than deleting
        // the underlying requirement, preserving authority accounting.
        for decision in &all_decisions {
            if let Some(requirement) = requirement_graph
                .requirements
                .iter_mut()
                .find(|requirement| requirement.requirement_id == decision.requirement_id)
            {
                if decision.kind == DecisionKind::Defer {
                    requirement.status = RequirementStatus::DeferredByExplicitDecision;
                    requirement
                        .explicit_exceptions
                        .push(decision.decision_id.clone());
                } else if decision.kind == DecisionKind::NotApplicable {
                    requirement.status = RequirementStatus::NotApplicable;
                    requirement
                        .explicit_exceptions
                        .push(decision.decision_id.clone());
                } else if decision.kind == DecisionKind::ExplicitException {
                    requirement.status = RequirementStatus::DeferredByExplicitDecision;
                    requirement
                        .explicit_exceptions
                        .push(decision.decision_id.clone());
                }
            }
        }
        // Decisions are authority input, so task status must be derived after
        // they are applied. The requirement remains in the graph for
        // accounting, while deferred/N-A work is not executable.
        let task_graph = self.decompose_tasks(&requirement_graph)?;
        let authority_digest = project_authority_digest(&project_authority)?;
        let authority_notes = project_authority_notes(&project_authority);
        Ok(MissionDraft {
            mission_id: mission_id.into(),
            project_id: project_id.into(),
            project_source_revision: project_source_revision.into(),
            blueprint_or_takeover_fingerprint: blueprint_or_takeover_fingerprint.into(),
            registry,
            applicability,
            applicability_context: context.clone(),
            applicability_context_digest: applicability_context_digest(context)?,
            project_authority_fingerprint: if project_authority.source_fingerprint.is_empty() {
                authority_digest.clone()
            } else {
                project_authority.source_fingerprint.clone()
            },
            project_authority,
            project_authority_digest: authority_digest,
            requirement_graph,
            task_graph,
            decisions: all_decisions,
            scope,
            architecture_decisions: authority_notes,
            revision: 1,
            schema_version: P6_SCHEMA_VERSION,
        })
    }

    pub fn preseal(
        &self,
        draft: &MissionDraft,
        trusted: &TrustedSignerSet,
    ) -> Result<(), AuthorityError> {
        draft.registry.verify(trusted, true)?;
        let context_digest = applicability_context_digest(&draft.applicability_context)?;
        if context_digest != draft.applicability_context_digest {
            return Err(AuthorityError::ApplicabilityMismatch(
                "applicability context digest changed after evaluation".into(),
            ));
        }
        let authority_digest = project_authority_digest(&draft.project_authority)?;
        if authority_digest != draft.project_authority_digest {
            return Err(AuthorityError::ApplicabilityMismatch(
                "project authority input changed after graph construction".into(),
            ));
        }
        let (recomputed_ledger, _) =
            self.evaluate(&draft.registry, &draft.applicability_context)?;
        if recomputed_ledger != draft.applicability {
            return Err(AuthorityError::ApplicabilityMismatch(
                "applicability ledger does not match deterministic re-evaluation".into(),
            ));
        }
        if draft.applicability.iter().any(|result| {
            matches!(
                result.outcome,
                ApplicabilityOutcome::BlockedByUnknown | ApplicabilityOutcome::NeedsDecision
            )
        }) {
            return Err(AuthorityError::UnknownApplicability(
                "one or more rules need facts or a decision".into(),
            ));
        }
        draft.requirement_graph.validate()?;
        draft.task_graph.validate(&draft.requirement_graph)?;
        for seed in &draft.project_authority.requirements {
            let requirement_id = project_requirement_id(&draft.project_id, seed);
            let expected = requirement_from_project_seed(seed, &requirement_id);
            let Some(actual) = draft
                .requirement_graph
                .requirements
                .iter()
                .find(|requirement| requirement.requirement_id == requirement_id)
            else {
                return Err(AuthorityError::MissingRequirement(requirement_id));
            };
            let mut normalized_actual = actual.clone();
            // Status and explicit exceptions are decision-owned fields. All
            // seed-controlled authority fields remain protected by the full
            // comparison below.
            normalized_actual.status = expected.status;
            normalized_actual.explicit_exceptions = expected.explicit_exceptions.clone();
            if normalized_actual != expected {
                return Err(AuthorityError::ApplicabilityMismatch(format!(
                    "project authority requirement {requirement_id} changed before sealing"
                )));
            }
        }
        for result in &draft.applicability {
            if result.outcome != ApplicabilityOutcome::Applicable {
                continue;
            }
            let Some(requirement_id) = &result.requirement_id else {
                return Err(AuthorityError::CriticalRuleMissing(result.rule_id.clone()));
            };
            let Some(requirement) = draft
                .requirement_graph
                .requirements
                .iter()
                .find(|requirement| &requirement.requirement_id == requirement_id)
            else {
                return Err(AuthorityError::CriticalRuleMissing(result.rule_id.clone()));
            };
            let RequirementSource::Standard {
                registry_id,
                registry_version,
                rule_id,
                rule_digest,
                applicability_id,
                ..
            } = &requirement.source
            else {
                return Err(AuthorityError::CriticalRuleMissing(result.rule_id.clone()));
            };
            if registry_id != &draft.registry.registry_id
                || *registry_version != draft.registry.registry_version
                || rule_id != &result.rule_id
                || rule_digest != &result.rule_digest
                || applicability_id != &stable_id("applicability", &[&result.rule_id])
                || requirement.applicability != ApplicabilityOutcome::Applicable
            {
                return Err(AuthorityError::CriticalRuleMissing(result.rule_id.clone()));
            }
        }
        for requirement in &draft.requirement_graph.requirements {
            if requirement.applicability != ApplicabilityOutcome::Applicable {
                continue;
            }
            if requirement.acceptance_criteria.is_empty() {
                return Err(AuthorityError::MissingAcceptance(
                    requirement.requirement_id.clone(),
                ));
            }
            if requirement.verification_policy.obligations.is_empty() {
                return Err(AuthorityError::MissingEvidencePolicy(
                    requirement.requirement_id.clone(),
                ));
            }
            if let Some(rule) = requirement.origin_rule_id.as_deref() {
                let ledger = draft
                    .applicability
                    .iter()
                    .find(|result| result.rule_id == rule);
                if ledger.is_none_or(|result| {
                    result.requirement_id.as_deref() != Some(&requirement.requirement_id)
                }) {
                    return Err(AuthorityError::CriticalRuleMissing(rule.into()));
                }
            }
            if requirement.risk == RequirementRisk::Critical
                && !draft
                    .task_graph
                    .links
                    .iter()
                    .any(|link| link.requirement_id == requirement.requirement_id)
            {
                return Err(AuthorityError::MissingTaskCoverage(
                    requirement.requirement_id.clone(),
                ));
            }
        }
        validate_decisions(&draft.registry, &draft.requirement_graph, &draft.decisions)?;
        Ok(())
    }

    pub fn seal(
        &self,
        draft: &MissionDraft,
        trusted: &TrustedSignerSet,
        sealed_at: &str,
    ) -> Result<(MissionRevision, ExecutionHandoff), AuthorityError> {
        self.preseal(draft, trusted)?;
        let mut contract = MissionContract::from_draft(draft)?;
        let mut requirement_hashes = BTreeMap::new();
        for requirement in &mut contract.requirement_graph.requirements {
            let hash = requirement_sealed_hash(requirement)?;
            requirement.sealed_hash = Some(hash.clone());
            requirement_hashes.insert(requirement.requirement_id.clone(), hash);
        }
        let contract_hash = contract.canonical_hash()?;
        let manifest = contract.authority_manifest.clone();
        let seal = MissionSeal {
            mission_id: draft.mission_id.clone(),
            revision: draft.revision,
            contract_hash: contract_hash.clone(),
            manifest_digest: manifest_digest(&manifest)?,
            project_id: draft.project_id.clone(),
            project_source_revision: draft.project_source_revision.clone(),
            blueprint_or_takeover_fingerprint: draft.blueprint_or_takeover_fingerprint.clone(),
            requirement_hashes,
            manifest,
            scope_fingerprint: draft.scope.value.clone(),
            sealed_at: sealed_at.into(),
            schema_version: P6_SCHEMA_VERSION,
            state: "SEALED".into(),
        };
        let revision = MissionRevision {
            revision: draft.revision,
            contract,
            seal,
        };
        let executable_task_ids = revision
            .contract
            .task_graph
            .tasks
            .iter()
            .filter(|task| {
                !matches!(
                    task.status,
                    RequirementStatus::DeferredByExplicitDecision
                        | RequirementStatus::NotApplicable
                )
            })
            .map(|task| task.task_id.as_str())
            .collect::<BTreeSet<_>>();
        let task_order = revision
            .contract
            .task_graph
            .topological_order()?
            .into_iter()
            .filter(|task_id| executable_task_ids.contains(task_id.as_str()))
            .collect();
        let handoff = ExecutionHandoff {
            mission_id: draft.mission_id.clone(),
            revision: draft.revision,
            contract_hash,
            state: "READY_FOR_EXECUTION".into(),
            task_order,
            scheduler_owner: "P7_NOT_STARTED".into(),
        };
        Ok((revision, handoff))
    }

    pub fn validate_seal(
        &self,
        revision: &MissionRevision,
    ) -> Result<SealValidationResult, AuthorityError> {
        if !seal_integrity_reasons(revision)?.is_empty() {
            return Ok(SealValidationResult {
                valid: false,
                state: "REVALIDATION_REQUIRED".into(),
                reasons: vec![RevalidationReason::ContractHashChanged],
            });
        }
        Ok(SealValidationResult {
            valid: true,
            state: "SEALED".into(),
            reasons: Vec::new(),
        })
    }

    pub fn revalidate(
        &self,
        sealed: &MissionRevision,
        candidate: &MissionContract,
    ) -> Result<RevalidationResult, AuthorityError> {
        let old = &sealed.contract;
        let mut reasons = Vec::new();
        if old.registry_digest != candidate.registry_digest {
            reasons.push(RevalidationReason::RegistryChanged);
        }
        if old.scope != candidate.scope {
            reasons.push(RevalidationReason::ScopeChanged);
        }
        let old_requirements = old
            .requirement_graph
            .requirements
            .iter()
            .map(|requirement| (&requirement.requirement_id, requirement))
            .collect::<BTreeMap<_, _>>();
        let new_requirements = candidate
            .requirement_graph
            .requirements
            .iter()
            .map(|requirement| (&requirement.requirement_id, requirement))
            .collect::<BTreeMap<_, _>>();
        for (id, requirement) in &old_requirements {
            match new_requirements.get(id) {
                None => reasons.push(RevalidationReason::RequirementRemoved((*id).clone())),
                Some(candidate_requirement) if *candidate_requirement != *requirement => {
                    reasons.push(RevalidationReason::RequirementChanged((*id).clone()))
                }
                _ => {}
            }
        }
        if old.task_graph != candidate.task_graph {
            reasons.push(RevalidationReason::TaskCoverageChanged(
                "task graph changed".into(),
            ));
        }
        if old.decisions != candidate.decisions {
            reasons.push(RevalidationReason::DecisionChanged(
                "explicit decisions changed".into(),
            ));
        }
        if old.canonical_hash()? != candidate.canonical_hash()? {
            reasons.push(RevalidationReason::ContractHashChanged);
        }
        reasons.sort_by_key(|reason| format!("{reason:?}"));
        reasons.dedup();
        if reasons.is_empty() {
            Ok(RevalidationResult {
                status: "UNCHANGED".into(),
                reasons,
                new_revision_required: false,
            })
        } else {
            Ok(RevalidationResult {
                status: "REVALIDATION_REQUIRED".into(),
                reasons,
                new_revision_required: true,
            })
        }
    }
}

fn validate_decisions(
    registry: &StandardsRegistry,
    graph: &RequirementGraph,
    decisions: &[ExplicitDecision],
) -> Result<(), AuthorityError> {
    let requirements = graph
        .requirements
        .iter()
        .map(|requirement| (requirement.requirement_id.as_str(), requirement))
        .collect::<BTreeMap<_, _>>();
    let rules = registry
        .packs
        .iter()
        .flat_map(|pack| pack.rules.iter())
        .map(|rule| (rule.rule_id.as_str(), rule))
        .collect::<BTreeMap<_, _>>();
    let mut decision_ids = BTreeSet::new();
    for decision in decisions {
        if !decision_ids.insert(decision.decision_id.clone())
            || decision.requirement_id.trim().is_empty()
            || decision.reason.trim().is_empty()
            || decision.provenance.trim().is_empty()
            || decision.timestamp.trim().is_empty()
            || decision.mission_revision == 0
        {
            return Err(AuthorityError::InvalidDecision(
                decision.decision_id.clone(),
            ));
        }
        if !matches!(decision.actor, DecisionActor::Human | DecisionActor::User)
            && decision.kind == DecisionKind::Defer
        {
            return Err(AuthorityError::InvalidDecision(format!(
                "{}: defer requires human or user provenance",
                decision.decision_id
            )));
        }
        let requirement = requirements
            .get(decision.requirement_id.as_str())
            .ok_or_else(|| AuthorityError::MissingRequirement(decision.requirement_id.clone()))?;
        let Some(rule_id) = requirement.origin_rule_id.as_deref() else {
            if matches!(
                decision.kind,
                DecisionKind::NotApplicable | DecisionKind::ExplicitException
            ) || (decision.kind == DecisionKind::Defer
                && !matches!(decision.actor, DecisionActor::Human | DecisionActor::User))
            {
                return Err(AuthorityError::InvalidDecision(
                    decision.decision_id.clone(),
                ));
            }
            continue;
        };
        let rule = rules
            .get(rule_id)
            .ok_or_else(|| AuthorityError::CriticalRuleMissing(rule_id.into()))?;
        match decision.kind {
            DecisionKind::Defer if rule.exception_policy == ExceptionPolicy::NonWaivable => {
                return Err(AuthorityError::InvalidDecision(format!(
                    "{}: non-waivable rule cannot be deferred",
                    decision.decision_id
                )))
            }
            DecisionKind::Defer => {}
            DecisionKind::NotApplicable
                if rule.exception_policy == ExceptionPolicy::ProjectNotApplicable => {}
            DecisionKind::ExplicitException
                if rule.exception_policy == ExceptionPolicy::ExplicitExceptionAllowed => {}
            DecisionKind::NotApplicable | DecisionKind::ExplicitException => {
                return Err(AuthorityError::InvalidDecision(format!(
                    "{}: critical or non-waivable rule cannot be dismissed",
                    decision.decision_id
                )))
            }
        }
    }
    Ok(())
}

impl RequirementGraph {
    pub fn validate(&self) -> Result<(), AuthorityError> {
        let mut ids = BTreeSet::new();
        for requirement in &self.requirements {
            if !ids.insert(requirement.requirement_id.clone()) {
                return Err(AuthorityError::DuplicateRequirement(
                    requirement.requirement_id.clone(),
                ));
            }
        }
        let mut edges = BTreeSet::new();
        for dependency in &self.dependencies {
            if !ids.contains(&dependency.requirement_id) {
                return Err(AuthorityError::MissingRequirement(
                    dependency.requirement_id.clone(),
                ));
            }
            if !ids.contains(&dependency.depends_on) {
                return Err(AuthorityError::MissingRequirement(
                    dependency.depends_on.clone(),
                ));
            }
            if dependency.requirement_id == dependency.depends_on {
                return Err(AuthorityError::SelfDependency(
                    dependency.requirement_id.clone(),
                ));
            }
            if !edges.insert((
                dependency.requirement_id.clone(),
                dependency.depends_on.clone(),
            )) {
                return Err(AuthorityError::DuplicateDependency(format!(
                    "{} -> {}",
                    dependency.requirement_id, dependency.depends_on
                )));
            }
        }
        detect_cycles(&ids, &edges, false)
    }

    pub fn deterministic_order(&self) -> Result<Vec<String>, AuthorityError> {
        self.validate()?;
        let ids = self
            .requirements
            .iter()
            .map(|requirement| requirement.requirement_id.clone())
            .collect::<BTreeSet<_>>();
        let edges = self
            .dependencies
            .iter()
            .map(|dependency| {
                (
                    dependency.requirement_id.clone(),
                    dependency.depends_on.clone(),
                )
            })
            .collect::<BTreeSet<_>>();
        topological_order(&ids, &edges)
    }
}

impl TaskGraph {
    pub fn validate(&self, requirements: &RequirementGraph) -> Result<(), AuthorityError> {
        let mut ids = BTreeSet::new();
        let requirement_ids = requirements
            .requirements
            .iter()
            .map(|requirement| requirement.requirement_id.clone())
            .collect::<BTreeSet<_>>();
        for task in &self.tasks {
            if !ids.insert(task.task_id.clone()) {
                return Err(AuthorityError::DuplicateTask(task.task_id.clone()));
            }
            if task.requirement_ids.is_empty() {
                return Err(AuthorityError::OrphanTask(task.task_id.clone()));
            }
            for requirement_id in &task.requirement_ids {
                if !requirement_ids.contains(requirement_id) {
                    return Err(AuthorityError::MissingRequirement(requirement_id.clone()));
                }
            }
        }
        let mut edges = BTreeSet::new();
        for dependency in &self.dependencies {
            if !ids.contains(&dependency.task_id) || !ids.contains(&dependency.depends_on) {
                return Err(AuthorityError::MissingTask(format!(
                    "{} -> {}",
                    dependency.task_id, dependency.depends_on
                )));
            }
            if dependency.task_id == dependency.depends_on {
                return Err(AuthorityError::SelfDependency(dependency.task_id.clone()));
            }
            if !edges.insert((dependency.task_id.clone(), dependency.depends_on.clone())) {
                return Err(AuthorityError::DuplicateDependency(format!(
                    "{} -> {}",
                    dependency.task_id, dependency.depends_on
                )));
            }
        }
        detect_cycles(&ids, &edges, true)?;
        let mut seen_links = BTreeSet::new();
        for link in &self.links {
            if !ids.contains(&link.task_id) {
                return Err(AuthorityError::MissingTask(link.task_id.clone()));
            }
            if !requirement_ids.contains(&link.requirement_id) {
                return Err(AuthorityError::MissingRequirement(
                    link.requirement_id.clone(),
                ));
            }
            if !seen_links.insert((link.task_id.clone(), link.requirement_id.clone())) {
                return Err(AuthorityError::DuplicateDependency(format!(
                    "duplicate task link {} -> {}",
                    link.task_id, link.requirement_id
                )));
            }
            let task = self
                .tasks
                .iter()
                .find(|task| task.task_id == link.task_id)
                .expect("validated task ID");
            if !task.requirement_ids.contains(&link.requirement_id) {
                return Err(AuthorityError::MissingTaskCoverage(format!(
                    "task {} does not declare linked requirement {}",
                    link.task_id, link.requirement_id
                )));
            }
        }
        for task in &self.tasks {
            for requirement_id in &task.requirement_ids {
                if !seen_links.contains(&(task.task_id.clone(), requirement_id.clone())) {
                    return Err(AuthorityError::MissingTaskCoverage(format!(
                        "task {} has an unlinked requirement {}",
                        task.task_id, requirement_id
                    )));
                }
            }
            let declared = task.dependency_ids.iter().cloned().collect::<BTreeSet<_>>();
            let authoritative = self
                .dependencies
                .iter()
                .filter(|dependency| dependency.task_id == task.task_id)
                .map(|dependency| dependency.depends_on.clone())
                .collect::<BTreeSet<_>>();
            if declared != authoritative {
                return Err(AuthorityError::MissingTaskCoverage(format!(
                    "task {} dependency declarations disagree with task graph",
                    task.task_id
                )));
            }
        }
        for requirement in requirements
            .requirements
            .iter()
            .filter(|requirement| requirement.applicability == ApplicabilityOutcome::Applicable)
        {
            if !self
                .links
                .iter()
                .any(|link| link.requirement_id == requirement.requirement_id)
            {
                return Err(AuthorityError::MissingTaskCoverage(
                    requirement.requirement_id.clone(),
                ));
            }
        }
        Ok(())
    }

    pub fn topological_order(&self) -> Result<Vec<String>, AuthorityError> {
        let ids = self
            .tasks
            .iter()
            .map(|task| task.task_id.clone())
            .collect::<BTreeSet<_>>();
        let edges = self
            .dependencies
            .iter()
            .map(|dependency| (dependency.task_id.clone(), dependency.depends_on.clone()))
            .collect::<BTreeSet<_>>();
        topological_order(&ids, &edges)
    }
}

impl MissionContract {
    pub fn from_draft(draft: &MissionDraft) -> Result<Self, AuthorityError> {
        Ok(Self {
            mission_id: draft.mission_id.clone(),
            project_id: draft.project_id.clone(),
            project_source_revision: draft.project_source_revision.clone(),
            blueprint_or_takeover_fingerprint: draft.blueprint_or_takeover_fingerprint.clone(),
            registry_id: draft.registry.registry_id.clone(),
            registry_version: draft.registry.registry_version,
            registry_digest: draft.registry.registry_digest.clone(),
            authority_manifest: manifest_for(&draft.registry),
            applicability: draft.applicability.clone(),
            applicability_context: draft.applicability_context.clone(),
            applicability_context_digest: draft.applicability_context_digest.clone(),
            project_authority_fingerprint: draft.project_authority_fingerprint.clone(),
            project_authority: draft.project_authority.clone(),
            project_authority_digest: draft.project_authority_digest.clone(),
            requirement_graph: draft.requirement_graph.clone(),
            task_graph: draft.task_graph.clone(),
            decisions: draft.decisions.clone(),
            scope: draft.scope.clone(),
            architecture_decisions: draft.architecture_decisions.clone(),
            revision: draft.revision,
            schema_version: draft.schema_version,
        })
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, AuthorityError> {
        let mut normalized = self.clone();
        normalized
            .applicability
            .sort_by(|a, b| a.rule_id.cmp(&b.rule_id));
        normalized
            .requirement_graph
            .requirements
            .sort_by(|a, b| a.requirement_id.cmp(&b.requirement_id));
        normalized.requirement_graph.dependencies.sort_by(|a, b| {
            (a.requirement_id.as_str(), a.depends_on.as_str())
                .cmp(&(b.requirement_id.as_str(), b.depends_on.as_str()))
        });
        normalized
            .task_graph
            .tasks
            .sort_by(|a, b| a.task_id.cmp(&b.task_id));
        normalized.task_graph.dependencies.sort_by(|a, b| {
            (a.task_id.as_str(), a.depends_on.as_str())
                .cmp(&(b.task_id.as_str(), b.depends_on.as_str()))
        });
        normalized.task_graph.links.sort_by(|a, b| {
            (a.task_id.as_str(), a.requirement_id.as_str())
                .cmp(&(b.task_id.as_str(), b.requirement_id.as_str()))
        });
        normalized
            .decisions
            .sort_by(|a, b| a.decision_id.cmp(&b.decision_id));
        normalized.architecture_decisions.sort();
        serde_json::to_vec(&normalized)
            .map_err(|error| AuthorityError::Canonicalization(error.to_string()))
    }

    pub fn canonical_hash(&self) -> Result<String, AuthorityError> {
        Ok(sha256_hex(&self.canonical_bytes()?))
    }
}

fn detect_cycles(
    ids: &BTreeSet<String>,
    edges: &BTreeSet<(String, String)>,
    tasks: bool,
) -> Result<(), AuthorityError> {
    let mut adjacency = BTreeMap::<String, Vec<String>>::new();
    for id in ids {
        adjacency.insert(id.clone(), Vec::new());
    }
    for (from, to) in edges {
        adjacency.entry(from.clone()).or_default().push(to.clone());
    }
    let mut visiting = BTreeSet::new();
    let mut visited = BTreeSet::new();
    for id in ids {
        let mut path = Vec::new();
        if let Some(cycle) = visit_cycle(id, &adjacency, &mut visiting, &mut visited, &mut path) {
            return if tasks {
                Err(AuthorityError::TaskCycle(cycle))
            } else {
                Err(AuthorityError::DependencyCycle(cycle))
            };
        }
    }
    Ok(())
}

fn visit_cycle(
    id: &str,
    adjacency: &BTreeMap<String, Vec<String>>,
    visiting: &mut BTreeSet<String>,
    visited: &mut BTreeSet<String>,
    path: &mut Vec<String>,
) -> Option<Vec<String>> {
    if visiting.contains(id) {
        let start = path
            .iter()
            .position(|value| value == id)
            .unwrap_or_default();
        return Some(path[start..].to_vec());
    }
    if visited.contains(id) {
        return None;
    }
    visiting.insert(id.into());
    path.push(id.into());
    for next in adjacency.get(id).into_iter().flatten() {
        if let Some(cycle) = visit_cycle(next, adjacency, visiting, visited, path) {
            return Some(cycle);
        }
    }
    path.pop();
    visiting.remove(id);
    visited.insert(id.into());
    None
}

fn topological_order(
    ids: &BTreeSet<String>,
    edges: &BTreeSet<(String, String)>,
) -> Result<Vec<String>, AuthorityError> {
    let mut incoming = ids
        .iter()
        .map(|id| (id.clone(), 0_usize))
        .collect::<BTreeMap<_, _>>();
    let mut outgoing = BTreeMap::<String, Vec<String>>::new();
    for (from, to) in edges {
        *incoming.entry(from.clone()).or_default() += 1;
        outgoing.entry(to.clone()).or_default().push(from.clone());
    }
    let mut ready = incoming
        .iter()
        .filter_map(|(id, count)| (*count == 0).then_some(id.clone()))
        .collect::<BTreeSet<_>>();
    let mut result = Vec::new();
    while let Some(id) = ready.pop_first() {
        result.push(id.clone());
        for dependent in outgoing.get(&id).into_iter().flatten() {
            let count = incoming
                .get_mut(dependent)
                .expect("graph node was initialized");
            *count -= 1;
            if *count == 0 {
                ready.insert(dependent.clone());
            }
        }
    }
    if result.len() != ids.len() {
        return Err(AuthorityError::DependencyCycle(result));
    }
    Ok(result)
}

fn requirement_from_rule(
    registry: &StandardsRegistry,
    pack: &StandardsPack,
    rule: &StandardRule,
    requirement_id: &str,
) -> Requirement {
    let risk = match rule.severity {
        RuleSeverity::Critical => RequirementRisk::Critical,
        RuleSeverity::High => RequirementRisk::High,
        RuleSeverity::Medium => RequirementRisk::Medium,
        RuleSeverity::Low => RequirementRisk::Low,
    };
    let priority = match rule.severity {
        RuleSeverity::Critical => RequirementPriority::P0,
        RuleSeverity::High => RequirementPriority::P1,
        RuleSeverity::Medium => RequirementPriority::P2,
        RuleSeverity::Low => RequirementPriority::P3,
    };
    let rule_digest = sha256_hex(&serde_json::to_vec(rule).unwrap_or_default());
    Requirement {
        requirement_id: requirement_id.into(),
        title: rule.title.clone(),
        intent: rule.rationale.clone(),
        source: RequirementSource::Standard {
            registry_id: registry.registry_id.clone(),
            registry_version: registry.registry_version,
            pack_id: pack.pack_id.clone(),
            rule_id: rule.rule_id.clone(),
            rule_digest,
            applicability_id: stable_id("applicability", &[&rule.rule_id]),
        },
        priority,
        applicability: ApplicabilityOutcome::Applicable,
        acceptance_criteria: vec![AcceptanceCriterion {
            criterion_id: stable_id("criterion", &[requirement_id]),
            statement: rule.acceptance_template.clone(),
            criterion_type: "deterministic-obligation".into(),
            machine_checkable: true,
        }],
        verification_policy: VerificationPolicy {
            obligations: rule
                .evidence_requirements
                .iter()
                .map(|class| EvidenceObligation {
                    class: *class,
                    minimum_confidence: EvidenceConfidence::StrongDeterministic,
                    rationale: format!("required by {}", rule.rule_id),
                    required: true,
                })
                .collect(),
            p8_collector_required: true,
        },
        dependencies: Vec::new(),
        risk,
        status: RequirementStatus::Unstarted,
        implementation_links: Vec::new(),
        evidence_links: Vec::new(),
        explicit_exceptions: Vec::new(),
        sealed_hash: None,
        requirement_type: pack.domain.id().into(),
        origin_rule_id: Some(rule.rule_id.clone()),
        revision: 1,
        schema_version: P6_SCHEMA_VERSION,
    }
}

fn requirement_from_project_seed(
    seed: &ProjectRequirementSeed,
    requirement_id: &str,
) -> Requirement {
    Requirement {
        requirement_id: requirement_id.into(),
        title: seed.title.clone(),
        intent: seed.intent.clone(),
        source: seed.source.clone(),
        priority: seed.priority,
        applicability: ApplicabilityOutcome::Applicable,
        acceptance_criteria: seed.acceptance_criteria.clone(),
        verification_policy: seed.verification_policy.clone(),
        dependencies: seed.dependencies.clone(),
        risk: seed.risk,
        status: RequirementStatus::Unstarted,
        implementation_links: Vec::new(),
        evidence_links: Vec::new(),
        explicit_exceptions: Vec::new(),
        sealed_hash: None,
        requirement_type: if seed.requirement_type.is_empty() {
            "project-authority".into()
        } else {
            seed.requirement_type.clone()
        },
        origin_rule_id: None,
        revision: 1,
        schema_version: P6_SCHEMA_VERSION,
    }
}

fn project_requirement_id(project_id: &str, seed: &ProjectRequirementSeed) -> String {
    match seed.requirement_id.as_deref() {
        Some(id) if canonical_requirement_id_shape(id) => id.into(),
        Some(id) if id.starts_with("project-") => stable_id("requirement", &[project_id, id]),
        Some(id) => id.into(),
        None => stable_id(
            "requirement",
            &[project_id, &seed.title, &format!("{:?}", seed.source)],
        ),
    }
}

fn canonical_requirement_id_shape(id: &str) -> bool {
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

fn canonicalize_project_authority(
    project_id: &str,
    mut authority: ProjectAuthorityInput,
    standard_requirement_ids: &BTreeSet<String>,
) -> Result<(ProjectAuthorityInput, BTreeMap<String, String>), AuthorityError> {
    let mut aliases = BTreeMap::new();
    let mut canonical_ids = BTreeSet::new();
    for seed in &authority.requirements {
        let canonical_id = project_requirement_id(project_id, seed);
        if !canonical_ids.insert(canonical_id.clone()) {
            return Err(AuthorityError::DuplicateRequirement(canonical_id));
        }
        aliases.insert(canonical_id.clone(), canonical_id.clone());
        if let Some(alias) = seed.requirement_id.as_ref() {
            if let Some(previous) = aliases.insert(alias.clone(), canonical_id.clone()) {
                if previous != canonical_id {
                    return Err(AuthorityError::DuplicateRequirement(alias.clone()));
                }
            }
        }
    }

    for seed in &mut authority.requirements {
        let canonical_id = project_requirement_id(project_id, seed);
        seed.requirement_id = Some(canonical_id);
        for dependency in &mut seed.dependencies {
            *dependency =
                resolve_requirement_reference(dependency, &aliases, standard_requirement_ids)?;
        }
    }
    for decision in &mut authority.decisions {
        decision.requirement_id = resolve_decision_reference(
            &decision.requirement_id,
            &aliases,
            standard_requirement_ids,
        );
    }
    Ok((authority, aliases))
}

fn resolve_requirement_reference(
    reference: &str,
    project_requirement_aliases: &BTreeMap<String, String>,
    standard_requirement_ids: &BTreeSet<String>,
) -> Result<String, AuthorityError> {
    if let Some(canonical) = project_requirement_aliases.get(reference) {
        return Ok(canonical.clone());
    }
    if standard_requirement_ids.contains(reference) {
        return Ok(reference.into());
    }
    Err(AuthorityError::MissingRequirement(reference.into()))
}

fn resolve_decision_reference(
    reference: &str,
    project_requirement_aliases: &BTreeMap<String, String>,
    standard_requirement_ids: &BTreeSet<String>,
) -> String {
    project_requirement_aliases
        .get(reference)
        .cloned()
        .or_else(|| {
            standard_requirement_ids
                .contains(reference)
                .then(|| reference.into())
        })
        .unwrap_or_else(|| reference.into())
}

fn project_authority_notes(input: &ProjectAuthorityInput) -> Vec<String> {
    let mut notes = input.architecture_decisions.clone();
    notes.extend(
        input
            .assumptions
            .iter()
            .map(|value| format!("ASSUMPTION: {value}")),
    );
    notes.extend(input.risks.iter().map(|value| format!("RISK: {value}")));
    if !input.source_revision.is_empty() {
        notes.push(format!(
            "PROJECT_AUTHORITY_REVISION: {}",
            input.source_revision
        ));
    }
    notes
}

fn project_authority_digest(input: &ProjectAuthorityInput) -> Result<String, AuthorityError> {
    serde_json::to_vec(input)
        .map(|bytes| sha256_hex(&bytes))
        .map_err(|error| AuthorityError::Canonicalization(error.to_string()))
}

fn applicability_context_digest(context: &ApplicabilityContext) -> Result<String, AuthorityError> {
    serde_json::to_vec(context)
        .map(|bytes| sha256_hex(&bytes))
        .map_err(|error| AuthorityError::Canonicalization(error.to_string()))
}

fn manifest_for(registry: &StandardsRegistry) -> MissionSealManifest {
    let mut pack_versions = BTreeMap::new();
    let mut rule_ids = Vec::new();
    let mut rule_digests = BTreeMap::new();
    for pack in &registry.packs {
        pack_versions.insert(pack.pack_id.clone(), pack.pack_version.clone());
        for rule in &pack.rules {
            rule_ids.push(rule.rule_id.clone());
            rule_digests.insert(
                rule.rule_id.clone(),
                sha256_hex(&serde_json::to_vec(rule).unwrap_or_default()),
            );
        }
    }
    rule_ids.sort();
    MissionSealManifest {
        registry_id: registry.registry_id.clone(),
        registry_version: registry.registry_version,
        registry_digest: registry.registry_digest.clone(),
        pack_versions,
        rule_ids,
        rule_digests,
    }
}

fn requirement_sealed_hash(requirement: &Requirement) -> Result<String, AuthorityError> {
    let mut normalized = requirement.clone();
    normalized.sealed_hash = None;
    serde_json::to_vec(&normalized)
        .map(|bytes| sha256_hex(&bytes))
        .map_err(|error| AuthorityError::Canonicalization(error.to_string()))
}

fn manifest_digest(manifest: &MissionSealManifest) -> Result<String, AuthorityError> {
    serde_json::to_vec(manifest)
        .map(|bytes| sha256_hex(&bytes))
        .map_err(|error| AuthorityError::Canonicalization(error.to_string()))
}

fn seal_integrity_reasons(
    revision: &MissionRevision,
) -> Result<Vec<RevalidationReason>, AuthorityError> {
    let mut reasons = Vec::new();
    let contract = &revision.contract;
    let seal = &revision.seal;
    if seal.state != "SEALED" {
        reasons.push(RevalidationReason::ContractHashChanged);
    }
    if contract.canonical_hash()? != seal.contract_hash {
        reasons.push(RevalidationReason::ContractHashChanged);
    }
    if revision.revision != contract.revision || revision.revision != seal.revision {
        reasons.push(RevalidationReason::ContractHashChanged);
    }
    if contract.mission_id != seal.mission_id || contract.mission_id != revision.seal.mission_id {
        reasons.push(RevalidationReason::ContractHashChanged);
    }
    if contract.project_id != seal.project_id
        || contract.project_source_revision != seal.project_source_revision
        || contract.blueprint_or_takeover_fingerprint != seal.blueprint_or_takeover_fingerprint
        || contract.scope.value != seal.scope_fingerprint
        || contract.schema_version != seal.schema_version
    {
        reasons.push(RevalidationReason::ContractHashChanged);
    }
    if contract.authority_manifest != seal.manifest
        || manifest_digest(&seal.manifest)? != seal.manifest_digest
    {
        reasons.push(RevalidationReason::ContractHashChanged);
    }
    if project_authority_digest(&contract.project_authority)? != contract.project_authority_digest {
        reasons.push(RevalidationReason::ContractHashChanged);
    }
    let mut expected_hashes = BTreeMap::new();
    for requirement in &contract.requirement_graph.requirements {
        let hash = requirement_sealed_hash(requirement)?;
        if requirement.sealed_hash.as_deref() != Some(hash.as_str()) {
            reasons.push(RevalidationReason::RequirementChanged(
                requirement.requirement_id.clone(),
            ));
        }
        expected_hashes.insert(requirement.requirement_id.clone(), hash);
    }
    if expected_hashes != seal.requirement_hashes {
        reasons.push(RevalidationReason::ContractHashChanged);
    }
    reasons.sort_by_key(|reason| format!("{reason:?}"));
    reasons.dedup();
    Ok(reasons)
}

pub fn scope_fingerprint(inputs: BTreeMap<String, String>) -> ScopeFingerprint {
    let bytes = serde_json::to_vec(&inputs).unwrap_or_default();
    ScopeFingerprint {
        algorithm: "sha256".into(),
        value: sha256_hex(&bytes),
        inputs,
    }
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

fn sha256_hex(bytes: &[u8]) -> String {
    hex_lower(&Sha256::digest(bytes))
}

fn hex_lower(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn fact(field: &str) -> ApplicabilityPredicate {
    ApplicabilityPredicate::Equals {
        field: field.into(),
        value: FactValue::Bool(true),
    }
}

fn internal_source(domain: DomainPack) -> StandardSource {
    StandardSource {
        source_id: format!("relintor-internal-{}", domain.id()),
        title: format!("Relintor professional {} rule set", domain.label()),
        uri: None,
        version: "p6-1".into(),
        published_or_reviewed: "2026-08-14".into(),
        verification: SourceVerification::InternalRelintorRule,
        rationale: "Structured internal rule with deterministic applicability and explicit evidence obligations.".into(),
    }
}

fn make_rule(
    domain: DomainPack,
    index: u8,
    critical: bool,
    title: &str,
    predicate: ApplicabilityPredicate,
) -> StandardRule {
    let severity = if critical {
        RuleSeverity::Critical
    } else {
        RuleSeverity::High
    };
    StandardRule {
        rule_id: format!(
            "P6-{}-{index:03}",
            domain.id().to_ascii_uppercase().replace('-', "_")
        ),
        title: title.into(),
        domain,
        rule_version: "1.0.0".into(),
        source: internal_source(domain),
        applicability: predicate,
        severity,
        rationale: format!(
            "The {} surface requires an explicit, reviewable {} obligation.",
            domain.label(),
            title.to_ascii_lowercase()
        ),
        expected_implementation_patterns: vec![
            "documented implementation boundary".into(),
            "deterministic regression check".into(),
        ],
        deterministic_check_hints: vec![
            "record source and test evidence".into(),
            "fail closed when unknown".into(),
        ],
        evidence_requirements: vec![if critical {
            EvidenceClass::SecurityScan
        } else {
            EvidenceClass::TestOutput
        }],
        exception_policy: if critical {
            ExceptionPolicy::NonWaivable
        } else {
            ExceptionPolicy::ExplicitExceptionAllowed
        },
        lifecycle: "active".into(),
        acceptance_template: format!(
            "A deterministic check demonstrates the {} obligation for the {} surface.",
            title.to_ascii_lowercase(),
            domain.label()
        ),
    }
}

pub fn builtin_registry() -> StandardsRegistry {
    let definitions = [
        (
            DomainPack::WebFrontend,
            "frontend rendering and client behavior",
            fact("web"),
        ),
        (
            DomainPack::BackendApi,
            "API contract and error behavior",
            fact("backend"),
        ),
        (
            DomainPack::Databases,
            "database integrity and migration behavior",
            fact("database"),
        ),
        (
            DomainPack::AuthenticationAuthorization,
            "authentication and authorization boundaries",
            fact("authentication"),
        ),
        (
            DomainPack::ApplicationSecurity,
            "application security controls",
            ApplicabilityPredicate::Exists {
                field: "platform".into(),
            },
        ),
        (
            DomainPack::Accessibility,
            "accessible interaction",
            fact("ui_surface"),
        ),
        (
            DomainPack::SeoDiscoverability,
            "crawlable public discoverability",
            fact("seo_relevance"),
        ),
        (
            DomainPack::Performance,
            "measured performance budget",
            fact("performance"),
        ),
        (
            DomainPack::DevopsReleaseEngineering,
            "repeatable release path",
            fact("deployment"),
        ),
        (
            DomainPack::ObservabilityOperations,
            "operational visibility",
            fact("observability"),
        ),
        (
            DomainPack::DataPrivacy,
            "privacy and data handling",
            fact("privacy"),
        ),
        (
            DomainPack::PaymentsFinancialWorkflows,
            "financial integrity and reconciliation",
            fact("payments"),
        ),
        (
            DomainPack::AiMlApplications,
            "AI/ML safety and evaluation",
            fact("ai"),
        ),
        (
            DomainPack::BlockchainWeb3,
            "key and transaction safety",
            fact("blockchain"),
        ),
        (
            DomainPack::Mobile,
            "mobile lifecycle and permissions",
            fact("mobile"),
        ),
        (
            DomainPack::Desktop,
            "desktop packaging and local boundaries",
            fact("desktop"),
        ),
        (
            DomainPack::DataEngineering,
            "data pipeline correctness",
            fact("data_engineering"),
        ),
        (
            DomainPack::ThirdPartyIntegrations,
            "external integration resilience",
            fact("integrations"),
        ),
    ];
    let packs = definitions
        .into_iter()
        .map(|(domain, first_title, predicate)| StandardsPack {
            pack_id: domain.id().into(),
            pack_version: "1.0.0".into(),
            domain,
            title: domain.label().into(),
            source: internal_source(domain),
            rules: vec![
                make_rule(
                    domain,
                    1,
                    !matches!(domain, DomainPack::BackendApi),
                    first_title,
                    predicate.clone(),
                ),
                make_rule(
                    domain,
                    2,
                    false,
                    "documented operational evidence",
                    predicate,
                ),
            ],
        })
        .collect::<Vec<_>>();
    let mut registry = StandardsRegistry {
        registry_id: "relintor-professional-standards".into(),
        registry_version: 1,
        created_at: "2026-08-14".into(),
        schema_version: P6_SCHEMA_VERSION,
        packs,
        pack_counts: BTreeMap::new(),
        pack_digests: BTreeMap::new(),
        registry_digest: String::new(),
        signature: None,
    };
    for pack in &registry.packs {
        registry
            .pack_counts
            .insert(pack.pack_id.clone(), pack.rules.len());
        registry.pack_digests.insert(
            pack.pack_id.clone(),
            sha256_hex(&serde_json::to_vec(pack).unwrap_or_default()),
        );
    }
    registry
}

pub fn sign_production_registry_artifact(
    registry: &StandardsRegistry,
    signer_key_id: &str,
    signing_key: &SigningKey,
) -> Result<ProductionRegistryArtifact, AuthorityError> {
    let mut signed = registry.clone();
    signed.sign(signer_key_id, signing_key)?;
    let envelope = signed
        .signature
        .as_ref()
        .ok_or(AuthorityError::UnsignedRegistry)?;
    Ok(ProductionRegistryArtifact {
        registry_id: signed.registry_id,
        registry_version: signed.registry_version,
        registry_digest: signed.registry_digest,
        signer_key_id: envelope.signer_key_id.clone(),
        algorithm: envelope.algorithm.clone(),
        public_key: BASE64.encode(signing_key.verifying_key().to_bytes()),
        signature: envelope.signature.clone(),
    })
}

pub fn production_registry() -> Result<(StandardsRegistry, TrustedSignerSet), AuthorityError> {
    let artifact: ProductionRegistryArtifact = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../standards/registry/production-registry.json"
    )))
    .map_err(|error| AuthorityError::InvalidRegistry(error.to_string()))?;
    let mut registry = builtin_registry();
    if artifact.registry_id != registry.registry_id
        || artifact.registry_version != registry.registry_version
    {
        return Err(AuthorityError::InvalidRegistry(
            "production registry artifact identity does not match the bundled registry".into(),
        ));
    }
    registry.registry_digest = artifact.registry_digest.clone();
    registry.signature = Some(SignatureEnvelope {
        signer_key_id: artifact.signer_key_id.clone(),
        algorithm: artifact.algorithm.clone(),
        signed_digest: artifact.registry_digest,
        signature: artifact.signature,
    });
    let trusted = TrustedSignerSet {
        signers: vec![TrustedSigner {
            key_id: artifact.signer_key_id,
            algorithm: artifact.algorithm,
            public_key: artifact.public_key,
        }],
    };
    registry.verify(&trusted, true)?;
    Ok((registry, trusted))
}

fn authority_table_is_revision_scoped(
    connection: &Connection,
    table: &str,
    stable_id_column: &str,
) -> Result<bool, AuthorityError> {
    let pragma = match table {
        "authority_requirements" => "PRAGMA table_info(authority_requirements)",
        "authority_tasks" => "PRAGMA table_info(authority_tasks)",
        "authority_decisions" => "PRAGMA table_info(authority_decisions)",
        _ => {
            return Err(AuthorityError::Persistence(format!(
                "unsupported authority table {table}"
            )))
        }
    };
    let mut statement = connection
        .prepare(pragma)
        .map_err(|error| AuthorityError::Persistence(error.to_string()))?;
    let mut rows = statement
        .query([])
        .map_err(|error| AuthorityError::Persistence(error.to_string()))?;
    let mut primary_key = Vec::new();
    while let Some(row) = rows
        .next()
        .map_err(|error| AuthorityError::Persistence(error.to_string()))?
    {
        let name: String = row
            .get(1)
            .map_err(|error| AuthorityError::Persistence(error.to_string()))?;
        let key_position: i64 = row
            .get(5)
            .map_err(|error| AuthorityError::Persistence(error.to_string()))?;
        if key_position > 0 {
            primary_key.push((key_position, name));
        }
    }
    primary_key.sort_by_key(|(position, _)| *position);
    Ok(primary_key.into_iter().map(|(_, name)| name).eq([
        "mission_id".to_string(),
        "revision".to_string(),
        stable_id_column.to_string(),
    ]))
}

/// Migration 007 is applied by relintor-core for normal databases. The
/// standards persistence boundary also upgrades a database created directly
/// from migration 006 so standalone authority clients cannot accidentally
/// retain globally unique requirement/task/decision IDs.
fn ensure_revision_scoped_authority_schema(
    connection: &mut Connection,
) -> Result<(), AuthorityError> {
    let tables = [
        ("authority_requirements", "requirement_id"),
        ("authority_tasks", "task_id"),
        ("authority_decisions", "decision_id"),
    ];
    let mut scoped = Vec::new();
    for (table, id_column) in tables {
        scoped.push(authority_table_is_revision_scoped(
            connection, table, id_column,
        )?);
    }
    if scoped.iter().all(|value| *value) {
        return Ok(());
    }
    if scoped.iter().any(|value| *value) {
        return Err(AuthorityError::Persistence(
            "authority identity tables are only partially migrated to revision scoping".into(),
        ));
    }

    let transaction = connection
        .transaction()
        .map_err(|error| AuthorityError::Persistence(error.to_string()))?;
    transaction
        .execute_batch(
            "ALTER TABLE authority_requirements RENAME TO authority_requirements_legacy_006;
             CREATE TABLE authority_requirements (
                 requirement_id TEXT NOT NULL,
                 mission_id TEXT NOT NULL,
                 revision INTEGER NOT NULL,
                 status TEXT NOT NULL,
                 applicability TEXT NOT NULL,
                 origin_rule_id TEXT,
                 requirement_json TEXT NOT NULL,
                 PRIMARY KEY (mission_id, revision, requirement_id)
             );
             INSERT INTO authority_requirements(requirement_id, mission_id, revision, status, applicability, origin_rule_id, requirement_json)
                 SELECT requirement_id, mission_id, revision, status, applicability, origin_rule_id, requirement_json
                 FROM authority_requirements_legacy_006;
             DROP TABLE authority_requirements_legacy_006;

             ALTER TABLE authority_tasks RENAME TO authority_tasks_legacy_006;
             CREATE TABLE authority_tasks (
                 task_id TEXT NOT NULL,
                 mission_id TEXT NOT NULL,
                 revision INTEGER NOT NULL,
                 status TEXT NOT NULL,
                 task_json TEXT NOT NULL,
                 PRIMARY KEY (mission_id, revision, task_id)
             );
             INSERT INTO authority_tasks(task_id, mission_id, revision, status, task_json)
                 SELECT task_id, mission_id, revision, status, task_json
                 FROM authority_tasks_legacy_006;
             DROP TABLE authority_tasks_legacy_006;

             ALTER TABLE authority_decisions RENAME TO authority_decisions_legacy_006;
             CREATE TABLE authority_decisions (
                 decision_id TEXT NOT NULL,
                 mission_id TEXT NOT NULL,
                 revision INTEGER NOT NULL,
                 decision_json TEXT NOT NULL,
                 PRIMARY KEY (mission_id, revision, decision_id)
             );
             INSERT INTO authority_decisions(decision_id, mission_id, revision, decision_json)
                 SELECT decision_id, mission_id, revision, decision_json
                 FROM authority_decisions_legacy_006;
             DROP TABLE authority_decisions_legacy_006;",
        )
        .map_err(|error| AuthorityError::Persistence(error.to_string()))?;
    transaction
        .commit()
        .map_err(|error| AuthorityError::Persistence(error.to_string()))
}

fn require_persistence_write(changed_rows: usize, label: &str) -> Result<(), AuthorityError> {
    if changed_rows == 0 {
        return Err(AuthorityError::Persistence(format!(
            "conflicting immutable authority row: {label}"
        )));
    }
    Ok(())
}

/// Persist only authority-bearing structured records. Private signing keys are
/// intentionally not accepted by this boundary and never enter SQLite.
pub fn persist_authority(
    path: &Path,
    registry: &StandardsRegistry,
    revision: &MissionRevision,
) -> Result<(), AuthorityError> {
    registry.validate_schema()?;
    if registry.registry_id != revision.contract.registry_id
        || registry.registry_version != revision.contract.registry_version
        || registry.registry_digest != revision.contract.registry_digest
    {
        return Err(AuthorityError::Persistence(
            "persisted registry does not match the mission contract".into(),
        ));
    }
    let integrity_reasons = seal_integrity_reasons(revision)?;
    if !integrity_reasons.is_empty() {
        return Err(AuthorityError::Persistence(format!(
            "mission revision failed seal integrity validation: {integrity_reasons:?}"
        )));
    }
    revision.contract.requirement_graph.validate()?;
    revision
        .contract
        .task_graph
        .validate(&revision.contract.requirement_graph)?;
    let mut connection =
        Connection::open(path).map_err(|error| AuthorityError::Persistence(error.to_string()))?;
    ensure_revision_scoped_authority_schema(&mut connection)?;
    let transaction = connection
        .transaction()
        .map_err(|error| AuthorityError::Persistence(error.to_string()))?;
    let registry_json = serde_json::to_string(registry)
        .map_err(|error| AuthorityError::Persistence(error.to_string()))?;
    let changed_rows = transaction
        .execute(
            "INSERT INTO standards_registries(registry_id, registry_version, schema_version, registry_digest, signer_key_id, signature_algorithm, signature, created_at, registry_json) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9) ON CONFLICT(registry_id, registry_version) DO UPDATE SET schema_version=excluded.schema_version, registry_digest=excluded.registry_digest, signer_key_id=excluded.signer_key_id, signature_algorithm=excluded.signature_algorithm, signature=excluded.signature, created_at=excluded.created_at, registry_json=excluded.registry_json WHERE standards_registries.registry_json = excluded.registry_json",
            params![
                &registry.registry_id,
                registry.registry_version,
                registry.schema_version,
                &registry.registry_digest,
                registry.signature.as_ref().map(|signature| signature.signer_key_id.clone()),
                registry.signature.as_ref().map(|signature| signature.algorithm.clone()),
                registry.signature.as_ref().map(|signature| signature.signature.clone()),
                &registry.created_at,
                registry_json,
            ],
        )
        .map_err(|error| AuthorityError::Persistence(error.to_string()))?;
    require_persistence_write(changed_rows, "standards registry")?;
    for pack in &registry.packs {
        let changed_rows = transaction
            .execute(
                "INSERT INTO standards_packs(registry_id, registry_version, pack_id, pack_version, domain, pack_digest, rule_count) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7) ON CONFLICT(registry_id, registry_version, pack_id) DO UPDATE SET pack_version=excluded.pack_version, domain=excluded.domain, pack_digest=excluded.pack_digest, rule_count=excluded.rule_count WHERE standards_packs.pack_version = excluded.pack_version AND standards_packs.domain = excluded.domain AND standards_packs.pack_digest = excluded.pack_digest AND standards_packs.rule_count = excluded.rule_count",
                params![
                    &registry.registry_id,
                    registry.registry_version,
                    &pack.pack_id,
                    &pack.pack_version,
                    format!("{:?}", pack.domain),
                    registry.pack_digests.get(&pack.pack_id),
                    pack.rules.len(),
                ],
            )
            .map_err(|error| AuthorityError::Persistence(error.to_string()))?;
        require_persistence_write(changed_rows, "standards pack")?;
        for rule in &pack.rules {
            let changed_rows = transaction
            .execute(
                    "INSERT INTO standards_rules(registry_id, registry_version, pack_id, rule_id, rule_version, severity, applicability, rule_json) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8) ON CONFLICT(registry_id, registry_version, rule_id) DO UPDATE SET pack_id=excluded.pack_id, rule_version=excluded.rule_version, severity=excluded.severity, applicability=excluded.applicability, rule_json=excluded.rule_json WHERE standards_rules.pack_id = excluded.pack_id AND standards_rules.rule_version = excluded.rule_version AND standards_rules.severity = excluded.severity AND standards_rules.applicability = excluded.applicability AND standards_rules.rule_json = excluded.rule_json",
                    params![
                        &registry.registry_id,
                        registry.registry_version,
                        &pack.pack_id,
                        &rule.rule_id,
                        &rule.rule_version,
                        format!("{:?}", rule.severity),
                        serde_json::to_string(&rule.applicability).unwrap_or_default(),
                        serde_json::to_string(rule).unwrap_or_default(),
                    ],
                )
            .map_err(|error| AuthorityError::Persistence(error.to_string()))?;
            require_persistence_write(changed_rows, "standards rule")?;
        }
    }
    for evaluation in &revision.contract.applicability {
        let evaluation_id = stable_id(
            "applicability",
            &[
                &revision.seal.mission_id,
                &revision.revision.to_string(),
                &evaluation.rule_id,
            ],
        );
        transaction
            .execute(
                "INSERT INTO applicability_evaluations(evaluation_id, mission_id, rule_id, outcome, reason, facts_json, predicate_version, evaluated_at_revision, requirement_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    evaluation_id,
                    &revision.seal.mission_id,
                    &evaluation.rule_id,
                    format!("{:?}", evaluation.outcome),
                    &evaluation.reason,
                    serde_json::to_string(&evaluation.facts_used).unwrap_or_default(),
                    &evaluation.predicate_version,
                    &evaluation.evaluated_at_revision,
                    &evaluation.requirement_id,
                ],
            )
            .map_err(|error| AuthorityError::Persistence(error.to_string()))?;
    }
    let contract_json = serde_json::to_string(&revision.contract)
        .map_err(|error| AuthorityError::Persistence(error.to_string()))?;
    transaction
        .execute(
            "INSERT INTO mission_drafts(mission_id, revision, project_id, scope_fingerprint, draft_json) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                &revision.seal.mission_id,
                revision.revision,
                &revision.contract.project_id,
                &revision.contract.scope.value,
                &contract_json,
            ],
        )
        .map_err(|error| AuthorityError::Persistence(error.to_string()))?;
    let changed_rows = transaction
        .execute(
            "INSERT INTO mission_revisions(mission_id, revision, contract_hash, contract_json, created_at) VALUES (?1, ?2, ?3, ?4, ?5) ON CONFLICT(mission_id, revision) DO UPDATE SET contract_hash=excluded.contract_hash, contract_json=excluded.contract_json, created_at=excluded.created_at WHERE mission_revisions.contract_hash = excluded.contract_hash AND mission_revisions.contract_json = excluded.contract_json AND mission_revisions.created_at = excluded.created_at",
            params![
                &revision.seal.mission_id,
                revision.revision,
                &revision.seal.contract_hash,
                contract_json,
                &revision.seal.sealed_at,
            ],
        )
        .map_err(|error| AuthorityError::Persistence(error.to_string()))?;
    require_persistence_write(changed_rows, "mission revision")?;
    let seal_json = serde_json::to_string(&revision.seal)
        .map_err(|error| AuthorityError::Persistence(error.to_string()))?;
    let manifest_json = serde_json::to_string(&revision.seal.manifest)
        .map_err(|error| AuthorityError::Persistence(error.to_string()))?;
    let changed_rows = transaction
        .execute(
            "INSERT INTO mission_seals(mission_id, revision, contract_hash, manifest_json, seal_json) VALUES (?1, ?2, ?3, ?4, ?5) ON CONFLICT(mission_id, revision) DO UPDATE SET contract_hash=excluded.contract_hash, manifest_json=excluded.manifest_json, seal_json=excluded.seal_json WHERE mission_seals.contract_hash = excluded.contract_hash AND mission_seals.manifest_json = excluded.manifest_json AND mission_seals.seal_json = excluded.seal_json",
            params![
                &revision.seal.mission_id,
                revision.revision,
                &revision.seal.contract_hash,
                manifest_json,
                seal_json,
            ],
        )
        .map_err(|error| AuthorityError::Persistence(error.to_string()))?;
    require_persistence_write(changed_rows, "mission seal")?;
    for requirement in &revision.contract.requirement_graph.requirements {
        transaction
            .execute(
                "INSERT INTO authority_requirements(requirement_id, mission_id, revision, status, applicability, origin_rule_id, requirement_json) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    &requirement.requirement_id,
                    &revision.seal.mission_id,
                    revision.revision,
                    format!("{:?}", requirement.status),
                    format!("{:?}", requirement.applicability),
                    &requirement.origin_rule_id,
                    serde_json::to_string(requirement).unwrap_or_default(),
                ],
            )
            .map_err(|error| AuthorityError::Persistence(error.to_string()))?;
        for criterion in &requirement.acceptance_criteria {
            transaction
                .execute(
                    "INSERT INTO authority_acceptance_criteria(mission_id, revision, requirement_id, criterion_id, criterion_json) VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![
                        &revision.seal.mission_id,
                        revision.revision,
                        &requirement.requirement_id,
                        &criterion.criterion_id,
                        serde_json::to_string(criterion).unwrap_or_default(),
                    ],
                )
                .map_err(|error| AuthorityError::Persistence(error.to_string()))?;
        }
        for obligation in &requirement.verification_policy.obligations {
            transaction
                .execute(
                    "INSERT INTO authority_evidence_policies(mission_id, revision, requirement_id, evidence_class, policy_json) VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![
                        &revision.seal.mission_id,
                        revision.revision,
                        &requirement.requirement_id,
                        format!("{:?}", obligation.class),
                        serde_json::to_string(obligation).unwrap_or_default(),
                    ],
                )
                .map_err(|error| AuthorityError::Persistence(error.to_string()))?;
        }
    }
    for dependency in &revision.contract.requirement_graph.dependencies {
        transaction
            .execute(
                "INSERT INTO authority_requirement_dependencies(mission_id, revision, requirement_id, depends_on, edge_json) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    &revision.seal.mission_id,
                    revision.revision,
                    &dependency.requirement_id,
                    &dependency.depends_on,
                    serde_json::to_string(dependency).unwrap_or_default(),
                ],
            )
            .map_err(|error| AuthorityError::Persistence(error.to_string()))?;
    }
    for task in &revision.contract.task_graph.tasks {
        transaction
            .execute(
                "INSERT INTO authority_tasks(task_id, mission_id, revision, status, task_json) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    &task.task_id,
                    &revision.seal.mission_id,
                    revision.revision,
                    format!("{:?}", task.status),
                    serde_json::to_string(task).unwrap_or_default(),
                ],
            )
            .map_err(|error| AuthorityError::Persistence(error.to_string()))?;
    }
    for dependency in &revision.contract.task_graph.dependencies {
        transaction
            .execute(
                "INSERT INTO authority_task_dependencies(mission_id, revision, task_id, depends_on, dependency_json) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    &revision.seal.mission_id,
                    revision.revision,
                    &dependency.task_id,
                    &dependency.depends_on,
                    serde_json::to_string(dependency).unwrap_or_default(),
                ],
            )
            .map_err(|error| AuthorityError::Persistence(error.to_string()))?;
    }
    for link in &revision.contract.task_graph.links {
        transaction
            .execute(
                "INSERT INTO authority_task_links(mission_id, revision, task_id, requirement_id) VALUES (?1, ?2, ?3, ?4)",
                params![
                    &revision.seal.mission_id,
                    revision.revision,
                    &link.task_id,
                    &link.requirement_id,
                ],
            )
            .map_err(|error| AuthorityError::Persistence(error.to_string()))?;
    }
    for decision in &revision.contract.decisions {
        transaction
            .execute(
                "INSERT INTO authority_decisions(decision_id, mission_id, revision, decision_json) VALUES (?1, ?2, ?3, ?4)",
                params![
                    &decision.decision_id,
                    &revision.seal.mission_id,
                    revision.revision,
                    serde_json::to_string(decision).unwrap_or_default(),
                ],
            )
            .map_err(|error| AuthorityError::Persistence(error.to_string()))?;
    }
    transaction
        .commit()
        .map_err(|error| AuthorityError::Persistence(error.to_string()))
}

pub fn load_mission_revision(
    path: &Path,
    mission_id: &str,
    revision: u64,
) -> Result<MissionRevision, AuthorityError> {
    let connection =
        Connection::open(path).map_err(|error| AuthorityError::Persistence(error.to_string()))?;
    let contract_json: String = connection
        .query_row(
            "SELECT contract_json FROM mission_revisions WHERE mission_id = ?1 AND revision = ?2",
            params![mission_id, revision],
            |row| row.get(0),
        )
        .map_err(|error| AuthorityError::Persistence(error.to_string()))?;
    let seal_json: String = connection
        .query_row(
            "SELECT seal_json FROM mission_seals WHERE mission_id = ?1 AND revision = ?2",
            params![mission_id, revision],
            |row| row.get(0),
        )
        .map_err(|error| AuthorityError::Persistence(error.to_string()))?;
    let contract = serde_json::from_str(&contract_json)
        .map_err(|error| AuthorityError::Persistence(error.to_string()))?;
    let seal = serde_json::from_str(&seal_json)
        .map_err(|error| AuthorityError::Persistence(error.to_string()))?;
    let loaded = MissionRevision {
        revision,
        contract,
        seal,
    };
    let integrity_reasons = seal_integrity_reasons(&loaded)?;
    if !integrity_reasons.is_empty() {
        return Err(AuthorityError::Persistence(format!(
            "persisted mission revision failed seal integrity validation: {integrity_reasons:?}"
        )));
    }
    Ok(loaded)
}

pub fn load_registry(
    path: &Path,
    registry_id: &str,
    registry_version: u64,
) -> Result<StandardsRegistry, AuthorityError> {
    let connection =
        Connection::open(path).map_err(|error| AuthorityError::Persistence(error.to_string()))?;
    let registry_json: String = connection
        .query_row(
            "SELECT registry_json FROM standards_registries WHERE registry_id = ?1 AND registry_version = ?2",
            params![registry_id, registry_version],
            |row| row.get(0),
        )
        .map_err(|error| AuthorityError::Persistence(error.to_string()))?;
    let registry: StandardsRegistry = serde_json::from_str(&registry_json)
        .map_err(|error| AuthorityError::Persistence(error.to_string()))?;
    registry.validate_schema()?;
    Ok(registry)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn signed_registry() -> (StandardsRegistry, TrustedSignerSet) {
        let key = SigningKey::from_bytes(&[7_u8; 32]);
        let mut registry = builtin_registry();
        registry.sign("test-key", &key).unwrap();
        let trusted = TrustedSignerSet {
            signers: vec![TrustedSigner {
                key_id: "test-key".into(),
                algorithm: "Ed25519".into(),
                public_key: BASE64.encode(key.verifying_key().to_bytes()),
            }],
        };
        (registry, trusted)
    }

    #[test]
    fn bundled_production_registry_artifact_verifies_strictly() {
        let (registry, trusted) = production_registry().expect("bundled production registry");
        registry
            .verify(&trusted, true)
            .expect("strict trust anchor");
        assert!(registry.signature.is_some());
    }

    #[test]
    fn signed_distribution_binds_artifact_and_rejects_tampering_and_replay() {
        let key = SigningKey::from_bytes(&[7_u8; 32]);
        let (registry, trusted) = signed_registry();
        let artifact = serde_json::to_vec(&registry).unwrap();
        let metadata = create_signed_distribution_metadata(
            &registry,
            &artifact,
            "stable",
            "https://updates.example.test/standards.json",
            "0.1.0",
            "test-key",
            &key,
        )
        .unwrap();

        let verified = verify_signed_distribution(None, &metadata, &artifact, &trusted).unwrap();
        assert_eq!(
            verified.update.registry.registry_version,
            registry.registry_version
        );

        let mut tampered_artifact = artifact.clone();
        tampered_artifact[0] ^= 1;
        assert!(matches!(
            verify_signed_distribution(None, &metadata, &tampered_artifact, &trusted),
            Err(AuthorityError::DistributionArtifactDigestMismatch)
        ));

        assert!(matches!(
            verify_signed_distribution(Some(&registry), &metadata, &artifact, &trusted),
            Err(AuthorityError::RegistryReplay)
        ));
    }

    #[test]
    fn project_authority_ids_are_canonical_and_task_references_are_exact() {
        let (registry, trusted) = signed_registry();
        let raw_parent = "project-nfr-nfr_parent";
        let raw_child = "project-nfr-nfr_child";
        let obligation = EvidenceObligation {
            class: EvidenceClass::TestOutput,
            minimum_confidence: EvidenceConfidence::StrongDeterministic,
            rationale: "deterministic project authority fixture".into(),
            required: true,
        };
        let seed = |id: &str, dependencies: Vec<&str>| ProjectRequirementSeed {
            requirement_id: Some(id.into()),
            title: format!("Requirement {id}"),
            intent: format!("Implement {id}"),
            source: RequirementSource::User {
                reference: format!("project://{id}"),
            },
            priority: RequirementPriority::P2,
            acceptance_criteria: vec![AcceptanceCriterion {
                criterion_id: format!("criterion-{id}"),
                statement: "The project behavior is demonstrated.".into(),
                criterion_type: "functional".into(),
                machine_checkable: true,
            }],
            verification_policy: VerificationPolicy {
                obligations: vec![obligation.clone()],
                p8_collector_required: false,
            },
            dependencies: dependencies.into_iter().map(str::to_owned).collect(),
            risk: RequirementRisk::Medium,
            requirement_type: "functional".into(),
        };
        let draft = AuthorityEngine
            .build_draft_with_project_authority(
                "mission-canonical-project",
                "project-canonical",
                "source-1",
                "scope-1",
                registry,
                &desktop_context(),
                scope_fingerprint(BTreeMap::from([("root".into(), "canonical".into())])),
                ProjectAuthorityInput {
                    requirements: vec![seed(raw_child, vec![raw_parent]), seed(raw_parent, vec![])],
                    source_revision: "source-1".into(),
                    source_fingerprint: "fingerprint-1".into(),
                    ..ProjectAuthorityInput::default()
                },
                Vec::new(),
            )
            .expect("canonical project authority draft");

        assert!(draft
            .requirement_graph
            .requirements
            .iter()
            .filter(|requirement| matches!(requirement.source, RequirementSource::User { .. }))
            .all(|requirement| requirement.requirement_id.starts_with("requirement_")));
        assert!(draft
            .task_graph
            .tasks
            .iter()
            .flat_map(|task| task.requirement_ids.iter())
            .all(|id| canonical_requirement_id_shape(id)));
        assert!(draft
            .requirement_graph
            .dependencies
            .iter()
            .all(|edge| edge.requirement_id != raw_child && edge.depends_on != raw_parent));
        AuthorityEngine
            .preseal(&draft, &trusted)
            .expect("canonical task graph preflight");

        let mut stale = draft.clone();
        stale.task_graph.tasks[0].requirement_ids[0] = raw_child.into();
        assert!(AuthorityEngine.preseal(&stale, &trusted).is_err());
    }

    #[test]
    fn signed_distribution_rejects_invalid_metadata_and_signature() {
        let key = SigningKey::from_bytes(&[7_u8; 32]);
        let (registry, trusted) = signed_registry();
        let artifact = serde_json::to_vec(&registry).unwrap();
        let mut metadata = create_signed_distribution_metadata(
            &registry,
            &artifact,
            "stable",
            "https://updates.example.test/standards.json",
            "0.1.0",
            "test-key",
            &key,
        )
        .unwrap();
        metadata.channel = "unsupported".into();
        assert!(matches!(
            verify_signed_distribution(None, &metadata, &artifact, &trusted),
            Err(AuthorityError::InvalidDistribution(_))
        ));

        let mut metadata = create_signed_distribution_metadata(
            &registry,
            &artifact,
            "stable",
            "https://updates.example.test/standards.json",
            "0.1.0",
            "test-key",
            &key,
        )
        .unwrap();
        metadata.signature.signature = BASE64.encode([0_u8; 64]);
        assert!(matches!(
            verify_signed_distribution(None, &metadata, &artifact, &trusted),
            Err(AuthorityError::InvalidSignature)
        ));
    }

    #[test]
    fn project_authority_is_sealed_accounted_and_persisted_with_provenance() {
        let (registry, trusted) = signed_registry();
        let authority = ProjectAuthorityInput {
            requirements: vec![
                ProjectRequirementSeed {
                    requirement_id: Some("user-requirement-1".into()),
                    title: "User-owned export".into(),
                    intent: "The owner can export a reviewable project record.".into(),
                    source: RequirementSource::User {
                        reference: "project://p1/idea".into(),
                    },
                    priority: RequirementPriority::P1,
                    acceptance_criteria: vec![AcceptanceCriterion {
                        criterion_id: "user-requirement-1-acceptance".into(),
                        statement: "An export contains the reviewed project record.".into(),
                        criterion_type: "user-acceptance".into(),
                        machine_checkable: false,
                    }],
                    verification_policy: VerificationPolicy {
                        obligations: vec![EvidenceObligation {
                            class: EvidenceClass::HumanDecision,
                            minimum_confidence: EvidenceConfidence::HumanAsserted,
                            rationale: "The project owner reviews the export result.".into(),
                            required: true,
                        }],
                        p8_collector_required: true,
                    },
                    dependencies: Vec::new(),
                    risk: RequirementRisk::High,
                    requirement_type: "user-requirement".into(),
                },
                ProjectRequirementSeed {
                    requirement_id: Some("takeover-finding-1".into()),
                    title: "Takeover finding: stale deployment path".into(),
                    intent: "The stale deployment path is resolved before implementation.".into(),
                    source: RequirementSource::TakeoverFinding {
                        reference: "takeover://finding-1".into(),
                    },
                    priority: RequirementPriority::P1,
                    acceptance_criteria: vec![AcceptanceCriterion {
                        criterion_id: "takeover-finding-1-acceptance".into(),
                        statement: "The stale deployment path has a recorded disposition.".into(),
                        criterion_type: "takeover-reconciliation".into(),
                        machine_checkable: false,
                    }],
                    verification_policy: VerificationPolicy {
                        obligations: vec![EvidenceObligation {
                            class: EvidenceClass::HumanDecision,
                            minimum_confidence: EvidenceConfidence::HumanAsserted,
                            rationale: "The takeover owner reviews the disposition.".into(),
                            required: true,
                        }],
                        p8_collector_required: true,
                    },
                    dependencies: Vec::new(),
                    risk: RequirementRisk::High,
                    requirement_type: "takeover-finding".into(),
                },
            ],
            architecture_decisions: vec!["ADR: local export boundary".into()],
            assumptions: vec!["The owner can review an export locally.".into()],
            risks: vec!["Export format drift".into()],
            decisions: Vec::new(),
            source_revision: "blueprint-1".into(),
            source_fingerprint: "p1-fingerprint".into(),
        };
        let user_requirement_id = project_requirement_id("p1", &authority.requirements[0]);
        let engine = AuthorityEngine;
        let draft = engine
            .build_draft_with_project_authority(
                "project-authority-mission",
                "p1",
                "blueprint-1",
                "takeover-1",
                registry.clone(),
                &desktop_context(),
                scope_fingerprint(BTreeMap::from([("root".into(), "p1".into())])),
                authority,
                Vec::new(),
            )
            .unwrap();
        let project_requirement = draft
            .requirement_graph
            .requirements
            .iter()
            .find(|requirement| requirement.requirement_id == user_requirement_id)
            .expect("project requirement in graph");
        assert!(matches!(
            project_requirement.source,
            RequirementSource::User { .. }
        ));
        assert!(draft
            .requirement_graph
            .requirements
            .iter()
            .any(|requirement| matches!(
                requirement.source,
                RequirementSource::TakeoverFinding { .. }
            )));
        let (sealed, _) = engine
            .seal(&draft, &trusted, "project-authority-sealed")
            .unwrap();
        assert!(sealed
            .contract
            .requirement_graph
            .requirements
            .iter()
            .all(|requirement| requirement.sealed_hash.is_some()));

        let mut removed = draft.clone();
        removed
            .requirement_graph
            .requirements
            .retain(|requirement| requirement.requirement_id != user_requirement_id);
        let removed_tasks = removed
            .task_graph
            .tasks
            .iter()
            .filter(|task| {
                task.requirement_ids
                    .iter()
                    .any(|id| id == &user_requirement_id)
            })
            .map(|task| task.task_id.clone())
            .collect::<BTreeSet<_>>();
        removed
            .task_graph
            .tasks
            .retain(|task| !removed_tasks.contains(&task.task_id));
        removed.task_graph.links.retain(|link| {
            link.requirement_id != user_requirement_id && !removed_tasks.contains(&link.task_id)
        });
        assert!(engine.preseal(&removed, &trusted).is_err());

        let path = std::env::temp_dir().join(format!(
            "relintor-p6-project-authority-{}.sqlite",
            std::process::id()
        ));
        let connection = Connection::open(&path).unwrap();
        connection
            .execute_batch(include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../db/migrations/006_authority_foundation.sql"
            )))
            .unwrap();
        drop(connection);
        persist_authority(&path, &registry, &sealed).unwrap();
        let loaded =
            load_mission_revision(&path, &sealed.seal.mission_id, sealed.revision).unwrap();
        assert!(matches!(
            loaded
                .contract
                .requirement_graph
                .requirements
                .iter()
                .find(|requirement| requirement.requirement_id == user_requirement_id)
                .map(|requirement| &requirement.source),
            Some(RequirementSource::User { .. })
        ));
        let _ = std::fs::remove_file(path);
    }

    fn desktop_context() -> ApplicabilityContext {
        let mut facts = BTreeMap::from([
            ("web".into(), FactValue::Bool(false)),
            ("backend".into(), FactValue::Bool(false)),
            ("database".into(), FactValue::Bool(false)),
            ("authentication".into(), FactValue::Bool(false)),
            ("ui_surface".into(), FactValue::Bool(false)),
            ("seo_relevance".into(), FactValue::Bool(false)),
            ("performance".into(), FactValue::Bool(false)),
            ("deployment".into(), FactValue::Bool(false)),
            ("observability".into(), FactValue::Bool(false)),
            ("privacy".into(), FactValue::Bool(false)),
            ("payments".into(), FactValue::Bool(false)),
            ("ai".into(), FactValue::Bool(false)),
            ("blockchain".into(), FactValue::Bool(false)),
            ("mobile".into(), FactValue::Bool(false)),
            ("desktop".into(), FactValue::Bool(true)),
            ("data_engineering".into(), FactValue::Bool(false)),
            ("integrations".into(), FactValue::Bool(false)),
        ]);
        facts.insert("platform".into(), FactValue::Text("desktop".into()));
        ApplicabilityContext {
            revision: "facts-1".into(),
            facts,
        }
    }

    #[test]
    fn all_eighteen_packs_are_meaningful_and_irrelevant_rules_become_na() {
        let (registry, _) = signed_registry();
        assert_eq!(registry.packs.len(), 18);
        assert!(registry.packs.iter().all(|pack| pack.rules.len() >= 2));
        let engine = AuthorityEngine;
        let (ledger, graph) = engine.evaluate(&registry, &desktop_context()).unwrap();
        assert!(ledger
            .iter()
            .any(|result| result.rule_id.contains("SEO_DISCOVERABILITY")
                && result.outcome == ApplicabilityOutcome::NotApplicable));
        assert!(graph
            .requirements
            .iter()
            .all(|requirement| requirement.requirement_type != "seo-discoverability"));
    }

    #[test]
    fn signatures_and_tamper_checks_fail_closed() {
        let (registry, trusted) = signed_registry();
        registry.verify(&trusted, true).unwrap();
        let mut tampered = registry.clone();
        tampered.packs[0].rules[0].title.push('x');
        assert!(matches!(
            tampered.verify(&trusted, true),
            Err(AuthorityError::RegistryDigestMismatch)
        ));
        let mut unsigned = builtin_registry();
        unsigned.registry_digest = unsigned.calculate_digest().unwrap();
        assert!(matches!(
            unsigned.verify(&trusted, true),
            Err(AuthorityError::UnsignedRegistry)
        ));
    }

    #[test]
    fn graphs_reject_cycles_and_accept_deterministic_dags() {
        let make = |id: &str| Requirement {
            requirement_id: id.into(),
            title: id.into(),
            intent: id.into(),
            source: RequirementSource::Inference {
                reference: id.into(),
            },
            priority: RequirementPriority::P2,
            applicability: ApplicabilityOutcome::Applicable,
            acceptance_criteria: vec![AcceptanceCriterion {
                criterion_id: id.into(),
                statement: "check".into(),
                criterion_type: "test".into(),
                machine_checkable: true,
            }],
            verification_policy: VerificationPolicy {
                obligations: vec![EvidenceObligation {
                    class: EvidenceClass::TestOutput,
                    minimum_confidence: EvidenceConfidence::StrongDeterministic,
                    rationale: "test".into(),
                    required: true,
                }],
                p8_collector_required: true,
            },
            dependencies: Vec::new(),
            risk: RequirementRisk::Medium,
            status: RequirementStatus::Unstarted,
            implementation_links: Vec::new(),
            evidence_links: Vec::new(),
            explicit_exceptions: Vec::new(),
            sealed_hash: None,
            requirement_type: "test".into(),
            origin_rule_id: None,
            revision: 1,
            schema_version: P6_SCHEMA_VERSION,
        };
        let graph = RequirementGraph {
            requirements: vec![make("a"), make("b")],
            dependencies: vec![RequirementDependency {
                requirement_id: "a".into(),
                depends_on: "b".into(),
                reason: "b first".into(),
                dependency_type: "build".into(),
            }],
        };
        assert_eq!(graph.deterministic_order().unwrap(), vec!["b", "a"]);
        let mut cyclic = graph.clone();
        cyclic.dependencies.push(RequirementDependency {
            requirement_id: "b".into(),
            depends_on: "a".into(),
            reason: "cycle".into(),
            dependency_type: "build".into(),
        });
        assert!(matches!(
            cyclic.validate(),
            Err(AuthorityError::DependencyCycle(_))
        ));
    }

    #[test]
    fn seal_is_deterministic_and_requirement_change_requires_revalidation() {
        let (registry, trusted) = signed_registry();
        let engine = AuthorityEngine;
        let draft = engine
            .build_draft(
                "m1",
                "p1",
                "source-1",
                "scope-1",
                registry,
                &desktop_context(),
                scope_fingerprint(BTreeMap::from([("root".into(), "p1".into())])),
                Vec::new(),
            )
            .unwrap();
        let (sealed, handoff) = engine.seal(&draft, &trusted, "fixed-time").unwrap();
        assert_eq!(handoff.state, "READY_FOR_EXECUTION");
        assert!(engine.validate_seal(&sealed).unwrap().valid);
        let mut changed = sealed.contract.clone();
        changed.requirement_graph.requirements[0]
            .intent
            .push_str(" changed");
        assert_eq!(
            engine.revalidate(&sealed, &changed).unwrap().status,
            "REVALIDATION_REQUIRED"
        );
        assert_eq!(
            sealed.seal.contract_hash,
            sealed.contract.canonical_hash().unwrap()
        );
        let reordered = {
            let mut value = sealed.contract.clone();
            value.requirement_graph.requirements.reverse();
            value
        };
        assert_eq!(
            sealed.contract.canonical_hash().unwrap(),
            reordered.canonical_hash().unwrap()
        );
    }

    #[test]
    fn preseal_requires_acceptance_evidence_and_human_deferral() {
        let (registry, trusted) = signed_registry();
        let engine = AuthorityEngine;
        let mut draft = engine
            .build_draft(
                "m1",
                "p1",
                "source-1",
                "scope-1",
                registry,
                &ApplicabilityContext {
                    revision: "facts".into(),
                    facts: {
                        let mut facts = desktop_context().facts;
                        facts.insert("platform".into(), FactValue::Text("api".into()));
                        facts.insert("backend".into(), FactValue::Bool(true));
                        facts
                    },
                },
                scope_fingerprint(BTreeMap::from([("root".into(), "p1".into())])),
                Vec::new(),
            )
            .unwrap();
        draft.requirement_graph.requirements[0]
            .acceptance_criteria
            .clear();
        assert!(matches!(
            engine.preseal(&draft, &trusted),
            Err(AuthorityError::MissingAcceptance(_))
        ));
        let decision = ExplicitDecision {
            decision_id: "d1".into(),
            kind: DecisionKind::Defer,
            actor: DecisionActor::Ai,
            requirement_id: "x".into(),
            reason: "later".into(),
            risk: "high".into(),
            timestamp: "t".into(),
            mission_revision: 1,
            provenance: "ai".into(),
        };
        assert!(matches!(
            engine.build_draft(
                "m2",
                "p1",
                "source-1",
                "scope-1",
                builtin_registry(),
                &desktop_context(),
                scope_fingerprint(BTreeMap::new()),
                vec![decision]
            ),
            Err(AuthorityError::InvalidDecision(_))
        ));
    }
}
