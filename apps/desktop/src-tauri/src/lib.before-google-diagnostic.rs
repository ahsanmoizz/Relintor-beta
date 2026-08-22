use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use rand::RngExt;
use relintor_core::{
    application_health, database_health, keychain_health, load_or_create_keychain_authority_key,
    migrate_database, specification_health, AntigravityHealth, DatabaseHealth, HealthStatus,
    KeychainHealth,
};
use relintor_distribution::{
    prepare_uninstall, EvidenceIdentity, UninstallChoice, UninstallResult,
};
use relintor_evidence::{
    environment_fingerprint, export_manifest, fingerprint_workspace, AiVerifierInput,
    AuthenticatedP7Execution, CompletionAuthority, CompletionCertificate, EvidenceManifest,
    EvidenceStore, EvidenceSummary, FreshnessContext, ProductionAiProvider, VerificationAuthority,
    VerificationCollectorOrchestrator, VerificationEngine,
};
use relintor_execution::{
    CheckpointKind, ConservativeProcessInspector, ExecutionRun, RecoveryAuthority,
    RecoveryCoordinator, RecoveryDisposition, RecoveryStore, SchedulerPolicy, SessionEndState,
};
use relintor_investigator::{
    persist_result, AnswerChoice, Blueprint, InvestigationView, Investigator, ProjectDraft,
    Provenance, UserAnswer,
};
use relintor_standards::{
    production_registry, AcceptanceCriterion, ApplicabilityContext, ApplicabilityOutcome,
    AuthorityEngine, EvidenceClass, EvidenceConfidence, EvidenceObligation, ExecutionHandoff,
    FactValue, MissionRevision, ProjectAuthorityInput, ProjectRequirementSeed, RequirementPriority,
    RequirementRisk, RequirementSource, RequirementStatus, StandardsRegistry, TrustedSignerSet,
    VerificationPolicy,
};
use relintor_takeover::{persist_takeover_for_project, TakeoverReport, TakeoverScanner};
use reqwest::blocking::Client;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Manager};
use tauri_plugin_updater::UpdaterExt;

