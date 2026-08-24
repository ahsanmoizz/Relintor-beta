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
    AuthenticatedP7Execution, CollectorOrchestrationResult, CompletionAuthority,
    CompletionCertificate, EvidenceManifest, EvidenceStore, EvidenceSummary,
    ExplicitUserDecisionInput, ExplicitUserDecisionRecorder, FreshnessContext,
    ProductionAiProvider, VerificationAuthority, VerificationCollectorOrchestrator,
    VerificationEngine,
};
use relintor_execution::{
    CheckpointKind, ConservativeProcessInspector, ExecutionRun, ProcessInspector,
    ProcessObservation, ProcessOwnershipRecord, RecoveryAuthority, RecoveryCoordinator,
    RecoveryDisposition, RecoveryStore, SchedulerPolicy, SessionEndState,
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
use std::ffi::OsStr;
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc, Condvar, Mutex, OnceLock,
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Manager};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons};
use tauri_plugin_notification::NotificationExt;
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
const EMBEDDED_GOOGLE_CLIENT_SECRET: &str = match option_env!("RELINTOR_GOOGLE_CLIENT_SECRET") {
    Some(value) => value,
    None => "",
};
const EMBEDDED_AI_GATEWAY_ENDPOINT: &str = match option_env!("RELINTOR_AI_GATEWAY_ENDPOINT") {
    Some(value) => value,
    None => "https://governments-amsterdam-incoming-previews.trycloudflare.com",
};

#[derive(Clone, PartialEq, Eq)]
struct DesktopPublicConfig {
    cloud_api_endpoint: String,
    google_client_id: String,
    google_client_secret: String,
    ai_gateway_endpoint: String,
}

impl DesktopPublicConfig {
    fn from_env() -> Result<Self, String> {
        Self::from_sources(
            std::env::var("RELINTOR_CLOUD_API_ENDPOINT").ok(),
            std::env::var("RELINTOR_GOOGLE_CLIENT_ID").ok(),
            std::env::var("RELINTOR_GOOGLE_CLIENT_SECRET").ok(),
            std::env::var("RELINTOR_AI_GATEWAY_ENDPOINT").ok(),
        )
    }

    fn from_sources(
        cloud_override: Option<String>,
        client_override: Option<String>,
        client_secret_override: Option<String>,
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
            google_client_secret: required_public_value(
                "RELINTOR_GOOGLE_CLIENT_SECRET",
                client_secret_override.as_deref(),
                EMBEDDED_GOOGLE_CLIENT_SECRET,
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
        browser_command("rundll32.exe")
            .arg("url.dll,FileProtocolHandler")
            .arg(url)
            .spawn()
            .map_err(|_| "could not open the system browser".to_string())?;
        return Ok(());
    }
    #[cfg(target_os = "macos")]
    {
        browser_command("open")
            .arg(url)
            .spawn()
            .map_err(|_| "could not open the system browser".to_string())?;
        return Ok(());
    }
    #[cfg(target_os = "linux")]
    {
        browser_command("xdg-open")
            .arg(url)
            .spawn()
            .map_err(|_| "could not open the system browser".to_string())?;
        return Ok(());
    }
    #[allow(unreachable_code)]
    Err("system browser integration is unavailable on this platform".into())
}

fn browser_command<S: AsRef<OsStr>>(program: S) -> Command {
    Command::new(program)
}

fn https_client() -> Result<Client, String> {
    Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|_| "could not construct the authentication client".into())
}

fn google_token_form(
    code: &str,
    verifier: &str,
    redirect_uri: &str,
    client_id: &str,
    client_secret: &str,
) -> String {
    let mut form = url::form_urlencoded::Serializer::new(String::new());
    form.append_pair("code", code);
    form.append_pair("client_id", client_id);
    form.append_pair("client_secret", client_secret);
    form.append_pair("code_verifier", verifier);
    form.append_pair("redirect_uri", redirect_uri);
    form.append_pair("grant_type", "authorization_code");
    form.finish()
}

fn exchange_google_code(
    client: &Client,
    code: &str,
    verifier: &str,
    redirect_uri: &str,
    client_id: &str,
    client_secret: &str,
) -> Result<String, String> {
    let response = client
        .post("https://oauth2.googleapis.com/token")
        .header("content-type", "application/x-www-form-urlencoded")
        .body(google_token_form(
            code,
            verifier,
            redirect_uri,
            client_id,
            client_secret,
        ))
        .send()
        .map_err(|_| "Google authentication failed".to_string())?;
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().unwrap_or_default();
        let error_code = serde_json::from_str::<serde_json::Value>(&body)
            .ok()
            .and_then(|value| {
                value
                    .get("error")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string)
            })
            .unwrap_or_else(|| "unknown_error".to_string());

        return Err(format!(
            "Google authentication failed ({status}): {error_code}"
        ));
    }
    let value: serde_json::Value = response
        .json()
        .map_err(|_| "Google authentication returned invalid JSON".to_string())?;
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
        &config.google_client_secret,
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

#[derive(Debug, Clone, Serialize)]
struct ProjectSummaryView {
    project_id: String,
    name: String,
    root_path: Option<String>,
    state: String,
    investigation_status: Option<String>,
    takeover_fingerprint: Option<String>,
    sealed_revision: Option<u64>,
}

#[derive(Debug, Serialize)]
struct ProjectOpenView {
    project: ProjectSummaryView,
    investigation: Option<InvestigationView>,
    authority_facts: Vec<AuthorityFactDecision>,
    handoff: Option<ExecutionHandoff>,
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
    project_name: String,
    mission_id: String,
    revision: u64,
    state: String,
    watchdog_state: String,
    current_turn: u32,
    active_task: Option<String>,
    current_task_objective: Option<String>,
    recovery_task_id: Option<String>,
    recovery_task_objective: Option<String>,
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
    recovery_action: String,
    dispatch_active: bool,
    execution_phase: String,
    execution_time_limit_ms: u64,
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
    workflow_stage: String,
    summary: String,
    human_decisions: Vec<HumanDecisionPromptView>,
    collector_activity: Vec<String>,
    collection_failures: Vec<String>,
    detail: String,
}

#[derive(Debug, Clone, Serialize)]
struct HumanDecisionPromptView {
    requirement_id: String,
    title: String,
    question: String,
    summary: String,
    criterion_ids: Vec<String>,
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

const ANTIGRAVITY_OFFICIAL_INSTALLER: &str = "https://antigravity.google/cli/install.ps1";
const ANTIGRAVITY_MIN_WORKING_SPACE_BYTES: u64 = 5 * 1024 * 1024 * 1024;
const ANTIGRAVITY_MAX_CAPTURE_BYTES: u64 = 64 * 1024;
const ANTIGRAVITY_READINESS_TTL_MS: i128 = 5_000;
const ANTIGRAVITY_CHILD_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const ANTIGRAVITY_AUTH_TIMEOUT: Duration = Duration::from_secs(10 * 60);
#[cfg(windows)]
const WINDOWS_CREATE_NEW_CONSOLE: u32 = 0x0000_0010;
const ANTIGRAVITY_TRUSTED_KEYS: &str =
    include_str!("../../../../integrations/antigravity/plugin/trusted-keys.json");

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct AntigravitySetupRecord {
    selected_install_root: Option<String>,
    cli_path: Option<String>,
    cli_version: Option<String>,
    installation_state: String,
    authentication_state: String,
    last_readiness_check_ms: i128,
    #[serde(default)]
    bridge_install_root: Option<String>,
    #[serde(default)]
    bridge_package_version: Option<String>,
}

impl Default for AntigravitySetupRecord {
    fn default() -> Self {
        Self {
            selected_install_root: None,
            cli_path: None,
            cli_version: None,
            installation_state: "not_checked".into(),
            authentication_state: "not_checked".into(),
            last_readiness_check_ms: 0,
            bridge_install_root: None,
            bridge_package_version: None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
struct DiskSpaceView {
    available_bytes: Option<u64>,
    safe_minimum_bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
struct AntigravitySetupView {
    stage: String,
    active: bool,
    progress_percent: Option<u8>,
    progress_indeterminate: bool,
    storage_path: Option<String>,
    recommended_storage_path: Option<String>,
    storage_detail: String,
    c: DiskSpaceView,
    d: DiskSpaceView,
    local_appdata_state: String,
    cli_path: Option<String>,
    cli_version: Option<String>,
    authentication_status: String,
    adapter_ready: bool,
    consent_required: bool,
    automatic_install_available: bool,
    requires_location: bool,
    can_cancel: bool,
    can_retry: bool,
    detail: String,
    error: Option<String>,
    advanced_details: String,
}

#[derive(Debug)]
struct AntigravitySetupRuntime {
    state: Mutex<Option<AntigravitySetupView>>,
    cancel: AtomicBool,
    operation_active: AtomicBool,
    active_child_pid: Mutex<Option<u32>>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

static ANTIGRAVITY_SETUP_RUNTIME: OnceLock<Arc<AntigravitySetupRuntime>> = OnceLock::new();

fn antigravity_setup_runtime() -> Arc<AntigravitySetupRuntime> {
    ANTIGRAVITY_SETUP_RUNTIME
        .get_or_init(|| {
            Arc::new(AntigravitySetupRuntime {
                state: Mutex::new(None),
                cancel: AtomicBool::new(false),
                operation_active: AtomicBool::new(false),
                active_child_pid: Mutex::new(None),
                worker: Mutex::new(None),
            })
        })
        .clone()
}

fn setup_config_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|error| format!("resolve Relintor setup storage: {error}"))?
        .join("antigravity")
        .join("setup.json"))
}

fn load_antigravity_setup_record(app: Option<&AppHandle>) -> AntigravitySetupRecord {
    let Some(app) = app else {
        return AntigravitySetupRecord::default();
    };
    setup_config_path(app)
        .ok()
        .and_then(|path| fs::read(path).ok())
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

fn configured_antigravity_cli(app: &AppHandle) -> Option<PathBuf> {
    load_antigravity_setup_record(Some(app))
        .cli_path
        .map(PathBuf::from)
        .filter(|path| path.is_file())
        .or_else(|| {
            relintor_antigravity::detect()
                .executable_path
                .map(PathBuf::from)
                .filter(|path| path.is_file())
        })
}

fn persist_antigravity_setup_record(
    app: &AppHandle,
    record: &AntigravitySetupRecord,
) -> Result<(), String> {
    let path = setup_config_path(app)?;
    let parent = path
        .parent()
        .ok_or_else(|| "Relintor setup storage has no parent directory".to_string())?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("prepare Relintor setup storage: {error}"))?;
    let temporary = parent.join(format!(".setup-{}.tmp", std::process::id()));
    let bytes = serde_json::to_vec_pretty(record)
        .map_err(|error| format!("encode Relintor setup metadata: {error}"))?;
    fs::write(&temporary, bytes)
        .map_err(|error| format!("write Relintor setup metadata: {error}"))?;
    atomic_replace_file(&temporary, &path)
        .map_err(|error| format!("commit Relintor setup metadata: {error}"))?;
    invalidate_antigravity_readiness();
    Ok(())
}

fn atomic_replace_file(temporary: &Path, destination: &Path) -> Result<(), String> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;

        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn MoveFileExW(existing: *const u16, replacement: *const u16, flags: u32) -> i32;
        }

        const MOVEFILE_REPLACE_EXISTING: u32 = 0x0000_0001;
        const MOVEFILE_WRITE_THROUGH: u32 = 0x0000_0008;
        let existing = temporary
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();
        let replacement = destination
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();
        let success = unsafe {
            MoveFileExW(
                existing.as_ptr(),
                replacement.as_ptr(),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        };
        if success == 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        fs::rename(temporary, destination).map_err(|error| error.to_string())
    }
}

fn disk_free_bytes(path: &Path) -> Option<u64> {
    #[cfg(windows)]
    {
        use std::ptr::null_mut;
        use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;

        let wide = wide_path(path);
        let mut available = 0_u64;
        let result =
            unsafe { GetDiskFreeSpaceExW(wide.as_ptr(), null_mut(), null_mut(), &mut available) };
        if result != 0 {
            Some(available)
        } else {
            None
        }
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        None
    }
}

#[cfg(windows)]
fn wide_path(path: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}

fn local_appdata_root() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
}

fn hidden_command<S: AsRef<OsStr>>(program: S) -> Command {
    let command = Command::new(program);
    #[cfg(windows)]
    let mut command = command;
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(relintor_antigravity::WINDOWS_CREATE_NO_WINDOW);
    }
    command
}

fn interactive_auth_command(path: &Path) -> Command {
    let mut command = Command::new(path);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(WINDOWS_CREATE_NEW_CONSOLE);
    }
    command
        .current_dir(std::env::temp_dir())
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    command
}

fn user_home_root() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
}

fn antigravity_plugin_install_root() -> Option<PathBuf> {
    std::env::var_os("RELINTOR_ANTIGRAVITY_PLUGIN_ROOT")
        .map(PathBuf::from)
        .or_else(|| {
            user_home_root().map(|home| {
                home.join(".gemini")
                    .join("config")
                    .join("plugins")
                    .join(relintor_antigravity::RELINTOR_PLUGIN_NAME)
            })
        })
}

fn antigravity_plugins_config_path() -> Option<PathBuf> {
    user_home_root().map(|home| home.join(".gemini").join("config").join("plugins.json"))
}

fn plugin_path_is_registered(install_root: &Path) -> bool {
    let Some(config_path) = antigravity_plugins_config_path() else {
        return false;
    };
    let Ok(bytes) = fs::read(config_path) else {
        return false;
    };
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
        return false;
    };
    let Some(entries) = value.get("entries").and_then(serde_json::Value::as_array) else {
        return false;
    };
    let parent = install_root
        .parent()
        .and_then(|path| fs::canonicalize(path).ok())
        .unwrap_or_else(|| install_root.parent().unwrap_or(install_root).to_path_buf());
    entries.iter().any(|entry| {
        entry
            .get("path")
            .and_then(serde_json::Value::as_str)
            .and_then(|path| fs::canonicalize(path).ok())
            .is_some_and(|path| path == parent)
    })
}

fn ensure_plugin_path_registered(install_root: &Path) -> Result<(), String> {
    let config_path = antigravity_plugins_config_path().ok_or_else(|| {
        "Antigravity global plugin configuration path is unavailable.".to_string()
    })?;
    let mut value = if config_path.is_file() {
        serde_json::from_slice::<serde_json::Value>(
            &fs::read(&config_path)
                .map_err(|error| format!("read Antigravity plugin configuration: {error}"))?,
        )
        .map_err(|error| format!("Antigravity plugin configuration is invalid: {error}"))?
    } else {
        serde_json::json!({"entries": []})
    };
    let entries = value
        .as_object_mut()
        .ok_or_else(|| "Antigravity plugin configuration must be a JSON object.".to_string())?
        .entry("entries")
        .or_insert_with(|| serde_json::json!([]));
    let entries = entries
        .as_array_mut()
        .ok_or_else(|| "Antigravity plugin configuration entries are invalid.".to_string())?;
    let parent = install_root
        .parent()
        .ok_or_else(|| "Antigravity plugin installation root has no parent.".to_string())?;
    let parent_text = parent.display().to_string();
    if !entries.iter().any(|entry| {
        entry
            .get("path")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|path| {
                fs::canonicalize(path)
                    .ok()
                    .is_some_and(|candidate| candidate == parent)
                    || path == parent_text
            })
    }) {
        entries.push(serde_json::json!({"path": parent_text}));
    }
    if let Some(parent) = config_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("prepare Antigravity plugin configuration: {error}"))?;
    }
    fs::write(
        &config_path,
        serde_json::to_vec_pretty(&value)
            .map_err(|error| format!("encode Antigravity plugin configuration: {error}"))?,
    )
    .map_err(|error| format!("write Antigravity plugin configuration: {error}"))
}

#[derive(Debug, Deserialize)]
struct BridgeTrustedKeys {
    schema_version: u32,
    active_key_id: String,
    keys: Vec<BridgeTrustedKey>,
}

#[derive(Debug, Deserialize)]
struct BridgeTrustedKey {
    key_id: String,
    public_key_base64: String,
    status: String,
}

fn trusted_bridge_key() -> Result<ed25519_dalek::VerifyingKey, String> {
    let trust: BridgeTrustedKeys = serde_json::from_str(ANTIGRAVITY_TRUSTED_KEYS)
        .map_err(|error| format!("Relintor bridge trust configuration is invalid: {error}"))?;
    if trust.schema_version != 1 {
        return Err("Relintor bridge trust configuration is unsupported.".into());
    }
    let entry = trust
        .keys
        .into_iter()
        .find(|key| key.key_id == trust.active_key_id && key.status == "active")
        .ok_or_else(|| "Relintor bridge trust root is unavailable.".to_string())?;
    let key = relintor_antigravity::verifying_key_from_base64(&entry.public_key_base64)
        .map_err(|error| error.to_string())?;
    if relintor_antigravity::trusted_key_id(&key) != entry.key_id {
        return Err("Relintor bridge trust root identity is invalid.".into());
    }
    Ok(key)
}

fn installed_bridge_root() -> Option<PathBuf> {
    antigravity_plugin_install_root().filter(|root| root.is_dir())
}

fn default_antigravity_root() -> Option<PathBuf> {
    std::env::var_os("RELINTOR_ANTIGRAVITY_INSTALL_ROOT")
        .map(PathBuf::from)
        .or_else(|| local_appdata_root().map(|path| path.join("agy")))
}

fn preferred_antigravity_root() -> Option<PathBuf> {
    let root = default_antigravity_root()?;
    let drive = root.components().next()?.as_os_str().to_string_lossy();
    let drive_root = PathBuf::from(format!("{drive}\\"));
    (meets_storage_reserve(disk_free_bytes(&drive_root)) && path_is_writable(&drive_root))
        .then_some(root)
}

fn meets_storage_reserve(available_bytes: Option<u64>) -> bool {
    available_bytes.is_some_and(|free| free >= ANTIGRAVITY_MIN_WORKING_SPACE_BYTES)
}

fn path_is_writable(path: &Path) -> bool {
    let candidate = if path.is_dir() {
        path.to_path_buf()
    } else {
        path.parent().unwrap_or(path).to_path_buf()
    };
    fs::metadata(candidate)
        .map(|metadata| metadata.is_dir() && !metadata.permissions().readonly())
        .unwrap_or(false)
}

#[cfg(windows)]
fn is_reparse_point(path: &Path) -> bool {
    use std::os::windows::fs::MetadataExt;
    fs::symlink_metadata(path)
        .map(|metadata| metadata.file_attributes() & 0x400 != 0)
        .unwrap_or(false)
}

#[cfg(not(windows))]
fn is_reparse_point(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .map(|metadata| metadata.file_type().is_symlink())
        .unwrap_or(false)
}

fn find_cli_path(root: &Path) -> Option<PathBuf> {
    [
        root.join("bin").join("agy.exe"),
        root.join("bin").join("agy"),
        root.join("agy.exe"),
        root.join("agy"),
    ]
    .into_iter()
    .find(|candidate| candidate.is_file())
}

fn detect_cli_at(path: Option<&Path>) -> Option<relintor_antigravity::CompatibilityReport> {
    let path = path?;
    let report = relintor_antigravity::detect_with_override(Some(path.to_path_buf()));
    report.executable_present.then_some(report)
}

fn local_appdata_install_state() -> (String, Option<PathBuf>, Option<PathBuf>) {
    let Some(local_appdata) = local_appdata_root() else {
        return ("unavailable".into(), None, None);
    };
    let link = local_appdata.join("agy");
    let Ok(metadata) = fs::symlink_metadata(&link) else {
        return ("absent".into(), None, None);
    };
    if is_reparse_point(&link) {
        let target = fs::canonicalize(&link).ok();
        let valid_cli = target.as_deref().and_then(find_cli_path);
        return (
            if valid_cli.is_some() {
                "junction_valid"
            } else {
                "junction_invalid"
            }
            .into(),
            target,
            valid_cli,
        );
    }
    if metadata.is_dir() {
        let valid_cli = find_cli_path(&link);
        return (
            if valid_cli.is_some() {
                "real_directory_valid"
            } else {
                "real_directory_conflict"
            }
            .into(),
            Some(link),
            valid_cli,
        );
    }
    ("conflict".into(), Some(link), None)
}

