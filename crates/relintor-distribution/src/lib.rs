//! Release-boundary primitives for signed application updates and safe local
//! evidence preservation.  Network transport and installer execution remain
//! owned by the Tauri updater plugin; this crate owns the data authority that
//! must be satisfied before those operations are considered successful.

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs,
    io::Write,
    path::{Component, Path},
    time::{SystemTime, UNIX_EPOCH},
};
use walkdir::WalkDir;

pub const UPDATE_SCHEMA_VERSION: u32 = 1;
pub const EXPORT_SCHEMA_VERSION: u32 = 1;
pub const MAX_UPDATE_ARTIFACT_BYTES: usize = 512 * 1024 * 1024;
pub const MAX_EXPORT_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_EXPORT_FILES: usize = 20_000;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SignedUpdateManifest {
    pub schema_version: u32,
    pub product: String,
    pub target: String,
    pub version: String,
    pub artifact_sha256: String,
    pub signature: String,
    pub key_id: String,
    pub update_id: String,
    pub issued_at_ms: u64,
    pub expires_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UpdateState {
    pub current_version: String,
    pub applied_update_ids: BTreeSet<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VerifiedUpdate {
    pub update_id: String,
    pub target: String,
    pub version: String,
    pub artifact_sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StagedUpdate {
    pub update: VerifiedUpdate,
    pub path: String,
    pub bytes: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum UpdateStatus {
    Available(VerifiedUpdate),
    NotAvailable,
    Unavailable(String),
}

#[derive(Debug)]
pub enum DistributionError {
    InvalidManifest(String),
    InvalidSignature(String),
    Artifact(String),
    Version(String),
    Replay(String),
    Path(String),
    Io(String),
    Export(String),
}

impl std::fmt::Display for DistributionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidManifest(value) => write!(f, "invalid update manifest: {value}"),
            Self::InvalidSignature(value) => write!(f, "update signature rejected: {value}"),
            Self::Artifact(value) => write!(f, "update artifact rejected: {value}"),
            Self::Version(value) => write!(f, "update version rejected: {value}"),
            Self::Replay(value) => write!(f, "update replay rejected: {value}"),
            Self::Path(value) => write!(f, "unsafe distribution path: {value}"),
            Self::Io(value) => write!(f, "distribution I/O failed: {value}"),
            Self::Export(value) => write!(f, "evidence export rejected: {value}"),
        }
    }
}
impl std::error::Error for DistributionError {}

#[derive(Serialize)]
struct ManifestSigningView<'a> {
    schema_version: u32,
    product: &'a str,
    target: &'a str,
    version: &'a str,
    artifact_sha256: &'a str,
    key_id: &'a str,
    update_id: &'a str,
    issued_at_ms: u64,
    expires_at_ms: u64,
}

fn manifest_signing_bytes(manifest: &SignedUpdateManifest) -> Result<Vec<u8>, DistributionError> {
    serde_json::to_vec(&ManifestSigningView {
        schema_version: manifest.schema_version,
        product: &manifest.product,
        target: &manifest.target,
        version: &manifest.version,
        artifact_sha256: &manifest.artifact_sha256,
        key_id: &manifest.key_id,
        update_id: &manifest.update_id,
        issued_at_ms: manifest.issued_at_ms,
        expires_at_ms: manifest.expires_at_ms,
    })
    .map_err(|error| DistributionError::InvalidManifest(error.to_string()))
}

/// Canonical bytes used by a release publisher to sign update metadata.  The
/// private signing key never belongs in this crate or in the desktop bundle.
pub fn update_manifest_signing_bytes(
    manifest: &SignedUpdateManifest,
) -> Result<Vec<u8>, DistributionError> {
    manifest_signing_bytes(manifest)
}

pub fn public_key_id(key: &VerifyingKey) -> String {
    sha256_hex(key.as_bytes())
}