#[derive(Debug, Serialize)]
pub struct DesktopHealth {
    pub application: HealthStatus,
    pub database: DatabaseHealth,
    pub specification: HealthStatus,
    pub keychain: KeychainHealth,
    pub antigravity: AntigravityHealth,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CloudSessionResponse {
    session_id: String,
    access_token: String,
    refresh_token: String,
    account: CloudAccount,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CloudAccount {
    user: CloudUser,
    organization: CloudOrganization,
    role: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CloudUser {
    id: String,
    email: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CloudOrganization {
    id: String,
    name: String,
}

#[derive(Debug, Clone, Serialize)]
struct AccountStateView {
    status: String,
    email: Option<String>,
    plan: Option<String>,
    entitlement: String,
    detail: String,
}

#[derive(Debug, Clone)]
struct PkcePair {
    verifier: String,
    challenge: String,
}

const EMBEDDED_CLOUD_API_ENDPOINT: &str = match option_env!("RELINTOR_CLOUD_API_ENDPOINT") {
    Some(value) => value,
    None => "https://projects-cafe-asian-saved.trycloudflare.com",
};
const EMBEDDED_GOOGLE_CLIENT_ID: &str = match option_env!("RELINTOR_GOOGLE_CLIENT_ID") {
    Some(value) => value,
    None => "1073099697494-qg60qkt0vc0odjdiema9ee2plthhg7oa.apps.googleusercontent.com",
};
const EMBEDDED_AI_GATEWAY_ENDPOINT: &str = match option_env!("RELINTOR_AI_GATEWAY_ENDPOINT") {
    Some(value) => value,
    None => "https://governments-amsterdam-incoming-previews.trycloudflare.com",
};

#[derive(Debug, Clone, PartialEq, Eq)]
struct DesktopPublicConfig {
    cloud_api_endpoint: String,
    google_client_id: String,
    ai_gateway_endpoint: String,
}

impl DesktopPublicConfig {
    fn from_env() -> Result<Self, String> {
        Self::from_sources(
            std::env::var("RELINTOR_CLOUD_API_ENDPOINT").ok(),
            std::env::var("RELINTOR_GOOGLE_CLIENT_ID").ok(),
            std::env::var("RELINTOR_AI_GATEWAY_ENDPOINT").ok(),
        )
    }

    fn from_sources(
        cloud_override: Option<String>,
        client_override: Option<String>,
        ai_override: Option<String>,
    ) -> Result<Self, String> {
        Ok(Self {
            cloud_api_endpoint: required_https_public_value(
                "RELINTOR_CLOUD_API_ENDPOINT",
                cloud_override.as_deref(),
                EMBEDDED_CLOUD_API_ENDPOINT,
            )?,
            google_client_id: required_public_value(
                "RELINTOR_GOOGLE_CLIENT_ID",
                client_override.as_deref(),
                EMBEDDED_GOOGLE_CLIENT_ID,
            )?,
            ai_gateway_endpoint: required_https_public_value(
                "RELINTOR_AI_GATEWAY_ENDPOINT",
                ai_override.as_deref(),
                EMBEDDED_AI_GATEWAY_ENDPOINT,
            )?,
        })
    }
}

fn required_public_value(
    name: &str,
    runtime_override: Option<&str>,
    embedded_value: &str,
) -> Result<String, String> {
    let value = runtime_override.unwrap_or(embedded_value).trim();
    if value.is_empty() {
        return Err(format!("{name} is required and cannot be empty"));
    }
    Ok(value.to_string())
}

fn required_https_public_value(
    name: &str,
    runtime_override: Option<&str>,
    embedded_value: &str,
) -> Result<String, String> {
    let value = required_public_value(name, runtime_override, embedded_value)?;
    let parsed = url::Url::parse(&value).map_err(|_| format!("{name} must be a valid URL"))?;
    if parsed.scheme() != "https" || parsed.host_str().is_none() {
        return Err(format!("{name} must use HTTPS"));
    }
    Ok(value)
}

fn cloud_access_token() -> Option<String> {
    cloud_session()
        .lock()
        .ok()
        .and_then(|session| session.as_ref().map(|value| value.access_token.clone()))
}

static CLOUD_SESSION: OnceLock<Mutex<Option<CloudSessionResponse>>> = OnceLock::new();

fn cloud_session() -> &'static Mutex<Option<CloudSessionResponse>> {
    CLOUD_SESSION.get_or_init(|| Mutex::new(None))
}

fn account_state_view() -> AccountStateView {
    let Some(session) = cloud_session().lock().ok().and_then(|guard| guard.clone()) else {
        return AccountStateView {
            status: "signed_out".into(),
            email: None,
            plan: None,
            entitlement: "UNAVAILABLE".into(),
            detail: "Sign in with Google to use authenticated cloud features.".into(),
        };
    };
    AccountStateView {
        status: "signed_in".into(),
        email: Some(session.account.user.email),
        plan: None,
        entitlement: "UNAVAILABLE".into(),
        detail: "Signed in through Google; cloud entitlement is resolved by the server.".into(),
    }
}

fn random_url_token() -> String {
    let mut bytes = [0u8; 32];
    rand::rng().fill(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

fn pkce_pair() -> PkcePair {
    let verifier = random_url_token();
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    PkcePair {
        verifier,
        challenge,
    }
}

fn callback_url(port: u16) -> String {
    format!("http://127.0.0.1:{port}/oauth/callback")
}

fn parse_loopback_callback(request: &str, expected_state: &str) -> Result<String, String> {
    let request_line = request
        .lines()
        .next()
        .ok_or("authentication callback was empty")?;
    let target = request_line
        .strip_prefix("GET ")
        .and_then(|value| value.split_whitespace().next())
        .ok_or("authentication callback was invalid")?;
    let url = url::Url::parse(&format!("http://127.0.0.1{target}"))
        .map_err(|_| "authentication callback was invalid".to_string())?;
    if url.path() != "/oauth/callback" {
        return Err("authentication callback path was invalid".into());
    }
    let query = url.query_pairs().collect::<BTreeMap<_, _>>();
    if query.get("state").map(|value| value.as_ref()) != Some(expected_state) {
        return Err("authentication state did not match".into());
    }
    if let Some(error) = query.get("error") {
        return Err(match error.as_ref() {
            "access_denied" => "authentication cancelled".into(),
            _ => "authentication failed".into(),
        });
    }
    query
        .get("code")
        .filter(|code| !code.trim().is_empty())
        .map(|code| code.to_string())
        .ok_or_else(|| "authentication failed".into())
}

fn write_callback_response(stream: &mut TcpStream, message: &str) {
    let body = format!("<html><body>{message}. You may close this window.</body></html>");
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(), body
    );
    let _ = stream.write_all(response.as_bytes());
}

fn wait_for_loopback_callback(listener: &TcpListener, state: &str) -> Result<String, String> {
    listener
        .set_nonblocking(true)
        .map_err(|_| "authentication callback could not be prepared".to_string())?;
    let deadline = SystemTime::now() + Duration::from_secs(180);
    loop {
        match listener.accept() {
            Ok((mut stream, _)) => {
                let mut buffer = [0u8; 16 * 1024];
                let count = stream
                    .read(&mut buffer)
                    .map_err(|_| "authentication callback could not be read".to_string())?;
                let request = String::from_utf8_lossy(&buffer[..count]);
                let result = parse_loopback_callback(&request, state);
                write_callback_response(
                    &mut stream,
                    if result.is_ok() {
                        "Authentication complete"
                    } else {
                        "Authentication was not completed"
                    },
                );
                return result;
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                if SystemTime::now() >= deadline {
                    return Err("authentication timed out".into());
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(_) => return Err("authentication callback failed".into()),
        }
    }
}

fn open_system_browser(url: &str) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("rundll32.exe")
            .arg("url.dll,FileProtocolHandler")
            .arg(url)
            .spawn()
            .map_err(|_| "could not open the system browser".to_string())?;
        return Ok(());
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(url)
            .spawn()
            .map_err(|_| "could not open the system browser".to_string())?;
        return Ok(());
    }
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(url)
            .spawn()
            .map_err(|_| "could not open the system browser".to_string())?;
        return Ok(());
    }
    #[allow(unreachable_code)]
    Err("system browser integration is unavailable on this platform".into())
}

fn https_client() -> Result<Client, String> {
    Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|_| "could not construct the authentication client".into())
}

fn exchange_google_code(
    client: &Client,
    code: &str,
    verifier: &str,
    redirect_uri: &str,
    client_id: &str,
) -> Result<String, String> {
    let mut form = url::form_urlencoded::Serializer::new(String::new());
    form.append_pair("code", code);
    form.append_pair("client_id", client_id);
    form.append_pair("code_verifier", verifier);
    form.append_pair("redirect_uri", redirect_uri);
    form.append_pair("grant_type", "authorization_code");
    let response = client
        .post("https://oauth2.googleapis.com/token")
        .header("content-type", "application/x-www-form-urlencoded")
        .body(form.finish())
        .send()
        .map_err(|_| "Google authentication failed".to_string())?;
    if !response.status().is_success() {
        return Err("Google authentication failed".into());
    }
    let value: serde_json::Value = response
        .json()
        .map_err(|_| "Google authentication failed".to_string())?;
    value
        .get("id_token")
        .and_then(serde_json::Value::as_str)
        .filter(|token| !token.is_empty())
        .map(str::to_string)
        .ok_or_else(|| "Google authentication failed".into())
}

fn exchange_cloud_session(
    client: &Client,
    cloud_endpoint: &str,
    id_token: &str,
) -> Result<CloudSessionResponse, String> {
    let base = url::Url::parse(cloud_endpoint)
        .map_err(|_| "cloud authentication endpoint is invalid".to_string())?;
    if base.scheme() != "https" {
        return Err("cloud authentication endpoint must use HTTPS".into());
    }
    let endpoint = base
        .join("/v1/auth/google/exchange")
        .map_err(|_| "cloud authentication endpoint is invalid".to_string())?;
    let response = client
        .post(endpoint)
        .json(&serde_json::json!({"id_token": id_token, "device_name": "Relintor Windows Beta"}))
        .send()
        .map_err(|_| "cloud authentication failed".to_string())?;
    if !response.status().is_success() {
        return Err("cloud authentication failed".into());
    }
    response
        .json()
        .map_err(|_| "cloud authentication returned an invalid session".into())
}

#[tauri::command]
fn account_state() -> AccountStateView {
    account_state_view()
}

#[tauri::command]
fn sign_out() -> Result<AccountStateView, String> {
    if let Ok(mut session) = cloud_session().lock() {
        *session = None;
    }
    Ok(account_state_view())
}

#[tauri::command]
fn sign_in_with_google() -> Result<AccountStateView, String> {
    let config = DesktopPublicConfig::from_env()?;
    let listener = TcpListener::bind("127.0.0.1:0")
        .map_err(|_| "could not open the local authentication callback".to_string())?;
    let redirect_uri = callback_url(
        listener
            .local_addr()
            .map_err(|_| "authentication callback failed")?
            .port(),
    );
    let state = random_url_token();
    let pkce = pkce_pair();
    let mut query = url::form_urlencoded::Serializer::new(String::new());
    query.append_pair("client_id", &config.google_client_id);
    query.append_pair("redirect_uri", &redirect_uri);
    query.append_pair("response_type", "code");
    query.append_pair("scope", "openid email profile");
    query.append_pair("state", &state);
    query.append_pair("code_challenge", &pkce.challenge);
    query.append_pair("code_challenge_method", "S256");
    let authorization_url = format!(
        "https://accounts.google.com/o/oauth2/v2/auth?{}",
        query.finish()
    );
    open_system_browser(&authorization_url)
        .map_err(|_| "could not open the system browser".to_string())?;
    let code = wait_for_loopback_callback(&listener, &state)?;
    let client = https_client()?;
    let id_token = exchange_google_code(
        &client,
        &code,
        &pkce.verifier,
        &redirect_uri,
        &config.google_client_id,
    )?;
    let session = exchange_cloud_session(&client, &config.cloud_api_endpoint, &id_token)?;
    let state = AccountStateView {
        status: "signed_in".into(),
        email: Some(session.account.user.email.clone()),
        plan: None,
        entitlement: "UNAVAILABLE".into(),
        detail: "Signed in through Google; cloud entitlement is resolved by the server.".into(),
    };
    if let Ok(mut stored) = cloud_session().lock() {
        *stored = Some(session);
    }
    Ok(state)
}

#[derive(Debug, Clone, Serialize)]
struct UpdateStatusView {
    status: String,
    available: bool,
    current_version: String,
    target_version: Option<String>,
    notes: Option<String>,
    detail: String,
}

#[derive(Debug, Clone, Deserialize)]
struct UninstallRequest {
    choice: String,
    export_destination: Option<String>,
    mission_id: String,
    project_id: String,
}

#[derive(Debug, Serialize)]
struct AuthorityPackPreview {
    pack_id: String,
    title: String,
    applicable_rules: usize,
    not_applicable_rules: usize,
    explanation: String,
}

#[derive(Debug, Serialize)]
struct AuthorityRequirementPreview {
    requirement_id: String,
    title: String,
    source: String,
    priority: String,
    state: String,
    why_required: String,
    acceptance: Vec<String>,
    evidence: Vec<String>,
    dependencies: Vec<String>,
}

#[derive(Debug, Serialize)]
struct AuthorityPreview {
    registry_version: u64,
    total_rules: usize,
    applicable_rules: usize,
    not_applicable_rules: usize,
    packs: Vec<AuthorityPackPreview>,
    requirements: Vec<AuthorityRequirementPreview>,
    task_count: usize,
    sealing_state: String,
    blockers: Vec<String>,
    review_digest: String,
}

#[derive(Debug, Serialize)]
struct TakeoverScanResult {
    #[serde(flatten)]
    report: TakeoverReport,
    project_id: String,
}

#[derive(Debug, Serialize)]
struct ExecutionEventView {
    sequence: u64,
    occurred_at_ms: u64,
    task_id: Option<String>,
    kind: String,
    detail: String,
}

#[derive(Debug, Serialize)]
struct ExecutionStatusView {
    project_id: String,
    mission_id: String,
    revision: u64,
    state: String,
    watchdog_state: String,
    current_turn: u32,
    active_task: Option<String>,
    runnable_tasks: Vec<String>,
    total_tasks: usize,
    finished_tasks: usize,
    tool_calls: u64,
    execution_steps: u64,
    estimated_cost_micros: Option<u64>,
    safe_boundary_reached: bool,
    last_event: Option<String>,
    events: Vec<ExecutionEventView>,
    ledger_path: String,
    recovery_state: String,
    last_safe_checkpoint: Option<String>,
    resume_disposition: Option<String>,
    resume_blocker: Option<String>,
    external_changes: Vec<String>,
    recovery_detected: bool,
}

#[derive(Debug, Serialize, Clone)]
struct VerificationStatusView {
    project_id: String,
    mission_id: String,
    revision: u64,
    execution_run_id: String,
    state: String,
    completion_state: String,
    requirements_verified: usize,
    requirements_total: usize,
    missing_evidence: Vec<String>,
    failed_checks: Vec<String>,
    skipped_checks: Vec<String>,
    stale_evidence: Vec<String>,
    blocked_external: Vec<String>,
    accepted_risks: Vec<String>,
    evidence_count: usize,
    certificate: Option<CompletionCertificateView>,
    detail: String,
}

#[derive(Debug, Serialize, Clone)]
struct CompletionCertificateView {
    certificate_id: String,
    final_state: String,
    digest: String,
}

#[derive(Debug, Serialize, Clone)]
struct VerificationEvidenceView {
    evidence_id: String,
    class: String,
    result: String,
    confidence: String,
    digest: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum AuthorityFactDecisionValue {
    Yes,
    No,
    NotSure,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct AuthorityFactDecision {
    id: String,
    decision: AuthorityFactDecisionValue,
}

const AUTHORITY_FACT_FIELDS: &[&str] = &[
    "web",
    "backend",
    "database",
    "authentication",
    "ui_surface",
    "seo_relevance",
    "performance",
    "deployment",
    "observability",
    "privacy",
    "payments",
    "ai",
    "blockchain",
    "mobile",
    "desktop",
    "data_engineering",
    "integrations",
];

#[cfg(debug_assertions)]
fn workspace_root() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .ancestors()
        .nth(3)
        .map(PathBuf::from)
        .unwrap_or(manifest_dir)
}

fn database_path(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map(|dir| dir.join("relintor.sqlite"))
        .map_err(|e| format!("resolve app data directory: {e}"))
}

fn specification_paths(app: &AppHandle) -> Result<(PathBuf, PathBuf), String> {
    let resource_dir = app
        .path()
        .resource_dir()
        .map_err(|e| format!("resolve application resource directory: {e}"))?;

    let resource_locked = resource_dir.join("spec").join("locked");
    let resource_pin = resource_dir.join("spec").join("LOCKED_MANIFEST.sha256");

    if resource_locked.is_dir() && resource_pin.is_file() {
        return Ok((resource_locked, resource_pin));
    }

    #[cfg(debug_assertions)]
    {
        let root = workspace_root();
        let development_locked = root.join("spec").join("locked");
        let development_pin = root.join("spec").join("LOCKED_MANIFEST.sha256");
        if development_locked.is_dir() && development_pin.is_file() {
            return Ok((development_locked, development_pin));
        }
    }

    Err("bundled locked specification resources are unavailable".into())
}

#[tauri::command]
fn health_application() -> HealthStatus {
    application_health()
}

#[tauri::command]
fn health_database(app: AppHandle) -> DatabaseHealth {
    match database_path(&app) {
        Ok(path) => database_health(&path),
        Err(error) => DatabaseHealth::unavailable(error),
    }
}

#[tauri::command]
fn health_specification(app: AppHandle) -> HealthStatus {
    match specification_paths(&app) {
        Ok((locked, pin)) => specification_health(&locked, &pin),
        Err(detail) => HealthStatus {
            status: "unavailable".into(),
            detail,
        },
    }
}

#[tauri::command]
fn health_keychain() -> KeychainHealth {
    keychain_health()
}

#[tauri::command]
fn health_antigravity() -> AntigravityHealth {
    let report = relintor_antigravity::detect();
    let status = match report.compatibility {
        relintor_antigravity::Compatibility::Supported => "compatible",
        relintor_antigravity::Compatibility::Unsupported => "unsupported",
        relintor_antigravity::Compatibility::Unknown => "unknown",
        relintor_antigravity::Compatibility::NotInstalled => "not_installed",
    };
    AntigravityHealth {
        status: status.into(),
        version: report.version,
        executable: report.executable_path,
        compatibility: format!("{:?}", report.compatibility).to_ascii_lowercase(),
        cli_invocation_capability: report.cli_invocation_capability,
        plugin_hook_capability: report.plugin_hook_capability,
        environment: report.environment,
        platform: report.platform,
        detected_at_ms: report.detected_at_ms,
        detail: report.detail,
    }
}

#[tauri::command]
fn health_all(app: AppHandle) -> DesktopHealth {
    DesktopHealth {
        application: application_health(),
        database: health_database(app.clone()),
        specification: health_specification(app),
        keychain: keychain_health(),
        antigravity: health_antigravity(),
    }
}

fn updater_builder(app: &AppHandle) -> Result<tauri_plugin_updater::UpdaterBuilder, String> {
    let public_key = std::env::var("RELINTOR_UPDATE_PUBLIC_KEY").map_err(|_| {
        "BLOCKED_EXTERNAL: production updater public key is not configured".to_string()
    })?;
    if public_key.trim().is_empty() {
        return Err("BLOCKED_EXTERNAL: production updater public key is empty".into());
    }
    let endpoint = std::env::var("RELINTOR_UPDATE_ENDPOINT").map_err(|_| {
        "BLOCKED_EXTERNAL: production updater endpoint is not configured".to_string()
    })?;
    let local_test_endpoint = std::env::var("RELINTOR_ALLOW_LOCAL_UPDATE_TEST")
        .ok()
        .as_deref()
        == Some("1");
    if !endpoint.starts_with("https://")
        && !(local_test_endpoint
            && (endpoint.starts_with("http://127.0.0.1")
                || endpoint.starts_with("http://localhost")))
    {
        return Err("production updater endpoints must use HTTPS".into());
    }
    let endpoint = endpoint
        .parse::<url::Url>()
        .map_err(|error| format!("invalid updater endpoint: {error}"))?;
    app.updater_builder()
        .pubkey(public_key)
        .endpoints(vec![endpoint])
        .map_err(|error| format!("invalid updater endpoint: {error}"))
}

#[tauri::command]
async fn update_check(app: AppHandle) -> Result<UpdateStatusView, String> {
    let updater = updater_builder(&app)?
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|error| format!("updater configuration failed: {error}"))?;
    let current_version = env!("CARGO_PKG_VERSION").to_string();
    let update = updater
        .check()
        .await
        .map_err(|error| format!("update metadata unavailable: {error}"))?;
    Ok(match update {
        Some(update) => UpdateStatusView {
            status: "available".into(),
            available: true,
            current_version,
            target_version: Some(update.version),
            notes: update.body,
            detail: "signed updater metadata was verified by the Rust Tauri updater boundary"
                .into(),
        },
        None => UpdateStatusView {
            status: "current".into(),
            available: false,
            current_version,
            target_version: None,
            notes: None,
            detail: "update service reported no applicable signed update".into(),
        },
    })
}

#[tauri::command]
async fn update_apply(app: AppHandle) -> Result<UpdateStatusView, String> {
    let updater = updater_builder(&app)?
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|error| format!("updater configuration failed: {error}"))?;
    let current_version = env!("CARGO_PKG_VERSION").to_string();
    let Some(update) = updater
        .check()
        .await
        .map_err(|error| format!("update metadata unavailable: {error}"))?
    else {
        return Ok(UpdateStatusView {
            status: "current".into(),
            available: false,
            current_version,
            target_version: None,
            notes: None,
            detail: "update service reported no applicable signed update".into(),
        });
    };
    let target_version = update.version.clone();
    update
        .download_and_install(|_, _| {}, || {})
        .await
        .map_err(|error| format!("signed update application failed: {error}"))?;
    Ok(UpdateStatusView {
        status: "installed".into(),
        available: false,
        current_version,
        target_version: Some(target_version),
        notes: None,
        detail: "signed update downloaded, verified, and handed to the supported installer".into(),
    })
}

#[tauri::command]
fn uninstall_prepare(app: AppHandle, request: UninstallRequest) -> Result<UninstallResult, String> {
    let choice = match request.choice.as_str() {
        "PRESERVE" => UninstallChoice::Preserve,
        "EXPORT_THEN_REMOVE" => UninstallChoice::ExportThenRemove,
        "REMOVE_ALLOWED_STATE" => UninstallChoice::RemoveAllowedState,
        _ => return Err("an explicit supported uninstall choice is required".into()),
    };
    if request.mission_id.trim().is_empty() || request.project_id.trim().is_empty() {
        return Err("mission and project identity are required".into());
    }
    let app_data = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("resolve Relintor state directory: {error}"))?;
    let destination = request.export_destination.as_deref().map(PathBuf::from);
    let identity = EvidenceIdentity {
        account_id: sha256_hex(app_data.to_string_lossy().as_bytes()),
        mission_id: request.mission_id,
        project_id: request.project_id,
    };
    prepare_uninstall(&app_data, destination.as_deref(), identity, choice)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn investigator_new_project(app: AppHandle, idea: String) -> Result<InvestigationView, String> {
    let project = ProjectDraft::from_idea(&idea).map_err(|error| error.to_string())?;
    let result = Investigator::default().investigate(project, Vec::new(), &[])?;
    let path = database_path(&app)?;
    migrate_database(&path)?;
    persist_result(&path, &result)?;
    Ok(InvestigationView::from(&result))
}

#[derive(Debug, Deserialize)]
struct InvestigatorAnswerRequest {
    question_id: String,
    option_id: Option<String>,
    not_sure: bool,
}

#[tauri::command]
fn investigator_answer_questions(
    app: AppHandle,
    idea: String,
    answers: Vec<InvestigatorAnswerRequest>,
) -> Result<InvestigationView, String> {
    let project = ProjectDraft::from_idea(&idea).map_err(|error| error.to_string())?;
    let investigator = Investigator::default();
    let initial = investigator.investigate(project.clone(), Vec::new(), &[])?;
    let answers = answers
        .into_iter()
        .map(|answer| {
            let question_id = answer.question_id;
            let answer_value = if answer.not_sure {
                AnswerChoice::NotSure
            } else {
                AnswerChoice::Selected(answer.option_id.unwrap_or_default())
            };
            let choice_text = match &answer_value {
                AnswerChoice::Selected(option) => format!("selected:{option}"),
                AnswerChoice::NotSure => "not-sure".into(),
                AnswerChoice::Deferred => "deferred".into(),
            };
            let payload = format!(
                "question={};choice={choice_text};not_sure={}",
                question_id, answer.not_sure
            );
            let answer_hash = sha256_hex(payload.as_bytes());
            let answer_id = format!("answer_{}_{}", question_id, &answer_hash[..16]);
            UserAnswer {
                id: answer_id,
                question_id: question_id.clone(),
                choice: answer_value,
                explanation: if answer.not_sure {
                    "User selected Iâ€™m not sure; Relintor applied its recommendation as an assumption.".into()
                } else {
                    "User selected an option during blueprint review.".into()
                },
                confirmed: !answer.not_sure,
                provenance: Provenance {
                    source_id: format!("user-answer-{question_id}"),
                    locator: format!("user://investigator-answer/{question_id}"),
                    content_hash: answer_hash,
                    note: "Explicit review input; not inferred from project evidence.".into(),
                },
                created_at: 0,
            }
        })
        .collect::<Vec<_>>();
    let result = investigator.investigate(project, initial.sources, &answers)?;
    let path = database_path(&app)?;
    migrate_database(&path)?;
    persist_result(&path, &result)?;
    Ok(InvestigationView::from(&result))
}

#[tauri::command]
fn takeover_scan(app: AppHandle, root: String) -> Result<TakeoverScanResult, String> {
    let report = TakeoverScanner::default()
        .scan(&PathBuf::from(root))
        .map_err(|error| error.to_string())?;
    let path = database_path(&app)?;
    migrate_database(&path)?;
    let project_id = format!("takeover-project-{}", report.takeover.id);
    let connection =
        Connection::open(&path).map_err(|error| format!("open project database: {error}"))?;
    connection
        .execute(
            "INSERT INTO projects(id, name, root_path, created_at) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(id) DO UPDATE SET name=excluded.name, root_path=excluded.root_path",
            params![
                project_id.as_str(),
                format!("Takeover: {}", report.takeover.root),
                report.takeover.root.as_str(),
                report.takeover.created_at as i64
            ],
        )
        .map_err(|error| format!("create takeover project identity: {error}"))?;
    persist_takeover_for_project(&path, &report, &project_id).map_err(|error| error.to_string())?;
    Ok(TakeoverScanResult { report, project_id })
}

#[tauri::command]
fn authority_preview(
    app: AppHandle,
    project_id: String,
    facts: Vec<AuthorityFactDecision>,
) -> Result<AuthorityPreview, String> {
    let path = database_path(&app)?;
    migrate_database(&path)?;
    review_authority_at_path(&path, &project_id, &facts)
}

fn review_authority_at_path(
    path: &Path,
    project_id: &str,
    facts: &[AuthorityFactDecision],
) -> Result<AuthorityPreview, String> {
    let (registry, trusted) = production_registry().map_err(|error| error.to_string())?;
    let connection =
        Connection::open(path).map_err(|error| format!("open authority database: {error}"))?;
    let (project_authority, mut context, source_fingerprint, takeover_fingerprint) =
        build_project_authority(&connection, project_id)?;
    apply_reviewed_facts(&mut context, facts)?;
    let project_source_revision = project_authority.source_revision.clone();
    let mission_id = format!("mission-{project_id}");
    let scope = relintor_standards::scope_fingerprint(BTreeMap::<String, String>::from([
        ("project_id".into(), project_id.to_string()),
        ("registry_digest".into(), registry.registry_digest.clone()),
        ("source_fingerprint".into(), source_fingerprint),
    ]));
    let engine = AuthorityEngine;
    let draft = engine
        .build_draft_with_project_authority(
            &mission_id,
            project_id,
            &project_source_revision,
            &takeover_fingerprint,
            registry.clone(),
            &context,
            scope,
            project_authority,
            Vec::new(),
        )
        .map_err(|error| format!("build authority review: {error}"))?;
    let review_digest = authority_review_digest(&draft)?;
    let canonical_facts = canonicalize_reviewed_facts(facts)?;
    let facts_json = serde_json::to_string(&canonical_facts)
        .map_err(|error| format!("serialize authority review facts: {error}"))?;
    connection
        .execute(
            "INSERT INTO authority_reviews(project_id, review_digest, facts_json, reviewed_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(project_id) DO UPDATE SET review_digest=excluded.review_digest,
             facts_json=excluded.facts_json, reviewed_at=excluded.reviewed_at",
            params![
                project_id,
                review_digest.as_str(),
                facts_json,
                current_timestamp()
            ],
        )
        .map_err(|error| format!("persist authority review: {error}"))?;
    let ledger = &draft.applicability;
    let graph = &draft.requirement_graph;
    let blockers = match engine.preseal(&draft, &trusted) {
        Ok(()) => Vec::new(),
        Err(error) => vec![error.to_string()],
    };
    let tasks = engine
        .decompose_tasks(graph)
        .map_err(|error| error.to_string())?;
    let applicable_rules = ledger
        .iter()
        .filter(|result| result.outcome == ApplicabilityOutcome::Applicable)
        .count();
    let not_applicable_rules = ledger
        .iter()
        .filter(|result| result.outcome == ApplicabilityOutcome::NotApplicable)
        .count();
    let packs = registry
        .packs
        .iter()
        .map(|pack| {
            let pack_results = ledger
                .iter()
                .filter(|result| pack.rules.iter().any(|rule| rule.rule_id == result.rule_id));
            let results = pack_results.collect::<Vec<_>>();
            let applicable = results
                .iter()
                .filter(|result| result.outcome == ApplicabilityOutcome::Applicable)
                .count();
            let not_applicable = results
                .iter()
                .filter(|result| result.outcome == ApplicabilityOutcome::NotApplicable)
                .count();
            let explanation = results
                .first()
                .map(|result| result.reason.clone())
                .unwrap_or_else(|| "No deterministic applicability result was produced.".into());
            AuthorityPackPreview {
                pack_id: pack.pack_id.clone(),
                title: pack.title.clone(),
                applicable_rules: applicable,
                not_applicable_rules: not_applicable,
                explanation,
            }
        })
        .collect();
    let requirements = graph
        .requirements
        .iter()
        .map(|requirement| AuthorityRequirementPreview {
            requirement_id: requirement.requirement_id.clone(),
            title: requirement.title.clone(),
            source: format!("{:?}", requirement.source),
            priority: format!("{:?}", requirement.priority),
            state: format!("{:?}", requirement.status),
            why_required: requirement.intent.clone(),
            acceptance: requirement
                .acceptance_criteria
                .iter()
                .map(|criterion| criterion.statement.clone())
                .collect(),
            evidence: requirement
                .verification_policy
                .obligations
                .iter()
                .map(|obligation| format!("{:?}", obligation.class))
                .collect(),
            dependencies: requirement.dependencies.clone(),
        })
        .collect();
    Ok(AuthorityPreview {
        registry_version: registry.registry_version,
        total_rules: ledger.len(),
        applicable_rules,
        not_applicable_rules,
        packs,
        requirements,
        task_count: tasks.tasks.len(),
        sealing_state: if blockers.is_empty() {
            "READY_TO_SEAL".into()
        } else {
            "BLOCKED".into()
        },
        blockers,
        review_digest,
    })
}

fn apply_reviewed_facts(
    context: &mut ApplicabilityContext,
    facts: &[AuthorityFactDecision],
) -> Result<(), String> {
    let canonical_facts = canonicalize_reviewed_facts(facts)?;
    for fact in &canonical_facts {
        match fact.decision {
            AuthorityFactDecisionValue::Yes => {
                context.facts.insert(fact.id.clone(), FactValue::Bool(true));
            }
            AuthorityFactDecisionValue::No => {
                context
                    .facts
                    .insert(fact.id.clone(), FactValue::Bool(false));
            }
            AuthorityFactDecisionValue::NotSure => {
                if !matches!(context.facts.get(&fact.id), Some(FactValue::Bool(true))) {
                    context.facts.remove(&fact.id);
                }
            }
        }
    }
    context.revision = format!(
        "desktop-authority-review-{}",
        sha256_hex(
            serde_json::to_string(&canonical_facts)
                .map_err(|error| format!("serialize authority fact decisions: {error}"))?
                .as_bytes(),
        )
    );
    Ok(())
}

fn canonicalize_reviewed_facts(
    facts: &[AuthorityFactDecision],
) -> Result<Vec<AuthorityFactDecision>, String> {
    let mut canonical = BTreeMap::<String, AuthorityFactDecisionValue>::new();
    for fact in facts {
        let id = fact.id.trim().to_ascii_lowercase();
        if !AUTHORITY_FACT_FIELDS.contains(&id.as_str()) {
            return Err(format!("unsupported authority fact: {}", fact.id));
        }
        if let Some(previous) = canonical.insert(id.clone(), fact.decision.clone()) {
            if previous != fact.decision {
                return Err(format!("conflicting authority decisions for fact: {id}"));
            }
        }
    }
    Ok(canonical
        .into_iter()
        .map(|(id, decision)| AuthorityFactDecision { id, decision })
        .collect())
}

fn authority_review_digest(draft: &relintor_standards::MissionDraft) -> Result<String, String> {
    serde_json::to_vec(draft)
        .map(|bytes| sha256_hex(&bytes))
        .map_err(|error| format!("serialize authority review: {error}"))
}

fn current_timestamp() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().to_string())
        .unwrap_or_else(|_| "0".into())
}

