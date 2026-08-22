//! Rust-owned local authority boundary for the Relintor desktop shell.
//! Milestone 1 exposes health/detection APIs and owns database migration.
//! Renderer-facing health APIs never receive direct filesystem or SQL authority.

use rusqlite::{Connection, OpenFlags, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

const SCHEMA_VERSION: i64 = 7;
const EXPECTED_FEATURE_COUNT: usize = 144;
const MIGRATION_001_NAME: &str = "001_baseline";
const MIGRATION_002_NAME: &str = "004_investigator_foundation";
const MIGRATION_003_NAME: &str = "005_takeover_foundation";
const MIGRATION_004_NAME: &str = "006_authority_foundation";
const MIGRATION_005_NAME: &str = "007_authority_revision_scoping";
const MIGRATION_006_NAME: &str = "008_takeover_project_binding";
const MIGRATION_007_NAME: &str = "009_project_workflow_state";
const MIGRATION_001_SQL: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../db/migrations/001_baseline.sql"
));
const MIGRATION_002_SQL: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../db/migrations/004_investigator_foundation.sql"
));
const MIGRATION_003_SQL: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../db/migrations/005_takeover_foundation.sql"
));
const MIGRATION_004_SQL: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../db/migrations/006_authority_foundation.sql"
));
const MIGRATION_005_SQL: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../db/migrations/007_authority_revision_scoping.sql"
));
const MIGRATION_006_SQL: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../db/migrations/008_takeover_project_binding.sql"
));
const MIGRATION_007_SQL: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../db/migrations/009_project_workflow_state.sql"
));

const EXPECTED_MANIFEST_FILES: &[&str] = &[
    "00_PRODUCT_CONSTITUTION.md",
    "01_UX_UI_SPEC.md",
    "02_SYSTEM_ARCHITECTURE.md",
    "03_ANTIGRAVITY_INTEGRATION_CONTRACT.md",
    "04_INVESTIGATION_AND_STANDARDS_ENGINE.md",
    "05_REQUIREMENT_EVIDENCE_GRAPH.md",
    "06_EXECUTION_WATCHDOG_RECOVERY.md",
    "07_SUBSCRIPTIONS_TEAMS_ADMIN.md",
    "08_FEATURE_REGISTER_144.md",
    "09_IMPLEMENTATION_PHASES.md",
    "10_VERIFICATION_AND_RELEASE_GATES.md",
    "11_COMPETITIVE_POSITIONING.md",
    "12_BRAND_AND_HERO.md",
    "README.md",
    "RELINTOR_MASTER_SPEC.md",
    "feature-register.json",
    "hero-prototype.html",
];

