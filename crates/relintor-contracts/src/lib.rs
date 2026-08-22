//! Cross-platform contracts shared by cloud services and the desktop.

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const ENTITLEMENT_SCHEMA_VERSION: u16 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PlanTier {
    Individual,
    Pro,
    Team,
    Enterprise,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EntitlementSource {
    Plan,
    Founder,
    Complimentary,
    Trial,
    Admin,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntitlementClaims {
    pub subject: String,
    pub organization: Option<String>,
    pub plan: PlanTier,
    pub source: EntitlementSource,
    pub capabilities: Vec<String>,
    pub limits: BTreeMap<String, u64>,
    pub issued_at: i64,
    pub expires_at: i64,
    /// Offline grace is signed by the server. A client cannot extend it by
    /// passing a larger local duration to the verifier.
    pub grace_until: i64,
    pub schema_version: u16,
    pub issuer: String,
    pub id: String,
    pub key_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignedEntitlement {
    pub claims: EntitlementClaims,
    pub signature: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EntitlementState {
    Valid,
    Grace,
    Expired,
    Unavailable,
    InvalidSignature,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntitlementVerificationError {
    InvalidEncoding,
    InvalidSignature,
    UnknownKeyVersion,
    SchemaVersionMismatch,
    WrongSubject,
    WrongOrganization,
    WrongIssuer,
    InvalidValidityWindow,
    NotYetValid,
}

impl std::fmt::Display for EntitlementVerificationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::InvalidEncoding => "invalid entitlement encoding",
            Self::InvalidSignature => "invalid entitlement signature",
            Self::UnknownKeyVersion => "unknown entitlement key version",
            Self::SchemaVersionMismatch => "unsupported entitlement schema version",
            Self::WrongSubject => "entitlement subject does not match account",
            Self::WrongOrganization => {
                "entitlement organization does not match active organization"
            }
            Self::WrongIssuer => "entitlement issuer is not trusted",
            Self::InvalidValidityWindow => "entitlement validity window is invalid",
            Self::NotYetValid => "entitlement was issued in the future",
        };
        f.write_str(message)
    }
}

impl std::error::Error for EntitlementVerificationError {}

pub fn sign_entitlement(claims: EntitlementClaims, key: &SigningKey) -> SignedEntitlement {
    let signature = key.sign(&canonical_claims(&claims));
    SignedEntitlement {
        claims,
        signature: BASE64.encode(signature.to_bytes()),
    }
}

pub fn verify_entitlement(
    document: &SignedEntitlement,
    keyring: &BTreeMap<String, VerifyingKey>,
    expected_subject: &str,
    expected_organization: Option<&str>,
    expected_issuer: &str,
    now: i64,
) -> Result<EntitlementState, EntitlementVerificationError> {
    // key_id necessarily selects a verification key before the document can be
    // authenticated. All other semantic checks happen only after signature
    // verification, so modified claims are not classified as trusted claims.
    let key = keyring
        .get(&document.claims.key_id)
        .ok_or(EntitlementVerificationError::UnknownKeyVersion)?;

    let signature_bytes = BASE64
        .decode(&document.signature)
        .map_err(|_| EntitlementVerificationError::InvalidEncoding)?;
    let signature = Signature::from_slice(&signature_bytes)
        .map_err(|_| EntitlementVerificationError::InvalidEncoding)?;

    key.verify(&canonical_claims(&document.claims), &signature)
        .map_err(|_| EntitlementVerificationError::InvalidSignature)?;

    if document.claims.schema_version != ENTITLEMENT_SCHEMA_VERSION {
        return Err(EntitlementVerificationError::SchemaVersionMismatch);
    }
    if document.claims.subject != expected_subject {
        return Err(EntitlementVerificationError::WrongSubject);
    }
    if document.claims.organization.as_deref() != expected_organization {
        return Err(EntitlementVerificationError::WrongOrganization);
    }
    if document.claims.issuer != expected_issuer {
        return Err(EntitlementVerificationError::WrongIssuer);
    }
    if document.claims.issued_at > document.claims.expires_at
        || document.claims.expires_at > document.claims.grace_until
    {
        return Err(EntitlementVerificationError::InvalidValidityWindow);
    }
    if now < document.claims.issued_at {
        return Err(EntitlementVerificationError::NotYetValid);
    }

    Ok(classify_entitlement(&document.claims, now))
}

pub fn classify_entitlement(claims: &EntitlementClaims, now: i64) -> EntitlementState {
    if now <= claims.expires_at {
        EntitlementState::Valid
    } else if now <= claims.grace_until {
        EntitlementState::Grace
    } else {
        EntitlementState::Expired
    }
}

pub fn capability_names(plan: PlanTier) -> Vec<String> {
    let mut capabilities = vec!["verification".to_string()];
    match plan {
        PlanTier::Individual => {}
        PlanTier::Pro => capabilities.extend([
            "sealed_missions".to_string(),
            "project_takeover".to_string(),
            "professional_standards".to_string(),
            "ai_usage".to_string(),
        ]),
        PlanTier::Team => capabilities.extend([
            "sealed_missions".to_string(),
            "project_takeover".to_string(),
            "professional_standards".to_string(),
            "ai_usage".to_string(),
            "team_members".to_string(),
        ]),
        PlanTier::Enterprise => capabilities.extend([
            "sealed_missions".to_string(),
            "project_takeover".to_string(),
            "professional_standards".to_string(),
            "ai_usage".to_string(),
            "team_members".to_string(),
            "admin_features".to_string(),
        ]),
    }
    capabilities.sort();
    capabilities.dedup();
    capabilities
}

fn canonical_claims(claims: &EntitlementClaims) -> Vec<u8> {
    // Struct field order is fixed by this schema and all maps are BTreeMap, so
    // the signed representation is deterministic inside schema version 1.
    serde_json::to_vec(claims).expect("entitlement claims are serializable")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (SignedEntitlement, BTreeMap<String, VerifyingKey>) {
        let signing_key = SigningKey::from_bytes(&[7u8; 32]);
        let claims = EntitlementClaims {
            subject: "user-1".into(),
            organization: Some("org-1".into()),
            plan: PlanTier::Pro,
            source: EntitlementSource::Plan,
            capabilities: capability_names(PlanTier::Pro),
            limits: BTreeMap::from([("monthly_ai_units".into(), 10_000)]),
            issued_at: 100,
            expires_at: 200,
            grace_until: 260,
            schema_version: ENTITLEMENT_SCHEMA_VERSION,
            issuer: "relintor-cloud-test".into(),
            id: "lease-1".into(),
            key_id: "test-2026-01".into(),
        };
        let mut keyring = BTreeMap::new();
        keyring.insert(claims.key_id.clone(), signing_key.verifying_key());
        (sign_entitlement(claims, &signing_key), keyring)
    }

    fn verify(
        document: &SignedEntitlement,
        keyring: &BTreeMap<String, VerifyingKey>,
        now: i64,
    ) -> Result<EntitlementState, EntitlementVerificationError> {
        verify_entitlement(
            document,
            keyring,
            "user-1",
            Some("org-1"),
            "relintor-cloud-test",
            now,
        )
    }

    #[test]
    fn valid_signed_entitlement_is_verified() {
        let (document, keyring) = fixture();
        assert_eq!(
            verify(&document, &keyring, 150),
            Ok(EntitlementState::Valid)
        );
    }

    #[test]
    fn bad_signature_is_rejected_before_claim_semantics_are_trusted() {
        let (mut document, keyring) = fixture();
        document.claims.subject = "attacker".into();
        assert_eq!(
            verify(&document, &keyring, 150),
            Err(EntitlementVerificationError::InvalidSignature)
        );
    }

    #[test]
    fn signed_offline_grace_is_enforced() {
        let (document, keyring) = fixture();
        assert_eq!(
            verify(&document, &keyring, 230),
            Ok(EntitlementState::Grace)
        );
        assert_eq!(
            verify(&document, &keyring, 261),
            Ok(EntitlementState::Expired)
        );
    }

    #[test]
    fn wrong_subject_is_rejected() {
        let (document, keyring) = fixture();
        assert_eq!(
            verify_entitlement(
                &document,
                &keyring,
                "user-2",
                Some("org-1"),
                "relintor-cloud-test",
                150,
            ),
            Err(EntitlementVerificationError::WrongSubject)
        );
    }

    #[test]
    fn wrong_organization_is_rejected() {
        let (document, keyring) = fixture();
        assert_eq!(
            verify_entitlement(
                &document,
                &keyring,
                "user-1",
                Some("org-2"),
                "relintor-cloud-test",
                150,
            ),
            Err(EntitlementVerificationError::WrongOrganization)
        );
    }

    #[test]
    fn wrong_issuer_is_rejected() {
        let (document, keyring) = fixture();
        assert_eq!(
            verify_entitlement(
                &document,
                &keyring,
                "user-1",
                Some("org-1"),
                "other-issuer",
                150,
            ),
            Err(EntitlementVerificationError::WrongIssuer)
        );
    }

    #[test]
    fn key_version_mismatch_is_rejected() {
        let (document, mut keyring) = fixture();
        keyring.clear();
        assert_eq!(
            verify(&document, &keyring, 150),
            Err(EntitlementVerificationError::UnknownKeyVersion)
        );
    }

    #[test]
    fn invalid_validity_window_is_rejected() {
        let signing_key = SigningKey::from_bytes(&[7u8; 32]);
        let (mut document, keyring) = fixture();
        document.claims.grace_until = 150;
        document = sign_entitlement(document.claims, &signing_key);
        assert_eq!(
            verify(&document, &keyring, 140),
            Err(EntitlementVerificationError::InvalidValidityWindow)
        );
    }
}