#[derive(Debug, Default)]
struct AntigravityReadinessState {
    cached: Option<(i128, AntigravityHealth)>,
    in_flight: bool,
}

#[derive(Debug, Default)]
struct AntigravityReadinessRuntime {
    state: Mutex<AntigravityReadinessState>,
    changed: Condvar,
}

static ANTIGRAVITY_READINESS_RUNTIME: OnceLock<AntigravityReadinessRuntime> = OnceLock::new();

fn antigravity_readiness_runtime() -> &'static AntigravityReadinessRuntime {
    ANTIGRAVITY_READINESS_RUNTIME.get_or_init(AntigravityReadinessRuntime::default)
}

fn invalidate_antigravity_readiness() {
    let runtime = antigravity_readiness_runtime();
    if let Ok(mut state) = runtime.state.lock() {
        state.cached = None;
        runtime.changed.notify_all();
    }
}

fn health_antigravity_for_app(app: Option<&AppHandle>) -> AntigravityHealth {
    let runtime = antigravity_readiness_runtime();
    let setup_in_progress = antigravity_setup_runtime()
        .operation_active
        .load(Ordering::Acquire);
    loop {
        let mut state = runtime
            .state
            .lock()
            .expect("Antigravity readiness state is not poisoned");
        let now = execution_now_ms() as i128;
        if let Some((checked_at, value)) = state.cached.as_ref() {
            if setup_in_progress || now.saturating_sub(*checked_at) <= ANTIGRAVITY_READINESS_TTL_MS
            {
                return value.clone();
            }
        }
        if state.in_flight {
            state = runtime
                .changed
                .wait(state)
                .expect("Antigravity readiness state is not poisoned");
            drop(state);
            continue;
        }
        state.in_flight = true;
        drop(state);
        let value = health_antigravity_uncached(app);
        let mut state = runtime
            .state
            .lock()
            .expect("Antigravity readiness state is not poisoned");
        state.cached = Some((execution_now_ms() as i128, value.clone()));
        state.in_flight = false;
        runtime.changed.notify_all();
        return value;
    }
}

fn health_antigravity_uncached(app: Option<&AppHandle>) -> AntigravityHealth {
    let record = load_antigravity_setup_record(app);
    let configured_path = app.and_then(configured_antigravity_cli).or_else(|| {
        record
            .cli_path
            .as_deref()
            .map(PathBuf::from)
            .filter(|path| path.is_file())
    });
    let report = if configured_path.is_some() {
        relintor_antigravity::detect_with_override(configured_path)
    } else {
        relintor_antigravity::detect()
    };
    let (bridge_ready, bridge_detail) = antigravity_bridge_readiness(report.version.as_deref());
    let auth_ready = record.authentication_state == "signed_in";
    let cli_ready = report.sealed_execution_allowed();
    let adapter_ready = antigravity_adapter_ready(cli_ready, auth_ready, bridge_ready);
    let status = match report.compatibility {
        relintor_antigravity::Compatibility::Supported if adapter_ready => "ready",
        relintor_antigravity::Compatibility::Supported if !auth_ready => "authentication_required",
        relintor_antigravity::Compatibility::Supported => "compatible",
        relintor_antigravity::Compatibility::Unsupported => "unsupported",
        relintor_antigravity::Compatibility::Unknown => "unknown",
        relintor_antigravity::Compatibility::NotInstalled => "not_installed",
    };
    let cli_status = if !report.executable_present {
        "setup_required"
    } else if report.cli_invocation_capability {
        "installed_and_invokable"
    } else {
        "installed_unverified"
    };
    let authentication_status = if !report.executable_present {
        "not_available"
    } else if auth_ready {
        "ready"
    } else {
        "sign_in_required"
    };
    let detail = if !report.executable_present {
        "Relintor needs the Antigravity CLI to execute this mission.".into()
    } else if !report.sealed_execution_allowed() {
        report.detail
    } else if !auth_ready {
        "Antigravity needs Google sign-in before it can execute tasks.".into()
    } else if !bridge_ready {
        bridge_detail.into()
    } else {
        "Antigravity is ready for bounded Relintor execution.".into()
    };
    AntigravityHealth {
        status: status.into(),
        version: report.version,
        executable: report.executable_path,
        compatibility: format!("{:?}", report.compatibility).to_ascii_lowercase(),
        cli_invocation_capability: report.cli_invocation_capability,
        plugin_hook_capability: report.plugin_hook_capability,
        ide_status: "not_verified".into(),
        cli_status: cli_status.into(),
        authentication_status: authentication_status.into(),
        executable_detectable: report.executable_present,
        adapter_ready,
        setup_required: !adapter_ready,
        environment: report.environment,
        platform: report.platform,
        detected_at_ms: report.detected_at_ms,
        detail,
    }
}

#[tauri::command]
fn health_antigravity(app: AppHandle) -> AntigravityHealth {
    health_antigravity_for_app(Some(&app))
}

fn antigravity_adapter_ready(cli_ready: bool, auth_ready: bool, bridge_ready: bool) -> bool {
    cli_ready && auth_ready && bridge_ready
}

fn antigravity_bridge_readiness(cli_version: Option<&str>) -> (bool, &'static str) {
    let Some(install_root) = installed_bridge_root() else {
        return (
            false,
            "Antigravity is installed. Relintor is preparing its verified connection.",
        );
    };
    let Ok(trusted_key) = trusted_bridge_key() else {
        return (
            false,
            "Relintor could not load its bridge trust root. Execution remains blocked until the signed package can be verified.",
        );
    };
    if relintor_antigravity::verify_installed_bridge(&install_root, &trusted_key, cli_version)
        .is_err()
    {
        return (
            false,
            "Relintor could not verify its installed Antigravity connection. Execution remains blocked.",
        );
    }
    if !plugin_path_is_registered(&install_root) {
        return (
            false,
            "Relintor's signed Antigravity plugin is installed but not registered with the CLI. Re-run setup to enable the verified hook bridge.",
        );
    }
    (
        true,
        "The signed Relintor bridge package is ready for bounded execution.",
    )
}

fn setup_c_drive() -> PathBuf {
    #[cfg(windows)]
    {
        PathBuf::from(r"C:\")
    }
    #[cfg(not(windows))]
    {
        PathBuf::from("/")
    }
}

fn setup_d_drive() -> PathBuf {
    #[cfg(windows)]
    {
        PathBuf::from(r"D:\")
    }
    #[cfg(not(windows))]
    {
        PathBuf::from("/")
    }
}

fn setup_view(
    app: &AppHandle,
    stage_override: Option<&str>,
    active: bool,
    error: Option<String>,
) -> AntigravitySetupView {
    let record = load_antigravity_setup_record(Some(app));
    let readiness = health_antigravity_for_app(Some(app));
    let (local_state, local_root, local_cli) = local_appdata_install_state();
    let selected_root = record
        .selected_install_root
        .as_deref()
        .map(PathBuf::from)
        .or(local_root)
        .or_else(preferred_antigravity_root);
    let stage = stage_override.map(str::to_string).unwrap_or_else(|| {
        if readiness.adapter_ready {
            "READY".into()
        } else if !readiness.executable_detectable {
            "CHECKING_EXISTING_INSTALL".into()
        } else if readiness.authentication_status == "sign_in_required" {
            "AUTH_REQUIRED".into()
        } else {
            "PREPARING_RELINTOR_BRIDGE".into()
        }
    });
    let c = DiskSpaceView {
        available_bytes: disk_free_bytes(&setup_c_drive()),
        safe_minimum_bytes: ANTIGRAVITY_MIN_WORKING_SPACE_BYTES,
    };
    let d = DiskSpaceView {
        available_bytes: disk_free_bytes(&setup_d_drive()),
        safe_minimum_bytes: ANTIGRAVITY_MIN_WORKING_SPACE_BYTES,
    };
    let storage_detail = if selected_root.is_some() {
        format!(
            "Relintor keeps the official CLI in the selected location and never replaces an existing folder. A {} safety reserve is required before setup begins.",
            "5 GB"
        )
    } else {
        "Choose a writable installation location with at least 5 GB available. Relintor will not silently use a critically low system drive.".into()
    };
    let detail = error.clone().unwrap_or_else(|| readiness.detail.clone());
    let can_retry = !active && !readiness.adapter_ready;
    AntigravitySetupView {
        stage,
        active,
        progress_percent: None,
        progress_indeterminate: active,
        storage_path: selected_root
            .as_ref()
            .map(|path| path.display().to_string()),
        recommended_storage_path: preferred_antigravity_root()
            .map(|path| path.display().to_string()),
        storage_detail,
        c: c.clone(),
        d: d.clone(),
        local_appdata_state: local_state.clone(),
        cli_path: record
            .cli_path
            .or_else(|| local_cli.map(|path| path.display().to_string())),
        cli_version: record.cli_version.or(readiness.version),
        authentication_status: readiness.authentication_status.clone(),
        adapter_ready: readiness.adapter_ready,
        consent_required: !readiness.adapter_ready,
        automatic_install_available: cfg!(windows),
        requires_location: selected_root.is_none() && !readiness.executable_detectable,
        can_cancel: active,
        can_retry,
        detail,
        error,
        advanced_details: format!(
            "stage={}; cli_status={}; compatibility={}; bridge_ready={}; local_appdata_state={}; bridge_install_root={:?}; bridge_package_version={:?}; c_free_bytes={:?}; d_free_bytes={:?}",
            readiness.status,
            readiness.cli_status,
            readiness.compatibility,
            readiness.adapter_ready,
            local_state,
            record.bridge_install_root,
            record.bridge_package_version,
            c.available_bytes,
            d.available_bytes
        ),
    }
}

fn set_setup_view(
    app: &AppHandle,
    runtime: &AntigravitySetupRuntime,
    stage: &str,
    active: bool,
    detail: Option<String>,
    error: Option<String>,
) {
    let mut view = setup_view(app, Some(stage), active, error);
    if let Some(detail) = detail {
        view.detail = detail;
    }
    if let Ok(mut state) = runtime.state.lock() {
        *state = Some(view);
    }
}

fn setup_cancelled(runtime: &AntigravitySetupRuntime) -> bool {
    runtime.cancel.load(Ordering::Acquire)
}

fn prepare_install_root(root: &Path) -> Result<(), String> {
    if !root.is_absolute() {
        return Err("Choose an absolute installation location.".into());
    }
    if root.exists() {
        if is_reparse_point(root) {
            return Err("The selected installation location is a link or reparse point. Choose a real folder.".into());
        }
        if !root.is_dir() {
            return Err("The selected installation location is not a folder.".into());
        }
        if fs::read_dir(root)
            .map_err(|error| format!("inspect selected installation location: {error}"))?
            .next()
            .is_some()
        {
            return Err(
                "The selected installation folder is not empty. Relintor will not overwrite it."
                    .into(),
            );
        }
    } else {
        fs::create_dir_all(root)
            .map_err(|error| format!("create selected installation location: {error}"))?;
    }
    if !path_is_writable(root) {
        return Err(
            "The selected installation location is not writable by this Windows user.".into(),
        );
    }
    Ok(())
}

fn ensure_local_appdata_junction(target: &Path) -> Result<(), String> {
    #[cfg(windows)]
    {
        let local_appdata = local_appdata_root()
            .ok_or_else(|| "Windows user data location is unavailable.".to_string())?;
        let link = local_appdata.join("agy");
        if link.exists() || fs::symlink_metadata(&link).is_ok() {
            if is_reparse_point(&link) {
                let actual = fs::canonicalize(&link)
                    .map_err(|error| format!("verify existing Antigravity junction: {error}"))?;
                let expected = fs::canonicalize(target).map_err(|error| {
                    format!("verify Antigravity installation location: {error}")
                })?;
                if actual == expected {
                    return Ok(());
                }
                return Err("The existing Antigravity junction points somewhere else. Relintor will not replace it.".into());
            }
            return Err("An existing %LOCALAPPDATA%\\agy folder was found. Choose that existing CLI or a different location.".into());
        }
        fs::create_dir_all(&local_appdata)
            .map_err(|error| format!("prepare Windows user data location: {error}"))?;
        let status = hidden_command("cmd.exe")
            .args(["/d", "/c", "mklink", "/J"])
            .arg(&link)
            .arg(target)
            .status()
            .map_err(|error| format!("create Antigravity junction: {error}"))?;
        if !status.success() {
            return Err("Windows could not create the Antigravity storage junction.".into());
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = target;
        Err("Automatic Antigravity setup is supported only on Windows in this beta.".into())
    }
}

fn installer_command() -> Command {
    let mut command = hidden_command("powershell.exe");
    command
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
        ])
        .arg("-Command")
        .arg(format!(
            "$ErrorActionPreference='Stop'; irm '{}' | iex",
            ANTIGRAVITY_OFFICIAL_INSTALLER
        ))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}

fn bridge_package_source(app: &AppHandle) -> Result<PathBuf, String> {
    let candidates = [
        std::env::var_os("RELINTOR_ANTIGRAVITY_BRIDGE_PACKAGE").map(PathBuf::from),
        app.path()
            .resource_dir()
            .ok()
            .map(|path| path.join("antigravity-bridge")),
        #[cfg(debug_assertions)]
        Some(
            workspace_root()
                .join("integrations")
                .join("antigravity")
                .join("plugin")
                .join("beta-package"),
        ),
    ];
    candidates
        .into_iter()
        .flatten()
        .find(|path| path.join(relintor_antigravity::PLUGIN_PACKAGE_MANIFEST_FILE).is_file())
        .ok_or_else(|| {
            "Relintor's signed Antigravity package is not present in this build. Execution remains blocked until the release package is provisioned.".into()
        })
}

fn run_bridge_self_test(path: &Path) -> Result<(), String> {
    let mut command = hidden_command(path);
    let output = command
        .arg("--self-test")
        .output()
        .map_err(|error| format!("verify the installed bridge executable: {error}"))?;
    if output.status.success() && output.stdout == b"RELINTOR_BRIDGE_READY\n" {
        Ok(())
    } else {
        Err("The installed Relintor bridge did not pass its self-test.".into())
    }
}

fn run_cli_plugin_probe(cli: &Path, install_root: &Path) -> Result<(), String> {
    ensure_plugin_path_registered(install_root)?;
    let output = hidden_command(cli)
        .args(["plugin", "validate"])
        .arg(install_root)
        .current_dir(std::env::temp_dir())
        .output()
        .map_err(|error| format!("check Antigravity plugin discovery: {error}"))?;
    let mut combined = output.stdout;
    combined.extend_from_slice(&output.stderr);
    let discovered = String::from_utf8_lossy(&combined);
    if output.status.success()
        && discovered.contains("hooks")
        && plugin_path_is_registered(install_root)
    {
        Ok(())
    } else {
        Err("Antigravity did not report the installed Relintor plugin.".into())
    }
}

fn provision_antigravity_bridge(
    app: &AppHandle,
    runtime: &AntigravitySetupRuntime,
    record: &mut AntigravitySetupRecord,
    cli: &Path,
    cli_version: Option<&str>,
) -> Result<(), String> {
    set_setup_view(
        app,
        runtime,
        "PREPARING_RELINTOR_BRIDGE",
        true,
        Some("Antigravity is installed. Relintor is preparing its verified connection.".into()),
        None,
    );
    if setup_cancelled(runtime) {
        return Err("SETUP_CANCELLED".into());
    }
    let package_root = bridge_package_source(app)?;
    let trusted_key = trusted_bridge_key()?;
    let manifest = relintor_antigravity::read_plugin_package_manifest(&package_root)
        .map_err(|error| error.to_string())?;
    set_setup_view(
        app,
        runtime,
        "VERIFYING_BRIDGE_SIGNATURE",
        true,
        Some("Verifying the Relintor bridge signature, target, and file digests.".into()),
        None,
    );
    relintor_antigravity::verify_plugin_package(
        &package_root,
        &manifest,
        &trusted_key,
        cli_version,
    )
    .map_err(|error| error.to_string())?;
    if setup_cancelled(runtime) {
        return Err("SETUP_CANCELLED".into());
    }
    let install_root = antigravity_plugin_install_root()
        .ok_or_else(|| "The Antigravity user plugin location is unavailable.".to_string())?;
    set_setup_view(
        app,
        runtime,
        "INSTALLING_RELINTOR_BRIDGE",
        true,
        Some(
            "Installing only the verified Relintor plugin into Antigravity's user plugin location."
                .into(),
        ),
        None,
    );
    let installed = relintor_antigravity::install_verified_plugin(
        &package_root,
        &install_root,
        &manifest,
        &trusted_key,
    )
    .map_err(|error| error.to_string())?;
    if !installed.verified || setup_cancelled(runtime) {
        return Err(if setup_cancelled(runtime) {
            "SETUP_CANCELLED".into()
        } else {
            "The Relintor bridge installation was not verified.".into()
        });
    }
    set_setup_view(
        app,
        runtime,
        "VERIFYING_INSTALLED_BRIDGE",
        true,
        Some("Verifying the installed plugin again after atomic replacement.".into()),
        None,
    );
    let bridge_identity =
        relintor_antigravity::verify_installed_bridge(&install_root, &trusted_key, cli_version)
            .map_err(|error| error.to_string())?;
    let bridge_manifest: relintor_antigravity::BridgeManifest = serde_json::from_slice(
        &fs::read(install_root.join("bridge-manifest.json"))
            .map_err(|error| format!("read installed bridge identity: {error}"))?,
    )
    .map_err(|error| format!("installed bridge identity is invalid: {error}"))?;
    if bridge_manifest.signing_status != "verified" || bridge_manifest.expected_sha256.is_none() {
        return Err("The installed Relintor bridge is not signed with an exact digest.".into());
    }
    let bridge_path = PathBuf::from(bridge_identity.canonical_path);
    set_setup_view(
        app,
        runtime,
        "TESTING_ADAPTER",
        true,
        Some("Testing the bridge executable and confirming Antigravity plugin discovery.".into()),
        None,
    );
    run_bridge_self_test(&bridge_path)?;
    run_cli_plugin_probe(cli, &install_root)?;
    record.bridge_install_root = Some(install_root.display().to_string());
    record.bridge_package_version = Some(manifest.package_version);
    record.last_readiness_check_ms = execution_now_ms() as i128;
    persist_antigravity_setup_record(app, record)?;
    set_setup_view(
        app,
        runtime,
        "READY",
        false,
        Some("Antigravity and the verified Relintor bridge are ready. Revalidate and retry the mission.".into()),
        None,
    );
    Ok(())
}

fn run_setup_process(
    mut child: std::process::Child,
    runtime: &AntigravitySetupRuntime,
    timeout: Duration,
    failure_message: &str,
) -> Result<(), String> {
    let deadline = Instant::now() + timeout;
    if let Ok(mut active_child_pid) = runtime.active_child_pid.lock() {
        *active_child_pid = Some(child.id());
    }
    let result = loop {
        if setup_cancelled(runtime) {
            let _ = child.kill();
            let _ = child.wait();
            break Err("SETUP_CANCELLED".into());
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            break Err("SETUP_TIMEOUT".into());
        }
        match child
            .try_wait()
            .map_err(|error| format!("monitor Antigravity setup: {error}"))?
        {
            Some(status) if status.success() => {
                if let Some(mut stdout) = child.stdout.take() {
                    let mut sink = Vec::new();
                    let _ = stdout
                        .by_ref()
                        .take(ANTIGRAVITY_MAX_CAPTURE_BYTES)
                        .read_to_end(&mut sink);
                }
                if let Some(mut stderr) = child.stderr.take() {
                    let mut sink = Vec::new();
                    let _ = stderr
                        .by_ref()
                        .take(ANTIGRAVITY_MAX_CAPTURE_BYTES)
                        .read_to_end(&mut sink);
                }
                break Ok(());
            }
            Some(_) => {
                break Err(failure_message.into());
            }
            None => thread::sleep(Duration::from_millis(150)),
        }
    };
    if let Ok(mut active_child_pid) = runtime.active_child_pid.lock() {
        *active_child_pid = None;
    }
    result
}

fn run_setup_job(app: AppHandle, runtime: Arc<AntigravitySetupRuntime>, requested: Option<String>) {
    let result = (|| -> Result<(), String> {
        set_setup_view(
            &app,
            &runtime,
            "CHECKING_SYSTEM",
            true,
            Some("Checking Windows storage and the existing Antigravity installation.".into()),
            None,
        );
        if setup_cancelled(&runtime) {
            return Err("SETUP_CANCELLED".into());
        }
        let mut record = load_antigravity_setup_record(Some(&app));
        let (local_state, _local_root, local_cli) = local_appdata_install_state();
        set_setup_view(
            &app,
            &runtime,
            "CHECKING_EXISTING_INSTALL",
            true,
            Some("Checking for a compatible CLI without replacing existing files.".into()),
            None,
        );
        let configured_cli = record.cli_path.as_deref().map(PathBuf::from);
        let existing_cli = configured_cli
            .as_deref()
            .and_then(|path| detect_cli_at(Some(path)))
            .map(|report| {
                (
                    PathBuf::from(report.executable_path.clone().unwrap_or_default()),
                    report,
                )
            })
            .or_else(|| {
                local_cli
                    .as_deref()
                    .and_then(|path| detect_cli_at(Some(path)))
                    .map(|report| {
                        (
                            PathBuf::from(report.executable_path.clone().unwrap_or_default()),
                            report,
                        )
                    })
            })
            .or_else(|| {
                let report = relintor_antigravity::detect();
                report
                    .executable_path
                    .clone()
                    .map(|path| (PathBuf::from(path), report))
            });
        if let Some((cli_path, report)) = existing_cli {
            if report.sealed_execution_allowed() {
                record.cli_path = Some(cli_path.display().to_string());
                record.cli_version = report.version;
                record.installation_state = "existing_valid".into();
                persist_antigravity_setup_record(&app, &record)?;
                if record.authentication_state == "signed_in" {
                    let cli_version = record.cli_version.clone();
                    provision_antigravity_bridge(
                        &app,
                        &runtime,
                        &mut record,
                        &cli_path,
                        cli_version.as_deref(),
                    )?;
                    return Ok(());
                }
                set_setup_view(
                    &app,
                    &runtime,
                    "AUTH_REQUIRED",
                    false,
                    Some(
                        "The compatible Antigravity CLI is installed. Sign in to continue.".into(),
                    ),
                    None,
                );
                return Ok(());
            }
        }
        if local_state == "real_directory_conflict" {
            return Err("An existing %LOCALAPPDATA%\\agy folder is not a valid Antigravity CLI. Relintor will not replace it; choose an existing executable or a different location.".into());
        }
        let root = requested
            .as_deref()
            .map(PathBuf::from)
            .or_else(|| record.selected_install_root.as_deref().map(PathBuf::from))
            .or_else(preferred_antigravity_root)
            .ok_or_else(|| "Choose an installation location with at least 5 GB available. Relintor will not silently use a low-space system drive.".to_string())?;
        if !root.is_absolute() {
            return Err("Choose an absolute installation location.".into());
        }
        let root_drive = root
            .components()
            .next()
            .map(|component| {
                PathBuf::from(format!("{}\\", component.as_os_str().to_string_lossy()))
            })
            .ok_or_else(|| "The selected installation location has no drive root.".to_string())?;
        set_setup_view(
            &app,
            &runtime,
            "CHECKING_STORAGE",
            true,
            Some("Checking the selected drive before any download begins.".into()),
            None,
        );
        if !meets_storage_reserve(disk_free_bytes(&root_drive)) {
            return Err(
                "The selected drive does not have the required 5 GB safety reserve.".into(),
            );
        }
        if setup_cancelled(&runtime) {
            return Err("SETUP_CANCELLED".into());
        }
        set_setup_view(
            &app,
            &runtime,
            "PREPARING_STORAGE",
            true,
            Some(
                "Preparing a new bounded storage folder. Existing folders are never deleted."
                    .into(),
            ),
            None,
        );
        prepare_install_root(&root)?;
        record.selected_install_root = Some(root.display().to_string());
        ensure_local_appdata_junction(&root)?;
        persist_antigravity_setup_record(&app, &record)?;
        set_setup_view(
            &app,
            &runtime,
            "DOWNLOADING_INSTALLER",
            true,
            Some("Downloading only from the official Antigravity HTTPS installer.".into()),
            None,
        );
        let child = installer_command()
            .spawn()
            .map_err(|error| format!("start the official Antigravity installer: {error}"))?;
        set_setup_view(
            &app,
            &runtime,
            "INSTALLING",
            true,
            Some("Installing the official Antigravity CLI into the selected location.".into()),
            None,
        );
        run_setup_process(
            child,
            &runtime,
            ANTIGRAVITY_CHILD_TIMEOUT,
            "The official Antigravity installer did not complete successfully.",
        )?;
        let cli_path = find_cli_path(&root)
            .or_else(|| local_appdata_install_state().2)
            .ok_or_else(|| {
                "The installer finished, but the Antigravity CLI executable could not be verified."
                    .to_string()
            })?;
        set_setup_view(
            &app,
            &runtime,
            "VERIFYING_EXECUTABLE",
            true,
            Some("Verifying the installed executable before it can be used.".into()),
            None,
        );
        let report = detect_cli_at(Some(&cli_path)).ok_or_else(|| {
            "The installed Antigravity executable could not be invoked.".to_string()
        })?;
        set_setup_view(
            &app,
            &runtime,
            "CHECKING_VERSION",
            true,
            Some(
                "Checking the installed version against Relintor's compatibility registry.".into(),
            ),
            None,
        );
        if !report.sealed_execution_allowed() {
            return Err(
                "This Antigravity CLI version is not supported by this Relintor release.".into(),
            );
        }
        record.cli_path = Some(cli_path.display().to_string());
        record.cli_version = report.version;
        record.installation_state = "installed_verified".into();
        record.authentication_state = "sign_in_required".into();
        persist_antigravity_setup_record(&app, &record)?;
        set_setup_view(
            &app,
            &runtime,
            "AUTH_REQUIRED",
            false,
            Some("Antigravity is installed. Sign in to enable bounded execution.".into()),
            None,
        );
        Ok(())
    })();
    if let Err(error) = result {
        let cancelled = error == "SETUP_CANCELLED";
        let authentication_ready =
            load_antigravity_setup_record(Some(&app)).authentication_state == "signed_in";
        set_setup_view(
            &app,
            &runtime,
            if cancelled {
                "CANCELLED"
            } else if authentication_ready {
                "BRIDGE_FAILED"
            } else {
                "FAILED"
            },
            false,
            Some(if cancelled {
                "Setup was cancelled. No existing installation was changed.".into()
            } else if authentication_ready {
                "Antigravity is signed in, but Relintor could not prepare its verified connection."
                    .into()
            } else {
                "Relintor could not complete Antigravity setup. Review the storage and installation details, then retry.".into()
            }),
            if cancelled { None } else { Some(error) },
        );
    }
    runtime.cancel.store(false, Ordering::Release);
    runtime.operation_active.store(false, Ordering::Release);
    if let Ok(mut worker) = runtime.worker.lock() {
        *worker = None;
    }
}

#[tauri::command]
fn antigravity_setup_status(app: AppHandle) -> AntigravitySetupView {
    let runtime = antigravity_setup_runtime();
    runtime
        .state
        .lock()
        .ok()
        .and_then(|state| state.clone())
        .unwrap_or_else(|| setup_view(&app, None, false, None))
}

#[tauri::command]
fn antigravity_setup_start(
    app: AppHandle,
    requested_path: Option<String>,
) -> Result<AntigravitySetupView, String> {
    let runtime = antigravity_setup_runtime();
    if health_antigravity_for_app(Some(&app)).adapter_ready {
        return Ok(setup_view(&app, Some("READY"), false, None));
    }
    if runtime.operation_active.load(Ordering::Acquire) {
        return Ok(antigravity_setup_status(app));
    }
    if let Some(path) = requested_path.as_deref() {
        if !Path::new(path).is_absolute() {
            return Err("Choose an absolute installation location.".into());
        }
    }
    if runtime
        .operation_active
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return Ok(antigravity_setup_status(app));
    }
    runtime.cancel.store(false, Ordering::Release);
    set_setup_view(
        &app,
        &runtime,
        "CHECKING_SYSTEM",
        true,
        Some("Setup has started with your explicit consent.".into()),
        None,
    );
    let worker_runtime = runtime.clone();
    let worker_app = app.clone();
    let handle = thread::spawn(move || run_setup_job(worker_app, worker_runtime, requested_path));
    runtime
        .worker
        .lock()
        .map_err(|_| "Antigravity setup state is unavailable.".to_string())?
        .replace(handle);
    Ok(antigravity_setup_status(app))
}