const EXPECTED_LOCKED_FILES: &[&str] = &[
    "00_PRODUCT_CONSTITUTION.md",
    "01_UX_UI_SPEC.md",
    "02_SYSTEM_ARCHITECTURE.md",
    "03_ANTIGRAVITY_INTEGRATION_CONTRACT.md",
    "04_INVESTIGATION_AND_STANDARDS_ENGINE.md",
    "05_REQUIREMENT_EVIDENCE_GRAPH.md",
    "06_EXECUTION_WATCHDOG_RECOVERY.md",
    "07_SUBSCRIPTIONS_TEAMS_ADMIN.md",
    "08_FEATURE_REGISTER_144.md",
    "09_IMPLEMENTATION_PHASES.md",
    "10_VERIFICATION_AND_RELEASE_GATES.md",
    "11_COMPETITIVE_POSITIONING.md",
    "12_BRAND_AND_HERO.md",
    "README.md",
    "RELINTOR_MASTER_SPEC.md",
    "feature-register.json",
    "hero-prototype.html",
    "MANIFEST.sha256.json",
];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HealthStatus {
    pub status: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DatabaseHealth {
    pub status: String,
    pub detail: String,
    pub schema_version: Option<i64>,
    pub database_path: Option<String>,
}

impl DatabaseHealth {
    pub fn unavailable(detail: impl Into<String>) -> Self {
        Self {
            status: "unavailable".into(),
            detail: detail.into(),
            schema_version: None,
            database_path: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct KeychainHealth {
    pub status: String,
    pub provider: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AntigravityHealth {
    pub status: String,
    pub version: Option<String>,
    pub executable: Option<String>,
    pub compatibility: String,
    pub cli_invocation_capability: bool,
    pub plugin_hook_capability: bool,
    pub ide_status: String,
    pub cli_status: String,
    pub authentication_status: String,
    pub executable_detectable: bool,
    pub adapter_ready: bool,
    pub setup_required: bool,
    pub environment: String,
    pub platform: String,
    pub detected_at_ms: i128,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MigrationReport {
    pub status: String,
    pub schema_version: i64,
    pub database_path: String,
    pub applied_migrations: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct ManifestRecord {
    sha256: String,
    bytes: u64,
}

pub fn application_health() -> HealthStatus {
    HealthStatus {
        status: "healthy".into(),
        detail: "Rust local authority is available.".into(),
    }
}

pub fn keychain_health() -> KeychainHealth {
    #[cfg(target_os = "windows")]
    {
        return KeychainHealth {
            status: "supported_unverified".into(),
            provider: "Windows Credential Manager".into(),
            detail: "Windows provides the selected secure-store backend. Milestone 1 has not performed a credential write/read round trip; plaintext fallback is disabled.".into(),
        };
    }

    #[cfg(target_os = "macos")]
    {
        return KeychainHealth {
            status: "supported_unverified".into(),
            provider: "macOS Keychain".into(),
            detail: "macOS provides the selected secure-store backend. Milestone 1 has not performed a credential write/read round trip; plaintext fallback is disabled.".into(),
        };
    }

    #[cfg(target_os = "linux")]
    {
        let has_session_bus = env::var_os("DBUS_SESSION_BUS_ADDRESS").is_some();
        return if has_session_bus {
            KeychainHealth {
                status: "supported_unverified".into(),
                provider: "Secret Service (D-Bus)".into(),
                detail: "A D-Bus desktop session is present. Secret Service round-trip verification is still required; plaintext fallback is disabled.".into(),
            }
        } else {
            KeychainHealth {
                status: "degraded".into(),
                provider: "Secret Service (D-Bus)".into(),
                detail: "No D-Bus desktop session was detected. Secure-store availability is unproven and plaintext fallback is disabled.".into(),
            }
        };
    }

    #[allow(unreachable_code)]
    KeychainHealth {
        status: "unavailable".into(),
        provider: "unsupported OS keychain".into(),
        detail: "No supported secure store is configured and plaintext fallback is disabled."
            .into(),
    }
}

static KEYCHAIN_AUTHORITY_INIT_LOCK: Mutex<()> = Mutex::new(());

/// Load the desktop authority secret from the operating-system credential
/// store.  The secret is never written to the application data directory.
/// A missing credential is initialized with OS randomness and immediately
/// persisted in the native keychain; all keychain errors fail closed.
pub fn load_or_create_keychain_authority_key(
    service: &str,
    account: &str,
) -> Result<Vec<u8>, String> {
    if service.trim().is_empty() || account.trim().is_empty() {
        return Err("secure-store service and account are required".into());
    }
    let _guard = KEYCHAIN_AUTHORITY_INIT_LOCK.lock().map_err(|_| {
        "OS keychain authority initialization lock is poisoned; refusing access".to_string()
    })?;
    let entry = keyring::Entry::new(service, account)
        .map_err(|error| format!("create OS keychain entry: {error}"))?;
    match entry.get_password() {
        Ok(value) => {
            let bytes = hex_decode(&value)?;
            if bytes.len() != 32 {
                return Err("OS keychain authority secret has an unexpected length".into());
            }
            Ok(bytes)
        }
        Err(keyring::Error::NoEntry) => {
            let mut bytes = [0_u8; 32];
            getrandom::getrandom(&mut bytes)
                .map_err(|error| format!("generate authority secret: {error}"))?;
            let encoded = hex_encode(&bytes);
            entry
                .set_password(&encoded)
                .map_err(|error| format!("write authority secret to OS keychain: {error}"))?;
            let persisted = entry
                .get_password()
                .map_err(|error| format!("verify authority secret in OS keychain: {error}"))?;
            if persisted != encoded {
                return Err(
                    "OS keychain authority initialization changed unexpectedly; refusing ambiguous authority"
                        .into(),
                );
            }
            Ok(bytes.to_vec())
        }
        Err(error) => Err(format!(
            "read authority secret from OS keychain: {error}; refusing automatic key rotation"
        )),
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn hex_decode(value: &str) -> Result<Vec<u8>, String> {
    if !value.len().is_multiple_of(2) || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("OS keychain authority secret is not hexadecimal".into());
    }
    (0..value.len())
        .step_by(2)
        .map(|index| {
            u8::from_str_radix(&value[index..index + 2], 16)
                .map_err(|error| format!("decode OS keychain authority secret: {error}"))
        })
        .collect()
}

pub fn migrate_database(path: &Path) -> Result<MigrationReport, String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("create database directory: {e}"))?;
    }

    let mut conn =
        Connection::open(path).map_err(|e| format!("open SQLite database for migration: {e}"))?;
    conn.execute_batch(
        "PRAGMA foreign_keys = ON;
         CREATE TABLE IF NOT EXISTS schema_migrations (
             version INTEGER PRIMARY KEY,
             name TEXT NOT NULL,
             applied_at INTEGER NOT NULL
         );",
    )
    .map_err(|e| format!("initialize migration table: {e}"))?;

    let applied: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
            [],
            |row| row.get(0),
        )
        .map_err(|e| format!("read schema version: {e}"))?;

    if applied > SCHEMA_VERSION {
        return Err(format!(
            "database schema version {applied} is newer than supported version {SCHEMA_VERSION}"
        ));
    }

    let mut applied_migrations = Vec::new();

    if applied < 1 {
        let tx = conn
            .transaction()
            .map_err(|e| format!("begin baseline migration transaction: {e}"))?;

        tx.execute_batch(MIGRATION_001_SQL)
            .map_err(|e| format!("apply {MIGRATION_001_NAME}: {e}"))?;

        tx.execute(
            "INSERT OR IGNORE INTO schema_migrations(version, name, applied_at) VALUES (?1, ?2, ?3)",
            (1_i64, MIGRATION_001_NAME, now_seconds()),
        )
        .map_err(|e| format!("record {MIGRATION_001_NAME}: {e}"))?;

        tx.commit()
            .map_err(|e| format!("commit {MIGRATION_001_NAME}: {e}"))?;
        applied_migrations.push(MIGRATION_001_NAME.into());
    }

    if applied < 2 {
        let tx = conn
            .transaction()
            .map_err(|e| format!("begin investigator migration transaction: {e}"))?;

        tx.execute_batch(MIGRATION_002_SQL)
            .map_err(|e| format!("apply {MIGRATION_002_NAME}: {e}"))?;

        tx.execute(
            "INSERT OR IGNORE INTO schema_migrations(version, name, applied_at) VALUES (?1, ?2, ?3)",
            (2_i64, MIGRATION_002_NAME, now_seconds()),
        )
        .map_err(|e| format!("record {MIGRATION_002_NAME}: {e}"))?;

        tx.commit()
            .map_err(|e| format!("commit {MIGRATION_002_NAME}: {e}"))?;
        applied_migrations.push(MIGRATION_002_NAME.into());
    }

    if applied < 3 {
        let tx = conn
            .transaction()
            .map_err(|e| format!("begin takeover migration transaction: {e}"))?;

        tx.execute_batch(MIGRATION_003_SQL)
            .map_err(|e| format!("apply {MIGRATION_003_NAME}: {e}"))?;

        tx.execute(
            "INSERT OR IGNORE INTO schema_migrations(version, name, applied_at) VALUES (?1, ?2, ?3)",
            (3_i64, MIGRATION_003_NAME, now_seconds()),
        )
        .map_err(|e| format!("record {MIGRATION_003_NAME}: {e}"))?;

        tx.commit()
            .map_err(|e| format!("commit {MIGRATION_003_NAME}: {e}"))?;
        applied_migrations.push(MIGRATION_003_NAME.into());
    }

    if applied < 4 {
        let tx = conn
            .transaction()
            .map_err(|e| format!("begin authority migration transaction: {e}"))?;

        tx.execute_batch(MIGRATION_004_SQL)
            .map_err(|e| format!("apply {MIGRATION_004_NAME}: {e}"))?;

        tx.execute(
            "INSERT OR IGNORE INTO schema_migrations(version, name, applied_at) VALUES (?1, ?2, ?3)",
            (4_i64, MIGRATION_004_NAME, now_seconds()),
        )
        .map_err(|e| format!("record {MIGRATION_004_NAME}: {e}"))?;

        tx.commit()
            .map_err(|e| format!("commit {MIGRATION_004_NAME}: {e}"))?;
        applied_migrations.push(MIGRATION_004_NAME.into());
    }

    if applied < 5 {
        let tx = conn
            .transaction()
            .map_err(|e| format!("begin authority revision scoping migration transaction: {e}"))?;

        tx.execute_batch(MIGRATION_005_SQL)
            .map_err(|e| format!("apply {MIGRATION_005_NAME}: {e}"))?;

        tx.execute(
            "INSERT INTO schema_migrations(version, name, applied_at) VALUES (?1, ?2, ?3)",
            (5_i64, MIGRATION_005_NAME, now_seconds()),
        )
        .map_err(|e| format!("record {MIGRATION_005_NAME}: {e}"))?;

        tx.commit()
            .map_err(|e| format!("commit {MIGRATION_005_NAME}: {e}"))?;
        applied_migrations.push(MIGRATION_005_NAME.into());
    }

    if applied < 6 {
        let tx = conn
            .transaction()
            .map_err(|e| format!("begin takeover project binding migration transaction: {e}"))?;

        tx.execute_batch(MIGRATION_006_SQL)
            .map_err(|e| format!("apply {MIGRATION_006_NAME}: {e}"))?;
        tx.execute(
            "INSERT OR IGNORE INTO schema_migrations(version, name, applied_at) VALUES (?1, ?2, ?3)",
            (6_i64, MIGRATION_006_NAME, now_seconds()),
        )
        .map_err(|e| format!("record {MIGRATION_006_NAME}: {e}"))?;
        tx.commit()
            .map_err(|e| format!("commit {MIGRATION_006_NAME}: {e}"))?;
        applied_migrations.push(MIGRATION_006_NAME.into());
    }

    if applied < 7 {
        let tx = conn
            .transaction()
            .map_err(|e| format!("begin project workflow migration transaction: {e}"))?;
        tx.execute_batch(MIGRATION_007_SQL)
            .map_err(|e| format!("apply {MIGRATION_007_NAME}: {e}"))?;
        tx.execute(
            "INSERT OR IGNORE INTO schema_migrations(version, name, applied_at) VALUES (?1, ?2, ?3)",
            (7_i64, MIGRATION_007_NAME, now_seconds()),
        )
        .map_err(|e| format!("record {MIGRATION_007_NAME}: {e}"))?;
        tx.commit()
            .map_err(|e| format!("commit {MIGRATION_007_NAME}: {e}"))?;
        applied_migrations.push(MIGRATION_007_NAME.into());
    }

    let health = database_health(path);
    if health.status != "healthy" {
        return Err(format!(
            "database health failed after migration: {}",
            health.detail
        ));
    }

    Ok(MigrationReport {
        status: "healthy".into(),
        schema_version: health.schema_version.unwrap_or(SCHEMA_VERSION),
        database_path: path.display().to_string(),
        applied_migrations,
    })
}

pub fn database_health(path: &Path) -> DatabaseHealth {
    let path_display = path.display().to_string();

    if !path.is_file() {
        return DatabaseHealth {
            status: "unavailable".into(),
            detail: "Local database has not been initialized.".into(),
            schema_version: None,
            database_path: Some(path_display),
        };
    }

    let conn = match Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY) {
        Ok(conn) => conn,
        Err(e) => {
            return DatabaseHealth {
                status: "failed".into(),
                detail: format!("Open local database read-only: {e}"),
                schema_version: None,
                database_path: Some(path_display),
            }
        }
    };

    let quick_check: String = match conn.query_row("PRAGMA quick_check;", [], |row| row.get(0)) {
        Ok(value) => value,
        Err(e) => {
            return DatabaseHealth {
                status: "failed".into(),
                detail: format!("Run SQLite quick_check: {e}"),
                schema_version: None,
                database_path: Some(path_display),
            }
        }
    };

    if quick_check != "ok" {
        return DatabaseHealth {
            status: "failed".into(),
            detail: format!("SQLite quick_check returned: {quick_check}"),
            schema_version: None,
            database_path: Some(path_display),
        };
    }

    let migration_table: i64 = match conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='schema_migrations'",
        [],
        |row| row.get(0),
    ) {
        Ok(value) => value,
        Err(e) => {
            return DatabaseHealth {
                status: "failed".into(),
                detail: format!("Inspect migration table: {e}"),
                schema_version: None,
                database_path: Some(path_display),
            }
        }
    };

    if migration_table != 1 {
        return DatabaseHealth {
            status: "invalid".into(),
            detail: "schema_migrations table is missing.".into(),
            schema_version: None,
            database_path: Some(path_display),
        };
    }

    let schema_version: i64 = match conn.query_row(
        "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
        [],
        |row| row.get(0),
    ) {
        Ok(value) => value,
        Err(e) => {
            return DatabaseHealth {
                status: "failed".into(),
                detail: format!("Read schema version: {e}"),
                schema_version: None,
                database_path: Some(path_display),
            }
        }
    };

    if schema_version != SCHEMA_VERSION {
        return DatabaseHealth {
            status: "invalid".into(),
            detail: format!(
                "Database schema version is {schema_version}; Relintor expects {SCHEMA_VERSION}."
            ),
            schema_version: Some(schema_version),
            database_path: Some(path_display),
        };
    }

    let takeover_name: Option<String> = conn
        .query_row(
            "SELECT name FROM schema_migrations WHERE version = 3",
            [],
            |row| row.get(0),
        )
        .optional()
        .unwrap_or(None);

    let authority_name: Option<String> = conn
        .query_row(
            "SELECT name FROM schema_migrations WHERE version = 4",
            [],
            |row| row.get(0),
        )
        .optional()
        .unwrap_or(None);

    let authority_scoping_name: Option<String> = conn
        .query_row(
            "SELECT name FROM schema_migrations WHERE version = 5",
            [],
            |row| row.get(0),
        )
        .optional()
        .unwrap_or(None);

    let takeover_binding_name: Option<String> = conn
        .query_row(
            "SELECT name FROM schema_migrations WHERE version = 6",
            [],
            |row| row.get(0),
        )
        .optional()
        .unwrap_or(None);

    let workflow_state_name: Option<String> = conn
        .query_row(
            "SELECT name FROM schema_migrations WHERE version = 7",
            [],
            |row| row.get(0),
        )
        .optional()
        .unwrap_or(None);

    let baseline_name: Option<String> = conn
        .query_row(
            "SELECT name FROM schema_migrations WHERE version = 1",
            [],
            |row| row.get(0),
        )
        .optional()
        .unwrap_or(None);

    let investigator_name: Option<String> = conn
        .query_row(
            "SELECT name FROM schema_migrations WHERE version = 2",
            [],
            |row| row.get(0),
        )
        .optional()
        .unwrap_or(None);

    if baseline_name.as_deref() != Some(MIGRATION_001_NAME)
        || investigator_name.as_deref() != Some(MIGRATION_002_NAME)
        || takeover_name.as_deref() != Some(MIGRATION_003_NAME)
        || authority_name.as_deref() != Some(MIGRATION_004_NAME)
        || authority_scoping_name.as_deref() != Some(MIGRATION_005_NAME)
        || takeover_binding_name.as_deref() != Some(MIGRATION_006_NAME)
        || workflow_state_name.as_deref() != Some(MIGRATION_007_NAME)
    {
        return DatabaseHealth {
            status: "invalid".into(),
            detail: "Local migration identities do not match the expected migrations.".into(),
            schema_version: Some(schema_version),
            database_path: Some(path_display),
        };
    }

    for table in [
        "projects",
        "health_checks",
        "project_drafts",
        "investigations",
        "source_documents",
        "blueprint_revisions",
        "project_takeovers",
        "repository_snapshots",
        "takeover_inventory",
        "takeover_capabilities",
        "takeover_findings",
        "takeover_recommendations",
        "takeover_runtime_probes",
        "takeover_revisions",
        "standards_registries",
        "standards_packs",
        "standards_rules",
        "applicability_evaluations",
        "authority_requirements",
        "authority_requirement_dependencies",
        "authority_acceptance_criteria",
        "authority_evidence_policies",
        "authority_tasks",
        "authority_task_dependencies",
        "authority_task_links",
        "authority_decisions",
        "mission_drafts",
        "mission_revisions",
        "mission_seals",
        "mission_revalidation",
        "authority_reviews",
        "project_workflow_state",
    ] {
        let exists: i64 = match conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
            [table],
            |row| row.get(0),
        ) {
            Ok(value) => value,
            Err(e) => {
                return DatabaseHealth {
                    status: "failed".into(),
                    detail: format!("Inspect table {table}: {e}"),
                    schema_version: Some(schema_version),
                    database_path: Some(path_display),
                }
            }
        };

        if exists != 1 {
            return DatabaseHealth {
                status: "invalid".into(),
                detail: format!("Required table {table} is missing."),
                schema_version: Some(schema_version),
                database_path: Some(path_display),
            };
        }
    }

    DatabaseHealth {
        status: "healthy".into(),
        detail: "Local SQLite schema and integrity checks passed.".into(),
        schema_version: Some(schema_version),
        database_path: Some(path_display),
    }
}

pub fn specification_health(locked: &Path, pin_path: &Path) -> HealthStatus {
    match verify_locked_specification(locked, pin_path) {
        Ok(()) => HealthStatus {
            status: "healthy".into(),
            detail:
                "Locked specifications match the externally pinned manifest and contain exactly 144 expected feature IDs."
                    .into(),
        },
        Err(detail) => HealthStatus {
            status: "failed".into(),
            detail,
        },
    }
}

fn verify_locked_specification(locked: &Path, pin_path: &Path) -> Result<(), String> {
    if !locked.is_dir() {
        return Err(format!(
            "locked specification directory missing: {}",
            locked.display()
        ));
    }
    if !pin_path.is_file() {
        return Err(format!(
            "external manifest pin missing: {}",
            pin_path.display()
        ));
    }

    let pin = fs::read_to_string(pin_path)
        .map_err(|e| format!("read external manifest pin: {e}"))?
        .trim()
        .to_ascii_lowercase();

    if pin.len() != 64 || !pin.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("external manifest pin is not exactly one SHA-256 digest".into());
    }

    let expected_locked: BTreeSet<String> = EXPECTED_LOCKED_FILES
        .iter()
        .map(|name| (*name).to_string())
        .collect();
    let mut actual_locked = BTreeSet::new();

    for entry in fs::read_dir(locked).map_err(|e| format!("read locked directory: {e}"))? {
        let entry = entry.map_err(|e| format!("read locked directory entry: {e}"))?;
        let file_type = entry
            .file_type()
            .map_err(|e| format!("read locked entry type: {e}"))?;
        if !file_type.is_file() {
            return Err(format!(
                "unexpected non-file entry in locked authority: {}",
                entry.path().display()
            ));
        }
        actual_locked.insert(entry.file_name().to_string_lossy().to_string());
    }

    if actual_locked != expected_locked {
        return Err(format!(
            "locked file set mismatch; expected {} files, found {}",
            expected_locked.len(),
            actual_locked.len()
        ));
    }

    let manifest_path = locked.join("MANIFEST.sha256.json");
    let manifest_bytes =
        fs::read(&manifest_path).map_err(|e| format!("read locked manifest: {e}"))?;

    if sha256_hex(&manifest_bytes) != pin {
        return Err("MANIFEST.sha256.json does not match the external pinned hash".into());
    }

    let manifest: BTreeMap<String, ManifestRecord> = serde_json::from_slice(&manifest_bytes)
        .map_err(|e| format!("parse locked manifest: {e}"))?;

    let expected_manifest: BTreeSet<String> = EXPECTED_MANIFEST_FILES
        .iter()
        .map(|name| (*name).to_string())
        .collect();
    let actual_manifest: BTreeSet<String> = manifest.keys().cloned().collect();

    if actual_manifest != expected_manifest {
        return Err("manifest entries do not match the expected sealed file set".into());
    }

    for (name, record) in &manifest {
        if record.sha256.len() != 64 || !record.sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(format!("manifest SHA-256 is malformed for {name}"));
        }

        let path = locked.join(name);
        let bytes = fs::read(&path).map_err(|e| format!("read locked file {name}: {e}"))?;
        if bytes.len() as u64 != record.bytes {
            return Err(format!("byte length mismatch for {name}"));
        }
        if sha256_hex(&bytes) != record.sha256.to_ascii_lowercase() {
            return Err(format!("SHA-256 mismatch for {name}"));
        }
    }

    let features: serde_json::Value = serde_json::from_slice(
        &fs::read(locked.join("feature-register.json"))
            .map_err(|e| format!("read feature register: {e}"))?,
    )
    .map_err(|e| format!("parse feature register: {e}"))?;

    let records = features
        .get("features")
        .and_then(|value| value.as_array())
        .ok_or_else(|| "feature-register.json has no features array".to_string())?;

    if records.len() != EXPECTED_FEATURE_COUNT {
        return Err(format!(
            "expected {EXPECTED_FEATURE_COUNT} features, found {}",
            records.len()
        ));
    }

    let expected_ids = expected_feature_ids();
    let mut actual_ids = BTreeSet::new();
    for record in records {
        let id = record
            .get("id")
            .and_then(|value| value.as_str())
            .ok_or_else(|| "feature record has no id".to_string())?
            .to_string();
        if !actual_ids.insert(id.clone()) {
            return Err(format!("duplicate feature ID {id}"));
        }
    }

    if actual_ids != expected_ids {
        return Err("feature ID set does not match A-01 through L-12".into());
    }

    Ok(())
}

