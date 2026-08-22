//! Relintor-owned Antigravity bridge boundary.
//!
//! This crate does not automate the GUI and never emits a verification result.
//! It owns only typed compatibility, task, session, process, event, and bridge
//! identity primitives.

use ed25519_dalek::Verifier;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    env,
    ffi::{OsStr, OsString},
    fs,
    io::{self, Read},
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc::{self, Receiver},
        Arc,
    },
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[cfg(windows)]
pub const WINDOWS_CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[cfg(not(windows))]
pub const WINDOWS_CREATE_NO_WINDOW: u32 = 0;

pub const PRODUCTION_PROCESS_TIMEOUT: Duration = Duration::from_secs(45 * 60);
/// A healthy long-running executor may be quiet for a while (for example while
/// compiling or reasoning). Five minutes remains a finite no-output safety
/// boundary while avoiding the old 90-second false-stop behavior.
pub const PRODUCTION_IDLE_TIMEOUT: Duration = Duration::from_secs(5 * 60);

fn hidden_command<S: AsRef<OsStr>>(program: S) -> Command {
    let mut command = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(WINDOWS_CREATE_NO_WINDOW);
    }
    command
}

fn force_kill_owned_process_tree(pid: u32) {
    #[cfg(windows)]
    {
        let mut command = hidden_command("taskkill.exe");
        let _ = command
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .status();
    }
    #[cfg(not(windows))]
    {
        let _ = pid;
    }
}

fn inherited_runtime_environment() -> Vec<(OsString, OsString)> {
    [
        "APPDATA",
        "COMSPEC",
        "HOME",
        "LOCALAPPDATA",
        "PATH",
        "PROGRAMDATA",
        "SYSTEMROOT",
        "TEMP",
        "TMP",
        "USERPROFILE",
        "WINDIR",
    ]
    .into_iter()
    .filter_map(|key| env::var_os(key).map(|value| (OsString::from(key), value)))
    .collect()
}