#[tauri::command]
fn antigravity_setup_cancel() -> AntigravitySetupView {
    let runtime = antigravity_setup_runtime();
    runtime.cancel.store(true, Ordering::Release);
    invalidate_antigravity_readiness();
    if let Ok(mut state) = runtime.state.lock() {
        if let Some(view) = state.as_mut() {
            view.stage = "CANCELLED".into();
            view.active = false;
            view.can_cancel = false;
            view.can_retry = true;
            view.progress_indeterminate = false;
            view.detail = "Setup was cancelled. No existing installation was changed.".into();
        }
    }
    runtime
        .state
        .lock()
        .ok()
        .and_then(|state| state.clone())
        .unwrap_or(AntigravitySetupView {
            stage: "CANCELLED".into(),
            active: false,
            progress_percent: None,
            progress_indeterminate: false,
            storage_path: None,
            recommended_storage_path: None,
            storage_detail: "No setup was running.".into(),
            c: DiskSpaceView {
                available_bytes: None,
                safe_minimum_bytes: ANTIGRAVITY_MIN_WORKING_SPACE_BYTES,
            },
            d: DiskSpaceView {
                available_bytes: None,
                safe_minimum_bytes: ANTIGRAVITY_MIN_WORKING_SPACE_BYTES,
            },
            local_appdata_state: "unknown".into(),
            cli_path: None,
            cli_version: None,
            authentication_status: "not_available".into(),
            adapter_ready: false,
            consent_required: true,
            automatic_install_available: cfg!(windows),
            requires_location: true,
            can_cancel: false,
            can_retry: true,
            detail: "Setup was cancelled. No existing installation was changed.".into(),
            error: None,
            advanced_details: "stage=CANCELLED".into(),
        })
}

#[tauri::command]
async fn antigravity_choose_install_location(app: AppHandle) -> Result<Option<String>, String> {
    let selected = app
        .dialog()
        .file()
        .set_title("Choose Antigravity installation location")
        .blocking_pick_folder();
    Ok(selected.map(|path| path.to_string()))
}

#[tauri::command]
async fn antigravity_choose_existing_cli(app: AppHandle) -> Result<Option<String>, String> {
    let selected = app
        .dialog()
        .file()
        .set_title("Choose the Antigravity agy executable")
        .blocking_pick_file();
    Ok(selected.map(|path| path.to_string()))
}

#[tauri::command]
fn antigravity_use_existing_cli(
    app: AppHandle,
    path: String,
) -> Result<AntigravitySetupView, String> {
    let candidate = PathBuf::from(path);
    if !candidate.is_absolute() || !candidate.is_file() || is_reparse_point(&candidate) {
        return Err("Choose the real Antigravity agy executable, not a shortcut or link.".into());
    }
    let filename = candidate
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if filename != "agy.exe" && filename != "agy" {
        return Err("Choose the official Antigravity agy executable.".into());
    }
    let report = detect_cli_at(Some(&candidate)).ok_or_else(|| {
        "Relintor could not invoke the selected Antigravity executable.".to_string()
    })?;
    if !report.sealed_execution_allowed() {
        return Err(
            "This Antigravity CLI version is not supported by this Relintor release.".into(),
        );
    }
    let mut record = load_antigravity_setup_record(Some(&app));
    record.selected_install_root = candidate
        .parent()
        .map(|path| path.to_string_lossy().into_owned());
    record.cli_path = Some(candidate.to_string_lossy().into_owned());
    record.cli_version = report.version;
    record.installation_state = "existing_valid".into();
    record.authentication_state = "sign_in_required".into();
    persist_antigravity_setup_record(&app, &record)?;
    Ok(setup_view(&app, Some("AUTH_REQUIRED"), false, None))
}

#[tauri::command]
fn antigravity_sign_in(app: AppHandle) -> Result<AntigravitySetupView, String> {
    let runtime = antigravity_setup_runtime();
    if runtime.operation_active.load(Ordering::Acquire) {
        return Ok(antigravity_setup_status(app));
    }
    if runtime
        .operation_active
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return Ok(antigravity_setup_status(app));
    }
    let record = load_antigravity_setup_record(Some(&app));
    let cli = record
        .cli_path
        .as_deref()
        .map(PathBuf::from)
        .filter(|path| path.is_file())
        .or_else(|| local_appdata_install_state().2)
        .or_else(|| configured_antigravity_cli(&app))
        .ok_or_else(|| "Install the Antigravity CLI before signing in.".to_string());
    let cli = match cli {
        Ok(cli) => cli,
        Err(error) => {
            runtime.operation_active.store(false, Ordering::Release);
            return Err(error);
        }
    };
    let report = match detect_cli_at(Some(&cli)) {
        Some(report) => report,
        None => {
            runtime.operation_active.store(false, Ordering::Release);
            return Err("Relintor could not invoke the Antigravity CLI.".into());
        }
    };
    if !report.sealed_execution_allowed() {
        runtime.operation_active.store(false, Ordering::Release);
        return Err(
            "This Antigravity CLI version is not supported by this Relintor release.".into(),
        );
    }
    runtime.cancel.store(false, Ordering::Release);
    set_setup_view(
        &app,
        &runtime,
        "AUTH_STARTING",
        true,
        Some(
            "Starting Antigravity's official interactive sign-in window. No Relintor command is required."
                .into(),
        ),
        None,
    );
    let worker_runtime = runtime.clone();
    let worker_app = app.clone();
    let handle = thread::spawn(move || {
        let result = (|| -> Result<(), String> {
            let child = interactive_auth_command(&cli)
                .spawn()
                .map_err(|error| format!("start Antigravity sign-in: {error}"))?;
            set_setup_view(
                &worker_app,
                &worker_runtime,
                "AUTH_WAITING_FOR_USER",
                true,
                Some(
                    "The official Antigravity sign-in window is open. Complete sign-in there; do not enter commands."
                        .into(),
                ),
                None,
            );
            run_setup_process(
                child,
                &worker_runtime,
                ANTIGRAVITY_AUTH_TIMEOUT,
                "AUTH_FAILED: the official Antigravity sign-in window closed without completing.",
            )?;
            set_setup_view(
                &worker_app,
                &worker_runtime,
                "AUTH_VERIFYING",
                true,
                Some("Antigravity sign-in completed. Verifying the authenticated CLI before bridge setup.".into()),
                None,
            );
            let mut updated = load_antigravity_setup_record(Some(&worker_app));
            updated.authentication_state = "signed_in".into();
            updated.last_readiness_check_ms = execution_now_ms() as i128;
            persist_antigravity_setup_record(&worker_app, &updated)?;
            set_setup_view(
                &worker_app,
                &worker_runtime,
                "AUTH_READY",
                true,
                Some(
                    "Antigravity authentication is ready. Preparing Relintor's signed bridge."
                        .into(),
                ),
                None,
            );
            let cli_version = updated.cli_version.clone();
            if let Err(error) = provision_antigravity_bridge(
                &worker_app,
                &worker_runtime,
                &mut updated,
                &cli,
                cli_version.as_deref(),
            ) {
                set_setup_view(
                    &worker_app,
                    &worker_runtime,
                    "BRIDGE_FAILED",
                    false,
                    Some("Antigravity is signed in, but Relintor could not prepare its verified connection.".into()),
                    Some(error),
                );
            }
            Ok(())
        })();
        if let Err(error) = result {
            let cancelled = error == "SETUP_CANCELLED";
            let timed_out = error == "SETUP_TIMEOUT";
            let auth_failed = error.starts_with("AUTH_FAILED")
                || (!cancelled && !timed_out && !error.starts_with("Antigravity is signed in"));
            set_setup_view(
                &worker_app,
                &worker_runtime,
                if cancelled {
                    "AUTH_CANCELLED"
                } else if timed_out {
                    "AUTH_TIMED_OUT"
                } else if auth_failed {
                    "AUTH_FAILED"
                } else {
                    "BRIDGE_FAILED"
                },
                false,
                Some(if cancelled {
                    "Antigravity sign-in was cancelled. You can try again when ready.".into()
                } else if timed_out {
                    "Antigravity sign-in timed out before completing. You can try again.".into()
                } else if auth_failed {
                    "Relintor could not complete the official Antigravity sign-in flow. Try again."
                        .into()
                } else {
                    "Antigravity is signed in, but Relintor could not prepare its verified connection.".into()
                }),
                if cancelled { None } else { Some(error) },
            );
        }
        worker_runtime
            .operation_active
            .store(false, Ordering::Release);
        worker_runtime.cancel.store(false, Ordering::Release);
        if let Ok(mut worker) = worker_runtime.worker.lock() {
            *worker = None;
        }
    });
    runtime
        .worker
        .lock()
        .map_err(|_| "Antigravity setup state is unavailable.".to_string())?
        .replace(handle);
    Ok(antigravity_setup_status(app))
}