pub fn antigravity_detection() -> AntigravityHealth {
    let override_path = env::var_os("ANTIGRAVITY_PATH").map(PathBuf::from);
    let search_dirs: Vec<PathBuf> = env::var_os("PATH")
        .map(|value| env::split_paths(&value).collect())
        .unwrap_or_default();

    let executable = find_antigravity_executable(override_path, search_dirs.iter());

    let Some(path) = executable else {
        return AntigravityHealth {
            status: "not_installed".into(),
            version: None,
            executable: None,
            compatibility: "unknown".into(),
            cli_invocation_capability: false,
            plugin_hook_capability: false,
            ide_status: "not_verified".into(),
            cli_status: "not_installed".into(),
            authentication_status: "not_available".into(),
            executable_detectable: false,
            adapter_ready: false,
            setup_required: true,
            environment: "local".into(),
            platform: std::env::consts::OS.into(),
            detected_at_ms: now_seconds() as i128 * 1000,
            detail:
                "Antigravity CLI executable (agy) was not found. Compatibility is not inferred."
                    .into(),
        };
    };

    let version = Command::new(&path)
        .arg("--version")
        .output()
        .ok()
        .and_then(|out| {
            let stdout = String::from_utf8(out.stdout).ok().unwrap_or_default();
            let stderr = String::from_utf8(out.stderr).ok().unwrap_or_default();
            let text = if stdout.trim().is_empty() {
                stderr
            } else {
                stdout
            };
            let trimmed = text.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        });

    AntigravityHealth {
        status: "installed".into(),
        version,
        executable: Some(path.display().to_string()),
        compatibility: "unknown".into(),
        cli_invocation_capability: false,
        plugin_hook_capability: false,
        ide_status: "not_verified".into(),
        cli_status: "installed".into(),
        authentication_status: "not_verified".into(),
        executable_detectable: true,
        adapter_ready: false,
        setup_required: true,
        environment: "local".into(),
        platform: std::env::consts::OS.into(),
        detected_at_ms: now_seconds() as i128 * 1000,
        detail:
            "Antigravity CLI detected. Compatibility remains unknown until the later adapter compatibility gate."
                .into(),
    }
}