fn project_seed(
    requirement_id: impl Into<String>,
    title: impl Into<String>,
    intent: impl Into<String>,
    source: RequirementSource,
    risk: RequirementRisk,
    requirement_type: &str,
) -> ProjectRequirementSeed {
    let requirement_id = requirement_id.into();
    let title = title.into();
    let intent = intent.into();
    let (verification_policy, machine_checkable) =
        imported_evidence_policy(requirement_type, &title, &intent);
    ProjectRequirementSeed {
        requirement_id: Some(requirement_id.clone()),
        title: title.clone(),
        intent,
        source,
        priority: match risk {
            RequirementRisk::Critical => RequirementPriority::P0,
            RequirementRisk::High => RequirementPriority::P1,
            RequirementRisk::Medium => RequirementPriority::P2,
            RequirementRisk::Low => RequirementPriority::P3,
        },
        acceptance_criteria: vec![AcceptanceCriterion {
            criterion_id: format!("criterion-{requirement_id}"),
            statement: format!("A reviewable evidence record demonstrates: {title}"),
            criterion_type: "project-authority-obligation".into(),
            machine_checkable,
        }],
        verification_policy,
        dependencies: Vec::new(),
        risk,
        requirement_type: requirement_type.into(),
    }
}

fn imported_evidence_policy(
    requirement_type: &str,
    title: &str,
    intent: &str,
) -> (VerificationPolicy, bool) {
    let normalized = format!("{requirement_type} {title} {intent}").to_ascii_lowercase();
    let (classes, confidence, rationale) = if requirement_type == "decision"
        || normalized.contains("approval")
        || normalized.contains("decision")
    {
        (
            vec![EvidenceClass::HumanDecision],
            EvidenceConfidence::HumanAsserted,
            "A genuine project decision requires explicit human review.",
        )
    } else if normalized.contains("security") || normalized.contains("auth") {
        (
            vec![EvidenceClass::SecurityScan],
            EvidenceConfidence::StrongDeterministic,
            "Security obligations require a security scan collected by P8.",
        )
    } else if normalized.contains("performance") || normalized.contains("latency") {
        (
            vec![EvidenceClass::PerformanceResult],
            EvidenceConfidence::StrongRuntime,
            "Performance obligations require measured performance evidence.",
        )
    } else if normalized.contains("accessibility") || normalized.contains("a11y") {
        (
            vec![EvidenceClass::AccessibilityResult],
            EvidenceConfidence::StrongRuntime,
            "Accessibility obligations require an accessibility result.",
        )
    } else if normalized.contains("database") || normalized.contains("migration") {
        (
            vec![EvidenceClass::DatabaseQuery, EvidenceClass::TestOutput],
            EvidenceConfidence::StrongDeterministic,
            "Database and migration obligations require query and test evidence.",
        )
    } else if normalized.contains("deployment") || normalized.contains("release") {
        (
            vec![EvidenceClass::DeploymentProbe],
            EvidenceConfidence::StrongRuntime,
            "Deployment obligations require a bounded deployment probe.",
        )
    } else {
        (
            vec![EvidenceClass::TestOutput],
            EvidenceConfidence::StrongRuntime,
            "Functional and remediation obligations require deterministic or runtime test evidence.",
        )
    };
    (
        VerificationPolicy {
            obligations: classes
                .into_iter()
                .map(|class| EvidenceObligation {
                    class,
                    minimum_confidence: confidence,
                    rationale: rationale.into(),
                    required: true,
                })
                .collect(),
            p8_collector_required: true,
        },
        confidence != EvidenceConfidence::HumanAsserted,
    )
}