pub fn verify_signed_update(
    manifest: &SignedUpdateManifest,
    artifact: &[u8],
    trusted_key: &VerifyingKey,
    state: &UpdateState,
    expected_target: &str,
    now_ms: u64,
) -> Result<VerifiedUpdate, DistributionError> {
    if manifest.schema_version != UPDATE_SCHEMA_VERSION
        || manifest.product != "Relintor"
        || manifest.target != expected_target
        || manifest.update_id.trim().is_empty()
        || manifest.expires_at_ms <= manifest.issued_at_ms
    {
        return Err(DistributionError::InvalidManifest(
            "schema, product, target, identity, or validity window is invalid".into(),
        ));
    }
    if manifest.issued_at_ms > now_ms.saturating_add(5 * 60 * 1000) {
        return Err(DistributionError::InvalidManifest(
            "metadata is issued in the future".into(),
        ));
    }
    if now_ms >= manifest.expires_at_ms {
        return Err(DistributionError::InvalidManifest(
            "metadata is stale".into(),
        ));
    }
    if manifest.key_id != public_key_id(trusted_key) {
        return Err(DistributionError::InvalidSignature(
            "manifest signer identity does not match the trusted key".into(),
        ));
    }
    let signature_bytes = BASE64
        .decode(&manifest.signature)
        .map_err(|_| DistributionError::InvalidSignature("signature is not base64".into()))?;
    let signature = Signature::from_slice(&signature_bytes)
        .map_err(|_| DistributionError::InvalidSignature("signature length is invalid".into()))?;
    let signing_bytes = manifest_signing_bytes(manifest)?;
    trusted_key
        .verify(&signing_bytes, &signature)
        .map_err(|_| DistributionError::InvalidSignature("signature verification failed".into()))?;
    if artifact.len() > MAX_UPDATE_ARTIFACT_BYTES {
        return Err(DistributionError::Artifact(
            "artifact exceeds bounded size".into(),
        ));
    }
    let artifact_sha256 = sha256_hex(artifact);
    if manifest.artifact_sha256.to_ascii_lowercase() != artifact_sha256 {
        return Err(DistributionError::Artifact(
            "artifact digest does not match signed metadata".into(),
        ));
    }
    let update_version = parse_version(&manifest.version)?;
    let current_version = parse_version(&state.current_version)?;
    if update_version <= current_version {
        return Err(DistributionError::Version(
            "downgrade or same-version update is not allowed".into(),
        ));
    }
    if state.applied_update_ids.contains(&manifest.update_id) {
        return Err(DistributionError::Replay(manifest.update_id.clone()));
    }
    Ok(VerifiedUpdate {
        update_id: manifest.update_id.clone(),
        target: manifest.target.clone(),
        version: manifest.version.clone(),
        artifact_sha256,
    })
}

pub fn stage_verified_update(
    destination: &Path,
    manifest: &SignedUpdateManifest,
    artifact: &[u8],
    trusted_key: &VerifyingKey,
    state: &UpdateState,
    expected_target: &str,
    now_ms: u64,
) -> Result<StagedUpdate, DistributionError> {
    let verified = verify_signed_update(
        manifest,
        artifact,
        trusted_key,
        state,
        expected_target,
        now_ms,
    )?;
    if !destination.is_absolute() {
        return Err(DistributionError::Path(
            "update staging destination must be absolute".into(),
        ));
    }
    fs::create_dir_all(destination).map_err(|e| DistributionError::Io(e.to_string()))?;
    let final_path = destination.join(format!("{}.bundle", verified.update_id));
    let temporary = destination.join(format!(".{}.tmp", verified.update_id));
    let mut file =
        fs::File::create(&temporary).map_err(|e| DistributionError::Io(e.to_string()))?;
    file.write_all(artifact)
        .and_then(|_| file.sync_all())
        .map_err(|e| DistributionError::Io(e.to_string()))?;
    let read_back = fs::read(&temporary).map_err(|e| DistributionError::Io(e.to_string()))?;
    if sha256_hex(&read_back) != verified.artifact_sha256 {
        let _ = fs::remove_file(&temporary);
        return Err(DistributionError::Artifact(
            "staged artifact failed read-back integrity validation".into(),
        ));
    }
    fs::rename(&temporary, &final_path).map_err(|e| DistributionError::Io(e.to_string()))?;
    Ok(StagedUpdate {
        update: verified,
        path: final_path.display().to_string(),
        bytes: artifact.len(),
    })
}

