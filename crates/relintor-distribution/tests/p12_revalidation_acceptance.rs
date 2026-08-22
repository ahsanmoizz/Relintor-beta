use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use ed25519_dalek::{Signer, SigningKey};
use relintor_distribution::{
    export_supported_state, prepare_uninstall, public_key_id, read_export,
    update_manifest_signing_bytes, verify_signed_update, DistributionError, EvidenceIdentity,
    SignedUpdateManifest, UninstallChoice, UpdateState, UPDATE_SCHEMA_VERSION,
};
use std::{collections::BTreeSet, fs};
use tempfile::tempdir;

fn signing_key() -> SigningKey {
    SigningKey::from_bytes(&[19_u8; 32])
}

fn manifest(version: &str, artifact: &[u8]) -> SignedUpdateManifest {
    let key = signing_key();
    let mut value = SignedUpdateManifest {
        schema_version: UPDATE_SCHEMA_VERSION,
        product: "Relintor".into(),
        target: "windows-x86_64".into(),
        version: version.into(),
        artifact_sha256: sha256(artifact),
        signature: String::new(),
        key_id: public_key_id(&key.verifying_key()),
        update_id: format!("acceptance-{version}"),
        issued_at_ms: 1_000,
        expires_at_ms: 20_000,
    };
    value.signature = BASE64.encode(
        key.sign(&update_manifest_signing_bytes(&value).unwrap())
            .to_bytes(),
    );
    value
}

fn state() -> UpdateState {
    UpdateState {
        current_version: "1.0.0".into(),
        applied_update_ids: BTreeSet::new(),
    }
}

fn identity() -> EvidenceIdentity {
    EvidenceIdentity {
        account_id: "account-acceptance".into(),
        mission_id: "mission-acceptance".into(),
        project_id: "project-acceptance".into(),
    }
}

fn sha256(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut digest = Sha256::new();
    digest.update(bytes);
    format!("{:x}", digest.finalize())
}

#[test]
fn a08_signed_update_matrix_is_executed_against_real_verifier() {
    let artifact = b"release-artifact";
    let key = signing_key();
    let valid = manifest("1.1.0", artifact);
    assert!(verify_signed_update(
        &valid,
        artifact,
        &key.verifying_key(),
        &state(),
        "windows-x86_64",
        2_000
    )
    .is_ok());

    let wrong_signer = SigningKey::from_bytes(&[20_u8; 32]);
    assert!(matches!(
        verify_signed_update(
            &valid,
            artifact,
            &wrong_signer.verifying_key(),
            &state(),
            "windows-x86_64",
            2_000
        ),
        Err(DistributionError::InvalidSignature(_))
    ));
    assert!(verify_signed_update(
        &valid,
        b"tampered",
        &key.verifying_key(),
        &state(),
        "windows-x86_64",
        2_000
    )
    .is_err());

    let mut manifest_tamper = valid.clone();
    manifest_tamper.version = "9.9.9".into();
    assert!(verify_signed_update(
        &manifest_tamper,
        artifact,
        &key.verifying_key(),
        &state(),
        "windows-x86_64",
        2_000
    )
    .is_err());

    let mut replay = state();
    replay.applied_update_ids.insert(valid.update_id.clone());
    assert!(matches!(
        verify_signed_update(
            &valid,
            artifact,
            &key.verifying_key(),
            &replay,
            "windows-x86_64",
            2_000
        ),
        Err(DistributionError::Replay(_))
    ));
    assert!(verify_signed_update(
        &manifest("0.9.0", artifact),
        artifact,
        &key.verifying_key(),
        &state(),
        "windows-x86_64",
        2_000
    )
    .is_err());
    assert!(verify_signed_update(
        &valid,
        artifact,
        &key.verifying_key(),
        &state(),
        "linux-x86_64",
        2_000
    )
    .is_err());
    assert!(verify_signed_update(
        &valid,
        artifact,
        &key.verifying_key(),
        &state(),
        "windows-x86_64",
        20_000
    )
    .is_err());
}

#[test]
fn a12_export_restore_and_uninstall_matrix_is_executed_on_real_files() {
    let root = tempdir().unwrap();
    fs::create_dir_all(root.path().join("projects")).unwrap();
    fs::write(
        root.path().join("projects/mission.json"),
        b"supported evidence",
    )
    .unwrap();
    fs::write(root.path().join("unrelated.cache"), b"must remain").unwrap();
    let destination = root.path().parent().unwrap().join(format!(
        "p12-export-{}-{}.json",
        std::process::id(),
        std::process::id()
    ));
    let exported = export_supported_state(root.path(), &destination, identity()).unwrap();
    assert!(read_export(&destination, &identity()).is_ok());
    assert!(!exported.items.is_empty());

    let mut tampered = fs::read(&destination).unwrap();
    tampered.truncate(tampered.len() / 2);
    fs::write(&destination, tampered).unwrap();
    assert!(read_export(&destination, &identity()).is_err());
    fs::remove_file(&destination).unwrap();

    let destination = root.path().parent().unwrap().join(format!(
        "p12-export-valid-{}-{}.json",
        std::process::id(),
        std::process::id()
    ));
    let result = prepare_uninstall(
        root.path(),
        Some(&destination),
        identity(),
        UninstallChoice::ExportThenRemove,
    )
    .unwrap();
    assert!(result.preservation_verified);
    assert!(!root.path().join("projects").exists());
    assert!(root.path().join("unrelated.cache").exists());
    assert!(read_export(&destination, &identity()).is_ok());
    fs::remove_file(destination).unwrap();
}

#[test]
fn a12_secret_and_unsafe_paths_are_not_exported() {
    let root = tempdir().unwrap();
    fs::create_dir_all(root.path().join("projects")).unwrap();
    fs::write(
        root.path().join("projects/provider_key.json"),
        b"provider_api_key=bad",
    )
    .unwrap();
    let destination = root.path().parent().unwrap().join(format!(
        "p12-secret-export-{}-{}.json",
        std::process::id(),
        std::process::id()
    ));
    assert!(export_supported_state(root.path(), &destination, identity()).is_err());
}