#[tauri::command]
fn health_all(app: AppHandle) -> DesktopHealth {
    DesktopHealth {
        application: application_health(),
        database: health_database(app.clone()),
        specification: health_specification(app.clone()),
        keychain: keychain_health(),
        antigravity: health_antigravity(app),
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
                    "User selected IÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â¢ÃƒÆ’Ã‚Â¢ÃƒÂ¢Ã¢â€šÂ¬Ã…Â¡Ãƒâ€šÃ‚Â¬ÃƒÆ’Ã‚Â¢ÃƒÂ¢Ã¢â€šÂ¬Ã…Â¾Ãƒâ€šÃ‚Â¢m not sure; Relintor applied its recommendation as an assumption.".into()
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
async fn choose_workspace(app: AppHandle) -> Result<Option<String>, String> {
    let selected = app
        .dialog()
        .file()
        .set_title("Choose a project workspace")
        .blocking_pick_folder();
    selected
        .map(|path| {
            path.into_path()
                .map(|value| value.to_string_lossy().into_owned())
                .map_err(|error| format!("resolve selected workspace: {error}"))
        })
        .transpose()
}

#[tauri::command]
fn investigator_takeover_project(
    app: AppHandle,
    project_id: String,
    goal: String,
) -> Result<InvestigationView, String> {
    let path = database_path(&app)?;
    migrate_database(&path)?;
    let connection =
        Connection::open(&path).map_err(|error| format!("open project database: {error}"))?;
    let canonical_name: String = connection
        .query_row(
            "SELECT name FROM projects WHERE id = ?1",
            params![project_id.as_str()],
            |row| row.get(0),
        )
        .map_err(|_| "the scanned project is no longer available; scan it again".to_string())?;
    let mut project = ProjectDraft::from_idea(&goal).map_err(|error| error.to_string())?;
    project.id = project_id;
    project.name = canonical_name;
    let result = Investigator::default().investigate(project, Vec::new(), &[])?;
    persist_result(&path, &result)?;
    Ok(InvestigationView::from(&result))
}

fn project_summaries(connection: &Connection) -> Result<Vec<ProjectSummaryView>, String> {
    let mut statement = connection
        .prepare(
            "SELECT p.id, p.name, p.root_path,
                    (SELECT fingerprint FROM project_takeovers
                     WHERE project_id = p.id ORDER BY created_at DESC LIMIT 1),
                    (SELECT status FROM investigations
                     WHERE project_id = p.id ORDER BY updated_at DESC LIMIT 1),
                    (SELECT MAX(revision) FROM mission_revisions
                     WHERE mission_id = 'mission-' || p.id),
                    EXISTS(SELECT 1 FROM authority_reviews WHERE project_id = p.id)
             FROM projects p
             ORDER BY p.created_at DESC, p.id",
        )
        .map_err(|error| format!("prepare project list: {error}"))?;
    let rows = statement
        .query_map([], |row| {
            let root_path: Option<String> = row.get(2)?;
            let takeover_fingerprint: Option<String> = row.get(3)?;
            let investigation_status: Option<String> = row.get(4)?;
            let sealed_revision: Option<u64> = row.get(5)?;
            let authority_reviewed: bool = row.get::<_, i64>(6)? != 0;
            let state = if sealed_revision.is_some_and(|revision| revision > 0) {
                "sealed"
            } else if root_path.is_none() && investigation_status.is_some() {
                "workspace_required"
            } else if authority_reviewed {
                "ready_to_seal"
            } else if investigation_status.is_some() {
                "investigation"
            } else if takeover_fingerprint.is_some() {
                "reality_report"
            } else {
                "project"
            };
            Ok(ProjectSummaryView {
                project_id: row.get(0)?,
                name: row.get(1)?,
                root_path,
                state: state.into(),
                investigation_status,
                takeover_fingerprint,
                sealed_revision,
            })
        })
        .map_err(|error| format!("read project list: {error}"))?;
    rows.map(|row| row.map_err(|error| format!("decode project list: {error}")))
        .collect()
}

fn open_project_at_path(path: &Path, project_id: &str) -> Result<ProjectOpenView, String> {
    let connection =
        Connection::open(path).map_err(|error| format!("open project database: {error}"))?;
    let project = project_summaries(&connection)?
        .into_iter()
        .find(|candidate| candidate.project_id == project_id)
        .ok_or_else(|| {
            "the selected project is no longer available; refresh Projects".to_string()
        })?;
    let investigation_json: Option<String> = connection
        .query_row(
            "SELECT investigation_json FROM project_workflow_state WHERE project_id = ?1",
            params![project_id],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()
        .map_err(|error| format!("read persisted project workflow: {error}"))?
        .flatten();
    let investigation = investigation_json
        .map(|value| {
            serde_json::from_str(&value)
                .map_err(|error| format!("decode persisted project workflow: {error}"))
        })
        .transpose()?;
    let authority_facts_json: Option<String> = connection
        .query_row(
            "SELECT facts_json FROM authority_reviews WHERE project_id = ?1",
            params![project_id],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|error| format!("read persisted authority review: {error}"))?;
    let authority_facts = authority_facts_json
        .map(|value| {
            serde_json::from_str(&value)
                .map_err(|error| format!("decode persisted authority review: {error}"))
        })
        .transpose()?
        .unwrap_or_default();
    let handoff = project
        .sealed_revision
        .filter(|revision| *revision > 0)
        .map(|revision| {
            let mission_id = format!("mission-{project_id}");
            let sealed = relintor_standards::load_mission_revision(path, &mission_id, revision)
                .map_err(|error| format!("load sealed project mission: {error}"))?;
            execution_handoff(&sealed)
        })
        .transpose()?;
    Ok(ProjectOpenView {
        project,
        investigation,
        authority_facts,
        handoff,
    })
}

#[tauri::command]
fn projects_list(app: AppHandle) -> Result<Vec<ProjectSummaryView>, String> {
    let path = database_path(&app)?;
    migrate_database(&path)?;
    let connection =
        Connection::open(&path).map_err(|error| format!("open project database: {error}"))?;
    project_summaries(&connection)
}

#[tauri::command]
fn project_open(app: AppHandle, project_id: String) -> Result<ProjectOpenView, String> {
    let path = database_path(&app)?;
    migrate_database(&path)?;
    open_project_at_path(&path, &project_id)
}

#[tauri::command]
fn set_project_workspace(
    app: AppHandle,
    project_id: String,
    root_path: String,
) -> Result<ProjectOpenView, String> {
    let requested = root_path.trim();
    if requested.is_empty() {
        return Err(
            "WORKSPACE_REQUIRED: choose an existing project folder before continuing".into(),
        );
    }
    let canonical_root = fs::canonicalize(requested)
        .map_err(|_| "WORKSPACE_INVALID: the selected project folder is unavailable".to_string())?;
    if !canonical_root.is_dir() {
        return Err("WORKSPACE_INVALID: choose an existing folder, not a file".into());
    }
    let path = database_path(&app)?;
    migrate_database(&path)?;
    let connection =
        Connection::open(&path).map_err(|error| format!("open project database: {error}"))?;
    let changed = connection
        .execute(
            "UPDATE projects SET root_path = ?1 WHERE id = ?2",
            params![
                canonical_root.to_string_lossy().as_ref(),
                project_id.as_str()
            ],
        )
        .map_err(|error| format!("persist project workspace: {error}"))?;
    if changed != 1 {
        return Err(
            "PROJECT_NOT_FOUND: the project is no longer available; refresh Projects".into(),
        );
    }
    drop(connection);
    open_project_at_path(&path, &project_id)
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
    let mut blockers = match engine.preseal(&draft, &trusted) {
        Ok(()) => Vec::new(),
        Err(error) => vec![error.to_string()],
    };
    if let Err(error) = project_execution_scope(&connection, project_id) {
        blockers.push(error);
    }
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

fn canonical_project_id(project_id: &str) -> Result<String, String> {
    let trimmed = project_id.trim();
    let canonical = trimmed.strip_prefix("mission-").unwrap_or(trimmed).trim();
    if canonical.is_empty() {
        return Err("PROJECT_ID_REQUIRED: a persisted project identity is required".into());
    }
    Ok(canonical.into())
}

fn canonical_mission_id(project_id: &str) -> String {
    format!("mission-{project_id}")
}

fn normalized_workspace_path(raw: &str) -> PathBuf {
    let trimmed = raw.trim().trim_matches('"');
    let path = PathBuf::from(trimmed);
    if path.is_dir() {
        return path;
    }

    #[cfg(windows)]
    {
        let candidates = [
            trimmed
                .strip_prefix("\\\\?\\UNC\\")
                .map(|rest| format!("\\\\{rest}")),
            trimmed
                .strip_prefix("\\\\?\\")
                .map(std::string::ToString::to_string),
        ];
        for candidate in candidates.into_iter().flatten() {
            let candidate = PathBuf::from(candidate);
            if candidate.is_dir() {
                return candidate;
            }
        }
    }

    path
}

fn project_execution_scope(connection: &Connection, project_id: &str) -> Result<PathBuf, String> {
    let project_id = canonical_project_id(project_id)?;
    let root: Option<Option<String>> = connection
        .query_row(
            "SELECT root_path FROM projects WHERE id = ?1",
            params![project_id.as_str()],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()
        .map_err(|_| {
            "PROJECT_EXECUTION_SCOPE_UNAVAILABLE: the project workspace could not be read"
                .to_string()
        })?;
    let Some(root) = root else {
        return Err(
            "PROJECT_EXECUTION_SCOPE_MISSING: the canonical project record could not be found"
                .into(),
        );
    };
    let Some(root) = root else {
        return Err(
            "PROJECT_EXECUTION_SCOPE_REQUIRED: choose a real project workspace before Seal & Build"
                .into(),
        );
    };
    let root = root.trim();
    if root.is_empty() {
        return Err(
            "PROJECT_EXECUTION_SCOPE_REQUIRED: choose a real project workspace before Seal & Build"
                .into(),
        );
    }
    let path = normalized_workspace_path(root);
    if !path.is_dir() {
        return Err(
            "PROJECT_EXECUTION_SCOPE_INVALID: the selected project workspace is unavailable".into(),
        );
    }
    Ok(path)
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

fn notify_user(app: &AppHandle, title: &str, body: &str) {
    if let Err(error) = app.notification().builder().title(title).body(body).show() {
        eprintln!("Relintor native notification was unavailable: {error}");
    }
}

static EXECUTION_MUTATION_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct ActiveExecution {
    cancel: Arc<AtomicU64>,
    process_started: AtomicBool,
    process_exited: AtomicBool,
    process: Mutex<Option<ProcessOwnershipRecord>>,
}

static ACTIVE_EXECUTIONS: OnceLock<Mutex<BTreeMap<String, Arc<ActiveExecution>>>> = OnceLock::new();

fn active_executions() -> &'static Mutex<BTreeMap<String, Arc<ActiveExecution>>> {
    ACTIVE_EXECUTIONS.get_or_init(|| Mutex::new(BTreeMap::new()))
}

fn active_execution(project_id: &str) -> Result<Option<Arc<ActiveExecution>>, String> {
    active_executions()
        .lock()
        .map_err(|_| "execution runtime state is poisoned".to_string())
        .map(|active| active.get(project_id).cloned())
}

fn remove_active_execution(project_id: &str) {
    if let Ok(mut active) = active_executions().lock() {
        active.remove(project_id);
    }
}

fn require_execution_not_active(project_id: &str) -> Result<(), String> {
    if active_execution(project_id)?.is_some() {
        Err("EXECUTION_RUNNING: this mission already owns an active task attempt".into())
    } else {
        Ok(())
    }
}

fn persist_execution_boundary(
    run: &ExecutionRun,
    ledger_path: &Path,
    recovery: &RecoveryStore,
    revision: &MissionRevision,
    detail: &str,
    kind: CheckpointKind,
    processes: Vec<ProcessOwnershipRecord>,
) -> Result<(), relintor_execution::ExecutionError> {
    run.persist_snapshot(ledger_path)?;
    RecoveryCoordinator::new(recovery.clone())
        .checkpoint_run(
            run,
            recovery_authority(run, revision),
            kind,
            &run.workspace,
            processes,
            Vec::new(),
            detail,
            execution_now_ms(),
        )
        .map_err(|error| relintor_execution::ExecutionError::Ledger(error.to_string()))?;
    Ok(())
}

fn process_ownership_record(
    run: &ExecutionRun,
    identity: &relintor_antigravity::ProcessIdentity,
) -> Result<ProcessOwnershipRecord, relintor_execution::ExecutionError> {
    let attempt = run
        .attempts
        .iter()
        .rev()
        .find(|attempt| attempt.state == relintor_execution::TaskAttemptState::Running)
        .ok_or_else(|| {
            relintor_execution::ExecutionError::Ledger(
                "executor started without a running task attempt".into(),
            )
        })?;
    let launched_at_ms = u64::try_from(identity.started_at_ms).map_err(|_| {
        relintor_execution::ExecutionError::Ledger(
            "executor start time is outside the supported range".into(),
        )
    })?;
    let executable_digest = fs::read(&identity.executable)
        .map(|bytes| sha256_hex(&bytes))
        .map_err(|error| {
            relintor_execution::ExecutionError::Ledger(format!(
                "hash owned Antigravity executable: {error}"
            ))
        })?;
    Ok(ProcessOwnershipRecord {
        process_id: identity.pid,
        process_start_time_ms: Some(launched_at_ms),
        executable_path: identity.executable.clone(),
        executable_digest,
        command_digest: identity.command_digest.clone(),
        run_id: run.run_id.clone(),
        task_id: Some(attempt.task_id.clone()),
        attempt_id: Some(attempt.attempt_id.clone()),
        lease_id: Some(attempt.lease_id.clone()),
        launched_at_ms,
        observation: ProcessObservation::OwnedProcessStillRunning,
    })
}

fn owned_process_records(
    control: &ActiveExecution,
    observation: ProcessObservation,
) -> Result<Vec<ProcessOwnershipRecord>, String> {
    let mut record = control
        .process
        .lock()
        .map_err(|_| "execution process ownership is poisoned".to_string())?
        .clone();
    if let Some(record) = record.as_mut() {
        record.observation = observation;
    }
    Ok(record.into_iter().collect())
}

fn run_execution_step_worker(
    app: AppHandle,
    project_id: String,
    cli_path: PathBuf,
    control: Arc<ActiveExecution>,
) {
    let result = (|| -> Result<(), String> {
        let (mut run, ledger_path, revision, handoff) = load_execution_run(&app, &project_id)?;
        let recovery = recovery_store(&app, &revision)?;
        let cancel = Arc::clone(&control.cancel);
        let mut adapter = relintor_antigravity::ProductionAdapter::with_cli_path_and_cancel(
            Some(cli_path),
            Some(cancel),
        );
        if control.cancel.load(Ordering::Acquire) != 0 {
            run.mark_stopped_incomplete(
                "execution cancelled before the owned executor started",
                execution_now_ms(),
            )
            .map_err(|error| error.to_string())?;
            persist_execution_boundary(
                &run,
                &ledger_path,
                &recovery,
                &revision,
                "execution cancelled before the owned executor started",
                CheckpointKind::EmergencyStop,
                Vec::new(),
            )
            .map_err(|error| error.to_string())?;
            return Ok(());
        }
        loop {
            if control.cancel.load(Ordering::Acquire) != 0 {
                run.mark_stopped_incomplete(
                    "execution cancelled at a controlled task boundary",
                    execution_now_ms(),
                )
                .map_err(|error| error.to_string())?;
                persist_execution_boundary(
                    &run,
                    &ledger_path,
                    &recovery,
                    &revision,
                    "execution cancelled before the next bounded task turn",
                    CheckpointKind::EmergencyStop,
                    owned_process_records(
                        &control,
                        ProcessObservation::ProcessCompletedResultAvailable,
                    )?,
                )
                .map_err(|error| error.to_string())?;
                notify_user(
                    &app,
                    "Relintor needs your attention",
                    "This task stopped at a protected boundary. Review recovery before retrying.",
                );
                break;
            }
            control.process_started.store(false, Ordering::Release);
            control.process_exited.store(false, Ordering::Release);
            let dispatch = run.dispatch_next_with_adapter_with_callbacks(
                &mut adapter,
                &revision,
                &handoff,
                execution_now_ms(),
                |snapshot, identity| {
                    let record = process_ownership_record(snapshot, identity)?;
                    control.process_started.store(true, Ordering::Release);
                    control
                        .process
                        .lock()
                        .map_err(|_| {
                            relintor_execution::ExecutionError::Ledger(
                                "execution process ownership is poisoned".into(),
                            )
                        })?
                        .replace(record.clone());
                    persist_execution_boundary(
                        snapshot,
                        &ledger_path,
                        &recovery,
                        &revision,
                        "task attempt started and executor process ownership persisted",
                        CheckpointKind::AfterTaskPersistence,
                        vec![record],
                    )
                },
                |snapshot| {
                    let processes = owned_process_records(
                        &control,
                        ProcessObservation::ProcessCompletedResultAvailable,
                    )
                    .map_err(relintor_execution::ExecutionError::Ledger)?;
                    persist_execution_boundary(
                        snapshot,
                        &ledger_path,
                        &recovery,
                        &revision,
                        "trusted executor completion receipt and exact post-workspace persisted before task promotion",
                        CheckpointKind::AfterAtomicAction,
                        processes,
                    )
                },
            );
            control.process_exited.store(true, Ordering::Release);
            if let Err(error) = dispatch {
                if run.state == relintor_execution::ExecutionRunState::Running {
                    run.mark_stopped_incomplete(
                        "execution stopped because its durable runtime boundary could not be maintained",
                        execution_now_ms(),
                    )
                    .map_err(|nested| nested.to_string())?;
                }
                let kind = if run.state == relintor_execution::ExecutionRunState::StoppedIncomplete
                {
                    CheckpointKind::EmergencyStop
                } else {
                    CheckpointKind::AfterAtomicAction
                };
                persist_execution_boundary(
                    &run,
                    &ledger_path,
                    &recovery,
                    &revision,
                    &format!("task execution ended at a durable boundary: {error}"),
                    kind,
                    owned_process_records(
                        &control,
                        ProcessObservation::ProcessCompletedResultAvailable,
                    )?,
                )
                .map_err(|error| error.to_string())?;
                break;
            }
            let kind = if run.state == relintor_execution::ExecutionRunState::StoppedIncomplete {
                CheckpointKind::EmergencyStop
            } else {
                CheckpointKind::AfterAtomicAction
            };
            persist_execution_boundary(
                &run,
                &ledger_path,
                &recovery,
                &revision,
                "task execution returned at a durable boundary",
                kind,
                owned_process_records(
                    &control,
                    ProcessObservation::ProcessCompletedResultAvailable,
                )?,
            )
            .map_err(|error| error.to_string())?;
            if run.state
                == relintor_execution::ExecutionRunState::ExecutionTasksFinishedAwaitingVerification
            {
                notify_user(
                    &app,
                    "Relintor execution finished",
                    "All authorized tasks finished. Evidence is ready for verification.",
                );
            } else if matches!(
                run.state,
                relintor_execution::ExecutionRunState::Stopped
                    | relintor_execution::ExecutionRunState::StoppedIncomplete
                    | relintor_execution::ExecutionRunState::SafeBoundaryReached
            ) {
                notify_user(
                    &app,
                    "Relintor execution stopped safely",
                    "The mission is preserved. Review the Activity screen for the next action.",
                );
            }
            if run.state != relintor_execution::ExecutionRunState::Ready {
                break;
            }
            control
                .process
                .lock()
                .map_err(|_| "execution process ownership is poisoned".to_string())?
                .take();
            if control.cancel.load(Ordering::Acquire) != 0 {
                continue;
            }
            // A failed/expired attempt may have already received explicit
            // bounded retry or progress-renewal authority. That is a different
            // boundary from moving to the next successful task, so do not
            // require a TASK_IMPLEMENTATION_FINISHED event before the fresh
            // lease is issued.
            if run.has_pending_retry() {
                continue;
            }
            match run.authorize_next_task_continuation(&revision, &handoff, execution_now_ms()) {
                Ok(Some(_next_task)) => {
                    persist_execution_boundary(
                        &run,
                        &ledger_path,
                        &recovery,
                        &revision,
                        "automatic continuation authorized at a verified task boundary",
                        CheckpointKind::AfterTaskPersistence,
                        Vec::new(),
                    )
                    .map_err(|error| error.to_string())?;
                }
                Ok(None) => break,
                Err(error) => {
                    run.state = relintor_execution::ExecutionRunState::RevalidationRequired;
                    run.last_error = Some(error.to_string());
                    persist_execution_boundary(
                        &run,
                        &ledger_path,
                        &recovery,
                        &revision,
                        "automatic continuation could not be re-authorized",
                        CheckpointKind::RestartRecovery,
                        Vec::new(),
                    )
                    .map_err(|error| error.to_string())?;
                    break;
                }
            }
        }
        Ok(())
    })();
    if let Err(error) = result {
        eprintln!("Relintor execution worker stopped at a bounded failure: {error}");
    }
    if let Ok(mut active) = active_executions().lock() {
        active.remove(&project_id);
    }
    // Normal healthy execution closes itself through evidence, verification,
    // bounded correction (when deterministic work is actually wrong), and a
    // completion certificate. The user should not have to babysit task-to-P8
    // transitions.
    if let Ok((run, _, _, _)) = load_execution_run(&app, &project_id) {
        if run.state
            == relintor_execution::ExecutionRunState::ExecutionTasksFinishedAwaitingVerification
        {
            if let Err(error) = automatic_verification_closure(&app, &project_id) {
                eprintln!("Relintor automatic verification closure stopped: {error}");
                notify_user(
                    &app,
                    "Relintor needs your attention",
                    "Implementation is preserved, but verification could not close automatically. Review verification before retrying.",
                );
            }
        }
    }
}

fn launch_execution_worker(
    app: AppHandle,
    project_id: String,
    cli_path: PathBuf,
) -> Result<Arc<ActiveExecution>, String> {
    require_execution_not_active(&project_id)?;
    let control = Arc::new(ActiveExecution {
        cancel: Arc::new(AtomicU64::new(0)),
        process_started: AtomicBool::new(false),
        process_exited: AtomicBool::new(false),
        process: Mutex::new(None),
    });
    active_executions()
        .lock()
        .map_err(|_| "execution runtime state is poisoned".to_string())?
        .insert(project_id.clone(), Arc::clone(&control));
    let worker_control = Arc::clone(&control);
    let worker_project_id = project_id.clone();
    let spawn_result = thread::Builder::new()
        .name(format!("relintor-execution-{}", &project_id))
        .spawn(move || {
            run_execution_step_worker(app, worker_project_id, cli_path, worker_control);
        });
    if let Err(error) = spawn_result {
        if let Ok(mut active) = active_executions().lock() {
            active.remove(&project_id);
        }
        return Err(format!("EXECUTOR_WORKER_SPAWN_FAILED: {error}"));
    }
    Ok(control)
}

fn launch_reattached_monitor(
    app: AppHandle,
    project_id: String,
    control: Arc<ActiveExecution>,
) -> Result<(), String> {
    thread::Builder::new()
        .name(format!("relintor-reattach-{}", &project_id))
        .spawn(move || loop {
            thread::sleep(Duration::from_secs(1));
            let Some(record) = control
                .process
                .lock()
                .ok()
                .and_then(|process| process.clone())
            else {
                control.process_exited.store(true, Ordering::Release);
                remove_active_execution(&project_id);
                break;
            };
            let observation = if control.cancel.load(Ordering::Acquire) != 0 {
                ConservativeProcessInspector
                    .terminate(&record)
                    .unwrap_or_else(|_| ConservativeProcessInspector.observe(&record))
            } else {
                ConservativeProcessInspector.observe(&record)
            };
            if observation == ProcessObservation::OwnedProcessStillRunning {
                continue;
            }
            control.process_exited.store(true, Ordering::Release);
            if let Ok((run, ledger_path, revision, _)) = load_execution_run(&app, &project_id) {
                let recovery = recovery_store(&app, &revision);
                if let Ok(recovery) = recovery {
                    let mut observed_record = record;
                    observed_record.observation = observation;
                    let _ = persist_execution_boundary(
                        &run,
                        &ledger_path,
                        &recovery,
                        &revision,
                        "reattached Antigravity process exited or could not be re-proven; completion was not inferred",
                        CheckpointKind::RestartRecovery,
                        vec![observed_record],
                    );
                }
            }
            remove_active_execution(&project_id);
            // The next authority read performs the durable RUNNING ->
            // interrupted transition from the just-recorded observation.
            let _ = load_execution_run(&app, &project_id);
            break;
        })
        .map(|_| ())
        .map_err(|error| format!("reattached executor monitor could not start: {error}"))
}

fn reconcile_persisted_running_execution(
    app: &AppHandle,
    project_id: &str,
    run: &mut ExecutionRun,
    ledger_path: &Path,
    revision: &MissionRevision,
    recovery: &RecoveryStore,
) -> Result<(), String> {
    // First recover the narrow crash window where Antigravity already exited
    // cleanly and Relintor durably persisted the exact completion receipt +
    // post-workspace inventory, but the desktop stopped before promoting the
    // task. This is stronger than process/file inference: the P7 receipt,
    // lease, packet, attempt, and current workspace must all still match.
    match run.recover_trusted_completion_after_restart(execution_now_ms()) {
        Ok(true) => {
            persist_execution_boundary(
                run,
                ledger_path,
                recovery,
                revision,
                "restart recovered a previously persisted trusted executor completion before task promotion",
                CheckpointKind::AfterAtomicAction,
                Vec::new(),
            )
            .map_err(|error| format!("persist recovered trusted completion: {error}"))?;
            return Ok(());
        }
        Ok(false) => {}
        Err(relintor_execution::ExecutionError::RevalidationRequired(reason)) => {
            run.state = relintor_execution::ExecutionRunState::RevalidationRequired;
            run.last_error = Some(reason.clone());
            persist_execution_boundary(
                run,
                ledger_path,
                recovery,
                revision,
                &format!("trusted completion recovery requires revalidation: {reason}"),
                CheckpointKind::RestartRecovery,
                Vec::new(),
            )
            .map_err(|error| format!("persist trusted completion revalidation: {error}"))?;
            return Ok(());
        }
        Err(error) => {
            return Err(format!("recover persisted trusted completion: {error}"));
        }
    }

    let latest = recovery
        .load_latest()
        .map_err(|error| format!("load persisted executor identity: {error}"))?;
    let mut records = latest
        .as_ref()
        .map(|checkpoint| checkpoint.content.processes.clone())
        .unwrap_or_default();
    let observations = records
        .iter()
        .map(|record| ConservativeProcessInspector.observe(record))
        .collect::<Vec<_>>();
    for (record, observation) in records.iter_mut().zip(&observations) {
        record.observation = *observation;
    }
    let exact_owned = records.len() == 1
        && observations.len() == 1
        && observations[0] == ProcessObservation::OwnedProcessStillRunning
        && records[0].binds_to_running_attempt(run);
    if exact_owned {
        let record = records[0].clone();
        let control = Arc::new(ActiveExecution {
            cancel: Arc::new(AtomicU64::new(0)),
            process_started: AtomicBool::new(true),
            process_exited: AtomicBool::new(false),
            process: Mutex::new(Some(record.clone())),
        });
        active_executions()
            .lock()
            .map_err(|_| "execution runtime state is poisoned".to_string())?
            .insert(project_id.to_string(), Arc::clone(&control));
        if let Err(error) = persist_execution_boundary(
            run,
            ledger_path,
            recovery,
            revision,
            "desktop restarted and reattached the exact owned Antigravity process",
            CheckpointKind::RestartRecovery,
            vec![record],
        ) {
            remove_active_execution(project_id);
            return Err(format!("persist restart process reconciliation: {error}"));
        }
        if let Err(error) = launch_reattached_monitor(app.clone(), project_id.to_string(), control)
        {
            remove_active_execution(project_id);
            let reason = format!(
                "Relintor could not safely reattach monitoring after restart; the attempt was marked interrupted and requires recovery: {error}"
            );
            run.mark_stopped_incomplete(&reason, execution_now_ms())
                .map_err(|nested| nested.to_string())?;
            persist_execution_boundary(
                run,
                ledger_path,
                recovery,
                revision,
                &reason,
                CheckpointKind::EmergencyStop,
                records,
            )
            .map_err(|nested| nested.to_string())?;
        }
        return Ok(());
    }

    let reason = if records.is_empty() {
        "Relintor could not find a durable process identity for the previous executor after restart; the attempt was marked interrupted and requires recovery".to_string()
    } else if observations.contains(&ProcessObservation::ProcessStateUnknown) {
        "Relintor could not prove that the previous Antigravity executor is still the same owned process after restart; the attempt was marked interrupted and requires recovery".to_string()
    } else if observations.contains(&ProcessObservation::PidReusedNotOurs) {
        "The previous executor PID no longer identifies the owned Antigravity process; the attempt was marked interrupted and requires recovery".to_string()
    } else {
        "The previous Antigravity executor exited while Relintor was closed without a trusted completion record; the attempt was marked interrupted and requires recovery".to_string()
    };
    run.mark_stopped_incomplete(&reason, execution_now_ms())
        .map_err(|error| error.to_string())?;
    persist_execution_boundary(
        run,
        ledger_path,
        recovery,
        revision,
        &reason,
        CheckpointKind::EmergencyStop,
        records,
    )
    .map_err(|error| format!("persist restart interruption reconciliation: {error}"))
}

fn persist_executor_launch_failure(
    app: &AppHandle,
    project_id: &str,
    run: ExecutionRun,
    ledger_path: &Path,
    revision: &MissionRevision,
    action: &str,
    error: String,
) -> Result<ExecutionStatusView, String> {
    let detail = format!("Antigravity could not start {action}: {error}");
    let mut run = run;
    run.state = relintor_execution::ExecutionRunState::BlockedExternal;
    run.last_error = Some(detail.clone());
    let recovery = recovery_store(app, revision)?;
    persist_execution_boundary(
        &run,
        ledger_path,
        &recovery,
        revision,
        &detail,
        CheckpointKind::AfterAtomicAction,
        Vec::new(),
    )
    .map_err(|error| error.to_string())?;
    p9_status_view(app, project_id, ledger_path, &run, revision)
}

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
    let project_id = canonical_project_id(project_id)?;
    let (root, revision_number, fingerprint): (PathBuf, u64, Option<String>) = {
        let connection =
            Connection::open(path).map_err(|error| format!("open execution database: {error}"))?;
        let root = project_execution_scope(&connection, &project_id)?;
        let mission_id = canonical_mission_id(&project_id);
        let revision: u64 = connection
            .query_row(
                "SELECT COALESCE(MAX(revision), 0) FROM mission_revisions WHERE mission_id = ?1",
                params![mission_id],
                |row| row.get(0),
            )
            .map_err(|_| {
                "load mission revision: the sealed mission state could not be read".to_string()
            })?;
        let fingerprint = connection
            .query_row(
                "SELECT fingerprint FROM project_takeovers WHERE project_id = ?1 ORDER BY created_at DESC LIMIT 1",
                params![project_id.as_str()],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| format!("load workspace fingerprint: {error}"))?;
        (root, revision, fingerprint)
    };
    if revision_number == 0 {
        return Err("P7 execution requires a sealed mission revision".into());
    }
    let mission_id = canonical_mission_id(&project_id);
    let revision = relintor_standards::load_mission_revision(path, &mission_id, revision_number)
        .map_err(|error| format!("load sealed mission revision: {error}"))?;
    let handoff = execution_handoff(&revision)?;
    let (registry, trusted) = production_registry().map_err(|error| error.to_string())?;
    let workspace_fingerprint =
        fingerprint.unwrap_or_else(|| sha256_hex(root.to_string_lossy().as_bytes()));
    Ok((
        revision,
        handoff,
        registry,
        trusted,
        root,
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
    let current_task_id = active_task
        .as_ref()
        .cloned()
        .or_else(|| run.runnable_tasks().into_iter().next());
    let current_task_objective = current_task_id
        .as_ref()
        .and_then(|task_id| run.tasks.get(task_id))
        .map(|task| task.objective.clone());
    ExecutionStatusView {
        project_id: project_id.into(),
        project_name: project_id.into(),
        mission_id: run.mission_id.clone(),
        revision: run.mission_revision,
        state: format!("{:?}", run.state),
        watchdog_state: format!("{:?}", run.watchdog_state),
        current_turn: run.current_turn,
        active_task,
        current_task_objective,
        recovery_task_id: None,
        recovery_task_objective: None,
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
        recovery_action: "NONE".into(),
        dispatch_active: false,
        execution_phase: "READY".into(),
        execution_time_limit_ms: run.policy.execution_time_policy.task_wall_clock_ms,
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

fn revalidation_record_is_current(
    record: &relintor_execution::RevalidationRecord,
    checkpoint: Option<&relintor_execution::CheckpointRecord>,
) -> bool {
    checkpoint.is_none_or(|checkpoint| {
        record.created_at_ms > checkpoint.content.created_at_ms
            || (record.created_at_ms == checkpoint.content.created_at_ms
                && record.checkpoint_id.as_deref() == Some(checkpoint.checkpoint_id.as_str()))
    })
}

fn execution_phase_for(
    state: &relintor_execution::ExecutionRunState,
    continuation_authorized: bool,
    active: bool,
    process_started: bool,
    process_exited: bool,
) -> &'static str {
    if active {
        if process_exited {
            return match state {
                relintor_execution::ExecutionRunState::Stopped
                | relintor_execution::ExecutionRunState::StoppedIncomplete
                | relintor_execution::ExecutionRunState::SafeBoundaryReached => "STOPPED",
                relintor_execution::ExecutionRunState::Failed
                | relintor_execution::ExecutionRunState::BlockedExternal
                | relintor_execution::ExecutionRunState::RevalidationRequired => "FAILED",
                _ => "EXECUTOR_EXITED",
            };
        }
        if process_started && *state == relintor_execution::ExecutionRunState::Running {
            return "RUNNING";
        }
        if process_started {
            return "EXECUTOR_PROCESS_STARTED";
        }
        return "CONTINUATION_DISPATCHING";
    }
    match state {
        relintor_execution::ExecutionRunState::ExecutionTasksFinishedAwaitingVerification => {
            "EXECUTOR_EXITED"
        }
        relintor_execution::ExecutionRunState::Stopped
        | relintor_execution::ExecutionRunState::StoppedIncomplete
        | relintor_execution::ExecutionRunState::SafeBoundaryReached => "STOPPED",
        relintor_execution::ExecutionRunState::Failed
        | relintor_execution::ExecutionRunState::BlockedExternal
        | relintor_execution::ExecutionRunState::RevalidationRequired => "FAILED",
        relintor_execution::ExecutionRunState::Ready if continuation_authorized => {
            "CONTINUATION_AUTHORIZED"
        }
        relintor_execution::ExecutionRunState::Ready => "READY",
        relintor_execution::ExecutionRunState::Running => "STOPPED",
        _ => "READY",
    }
}

fn p9_status_view(
    app: &AppHandle,
    project_id: &str,
    ledger_path: &Path,
    run: &ExecutionRun,
    revision: &MissionRevision,
) -> Result<ExecutionStatusView, String> {
    let mut view = execution_status_view_base(project_id, ledger_path, run);
    let database = database_path(app)?;
    let connection = Connection::open(database)
        .map_err(|error| format!("open project display record: {error}"))?;
    view.project_name = connection
        .query_row(
            "SELECT name FROM projects WHERE id = ?1",
            params![project_id],
            |row| row.get(0),
        )
        .map_err(|error| format!("read project display name: {error}"))?;
    let active = active_execution(project_id)?;
    view.dispatch_active = active.is_some();
    let (process_started, process_exited) = active
        .as_ref()
        .map(|control| {
            (
                control.process_started.load(Ordering::Acquire),
                control.process_exited.load(Ordering::Acquire),
            )
        })
        .unwrap_or((false, false));
    let continuation_authorized = run.events.last().is_some_and(|event| {
        matches!(
            event.kind,
            relintor_execution::ExecutionEventKind::ContinuationStarted
                | relintor_execution::ExecutionEventKind::ContinuationAuthorized
        )
    });
    view.execution_phase = execution_phase_for(
        &run.state,
        continuation_authorized,
        active.is_some(),
        process_started,
        process_exited,
    )
    .into();
    let store = recovery_store(app, revision)?;
    let coordinator = RecoveryCoordinator::new(store.clone());
    let latest = store
        .load_latest()
        .map_err(|error| format!("load P9 checkpoint: {error}"))?;
    let latest_revalidation = store
        .load_latest_revalidation()
        .map_err(|error| format!("load P9 revalidation decision: {error}"))?;
    let expected = expected_recovery_authority(&store, run, revision)?;
    if !run.recovery_status_requires_attention() {
        if let Some(record) = latest.as_ref().filter(|record| record.is_safe_to_resume()) {
            view.last_safe_checkpoint =
                Some(format!("{}#{}", record.checkpoint_id, record.sequence));
        }
        view.recovery_state = "NO_RECOVERY_REQUIRED".into();
        view.resume_disposition = None;
        view.resume_blocker = None;
        view.external_changes.clear();
        view.recovery_detected = false;
        view.recovery_action = "NONE".into();
        return Ok(view);
    }
    let integrity = coordinator
        .resume_integrity_for_run(
            run,
            &expected,
            &run.workspace,
            &ConservativeProcessInspector,
            execution_now_ms(),
        )
        .map_err(|error| format!("evaluate P9 resume integrity: {error}"))?;
    view.recovery_task_id = integrity
        .target
        .as_ref()
        .map(|target| target.task_id.clone());
    view.recovery_task_objective = integrity
        .target
        .as_ref()
        .and_then(|target| run.tasks.get(&target.task_id))
        .map(|task| task.objective.clone());
    let revalidation_is_current = latest_revalidation.as_ref().is_some_and(|record| {
        record.project_id == expected.project_id
            && record.mission_id == expected.mission_id
            && record.mission_revision == expected.mission_revision
            && record.p7_run_id == expected.p7_run_id
            && record.target == integrity.target
            && !(integrity.disposition == RecoveryDisposition::PreExecutionRetryAuthorized
                && record.disposition != RecoveryDisposition::PreExecutionRetryAuthorized)
            && revalidation_record_is_current(record, latest.as_ref())
    });
    view.recovery_action = if revalidation_is_current
        && integrity.target.as_ref().is_some_and(|target| {
            target.execution_boundary
                == relintor_execution::AttemptExecutionBoundary::ExternalProcessStarted
        })
        && !integrity.process_observations.is_empty()
        && integrity
            .process_observations
            .iter()
            .all(|observation| *observation == relintor_execution::ProcessObservation::ProcessGone)
    {
        "MANUAL_REVIEW_RETRY".into()
    } else {
        "CHECK_SAFETY".into()
    };
    if let Some(record) = latest_revalidation.filter(|_| revalidation_is_current) {
        view.recovery_state = format!("{:?}", record.disposition);
        view.resume_disposition = Some(format!("{:?}", record.disposition));
        view.resume_blocker = (record.disposition
            != RecoveryDisposition::PreExecutionRetryAuthorized)
            .then(|| record.reasons.first().cloned())
            .flatten();
        view.external_changes = record.affected_paths;
        view.recovery_detected = true;
    } else if let Some(record) = latest {
        if record.is_safe_to_resume() {
            view.last_safe_checkpoint =
                Some(format!("{}#{}", record.checkpoint_id, record.sequence));
        }
        view.recovery_state = format!("{:?}", integrity.disposition);
        view.resume_disposition = Some(format!("{:?}", integrity.disposition));
        view.resume_blocker = integrity.reasons.first().cloned();
        view.external_changes = integrity.changed_paths;
        view.recovery_detected = !integrity.reasons.is_empty();
    } else {
        view.recovery_state = format!("{:?}", integrity.disposition);
        view.resume_disposition = Some(format!("{:?}", integrity.disposition));
        view.resume_blocker = (integrity.disposition
            != RecoveryDisposition::PreExecutionRetryAuthorized)
            .then(|| integrity.reasons.first().cloned())
            .flatten();
        view.external_changes = integrity.changed_paths;
        view.recovery_detected = !integrity.reasons.is_empty();
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
        Some(
            ExecutionRun::restore_snapshot(&ledger_path)
                .map_err(|error| format!("restore P7 execution ledger: {error}"))?,
        )
    } else {
        None
    };
    let recovery_run = recovery
        .load_run()
        .map_err(|error| format!("restore P9 recovery checkpoint: {error}"))?;
    let mut run = match (ledger_run, recovery_run) {
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
    if run.state == relintor_execution::ExecutionRunState::Running
        && active_execution(project_id)?.is_none()
    {
        reconcile_persisted_running_execution(
            app,
            project_id,
            &mut run,
            &ledger_path,
            &revision,
            &recovery,
        )?;
    }
    Ok((run, ledger_path, revision, handoff))
}

fn authority_read_failure(error: String) -> String {
    let normalized = error.to_ascii_lowercase();
    if normalized.contains("unsupported execution ledger record version")
        || normalized.contains("integrity version is unsupported")
        || normalized.contains("checkpoint index version/sequence is invalid")
    {
        return "AUTHORITY_READ_FORMAT_UNSUPPORTED: the sealed mission is readable, but one persisted execution record uses an unsupported format; no history was discarded".into();
    }
    if normalized.contains("integrity verification failed")
        || normalized.contains("authentication failed")
        || normalized.contains("checkpoint authentication")
        || normalized.contains("integrity proof")
    {
        return "AUTHORITY_READ_INTEGRITY_FAILURE: the sealed mission was found, but an authenticated execution or recovery record failed verification; history was preserved and execution remains blocked".into();
    }
    if normalized.contains("task usage invariant failed")
        || normalized.contains("budget exhausted")
        || normalized.contains("historical budget boundary")
    {
        return "AUTHORITY_READ_RECOVERY_REQUIRED: the sealed mission is readable and retains its historical execution failure, but recovery is required before another attempt".into();
    }
    if normalized.contains("recovery record corrupt") {
        return "AUTHORITY_READ_RECOVERY_RECORD_INVALID: the sealed mission is readable, but its recovery record is structurally invalid; execution remains blocked".into();
    }
    if normalized.contains("query returned no rows")
        || normalized.contains("load sealed mission revision")
        || normalized.contains("load current sealed mission")
    {
        return "AUTHORITY_READ_SEALED_MISSION_UNAVAILABLE: the sealed mission revision could not be read from the local authority; restart Relintor and retry the authority read".into();
    }
    if error.contains("PROJECT_EXECUTION_SCOPE") {
        return error;
    }
    "AUTHORITY_READ_FAILED: the sealed mission authority could not be read; restart Relintor and retry the authority read".into()
}

struct P8VerificationContext {
    authority: VerificationAuthority,
    current: FreshnessContext,
    store: EvidenceStore,
    p7_execution: AuthenticatedP7Execution,
    local_key: Vec<u8>,
}

fn ensure_terminal_p7_workspace_matches(
    current_workspace_fingerprint: &str,
    terminal_p7_workspace_fingerprint: &str,
) -> Result<(), String> {
    if current_workspace_fingerprint != terminal_p7_workspace_fingerprint {
        return Err(
            "P8_REVALIDATION_REQUIRED: the workspace changed after the authenticated terminal P7 execution boundary; new evidence cannot be attributed to that completed run"
                .into(),
        );
    }
    Ok(())
}

fn load_p8_verification_context(
    app: &AppHandle,
    project_id: &str,
) -> Result<P8VerificationContext, String> {
    let path = database_path(app)?;
    migrate_database(&path)?;
    let (revision, handoff, registry, trusted, workspace, _takeover_workspace_fingerprint) =
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
    // P8 verifies the exact terminal workspace produced by the authenticated P7 run.
    // The takeover fingerprint describes the source state before execution and must
    // never be substituted for the post-execution authority boundary.
    let verification_workspace_fingerprint = p7_execution.run.workspace_fingerprint.clone();
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
        workspace_fingerprint: verification_workspace_fingerprint.clone(),
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
    ensure_terminal_p7_workspace_matches(
        &current_workspace_fingerprint,
        &verification_workspace_fingerprint,
    )?;
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
    collect_machine_evidence: bool,
) -> Result<
    (
        P8VerificationContext,
        relintor_evidence::VerificationReport,
        EvidenceManifest,
        CollectorOrchestrationResult,
    ),
    String,
> {
    let context = load_p8_verification_context(app, project_id)?;
    let collection = if collect_machine_evidence {
        let workspace = context
            .current
            .workspace_root
            .clone()
            .ok_or_else(|| "verification workspace is unavailable".to_string())?;
        VerificationCollectorOrchestrator::new(workspace)
            .run_required_collectors(
                &context.authority,
                &context.current,
                &context.store,
                &context.p7_execution,
            )
            .map_err(|error| format!("collect P8 verification evidence: {error}"))?
    } else {
        CollectorOrchestrationResult::default()
    };
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
    let current_evidence_ids = initial_report
        .requirement_statuses
        .iter()
        .flat_map(|status| status.evidence_ids.iter().cloned())
        .collect::<BTreeSet<_>>();
    let evidence = context
        .store
        .list()
        .map_err(|error| format!("load P8 evidence for AI review: {error}"))?
        .into_iter()
        .filter(|artifact| current_evidence_ids.contains(&artifact.metadata.evidence_id))
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
    Ok((context, report, manifest, collection))
}

fn human_decision_prompts(
    context: &P8VerificationContext,
    report: &relintor_evidence::VerificationReport,
) -> Vec<HumanDecisionPromptView> {
    report
        .requirement_statuses
        .iter()
        .filter(|status| {
            status
                .missing_obligations
                .contains(&EvidenceClass::HumanDecision)
        })
        .filter_map(|status| {
            context
                .authority
                .revision
                .contract
                .requirement_graph
                .requirements
                .iter()
                .find(|requirement| requirement.requirement_id == status.requirement_id)
        })
        .map(|requirement| HumanDecisionPromptView {
            requirement_id: requirement.requirement_id.clone(),
            title: requirement.title.clone(),
            question: format!(
                "Does the completed result satisfy the approved {} for this mission?",
                requirement.title.to_lowercase()
            ),
            summary: requirement.intent.clone(),
            criterion_ids: requirement
                .acceptance_criteria
                .iter()
                .filter(|criterion| !criterion.machine_checkable)
                .map(|criterion| criterion.criterion_id.clone())
                .collect(),
        })
        .collect()
}

fn verification_view(
    project_id: &str,
    context: &P8VerificationContext,
    report: &relintor_evidence::VerificationReport,
    manifest: &EvidenceManifest,
    certificate: Option<&CompletionCertificate>,
    collection: &CollectorOrchestrationResult,
    workflow_stage: Option<&str>,
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
    let human_decisions = human_decision_prompts(context, report);
    let failed_count = report
        .requirement_statuses
        .iter()
        .filter(|status| status.status == RequirementStatus::Failed)
        .count();
    let user_rejected = report.requirement_statuses.iter().any(|status| {
        status.failed_evidence.iter().any(|evidence_id| {
            context
                .store
                .load(evidence_id)
                .is_ok_and(|stored| stored.artifact.metadata.class == EvidenceClass::HumanDecision)
        })
    });
    let collection_failures = collection
        .blocked_external
        .iter()
        .map(|item| {
            if item.contains("TestOutput") {
                "The project test command could not be completed.".to_string()
            } else if item.contains("AccessibilityResult") {
                "The accessibility check could not be completed.".to_string()
            } else if item.contains("PerformanceResult") {
                "The performance check could not be completed.".to_string()
            } else if item.contains("SecurityScan") {
                "The security check could not be completed.".to_string()
            } else if item.contains("HumanDecision") {
                "Relintor is waiting for your decision.".to_string()
            } else {
                "A required automated verification check could not be completed.".to_string()
            }
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let stage = workflow_stage.unwrap_or_else(|| {
        if certificate.is_some() {
            "VERIFIED_COMPLETE"
        } else if !human_decisions.is_empty() && !manifest.evidence.is_empty() {
            "WAITING_FOR_USER_DECISION"
        } else if manifest.evidence.is_empty() {
            "READY_TO_VERIFY"
        } else if report.decision.state == relintor_evidence::CompletionState::BlockedExternal {
            "COLLECTION_BLOCKED"
        } else if user_rejected {
            "USER_DECISION_REJECTED"
        } else if failed_count > 0 {
            "VERIFICATION_NEEDS_ATTENTION"
        } else {
            "VERIFICATION_FINISHED"
        }
    });
    let summary = match stage {
        "VERIFIED_COMPLETE" => "All required evidence passed and the completion certificate is valid.",
        "WAITING_FOR_USER_DECISION" => {
            "Automated checks are complete. Relintor needs your decision before verification can continue."
        }
        "CORRECTING_FAILED_REQUIREMENT" => {
            "A real automated check failed. Antigravity is correcting only the affected work before Relintor verifies again."
        }
        "COLLECTION_BLOCKED" => {
            "Verification could not collect every required automated result. The mission remains safely incomplete."
        }
        "USER_DECISION_REJECTED" => {
            "You rejected the completed result. Relintor preserved your decision and did not start an automatic correction or issue a certificate."
        }
        _ if failed_count > 0 => {
            "One or more automated checks failed. Relintor has not called the mission complete."
        }
        _ => "Verification finished. Relintor is waiting for all required evidence.",
    };
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
            .chain(collection.blocked_external.iter().cloned())
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
        workflow_stage: stage.into(),
        summary: summary.into(),
        human_decisions,
        collector_activity: collection
            .executed
            .iter()
            .chain(collection.reused_fresh.iter())
            .cloned()
            .collect(),
        collection_failures,
        detail: report.decision.reason.clone(),
    }
}

fn completion_certificate_path(
    app: &AppHandle,
    context: &P8VerificationContext,
) -> Result<PathBuf, String> {
    let app_data = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("resolve certificate storage: {error}"))?;
    Ok(app_data.join("verification").join(format!(
        "{}-{}/certificate.json",
        context.authority.revision.seal.mission_id, context.authority.revision.revision
    )))
}

fn load_persisted_certificate(
    app: &AppHandle,
    context: &P8VerificationContext,
    manifest: &EvidenceManifest,
) -> Result<Option<CompletionCertificate>, String> {
    let path = completion_certificate_path(app, context)?;
    if !path.is_file() {
        return Ok(None);
    }
    let certificate = read_completion_certificate_file(&path)?;
    let authority = CompletionAuthority::new(&context.local_key)
        .map_err(|error| format!("open P8 completion authority: {error}"))?;
    authority
        .validate_with_store(
            &certificate,
            &context.authority,
            &context.store,
            manifest,
            &context.current,
        )
        .map_err(|error| format!("validate persisted completion certificate: {error}"))?;
    Ok(Some(certificate))
}

fn read_completion_certificate_file(path: &Path) -> Result<CompletionCertificate, String> {
    let bytes = fs::read(path).map_err(|error| format!("read completion certificate: {error}"))?;
    serde_json::from_slice(&bytes).map_err(|error| format!("parse completion certificate: {error}"))
}

fn persist_completion_certificate_file(
    path: &Path,
    certificate: &CompletionCertificate,
) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "certificate storage has no parent directory".to_string())?;
    fs::create_dir_all(parent).map_err(|error| format!("prepare certificate storage: {error}"))?;
    let temporary = parent.join(format!(".certificate-{}.tmp", std::process::id()));
    let bytes = serde_json::to_vec_pretty(certificate)
        .map_err(|error| format!("encode completion certificate: {error}"))?;
    let _ = fs::remove_file(&temporary);
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary)
        .map_err(|error| format!("create completion certificate: {error}"))?;
    file.write_all(&bytes)
        .map_err(|error| format!("write completion certificate: {error}"))?;
    file.sync_all()
        .map_err(|error| format!("sync completion certificate: {error}"))?;
    drop(file);
    atomic_replace_file(&temporary, path)
        .map_err(|error| format!("commit completion certificate: {error}"))
}

fn persist_completion_certificate(
    app: &AppHandle,
    context: &P8VerificationContext,
    certificate: &CompletionCertificate,
) -> Result<(), String> {
    let path = completion_certificate_path(app, context)?;
    persist_completion_certificate_file(&path, certificate)
}

fn ensure_verified_completion_certificate(
    app: &AppHandle,
    context: &P8VerificationContext,
    report: &relintor_evidence::VerificationReport,
    manifest: &EvidenceManifest,
) -> Result<Option<CompletionCertificate>, String> {
    if report.decision.state != relintor_evidence::CompletionState::VerifiedComplete {
        return Ok(None);
    }
    if let Some(existing) = load_persisted_certificate(app, context, manifest)? {
        return Ok(Some(existing));
    }
    let authority = CompletionAuthority::new(&context.local_key)
        .map_err(|error| format!("open P8 completion authority: {error}"))?;
    let certificate = authority
        .issue_with_p7_execution(report, &context.authority, &context.p7_execution)
        .map_err(|error| format!("issue completion certificate: {error}"))?;
    authority
        .validate_with_store(
            &certificate,
            &context.authority,
            &context.store,
            manifest,
            &context.current,
        )
        .map_err(|error| format!("validate completion certificate: {error}"))?;
    persist_completion_certificate(app, context, &certificate)?;
    notify_user(
        app,
        "Relintor verified complete",
        "All sealed requirements are verified and the completion certificate is valid.",
    );
    Ok(Some(certificate))
}

fn deterministic_failed_requirement_ids(
    report: &relintor_evidence::VerificationReport,
    store: &EvidenceStore,
) -> BTreeSet<String> {
    report
        .requirement_statuses
        .iter()
        .filter(|status| {
            status.status == RequirementStatus::Failed
                && status.failed_evidence.iter().any(|evidence_id| {
                    store.load(evidence_id).is_ok_and(|stored| {
                        !matches!(
                            stored.artifact.metadata.class,
                            EvidenceClass::HumanDecision
                                | EvidenceClass::AiVerifierJudgement
                                | EvidenceClass::ExternalServiceReceipt
                        )
                    })
                })
        })
        .map(|status| status.requirement_id.clone())
        .collect()
}

fn authorize_and_launch_verification_correction(
    app: &AppHandle,
    project_id: &str,
    report: &relintor_evidence::VerificationReport,
    store: &EvidenceStore,
) -> Result<bool, String> {
    let failed = deterministic_failed_requirement_ids(report, store);
    if failed.is_empty() {
        return Ok(false);
    }
    let project_id = canonical_project_id(project_id)?;
    with_execution_mutation_lock(|| {
        require_execution_not_active(&project_id)?;
        let (mut run, ledger_path, revision, _handoff) = load_execution_run(app, &project_id)?;
        let affected = run
            .authorize_verification_correction(&failed, execution_now_ms())
            .map_err(|error| error.to_string())?;
        let recovery = recovery_store(app, &revision)?;
        persist_execution_boundary(
            &run,
            &ledger_path,
            &recovery,
            &revision,
            &format!(
                "deterministic verification failed; bounded corrective execution authorized for {}",
                affected.join(",")
            ),
            CheckpointKind::AfterTaskPersistence,
            Vec::new(),
        )
        .map_err(|error| format!("persist verification correction authority: {error}"))?;

        let readiness = health_antigravity_for_app(Some(app));
        if !readiness.adapter_ready {
            return Err(format!("ANTIGRAVITY_SETUP_REQUIRED: {}", readiness.detail));
        }
        let cli_path = configured_antigravity_cli(app).ok_or_else(|| {
            "ANTIGRAVITY_SETUP_REQUIRED: the verified Antigravity CLI path is unavailable"
                .to_string()
        })?;
        launch_execution_worker(app.clone(), project_id.clone(), cli_path)
            .map_err(|error| format!("launch bounded verification correction: {error}"))?;
        Ok(true)
    })
}

fn automatic_verification_closure(app: &AppHandle, project_id: &str) -> Result<(), String> {
    let (context, report, manifest, _) = evaluate_p8(app, project_id, true)?;
    match report.decision.state {
        relintor_evidence::CompletionState::VerifiedComplete => {
            ensure_verified_completion_certificate(app, &context, &report, &manifest)?;
        }
        relintor_evidence::CompletionState::FailedVerification => {
            match authorize_and_launch_verification_correction(
                app,
                project_id,
                &report,
                &context.store,
            ) {
                Ok(true) => {}
                Ok(false) => notify_user(
                    app,
                    "Relintor needs your attention",
                    "Verification failed but no safe deterministic correction target was available.",
                ),
                Err(error) => {
                    eprintln!("Relintor verification correction was not authorized: {error}");
                    notify_user(
                        app,
                        "Relintor needs your attention",
                        "Verification found a real failure, but bounded automatic correction could not be authorized safely.",
                    );
                }
            }
        }
        relintor_evidence::CompletionState::BlockedExternal
        | relintor_evidence::CompletionState::RevalidationRequired
        | relintor_evidence::CompletionState::StoppedIncomplete
        | relintor_evidence::CompletionState::CompleteWithAcceptedRisks => {
            notify_user(
                app,
                "Relintor needs your attention",
                "The mission is preserved, but completion still requires an explicit verification or external decision.",
            );
        }
    }
    Ok(())
}

#[tauri::command]
fn verification_start(
    app: AppHandle,
    project_id: String,
) -> Result<VerificationStatusView, String> {
    let (context, report, manifest, collection) = evaluate_p8(&app, &project_id, true)?;
    let certificate = ensure_verified_completion_certificate(&app, &context, &report, &manifest)?;
    let mut workflow_stage = None;
    if report.decision.state == relintor_evidence::CompletionState::FailedVerification {
        // A deterministic implementation failure is not handed back to the
        // user as "done". Relintor authorizes one bounded correction path for
        // the exact failed requirement/dependents when policy still permits it.
        if authorize_and_launch_verification_correction(&app, &project_id, &report, &context.store)?
        {
            workflow_stage = Some("CORRECTING_FAILED_REQUIREMENT");
        }
    }
    if !human_decision_prompts(&context, &report).is_empty() && workflow_stage.is_none() {
        workflow_stage = Some("WAITING_FOR_USER_DECISION");
    }
    if !collection.blocked_external.is_empty()
        && human_decision_prompts(&context, &report).is_empty()
        && workflow_stage.is_none()
    {
        workflow_stage = Some("COLLECTION_BLOCKED");
    }
    Ok(verification_view(
        &project_id,
        &context,
        &report,
        &manifest,
        certificate.as_ref(),
        &collection,
        workflow_stage,
    ))
}

#[tauri::command]
fn verification_status(
    app: AppHandle,
    project_id: String,
) -> Result<VerificationStatusView, String> {
    let (context, report, manifest, collection) = evaluate_p8(&app, &project_id, false)?;
    let certificate =
        if report.decision.state == relintor_evidence::CompletionState::VerifiedComplete {
            load_persisted_certificate(&app, &context, &manifest)?
        } else {
            None
        };
    Ok(verification_view(
        &project_id,
        &context,
        &report,
        &manifest,
        certificate.as_ref(),
        &collection,
        None,
    ))
}

#[tauri::command]
fn verification_rerun(
    app: AppHandle,
    project_id: String,
) -> Result<VerificationStatusView, String> {
    verification_start(app, project_id)
}

#[tauri::command]
async fn verification_submit_human_decision(
    app: AppHandle,
    project_id: String,
    requirement_id: String,
    approved: bool,
    notes: String,
) -> Result<VerificationStatusView, String> {
    if notes.trim().len() > 4_000 {
        return Err("Decision notes exceed the 4,000 character limit.".into());
    }
    let (context, report, _, _) = evaluate_p8(&app, &project_id, false)?;
    let prompt = human_decision_prompts(&context, &report)
        .into_iter()
        .find(|prompt| prompt.requirement_id == requirement_id)
        .ok_or_else(|| {
            "This decision is not currently requested by the sealed verification authority."
                .to_string()
        })?;
    let dialog_app = app.clone();
    let decision = if approved { "approval" } else { "rejection" };
    let decision_button = if approved {
        "Record approval"
    } else {
        "Record rejection"
    };
    let notes_for_confirmation = if notes.trim().is_empty() {
        "No notes supplied".to_string()
    } else {
        notes.trim().to_string()
    };
    let confirmation = format!(
        "{}\n\n{}\n\nDecision: {}\nNotes: {}\n\nOnly confirm if this is your own decision.",
        prompt.question, prompt.summary, decision, notes_for_confirmation
    );
    let confirmed = tauri::async_runtime::spawn_blocking(move || {
        dialog_app
            .dialog()
            .message(confirmation)
            .title("Confirm your Relintor decision")
            .buttons(MessageDialogButtons::OkCancelCustom(
                decision_button.into(),
                "Cancel".into(),
            ))
            .blocking_show()
    })
    .await
    .map_err(|error| format!("confirm explicit user decision: {error}"))?;
    if !confirmed {
        return verification_status(app, project_id);
    }
    ExplicitUserDecisionRecorder
        .record(
            &context.authority,
            &context.current,
            &context.store,
            &context.p7_execution,
            ExplicitUserDecisionInput {
                requirement_id: &requirement_id,
                approved,
                notes: &notes,
            },
        )
        .map_err(|error| format!("record explicit user decision: {error}"))?;
    verification_start(app, project_id)
}

#[tauri::command]
fn verification_evidence(
    app: AppHandle,
    project_id: String,
) -> Result<Vec<VerificationEvidenceView>, String> {
    let (_, _, manifest, _) = evaluate_p8(&app, &project_id, false)?;
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
    let (context, report, manifest, _) = evaluate_p8(&app, &project_id, false)?;
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
    persist_completion_certificate(&app, &context, &certificate)?;
    notify_user(
        &app,
        "Relintor verified complete",
        "The mission certificate was issued and validated.",
    );
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
    let (_, _, manifest, _) = evaluate_p8(&app, &project_id, false)?;
    String::from_utf8(manifest.export_json().map_err(|error| error.to_string())?)
        .map_err(|error| format!("manifest is not UTF-8: {error}"))
}

#[tauri::command]
fn execution_status(app: AppHandle, project_id: String) -> Result<ExecutionStatusView, String> {
    let project_id = canonical_project_id(&project_id)?;
    let (run, ledger_path, revision, _) =
        load_execution_run(&app, &project_id).map_err(authority_read_failure)?;
    p9_status_view(&app, &project_id, &ledger_path, &run, &revision).map_err(authority_read_failure)
}

#[tauri::command]
fn execution_start(app: AppHandle, project_id: String) -> Result<ExecutionStatusView, String> {
    with_execution_mutation_lock(|| execution_start_inner(app, project_id))
}

fn execution_start_inner(
    app: AppHandle,
    project_id: String,
) -> Result<ExecutionStatusView, String> {
    let project_id = canonical_project_id(&project_id)?;
    require_execution_not_active(&project_id)?;
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
                | RecoveryDisposition::PreExecutionRetryAuthorized
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
    let project_id = canonical_project_id(&project_id)?;
    require_execution_not_active(&project_id)?;
    let readiness = health_antigravity_for_app(Some(&app));
    if !readiness.adapter_ready {
        return Err(format!("ANTIGRAVITY_SETUP_REQUIRED: {}", readiness.detail));
    }
    let cli_path = configured_antigravity_cli(&app).ok_or_else(|| {
        "ANTIGRAVITY_SETUP_REQUIRED: the verified Antigravity CLI path is unavailable".to_string()
    })?;
    let (run, ledger_path, revision, _) = load_execution_run(&app, &project_id)?;
    if run.state == relintor_execution::ExecutionRunState::Running {
        return Err("EXECUTION_RUNNING: this mission already has a running task attempt".into());
    }
    match launch_execution_worker(app.clone(), project_id.clone(), cli_path) {
        Ok(_) => p9_status_view(&app, &project_id, &ledger_path, &run, &revision),
        Err(error) => persist_executor_launch_failure(
            &app,
            &project_id,
            run,
            &ledger_path,
            &revision,
            "this task",
            error,
        ),
    }
}

#[tauri::command]
fn execution_pause(app: AppHandle, project_id: String) -> Result<ExecutionStatusView, String> {
    with_execution_mutation_lock(|| execution_pause_inner(app, project_id))
}

fn execution_pause_inner(
    app: AppHandle,
    project_id: String,
) -> Result<ExecutionStatusView, String> {
    let project_id = canonical_project_id(&project_id)?;
    if let Some(control) = active_execution(&project_id)? {
        control.cancel.store(1, Ordering::Release);
        let (run, ledger_path, revision, _) = load_execution_run(&app, &project_id)?;
        return p9_status_view(&app, &project_id, &ledger_path, &run, &revision);
    }
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
    let project_id = canonical_project_id(&project_id)?;
    if let Some(control) = active_execution(&project_id)? {
        control.cancel.store(1, Ordering::Release);
        let (run, ledger_path, revision, _) = load_execution_run(&app, &project_id)?;
        return p9_status_view(&app, &project_id, &ledger_path, &run, &revision);
    }
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
    let project_id = canonical_project_id(&project_id)?;
    require_execution_not_active(&project_id)?;
    let readiness = health_antigravity_for_app(Some(&app));
    if !readiness.adapter_ready {
        return Err(format!("ANTIGRAVITY_SETUP_REQUIRED: {}", readiness.detail));
    }
    let cli_path = configured_antigravity_cli(&app).ok_or_else(|| {
        "ANTIGRAVITY_SETUP_REQUIRED: the verified Antigravity CLI path is unavailable".to_string()
    })?;
    let (mut run, ledger_path, revision, handoff) = load_execution_run(&app, &project_id)?;
    let now = execution_now_ms();
    run.validate_authority_identity(&revision, &handoff, now)
        .map_err(|error| error.to_string())?;
    let recovery = RecoveryCoordinator::new(recovery_store(&app, &revision)?);
    let expected = expected_recovery_authority(&recovery.store, &run, &revision)?;
    let continuation_already_authorized =
        if run.state == relintor_execution::ExecutionRunState::Ready {
            let latest_checkpoint = recovery
                .store
                .load_latest()
                .map_err(|error| error.to_string())?;
            let latest_revalidation = recovery
                .store
                .load_latest_revalidation()
                .map_err(|error| error.to_string())?;
            latest_revalidation.is_some_and(|record| {
                record.project_id == expected.project_id
                    && record.mission_id == expected.mission_id
                    && record.mission_revision == expected.mission_revision
                    && record.p7_run_id == expected.p7_run_id
                    && matches!(
                        record.disposition,
                        RecoveryDisposition::SafeToResume
                            | RecoveryDisposition::SafeToResumeAfterProcessReconciliation
                            | RecoveryDisposition::PreExecutionRetryAuthorized
                            | RecoveryDisposition::StoppedIncomplete
                    )
                    && revalidation_record_is_current(&record, latest_checkpoint.as_ref())
            })
        } else {
            false
        };
    if !continuation_already_authorized {
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
                | RecoveryDisposition::PreExecutionRetryAuthorized
                | RecoveryDisposition::StoppedIncomplete
        ) {
            run.state = relintor_execution::ExecutionRunState::RevalidationRequired;
            run.last_error = integrity.reasons.first().cloned();
            let decision = recovery
                .begin_revalidation(&expected, &integrity, now)
                .map_err(|error| error.to_string())?;
            run.record_recovery_decision(
                integrity.target.as_ref(),
                &format!("{:?}", decision.disposition),
                now,
            )
            .map_err(|error| format!("record recovery decision: {error}"))?;
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
                    "continuation blocked by recovery integrity",
                    now,
                )
                .map_err(|error| error.to_string())?;
            return p9_status_view(&app, &project_id, &ledger_path, &run, &revision);
        }
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
            "continuation authorized; executor dispatch pending",
            now,
        )
        .map_err(|error| error.to_string())?;
    match launch_execution_worker(app.clone(), project_id.clone(), cli_path) {
        Ok(_) => p9_status_view(&app, &project_id, &ledger_path, &run, &revision),
        Err(error) => persist_executor_launch_failure(
            &app,
            &project_id,
            run,
            &ledger_path,
            &revision,
            "this continuation",
            error,
        ),
    }
}

#[tauri::command]
fn execution_revalidate(app: AppHandle, project_id: String) -> Result<ExecutionStatusView, String> {
    with_execution_mutation_lock(|| execution_revalidate_inner(app, project_id))
}

fn execution_revalidate_inner(
    app: AppHandle,
    project_id: String,
) -> Result<ExecutionStatusView, String> {
    let project_id = canonical_project_id(&project_id)?;
    require_execution_not_active(&project_id)?;
    let (mut run, ledger_path, revision, handoff) = load_execution_run(&app, &project_id)?;
    let now = execution_now_ms();
    run.validate_authority_identity(&revision, &handoff, now)
        .map_err(|error| error.to_string())?;
    let recovery = RecoveryCoordinator::new(recovery_store(&app, &revision)?);
    let expected = expected_recovery_authority(&recovery.store, &run, &revision)?;
    if run.state == relintor_execution::ExecutionRunState::Ready {
        let latest_checkpoint = recovery
            .store
            .load_latest()
            .map_err(|error| error.to_string())?;
        let latest_revalidation = recovery
            .store
            .load_latest_revalidation()
            .map_err(|error| error.to_string())?;
        let retry_already_authorized = latest_revalidation.is_some_and(|record| {
            record.project_id == expected.project_id
                && record.mission_id == expected.mission_id
                && record.mission_revision == expected.mission_revision
                && record.p7_run_id == expected.p7_run_id
                && record.target == run.current_recovery_attempt()
                && record.disposition == RecoveryDisposition::PreExecutionRetryAuthorized
                && revalidation_record_is_current(&record, latest_checkpoint.as_ref())
        });
        if retry_already_authorized {
            run.persist_snapshot(&ledger_path)
                .map_err(|error| error.to_string())?;
            return p9_status_view(&app, &project_id, &ledger_path, &run, &revision);
        }
    }
    let workspace = run.workspace.clone();
    let integrity = recovery
        .resume_integrity_for_run(
            &run,
            &expected,
            &workspace,
            &ConservativeProcessInspector,
            now,
        )
        .map_err(|error| error.to_string())?;
    let can_authorize = matches!(
        integrity.disposition,
        RecoveryDisposition::SafeToResume
            | RecoveryDisposition::SafeToResumeAfterProcessReconciliation
            | RecoveryDisposition::PreExecutionRetryAuthorized
            | RecoveryDisposition::StoppedIncomplete
    );
    if can_authorize
        && matches!(
            run.state,
            relintor_execution::ExecutionRunState::BlockedExternal
                | relintor_execution::ExecutionRunState::RevalidationRequired
                | relintor_execution::ExecutionRunState::StoppedIncomplete
        )
    {
        run.resume_from_recovery(integrity.disposition, now)
            .map_err(|error| format!("authorize recovery resume: {error}"))?;
        recovery
            .begin_revalidation(&expected, &integrity, now)
            .map_err(|error| error.to_string())?;
    } else if matches!(
        integrity.disposition,
        RecoveryDisposition::RevalidationRequired | RecoveryDisposition::BlockedExternal
    ) {
        let decision = recovery
            .begin_revalidation(&expected, &integrity, now)
            .map_err(|error| error.to_string())?;
        run.record_recovery_decision(
            integrity.target.as_ref(),
            &format!("{:?}", decision.disposition),
            now,
        )
        .map_err(|error| format!("record recovery decision: {error}"))?;
        run.state = relintor_execution::ExecutionRunState::RevalidationRequired;
        run.last_error = integrity.reasons.first().cloned();
    }
    run.persist_snapshot(&ledger_path)
        .map_err(|error| error.to_string())?;
    p9_status_view(&app, &project_id, &ledger_path, &run, &revision)
}

#[tauri::command]
fn execution_retry_recovered_task(
    app: AppHandle,
    project_id: String,
) -> Result<ExecutionStatusView, String> {
    with_execution_mutation_lock(|| execution_retry_recovered_task_inner(app, project_id))
}

fn execution_retry_recovered_task_inner(
    app: AppHandle,
    project_id: String,
) -> Result<ExecutionStatusView, String> {
    let project_id = canonical_project_id(&project_id)?;
    require_execution_not_active(&project_id)?;
    let readiness = health_antigravity_for_app(Some(&app));
    if !readiness.adapter_ready {
        return Err(format!("ANTIGRAVITY_SETUP_REQUIRED: {}", readiness.detail));
    }
    let cli_path = configured_antigravity_cli(&app).ok_or_else(|| {
        "ANTIGRAVITY_SETUP_REQUIRED: the verified Antigravity CLI path is unavailable".to_string()
    })?;
    let (mut run, ledger_path, revision, handoff) = load_execution_run(&app, &project_id)?;
    let now = execution_now_ms();
    run.validate_authority_identity(&revision, &handoff, now)
        .map_err(|error| error.to_string())?;
    let recovery = RecoveryCoordinator::new(recovery_store(&app, &revision)?);
    let expected = expected_recovery_authority(&recovery.store, &run, &revision)?;
    let integrity = recovery
        .resume_integrity_for_run(
            &run,
            &expected,
            &run.workspace,
            &ConservativeProcessInspector,
            now,
        )
        .map_err(|error| error.to_string())?;
    let latest = recovery
        .store
        .load_latest()
        .map_err(|error| error.to_string())?;
    let latest_revalidation = recovery
        .store
        .load_latest_revalidation()
        .map_err(|error| error.to_string())?;
    let reviewed = latest_revalidation.as_ref().is_some_and(|record| {
        record.project_id == expected.project_id
            && record.mission_id == expected.mission_id
            && record.mission_revision == expected.mission_revision
            && record.p7_run_id == expected.p7_run_id
            && record.target == integrity.target
            && record.disposition == RecoveryDisposition::RevalidationRequired
            && revalidation_record_is_current(record, latest.as_ref())
    });
    if !reviewed {
        return Err(
            "RECOVERY_REVIEW_REQUIRED: check recovery safety and review the listed workspace changes before retrying this task"
                .into(),
        );
    }
    let target = integrity.target.clone().ok_or_else(|| {
        "RECOVERY_REVIEW_REQUIRED: the interrupted task identity is unavailable".to_string()
    })?;
    recovery
        .authorize_manual_retry(&expected, &integrity, now)
        .map_err(|error| error.to_string())?;
    run.authorize_manual_recovery_retry(&target, now)
        .map_err(|error| format!("authorize reviewed recovery retry: {error}"))?;
    run.persist_snapshot(&ledger_path)
        .map_err(|error| error.to_string())?;
    recovery
        .checkpoint_run(
            &run,
            recovery_authority(&run, &revision),
            CheckpointKind::AfterTaskPersistence,
            &run.workspace,
            Vec::new(),
            Vec::new(),
            "explicit recovery review authorized a fresh attempt for the interrupted task",
            now,
        )
        .map_err(|error| format!("write recovery retry checkpoint: {error}"))?;
    match launch_execution_worker(app.clone(), project_id.clone(), cli_path) {
        Ok(_) => p9_status_view(&app, &project_id, &ledger_path, &run, &revision),
        Err(error) => persist_executor_launch_failure(
            &app,
            &project_id,
            run,
            &ledger_path,
            &revision,
            "the reviewed recovery retry",
            error,
        ),
    }
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
    project_execution_scope(&connection, project_id)?;
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
    if previous_revision > 0 {
        let existing =
            relintor_standards::load_mission_revision(path, mission_id.as_str(), previous_revision)
                .map_err(|error| format!("load current sealed mission: {error}"))?;
        let mut candidate = draft.clone();
        candidate.revision = previous_revision;
        let (candidate_revision, _) = engine
            .seal(&candidate, &trusted, "idempotency-check")
            .map_err(|error| format!("compare current sealed mission: {error}"))?;
        if existing.seal.contract_hash == candidate_revision.seal.contract_hash {
            return execution_handoff(&existing);
        }
    }
    draft.revision = previous_revision.saturating_add(1).max(1);
    let (revision, _) = engine
        .seal(&draft, &trusted, &current_timestamp())
        .map_err(|error| format!("Seal & Build blocked: {error}"))?;
    relintor_standards::persist_authority(path, &registry, &revision)
        .map_err(|error| format!("persist sealed mission: {error}"))?;
    execution_handoff(&revision)
}

fn sha256_hex(value: &[u8]) -> String {
    Sha256::digest(value)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn resume_pending_verification_on_startup(app: AppHandle) {
    std::thread::spawn(move || {
        // Let Tauri finish constructing its runtime before evidence collectors
        // or notifications are used. This is not a timer-based completion
        // decision; it only resumes already-finished P7 runs.
        std::thread::sleep(Duration::from_millis(750));
        let projects = match projects_list(app.clone()) {
            Ok(projects) => projects,
            Err(error) => {
                eprintln!("Relintor startup verification scan skipped: {error}");
                return;
            }
        };
        for project in projects {
            let Ok((run, _, _, _)) = load_execution_run(&app, &project.project_id) else {
                continue;
            };
            if run.state
                != relintor_execution::ExecutionRunState::ExecutionTasksFinishedAwaitingVerification
            {
                continue;
            }
            if let Err(error) = automatic_verification_closure(&app, &project.project_id) {
                eprintln!(
                    "Relintor startup verification closure stopped for {}: {error}",
                    project.project_id
                );
            }
        }
    });
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            let handle = app.handle().clone();
            match database_path(&handle) {
                Ok(path) => {
                    if let Err(error) = migrate_database(&path) {
                        eprintln!("Relintor local database initialization failed: {error}");
                    } else {
                        resume_pending_verification_on_startup(handle.clone());
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
            antigravity_setup_status,
            antigravity_setup_start,
            antigravity_setup_cancel,
            antigravity_choose_install_location,
            antigravity_choose_existing_cli,
            antigravity_use_existing_cli,
            antigravity_sign_in,
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
            choose_workspace,
            investigator_takeover_project,
            projects_list,
            project_open,
            set_project_workspace,
            authority_preview,
            seal_project_mission,
            verification_start,
            verification_status,
            verification_rerun,
            verification_submit_human_decision,
            verification_evidence,
            verification_certificate,
            verification_export_manifest,
            execution_status,
            execution_start,
            execution_step,
            execution_pause,
            execution_stop,
            execution_continue,
            execution_revalidate,
            execution_retry_recovered_task
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
        let root = std::env::temp_dir().to_string_lossy().into_owned();
        connection
            .execute(
                "INSERT INTO projects(id, name, root_path, created_at) VALUES (?1, ?2, ?3, 0)",
                params![project_id, format!("Project {project_id}"), root],
            )
            .expect("project identity");
    }

    #[test]
    fn p8_rejects_workspace_changed_after_authenticated_terminal_p7_boundary() {
        ensure_terminal_p7_workspace_matches("terminal", "terminal")
            .expect("exact terminal workspace is verification-eligible");
        assert!(ensure_terminal_p7_workspace_matches("changed", "terminal")
            .expect_err("post-P7 edit must require revalidation")
            .contains("P8_REVALIDATION_REQUIRED"));
    }

    #[test]
    fn completion_certificate_survives_atomic_persistence_and_fresh_reload() {
        let path = test_path("certificate-reload").with_extension("json");
        let certificate = CompletionCertificate {
            certificate_version: "p8-completion-certificate-v1".into(),
            certificate_id: "certificate-reload-fixture".into(),
            project_id: "project-reload".into(),
            mission_id: "mission-project-reload".into(),
            mission_revision: 1,
            p6_seal_hash: "seal".into(),
            registry_id: "registry".into(),
            registry_version: 1,
            registry_digest: "registry-digest".into(),
            p7_execution_run_id: "run".into(),
            workspace_source_fingerprint: "workspace".into(),
            verification_run_id: "verification".into(),
            requirement_status_ledger_hash: "requirements".into(),
            evidence_manifest_hash: "evidence".into(),
            accepted_risks: Vec::new(),
            blocked_external: Vec::new(),
            final_state: relintor_evidence::CompletionState::VerifiedComplete,
            issued_at_ms: 1,
            authority_version: "authority".into(),
            signature: "signature".into(),
        };

        persist_completion_certificate_file(&path, &certificate)
            .expect("persist certificate atomically");
        let reloaded = read_completion_certificate_file(&path).expect("reload certificate");
        assert_eq!(reloaded, certificate);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn legacy_takeover_mission_resolves_prefixed_identity_scope_and_revision_one() {
        let path = test_path("legacy-takeover-reopen");
        migrate_database(&path).expect("migrate");
        let project_id = "takeover-project-legacy-reopen";
        let root = std::fs::canonicalize(std::env::temp_dir()).expect("canonical temp root");
        let persisted_root = if cfg!(windows) {
            format!("\\\\?\\{}", root.display())
        } else {
            root.display().to_string()
        };
        let connection = Connection::open(&path).expect("open legacy fixture");
        connection
            .execute(
                "INSERT INTO projects(id, name, root_path, created_at) VALUES (?1, ?2, ?3, 0)",
                params![project_id, "Legacy takeover", persisted_root],
            )
            .expect("persist legacy project identity");
        drop(connection);

        let review = review_authority_at_path(&path, project_id, &reviewed_facts(&["desktop"]))
            .expect("review legacy takeover");
        assert!(review.blockers.is_empty(), "{review:?}");
        let sealed = seal_project_mission_at_path(&path, project_id, &review.review_digest)
            .expect("seal legacy takeover");
        assert_eq!(sealed.revision, 1);

        let (revision, handoff, _, _, resolved_root, _) =
            latest_execution_context(&path, &format!("mission-{project_id}"))
                .expect("reopen legacy takeover execution context");
        assert_eq!(revision.revision, 1);
        assert_eq!(revision.seal.project_id, project_id);
        assert_eq!(revision.seal.mission_id, format!("mission-{project_id}"));
        assert_eq!(handoff.mission_id, format!("mission-{project_id}"));
        assert!(resolved_root.is_dir());
        assert_eq!(resolved_root, root);

        let connection = Connection::open(&path).expect("reopen legacy fixture for identity");
        let stored_revision: u64 = connection
            .query_row(
                "SELECT MAX(revision) FROM mission_revisions WHERE mission_id = ?1",
                params![format!("mission-{project_id}")],
                |row| row.get(0),
            )
            .expect("stored revision");
        assert_eq!(stored_revision, 1);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn missing_canonical_project_scope_fails_closed_with_domain_error() {
        let path = test_path("missing-canonical-project-scope");
        migrate_database(&path).expect("migrate");
        let connection = Connection::open(&path).expect("open missing project fixture");
        let error = project_execution_scope(&connection, "missing-project")
            .expect_err("missing project scope must fail closed");
        assert!(
            error.starts_with("PROJECT_EXECUTION_SCOPE_MISSING"),
            "{error}"
        );
        let _ = fs::remove_file(path);
    }

    #[test]
    fn active_dispatch_ownership_rejects_duplicate_controls() {
        let project_id = format!("active-dispatch-{}", std::process::id());
        let control = Arc::new(ActiveExecution {
            cancel: Arc::new(AtomicU64::new(0)),
            process_started: AtomicBool::new(false),
            process_exited: AtomicBool::new(false),
            process: Mutex::new(None),
        });
        active_executions()
            .lock()
            .expect("active execution lock")
            .insert(project_id.clone(), control);
        let error = require_execution_not_active(&project_id)
            .expect_err("a second dispatch/recovery control must be rejected");
        assert!(error.starts_with("EXECUTION_RUNNING:"), "{error}");
        active_executions()
            .lock()
            .expect("active execution lock")
            .remove(&project_id);
        require_execution_not_active(&project_id).expect("ownership is released");
    }

    #[test]
    fn execution_phase_never_reports_running_without_current_owned_worker() {
        use relintor_execution::ExecutionRunState;

        assert_eq!(
            execution_phase_for(&ExecutionRunState::Ready, true, false, false, false),
            "CONTINUATION_AUTHORIZED"
        );
        assert_eq!(
            execution_phase_for(&ExecutionRunState::Ready, false, true, false, false),
            "CONTINUATION_DISPATCHING"
        );
        assert_eq!(
            execution_phase_for(&ExecutionRunState::Running, false, true, true, false),
            "RUNNING"
        );
        assert_eq!(
            execution_phase_for(&ExecutionRunState::Running, false, false, true, false),
            "STOPPED"
        );
        assert_eq!(
            execution_phase_for(
                &ExecutionRunState::BlockedExternal,
                false,
                true,
                false,
                true
            ),
            "FAILED"
        );
        assert_eq!(
            execution_phase_for(&ExecutionRunState::Ready, false, true, true, true),
            "EXECUTOR_EXITED"
        );
    }

    #[test]
    fn legacy_recovery_read_failure_has_a_distinct_actionable_domain() {
        let error = authority_read_failure(
            "restore P9 recovery checkpoint: recovery record corrupt: execution ledger integrity verification failed".into(),
        );
        assert!(error.starts_with("AUTHORITY_READ_INTEGRITY_FAILURE"));
        assert!(!error.contains("Query returned no rows"));
    }

    #[test]
    fn historical_budget_read_failure_is_recovery_required_not_corruption() {
        let error = authority_read_failure(
            "restore P7 execution ledger: execution ledger: historical budget boundary requires recovery".into(),
        );
        assert!(error.starts_with("AUTHORITY_READ_RECOVERY_REQUIRED"));
        assert!(!error.contains("older execution record"));
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
    fn new_idea_persists_canonical_project_and_seals_only_with_real_scope() {
        let path = test_path("new-idea-canonical-project");
        migrate_database(&path).expect("migrate");
        let result = Investigator::default()
            .investigate(
                ProjectDraft::from_idea("A private project tracker").expect("project draft"),
                Vec::new(),
                &[],
            )
            .expect("investigation");
        let project_id = result.project.id.clone();
        let investigation_project_id = result.investigation.project_id.clone();
        persist_result(&path, &result).expect("persist new idea");

        let connection = Connection::open(&path).expect("open");
        let project_count: u32 = connection
            .query_row(
                "SELECT COUNT(*) FROM projects WHERE id = ?1",
                params![project_id.as_str()],
                |row| row.get(0),
            )
            .expect("canonical project count");
        let root: Option<String> = connection
            .query_row(
                "SELECT root_path FROM projects WHERE id = ?1",
                params![project_id.as_str()],
                |row| row.get(0),
            )
            .expect("canonical project root");
        assert_eq!(project_count, 1);
        assert_eq!(investigation_project_id, project_id);
        assert!(
            root.is_none(),
            "new ideas must not receive a fake workspace"
        );
        drop(connection);

        let blocked_review =
            review_authority_at_path(&path, &project_id, &reviewed_facts(&["desktop"]))
                .expect("blocked authority review remains inspectable");
        assert!(
            blocked_review
                .blockers
                .iter()
                .any(|blocker| blocker.starts_with("PROJECT_EXECUTION_SCOPE_REQUIRED")),
            "{blocked_review:?}"
        );
        let blocked_seal =
            seal_project_mission_at_path(&path, &project_id, &blocked_review.review_digest)
                .expect_err("sealing without a real workspace must be blocked");
        assert!(blocked_seal.starts_with("PROJECT_EXECUTION_SCOPE_REQUIRED"));
        assert!(!blocked_seal.contains("Query returned no rows"));

        let real_workspace = std::env::temp_dir().to_string_lossy().into_owned();
        let connection = Connection::open(&path).expect("reopen");
        connection
            .execute(
                "UPDATE projects SET root_path = ?1 WHERE id = ?2",
                params![real_workspace.as_str(), project_id.as_str()],
            )
            .expect("attach real workspace");
        drop(connection);

        let review = review_authority_at_path(&path, &project_id, &reviewed_facts(&["desktop"]))
            .expect("authority review");
        assert!(review.blockers.is_empty(), "{review:?}");
        let handoff =
            seal_project_mission_at_path(&path, &project_id, &review.review_digest).expect("seal");
        let (revision, latest_handoff, _, _, root, _) =
            latest_execution_context(&path, &project_id).expect("execution context");
        assert_eq!(handoff, latest_handoff);
        assert_eq!(root, PathBuf::from(real_workspace));
        assert_eq!(revision.seal.project_id, project_id);
        assert_eq!(handoff.mission_id, format!("mission-{project_id}"));

        let connection = Connection::open(&path).expect("reopen for identity checks");
        let authority_project_id: String = connection
            .query_row(
                "SELECT project_id FROM authority_reviews WHERE project_id = ?1",
                params![project_id.as_str()],
                |row| row.get(0),
            )
            .expect("authority project identity");
        let revision_project_id: String = connection
            .query_row(
                "SELECT json_extract(seal_json, '$.project_id') FROM mission_seals WHERE mission_id = ?1 AND revision = ?2",
                params![handoff.mission_id.as_str(), handoff.revision],
                |row| row.get(0),
            )
            .expect("sealed mission identity");
        assert_eq!(authority_project_id, project_id);
        assert_eq!(revision_project_id, project_id);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn project_open_restores_persisted_investigation_and_sealed_handoff() {
        let path = test_path("project-open-restores-workflow");
        migrate_database(&path).expect("migrate");
        let result = Investigator::default()
            .investigate(
                ProjectDraft::from_idea("A project that can be reopened").expect("project draft"),
                Vec::new(),
                &[],
            )
            .expect("investigation");
        let project_id = result.project.id.clone();
        persist_result(&path, &result).expect("persist workflow");

        let reopened = open_project_at_path(&path, &project_id).expect("open persisted workflow");
        assert_eq!(reopened.project.project_id, project_id);
        assert!(reopened.investigation.is_some());
        assert!(reopened.authority_facts.is_empty());
        assert!(reopened.handoff.is_none());

        let workspace = std::env::temp_dir().to_string_lossy().into_owned();
        let connection = Connection::open(&path).expect("reopen database");
        connection
            .execute(
                "UPDATE projects SET root_path = ?1 WHERE id = ?2",
                params![workspace.as_str(), project_id.as_str()],
            )
            .expect("persist real workspace");
        drop(connection);

        let review = review_authority_at_path(&path, &project_id, &reviewed_facts(&["desktop"]))
            .expect("authority review");
        assert!(review.blockers.is_empty(), "{review:?}");
        let sealed =
            seal_project_mission_at_path(&path, &project_id, &review.review_digest).expect("seal");
        let reopened = open_project_at_path(&path, &project_id).expect("open sealed workflow");
        assert_eq!(
            reopened.project.project_id,
            sealed.mission_id.trim_start_matches("mission-").to_string()
        );
        assert_eq!(
            reopened.authority_facts,
            canonicalize_reviewed_facts(&reviewed_facts(&["desktop"])).expect("canonical facts")
        );
        assert_eq!(reopened.handoff, Some(sealed));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn sealing_unchanged_review_is_idempotent() {
        let path = test_path("seal-idempotent");
        migrate_database(&path).expect("migrate");
        let connection = Connection::open(&path).expect("open");
        seed_project(&connection, "idempotent-project");
        drop(connection);

        let review =
            review_authority_at_path(&path, "idempotent-project", &reviewed_facts(&["desktop"]))
                .expect("authority review");
        assert!(review.blockers.is_empty(), "{review:?}");
        let first =
            seal_project_mission_at_path(&path, "idempotent-project", &review.review_digest)
                .expect("first seal");
        let second =
            seal_project_mission_at_path(&path, "idempotent-project", &review.review_digest)
                .expect("idempotent seal");
        assert_eq!(first, second);
        let connection = Connection::open(&path).expect("reopen");
        let revisions: u32 = connection
            .query_row(
                "SELECT COUNT(*) FROM mission_revisions WHERE mission_id = 'mission-idempotent-project'",
                [],
                |row| row.get(0),
            )
            .expect("revision count");
        assert_eq!(revisions, 1);
        let _ = fs::remove_file(path);
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
        assert!(DesktopPublicConfig::from_sources(Some(" ".into()), None, None, None).is_err());
        assert!(DesktopPublicConfig::from_sources(None, Some("\t".into()), None, None).is_err());
        assert!(DesktopPublicConfig::from_sources(None, None, None, Some(String::new())).is_err());
    }

    #[test]
    fn public_config_requires_https_for_production_endpoints() {
        let cloud_error = DesktopPublicConfig::from_sources(
            Some("http://cloud.example.test".into()),
            None,
            None,
            None,
        )
        .err()
        .expect("expected configuration error");
        assert!(cloud_error.contains("RELINTOR_CLOUD_API_ENDPOINT"));

        let ai_error = DesktopPublicConfig::from_sources(
            None,
            None,
            Some("test-client-secret".into()),
            Some("http://gateway.example.test".into()),
        )
        .err()
        .expect("expected configuration error");
        assert!(ai_error.contains("RELINTOR_AI_GATEWAY_ENDPOINT"));
    }

    #[test]
    fn public_config_allows_explicit_runtime_overrides() {
        let config = DesktopPublicConfig::from_sources(
            Some("https://cloud.override.example.test/".into()),
            Some("client-override".into()),
            Some("client-secret-override".into()),
            Some("https://gateway.override.example.test/".into()),
        )
        .expect("runtime public overrides");
        assert_eq!(
            config.cloud_api_endpoint,
            "https://cloud.override.example.test/"
        );
        assert_eq!(config.google_client_id, "client-override");
        assert_eq!(config.google_client_secret, "client-secret-override");
        assert_eq!(
            config.ai_gateway_endpoint,
            "https://gateway.override.example.test/"
        );
    }

    #[test]
    fn google_token_form_contains_required_native_oauth_fields() {
        let secret = "test-client-secret";
        let form = google_token_form(
            "auth-code",
            "pkce-verifier",
            "http://127.0.0.1:43123/oauth/callback",
            "client-id",
            secret,
        );
        let fields = url::form_urlencoded::parse(form.as_bytes())
            .into_owned()
            .collect::<BTreeMap<_, _>>();

        assert_eq!(
            fields.get("client_id").map(String::as_str),
            Some("client-id")
        );
        assert_eq!(
            fields.get("client_secret").map(String::as_str),
            Some(secret)
        );
        assert_eq!(fields.get("code").map(String::as_str), Some("auth-code"));
        assert_eq!(
            fields.get("code_verifier").map(String::as_str),
            Some("pkce-verifier")
        );
        assert_eq!(
            fields.get("redirect_uri").map(String::as_str),
            Some("http://127.0.0.1:43123/oauth/callback")
        );
        assert_eq!(
            fields.get("grant_type").map(String::as_str),
            Some("authorization_code")
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

    #[test]
    fn antigravity_setup_policy_is_official_explicit_and_fail_closed() {
        assert!(ANTIGRAVITY_OFFICIAL_INSTALLER.starts_with("https://"));
        assert_eq!(ANTIGRAVITY_MIN_WORKING_SPACE_BYTES, 5 * 1024 * 1024 * 1024);
        assert!(!meets_storage_reserve(Some(
            ANTIGRAVITY_MIN_WORKING_SPACE_BYTES - 1
        )));
        assert!(meets_storage_reserve(Some(
            ANTIGRAVITY_MIN_WORKING_SPACE_BYTES
        )));
        assert!(!meets_storage_reserve(None));
        let command = installer_command();
        let args = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert!(args.iter().any(|arg| arg == "-NoProfile"));
        assert!(args
            .iter()
            .any(|arg| arg.contains(ANTIGRAVITY_OFFICIAL_INSTALLER)));
        assert!(args
            .iter()
            .all(|arg| !arg.contains("RELINTOR_ANTIGRAVITY_INSTALL_ROOT")));
    }

    #[test]
    fn setup_metadata_replacement_handles_existing_windows_file() {
        let root = test_path("setup-metadata-replacement");
        fs::create_dir_all(&root).expect("setup test directory");
        let destination = root.join("setup.json");
        let temporary = root.join(".setup-test.tmp");
        fs::write(&destination, b"old").expect("old setup metadata");
        fs::write(&temporary, b"new").expect("new setup metadata");
        atomic_replace_file(&temporary, &destination).expect("replace setup metadata");
        assert_eq!(fs::read(&destination).expect("read setup metadata"), b"new");
        assert!(!temporary.exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn internal_console_processes_use_hidden_window_policy() {
        assert_eq!(
            relintor_antigravity::WINDOWS_CREATE_NO_WINDOW,
            if cfg!(windows) { 0x0800_0000 } else { 0 }
        );
        let command = hidden_command("powershell.exe");
        assert_eq!(command.get_program(), OsStr::new("powershell.exe"));
    }

    #[test]
    fn antigravity_auth_uses_one_intentional_interactive_process() {
        let command = interactive_auth_command(Path::new("agy.exe"));
        assert_eq!(command.get_program(), OsStr::new("agy.exe"));
        assert!(command.get_current_dir().is_some());
        assert_eq!(ANTIGRAVITY_AUTH_TIMEOUT, Duration::from_secs(10 * 60));
        #[cfg(windows)]
        assert_eq!(WINDOWS_CREATE_NEW_CONSOLE, 0x0000_0010);
    }

    #[test]
    fn browser_launch_remains_a_separate_visible_path() {
        let command = browser_command("rundll32.exe");
        assert_eq!(command.get_program(), OsStr::new("rundll32.exe"));
    }

    #[test]
    fn setup_and_authentication_are_single_flight_and_bounded() {
        assert!(ANTIGRAVITY_CHILD_TIMEOUT <= Duration::from_secs(10 * 60));
        let runtime = AntigravitySetupRuntime {
            state: Mutex::new(None),
            cancel: AtomicBool::new(false),
            operation_active: AtomicBool::new(false),
            active_child_pid: Mutex::new(None),
            worker: Mutex::new(None),
        };
        assert!(runtime
            .operation_active
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok());
        assert!(runtime
            .operation_active
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err());
        runtime.cancel.store(true, Ordering::Release);
        assert!(setup_cancelled(&runtime));
    }

    #[test]
    fn antigravity_setup_does_not_replace_existing_installation_folder() {
        let root =
            std::env::temp_dir().join(format!("relintor-antigravity-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("existing.txt"), b"preserve").unwrap();
        let result = prepare_install_root(&root);
        assert!(result.is_err());
        assert!(root.join("existing.txt").is_file());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn antigravity_setup_status_has_no_token_or_secret_fields() {
        let view = AntigravitySetupView {
            stage: "AUTH_REQUIRED".into(),
            active: false,
            progress_percent: None,
            progress_indeterminate: false,
            storage_path: None,
            recommended_storage_path: None,
            storage_detail: "Choose a location.".into(),
            c: DiskSpaceView {
                available_bytes: Some(6 * 1024 * 1024 * 1024),
                safe_minimum_bytes: ANTIGRAVITY_MIN_WORKING_SPACE_BYTES,
            },
            d: DiskSpaceView {
                available_bytes: Some(18 * 1024 * 1024 * 1024),
                safe_minimum_bytes: ANTIGRAVITY_MIN_WORKING_SPACE_BYTES,
            },
            local_appdata_state: "absent".into(),
            cli_path: None,
            cli_version: None,
            authentication_status: "sign_in_required".into(),
            adapter_ready: false,
            consent_required: true,
            automatic_install_available: cfg!(windows),
            requires_location: true,
            can_cancel: false,
            can_retry: true,
            detail: "Sign-in required.".into(),
            error: None,
            advanced_details: "stage=AUTH_REQUIRED".into(),
        };
        let encoded = serde_json::to_string(&view).unwrap();
        assert!(!encoded.to_ascii_lowercase().contains("token"));
        assert!(!encoded.to_ascii_lowercase().contains("secret"));
        assert!(!encoded.contains("Authorization"));
    }

    #[test]
    fn bridge_trust_root_is_pinned_public_only() {
        let key = trusted_bridge_key().unwrap();
        assert_eq!(
            relintor_antigravity::trusted_key_id(&key),
            "0918bc34b503e98bb76d741d6168787d87153f3adf96d45c9a24eb02e0d2240d"
        );
        assert!(!ANTIGRAVITY_TRUSTED_KEYS
            .to_ascii_lowercase()
            .contains("private"));
        assert!(!ANTIGRAVITY_TRUSTED_KEYS
            .to_ascii_lowercase()
            .contains("secret"));
    }

    #[test]
    fn adapter_readiness_requires_cli_auth_and_verified_bridge() {
        assert!(!antigravity_adapter_ready(false, true, true));
        assert!(!antigravity_adapter_ready(true, false, true));
        assert!(!antigravity_adapter_ready(true, true, false));
        assert!(antigravity_adapter_ready(true, true, true));
    }
}
