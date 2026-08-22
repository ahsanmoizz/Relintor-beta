//! P11 distribution and rollout status authority.
//!
//! This module exposes read-only, deployment-backed status. It never invents
//! a release, a health result, or a rollout success: repository readiness is
//! probed at request time and signed standards metadata is verified against
//! the Rust trust anchor before it is reported as available. Mutable rollout
//! control remains a privileged deployment concern until a hosted control
//! plane is configured.

use super::{CloudRepository, Config};
use relintor_standards::{
    production_registry, verify_signed_distribution, StandardsDistributionMetadata,
};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RolloutStatusView {
    pub state: String,
    pub channel: String,
    pub supported_versions: Vec<String>,
    pub update_availability: String,
    pub control_mode: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ServiceStatusView {
    pub service: String,
    pub version: String,
    pub status: String,
    pub detail: String,
    pub checked_at_unix: i64,
    pub supported_versions: Vec<String>,
    pub release_channel: String,
    pub rollout: RolloutStatusView,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StandardsDistributionStatusView {
    pub state: String,
    pub detail: String,
    pub channel: String,
    pub registry_id: Option<String>,
    pub registry_version: Option<u64>,
    pub artifact_digest: Option<String>,
    pub metadata_digest: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SignedDistributionManifest {
    metadata: StandardsDistributionMetadata,
    artifact_path: String,
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default()
}

fn manifest_artifact_path(manifest_path: &Path, artifact_path: &str) -> PathBuf {
    let artifact = Path::new(artifact_path);
    if artifact.is_absolute() {
        artifact.to_path_buf()
    } else {
        manifest_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(artifact)
    }
}

pub fn standards_distribution_status(config: &Config) -> StandardsDistributionStatusView {
    let Some(manifest_path) = config.standards_distribution_manifest.as_deref() else {
        return StandardsDistributionStatusView {
            state: "unavailable".into(),
            detail: "PENDING_EXTERNAL_ENVIRONMENT: no signed distribution manifest is configured"
                .into(),
            channel: config.release_channel.clone(),
            registry_id: None,
            registry_version: None,
            artifact_digest: None,
            metadata_digest: None,
        };
    };

    let manifest_path = Path::new(manifest_path);
    let manifest_bytes = match fs::read(manifest_path) {
        Ok(bytes) => bytes,
        Err(error) => {
            return StandardsDistributionStatusView {
                state: "unavailable".into(),
                detail: format!("signed distribution manifest is unavailable: {error}"),
                channel: config.release_channel.clone(),
                registry_id: None,
                registry_version: None,
                artifact_digest: None,
                metadata_digest: None,
            }
        }
    };
    let manifest: SignedDistributionManifest = match serde_json::from_slice(&manifest_bytes) {
        Ok(manifest) => manifest,
        Err(error) => {
            return StandardsDistributionStatusView {
                state: "invalid".into(),
                detail: format!("signed distribution manifest is invalid: {error}"),
                channel: config.release_channel.clone(),
                registry_id: None,
                registry_version: None,
                artifact_digest: None,
                metadata_digest: None,
            }
        }
    };
    let artifact_path = manifest_artifact_path(manifest_path, &manifest.artifact_path);
    let artifact_bytes = match fs::read(&artifact_path) {
        Ok(bytes) => bytes,
        Err(error) => {
            return StandardsDistributionStatusView {
                state: "unavailable".into(),
                detail: format!("signed standards artifact is unavailable: {error}"),
                channel: manifest.metadata.channel.clone(),
                registry_id: Some(manifest.metadata.registry_id.clone()),
                registry_version: Some(manifest.metadata.registry_version),
                artifact_digest: Some(manifest.metadata.artifact_digest.clone()),
                metadata_digest: Some(manifest.metadata.metadata_digest.clone()),
            }
        }
    };

    let trusted = match production_registry() {
        Ok((_, trusted)) => trusted,
        Err(error) => {
            return StandardsDistributionStatusView {
                state: "unavailable".into(),
                detail: format!("standards trust anchor is unavailable: {error}"),
                channel: manifest.metadata.channel.clone(),
                registry_id: Some(manifest.metadata.registry_id.clone()),
                registry_version: Some(manifest.metadata.registry_version),
                artifact_digest: Some(manifest.metadata.artifact_digest.clone()),
                metadata_digest: Some(manifest.metadata.metadata_digest.clone()),
            }
        }
    };
    match verify_signed_distribution(None, &manifest.metadata, &artifact_bytes, &trusted) {
        Ok(verified) => StandardsDistributionStatusView {
            state: "available".into(),
            detail: "signed standards artifact verified by the Rust trust anchor".into(),
            channel: verified.metadata.channel,
            registry_id: Some(verified.metadata.registry_id),
            registry_version: Some(verified.metadata.registry_version),
            artifact_digest: Some(verified.metadata.artifact_digest),
            metadata_digest: Some(verified.metadata.metadata_digest),
        },
        Err(error) => StandardsDistributionStatusView {
            state: "invalid".into(),
            detail: format!("signed standards artifact failed closed: {error}"),
            channel: manifest.metadata.channel.clone(),
            registry_id: Some(manifest.metadata.registry_id.clone()),
            registry_version: Some(manifest.metadata.registry_version),
            artifact_digest: Some(manifest.metadata.artifact_digest.clone()),
            metadata_digest: Some(manifest.metadata.metadata_digest.clone()),
        },
    }
}

pub fn service_status<R: CloudRepository>(accounts: &mut R, config: &Config) -> ServiceStatusView {
    let (status, detail) = match accounts.readiness() {
        Ok(()) => (
            "healthy".to_string(),
            "cloud repository readiness probe passed".to_string(),
        ),
        Err(error) => (
            "unavailable".to_string(),
            format!("cloud repository readiness probe failed: {}", error.message),
        ),
    };
    let distribution = standards_distribution_status(config);
    let rollout_state = if status == "unavailable" {
        "blocked_by_service_readiness"
    } else if distribution.state == "available" {
        "distribution_available"
    } else {
        "deployment_configured_pending_distribution"
    };
    ServiceStatusView {
        service: "cloud-api".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        status,
        detail,
        checked_at_unix: now_unix(),
        supported_versions: config.supported_versions.clone(),
        release_channel: config.release_channel.clone(),
        rollout: RolloutStatusView {
            state: rollout_state.into(),
            channel: config.release_channel.clone(),
            supported_versions: config.supported_versions.clone(),
            update_availability: distribution.state,
            control_mode: "deployment_config_only; privileged hosted mutation unavailable".into(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CloudRepository, Config, MemoryStore};

    #[test]
    fn missing_distribution_is_reported_as_unavailable() {
        let config = Config::test_config();
        let status = standards_distribution_status(&config);
        assert_eq!(status.state, "unavailable");
        assert!(status.detail.contains("PENDING_EXTERNAL_ENVIRONMENT"));
    }

    #[test]
    fn service_status_uses_repository_probe_and_explicit_rollout_state() {
        let config = Config::test_config();
        let mut store = MemoryStore::default();
        let status = service_status(&mut store, &config);
        assert_eq!(status.status, "healthy");
        assert_eq!(status.version, env!("CARGO_PKG_VERSION"));
        assert_eq!(status.rollout.update_availability, "unavailable");
        assert_eq!(
            status.rollout.control_mode,
            "deployment_config_only; privileged hosted mutation unavailable"
        );
        let _ = CloudRepository::readiness(&mut store);
    }
}