fn find_antigravity_executable<'a>(
    override_path: Option<PathBuf>,
    search_dirs: impl IntoIterator<Item = &'a PathBuf>,
) -> Option<PathBuf> {
    if let Some(path) = override_path {
        if is_executable_file(&path) {
            return Some(path);
        }
        if path.is_dir() {
            for name in antigravity_candidate_names() {
                let candidate = path.join(name);
                if is_executable_file(&candidate) {
                    return Some(candidate);
                }
            }
        }
    }

    for dir in search_dirs {
        for name in antigravity_candidate_names() {
            let candidate = dir.join(name);
            if is_executable_file(&candidate) {
                return Some(candidate);
            }
        }
    }

    None
}

fn antigravity_candidate_names() -> &'static [&'static str] {
    &[
        "agy",
        "agy.exe",
        "antigravity",
        "antigravity.exe",
        "antigravity-cli",
        "antigravity-cli.exe",
    ]
}

fn is_executable_file(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        return fs::metadata(path)
            .map(|metadata| metadata.permissions().mode() & 0o111 != 0)
            .unwrap_or(false);
    }

    #[cfg(not(unix))]
    {
        true
    }
}

fn expected_feature_ids() -> BTreeSet<String> {
    let mut ids = BTreeSet::new();
    for letter in 'A'..='L' {
        for number in 1..=12 {
            ids.insert(format!("{letter}-{number:02}"));
        }
    }
    ids
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn now_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_path(label: &str, extension: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default();
        env::temp_dir().join(format!(
            "relintor-{label}-{}-{nonce}.{extension}",
            std::process::id()
        ))
    }

    fn repository_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .map(PathBuf::from)
            .expect("repository root")
    }

    #[test]
    fn migration_is_idempotent_and_health_is_read_only() {
        let path = temp_path("migration", "sqlite");

        let first = migrate_database(&path).expect("first migration");
        let second = migrate_database(&path).expect("second migration");
        let health = database_health(&path);

        assert_eq!(first.schema_version, SCHEMA_VERSION);
        assert_eq!(
            first.applied_migrations,
            vec![
                MIGRATION_001_NAME,
                MIGRATION_002_NAME,
                MIGRATION_003_NAME,
                MIGRATION_004_NAME,
                MIGRATION_005_NAME,
                MIGRATION_006_NAME,
                MIGRATION_007_NAME
            ]
        );
        assert_eq!(second.schema_version, SCHEMA_VERSION);
        assert!(second.applied_migrations.is_empty());
        assert_eq!(health.status, "healthy");
        assert_eq!(health.schema_version, Some(SCHEMA_VERSION));

        let _ = fs::remove_file(path);
    }

    #[test]
    fn migration_refuses_a_newer_unknown_schema() {
        let path = temp_path("newer-schema", "sqlite");
        let conn = Connection::open(&path).expect("open fixture database");
        conn.execute_batch(
            "CREATE TABLE schema_migrations (
                version INTEGER PRIMARY KEY,
                name TEXT NOT NULL,
                applied_at INTEGER NOT NULL
             );
             INSERT INTO schema_migrations(version, name, applied_at)
             VALUES (99, 'future', 0);",
        )
        .expect("seed newer schema");
        drop(conn);

        let error = migrate_database(&path).expect_err("newer schema must fail closed");
        assert!(error.contains("newer than supported"));

        let _ = fs::remove_file(path);
    }

    #[test]
    fn locked_specification_requires_the_external_pin() {
        let root = repository_root();
        let health = specification_health(
            &root.join("spec").join("locked"),
            &root.join("spec").join("LOCKED_MANIFEST.sha256"),
        );
        assert_eq!(health.status, "healthy", "{}", health.detail);
    }

    #[test]
    fn expected_feature_ids_are_exact() {
        let ids = expected_feature_ids();
        assert_eq!(ids.len(), EXPECTED_FEATURE_COUNT);
        assert!(ids.contains("A-01"));
        assert!(ids.contains("L-12"));
    }

    #[test]
    fn antigravity_candidates_include_the_official_agy_cli_name() {
        assert!(antigravity_candidate_names().contains(&"agy"));
        assert!(antigravity_candidate_names().contains(&"agy.exe"));
    }

    #[test]
    fn keychain_health_never_claims_a_plaintext_fallback() {
        let health = keychain_health();
        let combined = format!("{} {} {}", health.status, health.provider, health.detail);
        assert!(!combined
            .to_ascii_lowercase()
            .contains("plaintext fallback is enabled"));
    }
}