fn parse_version(value: &str) -> Result<(u64, u64, u64), DistributionError> {
    let value = value.strip_prefix('v').unwrap_or(value);
    let mut parts = value.split('.');
    let major = parts
        .next()
        .ok_or_else(|| DistributionError::Version("version is empty".into()))?
        .parse()
        .map_err(|_| DistributionError::Version("major version is invalid".into()))?;
    let minor = parts
        .next()
        .ok_or_else(|| DistributionError::Version("minor version is missing".into()))?
        .parse()
        .map_err(|_| DistributionError::Version("minor version is invalid".into()))?;
    let patch = parts
        .next()
        .ok_or_else(|| DistributionError::Version("patch version is missing".into()))?
        .parse()
        .map_err(|_| DistributionError::Version("patch version is invalid".into()))?;
    if parts.next().is_some() {
        return Err(DistributionError::Version(
            "only three numeric version components are supported".into(),
        ));
    }
    Ok((major, minor, patch))
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EvidenceIdentity {
    pub account_id: String,
    pub mission_id: String,
    pub project_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExportItem {
    pub relative_path: String,
    pub size_bytes: usize,
    pub sha256: String,
    pub content_base64: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EvidenceExport {
    pub schema_version: u32,
    pub identity: EvidenceIdentity,
    pub created_at_ms: u64,
    pub items: Vec<ExportItem>,
    pub integrity_sha256: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum UninstallChoice {
    Preserve,
    ExportThenRemove,
    RemoveAllowedState,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UninstallResult {
    pub choice: UninstallChoice,
    pub export_path: Option<String>,
    pub removed_paths: Vec<String>,
    pub preservation_verified: bool,
}

fn supported_relative(relative: &Path) -> bool {
    if relative == Path::new("relintor.sqlite") {
        return true;
    }
    let first = relative.components().next();
    matches!(first, Some(Component::Normal(value)) if matches!(value.to_string_lossy().as_ref(), "projects" | "missions" | "evidence" | "checkpoints"))
}

fn unsafe_name(value: &str) -> bool {
    let value = value.to_ascii_lowercase();
    [
        ".env",
        "api_key",
        "apikey",
        "bearer",
        "credential",
        "mfa",
        "password",
        "private_key",
        "provider_key",
        "refresh_token",
        "secret",
        "session_token",
    ]
    .iter()
    .any(|needle| value.contains(needle))
}

fn unsafe_content(bytes: &[u8]) -> bool {
    let text = String::from_utf8_lossy(bytes).to_ascii_lowercase();
    [
        "api_key",
        "apikey",
        "bearer ",
        "private_key",
        "mfa_secret",
        "refresh_token",
        "session_token",
        "provider_api_key",
    ]
    .iter()
    .any(|needle| text.contains(needle))
}

pub fn export_supported_state(
    state_root: &Path,
    destination: &Path,
    identity: EvidenceIdentity,
) -> Result<EvidenceExport, DistributionError> {
    let root =
        fs::canonicalize(state_root).map_err(|e| DistributionError::Export(e.to_string()))?;
    if !destination.is_absolute() || resolved_destination(destination)?.starts_with(&root) {
        return Err(DistributionError::Path(
            "export destination must be absolute and outside the state root".into(),
        ));
    }
    let mut items = Vec::new();
    let mut total_bytes = 0usize;
    for entry in WalkDir::new(&root).follow_links(false) {
        let entry = entry.map_err(|e| DistributionError::Export(e.to_string()))?;
        let path = entry.path();
        if path == root {
            continue;
        }
        let relative = path
            .strip_prefix(&root)
            .map_err(|e| DistributionError::Export(e.to_string()))?;
        if entry.file_type().is_symlink() {
            return Err(DistributionError::Path(format!(
                "symlink/reparse path is not exportable: {}",
                relative.display()
            )));
        }
        if !supported_relative(relative) {
            continue;
        }
        if unsafe_name(&relative.to_string_lossy()) {
            return Err(DistributionError::Export(format!(
                "secret-bearing state path is not exportable: {}",
                relative.display()
            )));
        }
        if !entry.file_type().is_file() {
            continue;
        }
        let bytes = fs::read(path).map_err(|e| DistributionError::Export(e.to_string()))?;
        if unsafe_content(&bytes) {
            return Err(DistributionError::Export(format!(
                "secret-bearing state content is not exportable: {}",
                relative.display()
            )));
        }
        total_bytes = total_bytes.saturating_add(bytes.len());
        if total_bytes > MAX_EXPORT_BYTES || items.len() >= MAX_EXPORT_FILES {
            return Err(DistributionError::Export(
                "export exceeds bounded file or byte limits".into(),
            ));
        }
        items.push(ExportItem {
            relative_path: relative.to_string_lossy().replace('\\', "/"),
            size_bytes: bytes.len(),
            sha256: sha256_hex(&bytes),
            content_base64: BASE64.encode(bytes),
        });
    }
    items.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    let mut export = EvidenceExport {
        schema_version: EXPORT_SCHEMA_VERSION,
        identity,
        created_at_ms: now_ms(),
        items,
        integrity_sha256: String::new(),
    };
    export.integrity_sha256 = export_integrity(&export)?;
    let json = serde_json::to_vec(&export).map_err(|e| DistributionError::Export(e.to_string()))?;
    fs::create_dir_all(destination.parent().unwrap_or(destination))
        .map_err(|e| DistributionError::Io(e.to_string()))?;
    if destination.exists() {
        return Err(DistributionError::Export(
            "export destination already exists".into(),
        ));
    }
    let temporary = destination.with_extension("tmp");
    let mut file =
        fs::File::create(&temporary).map_err(|e| DistributionError::Io(e.to_string()))?;
    file.write_all(&json)
        .and_then(|_| file.sync_all())
        .map_err(|e| DistributionError::Io(e.to_string()))?;
    let read_back = read_export(&temporary, &export.identity)?;
    if read_back.integrity_sha256 != export.integrity_sha256 {
        let _ = fs::remove_file(&temporary);
        return Err(DistributionError::Export("export read-back failed".into()));
    }
    fs::rename(&temporary, destination).map_err(|e| DistributionError::Io(e.to_string()))?;
    Ok(export)
}

pub fn read_export(
    path: &Path,
    expected_identity: &EvidenceIdentity,
) -> Result<EvidenceExport, DistributionError> {
    let bytes = fs::read(path).map_err(|e| DistributionError::Export(e.to_string()))?;
    if bytes.len() > MAX_EXPORT_BYTES {
        return Err(DistributionError::Export(
            "export exceeds bounded size".into(),
        ));
    }
    let export: EvidenceExport = serde_json::from_slice(&bytes)
        .map_err(|e| DistributionError::Export(format!("malformed export: {e}")))?;
    if export.schema_version != EXPORT_SCHEMA_VERSION || &export.identity != expected_identity {
        return Err(DistributionError::Export(
            "export schema or account/mission/project binding mismatch".into(),
        ));
    }
    if export.integrity_sha256 != export_integrity(&export)? {
        return Err(DistributionError::Export(
            "export integrity mismatch".into(),
        ));
    }
    for item in &export.items {
        let content = BASE64
            .decode(&item.content_base64)
            .map_err(|_| DistributionError::Export("invalid item encoding".into()))?;
        if content.len() != item.size_bytes || sha256_hex(&content) != item.sha256 {
            return Err(DistributionError::Export(format!(
                "item integrity mismatch: {}",
                item.relative_path
            )));
        }
        if unsafe_name(&item.relative_path) || unsafe_content(&content) {
            return Err(DistributionError::Export(
                "export contains secret-bearing content".into(),
            ));
        }
        let relative = Path::new(&item.relative_path);
        if !supported_relative(relative)
            || relative.is_absolute()
            || relative.components().any(|c| c == Component::ParentDir)
        {
            return Err(DistributionError::Path(format!(
                "unsafe exported path: {}",
                item.relative_path
            )));
        }
    }
    Ok(export)
}

pub fn prepare_uninstall(
    state_root: &Path,
    export_destination: Option<&Path>,
    identity: EvidenceIdentity,
    choice: UninstallChoice,
) -> Result<UninstallResult, DistributionError> {
    match choice {
        UninstallChoice::Preserve => Ok(UninstallResult {
            choice,
            export_path: None,
            removed_paths: Vec::new(),
            preservation_verified: true,
        }),
        UninstallChoice::ExportThenRemove => {
            let destination = export_destination.ok_or_else(|| {
                DistributionError::Export(
                    "EXPORT_THEN_REMOVE requires an export destination".into(),
                )
            })?;
            let export = export_supported_state(state_root, destination, identity.clone())?;
            let verified = read_export(destination, &identity).is_ok();
            if !verified || export.integrity_sha256.is_empty() {
                return Err(DistributionError::Export(
                    "preservation verification failed; removal was not attempted".into(),
                ));
            }
            let removed_paths = remove_allowed_state(state_root)?;
            Ok(UninstallResult {
                choice,
                export_path: Some(destination.display().to_string()),
                removed_paths,
                preservation_verified: true,
            })
        }
        UninstallChoice::RemoveAllowedState => Ok(UninstallResult {
            choice,
            export_path: None,
            removed_paths: remove_allowed_state(state_root)?,
            preservation_verified: false,
        }),
    }
}

fn remove_allowed_state(state_root: &Path) -> Result<Vec<String>, DistributionError> {
    let root = fs::canonicalize(state_root).map_err(|e| DistributionError::Io(e.to_string()))?;
    let mut removed = Vec::new();
    for name in ["projects", "missions", "evidence", "checkpoints"] {
        let path = root.join(name);
        if !path.exists() {
            continue;
        }
        if fs::symlink_metadata(&path)
            .map_err(|e| DistributionError::Io(e.to_string()))?
            .file_type()
            .is_symlink()
        {
            return Err(DistributionError::Path(format!(
                "refusing to remove symlinked state directory: {}",
                path.display()
            )));
        }
        fs::remove_dir_all(&path).map_err(|e| DistributionError::Io(e.to_string()))?;
        removed.push(path.display().to_string());
    }
    let database = root.join("relintor.sqlite");
    if database.exists() {
        if fs::symlink_metadata(&database)
            .map_err(|e| DistributionError::Io(e.to_string()))?
            .file_type()
            .is_symlink()
        {
            return Err(DistributionError::Path(
                "refusing to remove a symlinked local database".into(),
            ));
        }
        fs::remove_file(&database).map_err(|e| DistributionError::Io(e.to_string()))?;
        removed.push(database.display().to_string());
    }
    Ok(removed)
}

fn export_integrity(export: &EvidenceExport) -> Result<String, DistributionError> {
    let mut unsigned = export.clone();
    unsigned.integrity_sha256.clear();
    let bytes =
        serde_json::to_vec(&unsigned).map_err(|e| DistributionError::Export(e.to_string()))?;
    Ok(sha256_hex(&bytes))
}

fn resolved_destination(path: &Path) -> Result<std::path::PathBuf, DistributionError> {
    if path.exists() {
        return fs::canonicalize(path).map_err(|error| DistributionError::Path(error.to_string()));
    }
    let parent = path
        .parent()
        .ok_or_else(|| DistributionError::Path("destination has no parent".into()))?;
    let parent =
        fs::canonicalize(parent).map_err(|error| DistributionError::Path(error.to_string()))?;
    Ok(parent.join(
        path.file_name()
            .ok_or_else(|| DistributionError::Path("destination has no file name".into()))?,
    ))
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_millis() as u64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};
    use tempfile::tempdir;

    fn key() -> SigningKey {
        SigningKey::from_bytes(&[7_u8; 32])
    }

    fn signed(version: &str, artifact: &[u8]) -> SignedUpdateManifest {
        let signing_key = key();
        let mut manifest = SignedUpdateManifest {
            schema_version: UPDATE_SCHEMA_VERSION,
            product: "Relintor".into(),
            target: "windows-x86_64".into(),
            version: version.into(),
            artifact_sha256: sha256_hex(artifact),
            signature: String::new(),
            key_id: public_key_id(&signing_key.verifying_key()),
            update_id: format!("update-{version}"),
            issued_at_ms: 1_000,
            expires_at_ms: 10_000,
        };
        manifest.signature = BASE64.encode(
            signing_key
                .sign(&manifest_signing_bytes(&manifest).unwrap())
                .to_bytes(),
        );
        manifest
    }

    fn state() -> UpdateState {
        UpdateState {
            current_version: "1.0.0".into(),
            applied_update_ids: BTreeSet::new(),
        }
    }

    #[test]
    fn valid_signed_update_is_verified() {
        let signing_key = key();
        let artifact = b"signed-test-update";
        let verified = verify_signed_update(
            &signed("1.1.0", artifact),
            artifact,
            &signing_key.verifying_key(),
            &state(),
            "windows-x86_64",
            2_000,
        )
        .unwrap();
        assert_eq!(verified.version, "1.1.0");
    }

    #[test]
    fn wrong_signer_is_rejected() {
        let manifest = signed("1.1.0", b"artifact");
        assert!(matches!(
            verify_signed_update(
                &manifest,
                b"artifact",
                &SigningKey::from_bytes(&[8_u8; 32]).verifying_key(),
                &state(),
                "windows-x86_64",
                2_000
            ),
            Err(DistributionError::InvalidSignature(_))
        ));
    }

    #[test]
    fn manifest_and_artifact_tampering_is_rejected() {
        let signing_key = key();
        let mut manifest = signed("1.1.0", b"artifact");
        manifest.version = "9.9.9".into();
        assert!(verify_signed_update(
            &manifest,
            b"artifact",
            &signing_key.verifying_key(),
            &state(),
            "windows-x86_64",
            2_000
        )
        .is_err());
        assert!(verify_signed_update(
            &signed("1.1.0", b"artifact"),
            b"tampered",
            &signing_key.verifying_key(),
            &state(),
            "windows-x86_64",
            2_000
        )
        .is_err());
    }

    #[test]
    fn replay_downgrade_and_stale_metadata_are_rejected() {
        let signing_key = key();
        let manifest = signed("1.1.0", b"artifact");
        let mut replay = state();
        replay.applied_update_ids.insert(manifest.update_id.clone());
        assert!(matches!(
            verify_signed_update(
                &manifest,
                b"artifact",
                &signing_key.verifying_key(),
                &replay,
                "windows-x86_64",
                2_000
            ),
            Err(DistributionError::Replay(_))
        ));
        assert!(matches!(
            verify_signed_update(
                &signed("0.9.0", b"artifact"),
                b"artifact",
                &signing_key.verifying_key(),
                &state(),
                "windows-x86_64",
                2_000
            ),
            Err(DistributionError::Version(_))
        ));
        assert!(verify_signed_update(
            &manifest,
            b"artifact",
            &signing_key.verifying_key(),
            &state(),
            "windows-x86_64",
            10_000
        )
        .is_err());
    }

    #[test]
    fn staging_is_atomic_and_read_back_verified() {
        let dir = tempdir().unwrap();
        let staged = stage_verified_update(
            dir.path(),
            &signed("1.1.0", b"artifact"),
            b"artifact",
            &key().verifying_key(),
            &state(),
            "windows-x86_64",
            2_000,
        )
        .unwrap();
        assert_eq!(fs::read(&staged.path).unwrap(), b"artifact");
        assert!(!dir.path().join(".update-1.1.0.tmp").exists());
    }

    fn identity() -> EvidenceIdentity {
        EvidenceIdentity {
            account_id: "account-1".into(),
            mission_id: "mission-1".into(),
            project_id: "project-1".into(),
        }
    }

    fn state_root() -> tempfile::TempDir {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("projects")).unwrap();
        fs::write(
            dir.path().join("projects").join("mission.json"),
            b"evidence",
        )
        .unwrap();
        dir
    }

    #[test]
    fn preserve_choice_does_not_delete_state() {
        let root = state_root();
        let result =
            prepare_uninstall(root.path(), None, identity(), UninstallChoice::Preserve).unwrap();
        assert!(result.preservation_verified);
        assert!(root.path().join("projects/mission.json").exists());
    }

    #[test]
    fn export_then_remove_requires_verified_export() {
        let root = state_root();
        let export = root.path().parent().unwrap().join(format!(
            "relintor-export-{}-{}.json",
            std::process::id(),
            now_ms()
        ));
        let result = prepare_uninstall(
            root.path(),
            Some(&export),
            identity(),
            UninstallChoice::ExportThenRemove,
        )
        .unwrap();
        assert!(result.preservation_verified);
        assert!(!root.path().join("projects").exists());
        assert!(read_export(&export, &identity()).is_ok());
        let _ = fs::remove_file(export);
    }

    #[test]
    fn remove_choice_only_removes_supported_state() {
        let root = state_root();
        fs::write(root.path().join("unrelated.txt"), b"keep").unwrap();
        prepare_uninstall(
            root.path(),
            None,
            identity(),
            UninstallChoice::RemoveAllowedState,
        )
        .unwrap();
        assert!(root.path().join("unrelated.txt").exists());
    }

    #[test]
    fn export_tamper_truncation_wrong_identity_and_secrets_fail() {
        let root = state_root();
        let export = root.path().parent().unwrap().join(format!(
            "export-{}-{}.json",
            std::process::id(),
            now_ms()
        ));
        export_supported_state(root.path(), &export, identity()).unwrap();
        let mut bytes = fs::read(&export).unwrap();
        bytes[0] = b'!';
        fs::write(&export, &bytes).unwrap();
        assert!(read_export(&export, &identity()).is_err());
        fs::write(
            root.path().join("projects/secret.json"),
            b"api_key=never-export",
        )
        .unwrap();
        assert!(export_supported_state(
            root.path(),
            &root.path().parent().unwrap().join("bad.json"),
            identity()
        )
        .is_err());
        let _ = fs::remove_file(export);
    }

    #[test]
    fn unsafe_symlink_and_path_are_refused() {
        let root = state_root();
        assert!(
            export_supported_state(root.path(), &root.path().join("inside.json"), identity())
                .is_err()
        );
        #[cfg(windows)]
        let link = root.path().join("projects/link");
        #[cfg(windows)]
        if std::os::windows::fs::symlink_file(root.path().join("projects/mission.json"), &link)
            .is_ok()
        {
            #[cfg(windows)]
            assert!(export_supported_state(
                root.path(),
                &root.path().parent().unwrap().join("link.json"),
                identity()
            )
            .is_err());
        }
    }
}