pub const ADAPTER_VERSION: &str = "0.1.0";
pub const TASK_PACKET_VERSION: &str = "1";
pub const SESSION_VERSION: &str = "1";
pub const MAX_PACKET_BYTES: usize = 256 * 1024;
pub const MAX_OUTPUT_BYTES: usize = 1024 * 1024;
const REGISTRY: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../integrations/antigravity/compatibility/registry.json"
));
const TRUSTED_BRIDGE_KEYS: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../integrations/antigravity/plugin/trusted-keys.json"
));

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SupportedVersion {
    pub version_prefix: String,
    pub plugin_hook_capability: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CompatibilityRegistry {
    pub schema_version: u32,
    pub cli_names: Vec<String>,
    pub supported_versions: Vec<SupportedVersion>,
}

impl CompatibilityRegistry {
    pub fn bundled() -> Result<Self, BridgeError> {
        serde_json::from_str(REGISTRY).map_err(|e| BridgeError::Registry(e.to_string()))
    }
    fn compatibility(&self, version: Option<&str>) -> Compatibility {
        match version {
            None => Compatibility::Unknown,
            Some(version)
                if self
                    .supported_versions
                    .iter()
                    .any(|v| version.starts_with(&v.version_prefix)) =>
            {
                Compatibility::Supported
            }
            Some(_) => Compatibility::Unsupported,
        }
    }
    fn hook_capability(&self, version: Option<&str>) -> bool {
        version
            .and_then(|v| {
                self.supported_versions
                    .iter()
                    .find(|s| v.starts_with(&s.version_prefix))
            })
            .map(|s| s.plugin_hook_capability)
            .unwrap_or(false)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Compatibility {
    Supported,
    Unsupported,
    Unknown,
    NotInstalled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CompatibilityReport {
    pub executable_present: bool,
    pub executable_path: Option<String>,
    pub version: Option<String>,
    pub compatibility: Compatibility,
    pub cli_invocation_capability: bool,
    pub plugin_hook_capability: bool,
    pub environment: String,
    pub platform: String,
    pub detected_at_ms: i128,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CompatibilityMatrixEntry {
    pub status: Compatibility,
    pub antigravity_version: Option<String>,
    pub plugin_protocol: String,
    pub adapter_version: String,
    pub headless_runner_available: bool,
    pub hook_capability: bool,
    pub detail: String,
}

pub fn certify_compatibility(
    report: &CompatibilityReport,
    manifest: &BridgeManifest,
) -> CompatibilityMatrixEntry {
    let status = if !report.executable_present {
        Compatibility::NotInstalled
    } else if report.version.is_none() || !report.cli_invocation_capability {
        Compatibility::Unknown
    } else if report.compatibility != Compatibility::Supported {
        Compatibility::Unsupported
    } else {
        Compatibility::Supported
    };
    CompatibilityMatrixEntry {
        status,
        antigravity_version: report.version.clone(),
        plugin_protocol: manifest.hook_protocol.clone(),
        adapter_version: manifest.adapter_version.clone(),
        headless_runner_available: report.cli_invocation_capability,
        hook_capability: report.plugin_hook_capability,
        detail: match status {
            Compatibility::Supported => {
                "installed version and required runner capabilities are supported".into()
            }
            Compatibility::Unsupported => {
                "installed version is outside the sealed compatibility registry".into()
            }
            Compatibility::Unknown => {
                "runtime was present but version or runner capability could not be proven".into()
            }
            Compatibility::NotInstalled => "Antigravity executable is unavailable".into(),
        },
    }
}

impl CompatibilityReport {
    pub fn sealed_execution_allowed(&self) -> bool {
        self.executable_present
            && self.cli_invocation_capability
            && self.compatibility == Compatibility::Supported
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetectionInput {
    pub executable_path: Option<PathBuf>,
    pub version_output: Option<String>,
    pub cli_invocation_capability: bool,
    pub plugin_hook_capability: bool,
    pub environment: String,
    pub platform: String,
    pub detected_at_ms: i128,
}

pub fn detect() -> CompatibilityReport {
    detect_with_override(env::var_os("ANTIGRAVITY_PATH").map(PathBuf::from))
}

pub fn detect_with_override(configured_path: Option<PathBuf>) -> CompatibilityReport {
    let registry = CompatibilityRegistry::bundled().unwrap_or(CompatibilityRegistry {
        schema_version: 0,
        cli_names: vec!["agy".into(), "agy.exe".into()],
        supported_versions: vec![],
    });
    let override_path = configured_path;
    let mut dirs: Vec<PathBuf> = env::var_os("PATH")
        .map(|p| env::split_paths(&p).collect())
        .unwrap_or_default();
    dirs.extend(standard_cli_dirs());
    let path = find_executable(override_path, &dirs, &registry.cli_names);
    let Some(path) = path else {
        return report(None, None, false, false, &registry);
    };
    let version_probe = probe(&path, "--version");
    let help_probe = probe(&path, "--help");
    let invocation =
        version_probe.as_ref().is_some_and(|p| p.0) && help_probe.as_ref().is_some_and(|p| p.0);
    let version = version_probe.and_then(|p| p.1);
    let plugin = help_probe
        .and_then(|p| p.1)
        .map(|text| {
            let text = text.to_ascii_lowercase();
            text.contains("plugin") || text.contains("hook")
        })
        .unwrap_or(false);
    report(Some(path), version, invocation, plugin, &registry)
}

fn standard_cli_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    #[cfg(windows)]
    {
        if let Some(local_app_data) = env::var_os("LOCALAPPDATA") {
            dirs.push(PathBuf::from(local_app_data).join("agy").join("bin"));
        }
        if let Some(user_profile) = env::var_os("USERPROFILE") {
            dirs.push(
                PathBuf::from(user_profile)
                    .join("AppData")
                    .join("Local")
                    .join("agy")
                    .join("bin"),
            );
        }
    }
    #[cfg(unix)]
    if let Some(home) = env::var_os("HOME") {
        dirs.push(PathBuf::from(home).join(".local").join("bin"));
    }
    dirs
}

pub fn classify_detection(
    input: DetectionInput,
    registry: &CompatibilityRegistry,
) -> CompatibilityReport {
    let version = input.version_output.map(normalize_version);
    let compatibility = if input.executable_path.is_none() {
        Compatibility::NotInstalled
    } else if !input.cli_invocation_capability {
        Compatibility::Unknown
    } else {
        registry.compatibility(version.as_deref())
    };
    let detail = match compatibility {
        Compatibility::Supported => "Registry-supported Antigravity CLI.".into(),
        Compatibility::Unsupported => "CLI invoked, but version is not registry-supported.".into(),
        Compatibility::Unknown => "Compatibility is unknown; sealed execution fails closed.".into(),
        Compatibility::NotInstalled => "Official Antigravity CLI (agy) was not found.".into(),
    };
    CompatibilityReport {
        executable_present: input.executable_path.is_some(),
        executable_path: input.executable_path.map(|p| p.display().to_string()),
        version,
        compatibility,
        cli_invocation_capability: input.cli_invocation_capability,
        plugin_hook_capability: input.plugin_hook_capability,
        environment: input.environment,
        platform: input.platform,
        detected_at_ms: input.detected_at_ms,
        detail,
    }
}

fn report(
    path: Option<PathBuf>,
    version: Option<String>,
    invocation: bool,
    plugin: bool,
    registry: &CompatibilityRegistry,
) -> CompatibilityReport {
    let plugin_hook_capability = plugin && registry.hook_capability(version.as_deref());
    classify_detection(
        DetectionInput {
            executable_path: path,
            version_output: version,
            cli_invocation_capability: invocation,
            plugin_hook_capability,
            environment: env::var("RELINTOR_ENVIRONMENT").unwrap_or_else(|_| "local".into()),
            platform: env::consts::OS.into(),
            detected_at_ms: now_ms(),
        },
        registry,
    )
}

fn probe(path: &Path, arg: &str) -> Option<(bool, Option<String>)> {
    let output = hidden_command(path).arg(arg).output().ok()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let text = if stdout.trim().is_empty() {
        stderr.trim()
    } else {
        stdout.trim()
    };
    Some((
        output.status.success(),
        (!text.is_empty()).then(|| text.to_string()),
    ))
}

fn normalize_version(raw: String) -> String {
    raw.split_whitespace()
        .find(|part| part.chars().next().is_some_and(|c| c.is_ascii_digit()))
        .unwrap_or(raw.trim())
        .trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '.' && c != '-')
        .into()
}

fn find_executable(
    override_path: Option<PathBuf>,
    dirs: &[PathBuf],
    names: &[String],
) -> Option<PathBuf> {
    let candidates = override_path
        .into_iter()
        .chain(dirs.iter().map(PathBuf::clone));
    for base in candidates {
        if base.is_file() && is_executable(&base) {
            return fs::canonicalize(base).ok();
        }
        if base.is_dir() {
            for name in names {
                let candidate = base.join(name);
                if is_executable(&candidate) {
                    return fs::canonicalize(candidate).ok();
                }
            }
        }
    }
    None
}

fn is_executable(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::metadata(path)
            .map(|m| m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        true
    }
}

fn now_ms() -> i128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i128)
        .unwrap_or_default()
}

/// Returns the command identity that can be reconstructed from a live
/// Windows process after Relintor has restarted. Environment variables and
/// the current directory are deliberately excluded: Windows exposes the
/// executable and argv reliably, while those other launch details are not a
/// trustworthy restart-time process identity.
pub fn observable_process_command_digest(program: &Path, args: &[String]) -> String {
    let normalized_program = normalize_observable_path(program);
    format!(
        "p9-command-v2:{}",
        sha256(
            &serde_json::to_vec(&("p9-command-v2", normalized_program, args))
                .expect("command identity tuple is serializable"),
        )
    )
}

fn normalize_observable_path(path: &Path) -> String {
    let mut value = path.to_string_lossy().replace('/', "\\");
    if let Some(stripped) = value.strip_prefix(r"\\?\UNC\") {
        value = format!(r"\\{stripped}");
    } else if let Some(stripped) = value.strip_prefix(r"\\?\") {
        value = stripped.to_string();
    }
    value.to_ascii_lowercase()
}

/// Reads the operating-system process creation time. A missing value is an
/// explicit inability to prove identity; callers must not substitute the
/// process name or PID in its place.
#[cfg(windows)]
pub fn process_creation_time_ms(pid: u32) -> Option<i128> {
    use std::ffi::c_void;

    #[repr(C)]
    struct FileTime {
        low: u32,
        high: u32,
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn OpenProcess(desired_access: u32, inherit_handle: i32, process_id: u32) -> *mut c_void;
        fn GetProcessTimes(
            process: *mut c_void,
            creation: *mut FileTime,
            exit: *mut FileTime,
            kernel: *mut FileTime,
            user: *mut FileTime,
        ) -> i32;
        fn CloseHandle(object: *mut c_void) -> i32;
    }

    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
    const WINDOWS_TO_UNIX_100NS: i128 = 116_444_736_000_000_000;
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if handle.is_null() {
        return None;
    }
    let mut creation = FileTime { low: 0, high: 0 };
    let mut exit = FileTime { low: 0, high: 0 };
    let mut kernel = FileTime { low: 0, high: 0 };
    let mut user = FileTime { low: 0, high: 0 };
    let success =
        unsafe { GetProcessTimes(handle, &mut creation, &mut exit, &mut kernel, &mut user) } != 0;
    unsafe {
        CloseHandle(handle);
    }
    if !success {
        return None;
    }
    let ticks = (u64::from(creation.high) << 32) | u64::from(creation.low);
    let unix_100ns = i128::from(ticks) - WINDOWS_TO_UNIX_100NS;
    Some(unix_100ns / 10_000)
}

#[cfg(not(windows))]
pub fn process_creation_time_ms(_pid: u32) -> Option<i128> {
    None
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskPacket {
    pub contract_version: String,
    pub mission_id: String,
    #[serde(default)]
    pub mission_revision: u64,
    #[serde(default)]
    pub seal_hash: String,
    pub task_id: String,
    #[serde(default)]
    pub project_id: String,
    pub workspace: PathBuf,
    #[serde(default)]
    pub workspace_fingerprint: String,
    #[serde(default)]
    pub task_packet_digest: String,
    #[serde(default)]
    pub lease_id: String,
    #[serde(default)]
    pub lease_expires_at_ms: u64,
    /// Remaining task authority copied from the scheduler packet. The CLI
    /// uses this only as a matching safety cap; Rust remains authoritative.
    #[serde(default)]
    pub time_budget_ms: u64,
    #[serde(default)]
    pub allowed_tools: BTreeSet<String>,
    #[serde(default)]
    pub worktree_identity: String,
    #[serde(default)]
    pub subagent_identity: Option<String>,
    pub objective: String,
    pub scope: Vec<String>,
    pub sealed_requirement_ids: Vec<String>,
    pub architecture_decisions: Vec<String>,
    pub professional_constraints: Vec<String>,
    pub required_evidence: Vec<String>,
    pub previous_failures: Vec<String>,
    pub forbidden_changes: Vec<String>,
    pub stop_condition: StopCondition,
    /// The one action capability issued by P7 for this task.  Hook messages
    /// must reproduce this identity exactly; the hook caller cannot mint a
    /// new operation by supplying a self-consistent digest.
    pub authorized_action: AuthorizedAction,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AuthorizedAction {
    pub sequence: u64,
    pub tool: String,
    pub operation: String,
    pub arguments: Vec<String>,
    pub paths: Vec<String>,
    pub working_scope: String,
    pub expires_at_ms: u64,
    pub nonce: String,
    pub digest: String,
}

impl AuthorizedAction {
    pub fn refresh_digest(&mut self) -> Result<(), BridgeError> {
        self.digest = canonical_authorized_action_digest(self)?;
        Ok(())
    }

    pub fn validate(&self) -> Result<(), BridgeError> {
        if self.sequence == 0
            || self.tool.trim().is_empty()
            || self.operation.trim().is_empty()
            || self.working_scope.trim().is_empty()
            || self.expires_at_ms == 0
            || self.nonce.trim().is_empty()
            || self.digest != canonical_authorized_action_digest(self)?
        {
            return Err(BridgeError::Authority(
                "authorized action capability is incomplete or forged".into(),
            ));
        }
        Ok(())
    }
}

pub fn canonical_authorized_action_digest(
    action: &AuthorizedAction,
) -> Result<String, BridgeError> {
    let bytes = serde_json::to_vec(&(
        action.sequence,
        action.tool.to_ascii_lowercase(),
        action.operation.to_ascii_lowercase(),
        action.arguments.clone(),
        action.paths.clone(),
        action.working_scope.clone(),
        action.expires_at_ms,
        action.nonce.clone(),
    ))
    .map_err(|error| BridgeError::Packet(error.to_string()))?;
    Ok(sha256(&bytes))
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum StopCondition {
    AfterProcessExit,
    AfterArtifactCollection,
    OnError,
}

impl TaskPacket {
    pub fn validate(&self, allowed_root: &Path) -> Result<(), BridgeError> {
        if self.contract_version != TASK_PACKET_VERSION {
            return Err(BridgeError::Packet("unsupported packet version".into()));
        }
        if self.mission_id.trim().is_empty() || self.task_id.trim().is_empty() {
            return Err(BridgeError::Packet(
                "mission/task identity is required".into(),
            ));
        }
        if self.mission_revision == 0 || self.seal_hash.trim().is_empty() {
            return Err(BridgeError::Packet(
                "sealed mission revision and seal hash are required".into(),
            ));
        }
        if self.project_id.trim().is_empty()
            || self.workspace_fingerprint.trim().is_empty()
            || self.task_packet_digest.trim().is_empty()
            || self.lease_id.trim().is_empty()
            || self.lease_expires_at_ms == 0
            || self.worktree_identity.trim().is_empty()
        {
            return Err(BridgeError::Packet(
                "trusted execution identity and lease binding are required".into(),
            ));
        }
        if self.objective.trim().is_empty() {
            return Err(BridgeError::Packet("objective is required".into()));
        }
        self.authorized_action.validate()?;
        if self.authorized_action.expires_at_ms != self.lease_expires_at_ms {
            return Err(BridgeError::Authority(
                "authorized action expiry is not bound to the active lease".into(),
            ));
        }
        if self
            .sealed_requirement_ids
            .iter()
            .any(|id| !valid_requirement_id(id))
        {
            return Err(BridgeError::Packet("invalid sealed requirement ID".into()));
        }
        let root =
            fs::canonicalize(allowed_root).map_err(|e| BridgeError::Workspace(e.to_string()))?;
        let workspace =
            fs::canonicalize(&self.workspace).map_err(|e| BridgeError::Workspace(e.to_string()))?;
        if !workspace.starts_with(root) {
            return Err(BridgeError::Workspace(
                "workspace is outside allowed root".into(),
            ));
        }
        if serde_json::to_vec(self)
            .map_err(|e| BridgeError::Packet(e.to_string()))?
            .len()
            > MAX_PACKET_BYTES
        {
            return Err(BridgeError::Packet("packet exceeds bounded size".into()));
        }
        Ok(())
    }
    pub fn to_json(&self) -> Result<String, BridgeError> {
        serde_json::to_string(self).map_err(|e| BridgeError::Packet(e.to_string()))
    }

    pub fn binding_digest(&self) -> Result<String, BridgeError> {
        let mut view = self.clone();
        view.task_packet_digest.clear();
        Ok(sha256(
            &serde_json::to_vec(&view).map_err(|error| BridgeError::Packet(error.to_string()))?,
        ))
    }
}

fn valid_requirement_id(id: &str) -> bool {
    let b = id.as_bytes();
    let compact_spec_id = b.len() == 4
        && (b'A'..=b'L').contains(&b[0])
        && b[1] == b'-'
        && b[2].is_ascii_digit()
        && b[3].is_ascii_digit()
        && (1..=12).contains(&id[2..].parse::<u8>().unwrap_or(0));
    let stable_requirement_id = id.strip_prefix("requirement_").is_some_and(|digest| {
        (16..=64).contains(&digest.len()) && digest.bytes().all(|byte| byte.is_ascii_hexdigit())
    });
    compact_spec_id || stable_requirement_id
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProcessCommand {
    pub program: PathBuf,
    pub args: Vec<OsString>,
    pub current_dir: PathBuf,
    pub environment: Vec<(OsString, OsString)>,
    pub clear_environment: bool,
}

impl ProcessCommand {
    pub fn new(program: impl Into<PathBuf>, current_dir: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
            args: vec![],
            current_dir: current_dir.into(),
            environment: vec![],
            clear_environment: false,
        }
    }
    pub fn arg(mut self, arg: impl AsRef<OsStr>) -> Self {
        self.args.push(arg.as_ref().to_os_string());
        self
    }
    pub fn environment(mut self, key: impl AsRef<OsStr>, value: impl AsRef<OsStr>) -> Self {
        self.environment
            .push((key.as_ref().to_os_string(), value.as_ref().to_os_string()));
        self
    }
    pub fn minimal_environment(mut self) -> Self {
        self.clear_environment = true;
        self
    }
    fn validate(&self, root: &Path) -> Result<(), BridgeError> {
        if self.program.as_os_str().is_empty() {
            return Err(BridgeError::Process("program is required".into()));
        }
        let root = fs::canonicalize(root).map_err(|e| BridgeError::Workspace(e.to_string()))?;
        let cwd = fs::canonicalize(&self.current_dir)
            .map_err(|e| BridgeError::Workspace(e.to_string()))?;
        if !cwd.starts_with(root) {
            return Err(BridgeError::Workspace(
                "process cwd is outside allowed root".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StopPolicy {
    pub timeout: Duration,
    pub allow_force_kill: bool,
}
impl Default for StopPolicy {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(2),
            allow_force_kill: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProcessIdentity {
    pub pid: u32,
    pub executable: String,
    pub started_at_ms: i128,
    pub command_digest: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProcessResult {
    pub identity: ProcessIdentity,
    pub state: ProcessState,
    pub exit_code: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub output_overflow: bool,
    pub started_at_ms: i128,
    pub ended_at_ms: i128,
    /// The operating-system process exit boundary, captured before pipe
    /// draining. Authority decisions must use this value, never the later
    /// diagnostic collection boundary.
    #[serde(default)]
    pub process_exited_at_ms: i128,
    /// When stdout/stderr draining completed. This is diagnostic timing only
    /// and can never extend execution authority.
    #[serde(default)]
    pub output_collected_at_ms: i128,
    pub forced: bool,
    /// Artifacts captured by the adapter after the owned process exited.
    /// This is observation only; Rust still decides whether the process may
    /// authorize task completion.
    #[serde(default)]
    pub artifact_paths: Vec<PathBuf>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ProcessState {
    RunningWithProgress,
    RunningIdle,
    ExitedSuccess,
    ExitedFailure,
    TimedOut,
    Cancelled,
    BridgeProtocolFailure,
}

pub struct SupervisedProcess {
    child: Child,
    identity: ProcessIdentity,
    stdout: Option<Receiver<io::Result<Vec<u8>>>>,
    stderr: Option<Receiver<io::Result<Vec<u8>>>>,
    limit: usize,
    started_at_ms: i128,
    last_progress_ms: Arc<AtomicU64>,
}

impl Drop for SupervisedProcess {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            force_kill_owned_process_tree(self.identity.pid);
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

impl SupervisedProcess {
    pub fn start(spec: &ProcessCommand, root: &Path, limit: usize) -> Result<Self, BridgeError> {
        if limit == 0 || limit > MAX_OUTPUT_BYTES {
            return Err(BridgeError::Process(
                "output limit outside safe bound".into(),
            ));
        }
        spec.validate(root)?;
        let mut command = hidden_command(&spec.program);
        if spec.clear_environment {
            command.env_clear();
        }
        command
            .args(&spec.args)
            .current_dir(&spec.current_dir)
            .envs(spec.environment.iter().map(|(k, v)| (k, v)))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let observable_args = spec
            .args
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        let mut child = command
            .spawn()
            .map_err(|e| BridgeError::Process(format!("spawn: {e}")))?;
        let started_at_ms = process_creation_time_ms(child.id()).unwrap_or_else(now_ms);
        let command_digest = observable_process_command_digest(&spec.program, &observable_args);
        let last_progress_ms = Arc::new(AtomicU64::new(now_ms() as u64));
        let stdout = child
            .stdout
            .take()
            .map(|r| reader_thread(r, limit, Arc::clone(&last_progress_ms)));
        let stderr = child
            .stderr
            .take()
            .map(|r| reader_thread(r, limit, Arc::clone(&last_progress_ms)));
        Ok(Self {
            identity: ProcessIdentity {
                pid: child.id(),
                executable: spec.program.display().to_string(),
                started_at_ms,
                command_digest,
            },
            child,
            stdout,
            stderr,
            limit,
            started_at_ms: now_ms(),
            last_progress_ms,
        })
    }
    pub fn identity(&self) -> &ProcessIdentity {
        &self.identity
    }
    pub fn progress_state(&self, idle_timeout: Duration) -> ProcessState {
        let last_progress = self.last_progress_ms.load(Ordering::Acquire);
        let now = now_ms() as u64;
        if now.saturating_sub(last_progress) >= idle_timeout.as_millis() as u64 {
            ProcessState::RunningIdle
        } else {
            ProcessState::RunningWithProgress
        }
    }
    pub fn wait(mut self) -> Result<ProcessResult, BridgeError> {
        let status = self
            .child
            .wait()
            .map_err(|e| BridgeError::Process(format!("wait: {e}")))?;
        self.collect(status, false, None)
    }
    pub fn wait_with_timeout(self, timeout: Duration) -> Result<ProcessResult, BridgeError> {
        self.wait_with_monitor(timeout, Duration::MAX, None)
    }
    pub fn wait_with_monitor(
        mut self,
        timeout: Duration,
        idle_timeout: Duration,
        cancel: Option<&AtomicU64>,
    ) -> Result<ProcessResult, BridgeError> {
        let deadline = SystemTime::now() + timeout;
        loop {
            if let Some(status) = self
                .child
                .try_wait()
                .map_err(|e| BridgeError::Process(format!("poll: {e}")))?
            {
                return self.collect(status, false, None);
            }
            if cancel.is_some_and(|flag| flag.load(Ordering::Acquire) != 0) {
                force_kill_owned_process_tree(self.identity.pid);
                self.child
                    .kill()
                    .map_err(|e| BridgeError::Process(format!("cancel stop: {e}")))?;
                let status = self
                    .child
                    .wait()
                    .map_err(|e| BridgeError::Process(format!("wait after cancel: {e}")))?;
                return self.collect(status, true, Some(ProcessState::Cancelled));
            }
            if SystemTime::now() >= deadline {
                force_kill_owned_process_tree(self.identity.pid);
                self.child
                    .kill()
                    .map_err(|e| BridgeError::Process(format!("timeout force stop: {e}")))?;
                let status = self
                    .child
                    .wait()
                    .map_err(|e| BridgeError::Process(format!("wait after timeout: {e}")))?;
                return self.collect(status, true, Some(ProcessState::TimedOut));
            }
            if idle_timeout != Duration::MAX {
                let last_progress = self.last_progress_ms.load(Ordering::Acquire);
                let now = now_ms() as u64;
                if now.saturating_sub(last_progress) >= idle_timeout.as_millis() as u64 {
                    force_kill_owned_process_tree(self.identity.pid);
                    self.child
                        .kill()
                        .map_err(|e| BridgeError::Process(format!("idle timeout stop: {e}")))?;
                    let status = self.child.wait().map_err(|e| {
                        BridgeError::Process(format!("wait after idle timeout: {e}"))
                    })?;
                    return self.collect(status, true, Some(ProcessState::TimedOut));
                }
            }
            thread::sleep(Duration::from_millis(50));
        }
    }
    pub fn stop(&mut self, policy: StopPolicy) -> Result<ProcessResult, BridgeError> {
        let deadline = SystemTime::now() + policy.timeout;
        loop {
            if let Some(status) = self
                .child
                .try_wait()
                .map_err(|e| BridgeError::Process(format!("poll: {e}")))?
            {
                return self.collect(status, false, None);
            }
            if SystemTime::now() >= deadline {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        if !policy.allow_force_kill {
            return Err(BridgeError::StopTimeout);
        }
        force_kill_owned_process_tree(self.identity.pid);
        self.child
            .kill()
            .map_err(|e| BridgeError::Process(format!("force stop: {e}")))?;
        let status = self
            .child
            .wait()
            .map_err(|e| BridgeError::Process(format!("wait after force stop: {e}")))?;
        self.collect(status, true, Some(ProcessState::Cancelled))
    }
    fn collect(
        &mut self,
        status: ExitStatus,
        forced: bool,
        override_state: Option<ProcessState>,
    ) -> Result<ProcessResult, BridgeError> {
        let process_exited_at_ms = now_ms();
        let stdout = receive(self.stdout.take())?;
        let stderr = receive(self.stderr.take())?;
        let output_collected_at_ms = now_ms();
        let overflow = stdout.len() > self.limit || stderr.len() > self.limit;
        let trim = |mut v: Vec<u8>| {
            if v.len() > self.limit {
                v.truncate(self.limit);
            }
            v
        };
        Ok(ProcessResult {
            identity: self.identity.clone(),
            state: override_state.unwrap_or(if status.success() {
                ProcessState::ExitedSuccess
            } else {
                ProcessState::ExitedFailure
            }),
            exit_code: status.code(),
            stdout: trim(stdout),
            stderr: trim(stderr),
            output_overflow: overflow,
            started_at_ms: self.started_at_ms,
            ended_at_ms: process_exited_at_ms,
            forced,
            process_exited_at_ms,
            output_collected_at_ms,
            artifact_paths: Vec::new(),
        })
    }
}

fn reader_thread<R: Read + Send + 'static>(
    mut reader: R,
    limit: usize,
    last_progress_ms: Arc<AtomicU64>,
) -> Receiver<io::Result<Vec<u8>>> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut output = Vec::new();
        let mut buf = [0_u8; 8192];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    last_progress_ms.store(now_ms() as u64, Ordering::Release);
                    let remaining = limit.saturating_add(1).saturating_sub(output.len());
                    output.extend_from_slice(&buf[..n.min(remaining)]);
                }
                Err(e) => {
                    let _ = tx.send(Err(e));
                    return;
                }
            }
        }
        let _ = tx.send(Ok(output));
    });
    rx
}

fn receive(rx: Option<Receiver<io::Result<Vec<u8>>>>) -> Result<Vec<u8>, BridgeError> {
    rx.map(|r| {
        r.recv()
            .map_err(|e| BridgeError::Process(e.to_string()))
            .and_then(|v| v.map_err(|e| BridgeError::Process(e.to_string())))
    })
    .unwrap_or_else(|| Ok(Vec::new()))
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EventType {
    ExecutionStarted,
    AgentMessage,
    ToolStarted,
    ToolFinished,
    ArtifactReported,
    Warning,
    Error,
    StopRequested,
    ProcessExited,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NormalizedEvent {
    pub event_id: String,
    pub execution_id: String,
    pub timestamp_ms: i128,
    pub source: String,
    pub event_type: EventType,
    pub sequence: u64,
    pub payload: Value,
    pub raw_sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RawOutputRecord {
    pub execution_id: String,
    pub source: String,
    pub captured_at_ms: i128,
    pub bytes: Vec<u8>,
    pub sha256: String,
}

#[derive(Debug, Default)]
pub struct EventNormalizer {
    ids: BTreeSet<String>,
    sequences: BTreeSet<(String, u64)>,
}
impl EventNormalizer {
    pub fn normalize(
        &mut self,
        execution_id: &str,
        source: &str,
        raw: &[u8],
    ) -> Result<NormalizedEvent, BridgeError> {
        let value: Value =
            serde_json::from_slice(raw).map_err(|e| BridgeError::Event(e.to_string()))?;
        let object = value
            .as_object()
            .ok_or_else(|| BridgeError::Event("event must be an object".into()))?;
        let id: String = object
            .get("event_id")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| BridgeError::Event("event_id required".into()))?
            .into();
        let sequence = object
            .get("sequence")
            .and_then(Value::as_u64)
            .ok_or_else(|| BridgeError::Event("sequence required".into()))?;
        let event_type = object
            .get("type")
            .cloned()
            .ok_or_else(|| BridgeError::Event("type required".into()))
            .and_then(|v| {
                serde_json::from_value(v).map_err(|e| BridgeError::Event(e.to_string()))
            })?;
        if !self.ids.insert(id.clone()) {
            return Err(BridgeError::DuplicateEvent(id));
        }
        if !self.sequences.insert((execution_id.into(), sequence)) {
            return Err(BridgeError::DuplicateSequence(sequence));
        }
        Ok(NormalizedEvent {
            event_id: id,
            execution_id: execution_id.into(),
            timestamp_ms: object
                .get("timestamp_ms")
                .and_then(Value::as_i64)
                .map(i128::from)
                .unwrap_or_else(now_ms),
            source: source.into(),
            event_type,
            sequence,
            payload: object.get("payload").cloned().unwrap_or(Value::Null),
            raw_sha256: sha256(raw),
        })
    }
}
pub fn raw_output(execution_id: &str, source: &str, bytes: Vec<u8>) -> RawOutputRecord {
    RawOutputRecord {
        execution_id: execution_id.into(),
        source: source.into(),
        captured_at_ms: now_ms(),
        sha256: sha256(&bytes),
        bytes,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExecutionSession {
    pub contract_version: String,
    pub execution_id: String,
    pub mission_id: String,
    pub task_id: String,
    pub workspace: String,
    pub adapter_version: String,
    pub antigravity_version: Option<String>,
    pub started_at_ms: i128,
    pub status: ExecutionStatus,
    pub process_identity: Option<ProcessIdentity>,
    pub environment_fingerprint: String,
    pub requested_stop: bool,
    pub exit_state: Option<ExitState>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExitState {
    pub exit_code: Option<i32>,
    pub exited_at_ms: i128,
    pub forced: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ExecutionStatus {
    Created,
    Starting,
    Running,
    StopRequested,
    Exited,
    Failed,
    Blocked,
    Lost,
}

impl ExecutionSession {
    pub fn new(packet: &TaskPacket, version: Option<String>, fingerprint: String) -> Self {
        Self {
            contract_version: SESSION_VERSION.into(),
            execution_id: format!("exec-{}-{}", std::process::id(), now_ms()),
            mission_id: packet.mission_id.clone(),
            task_id: packet.task_id.clone(),
            workspace: packet.workspace.display().to_string(),
            adapter_version: ADAPTER_VERSION.into(),
            antigravity_version: version,
            started_at_ms: now_ms(),
            status: ExecutionStatus::Created,
            process_identity: None,
            environment_fingerprint: fingerprint,
            requested_stop: false,
            exit_state: None,
        }
    }
    pub fn transition(&mut self, next: ExecutionStatus) -> Result<(), BridgeError> {
        let valid = matches!(
            (self.status, next),
            (ExecutionStatus::Created, ExecutionStatus::Starting)
                | (ExecutionStatus::Starting, ExecutionStatus::Running)
                | (ExecutionStatus::Starting, ExecutionStatus::Failed)
                | (ExecutionStatus::Running, ExecutionStatus::StopRequested)
                | (ExecutionStatus::Running, ExecutionStatus::Exited)
                | (ExecutionStatus::Running, ExecutionStatus::Failed)
                | (ExecutionStatus::Running, ExecutionStatus::Lost)
                | (ExecutionStatus::StopRequested, ExecutionStatus::Exited)
                | (ExecutionStatus::StopRequested, ExecutionStatus::Failed)
                | (ExecutionStatus::StopRequested, ExecutionStatus::Lost)
        );
        if !valid {
            return Err(BridgeError::Transition {
                from: self.status,
                to: next,
            });
        }
        self.status = next;
        if next == ExecutionStatus::StopRequested {
            self.requested_stop = true;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BridgeManifest {
    pub schema_version: u32,
    pub adapter_version: String,
    pub expected_executable: String,
    pub expected_sha256: Option<String>,
    pub supported_antigravity_versions: Vec<String>,
    pub hook_protocol: String,
    pub signing_status: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BridgeIdentity {
    pub canonical_path: String,
    pub sha256: String,
    pub adapter_version: String,
    pub signing_status: String,
}

pub fn validate_bridge_identity(
    path: &Path,
    manifest: &BridgeManifest,
) -> Result<BridgeIdentity, BridgeError> {
    let canonical = fs::canonicalize(path).map_err(|e| BridgeError::Bridge(e.to_string()))?;
    if canonical.file_name().and_then(OsStr::to_str) != Some(manifest.expected_executable.as_str())
    {
        return Err(BridgeError::Bridge("unexpected bridge path".into()));
    }
    let digest = sha256(&fs::read(&canonical).map_err(|e| BridgeError::Bridge(e.to_string()))?);
    if manifest
        .expected_sha256
        .as_deref()
        .is_some_and(|expected| expected != digest)
    {
        return Err(BridgeError::Bridge("bridge hash mismatch".into()));
    }
    Ok(BridgeIdentity {
        canonical_path: canonical.display().to_string(),
        sha256: digest,
        adapter_version: manifest.adapter_version.clone(),
        signing_status: manifest.signing_status.clone(),
    })
}

pub trait AntigravityAdapter {
    fn detect(&self) -> CompatibilityReport;
    fn validate_version(&self, report: &CompatibilityReport) -> Result<(), BridgeError>;
    fn install_or_validate_bridge(
        &self,
        path: &Path,
        manifest: &BridgeManifest,
    ) -> Result<BridgeIdentity, BridgeError>;
    fn create_execution(
        &mut self,
        packet: TaskPacket,
        root: &Path,
    ) -> Result<ExecutionSession, BridgeError>;
    fn send_task(&mut self, execution_id: &str, packet: &TaskPacket) -> Result<(), BridgeError>;
    fn process_identity(&self, execution_id: &str) -> Result<ProcessIdentity, BridgeError>;
    fn stream_events(&mut self, execution_id: &str) -> Result<Vec<NormalizedEvent>, BridgeError>;
    fn request_stop(&mut self, execution_id: &str) -> Result<(), BridgeError>;
    fn collect_artifacts(&mut self, execution_id: &str) -> Result<Vec<PathBuf>, BridgeError>;
    /// Wait for and return the adapter-owned process result. Production
    /// adapters must obtain this from the supervised child process; callers
    /// must never manufacture a successful result.
    fn wait_for_exit(&mut self, execution_id: &str) -> Result<ProcessResult, BridgeError> {
        let _ = execution_id;
        Err(BridgeError::Process(
            "adapter did not expose process exit".into(),
        ))
    }
    /// Wait under the caller's already-derived authority boundary. The
    /// default preserves compatibility for test adapters; production adapters
    /// override it and enforce the bound while supervising the owned process.
    fn wait_for_exit_with_timeouts(
        &mut self,
        execution_id: &str,
        timeout: Duration,
        idle_timeout: Duration,
    ) -> Result<ProcessResult, BridgeError> {
        let _ = (timeout, idle_timeout);
        self.wait_for_exit(execution_id)
    }
    fn reconcile_exit(
        &mut self,
        execution_id: &str,
        result: ProcessResult,
    ) -> Result<ExecutionSession, BridgeError>;
}

#[derive(Debug, Default)]
pub struct MockAdapter {
    pub report: Option<CompatibilityReport>,
    sessions: Vec<ExecutionSession>,
}
impl MockAdapter {
    pub fn supported() -> Self {
        Self {
            report: Some(CompatibilityReport {
                executable_present: true,
                executable_path: Some("agy".into()),
                version: Some("1.0.0".into()),
                compatibility: Compatibility::Supported,
                cli_invocation_capability: true,
                plugin_hook_capability: false,
                environment: "test".into(),
                platform: env::consts::OS.into(),
                detected_at_ms: now_ms(),
                detail: "deterministic mock".into(),
            }),
            sessions: vec![],
        }
    }
}
impl AntigravityAdapter for MockAdapter {
    fn detect(&self) -> CompatibilityReport {
        self.report.clone().unwrap_or_else(|| {
            report(
                None,
                None,
                false,
                false,
                &CompatibilityRegistry::bundled().unwrap(),
            )
        })
    }
    fn validate_version(&self, report: &CompatibilityReport) -> Result<(), BridgeError> {
        report
            .sealed_execution_allowed()
            .then_some(())
            .ok_or(BridgeError::Incompatible)
    }
    fn install_or_validate_bridge(
        &self,
        path: &Path,
        manifest: &BridgeManifest,
    ) -> Result<BridgeIdentity, BridgeError> {
        validate_bridge_identity(path, manifest)
    }
    fn create_execution(
        &mut self,
        packet: TaskPacket,
        root: &Path,
    ) -> Result<ExecutionSession, BridgeError> {
        packet.validate(root)?;
        let detected = self.detect();
        self.validate_version(&detected)?;
        let session = ExecutionSession::new(&packet, detected.version, "mock-environment".into());
        self.sessions.push(session.clone());
        Ok(session)
    }
    fn send_task(&mut self, id: &str, _packet: &TaskPacket) -> Result<(), BridgeError> {
        let s = self
            .sessions
            .iter_mut()
            .find(|s| s.execution_id == id)
            .ok_or_else(|| BridgeError::Unknown(id.into()))?;
        s.process_identity = Some(ProcessIdentity {
            pid: 0,
            executable: "mock-adapter".into(),
            started_at_ms: now_ms(),
            command_digest: sha256(b"mock-adapter"),
        });
        s.transition(ExecutionStatus::Starting)?;
        s.transition(ExecutionStatus::Running)
    }
    fn process_identity(&self, id: &str) -> Result<ProcessIdentity, BridgeError> {
        self.sessions
            .iter()
            .find(|session| session.execution_id == id)
            .and_then(|session| session.process_identity.clone())
            .ok_or_else(|| BridgeError::Process("mock process was not started".into()))
    }
    fn stream_events(&mut self, id: &str) -> Result<Vec<NormalizedEvent>, BridgeError> {
        if self.sessions.iter().any(|s| s.execution_id == id) {
            Ok(vec![])
        } else {
            Err(BridgeError::Unknown(id.into()))
        }
    }
    fn request_stop(&mut self, id: &str) -> Result<(), BridgeError> {
        self.sessions
            .iter_mut()
            .find(|s| s.execution_id == id)
            .ok_or_else(|| BridgeError::Unknown(id.into()))?
            .transition(ExecutionStatus::StopRequested)
    }
    fn collect_artifacts(&mut self, id: &str) -> Result<Vec<PathBuf>, BridgeError> {
        if self.sessions.iter().any(|s| s.execution_id == id) {
            Ok(vec![])
        } else {
            Err(BridgeError::Unknown(id.into()))
        }
    }
    fn wait_for_exit(&mut self, id: &str) -> Result<ProcessResult, BridgeError> {
        let session = self
            .sessions
            .iter()
            .find(|s| s.execution_id == id)
            .ok_or_else(|| BridgeError::Unknown(id.into()))?;
        Ok(ProcessResult {
            identity: ProcessIdentity {
                pid: 0,
                executable: "mock-adapter".into(),
                started_at_ms: session.started_at_ms,
                command_digest: sha256(b"mock-adapter"),
            },
            state: ProcessState::ExitedSuccess,
            exit_code: Some(0),
            stdout: vec![],
            stderr: vec![],
            output_overflow: false,
            started_at_ms: session.started_at_ms,
            ended_at_ms: now_ms(),
            process_exited_at_ms: now_ms(),
            output_collected_at_ms: now_ms(),
            forced: false,
            artifact_paths: Vec::new(),
        })
    }
    fn reconcile_exit(
        &mut self,
        id: &str,
        result: ProcessResult,
    ) -> Result<ExecutionSession, BridgeError> {
        let s = self
            .sessions
            .iter_mut()
            .find(|s| s.execution_id == id)
            .ok_or_else(|| BridgeError::Unknown(id.into()))?;
        s.process_identity = Some(result.identity);
        s.exit_state = Some(ExitState {
            exit_code: result.exit_code,
            exited_at_ms: result.ended_at_ms,
            forced: result.forced,
        });
        s.transition(if result.exit_code == Some(0) {
            ExecutionStatus::Exited
        } else {
            ExecutionStatus::Failed
        })?;
        Ok(s.clone())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PluginPackageFile {
    pub relative_path: String,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PluginPackageManifest {
    pub schema_version: u32,
    pub package_id: String,
    pub package_version: String,
    pub protocol_version: String,
    pub target_triple: String,
    pub supported_antigravity_versions: Vec<String>,
    pub files: Vec<PluginPackageFile>,
    pub key_id: String,
    pub signature: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PluginPackageVerification {
    pub package_id: String,
    pub package_version: String,
    pub target_triple: String,
    pub files: Vec<String>,
    pub verified: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PluginInstallResult {
    pub package_id: String,
    pub package_version: String,
    pub target: String,
    pub files: Vec<String>,
    pub verified: bool,
}

#[derive(Serialize)]
struct PluginSigningView<'a> {
    schema_version: u32,
    package_id: &'a str,
    package_version: &'a str,
    protocol_version: &'a str,
    target_triple: &'a str,
    supported_antigravity_versions: &'a [String],
    files: &'a [PluginPackageFile],
    key_id: &'a str,
}

pub fn plugin_signing_bytes(manifest: &PluginPackageManifest) -> Result<Vec<u8>, BridgeError> {
    serde_json::to_vec(&PluginSigningView {
        schema_version: manifest.schema_version,
        package_id: &manifest.package_id,
        package_version: &manifest.package_version,
        protocol_version: &manifest.protocol_version,
        target_triple: &manifest.target_triple,
        supported_antigravity_versions: &manifest.supported_antigravity_versions,
        files: &manifest.files,
        key_id: &manifest.key_id,
    })
    .map_err(|error| BridgeError::Plugin(error.to_string()))
}

pub const PLUGIN_PACKAGE_MANIFEST_FILE: &str = "package-manifest.json";
pub const RELINTOR_PLUGIN_NAME: &str = "relintor-authority";

#[allow(unreachable_code)]
pub fn runtime_target_triple() -> &'static str {
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    {
        return "x86_64-pc-windows-msvc";
    }
    #[cfg(all(target_os = "windows", target_arch = "aarch64"))]
    {
        return "aarch64-pc-windows-msvc";
    }
    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    {
        return "x86_64-apple-darwin";
    }
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    {
        return "aarch64-apple-darwin";
    }
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    {
        return "x86_64-unknown-linux-gnu";
    }
    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    {
        return "aarch64-unknown-linux-gnu";
    }
    "unsupported-target"
}

pub fn trusted_key_id(key: &ed25519_dalek::VerifyingKey) -> String {
    sha256(key.as_bytes())
}

pub fn verifying_key_from_base64(value: &str) -> Result<ed25519_dalek::VerifyingKey, BridgeError> {
    let bytes = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, value.trim())
        .map_err(|_| BridgeError::Plugin("trusted bridge public key is not base64".into()))?;
    let bytes: [u8; 32] = bytes
        .try_into()
        .map_err(|_| BridgeError::Plugin("trusted bridge public key has invalid length".into()))?;
    ed25519_dalek::VerifyingKey::from_bytes(&bytes)
        .map_err(|_| BridgeError::Plugin("trusted bridge public key is invalid".into()))
}

fn version_supported(version: Option<&str>, supported: &[String]) -> bool {
    version.is_none_or(|version| {
        supported
            .iter()
            .any(|prefix| !prefix.trim().is_empty() && version.starts_with(prefix))
    })
}

fn is_link_or_reparse(path: &Path) -> Result<bool, BridgeError> {
    let metadata =
        fs::symlink_metadata(path).map_err(|error| BridgeError::Plugin(error.to_string()))?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        Ok(metadata.file_type().is_symlink() || metadata.file_attributes() & 0x400 != 0)
    }
    #[cfg(not(windows))]
    {
        Ok(metadata.file_type().is_symlink())
    }
}

fn collect_package_files(
    root: &Path,
    current: &Path,
    out: &mut Vec<String>,
) -> Result<(), BridgeError> {
    for entry in fs::read_dir(current).map_err(|error| BridgeError::Plugin(error.to_string()))? {
        let entry = entry.map_err(|error| BridgeError::Plugin(error.to_string()))?;
        let path = entry.path();
        let metadata =
            fs::symlink_metadata(&path).map_err(|error| BridgeError::Plugin(error.to_string()))?;
        if is_link_or_reparse(&path)? {
            return Err(BridgeError::Plugin(
                "plugin package contains a symlink or reparse point".into(),
            ));
        }
        if metadata.is_dir() {
            collect_package_files(root, &path, out)?;
        } else if metadata.is_file() {
            let relative = path
                .strip_prefix(root)
                .map_err(|_| BridgeError::Plugin("plugin package path is invalid".into()))?
                .to_string_lossy()
                .replace('\\', "/");
            out.push(relative);
        } else {
            return Err(BridgeError::Plugin(
                "plugin package contains a non-regular entry".into(),
            ));
        }
    }
    Ok(())
}

fn validate_plugin_layout(
    root: &Path,
    manifest: &PluginPackageManifest,
) -> Result<(), BridgeError> {
    let listed = manifest
        .files
        .iter()
        .map(|file| file.relative_path.replace('\\', "/"))
        .collect::<HashSet<_>>();
    if !listed.contains("plugin.json") || !listed.contains("hooks.json") {
        return Err(BridgeError::Plugin(
            "signed Antigravity package must contain plugin.json and hooks.json".into(),
        ));
    }
    let plugin: Value = serde_json::from_slice(
        &fs::read(root.join("plugin.json"))
            .map_err(|error| BridgeError::Plugin(error.to_string()))?,
    )
    .map_err(|error| BridgeError::Plugin(format!("plugin.json is invalid: {error}")))?;
    if plugin.get("name").and_then(Value::as_str) != Some(RELINTOR_PLUGIN_NAME) {
        return Err(BridgeError::Plugin(
            "signed Antigravity package has the wrong plugin name".into(),
        ));
    }
    let hooks: Value = serde_json::from_slice(
        &fs::read(root.join("hooks.json"))
            .map_err(|error| BridgeError::Plugin(error.to_string()))?,
    )
    .map_err(|error| BridgeError::Plugin(format!("hooks.json is invalid: {error}")))?;
    if !hooks
        .to_string()
        .contains("RELINTOR_ANTIGRAVITY_BRIDGE_PATH")
    {
        return Err(BridgeError::Plugin(
            "signed Antigravity hooks do not invoke the approved bridge".into(),
        ));
    }
    Ok(())
}

pub fn verify_plugin_package(
    package_root: &Path,
    manifest: &PluginPackageManifest,
    trusted_key: &ed25519_dalek::VerifyingKey,
    antigravity_version: Option<&str>,
) -> Result<PluginPackageVerification, BridgeError> {
    if is_link_or_reparse(package_root)? {
        return Err(BridgeError::Plugin(
            "plugin package root is a link or reparse point".into(),
        ));
    }
    if manifest.schema_version != 1
        || manifest.package_id != "com.relintor.antigravity"
        || manifest.package_version.trim().is_empty()
        || manifest.protocol_version != "relintor-antigravity-hooks-v1"
        || manifest.target_triple != runtime_target_triple()
        || manifest.files.is_empty()
        || !version_supported(
            antigravity_version,
            &manifest.supported_antigravity_versions,
        )
    {
        return Err(BridgeError::Plugin(
            "signed Antigravity package metadata is incompatible".into(),
        ));
    }
    if trusted_key_id(trusted_key) != manifest.key_id {
        return Err(BridgeError::Plugin(
            "plugin signer identity mismatch".into(),
        ));
    }
    let signature_bytes = base64::Engine::decode(
        &base64::engine::general_purpose::STANDARD,
        &manifest.signature,
    )
    .map_err(|_| BridgeError::Plugin("plugin signature is not base64".into()))?;
    let signature = ed25519_dalek::Signature::from_slice(&signature_bytes)
        .map_err(|_| BridgeError::Plugin("plugin signature length is invalid".into()))?;
    trusted_key
        .verify(&plugin_signing_bytes(manifest)?, &signature)
        .map_err(|_| BridgeError::Plugin("plugin signature verification failed".into()))?;
    let source = fs::canonicalize(package_root)
        .map_err(|error| BridgeError::Plugin(format!("package root: {error}")))?;
    if !source.is_dir() {
        return Err(BridgeError::Plugin(
            "plugin package root is not a directory".into(),
        ));
    }
    let on_disk_manifest: PluginPackageManifest = serde_json::from_slice(
        &fs::read(source.join(PLUGIN_PACKAGE_MANIFEST_FILE))
            .map_err(|error| BridgeError::Plugin(error.to_string()))?,
    )
    .map_err(|error| BridgeError::Plugin(format!("package manifest is invalid: {error}")))?;
    if &on_disk_manifest != manifest {
        return Err(BridgeError::Plugin(
            "package manifest does not match the signed manifest".into(),
        ));
    }
    let mut actual = Vec::new();
    collect_package_files(&source, &source, &mut actual)?;
    let actual = actual.into_iter().collect::<HashSet<_>>();
    let expected = manifest
        .files
        .iter()
        .map(|file| file.relative_path.replace('\\', "/"))
        .chain(std::iter::once(PLUGIN_PACKAGE_MANIFEST_FILE.into()))
        .collect::<HashSet<_>>();
    if actual != expected {
        return Err(BridgeError::Plugin(
            "plugin package file set is not exact".into(),
        ));
    }
    let mut seen = HashSet::new();
    for file in &manifest.files {
        let relative = Path::new(&file.relative_path);
        if relative.as_os_str().is_empty()
            || relative.is_absolute()
            || relative
                .components()
                .any(|component| matches!(component, std::path::Component::ParentDir))
            || !seen.insert(file.relative_path.replace('\\', "/"))
        {
            return Err(BridgeError::Plugin(
                "plugin file list contains an unsafe or duplicate path".into(),
            ));
        }
        let bytes = fs::read(source.join(relative))
            .map_err(|error| BridgeError::Plugin(error.to_string()))?;
        if sha256(&bytes) != file.sha256 {
            return Err(BridgeError::Plugin("plugin file digest mismatch".into()));
        }
    }
    validate_plugin_layout(&source, manifest)?;
    Ok(PluginPackageVerification {
        package_id: manifest.package_id.clone(),
        package_version: manifest.package_version.clone(),
        target_triple: manifest.target_triple.clone(),
        files: manifest
            .files
            .iter()
            .map(|file| file.relative_path.clone())
            .collect(),
        verified: true,
    })
}

pub fn read_plugin_package_manifest(
    package_root: &Path,
) -> Result<PluginPackageManifest, BridgeError> {
    serde_json::from_slice(
        &fs::read(package_root.join(PLUGIN_PACKAGE_MANIFEST_FILE))
            .map_err(|error| BridgeError::Plugin(error.to_string()))?,
    )
    .map_err(|error| BridgeError::Plugin(format!("plugin package manifest is invalid: {error}")))
}

pub fn verify_installed_plugin(
    install_root: &Path,
    trusted_key: &ed25519_dalek::VerifyingKey,
    antigravity_version: Option<&str>,
) -> Result<PluginPackageVerification, BridgeError> {
    let manifest = read_plugin_package_manifest(install_root)?;
    verify_plugin_package(install_root, &manifest, trusted_key, antigravity_version)
}

pub fn verify_installed_bridge(
    install_root: &Path,
    trusted_key: &ed25519_dalek::VerifyingKey,
    antigravity_version: Option<&str>,
) -> Result<BridgeIdentity, BridgeError> {
    let package = verify_installed_plugin(install_root, trusted_key, antigravity_version)?;
    if !package.verified {
        return Err(BridgeError::Plugin(
            "installed bridge package is not verified".into(),
        ));
    }
    let manifest: BridgeManifest = serde_json::from_slice(
        &fs::read(install_root.join("bridge-manifest.json"))
            .map_err(|error| BridgeError::Plugin(error.to_string()))?,
    )
    .map_err(|error| BridgeError::Plugin(format!("bridge identity: {error}")))?;
    if manifest.signing_status != "verified" || manifest.expected_sha256.is_none() {
        return Err(BridgeError::Plugin(
            "bridge identity is not signed with an exact digest".into(),
        ));
    }
    validate_bridge_identity(&install_root.join(&manifest.expected_executable), &manifest)
}

pub fn install_verified_plugin(
    package_root: &Path,
    install_root: &Path,
    manifest: &PluginPackageManifest,
    trusted_key: &ed25519_dalek::VerifyingKey,
) -> Result<PluginInstallResult, BridgeError> {
    verify_plugin_package(package_root, manifest, trusted_key, None)?;
    let source = fs::canonicalize(package_root)
        .map_err(|error| BridgeError::Plugin(format!("package root: {error}")))?;
    if !install_root.is_absolute() || install_root.starts_with(&source) {
        return Err(BridgeError::Workspace(
            "plugin install root must be absolute and outside the package".into(),
        ));
    }
    let staging = install_root.with_extension(format!("staging-{}", std::process::id()));
    if staging.exists() {
        return Err(BridgeError::Plugin(
            "plugin staging target already exists".into(),
        ));
    }
    let replacing = install_root.exists();
    let backup = install_root.with_extension(format!("previous-{}", std::process::id()));
    if replacing && (is_link_or_reparse(install_root)? || backup.exists()) {
        return Err(BridgeError::Plugin(
            "plugin replacement target is unsafe or already has a rollback copy".into(),
        ));
    }
    fs::create_dir_all(&staging).map_err(|error| BridgeError::Plugin(error.to_string()))?;
    let mut copied = Vec::new();
    for file in &manifest.files {
        let relative = Path::new(&file.relative_path);
        if relative.is_absolute()
            || relative
                .components()
                .any(|component| matches!(component, std::path::Component::ParentDir))
        {
            let _ = fs::remove_dir_all(&staging);
            return Err(BridgeError::Plugin(
                "plugin file path escapes package".into(),
            ));
        }
        let from = source.join(relative);
        let metadata =
            fs::symlink_metadata(&from).map_err(|error| BridgeError::Plugin(error.to_string()))?;
        if is_link_or_reparse(&from)? || !metadata.is_file() {
            let _ = fs::remove_dir_all(&staging);
            return Err(BridgeError::Plugin(
                "plugin package contains an unsafe or missing file".into(),
            ));
        }
        let bytes = fs::read(&from).map_err(|error| BridgeError::Plugin(error.to_string()))?;
        if sha256(&bytes) != file.sha256 {
            let _ = fs::remove_dir_all(&staging);
            return Err(BridgeError::Plugin("plugin file digest mismatch".into()));
        }
        let to = staging.join(relative);
        if let Some(parent) = to.parent() {
            fs::create_dir_all(parent).map_err(|error| BridgeError::Plugin(error.to_string()))?;
        }
        fs::write(&to, bytes).map_err(|error| BridgeError::Plugin(error.to_string()))?;
        copied.push(file.relative_path.clone());
    }
    fs::write(
        staging.join(PLUGIN_PACKAGE_MANIFEST_FILE),
        serde_json::to_vec_pretty(manifest)
            .map_err(|error| BridgeError::Plugin(error.to_string()))?,
    )
    .map_err(|error| BridgeError::Plugin(error.to_string()))?;
    if replacing {
        fs::rename(install_root, &backup).map_err(|error| {
            let _ = fs::remove_dir_all(&staging);
            BridgeError::Plugin(format!("plugin rollback preparation failed: {error}"))
        })?;
    }
    if let Err(error) = fs::rename(&staging, install_root) {
        if replacing {
            let _ = fs::rename(&backup, install_root);
        }
        let _ = fs::remove_dir_all(&staging);
        return Err(BridgeError::Plugin(format!(
            "atomic plugin install failed: {error}"
        )));
    }
    Ok(PluginInstallResult {
        package_id: manifest.package_id.clone(),
        package_version: manifest.package_version.clone(),
        target: install_root.display().to_string(),
        files: copied,
        verified: true,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum HookEventKind {
    PreTool,
    PostTool,
    Stop,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HookAction {
    pub tool: String,
    pub operation: String,
    pub arguments: Vec<String>,
    pub paths: Vec<String>,
    pub working_scope: String,
    pub action_digest: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HookResult {
    pub exit_code: Option<i32>,
    pub forced: bool,
    pub output_sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HookMessage {
    pub protocol_version: String,
    pub event_id: String,
    pub execution_id: String,
    pub packet_digest: String,
    pub lease_id: String,
    pub worktree_identity: String,
    pub action_sequence: u64,
    pub action_nonce: String,
    pub action_expires_at_ms: u64,
    pub timestamp_ms: i128,
    pub kind: HookEventKind,
    pub action: Option<HookAction>,
    pub result: Option<HookResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PermissionDecision {
    Allow,
    Deny,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HookDecision {
    pub event_id: String,
    pub decision: PermissionDecision,
    pub reason: String,
}

struct ProductionSession {
    session: ExecutionSession,
    packet: TaskPacket,
    root: PathBuf,
    process: Option<SupervisedProcess>,
    result: Option<ProcessResult>,
    events: Vec<NormalizedEvent>,
    transcripts: Vec<RawOutputRecord>,
    artifact_paths: Vec<PathBuf>,
    seen_hook_events: BTreeSet<String>,
    seen_hook_phases: BTreeSet<(u64, HookEventKind)>,
    completed_authorized_action: bool,
    context_path: Option<PathBuf>,
}

pub struct ProductionAdapter {
    report: CompatibilityReport,
    bridge_identity: Option<BridgeIdentity>,
    sessions: BTreeMap<String, ProductionSession>,
    cancel: Option<Arc<AtomicU64>>,
}

fn user_home_root() -> Option<PathBuf> {
    env::var_os("USERPROFILE")
        .or_else(|| env::var_os("HOME"))
        .map(PathBuf::from)
}

fn installed_plugin_roots() -> Vec<PathBuf> {
    let Some(home) = user_home_root() else {
        return Vec::new();
    };
    vec![
        home.join(".gemini")
            .join("config")
            .join("plugins")
            .join(RELINTOR_PLUGIN_NAME),
        home.join(".gemini")
            .join("antigravity-cli")
            .join("plugins")
            .join(RELINTOR_PLUGIN_NAME),
    ]
}

fn installed_plugin_root() -> Option<PathBuf> {
    env::var_os("RELINTOR_ANTIGRAVITY_PLUGIN_ROOT")
        .map(PathBuf::from)
        .or_else(|| {
            installed_plugin_roots()
                .into_iter()
                .find(|root| root.is_dir())
        })
}

fn installed_bridge_identity() -> Result<BridgeIdentity, BridgeError> {
    let root = installed_plugin_root()
        .filter(|root| root.is_dir())
        .ok_or_else(|| BridgeError::Plugin("signed Relintor plugin is not installed".into()))?;
    let trust: Value = serde_json::from_str(TRUSTED_BRIDGE_KEYS)
        .map_err(|error| BridgeError::Plugin(format!("bridge trust configuration: {error}")))?;
    let active = trust
        .get("active_key_id")
        .and_then(Value::as_str)
        .ok_or_else(|| BridgeError::Plugin("bridge trust root is unavailable".into()))?;
    let public_key = trust
        .get("keys")
        .and_then(Value::as_array)
        .and_then(|keys| {
            keys.iter().find(|entry| {
                entry.get("key_id").and_then(Value::as_str) == Some(active)
                    && entry.get("status").and_then(Value::as_str) == Some("active")
            })
        })
        .and_then(|entry| entry.get("public_key_base64"))
        .and_then(Value::as_str)
        .ok_or_else(|| BridgeError::Plugin("bridge trust root is unavailable".into()))?;
    let key = verifying_key_from_base64(public_key)?;
    verify_installed_bridge(&root, &key, None)
}

impl ProductionAdapter {
    pub fn new() -> Self {
        Self::with_cli_path(None)
    }

    pub fn with_cli_path(path: Option<PathBuf>) -> Self {
        Self::with_cli_path_and_cancel(path, None)
    }

    pub fn with_cli_path_and_cancel(path: Option<PathBuf>, cancel: Option<Arc<AtomicU64>>) -> Self {
        Self {
            report: path
                .map(|path| detect_with_override(Some(path)))
                .unwrap_or_else(detect),
            bridge_identity: None,
            sessions: BTreeMap::new(),
            cancel,
        }
    }

    pub fn handle_hook_json(&mut self, raw: &[u8]) -> Result<HookDecision, BridgeError> {
        let message: HookMessage = serde_json::from_slice(raw)
            .map_err(|error| BridgeError::Hook(format!("malformed hook message: {error}")))?;
        if message.protocol_version != "relintor-antigravity-hooks-v1"
            || message.event_id.trim().is_empty()
        {
            return Err(BridgeError::Hook(
                "unsupported hook protocol or event identity".into(),
            ));
        }
        let session = self
            .sessions
            .get_mut(&message.execution_id)
            .ok_or_else(|| BridgeError::Unknown(message.execution_id.clone()))?;
        if !session.seen_hook_events.insert(message.event_id.clone()) {
            return Err(BridgeError::DuplicateEvent(message.event_id));
        }
        if message.packet_digest != session.packet.task_packet_digest
            || message.lease_id != session.packet.lease_id
            || message.worktree_identity != session.packet.worktree_identity
            || message.action_sequence != session.packet.authorized_action.sequence
            || message.action_nonce != session.packet.authorized_action.nonce
            || message.action_expires_at_ms != session.packet.authorized_action.expires_at_ms
            || message.timestamp_ms < 0
            || message.timestamp_ms as u64 >= session.packet.authorized_action.expires_at_ms
        {
            return Err(BridgeError::Authority(
                "hook identity does not match the trusted execution packet".into(),
            ));
        }
        match message.kind {
            HookEventKind::PreTool => {
                let action = message
                    .action
                    .as_ref()
                    .ok_or_else(|| BridgeError::Hook("pre-tool action is required".into()))?;
                if session.completed_authorized_action
                    || !hook_action_matches_authority(
                        action,
                        &session.packet.authorized_action,
                        &session.packet.workspace,
                    )
                {
                    return Ok(HookDecision {
                        event_id: message.event_id,
                        decision: PermissionDecision::Deny,
                        reason: "action is not the exact P7-authorized operation".into(),
                    });
                }
                if !session
                    .seen_hook_phases
                    .insert((message.action_sequence, HookEventKind::PreTool))
                {
                    return Err(BridgeError::DuplicateEvent(format!(
                        "action phase {} was replayed",
                        message.action_sequence
                    )));
                }
                Ok(HookDecision {
                    event_id: message.event_id,
                    decision: PermissionDecision::Allow,
                    reason: "trusted Rust lease and exact action binding validated".into(),
                })
            }
            HookEventKind::PostTool => {
                let action = message
                    .action
                    .as_ref()
                    .ok_or_else(|| BridgeError::Hook("post-tool action is required".into()))?;
                let result = message
                    .result
                    .as_ref()
                    .ok_or_else(|| BridgeError::Hook("post-tool result is required".into()))?;
                if session.completed_authorized_action
                    || !hook_action_matches_authority(
                        action,
                        &session.packet.authorized_action,
                        &session.packet.workspace,
                    )
                {
                    return Err(BridgeError::Authority(
                        "post-tool action is not the exact P7-authorized operation".into(),
                    ));
                }
                if !session
                    .seen_hook_phases
                    .insert((message.action_sequence, HookEventKind::PostTool))
                {
                    return Err(BridgeError::DuplicateEvent(format!(
                        "action phase {} was replayed",
                        message.action_sequence
                    )));
                }
                if let Some(path) = action.paths.first() {
                    if safe_path_in_workspace(Path::new(path), &session.packet.workspace) {
                        session.artifact_paths.push(PathBuf::from(path));
                    }
                }
                session.events.push(NormalizedEvent {
                    event_id: message.event_id.clone(),
                    execution_id: message.execution_id,
                    timestamp_ms: message.timestamp_ms,
                    source: "relintor-production-hook".into(),
                    event_type: if result.exit_code == Some(0) {
                        EventType::ToolFinished
                    } else {
                        EventType::Error
                    },
                    sequence: session.events.len() as u64 + 1,
                    payload: serde_json::to_value(result)
                        .map_err(|error| BridgeError::Hook(error.to_string()))?,
                    raw_sha256: sha256(raw),
                });
                session.completed_authorized_action = true;
                Ok(HookDecision {
                    event_id: message.event_id,
                    decision: PermissionDecision::Allow,
                    reason: "post-tool result captured against exact action identity".into(),
                })
            }
            HookEventKind::Stop => {
                if !session
                    .seen_hook_phases
                    .insert((message.action_sequence, HookEventKind::Stop))
                {
                    return Err(BridgeError::DuplicateEvent(format!(
                        "action phase {} was replayed",
                        message.action_sequence
                    )));
                }
                session.session.requested_stop = true;
                if matches!(session.session.status, ExecutionStatus::Running) {
                    session.session.transition(ExecutionStatus::StopRequested)?;
                }
                Ok(HookDecision {
                    event_id: message.event_id,
                    decision: PermissionDecision::Deny,
                    reason: "stop is a safe incomplete boundary, never completion".into(),
                })
            }
        }
    }

    fn parse_process_events(
        session: &mut ProductionSession,
        output: &[u8],
    ) -> Result<(), BridgeError> {
        let mut normalizer = EventNormalizer::default();
        for line in output
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
        {
            let Ok(value) = serde_json::from_slice::<Value>(line) else {
                continue;
            };
            if value.get("event_id").is_none()
                || value.get("sequence").is_none()
                || value.get("type").is_none()
            {
                continue;
            }
            let event = normalizer
                .normalize(&session.session.execution_id, "antigravity-stdout", line)
                .map_err(|error| BridgeError::Protocol(error.to_string()))?;
            session.events.push(event);
        }
        Ok(())
    }

    fn collect_actual_artifacts(session: &ProductionSession) -> Result<Vec<PathBuf>, BridgeError> {
        let mut artifacts = Vec::new();
        for path in &session.artifact_paths {
            if !safe_path_in_workspace(path, &session.packet.workspace) {
                return Err(BridgeError::Workspace("artifact escaped workspace".into()));
            }
            let metadata = fs::symlink_metadata(path)
                .map_err(|error| BridgeError::Bridge(error.to_string()))?;
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err(BridgeError::Bridge("artifact is not a regular file".into()));
            }
            let bytes = fs::read(path).map_err(|error| BridgeError::Bridge(error.to_string()))?;
            if bytes.len() > MAX_OUTPUT_BYTES {
                return Err(BridgeError::Bridge(
                    "artifact exceeds bounded capture".into(),
                ));
            }
            let _ = sha256(&bytes);
            artifacts.push(path.clone());
        }
        Ok(artifacts)
    }
}

impl Default for ProductionAdapter {
    fn default() -> Self {
        Self::new()
    }
}

fn production_task_prompt(packet: &TaskPacket) -> String {
    format!(
        "Relintor authorized task {} for mission {} (revision {}). Work only inside {}. Objective: {}. Scope: {}. Required evidence: {}. Professional constraints: {}. Forbidden changes: {}. Follow the sealed task scope and stop when the objective is complete or blocked; never claim verification or completion beyond the actual work and tool results.",
        packet.task_id,
        packet.mission_id,
        packet.mission_revision,
        packet.workspace.display(),
        packet.objective,
        packet.scope.join("; "),
        packet.required_evidence.join("; "),
        packet.professional_constraints.join("; "),
        packet.forbidden_changes.join("; "),
    )
}

fn cli_workspace_path(path: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        let value = path.to_string_lossy();
        if let Some(unc) = value.strip_prefix(r"\\?\UNC\") {
            return PathBuf::from(format!(r"\\{unc}"));
        }
        if let Some(drive) = value.strip_prefix(r"\\?\") {
            return PathBuf::from(drive);
        }
    }
    path.to_path_buf()
}

fn production_task_command(
    executable: PathBuf,
    packet: &TaskPacket,
    context_path: &Path,
    bridge_path: PathBuf,
) -> ProcessCommand {
    let workspace = cli_workspace_path(&packet.workspace);
    let mut command = ProcessCommand::new(executable, &workspace)
        .arg("--print")
        .arg(production_task_prompt(packet))
        .arg("--add-dir")
        .arg(&workspace)
        .arg("--mode")
        .arg("accept-edits")
        // The P7 sealed packet and the verified hook bridge are the authority
        // boundary for this non-interactive Relintor execution.  Without this
        // supported CLI flag, print mode waits forever for a hidden permission
        // prompt that a desktop user cannot see or answer.
        .arg("--dangerously-skip-permissions")
        .arg("--output-format")
        .arg("stream-json")
        .arg("--print-timeout")
        // The scheduler packet carries the remaining authoritative task
        // budget. Keep the executor's own safety limit at that same boundary;
        // the Rust supervisor remains the final authority.
        .arg(format!("{}ms", packet.time_budget_ms.max(1)))
        .environment("RELINTOR_ANTIGRAVITY_BRIDGE_PATH", bridge_path)
        .environment("RELINTOR_ANTIGRAVITY_HOOK_CONTEXT", context_path);
    for (key, value) in inherited_runtime_environment() {
        command = command.environment(key, value);
    }
    command
}

impl AntigravityAdapter for ProductionAdapter {
    fn detect(&self) -> CompatibilityReport {
        self.report.clone()
    }

    fn validate_version(&self, report: &CompatibilityReport) -> Result<(), BridgeError> {
        if report.sealed_execution_allowed() {
            Ok(())
        } else {
            Err(BridgeError::Incompatible)
        }
    }

    fn install_or_validate_bridge(
        &self,
        path: &Path,
        manifest: &BridgeManifest,
    ) -> Result<BridgeIdentity, BridgeError> {
        if manifest.expected_sha256.is_none() || manifest.signing_status != "verified" {
            return Err(BridgeError::Plugin(
                "production bridge signing and exact digest are not configured".into(),
            ));
        }
        validate_bridge_identity(path, manifest)
    }

    fn create_execution(
        &mut self,
        packet: TaskPacket,
        root: &Path,
    ) -> Result<ExecutionSession, BridgeError> {
        if self.bridge_identity.is_none() {
            self.bridge_identity = Some(installed_bridge_identity()?);
        }
        packet.validate(root)?;
        if packet.binding_digest()? != packet.task_packet_digest {
            return Err(BridgeError::Authority(
                "task packet digest does not bind its canonical payload".into(),
            ));
        }
        if packet.worktree_identity != workspace_identity(&packet.workspace)? {
            return Err(BridgeError::Authority(
                "task packet worktree identity does not match the canonical workspace".into(),
            ));
        }
        let report = self.detect();
        self.validate_version(&report)?;
        let session = ExecutionSession::new(
            &packet,
            report.version.clone(),
            workspace_identity(&packet.workspace)?,
        );
        self.sessions.insert(
            session.execution_id.clone(),
            ProductionSession {
                session: session.clone(),
                packet,
                root: fs::canonicalize(root).map_err(|e| BridgeError::Workspace(e.to_string()))?,
                process: None,
                result: None,
                events: Vec::new(),
                transcripts: Vec::new(),
                artifact_paths: Vec::new(),
                seen_hook_events: BTreeSet::new(),
                seen_hook_phases: BTreeSet::new(),
                completed_authorized_action: false,
                context_path: None,
            },
        );
        Ok(session)
    }

    fn send_task(&mut self, id: &str, packet: &TaskPacket) -> Result<(), BridgeError> {
        let executable = self
            .report
            .executable_path
            .as_ref()
            .map(PathBuf::from)
            .ok_or_else(|| BridgeError::Process("Antigravity executable is unavailable".into()))?;
        let bridge_path = self
            .bridge_identity
            .as_ref()
            .map(|identity| PathBuf::from(&identity.canonical_path))
            .ok_or_else(|| BridgeError::Plugin("signed bridge identity is unavailable".into()))?;
        let session = self
            .sessions
            .get_mut(id)
            .ok_or_else(|| BridgeError::Unknown(id.into()))?;
        if &session.packet != packet || packet.binding_digest()? != packet.task_packet_digest {
            return Err(BridgeError::Authority(
                "send_task packet differs from the trusted creation packet".into(),
            ));
        }
        let packet_json = packet.to_json()?;
        let context_path = env::temp_dir().join(format!(
            "relintor-antigravity-context-{}.json",
            session.session.execution_id
        ));
        fs::write(&context_path, packet_json.as_bytes())
            .map_err(|error| BridgeError::Process(format!("write bridge context: {error}")))?;
        let command = production_task_command(executable, packet, &context_path, bridge_path);
        session.context_path = Some(context_path);
        let process = SupervisedProcess::start(&command, &session.root, MAX_OUTPUT_BYTES)?;
        session.session.process_identity = Some(process.identity().clone());
        session.process = Some(process);
        session.session.transition(ExecutionStatus::Starting)?;
        session.session.transition(ExecutionStatus::Running)?;
        Ok(())
    }

    fn process_identity(&self, id: &str) -> Result<ProcessIdentity, BridgeError> {
        self.sessions
            .get(id)
            .and_then(|session| session.session.process_identity.clone())
            .ok_or_else(|| BridgeError::Process("owned Antigravity process was not started".into()))
    }

    fn stream_events(&mut self, id: &str) -> Result<Vec<NormalizedEvent>, BridgeError> {
        self.sessions
            .get(id)
            .map(|session| session.events.clone())
            .ok_or_else(|| BridgeError::Unknown(id.into()))
    }

    fn request_stop(&mut self, id: &str) -> Result<(), BridgeError> {
        let session = self
            .sessions
            .get_mut(id)
            .ok_or_else(|| BridgeError::Unknown(id.into()))?;
        session.session.transition(ExecutionStatus::StopRequested)?;
        if let Some(process) = session.process.as_mut() {
            session.result = Some(process.stop(StopPolicy {
                timeout: Duration::from_secs(2),
                allow_force_kill: true,
            })?);
        }
        if let Some(context_path) = session.context_path.take() {
            let _ = fs::remove_file(context_path);
        }
        Ok(())
    }

    fn collect_artifacts(&mut self, id: &str) -> Result<Vec<PathBuf>, BridgeError> {
        let session = self
            .sessions
            .get(id)
            .ok_or_else(|| BridgeError::Unknown(id.into()))?;
        Self::collect_actual_artifacts(session)
    }

    fn wait_for_exit(&mut self, id: &str) -> Result<ProcessResult, BridgeError> {
        self.wait_for_exit_with_timeouts(id, PRODUCTION_PROCESS_TIMEOUT, PRODUCTION_IDLE_TIMEOUT)
    }

    fn wait_for_exit_with_timeouts(
        &mut self,
        id: &str,
        timeout: Duration,
        idle_timeout: Duration,
    ) -> Result<ProcessResult, BridgeError> {
        let session = self
            .sessions
            .get_mut(id)
            .ok_or_else(|| BridgeError::Unknown(id.into()))?;
        if session.result.is_none() {
            let process = session
                .process
                .take()
                .ok_or_else(|| BridgeError::Process("process was not started".into()))?;
            session.result = Some(process.wait_with_monitor(
                timeout.min(PRODUCTION_PROCESS_TIMEOUT),
                idle_timeout.min(PRODUCTION_IDLE_TIMEOUT),
                self.cancel.as_deref(),
            )?);
        }
        let result = session.result.clone().expect("result set above");
        let safe_stdout = redact_sensitive(&result.stdout);
        let safe_stderr = redact_sensitive(&result.stderr);
        session.transcripts.push(raw_output(
            &session.session.execution_id,
            "antigravity-stdout",
            safe_stdout.clone(),
        ));
        session.transcripts.push(raw_output(
            &session.session.execution_id,
            "antigravity-stderr",
            safe_stderr,
        ));
        Self::parse_process_events(session, &safe_stdout)?;
        if let Some(context_path) = session.context_path.take() {
            let _ = fs::remove_file(context_path);
        }
        Ok(result)
    }

    fn reconcile_exit(
        &mut self,
        id: &str,
        result: ProcessResult,
    ) -> Result<ExecutionSession, BridgeError> {
        let session = self
            .sessions
            .get_mut(id)
            .ok_or_else(|| BridgeError::Unknown(id.into()))?;
        if session.result.as_ref() != Some(&result) {
            return Err(BridgeError::Authority(
                "caller-supplied process result does not match adapter-owned result".into(),
            ));
        }
        session.session.process_identity = Some(result.identity.clone());
        session.session.exit_state = Some(ExitState {
            exit_code: result.exit_code,
            exited_at_ms: result.ended_at_ms,
            forced: result.forced,
        });
        let next = if result.exit_code == Some(0) && !result.forced && !result.output_overflow {
            ExecutionStatus::Exited
        } else {
            ExecutionStatus::Failed
        };
        if matches!(
            session.session.status,
            ExecutionStatus::Running | ExecutionStatus::StopRequested
        ) {
            session.session.transition(next)?;
        }
        Ok(session.session.clone())
    }
}

impl ProductionAdapter {
    pub fn transcripts(&self, execution_id: &str) -> Result<Vec<RawOutputRecord>, BridgeError> {
        self.sessions
            .get(execution_id)
            .map(|session| session.transcripts.clone())
            .ok_or_else(|| BridgeError::Unknown(execution_id.into()))
    }
}

fn action_digest(action: &HookAction) -> String {
    canonical_hook_action_digest(
        &action.tool,
        &action.operation,
        &action.arguments,
        &action.paths,
        &action.working_scope,
    )
    .unwrap_or_default()
}

pub fn canonical_hook_action_digest(
    tool: &str,
    operation: &str,
    arguments: &[String],
    paths: &[String],
    working_scope: &str,
) -> Result<String, BridgeError> {
    let mut hasher = Sha256::new();
    hasher.update(
        serde_json::to_vec(&(
            tool.to_ascii_lowercase(),
            operation.to_ascii_lowercase(),
            arguments,
            paths,
            working_scope,
        ))
        .map_err(|error| BridgeError::Hook(error.to_string()))?,
    );
    Ok(format!("{:x}", hasher.finalize()))
}

pub fn hook_action_matches_authority(
    action: &HookAction,
    expected: &AuthorizedAction,
    workspace: &Path,
) -> bool {
    if action_digest(action) != action.action_digest
        || action.tool != expected.tool
        || action.operation != expected.operation
        || action.arguments != expected.arguments
        || action.paths.len() != expected.paths.len()
        || action.working_scope != expected.working_scope
    {
        return false;
    }
    action
        .paths
        .iter()
        .zip(&expected.paths)
        .all(|(actual, expected)| {
            safe_path_in_workspace(Path::new(actual), workspace)
                && canonical_hook_path(actual) == canonical_hook_path(expected)
        })
}

fn canonical_hook_path(value: &str) -> String {
    resolve_path(Path::new(value))
        .map(|path| {
            let value = path.display().to_string().replace('\\', "/");
            if cfg!(windows) {
                value.to_ascii_lowercase()
            } else {
                value
            }
        })
        .unwrap_or_else(|| value.replace('\\', "/"))
}

fn redact_sensitive(bytes: &[u8]) -> Vec<u8> {
    let text = String::from_utf8_lossy(bytes);
    let mut output = Vec::with_capacity(text.len());
    for line in text.lines() {
        let lower = line.to_ascii_lowercase();
        if [
            "api_key=",
            "apikey=",
            "authorization:",
            "bearer ",
            "private_key=",
            "refresh_token=",
            "session_token=",
            "password=",
        ]
        .iter()
        .any(|needle| lower.contains(needle))
        {
            output.extend_from_slice(b"[REDACTED_SENSITIVE_OUTPUT]\n");
        } else {
            output.extend_from_slice(line.as_bytes());
            output.push(b'\n');
        }
    }
    output.truncate(MAX_OUTPUT_BYTES);
    output
}

fn safe_path_in_workspace(candidate: &Path, workspace: &Path) -> bool {
    let Some(candidate) = resolve_path(candidate) else {
        return false;
    };
    let Some(workspace) = resolve_path(workspace) else {
        return false;
    };
    candidate.starts_with(workspace)
}

fn workspace_identity(workspace: &Path) -> Result<String, BridgeError> {
    let canonical =
        fs::canonicalize(workspace).map_err(|error| BridgeError::Workspace(error.to_string()))?;
    let git_marker = canonical.join(".git").display().to_string();
    Ok(sha256(
        format!("{}\0{}", canonical.display(), git_marker).as_bytes(),
    ))
}

fn resolve_path(path: &Path) -> Option<PathBuf> {
    if !path.is_absolute() {
        return None;
    }
    let mut existing = path.to_path_buf();
    let mut suffix = Vec::new();
    while !existing.exists() {
        suffix.push(existing.file_name()?.to_owned());
        existing = existing.parent()?.to_path_buf();
    }
    let mut resolved = fs::canonicalize(existing).ok()?;
    for part in suffix.iter().rev() {
        resolved.push(part);
    }
    Some(resolved)
}

#[derive(Debug)]
pub enum BridgeError {
    Registry(String),
    Packet(String),
    Workspace(String),
    Process(String),
    StopTimeout,
    Event(String),
    Protocol(String),
    DuplicateEvent(String),
    DuplicateSequence(u64),
    Bridge(String),
    Plugin(String),
    Hook(String),
    Authority(String),
    Incompatible,
    Unknown(String),
    Transition {
        from: ExecutionStatus,
        to: ExecutionStatus,
    },
}
impl std::fmt::Display for BridgeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Registry(e) => write!(f, "invalid compatibility registry: {e}"),
            Self::Packet(e) => write!(f, "invalid task packet: {e}"),
            Self::Workspace(e) => write!(f, "workspace safety violation: {e}"),
            Self::Process(e) => write!(f, "process supervision error: {e}"),
            Self::StopTimeout => write!(f, "stop timeout without explicit force-stop policy"),
            Self::Event(e) => write!(f, "malformed event: {e}"),
            Self::Protocol(e) => write!(f, "bridge protocol failure: {e}"),
            Self::DuplicateEvent(e) => write!(f, "duplicate event ID: {e}"),
            Self::DuplicateSequence(e) => write!(f, "duplicate event sequence: {e}"),
            Self::Bridge(e) => write!(f, "bridge identity rejected: {e}"),
            Self::Plugin(e) => write!(f, "plugin boundary rejected: {e}"),
            Self::Hook(e) => write!(f, "hook message rejected: {e}"),
            Self::Authority(e) => write!(f, "execution authority rejected: {e}"),
            Self::Incompatible => write!(f, "Antigravity version is incompatible"),
            Self::Unknown(e) => write!(f, "unknown execution: {e}"),
            Self::Transition { from, to } => {
                write!(f, "invalid status transition {from:?} -> {to:?}")
            }
        }
    }
}
impl std::error::Error for BridgeError {}

fn sha256(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    format!("{:x}", h.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
    use ed25519_dalek::{Signer, SigningKey};
    fn root(label: &str) -> PathBuf {
        let p = env::temp_dir().join(format!("relintor-agy-{label}-{}", now_ms()));
        fs::create_dir_all(&p).unwrap();
        p
    }
    fn packet(workspace: PathBuf) -> TaskPacket {
        let mut authorized_action = AuthorizedAction {
            sequence: 1,
            tool: "antigravity".into(),
            operation: "execute_task".into(),
            arguments: vec!["t1".into()],
            paths: vec![workspace.display().to_string()],
            working_scope: workspace.display().to_string(),
            expires_at_ms: u64::MAX,
            nonce: "test-action-nonce".into(),
            digest: String::new(),
        };
        authorized_action.refresh_digest().unwrap();
        let mut packet = TaskPacket {
            contract_version: TASK_PACKET_VERSION.into(),
            mission_id: "m1".into(),
            mission_revision: 1,
            seal_hash: "seal-test".into(),
            task_id: "t1".into(),
            project_id: "project-test".into(),
            workspace: workspace.clone(),
            workspace_fingerprint: "workspace-test".into(),
            task_packet_digest: String::new(),
            lease_id: "lease-test".into(),
            lease_expires_at_ms: u64::MAX,
            time_budget_ms: 300_000,
            allowed_tools: ["antigravity".into()].into_iter().collect(),
            worktree_identity: workspace_identity(&workspace).unwrap(),
            subagent_identity: None,
            objective: "bounded bridge test".into(),
            scope: vec!["bridge".into()],
            sealed_requirement_ids: vec!["F-06".into()],
            architecture_decisions: vec!["Relintor remains authority".into()],
            professional_constraints: vec!["no GUI automation".into()],
            required_evidence: vec!["transcript".into()],
            previous_failures: vec![],
            forbidden_changes: vec!["spec/locked".into()],
            stop_condition: StopCondition::AfterProcessExit,
            authorized_action,
        };
        packet.task_packet_digest = packet.binding_digest().unwrap();
        packet
    }
    #[test]
    fn registry_and_detection_fail_closed() {
        let r = CompatibilityRegistry::bundled().unwrap();
        let absent = classify_detection(
            DetectionInput {
                executable_path: None,
                version_output: None,
                cli_invocation_capability: false,
                plugin_hook_capability: false,
                environment: "test".into(),
                platform: "windows".into(),
                detected_at_ms: 1,
            },
            &r,
        );
        assert_eq!(absent.compatibility, Compatibility::NotInstalled);
        let unknown = classify_detection(
            DetectionInput {
                executable_path: Some("agy".into()),
                version_output: Some("Antigravity 99.0.0".into()),
                cli_invocation_capability: true,
                plugin_hook_capability: true,
                environment: "test".into(),
                platform: "windows".into(),
                detected_at_ms: 2,
            },
            &r,
        );
        assert!(!unknown.sealed_execution_allowed());
        assert_eq!(unknown.compatibility, Compatibility::Unsupported);
    }
    #[test]
    fn packet_validation_serialization_and_workspace_traversal() {
        let r = root("packet");
        let w = r.join("workspace");
        fs::create_dir_all(&w).unwrap();
        let p = packet(w.clone());
        p.validate(&r).unwrap();
        assert!(p.to_json().unwrap().contains("bounded"));
        let mut bad = p.clone();
        bad.workspace = r.join("outside");
        assert!(matches!(bad.validate(&r), Err(BridgeError::Workspace(_))));
        bad.workspace = w;
        bad.sealed_requirement_ids = vec!["Z-99".into()];
        assert!(matches!(bad.validate(&r), Err(BridgeError::Packet(_))));
        let _ = fs::remove_dir_all(r);
    }
    #[test]
    fn mock_adapter_and_explicit_session_states() {
        let r = root("mock");
        let w = r.join("w");
        fs::create_dir_all(&w).unwrap();
        let p = packet(w);
        let mut a = MockAdapter::supported();
        let s = a.create_execution(p.clone(), &r).unwrap();
        a.send_task(&s.execution_id, &p).unwrap();
        a.request_stop(&s.execution_id).unwrap();
        let result = ProcessResult {
            identity: ProcessIdentity {
                pid: 1,
                executable: "agy".into(),
                started_at_ms: 1,
                command_digest: sha256(b"agy"),
            },
            state: ProcessState::Cancelled,
            exit_code: Some(0),
            stdout: vec![],
            stderr: vec![],
            output_overflow: false,
            started_at_ms: 1,
            ended_at_ms: 2,
            process_exited_at_ms: 2,
            output_collected_at_ms: 2,
            forced: false,
            artifact_paths: Vec::new(),
        };
        assert_eq!(
            a.reconcile_exit(&s.execution_id, result).unwrap().status,
            ExecutionStatus::Exited
        );
        let _ = fs::remove_dir_all(r);
    }
    #[test]
    fn arguments_are_structured_not_shell_strings() {
        let r = root("args");
        let c = ProcessCommand::new("agy", &r)
            .arg("--task")
            .arg("$(touch hacked)");
        assert_eq!(c.args[1], OsString::from("$(touch hacked)"));
        let _ = fs::remove_dir_all(r);
    }

    #[test]
    fn production_task_command_uses_supported_cli_contract_and_runtime_environment() {
        let r = root("production-command");
        let workspace = r.join("workspace");
        fs::create_dir_all(&workspace).unwrap();
        let packet = packet(workspace.clone());
        let context = r.join("context.json");
        let command = production_task_command(
            PathBuf::from("agy.exe"),
            &packet,
            &context,
            r.join("relintor-antigravity-bridge.exe"),
        );
        let args = command
            .args
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert!(args
            .windows(2)
            .any(|pair| pair[0] == "--print" && pair[1] == production_task_prompt(&packet)));
        assert!(args
            .windows(2)
            .any(|pair| { pair[0] == "--add-dir" && pair[1] == workspace.display().to_string() }));
        assert!(args
            .windows(2)
            .any(|pair| pair[0] == "--mode" && pair[1] == "accept-edits"));
        assert!(args
            .windows(2)
            .any(|pair| pair[0] == "--output-format" && pair[1] == "stream-json"));
        assert!(args
            .iter()
            .any(|arg| arg == "--dangerously-skip-permissions"));
        assert!(!args.iter().any(|arg| {
            matches!(
                arg.as_str(),
                "--headless" | "--structured-events" | "--task-packet-json"
            )
        }));
        let _ = fs::remove_dir_all(r);
    }

    #[cfg(windows)]
    #[test]
    fn production_command_normalizes_extended_windows_workspace_paths() {
        let root = root("extended-path");
        let mut packet = packet(root.clone());
        packet.workspace = PathBuf::from(r"\\?\D:\Relintor-temp\workspace");
        let command = production_task_command(
            PathBuf::from("agy.exe"),
            &packet,
            Path::new(r"\\?\D:\Relintor-temp\context.json"),
            PathBuf::from("bridge.exe"),
        );
        let args = command
            .args
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert!(args
            .windows(2)
            .any(|pair| pair[0] == "--add-dir" && pair[1] == r"D:\Relintor-temp\workspace"));
        assert_eq!(
            command.current_dir,
            PathBuf::from(r"D:\Relintor-temp\workspace")
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn internal_console_processes_use_the_hidden_windows_policy() {
        assert_eq!(
            WINDOWS_CREATE_NO_WINDOW,
            if cfg!(windows) { 0x0800_0000 } else { 0 }
        );
    }
    #[test]
    fn event_normalization_rejects_malformed_and_duplicates() {
        let raw = br#"{"event_id":"e1","sequence":1,"type":"agent_message","payload":{"text":"untrusted"}}"#;
        let mut n = EventNormalizer::default();
        assert_eq!(
            n.normalize("x", "stdout", raw).unwrap().event_type,
            EventType::AgentMessage
        );
        assert!(matches!(
            n.normalize("x", "stdout", raw),
            Err(BridgeError::DuplicateEvent(_))
        ));
        assert!(matches!(
            n.normalize("x", "stdout", br"not-json"),
            Err(BridgeError::Event(_))
        ));
        let second = br#"{"event_id":"e2","sequence":1,"type":"warning","payload":null}"#;
        assert!(matches!(
            n.normalize("x", "stdout", second),
            Err(BridgeError::DuplicateSequence(1))
        ));
    }
    #[test]
    fn bridge_identity_records_integrity() {
        let r = root("identity");
        let p = r.join("bridge");
        fs::write(&p, b"bridge").unwrap();
        let m = BridgeManifest {
            schema_version: 1,
            adapter_version: ADAPTER_VERSION.into(),
            expected_executable: "bridge".into(),
            expected_sha256: Some(sha256(b"bridge")),
            supported_antigravity_versions: vec!["1.".into()],
            hook_protocol: "documented-only".into(),
            signing_status: "not_implemented".into(),
        };
        assert_eq!(
            validate_bridge_identity(&p, &m).unwrap().sha256,
            sha256(b"bridge")
        );
        let wrong = BridgeManifest {
            expected_executable: "unexpected".into(),
            ..m
        };
        assert!(matches!(
            validate_bridge_identity(&p, &wrong),
            Err(BridgeError::Bridge(_))
        ));
        let _ = fs::remove_dir_all(r);
    }

    #[cfg(windows)]
    #[test]
    fn stop_timeout_requires_explicit_force_policy() {
        let r = root("stop");
        let mut process = SupervisedProcess::start(
            &ProcessCommand::new("cmd.exe", &r)
                .arg("/C")
                .arg("ping 127.0.0.1 -n 6 >nul"),
            &r,
            4096,
        )
        .unwrap();
        assert!(matches!(
            process.stop(StopPolicy {
                timeout: Duration::from_millis(1),
                allow_force_kill: false,
            }),
            Err(BridgeError::StopTimeout)
        ));
        let result = process
            .stop(StopPolicy {
                timeout: Duration::from_millis(1),
                allow_force_kill: true,
            })
            .unwrap();
        assert!(result.forced);
        let _ = fs::remove_dir_all(r);
    }

    #[test]
    fn output_reader_marks_overflow_without_unbounded_buffering() {
        let receiver = reader_thread(
            std::io::Cursor::new(vec![b'x'; 32]),
            8,
            Arc::new(AtomicU64::new(now_ms() as u64)),
        );
        let bytes = receive(Some(receiver)).unwrap();
        assert!(bytes.len() > 8);
        assert!(bytes.len() <= 32);
    }

    #[cfg(windows)]
    #[test]
    fn idle_process_is_terminated_as_a_timeout() {
        let r = root("idle-timeout");
        let result = SupervisedProcess::start(
            &ProcessCommand::new("cmd.exe", &r)
                .arg("/C")
                .arg("ping 127.0.0.1 -n 20 >nul"),
            &r,
            4096,
        )
        .unwrap()
        .wait_with_monitor(Duration::from_secs(5), Duration::from_millis(20), None)
        .unwrap();
        assert_eq!(result.state, ProcessState::TimedOut);
        assert!(result.forced);
        let _ = fs::remove_dir_all(r);
    }

    #[cfg(windows)]
    #[test]
    fn cancellation_is_reconciled_as_cancelled() {
        let r = root("cancelled");
        let cancel = Arc::new(AtomicU64::new(0));
        let trigger = Arc::clone(&cancel);
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(30));
            trigger.store(1, Ordering::Release);
        });
        let result = SupervisedProcess::start(
            &ProcessCommand::new("cmd.exe", &r)
                .arg("/C")
                .arg("ping 127.0.0.1 -n 20 >nul"),
            &r,
            4096,
        )
        .unwrap()
        .wait_with_monitor(
            Duration::from_secs(5),
            Duration::from_secs(5),
            Some(&cancel),
        )
        .unwrap();
        assert_eq!(result.state, ProcessState::Cancelled);
        assert!(result.forced);
        let _ = fs::remove_dir_all(r);
    }

    #[cfg(windows)]
    #[test]
    fn running_process_reports_progress_and_idle_states() {
        let r = root("progress-state");
        let process = SupervisedProcess::start(
            &ProcessCommand::new("cmd.exe", &r)
                .arg("/C")
                .arg("ping 127.0.0.1 -n 4 >nul"),
            &r,
            4096,
        )
        .unwrap();
        assert_eq!(
            process.progress_state(Duration::from_secs(5)),
            ProcessState::RunningWithProgress
        );
        thread::sleep(Duration::from_millis(25));
        assert_eq!(
            process.progress_state(Duration::from_millis(1)),
            ProcessState::RunningIdle
        );
        let _ = process
            .wait_with_monitor(Duration::from_secs(5), Duration::from_secs(5), None)
            .unwrap();
        let _ = fs::remove_dir_all(r);
    }

    #[cfg(windows)]
    #[test]
    fn process_supervision_captures_output_and_exit() {
        let r = root("process");
        let c = ProcessCommand::new("cmd.exe", &r)
            .arg("/C")
            .arg("echo out & echo err 1>&2 & exit /B 3");
        let result = SupervisedProcess::start(&c, &r, 4096)
            .unwrap()
            .wait()
            .unwrap();
        assert_eq!(result.exit_code, Some(3));
        assert!(String::from_utf8_lossy(&result.stdout).contains("out"));
        assert!(String::from_utf8_lossy(&result.stderr).contains("err"));
        assert!(result.process_exited_at_ms <= result.output_collected_at_ms);
        assert_eq!(result.ended_at_ms, result.process_exited_at_ms);
    }

    fn supported_production_adapter() -> ProductionAdapter {
        ProductionAdapter {
            report: CompatibilityReport {
                executable_present: true,
                executable_path: Some("agy".into()),
                version: Some("1.2.0".into()),
                compatibility: Compatibility::Supported,
                cli_invocation_capability: true,
                plugin_hook_capability: true,
                environment: "test-transport".into(),
                platform: env::consts::OS.into(),
                detected_at_ms: 1,
                detail: "explicit test transport report".into(),
            },
            bridge_identity: Some(BridgeIdentity {
                canonical_path: "test-bridge".into(),
                sha256: "test-bridge-hash".into(),
                adapter_version: ADAPTER_VERSION.into(),
                signing_status: "verified".into(),
            }),
            sessions: BTreeMap::new(),
            cancel: None,
        }
    }

    fn hook_action(workspace: &Path, arguments: Vec<String>) -> HookAction {
        let mut action = HookAction {
            tool: "antigravity".into(),
            operation: "execute_task".into(),
            arguments,
            paths: vec![workspace.display().to_string()],
            working_scope: workspace.display().to_string(),
            action_digest: String::new(),
        };
        action.action_digest = action_digest(&action);
        action
    }

    #[test]
    fn production_adapter_never_falls_back_to_mock_when_runtime_is_missing() {
        let root = root("production-missing");
        let workspace = root.join("workspace");
        fs::create_dir_all(&workspace).unwrap();
        let packet = packet(workspace);
        let report = classify_detection(
            DetectionInput {
                executable_path: None,
                version_output: None,
                cli_invocation_capability: false,
                plugin_hook_capability: false,
                environment: "test-missing-runtime".into(),
                platform: env::consts::OS.into(),
                detected_at_ms: 1,
            },
            &CompatibilityRegistry::bundled().unwrap(),
        );
        let mut adapter = ProductionAdapter {
            report,
            bridge_identity: None,
            sessions: BTreeMap::new(),
            cancel: None,
        };
        assert_eq!(adapter.detect().compatibility, Compatibility::NotInstalled);
        assert!(adapter.create_execution(packet, &root).is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn production_hooks_bind_pre_post_stop_to_one_execution() {
        let root = root("production-hooks");
        let workspace = root.join("workspace");
        fs::create_dir_all(workspace.join("src")).unwrap();
        let packet = packet(workspace.clone());
        let mut adapter = supported_production_adapter();
        let session = adapter.create_execution(packet.clone(), &root).unwrap();
        let action = hook_action(&workspace, vec!["t1".into()]);
        let pre = HookMessage {
            protocol_version: "relintor-antigravity-hooks-v1".into(),
            event_id: "pre-1".into(),
            execution_id: session.execution_id.clone(),
            packet_digest: packet.task_packet_digest.clone(),
            lease_id: packet.lease_id.clone(),
            worktree_identity: packet.worktree_identity.clone(),
            action_sequence: packet.authorized_action.sequence,
            action_nonce: packet.authorized_action.nonce.clone(),
            action_expires_at_ms: packet.authorized_action.expires_at_ms,
            timestamp_ms: 2,
            kind: HookEventKind::PreTool,
            action: Some(action.clone()),
            result: None,
        };
        assert_eq!(
            adapter
                .handle_hook_json(&serde_json::to_vec(&pre).unwrap())
                .unwrap()
                .decision,
            PermissionDecision::Allow
        );
        let mut forged = action.clone();
        forged.arguments = vec!["different-file.rs".into()];
        forged.action_digest = action_digest(&forged);
        let forged_message = HookMessage {
            event_id: "pre-forged".into(),
            action: Some(forged),
            ..pre.clone()
        };
        assert_eq!(
            adapter
                .handle_hook_json(&serde_json::to_vec(&forged_message).unwrap())
                .unwrap()
                .decision,
            PermissionDecision::Deny
        );
        fs::write(workspace.join("src/main.rs"), b"captured").unwrap();
        let post = HookMessage {
            event_id: "post-1".into(),
            kind: HookEventKind::PostTool,
            result: Some(HookResult {
                exit_code: Some(0),
                forced: false,
                output_sha256: sha256(b"captured"),
            }),
            ..pre.clone()
        };
        assert_eq!(
            adapter
                .handle_hook_json(&serde_json::to_vec(&post).unwrap())
                .unwrap()
                .decision,
            PermissionDecision::Allow
        );
        assert!(matches!(
            adapter.handle_hook_json(&serde_json::to_vec(&post).unwrap()),
            Err(BridgeError::DuplicateEvent(_))
        ));
        let stop = HookMessage {
            event_id: "stop-1".into(),
            kind: HookEventKind::Stop,
            action: None,
            result: None,
            ..pre
        };
        assert_eq!(
            adapter
                .handle_hook_json(&serde_json::to_vec(&stop).unwrap())
                .unwrap()
                .decision,
            PermissionDecision::Deny
        );
        assert!(
            adapter
                .sessions
                .get(&session.execution_id)
                .unwrap()
                .session
                .requested_stop
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn plugin_install_requires_signed_manifest_and_exact_files() {
        let root = root("plugin-install");
        let package = root.join("package");
        fs::create_dir_all(&package).unwrap();
        let bridge = package.join("relintor-antigravity-bridge");
        fs::write(&bridge, b"signed bridge bytes").unwrap();
        fs::write(
            package.join("plugin.json"),
            format!(r#"{{"name":"{RELINTOR_PLUGIN_NAME}"}}"#),
        )
        .unwrap();
        fs::write(
            package.join("hooks.json"),
            r#"{"PreToolUse":[{"matcher":"*","hooks":[{"type":"command","command":"%RELINTOR_ANTIGRAVITY_BRIDGE_PATH%"}]}]}"#,
        )
        .unwrap();
        let signing_key = SigningKey::from_bytes(&[23_u8; 32]);
        let mut manifest = PluginPackageManifest {
            schema_version: 1,
            package_id: "com.relintor.antigravity".into(),
            package_version: "1.0.0".into(),
            protocol_version: "relintor-antigravity-hooks-v1".into(),
            target_triple: runtime_target_triple().into(),
            supported_antigravity_versions: vec!["1.".into()],
            files: vec![PluginPackageFile {
                relative_path: "relintor-antigravity-bridge".into(),
                sha256: sha256(b"signed bridge bytes"),
            }, PluginPackageFile {
                relative_path: "plugin.json".into(),
                sha256: sha256(format!(r#"{{"name":"{RELINTOR_PLUGIN_NAME}"}}"#).as_bytes()),
            }, PluginPackageFile {
                relative_path: "hooks.json".into(),
                sha256: sha256(br#"{"PreToolUse":[{"matcher":"*","hooks":[{"type":"command","command":"%RELINTOR_ANTIGRAVITY_BRIDGE_PATH%"}]}]}"#),
            }],
            key_id: sha256(signing_key.verifying_key().as_bytes()),
            signature: String::new(),
        };
        manifest.signature = BASE64.encode(
            signing_key
                .sign(&plugin_signing_bytes(&manifest).unwrap())
                .to_bytes(),
        );
        fs::write(
            package.join(PLUGIN_PACKAGE_MANIFEST_FILE),
            serde_json::to_vec_pretty(&manifest).unwrap(),
        )
        .unwrap();
        let target = root.join("installed");
        let result =
            install_verified_plugin(&package, &target, &manifest, &signing_key.verifying_key())
                .unwrap();
        assert!(result.verified);
        assert_eq!(
            fs::read(target.join("relintor-antigravity-bridge")).unwrap(),
            b"signed bridge bytes"
        );
        assert!(
            verify_installed_plugin(&target, &signing_key.verifying_key(), Some("1.2.3"),)
                .unwrap()
                .verified
        );
        assert!(install_verified_plugin(
            &package,
            &target,
            &manifest,
            &signing_key.verifying_key(),
        )
        .is_ok());
        let _ = fs::remove_dir_all(root);
    }

    fn signed_package_fixture(
        label: &str,
    ) -> (PathBuf, PathBuf, SigningKey, PluginPackageManifest) {
        let root = root(label);
        let package = root.join("package");
        fs::create_dir_all(&package).unwrap();
        fs::write(
            package.join("relintor-antigravity-bridge.exe"),
            b"signed bridge bytes",
        )
        .unwrap();
        let plugin = format!(r#"{{"name":"{RELINTOR_PLUGIN_NAME}"}}"#);
        let hooks = r#"{"PreToolUse":[{"matcher":"*","hooks":[{"type":"command","command":"%RELINTOR_ANTIGRAVITY_BRIDGE_PATH%"}]}]}"#;
        let bridge_manifest = format!(
            r#"{{"schema_version":1,"adapter_version":"0.1.0","expected_executable":"relintor-antigravity-bridge.exe","expected_sha256":"{}","supported_antigravity_versions":["1."],"hook_protocol":"relintor-antigravity-hooks-v1","signing_status":"verified"}}"#,
            sha256(b"signed bridge bytes")
        );
        fs::write(package.join("plugin.json"), &plugin).unwrap();
        fs::write(package.join("hooks.json"), hooks).unwrap();
        fs::write(package.join("bridge-manifest.json"), &bridge_manifest).unwrap();
        let signing_key = SigningKey::from_bytes(&[41_u8; 32]);
        let mut manifest = PluginPackageManifest {
            schema_version: 1,
            package_id: "com.relintor.antigravity".into(),
            package_version: "1.0.0".into(),
            protocol_version: "relintor-antigravity-hooks-v1".into(),
            target_triple: runtime_target_triple().into(),
            supported_antigravity_versions: vec!["1.".into()],
            files: vec![
                PluginPackageFile {
                    relative_path: "relintor-antigravity-bridge.exe".into(),
                    sha256: sha256(b"signed bridge bytes"),
                },
                PluginPackageFile {
                    relative_path: "plugin.json".into(),
                    sha256: sha256(plugin.as_bytes()),
                },
                PluginPackageFile {
                    relative_path: "hooks.json".into(),
                    sha256: sha256(hooks.as_bytes()),
                },
                PluginPackageFile {
                    relative_path: "bridge-manifest.json".into(),
                    sha256: sha256(bridge_manifest.as_bytes()),
                },
            ],
            key_id: trusted_key_id(&signing_key.verifying_key()),
            signature: String::new(),
        };
        manifest.signature = BASE64.encode(
            signing_key
                .sign(&plugin_signing_bytes(&manifest).unwrap())
                .to_bytes(),
        );
        fs::write(
            package.join(PLUGIN_PACKAGE_MANIFEST_FILE),
            serde_json::to_vec_pretty(&manifest).unwrap(),
        )
        .unwrap();
        (root, package, signing_key, manifest)
    }

    #[test]
    fn signed_package_rejects_unsigned_wrong_signer_digest_missing_and_extra_files() {
        let (root, package, signing_key, manifest) = signed_package_fixture("plugin-rejections");
        let verify = |manifest: &PluginPackageManifest| {
            verify_plugin_package(
                &package,
                manifest,
                &signing_key.verifying_key(),
                Some("1.2.0"),
            )
        };
        let mut unsigned = manifest.clone();
        unsigned.signature.clear();
        assert!(verify(&unsigned).is_err());
        let wrong_signer = SigningKey::from_bytes(&[42_u8; 32]);
        assert!(verify_plugin_package(
            &package,
            &manifest,
            &wrong_signer.verifying_key(),
            Some("1.2.0")
        )
        .is_err());
        fs::write(package.join("relintor-antigravity-bridge.exe"), b"altered").unwrap();
        assert!(verify(&manifest).is_err());
        fs::write(
            package.join("relintor-antigravity-bridge.exe"),
            b"signed bridge bytes",
        )
        .unwrap();
        fs::remove_file(package.join("relintor-antigravity-bridge.exe")).unwrap();
        assert!(verify(&manifest).is_err());
        fs::write(
            package.join("relintor-antigravity-bridge.exe"),
            b"signed bridge bytes",
        )
        .unwrap();
        fs::write(package.join("unexpected.txt"), b"not approved").unwrap();
        assert!(verify(&manifest).is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn installed_package_rejects_post_install_file_tampering_and_external_required_identity() {
        let (root, package, signing_key, manifest) = signed_package_fixture("plugin-installed");
        let target = root.join("installed");
        install_verified_plugin(&package, &target, &manifest, &signing_key.verifying_key())
            .unwrap();
        assert!(
            verify_installed_bridge(&target, &signing_key.verifying_key(), Some("1.2.0"),).is_ok()
        );
        fs::write(target.join("relintor-antigravity-bridge.exe"), b"tampered").unwrap();
        assert!(
            verify_installed_plugin(&target, &signing_key.verifying_key(), Some("1.2.0")).is_err()
        );
        let identity = BridgeManifest {
            schema_version: 1,
            adapter_version: ADAPTER_VERSION.into(),
            expected_executable: "bridge".into(),
            expected_sha256: Some(sha256(b"bridge")),
            supported_antigravity_versions: vec!["1.".into()],
            hook_protocol: "relintor-antigravity-hooks-v1".into(),
            signing_status: "external_required".into(),
        };
        let bridge = root.join("bridge");
        fs::write(&bridge, b"bridge").unwrap();
        let adapter = supported_production_adapter();
        assert!(adapter
            .install_or_validate_bridge(&bridge, &identity)
            .is_err());
        let _ = fs::remove_dir_all(root);
    }
}