fn build_project_authority(
    connection: &Connection,
    project_id: &str,
) -> Result<(ProjectAuthorityInput, ApplicabilityContext, String, String), String> {
    let persisted_blueprint: Option<(String, u32, String, String)> = connection
        .query_row(
            "SELECT p.idea, b.revision, b.fingerprint, b.normalized_blueprint
             FROM project_drafts p
             JOIN investigations i ON i.project_id = p.id
             JOIN blueprint_revisions b ON b.investigation_id = i.id
             WHERE p.id = ?1
             ORDER BY b.revision DESC
             LIMIT 1",
            params![&project_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()
        .map_err(|error| format!("load approved project blueprint: {error}"))?;
    let (idea, blueprint_revision, blueprint_fingerprint, blueprint) = match persisted_blueprint {
        Some((idea, revision, fingerprint, normalized)) => (
            idea,
            revision,
            fingerprint,
            Some(
                serde_json::from_str::<Blueprint>(&normalized)
                    .map_err(|error| format!("decode persisted project blueprint: {error}"))?,
            ),
        ),
        None => (
            String::new(),
            0,
            format!("takeover-project-{project_id}"),
            None,
        ),
    };
    let takeover: Option<(String, String)> = connection
        .query_row(
            "SELECT id, fingerprint FROM project_takeovers
             WHERE project_id = ?1 ORDER BY created_at DESC LIMIT 1",
            params![&project_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(|error| format!("load project-bound takeover authority: {error}"))?;

    let mut requirements = Vec::new();
    let (mut architecture_decisions, mut assumptions, mut risks) = if let Some(blueprint) =
        blueprint.as_ref()
    {
        requirements.push(project_seed(
            format!("project-{project_id}-purpose"),
            "User product purpose",
            blueprint.product_definition.clone(),
            RequirementSource::User {
                reference: format!("project://{project_id}/idea"),
            },
            RequirementRisk::High,
            "decision",
        ));
        requirements.push(project_seed(
            format!("project-{project_id}-outcome"),
            "User problem outcome",
            blueprint.problem_outcome.clone(),
            RequirementSource::User {
                reference: format!("project://{project_id}/problem-outcome"),
            },
            RequirementRisk::High,
            "decision",
        ));
        for candidate in &blueprint.candidate_requirements {
            requirements.push(project_seed(
                format!("project-blueprint-{}", candidate.id),
                candidate.statement.clone(),
                candidate.rationale.clone(),
                RequirementSource::Blueprint {
                    reference: format!("blueprint://{}/candidate/{}", blueprint.id, candidate.id),
                },
                RequirementRisk::High,
                "functional",
            ));
        }
        for nfr in &blueprint.non_functional_requirements {
            requirements.push(project_seed(
                format!("project-nfr-{}", nfr.id),
                format!("NFR: {}", nfr.domain),
                nfr.statement.clone(),
                RequirementSource::Blueprint {
                    reference: format!("blueprint://{}/nfr/{}", blueprint.id, nfr.id),
                },
                RequirementRisk::High,
                &nfr.domain,
            ));
        }
        (
            blueprint
                .architecture_decisions
                .iter()
                .map(|decision| {
                    format!(
                        "ADR {}: {} => {}; rationale: {}",
                        decision.id,
                        decision.decision_question,
                        decision.selected_option.as_deref().unwrap_or("unselected"),
                        decision.reason
                    )
                })
                .collect::<Vec<_>>(),
            blueprint
                .assumptions
                .iter()
                .map(|assumption| assumption.statement.clone())
                .collect::<Vec<_>>(),
            blueprint
                .risks
                .iter()
                .map(|risk| format!("{}: {}", risk.title, risk.description))
                .collect::<Vec<_>>(),
        )
    } else {
        requirements.push(project_seed(
            format!("project-{project_id}-takeover-purpose"),
            "Existing project takeover scope",
            "The imported repository is the project scope for this Relintor mission.",
            RequirementSource::User {
                reference: format!("project://{project_id}/takeover"),
            },
            RequirementRisk::High,
            "decision",
        ));
        (
            vec!["Takeover-only project identity created by the Rust authority.".into()],
            Vec::new(),
            Vec::new(),
        )
    };

    let mut facts = BTreeMap::new();
    // `platform` means that a target project exists for the general
    // application-security pack. It is deliberately not the Relintor host
    // platform, and target-project desktop applicability remains unresolved
    // until the review UI supplies a decision or evidence derives it.
    facts.insert("platform".into(), FactValue::Text("target-project".into()));
    let blueprint_text = format!(
        "{} {} {}",
        idea,
        blueprint
            .as_ref()
            .map(|value| value.product_definition.as_str())
            .unwrap_or_default(),
        blueprint
            .as_ref()
            .map(|value| value.problem_outcome.as_str())
            .unwrap_or_default()
    )
    .to_ascii_lowercase();
    for (field, keywords) in [
        ("web", ["web", "browser", "frontend"].as_slice()),
        ("backend", ["backend", "api", "server"].as_slice()),
        ("database", ["database", "sql", "migration"].as_slice()),
        (
            "authentication",
            ["auth", "login", "identity", "permission"].as_slice(),
        ),
        (
            "payments",
            ["payment", "billing", "subscription"].as_slice(),
        ),
        ("ai", ["ai", "machine learning", "model"].as_slice()),
    ] {
        if keywords
            .iter()
            .any(|keyword| blueprint_text.contains(keyword))
        {
            facts.insert(field.into(), FactValue::Bool(true));
        }
    }

    let mut takeover_fingerprint = String::from("none");
    if let Some((takeover_id, fingerprint)) = takeover {
        takeover_fingerprint = fingerprint.clone();
        let mut findings = connection
            .prepare(
                "SELECT id, finding_type, summary, severity FROM takeover_findings
                 WHERE takeover_id = ?1 ORDER BY id",
            )
            .map_err(|error| format!("prepare takeover authority query: {error}"))?;
        let rows = findings
            .query_map(params![takeover_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            })
            .map_err(|error| format!("read takeover authority findings: {error}"))?;
        for row in rows {
            let (finding_id, finding_type, summary, severity) =
                row.map_err(|error| format!("decode takeover authority finding: {error}"))?;
            let risk = if severity.eq_ignore_ascii_case("critical") {
                RequirementRisk::Critical
            } else if severity.eq_ignore_ascii_case("high") {
                RequirementRisk::High
            } else {
                RequirementRisk::Medium
            };
            requirements.push(project_seed(
                format!("project-takeover-{finding_id}"),
                format!("Takeover finding: {finding_type}"),
                summary,
                RequirementSource::TakeoverFinding {
                    reference: format!("takeover://{finding_id}"),
                },
                risk,
                &finding_type,
            ));
        }
        architecture_decisions.push(format!(
            "P5 takeover authority imported from takeover://{takeover_id}"
        ));
    }
    assumptions.push(format!(
        "Persisted P4 blueprint revision {blueprint_revision} was selected when available."
    ));
    risks.push("P6 seal is readiness authority only; P7 execution is not started.".into());
    let source_fingerprint = sha256_hex(
        format!("{project_id}:{blueprint_fingerprint}:{takeover_fingerprint}").as_bytes(),
    );
    let source_revision = format!(
        "blueprint-{blueprint_revision}-{}",
        &blueprint_fingerprint[..blueprint_fingerprint.len().min(16)]
    );
    let context = ApplicabilityContext {
        revision: source_revision.clone(),
        facts,
    };
    Ok((
        ProjectAuthorityInput {
            requirements,
            architecture_decisions,
            assumptions,
            risks,
            decisions: Vec::new(),
            source_revision,
            source_fingerprint: source_fingerprint.clone(),
        },
        context,
        source_fingerprint,
        takeover_fingerprint,
    ))
}

fn execution_now_ms() -> u64 {
    current_timestamp().parse::<u64>().unwrap_or(0)
}

static EXECUTION_MUTATION_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn with_execution_mutation_lock<T>(
    operation: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    let _guard = EXECUTION_MUTATION_LOCK
        .lock()
        .map_err(|_| "execution authority lock is poisoned".to_string())?;
    operation()
}

fn execution_ledger_path(
    app: &AppHandle,
    mission_id: &str,
    revision: u64,
) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|error| format!("resolve execution ledger directory: {error}"))?
        .join("execution")
        .join(format!("{mission_id}-{revision}.json")))
}

fn execution_handoff(revision: &MissionRevision) -> Result<ExecutionHandoff, String> {
    let executable = revision
        .contract
        .task_graph
        .tasks
        .iter()
        .filter(|task| {
            !matches!(
                task.status,
                RequirementStatus::DeferredByExplicitDecision | RequirementStatus::NotApplicable
            )
        })
        .map(|task| task.task_id.as_str())
        .collect::<BTreeSet<_>>();
    let task_order = revision
        .contract
        .task_graph
        .topological_order()
        .map_err(|error| format!("rebuild execution handoff: {error}"))?
        .into_iter()
        .filter(|task_id| executable.contains(task_id.as_str()))
        .collect();
    Ok(ExecutionHandoff {
        mission_id: revision.seal.mission_id.clone(),
        revision: revision.revision,
        contract_hash: revision.seal.contract_hash.clone(),
        state: "READY_FOR_EXECUTION".into(),
        task_order,
        scheduler_owner: "P7_RUST_SCHEDULER".into(),
    })
}

fn latest_execution_context(
    path: &Path,
    project_id: &str,
) -> Result<
    (
        MissionRevision,
        ExecutionHandoff,
        StandardsRegistry,
        TrustedSignerSet,
        PathBuf,
        String,
    ),
    String,
> {
    let (root, revision_number, fingerprint): (String, u64, Option<String>) = {
        let connection =
            Connection::open(path).map_err(|error| format!("open execution database: {error}"))?;
        let (root, revision): (String, u64) = connection
            .query_row(
                "SELECT root_path, COALESCE((SELECT MAX(revision) FROM mission_revisions WHERE mission_id = ?1), 0) FROM projects WHERE id = ?2",
                params![format!("mission-{project_id}"), project_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|error| format!("load project execution scope: {error}"))?;
        let fingerprint = connection
            .query_row(
                "SELECT fingerprint FROM project_takeovers WHERE project_id = ?1 ORDER BY created_at DESC LIMIT 1",
                params![project_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| format!("load workspace fingerprint: {error}"))?;
        (root, revision, fingerprint)
    };
    if revision_number == 0 {
        return Err("P7 execution requires a sealed mission revision".into());
    }
    let mission_id = format!("mission-{project_id}");
    let revision = relintor_standards::load_mission_revision(path, &mission_id, revision_number)
        .map_err(|error| format!("load sealed mission revision: {error}"))?;
    let handoff = execution_handoff(&revision)?;
    let (registry, trusted) = production_registry().map_err(|error| error.to_string())?;
    let workspace_fingerprint = fingerprint.unwrap_or_else(|| sha256_hex(root.as_bytes()));
    Ok((
        revision,
        handoff,
        registry,
        trusted,
        PathBuf::from(root),
        workspace_fingerprint,
    ))
}

fn execution_status_view_base(
    project_id: &str,
    ledger_path: &Path,
    run: &ExecutionRun,
) -> ExecutionStatusView {
    let active_task = run
        .attempts
        .iter()
        .rev()
        .find(|attempt| attempt.state == relintor_execution::TaskAttemptState::Running)
        .map(|attempt| attempt.task_id.clone());
    ExecutionStatusView {
        project_id: project_id.into(),
        mission_id: run.mission_id.clone(),
        revision: run.mission_revision,
        state: format!("{:?}", run.state),
        watchdog_state: format!("{:?}", run.watchdog_state),
        current_turn: run.current_turn,
        active_task,
        runnable_tasks: run.runnable_tasks(),
        total_tasks: run.tasks.len(),
        finished_tasks: run
            .tasks
            .values()
            .filter(|task| {
                task.state == relintor_execution::ExecutionTaskState::FinishedAwaitingVerification
            })
            .count(),
        tool_calls: run.usage.tool_calls,
        execution_steps: run.usage.execution_steps,
        estimated_cost_micros: run.usage.estimated_cost_micros,
        safe_boundary_reached: run
            .safe_boundary
            .as_ref()
            .is_some_and(|boundary| boundary.reached),
        last_event: run.events.last().map(|event| event.detail.clone()),
        events: run
            .events
            .iter()
            .map(|event| ExecutionEventView {
                sequence: event.sequence,
                occurred_at_ms: event.occurred_at_ms,
                task_id: event.task_id.clone(),
                kind: format!("{:?}", event.kind),
                detail: event.detail.clone(),
            })
            .collect(),
        ledger_path: ledger_path.display().to_string(),
        recovery_state: "P9_RECOVERY_NOT_EVALUATED".into(),
        last_safe_checkpoint: None,
        resume_disposition: None,
        resume_blocker: None,
        external_changes: Vec::new(),
        recovery_detected: false,
    }
}

fn recovery_store(app: &AppHandle, revision: &MissionRevision) -> Result<RecoveryStore, String> {
    let app_data = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("resolve P9 recovery directory: {error}"))?;
    let key = load_or_create_keychain_authority_key(
        "Relintor.P9.Recovery",
        &format!("{}-{}", revision.seal.project_id, revision.revision),
    )
    .map_err(|error| format!("load P9 OS keychain authority: {error}"))?;
    RecoveryStore::new(
        app_data.join("execution").join("recovery").join(format!(
            "{}-{}",
            revision.seal.mission_id, revision.revision
        )),
        key,
    )
    .map_err(|error| format!("open P9 recovery store: {error}"))
}

fn recovery_authority(run: &ExecutionRun, revision: &MissionRevision) -> RecoveryAuthority {
    RecoveryAuthority::new(
        run.project_id.clone(),
        run.mission_id.clone(),
        run.mission_revision,
        run.seal_hash.clone(),
        revision.contract.registry_id.clone(),
        run.run_id.clone(),
        run.workspace_fingerprint.clone(),
        format!(
            "revision-{}-workspace-input-{}",
            revision.revision, run.workspace_fingerprint
        ),
        None,
        "P8_EVIDENCE_REQUIRES_CURRENT_FRESHNESS_CHECK",
        execution_now_ms(),
    )
}

fn expected_recovery_authority(
    store: &RecoveryStore,
    run: &ExecutionRun,
    revision: &MissionRevision,
) -> Result<RecoveryAuthority, String> {
    let mut authority = recovery_authority(run, revision);
    if let Some(record) = store
        .load_latest()
        .map_err(|error| format!("load P9 authority checkpoint: {error}"))?
    {
        authority.workspace_identity = record.content.authority.workspace_identity;
    }
    Ok(authority)
}

fn p9_status_view(
    app: &AppHandle,
    project_id: &str,
    ledger_path: &Path,
    run: &ExecutionRun,
    revision: &MissionRevision,
) -> Result<ExecutionStatusView, String> {
    let mut view = execution_status_view_base(project_id, ledger_path, run);
    let store = recovery_store(app, revision)?;
    let coordinator = RecoveryCoordinator::new(store.clone());
    let latest = store
        .load_latest()
        .map_err(|error| format!("load P9 checkpoint: {error}"))?;
    if let Some(record) = latest {
        if record.is_safe_to_resume() {
            view.last_safe_checkpoint =
                Some(format!("{}#{}", record.checkpoint_id, record.sequence));
        }
        let expected = expected_recovery_authority(&store, run, revision)?;
        let result = coordinator
            .resume_integrity(
                &expected,
                &run.workspace,
                &ConservativeProcessInspector,
                execution_now_ms(),
            )
            .map_err(|error| format!("evaluate P9 resume integrity: {error}"))?;
        view.recovery_state = format!("{:?}", result.disposition);
        view.resume_disposition = Some(format!("{:?}", result.disposition));
        view.resume_blocker = result.reasons.first().cloned();
        view.external_changes = result.changed_paths;
        view.recovery_detected = !result.reasons.is_empty();
    } else {
        view.recovery_state = "P9_CHECKPOINT_NOT_YET_CREATED".into();
    }
    Ok(view)
}

fn load_execution_run(
    app: &AppHandle,
    project_id: &str,
) -> Result<(ExecutionRun, PathBuf, MissionRevision, ExecutionHandoff), String> {
    let path = database_path(app)?;
    migrate_database(&path)?;
    let (revision, handoff, registry, trusted, workspace, fingerprint) =
        latest_execution_context(&path, project_id)?;
    let ledger_path = execution_ledger_path(app, &revision.seal.mission_id, revision.revision)?;
    let recovery = recovery_store(app, &revision)?;
    let ledger_run = if ledger_path.is_file() {
        ExecutionRun::restore_snapshot(&ledger_path).ok()
    } else {
        None
    };
    let recovery_run = recovery
        .load_run()
        .map_err(|error| format!("restore P9 recovery checkpoint: {error}"))?;
    let run = match (ledger_run, recovery_run) {
        (Some(ledger), Some(recovery)) => {
            if ledger.run_id != recovery.run_id {
                return Err("P7 ledger and P9 checkpoint belong to different runs".into());
            }
            let recovery_is_newer = recovery.events.len() > ledger.events.len()
                || matches!(
                    recovery.state,
                    relintor_execution::ExecutionRunState::StoppedIncomplete
                        | relintor_execution::ExecutionRunState::RevalidationRequired
                ) && !matches!(
                    ledger.state,
                    relintor_execution::ExecutionRunState::StoppedIncomplete
                        | relintor_execution::ExecutionRunState::RevalidationRequired
                );
            if recovery_is_newer {
                recovery
            } else {
                ledger
            }
        }
        (Some(ledger), None) => ledger,
        (None, Some(recovery)) => recovery,
        (None, None) => ExecutionRun::from_p6_handoff_with_registry(
            &revision,
            &handoff,
            &registry,
            &trusted,
            workspace,
            &fingerprint,
            SchedulerPolicy::default(),
            execution_now_ms(),
        )
        .map_err(|error| error.to_string())?,
    };
    Ok((run, ledger_path, revision, handoff))
}

struct P8VerificationContext {
    authority: VerificationAuthority,
    current: FreshnessContext,
    store: EvidenceStore,
    p7_execution: AuthenticatedP7Execution,
    local_key: Vec<u8>,
}

fn load_p8_verification_context(
    app: &AppHandle,
    project_id: &str,
) -> Result<P8VerificationContext, String> {
    let path = database_path(app)?;
    migrate_database(&path)?;
    let (revision, handoff, registry, trusted, workspace, workspace_fingerprint) =
        latest_execution_context(&path, project_id)?;
    let ledger_path = execution_ledger_path(app, &revision.seal.mission_id, revision.revision)?;
    if !ledger_path.is_file() {
        return Err(
            "P7 execution ledger is not available; verification remains pending until execution finishes"
                .into(),
        );
    }
    let p7_execution = AuthenticatedP7Execution::from_snapshot(&ledger_path)
        .map_err(|error| format!("authenticate P7 execution ledger: {error}"))?;
    let p7_run_id = p7_execution.run.run_id.clone();
    let p7_state = "EXECUTION_TASKS_FINISHED_AWAITING_VERIFICATION".into();
    let source_revision = Some(revision.contract.project_source_revision.clone());
    let environment = environment_fingerprint(
        &workspace,
        &revision.seal.mission_id,
        revision.revision,
        &revision.seal.contract_hash,
        &registry,
        source_revision.clone(),
        BTreeMap::new(),
        BTreeMap::new(),
    )
    .map_err(|error| format!("fingerprint verification environment: {error}"))?;
    let environment_fingerprint = environment
        .digest()
        .map_err(|error| format!("digest verification environment: {error}"))?;
    let authority = VerificationAuthority {
        revision,
        handoff,
        registry,
        trusted_signers: trusted,
        p7_run_id,
        p7_state,
        workspace_fingerprint: workspace_fingerprint.clone(),
        source_revision: source_revision.clone(),
        environment_fingerprint: environment_fingerprint.clone(),
    };
    p7_execution
        .validate_against(&authority)
        .map_err(|error| format!("validate P7 execution authority: {error}"))?;
    let app_data = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("resolve P8 authority directory: {error}"))?;
    let local_key = load_or_create_keychain_authority_key(
        "Relintor.P8.Authority",
        &format!(
            "{}-{}",
            authority.revision.seal.project_id, authority.revision.revision
        ),
    )
    .map_err(|error| format!("load P8 OS keychain authority: {error}"))?;
    let store = EvidenceStore::new(
        app_data.join("verification").join(format!(
            "{}-{}",
            authority.revision.seal.mission_id, authority.revision.revision
        )),
        &local_key,
    )
    .map_err(|error| format!("open P8 evidence store: {error}"))?;
    let current_workspace_fingerprint = fingerprint_workspace(&workspace)
        .map_err(|error| format!("fingerprint current verification workspace: {error}"))?;
    let current = FreshnessContext {
        mission_id: authority.revision.seal.mission_id.clone(),
        mission_revision: authority.revision.revision,
        p6_seal_hash: authority.revision.seal.contract_hash.clone(),
        workspace_fingerprint: current_workspace_fingerprint,
        source_revision,
        environment_fingerprint: authority.environment_fingerprint.clone(),
        dependency_lock_hashes: BTreeMap::new(),
        workspace_root: Some(workspace),
    };
    Ok(P8VerificationContext {
        authority,
        current,
        store,
        p7_execution,
        local_key,
    })
}

fn evaluate_p8(
    app: &AppHandle,
    project_id: &str,
) -> Result<
    (
        P8VerificationContext,
        relintor_evidence::VerificationReport,
        EvidenceManifest,
    ),
    String,
> {
    let context = load_p8_verification_context(app, project_id)?;
    let workspace = context
        .current
        .workspace_root
        .clone()
        .ok_or_else(|| "verification workspace is unavailable".to_string())?;
    VerificationCollectorOrchestrator::new(workspace)
        .run_required_collectors(&context.authority, &context.current, &context.store)
        .map_err(|error| format!("collect P8 verification evidence: {error}"))?;
    let engine = VerificationEngine::new_with_p7_execution(
        context.store.clone(),
        context.authority.clone(),
        context.current.clone(),
        Vec::new(),
        context.p7_execution.clone(),
    )
    .map_err(|error| format!("start P8 verification: {error}"))?;
    let initial_report = engine
        .evaluate(None)
        .map_err(|error| format!("evaluate P8 requirements: {error}"))?;
    let evidence = context
        .store
        .list()
        .map_err(|error| format!("load P8 evidence for AI review: {error}"))?
        .into_iter()
        .map(|artifact| EvidenceSummary {
            evidence_id: artifact.metadata.evidence_id,
            class: artifact.metadata.class,
            result: artifact.metadata.result,
            confidence: artifact.metadata.confidence,
            digest: artifact.digest,
        })
        .collect::<Vec<_>>();
    let (gateway_endpoint, session_access_token) = match DesktopPublicConfig::from_env() {
        Ok(config) => (config.ai_gateway_endpoint, cloud_access_token()),
        Err(_) => (String::new(), None),
    };
    let provider = ProductionAiProvider::new(gateway_endpoint, session_access_token);
    let mut authenticated_ai_judgements = Vec::new();
    if provider.configured() {
        for requirement in context
            .authority
            .revision
            .contract
            .requirement_graph
            .requirements
            .iter()
            .filter(|item| {
                item.status == RequirementStatus::Unstarted
                    || item.status == RequirementStatus::ImplementedUnverified
            })
        {
            let input = AiVerifierInput {
                requirement_id: requirement.requirement_id.clone(),
                acceptance_criteria: requirement
                    .acceptance_criteria
                    .iter()
                    .map(|criterion| criterion.criterion_id.clone())
                    .collect(),
                evidence: evidence
                    .iter()
                    .filter(|item| {
                        context.store.list().ok().is_some_and(|artifacts| {
                            artifacts.iter().any(|artifact| {
                                artifact.metadata.evidence_id == item.evidence_id
                                    && artifact
                                        .metadata
                                        .requirement_ids
                                        .contains(&requirement.requirement_id)
                            })
                        })
                    })
                    .cloned()
                    .collect(),
                deterministic_gate_results: initial_report.decision.deterministic_gates.clone(),
                explicit_decisions: Vec::new(),
            };
            if let Ok(provider_result) = provider.judge_with_metadata(input.clone()) {
                if let Ok(authenticated) = engine.authenticate_ai_judgement(&input, provider_result)
                {
                    authenticated_ai_judgements.push(authenticated);
                }
            }
        }
    }
    let report = engine
        .with_ai_judgements_authenticated(authenticated_ai_judgements)
        .map_err(|error| format!("bind P8 AI verification judgement: {error}"))?
        .evaluate(None)
        .map_err(|error| format!("evaluate P8 requirements with AI judgement: {error}"))?;
    let manifest = export_manifest(
        &report,
        &context.authority,
        &context.store,
        Vec::new(),
        None,
    )
    .map_err(|error| format!("export P8 evidence manifest: {error}"))?;
    Ok((context, report, manifest))
}

fn verification_view(
    project_id: &str,
    context: &P8VerificationContext,
    report: &relintor_evidence::VerificationReport,
    manifest: &EvidenceManifest,
    certificate: Option<&CompletionCertificate>,
) -> VerificationStatusView {
    let missing_evidence = report
        .requirement_statuses
        .iter()
        .flat_map(|status| {
            status
                .missing_obligations
                .iter()
                .map(move |class| format!("{}: missing {:?}", status.requirement_id, class))
                .chain(status.missing_acceptance_criteria.iter().map(|criterion| {
                    format!("{}: missing criterion {criterion}", status.requirement_id)
                }))
        })
        .collect::<Vec<_>>();
    VerificationStatusView {
        project_id: project_id.into(),
        mission_id: context.authority.revision.seal.mission_id.clone(),
        revision: context.authority.revision.revision,
        execution_run_id: context.p7_execution.run.run_id.clone(),
        state: "VERIFICATION_FINISHED".into(),
        completion_state: format!("{:?}", report.decision.state),
        requirements_verified: report
            .requirement_statuses
            .iter()
            .filter(|status| status.status == RequirementStatus::Verified)
            .count(),
        requirements_total: report.coverage_total,
        missing_evidence,
        failed_checks: report
            .requirement_statuses
            .iter()
            .flat_map(|status| status.failed_evidence.iter().cloned())
            .collect(),
        skipped_checks: report
            .decision
            .deterministic_gates
            .iter()
            .filter(|gate| gate.detail.contains("skipped"))
            .filter_map(|gate| gate.evidence_id.clone())
            .collect(),
        stale_evidence: report
            .requirement_statuses
            .iter()
            .flat_map(|status| status.stale_evidence.iter().cloned())
            .collect(),
        blocked_external: report
            .decision
            .blocked_external
            .iter()
            .map(|item| format!("{}: {}", item.requirement_id, item.reason))
            .collect(),
        accepted_risks: report
            .decision
            .accepted_risks
            .iter()
            .map(|risk| format!("{}: {}", risk.requirement_id, risk.reason))
            .collect(),
        evidence_count: manifest.evidence.len(),
        certificate: certificate.map(|item| CompletionCertificateView {
            certificate_id: item.certificate_id.clone(),
            final_state: format!("{:?}", item.final_state),
            digest: item.digest().unwrap_or_default(),
        }),
        detail: report.decision.reason.clone(),
    }
}

#[tauri::command]
fn verification_start(
    app: AppHandle,
    project_id: String,
) -> Result<VerificationStatusView, String> {
    let (context, report, manifest) = evaluate_p8(&app, &project_id)?;
    Ok(verification_view(
        &project_id,
        &context,
        &report,
        &manifest,
        None,
    ))
}

#[tauri::command]
fn verification_status(
    app: AppHandle,
    project_id: String,
) -> Result<VerificationStatusView, String> {
    verification_start(app, project_id)
}

#[tauri::command]
fn verification_rerun(
    app: AppHandle,
    project_id: String,
) -> Result<VerificationStatusView, String> {
    verification_start(app, project_id)
}

#[tauri::command]
fn verification_evidence(
    app: AppHandle,
    project_id: String,
) -> Result<Vec<VerificationEvidenceView>, String> {
    let (_, _, manifest) = evaluate_p8(&app, &project_id)?;
    Ok(manifest
        .evidence
        .into_iter()
        .map(|item| VerificationEvidenceView {
            evidence_id: item.evidence_id,
            class: format!("{:?}", item.class),
            result: format!("{:?}", item.result),
            confidence: format!("{:?}", item.confidence),
            digest: item.digest,
        })
        .collect())
}

#[tauri::command]
fn verification_certificate(
    app: AppHandle,
    project_id: String,
) -> Result<CompletionCertificateView, String> {
    let (context, report, manifest) = evaluate_p8(&app, &project_id)?;
    let authority = CompletionAuthority::new(&context.local_key)
        .map_err(|error| format!("open P8 completion authority: {error}"))?;
    let certificate = authority
        .issue_with_p7_execution(&report, &context.authority, &context.p7_execution)
        .map_err(|error| format!("issue completion certificate: {error}"))?;
    authority
        .validate_with_store(
            &certificate,
            &context.authority,
            &context.store,
            &manifest,
            &context.current,
        )
        .map_err(|error| format!("validate completion certificate: {error}"))?;
    Ok(CompletionCertificateView {
        certificate_id: certificate.certificate_id.clone(),
        final_state: format!("{:?}", certificate.final_state),
        digest: certificate
            .digest()
            .map_err(|error| format!("digest completion certificate: {error}"))?,
    })
}

#[tauri::command]
fn verification_export_manifest(app: AppHandle, project_id: String) -> Result<String, String> {
    let (_, _, manifest) = evaluate_p8(&app, &project_id)?;
    String::from_utf8(manifest.export_json().map_err(|error| error.to_string())?)
        .map_err(|error| format!("manifest is not UTF-8: {error}"))
}

#[tauri::command]
fn execution_status(app: AppHandle, project_id: String) -> Result<ExecutionStatusView, String> {
    let (run, ledger_path, revision, _) = load_execution_run(&app, &project_id)?;
    p9_status_view(&app, &project_id, &ledger_path, &run, &revision)
}

#[tauri::command]
fn execution_start(app: AppHandle, project_id: String) -> Result<ExecutionStatusView, String> {
    with_execution_mutation_lock(|| execution_start_inner(app, project_id))
}

fn execution_start_inner(
    app: AppHandle,
    project_id: String,
) -> Result<ExecutionStatusView, String> {
    let (mut run, ledger_path, revision, handoff) = load_execution_run(&app, &project_id)?;
    let now = execution_now_ms();
    run.validate_authority_identity(&revision, &handoff, now)
        .map_err(|error| error.to_string())?;
    let recovery = recovery_store(&app, &revision)?;
    let crash = recovery
        .begin_session(
            &run.run_id,
            &sha256_hex(
                &std::fs::read(std::env::current_exe().map_err(|error| error.to_string())?)
                    .map_err(|error| error.to_string())?,
            ),
            &sha256_hex(b"relintor-desktop-p9-session"),
            now,
        )
        .map_err(|error| format!("begin P9 recovery session: {error}"))?;
    let coordinator = RecoveryCoordinator::new(recovery.clone());
    let checkpoint_kind = if crash.is_some() {
        let expected = expected_recovery_authority(&recovery, &run, &revision)?;
        let workspace = run.workspace.clone();
        let integrity = coordinator
            .resume_run(
                &mut run,
                &expected,
                &workspace,
                &ConservativeProcessInspector,
                now,
                false,
            )
            .map_err(|error| format!("evaluate crash recovery integrity: {error}"))?;
        if !matches!(
            integrity.disposition,
            RecoveryDisposition::SafeToResume
                | RecoveryDisposition::SafeToResumeAfterProcessReconciliation
                | RecoveryDisposition::StoppedIncomplete
        ) {
            run.state = relintor_execution::ExecutionRunState::RevalidationRequired;
            run.last_error = integrity.reasons.first().cloned();
            coordinator
                .begin_revalidation(&expected, &integrity, now)
                .map_err(|error| format!("persist crash revalidation: {error}"))?;
        }
        CheckpointKind::RestartRecovery
    } else {
        CheckpointKind::AfterTaskPersistence
    };
    run.persist_snapshot(&ledger_path)
        .map_err(|error| error.to_string())?;
    coordinator
        .checkpoint_run(
            &run,
            recovery_authority(&run, &revision),
            checkpoint_kind,
            &run.workspace,
            Vec::new(),
            Vec::new(),
            "execution session started and durable state persisted",
            now,
        )
        .map_err(|error| format!("write P9 start checkpoint: {error}"))?;
    p9_status_view(&app, &project_id, &ledger_path, &run, &revision)
}

#[tauri::command]
fn execution_step(app: AppHandle, project_id: String) -> Result<ExecutionStatusView, String> {
    with_execution_mutation_lock(|| execution_step_inner(app, project_id))
}

fn execution_step_inner(app: AppHandle, project_id: String) -> Result<ExecutionStatusView, String> {
    let (mut run, ledger_path, revision, handoff) = load_execution_run(&app, &project_id)?;
    let now = execution_now_ms();
    let recovery = recovery_store(&app, &revision)?;
    run.dispatch_next_with_production_adapter(&revision, &handoff, now)
        .map_err(|error| error.to_string())?;
    run.persist_snapshot(&ledger_path)
        .map_err(|error| error.to_string())?;
    RecoveryCoordinator::new(recovery)
        .checkpoint_run(
            &run,
            recovery_authority(&run, &revision),
            CheckpointKind::AfterAtomicAction,
            &run.workspace,
            Vec::new(),
            Vec::new(),
            "task dispatch returned at a durable boundary",
            now,
        )
        .map_err(|error| format!("write P9 action checkpoint: {error}"))?;
    p9_status_view(&app, &project_id, &ledger_path, &run, &revision)
}

#[tauri::command]
fn execution_pause(app: AppHandle, project_id: String) -> Result<ExecutionStatusView, String> {
    with_execution_mutation_lock(|| execution_pause_inner(app, project_id))
}

fn execution_pause_inner(
    app: AppHandle,
    project_id: String,
) -> Result<ExecutionStatusView, String> {
    let (mut run, ledger_path, revision, handoff) = load_execution_run(&app, &project_id)?;
    let now = execution_now_ms();
    run.validate_authority_identity(&revision, &handoff, now)
        .map_err(|error| error.to_string())?;
    run.request_stop(None, "user requested pause at a safe boundary", now)
        .map_err(|error| error.to_string())?;
    run.persist_snapshot(&ledger_path)
        .map_err(|error| error.to_string())?;
    let recovery = recovery_store(&app, &revision)?;
    RecoveryCoordinator::new(recovery)
        .checkpoint_run(
            &run,
            recovery_authority(&run, &revision),
            CheckpointKind::AfterTaskPersistence,
            &run.workspace,
            Vec::new(),
            Vec::new(),
            "user pause requested at a safe boundary",
            now,
        )
        .map_err(|error| format!("write P9 pause checkpoint: {error}"))?;
    p9_status_view(&app, &project_id, &ledger_path, &run, &revision)
}

#[tauri::command]
fn execution_stop(app: AppHandle, project_id: String) -> Result<ExecutionStatusView, String> {
    with_execution_mutation_lock(|| execution_stop_inner(app, project_id))
}

fn execution_stop_inner(app: AppHandle, project_id: String) -> Result<ExecutionStatusView, String> {
    let (mut run, ledger_path, revision, handoff) = load_execution_run(&app, &project_id)?;
    let now = execution_now_ms();
    run.validate_authority_identity(&revision, &handoff, now)
        .map_err(|error| error.to_string())?;
    let recovery = RecoveryCoordinator::new(recovery_store(&app, &revision)?);
    let workspace = run.workspace.clone();
    let authority = recovery_authority(&run, &revision);
    recovery
        .emergency_stop(
            &mut run,
            authority,
            &workspace,
            "user requested emergency stop",
            now,
        )
        .map_err(|error| error.to_string())?;
    run.persist_snapshot(&ledger_path)
        .map_err(|error| error.to_string())?;
    recovery
        .store
        .end_session(SessionEndState::EmergencyStop, now)
        .map_err(|error| error.to_string())?;
    p9_status_view(&app, &project_id, &ledger_path, &run, &revision)
}

#[tauri::command]
fn execution_continue(app: AppHandle, project_id: String) -> Result<ExecutionStatusView, String> {
    with_execution_mutation_lock(|| execution_continue_inner(app, project_id))
}

fn execution_continue_inner(
    app: AppHandle,
    project_id: String,
) -> Result<ExecutionStatusView, String> {
    let (mut run, ledger_path, revision, handoff) = load_execution_run(&app, &project_id)?;
    let now = execution_now_ms();
    run.validate_authority_identity(&revision, &handoff, now)
        .map_err(|error| error.to_string())?;
    let recovery = RecoveryCoordinator::new(recovery_store(&app, &revision)?);
    let expected = expected_recovery_authority(&recovery.store, &run, &revision)?;
    let workspace = run.workspace.clone();
    let continue_turn = run.state == relintor_execution::ExecutionRunState::TurnEndedIncomplete;
    let integrity = recovery
        .resume_run(
            &mut run,
            &expected,
            &workspace,
            &ConservativeProcessInspector,
            now,
            continue_turn,
        )
        .map_err(|error| error.to_string())?;
    if !matches!(
        integrity.disposition,
        RecoveryDisposition::SafeToResume
            | RecoveryDisposition::SafeToResumeAfterProcessReconciliation
            | RecoveryDisposition::StoppedIncomplete
    ) {
        run.state = relintor_execution::ExecutionRunState::RevalidationRequired;
        run.last_error = integrity.reasons.first().cloned();
        recovery
            .begin_revalidation(&expected, &integrity, now)
            .map_err(|error| error.to_string())?;
    }
    run.persist_snapshot(&ledger_path)
        .map_err(|error| error.to_string())?;
    recovery
        .checkpoint_run(
            &run,
            recovery_authority(&run, &revision),
            CheckpointKind::RestartRecovery,
            &run.workspace,
            Vec::new(),
            Vec::new(),
            "resume integrity evaluated before continuation",
            now,
        )
        .map_err(|error| error.to_string())?;
    p9_status_view(&app, &project_id, &ledger_path, &run, &revision)
}

#[tauri::command]
fn execution_revalidate(app: AppHandle, project_id: String) -> Result<ExecutionStatusView, String> {
    with_execution_mutation_lock(|| execution_revalidate_inner(app, project_id))
}

fn execution_revalidate_inner(
    app: AppHandle,
    project_id: String,
) -> Result<ExecutionStatusView, String> {
    let (mut run, ledger_path, revision, handoff) = load_execution_run(&app, &project_id)?;
    let now = execution_now_ms();
    run.validate_authority_identity(&revision, &handoff, now)
        .map_err(|error| error.to_string())?;
    let recovery = RecoveryCoordinator::new(recovery_store(&app, &revision)?);
    let expected = expected_recovery_authority(&recovery.store, &run, &revision)?;
    let workspace = run.workspace.clone();
    let integrity = recovery
        .resume_integrity(&expected, &workspace, &ConservativeProcessInspector, now)
        .map_err(|error| error.to_string())?;
    if matches!(
        integrity.disposition,
        RecoveryDisposition::RevalidationRequired | RecoveryDisposition::BlockedExternal
    ) {
        recovery
            .begin_revalidation(&expected, &integrity, now)
            .map_err(|error| error.to_string())?;
        run.state = relintor_execution::ExecutionRunState::RevalidationRequired;
        run.last_error = integrity.reasons.first().cloned();
        run.persist_snapshot(&ledger_path)
            .map_err(|error| error.to_string())?;
    }
    p9_status_view(&app, &project_id, &ledger_path, &run, &revision)
}

/// Rust-owned Seal & Build authority action. The renderer supplies only the
/// persisted project identity; all authority inputs, trust verification,
/// preseal checks, hashing, sealing, and persistence remain in Rust.
#[tauri::command]
fn seal_project_mission(
    app: AppHandle,
    project_id: String,
    expected_review_digest: String,
) -> Result<ExecutionHandoff, String> {
    let path = database_path(&app)?;
    migrate_database(&path)?;
    seal_project_mission_at_path(&path, &project_id, &expected_review_digest)
}

fn seal_project_mission_at_path(
    path: &Path,
    project_id: &str,
    expected_review_digest: &str,
) -> Result<ExecutionHandoff, String> {
    let (registry, trusted) = production_registry().map_err(|error| error.to_string())?;
    let connection =
        Connection::open(path).map_err(|error| format!("open authority database: {error}"))?;
    let (review_digest, facts_json): (String, String) = connection
        .query_row(
            "SELECT review_digest, facts_json FROM authority_reviews WHERE project_id = ?1",
            params![&project_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|error| format!("load current reviewed authority: {error}"))?;
    if review_digest != expected_review_digest {
        return Err("STALE_AUTHORITY_REVIEW: displayed review is no longer current".into());
    }
    let reviewed_facts: Vec<AuthorityFactDecision> = serde_json::from_str(&facts_json)
        .map_err(|error| format!("decode current authority review: {error}"))?;
    let (project_authority, mut context, source_fingerprint, takeover_fingerprint) =
        build_project_authority(&connection, project_id)?;
    apply_reviewed_facts(&mut context, &reviewed_facts)?;
    let mission_id = format!("mission-{project_id}");
    let scope = relintor_standards::scope_fingerprint(BTreeMap::<String, String>::from([
        ("project_id".into(), project_id.to_string()),
        ("registry_digest".into(), registry.registry_digest.clone()),
        ("source_fingerprint".into(), source_fingerprint.clone()),
    ]));
    let engine = AuthorityEngine;
    let source_revision = project_authority.source_revision.clone();
    let mut draft = engine
        .build_draft_with_project_authority(
            &mission_id,
            project_id,
            &source_revision,
            &takeover_fingerprint,
            registry.clone(),
            &context,
            scope,
            project_authority,
            Vec::new(),
        )
        .map_err(|error| format!("Seal & Build preseal preparation failed: {error}"))?;
    let recomputed_review_digest = authority_review_digest(&draft)?;
    if expected_review_digest != review_digest || review_digest != recomputed_review_digest {
        return Err("STALE_AUTHORITY_REVIEW: reviewed authority changed before sealing".into());
    }
    let previous_revision: u64 = connection
        .query_row(
            "SELECT COALESCE(MAX(revision), 0) FROM mission_revisions WHERE mission_id = ?1",
            params![mission_id.as_str()],
            |row| row.get(0),
        )
        .map_err(|error| format!("read mission revision: {error}"))?;
    draft.revision = previous_revision.saturating_add(1).max(1);
    let (revision, handoff) = engine
        .seal(&draft, &trusted, &current_timestamp())
        .map_err(|error| format!("Seal & Build blocked: {error}"))?;
    relintor_standards::persist_authority(path, &registry, &revision)
        .map_err(|error| format!("persist sealed mission: {error}"))?;
    Ok(handoff)
}

fn sha256_hex(value: &[u8]) -> String {
    Sha256::digest(value)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            let handle = app.handle().clone();
            match database_path(&handle) {
                Ok(path) => {
                    if let Err(error) = migrate_database(&path) {
                        eprintln!("Relintor local database initialization failed: {error}");
                    }
                }
                Err(error) => {
                    eprintln!("Relintor application data path resolution failed: {error}");
                }
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            health_application,
            health_database,
            health_specification,
            health_keychain,
            health_antigravity,
            health_all,
            account_state,
            sign_in_with_google,
            sign_out,
            update_check,
            update_apply,
            uninstall_prepare,
            investigator_new_project,
            investigator_answer_questions,
            takeover_scan,
            authority_preview,
            seal_project_mission,
            verification_start,
            verification_status,
            verification_rerun,
            verification_evidence,
            verification_certificate,
            verification_export_manifest,
            execution_status,
            execution_start,
            execution_step,
            execution_pause,
            execution_stop,
            execution_continue,
            execution_revalidate
        ])
        .run(tauri::generate_context!())
        .expect("error while running Relintor desktop");
}

#[cfg(test)]
mod tests {
    use super::*;
    use relintor_core::migrate_database;
    use relintor_standards::{
        load_mission_revision, MissionContract, MissionDraft, RequirementSource,
    };
    use std::fs;

    fn test_path(label: &str) -> PathBuf {
        let base = std::env::var_os("CARGO_TARGET_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        fs::create_dir_all(&base).expect("target directory");
        base.join(format!(
            "relintor-p6-final-desktop-{label}-{}.sqlite",
            std::process::id()
        ))
    }

    fn seed_project(connection: &Connection, project_id: &str) {
        connection
            .execute(
                "INSERT INTO projects(id, name, root_path, created_at) VALUES (?1, ?2, ?3, 0)",
                params![
                    project_id,
                    format!("Project {project_id}"),
                    format!("D:/fixtures/{project_id}")
                ],
            )
            .expect("project identity");
    }

    fn reviewed_facts(selected: &[&str]) -> Vec<AuthorityFactDecision> {
        AUTHORITY_FACT_FIELDS
            .iter()
            .map(|id| AuthorityFactDecision {
                id: (*id).into(),
                decision: if selected.contains(id) {
                    AuthorityFactDecisionValue::Yes
                } else {
                    AuthorityFactDecisionValue::No
                },
            })
            .collect()
    }

    #[test]
    fn reviewed_mission_revision_one_then_two_preserves_revision_one() {
        let path = test_path("revision");
        migrate_database(&path).expect("migrate");
        let connection = Connection::open(&path).expect("open");
        seed_project(&connection, "revision-project");
        drop(connection);

        let desktop = reviewed_facts(&["desktop"]);
        let first_review = review_authority_at_path(&path, "revision-project", &desktop)
            .expect("first authority review");
        assert!(first_review.blockers.is_empty(), "{first_review:?}");
        let first =
            seal_project_mission_at_path(&path, "revision-project", &first_review.review_digest)
                .expect("revision 1");
        assert_eq!(first.revision, 1);

        let changed = reviewed_facts(&["desktop", "backend"]);
        let second_review = review_authority_at_path(&path, "revision-project", &changed)
            .expect("revalidated authority review");
        assert!(second_review.blockers.is_empty(), "{second_review:?}");
        let second =
            seal_project_mission_at_path(&path, "revision-project", &second_review.review_digest)
                .expect("revision 2");
        assert_eq!(second.revision, 2);

        let revision_one = load_mission_revision(&path, "mission-revision-project", 1)
            .expect("immutable revision 1");
        assert_eq!(revision_one.seal.state, "SEALED");
        assert_eq!(revision_one.revision, 1);
        let revision_two =
            load_mission_revision(&path, "mission-revision-project", 2).expect("revision 2");
        assert_eq!(revision_two.revision, 2);
        assert_ne!(
            revision_one.contract.applicability_context,
            revision_two.contract.applicability_context
        );
        let _ = fs::remove_file(path);
    }

    #[test]
    fn takeover_only_sealing_is_project_isolated() {
        let path = test_path("takeover-isolation");
        migrate_database(&path).expect("migrate");
        let connection = Connection::open(&path).expect("open");
        seed_project(&connection, "project-a");
        seed_project(&connection, "project-b");
        for (project_id, takeover_id, finding_id, summary) in [
            ("project-a", "takeover-a", "finding-a", "Project A finding"),
            ("project-b", "takeover-b", "finding-b", "Project B finding"),
        ] {
            connection
                .execute(
                    "INSERT INTO project_takeovers(id, project_id, root, fingerprint, scanner_version, created_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, 0)",
                    params![takeover_id, project_id, format!("D:/fixtures/{project_id}"), takeover_id, "p6-test"],
                )
                .expect("takeover binding");
            connection
                .execute(
                    "INSERT INTO takeover_findings(id, takeover_id, finding_type, summary, severity, classification, evidence)
                     VALUES (?1, ?2, 'remediation', ?3, 'high', 'Unproven', '{}')",
                    params![finding_id, takeover_id, summary],
                )
                .expect("takeover finding");
        }
        drop(connection);

        let facts = reviewed_facts(&["desktop"]);
        let review = review_authority_at_path(&path, "project-b", &facts).expect("takeover review");
        assert!(review.blockers.is_empty(), "{review:?}");
        let handoff = seal_project_mission_at_path(&path, "project-b", &review.review_digest)
            .expect("takeover seal");
        assert_eq!(handoff.revision, 1);
        let sealed = load_mission_revision(&path, "mission-project-b", 1).expect("sealed takeover");
        let takeover_sources = sealed
            .contract
            .requirement_graph
            .requirements
            .iter()
            .filter_map(|requirement| match &requirement.source {
                RequirementSource::TakeoverFinding { reference } => Some(reference.clone()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(takeover_sources, vec!["takeover://finding-b"]);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn imported_project_evidence_policy_matches_obligation_type() {
        let cases = [
            (
                "decision",
                EvidenceClass::HumanDecision,
                EvidenceConfidence::HumanAsserted,
            ),
            (
                "functional",
                EvidenceClass::TestOutput,
                EvidenceConfidence::StrongRuntime,
            ),
            (
                "security",
                EvidenceClass::SecurityScan,
                EvidenceConfidence::StrongDeterministic,
            ),
            (
                "performance",
                EvidenceClass::PerformanceResult,
                EvidenceConfidence::StrongRuntime,
            ),
            (
                "accessibility",
                EvidenceClass::AccessibilityResult,
                EvidenceConfidence::StrongRuntime,
            ),
            (
                "migration",
                EvidenceClass::DatabaseQuery,
                EvidenceConfidence::StrongDeterministic,
            ),
            (
                "deployment",
                EvidenceClass::DeploymentProbe,
                EvidenceConfidence::StrongRuntime,
            ),
        ];
        for (kind, expected_class, expected_confidence) in cases {
            let (policy, machine_checkable) = imported_evidence_policy(kind, kind, kind);
            assert_eq!(policy.obligations[0].class, expected_class);
            assert_eq!(
                policy.obligations[0].minimum_confidence,
                expected_confidence
            );
            assert_eq!(machine_checkable, kind != "decision");
        }
    }

    #[test]
    fn full_target_fact_review_resolves_all_production_packs_without_host_default() {
        let path = test_path("full-fact-review");
        migrate_database(&path).expect("migrate");
        let connection = Connection::open(&path).expect("open");
        seed_project(&connection, "full-fact-project");
        drop(connection);

        let pack_facts = [
            ("web", "web-frontend"),
            ("backend", "backend-api"),
            ("database", "databases"),
            ("authentication", "authentication-authorization"),
            ("ui_surface", "accessibility"),
            ("seo_relevance", "seo-discoverability"),
            ("performance", "performance"),
            ("deployment", "devops-release-engineering"),
            ("observability", "observability-operations"),
            ("privacy", "data-privacy"),
            ("payments", "payments-financial-workflows"),
            ("ai", "ai-ml-applications"),
            ("blockchain", "blockchain-web3"),
            ("mobile", "mobile"),
            ("desktop", "desktop"),
            ("data_engineering", "data-engineering"),
            ("integrations", "third-party-integrations"),
        ];
        for (fact, pack_id) in pack_facts {
            let review =
                review_authority_at_path(&path, "full-fact-project", &reviewed_facts(&[fact]))
                    .expect("production fact review");
            assert!(review.blockers.is_empty(), "{fact}: {review:?}");
            assert!(review
                .packs
                .iter()
                .any(|pack| pack.pack_id == pack_id && pack.applicable_rules > 0));
        }

        let web_only =
            review_authority_at_path(&path, "full-fact-project", &reviewed_facts(&["web"]))
                .expect("web-only review");
        assert_eq!(
            web_only
                .packs
                .iter()
                .find(|pack| pack.pack_id == "desktop")
                .expect("desktop pack")
                .applicable_rules,
            0,
            "target project desktop must not inherit the Relintor host platform"
        );

        let unresolved = review_authority_at_path(
            &path,
            "full-fact-project",
            &AUTHORITY_FACT_FIELDS
                .iter()
                .map(|id| AuthorityFactDecision {
                    id: (*id).into(),
                    decision: AuthorityFactDecisionValue::NotSure,
                })
                .collect::<Vec<_>>(),
        )
        .expect("unresolved review remains inspectable");
        assert_eq!(unresolved.sealing_state, "BLOCKED");
        assert!(!unresolved.blockers.is_empty());

        let explicit_false =
            review_authority_at_path(&path, "full-fact-project", &reviewed_facts(&[]))
                .expect("explicit false review");
        assert_eq!(
            explicit_false
                .packs
                .iter()
                .find(|pack| pack.pack_id == "data-privacy")
                .expect("privacy pack")
                .applicable_rules,
            0
        );
        let _ = fs::remove_file(path);
    }

    #[test]
    fn stale_displayed_review_digest_is_rejected_at_seal_boundary() {
        let path = test_path("stale-review");
        migrate_database(&path).expect("migrate");
        let connection = Connection::open(&path).expect("open");
        seed_project(&connection, "stale-project");
        drop(connection);

        let review_a =
            review_authority_at_path(&path, "stale-project", &reviewed_facts(&["desktop"]))
                .expect("review A");
        let review_b = review_authority_at_path(
            &path,
            "stale-project",
            &reviewed_facts(&["desktop", "backend"]),
        )
        .expect("review B");
        assert_ne!(review_a.review_digest, review_b.review_digest);

        let stale = seal_project_mission_at_path(&path, "stale-project", &review_a.review_digest)
            .expect_err("review A must be stale after review B replaces it");
        assert!(stale.contains("STALE_AUTHORITY_REVIEW"), "{stale}");
        seal_project_mission_at_path(&path, "stale-project", &review_b.review_digest)
            .expect("current review B seals");
        let _ = fs::remove_file(path);
    }

    #[test]
    fn canonical_fact_order_and_duplicates_have_identical_authority_hashes() {
        let path = test_path("canonical-facts");
        migrate_database(&path).expect("migrate");
        let connection = Connection::open(&path).expect("open");
        seed_project(&connection, "canonical-project");
        drop(connection);

        let ordered = reviewed_facts(&["desktop", "backend", "ai"]);
        let mut reordered = ordered.clone();
        reordered.reverse();
        reordered.push(AuthorityFactDecision {
            id: "backend".into(),
            decision: AuthorityFactDecisionValue::Yes,
        });

        fn draft_for(
            path: &Path,
            project_id: &str,
            facts: &[AuthorityFactDecision],
        ) -> MissionDraft {
            let (registry, _) = production_registry().expect("production registry");
            let connection = Connection::open(path).expect("open authority database");
            let (authority, mut context, source, takeover) =
                build_project_authority(&connection, project_id).expect("project authority");
            apply_reviewed_facts(&mut context, facts).expect("canonical fact decisions");
            let authority_revision = authority.source_revision.clone();
            let scope = relintor_standards::scope_fingerprint(BTreeMap::from([
                ("project_id".into(), project_id.into()),
                ("registry_digest".into(), registry.registry_digest.clone()),
                ("source_fingerprint".into(), source),
            ]));
            AuthorityEngine
                .build_draft_with_project_authority(
                    &format!("mission-{project_id}"),
                    project_id,
                    &authority_revision,
                    &takeover,
                    registry,
                    &context,
                    scope,
                    authority,
                    Vec::new(),
                )
                .expect("draft")
        }

        let first = draft_for(&path, "canonical-project", &ordered);
        let second = draft_for(&path, "canonical-project", &reordered);
        assert_eq!(
            first.applicability_context_digest,
            second.applicability_context_digest
        );
        assert_eq!(
            authority_review_digest(&first).expect("first review digest"),
            authority_review_digest(&second).expect("second review digest")
        );
        assert_eq!(
            MissionContract::from_draft(&first)
                .expect("first contract")
                .canonical_hash()
                .expect("first contract hash"),
            MissionContract::from_draft(&second)
                .expect("second contract")
                .canonical_hash()
                .expect("second contract hash")
        );
        let _ = fs::remove_file(path);
    }

    #[test]
    fn public_config_rejects_empty_values() {
        assert!(DesktopPublicConfig::from_sources(Some(" ".into()), None, None,).is_err());
        assert!(DesktopPublicConfig::from_sources(None, Some("\t".into()), None,).is_err());
        assert!(DesktopPublicConfig::from_sources(None, None, Some(String::new()),).is_err());
    }

    #[test]
    fn public_config_requires_https_for_production_endpoints() {
        let cloud_error =
            DesktopPublicConfig::from_sources(Some("http://cloud.example.test".into()), None, None)
                .unwrap_err();
        assert!(cloud_error.contains("RELINTOR_CLOUD_API_ENDPOINT"));

        let ai_error = DesktopPublicConfig::from_sources(
            None,
            None,
            Some("http://gateway.example.test".into()),
        )
        .unwrap_err();
        assert!(ai_error.contains("RELINTOR_AI_GATEWAY_ENDPOINT"));
    }

    #[test]
    fn public_config_allows_explicit_runtime_overrides() {
        let config = DesktopPublicConfig::from_sources(
            Some("https://cloud.override.example.test/".into()),
            Some("client-override".into()),
            Some("https://gateway.override.example.test/".into()),
        )
        .expect("runtime public overrides");
        assert_eq!(
            config.cloud_api_endpoint,
            "https://cloud.override.example.test/"
        );
        assert_eq!(config.google_client_id, "client-override");
        assert_eq!(
            config.ai_gateway_endpoint,
            "https://gateway.override.example.test/"
        );
    }

    #[test]
    fn production_ai_requires_an_authenticated_cloud_session_bearer() {
        let provider = ProductionAiProvider::new("https://gateway.example.test", None);
        assert!(!provider.configured());
    }

    #[test]
    fn google_pkce_uses_s256_and_high_entropy_state() {
        let first = pkce_pair();
        let second = pkce_pair();
        assert_ne!(first.verifier, second.verifier);
        assert_ne!(first.challenge, second.challenge);
        assert_eq!(
            first.challenge,
            URL_SAFE_NO_PAD.encode(Sha256::digest(first.verifier.as_bytes()))
        );
        assert!(first.verifier.len() >= 43);
        assert!(first.challenge.len() >= 43);
    }

    #[test]
    fn loopback_callback_requires_exact_state_and_handles_cancel() {
        let state = "expected-state";
        let request = "GET /oauth/callback?code=auth-code&state=expected-state HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n";
        assert_eq!(
            parse_loopback_callback(request, state).unwrap(),
            "auth-code"
        );
        let mismatch = request.replace("expected-state", "other-state");
        assert!(parse_loopback_callback(&mismatch, state).is_err());
        let cancelled = "GET /oauth/callback?error=access_denied&state=expected-state HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n";
        assert_eq!(
            parse_loopback_callback(cancelled, state).unwrap_err(),
            "authentication cancelled"
        );
        let wrong_path =
            "GET /other?code=auth-code&state=expected-state HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n";
        assert!(parse_loopback_callback(wrong_path, state).is_err());
    }
}
