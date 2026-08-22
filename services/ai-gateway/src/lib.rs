//! Server-side AI provider boundary and minimum-context broker.
//!
//! Provider credentials are server-only. Runtime selection fails closed for
//! providers that are unavailable, unknown, or not explicitly configured.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::env;
use std::fmt;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;
use uuid::Uuid;

const DEFAULT_BIND_ADDR: &str = "127.0.0.1:8788";
const DEFAULT_TIMEOUT_MS: u64 = 10_000;
const DEFAULT_MAX_CONTEXT_CHARS: usize = 40_000;
const DEFAULT_DEV_BUDGET_UNITS: u64 = 100_000;
const MAX_HTTP_BYTES: usize = 1024 * 1024;
const DEFAULT_OPENAI_ENDPOINT: &str = "https://api.openai.com/v1/responses";
const DEFAULT_DEEPSEEK_ENDPOINT: &str = "https://api.deepseek.com/chat/completions";
const MAX_PROVIDER_RESPONSE_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct GatewayConfig {
    pub bind_addr: String,
    pub provider_name: String,
    pub model: String,
    pub timeout: Duration,
    pub max_context_chars: usize,
    pub dev_budget_units: u64,
}

impl GatewayConfig {
    pub fn from_env() -> Result<Self, String> {
        let timeout_ms = env::var("RELINTOR_AI_TIMEOUT_MS")
            .unwrap_or_else(|_| DEFAULT_TIMEOUT_MS.to_string())
            .parse::<u64>()
            .map_err(|_| "RELINTOR_AI_TIMEOUT_MS must be an integer".to_string())?;
        let max_context_chars = env::var("RELINTOR_AI_MAX_CONTEXT_CHARS")
            .unwrap_or_else(|_| DEFAULT_MAX_CONTEXT_CHARS.to_string())
            .parse::<usize>()
            .map_err(|_| "RELINTOR_AI_MAX_CONTEXT_CHARS must be an integer".to_string())?;
        let dev_budget_units = env::var("RELINTOR_AI_DEV_BUDGET_UNITS")
            .unwrap_or_else(|_| DEFAULT_DEV_BUDGET_UNITS.to_string())
            .parse::<u64>()
            .map_err(|_| "RELINTOR_AI_DEV_BUDGET_UNITS must be an integer".to_string())?;

        if timeout_ms == 0 || max_context_chars == 0 || dev_budget_units == 0 {
            return Err("AI timeout, context budget, and usage budget must be positive".into());
        }

        let provider_name =
            env::var("RELINTOR_AI_PROVIDER").unwrap_or_else(|_| "unconfigured".into());
        let model = env::var("RELINTOR_AI_MODEL")
            .map_err(|_| "RELINTOR_AI_MODEL is required for the AI gateway".to_string())?;
        if model.trim().is_empty() {
            return Err("RELINTOR_AI_MODEL cannot be empty".into());
        }

        Ok(Self {
            bind_addr: env::var("RELINTOR_AI_BIND_ADDR")
                .unwrap_or_else(|_| DEFAULT_BIND_ADDR.into()),
            provider_name,
            model,
            timeout: Duration::from_millis(timeout_ms),
            max_context_chars,
            dev_budget_units,
        })
    }

    pub fn test_config() -> Self {
        Self {
            bind_addr: "127.0.0.1:0".into(),
            provider_name: "mock".into(),
            model: "test-model".into(),
            timeout: Duration::from_millis(100),
            max_context_chars: 100,
            dev_budget_units: 1_000,
        }
    }
}

/// Intentionally non-serializable and non-Debug.
pub struct ProviderCredentials {
    api_key: String,
}

impl ProviderCredentials {
    pub fn from_env() -> Result<Self, String> {
        let api_key = env::var("RELINTOR_PROVIDER_API_KEY")
            .map_err(|_| "RELINTOR_PROVIDER_API_KEY is required for a real provider".to_string())?;
        Self::from_value(Some(&api_key))
    }

    fn from_value(api_key: Option<&str>) -> Result<Self, String> {
        let api_key = api_key.ok_or_else(|| {
            "RELINTOR_PROVIDER_API_KEY is required for a real provider".to_string()
        })?;
        if api_key.trim().is_empty() {
            return Err("RELINTOR_PROVIDER_API_KEY cannot be empty".into());
        }
        Ok(Self {
            api_key: api_key.to_string(),
        })
    }

    pub fn is_present(&self) -> bool {
        !self.api_key.is_empty()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContextItem {
    pub source: String,
    pub text: String,
    pub excluded: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrokeredContext {
    pub included: Vec<ContextItem>,
    pub excluded_count: usize,
    pub redacted_count: usize,
}

pub struct ContextBroker {
    max_chars: usize,
}

impl ContextBroker {
    pub fn new(max_chars: usize) -> Self {
        Self { max_chars }
    }

    pub fn prepare(&self, items: &[ContextItem]) -> BrokeredContext {
        let mut included = Vec::new();
        let mut excluded_count = 0;
        let mut redacted_count = 0;
        let mut used_chars = 0;

        for item in items {
            if item.excluded {
                excluded_count += 1;
                continue;
            }

            if contains_secret(&item.text) || contains_secret(&item.source) {
                redacted_count += 1;
                continue;
            }

            let remaining = self.max_chars.saturating_sub(used_chars);
            if remaining == 0 {
                excluded_count += 1;
                continue;
            }

            let text = item.text.chars().take(remaining).collect::<String>();
            used_chars += text.chars().count();
            included.push(ContextItem {
                source: item.source.clone(),
                text,
                excluded: false,
            });
        }

        BrokeredContext {
            included,
            excluded_count,
            redacted_count,
        }
    }
}

fn contains_secret(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    [
        "sk-",
        "akia",
        "-----begin ",
        "postgres://",
        "postgresql://",
        "password=",
        "\"password\"",
        "'password'",
        "api_key=",
        "apikey=",
        "authorization: bearer",
        "bearer ",
        "private_key",
        "client_secret",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayRequest {
    pub request_id: String,
    pub prompt: String,
    pub context: Vec<ContextItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayResponse {
    pub request_id: String,
    pub model: String,
    pub text: String,
    pub included_context_items: usize,
    pub excluded_context_items: usize,
    pub redacted_context_items: usize,
    pub usage_units: u64,
    pub remaining_budget_units: u64,
    pub provenance: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ProviderRequest {
    pub model: String,
    pub prompt: String,
    pub context: BrokeredContext,
}

#[derive(Debug, Clone)]
pub struct ProviderResponse {
    pub text: String,
    pub provenance: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GatewayError {
    InvalidRequest(String),
    Unauthorized,
    BudgetExceeded,
    ProviderTimeout,
    ProviderUnavailable(String),
    ProviderRejected(String),
}

impl std::fmt::Display for GatewayError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidRequest(message) => write!(f, "invalid request: {message}"),
            Self::Unauthorized => f.write_str("unauthorized"),
            Self::BudgetExceeded => f.write_str("AI usage budget exceeded"),
            Self::ProviderTimeout => f.write_str("provider timeout"),
            Self::ProviderUnavailable(message) => write!(f, "provider unavailable: {message}"),
            Self::ProviderRejected(message) => write!(f, "provider rejected request: {message}"),
        }
    }
}

pub trait ProviderAdapter: Send + Sync + 'static {
    fn complete(&self, request: ProviderRequest) -> Result<ProviderResponse, GatewayError>;
}

#[derive(Debug, Default)]
pub struct MockProvider;

impl ProviderAdapter for MockProvider {
    fn complete(&self, request: ProviderRequest) -> Result<ProviderResponse, GatewayError> {
        if request.prompt.trim().is_empty() {
            return Err(GatewayError::ProviderRejected("prompt is empty".into()));
        }
        let context_sources = request
            .context
            .included
            .iter()
            .map(|item| item.source.clone())
            .collect();
        Ok(ProviderResponse {
            text: format!(
                "Mock response for {} context item(s): {}",
                request.context.included.len(),
                request.prompt.trim()
            ),
            provenance: context_sources,
        })
    }
}

#[derive(Serialize)]
struct OpenAiRequest {
    model: String,
    input: String,
    store: bool,
}

#[derive(Serialize)]
struct DeepSeekMessage {
    role: &'static str,
    content: String,
}

#[derive(Serialize)]
struct DeepSeekRequest {
    model: String,
    messages: Vec<DeepSeekMessage>,
    stream: bool,
}

pub struct OpenAiProvider {
    client: reqwest::blocking::Client,
    credentials: ProviderCredentials,
    endpoint: String,
}

impl fmt::Debug for OpenAiProvider {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("OpenAiProvider")
            .field("endpoint", &self.endpoint)
            .field("credentials", &"[server-side key omitted]")
            .finish()
    }
}

impl OpenAiProvider {
    pub fn from_env(timeout: Duration) -> Result<Self, String> {
        let credentials = ProviderCredentials::from_env()?;
        let endpoint = env::var("RELINTOR_OPENAI_ENDPOINT")
            .unwrap_or_else(|_| DEFAULT_OPENAI_ENDPOINT.to_string());
        Self::new(
            endpoint,
            credentials,
            timeout,
            env::var("RELINTOR_AI_AUTH_MODE").as_deref() == Ok("development")
                && env::var("RELINTOR_AI_ALLOW_LOCAL_PROVIDER_TEST").as_deref() == Ok("1"),
        )
    }

    fn new(
        endpoint: String,
        credentials: ProviderCredentials,
        timeout: Duration,
        allow_local_test_endpoint: bool,
    ) -> Result<Self, String> {
        validate_openai_endpoint(&endpoint, allow_local_test_endpoint)?;
        let client = reqwest::blocking::Client::builder()
            .connect_timeout(timeout)
            .timeout(timeout)
            .build()
            .map_err(|_| "could not construct the provider HTTPS client".to_string())?;
        Ok(Self {
            client,
            credentials,
            endpoint,
        })
    }

    fn complete_request(
        &self,
        request: &ProviderRequest,
    ) -> Result<ProviderResponse, GatewayError> {
        validate_safe_provider_request(request)?;

        let input = brokered_input(request);
        let payload = OpenAiRequest {
            model: request.model.clone(),
            input,
            store: false,
        };
        let response = self
            .client
            .post(&self.endpoint)
            .bearer_auth(&self.credentials.api_key)
            .json(&payload)
            .send()
            .map_err(|error| map_provider_error("OpenAI", error))?;
        let status = response.status();
        if !status.is_success() {
            return if matches!(status.as_u16(), 400 | 401 | 403 | 422 | 429) {
                Err(GatewayError::ProviderRejected(
                    "OpenAI rejected the request".into(),
                ))
            } else {
                Err(GatewayError::ProviderUnavailable(format!(
                    "OpenAI returned HTTP {}",
                    status.as_u16()
                )))
            };
        }

        let body = read_provider_body(response)?;
        let value: serde_json::Value = serde_json::from_slice(&body).map_err(|_| {
            GatewayError::ProviderUnavailable("OpenAI returned malformed JSON".into())
        })?;
        let text = response_text(&value).ok_or_else(|| {
            GatewayError::ProviderRejected("OpenAI returned no usable text".into())
        })?;
        let mut provenance = vec!["provider:openai".into(), format!("model:{}", request.model)];
        provenance.extend(
            request
                .context
                .included
                .iter()
                .map(|item| item.source.clone()),
        );
        Ok(ProviderResponse { text, provenance })
    }
}

pub struct DeepSeekProvider {
    client: reqwest::blocking::Client,
    credentials: ProviderCredentials,
    endpoint: String,
}

impl fmt::Debug for DeepSeekProvider {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DeepSeekProvider")
            .field("endpoint", &self.endpoint)
            .field("credentials", &"[server-side key omitted]")
            .finish()
    }
}

impl DeepSeekProvider {
    pub fn from_env(timeout: Duration) -> Result<Self, String> {
        let credentials = ProviderCredentials::from_env()?;
        let endpoint = env::var("RELINTOR_DEEPSEEK_ENDPOINT")
            .unwrap_or_else(|_| DEFAULT_DEEPSEEK_ENDPOINT.to_string());
        Self::new(
            endpoint,
            credentials,
            timeout,
            env::var("RELINTOR_AI_AUTH_MODE").as_deref() == Ok("development")
                && env::var("RELINTOR_AI_ALLOW_LOCAL_PROVIDER_TEST").as_deref() == Ok("1"),
        )
    }

    fn new(
        endpoint: String,
        credentials: ProviderCredentials,
        timeout: Duration,
        allow_local_test_endpoint: bool,
    ) -> Result<Self, String> {
        validate_deepseek_endpoint(&endpoint, allow_local_test_endpoint)?;
        let client = reqwest::blocking::Client::builder()
            .connect_timeout(timeout)
            .timeout(timeout)
            .build()
            .map_err(|_| "could not construct the provider HTTPS client".to_string())?;
        Ok(Self {
            client,
            credentials,
            endpoint,
        })
    }
}

impl ProviderAdapter for DeepSeekProvider {
    fn complete(&self, request: ProviderRequest) -> Result<ProviderResponse, GatewayError> {
        validate_safe_provider_request(&request)?;
        let payload = DeepSeekRequest {
            model: request.model.clone(),
            messages: vec![DeepSeekMessage {
                role: "user",
                content: brokered_input(&request),
            }],
            stream: false,
        };
        let response = self
            .client
            .post(&self.endpoint)
            .bearer_auth(&self.credentials.api_key)
            .json(&payload)
            .send()
            .map_err(|error| map_provider_error("DeepSeek", error))?;
        let status = response.status();
        if !status.is_success() {
            return if matches!(status.as_u16(), 400 | 401 | 403 | 422 | 429) {
                Err(GatewayError::ProviderRejected(
                    "DeepSeek rejected the request".into(),
                ))
            } else {
                Err(GatewayError::ProviderUnavailable(format!(
                    "DeepSeek returned HTTP {}",
                    status.as_u16()
                )))
            };
        }
        let body = read_provider_body(response)?;
        let value: serde_json::Value = serde_json::from_slice(&body).map_err(|_| {
            GatewayError::ProviderUnavailable("DeepSeek returned malformed JSON".into())
        })?;
        let text = deepseek_response_text(&value).ok_or_else(|| {
            GatewayError::ProviderRejected("DeepSeek returned no usable text".into())
        })?;
        let mut provenance = vec![
            "provider:deepseek".into(),
            format!("model:{}", request.model),
        ];
        provenance.extend(
            request
                .context
                .included
                .iter()
                .map(|item| item.source.clone()),
        );
        Ok(ProviderResponse { text, provenance })
    }
}

fn validate_safe_provider_request(request: &ProviderRequest) -> Result<(), GatewayError> {
    if request.prompt.trim().is_empty()
        || contains_secret(&request.prompt)
        || request.context.included.iter().any(|item| {
            item.excluded || contains_secret(&item.source) || contains_secret(&item.text)
        })
    {
        return Err(GatewayError::InvalidRequest(
            "provider request was not safe to dispatch".into(),
        ));
    }
    Ok(())
}

fn deepseek_response_text(value: &serde_json::Value) -> Option<String> {
    value
        .get("choices")
        .and_then(serde_json::Value::as_array)
        .and_then(|choices| choices.first())
        .and_then(|choice| choice.get("message"))
        .and_then(|message| message.get("content"))
        .and_then(serde_json::Value::as_str)
        .filter(|text| !text.trim().is_empty())
        .map(str::to_string)
}

impl ProviderAdapter for OpenAiProvider {
    fn complete(&self, request: ProviderRequest) -> Result<ProviderResponse, GatewayError> {
        self.complete_request(&request)
    }
}

enum RuntimeProvider {
    Mock(MockProvider),
    OpenAi(OpenAiProvider),
    DeepSeek(DeepSeekProvider),
}

impl ProviderAdapter for RuntimeProvider {
    fn complete(&self, request: ProviderRequest) -> Result<ProviderResponse, GatewayError> {
        match self {
            Self::Mock(provider) => provider.complete(request),
            Self::OpenAi(provider) => provider.complete(request),
            Self::DeepSeek(provider) => provider.complete(request),
        }
    }
}

fn select_provider(
    config: &GatewayConfig,
    api_key: Option<&str>,
    endpoint: Option<&str>,
    allow_mock: bool,
    allow_local_test_endpoint: bool,
) -> Result<RuntimeProvider, String> {
    match config.provider_name.as_str() {
        "mock" if allow_mock => Ok(RuntimeProvider::Mock(MockProvider)),
        "mock" => Err("mock provider is restricted to explicit development/test mode".into()),
        "openai" => {
            let credentials = ProviderCredentials::from_value(api_key)?;
            let endpoint = endpoint.unwrap_or(DEFAULT_OPENAI_ENDPOINT).to_string();
            Ok(RuntimeProvider::OpenAi(OpenAiProvider::new(
                endpoint,
                credentials,
                config.timeout,
                allow_local_test_endpoint,
            )?))
        }
        "deepseek" => {
            let credentials = ProviderCredentials::from_value(api_key)?;
            let endpoint = endpoint.unwrap_or(DEFAULT_DEEPSEEK_ENDPOINT).to_string();
            Ok(RuntimeProvider::DeepSeek(DeepSeekProvider::new(
                endpoint,
                credentials,
                config.timeout,
                allow_local_test_endpoint,
            )?))
        }
        other if other == "unconfigured" || other.is_empty() => Err(
            "RELINTOR_AI_PROVIDER must be explicitly set to openai or an allowed development mock"
                .into(),
        ),
        other => Err(format!("unknown AI provider: {other}")),
    }
}

fn validate_provider_endpoint(
    endpoint: &str,
    default_endpoint: &str,
    provider_name: &str,
    allow_local_test_endpoint: bool,
) -> Result<(), String> {
    let url = reqwest::Url::parse(endpoint)
        .map_err(|_| format!("{provider_name} endpoint is not a valid URL"))?;
    let host = url.host_str().unwrap_or_default();
    let is_loopback = matches!(host, "localhost" | "127.0.0.1" | "::1");
    let is_default = endpoint == default_endpoint && url.scheme() == "https";
    if is_default
        || (allow_local_test_endpoint && is_loopback && matches!(url.scheme(), "http" | "https"))
    {
        return Ok(());
    }
    Err(format!(
        "{provider_name} traffic must use HTTPS; only explicit loopback test endpoints may override it"
    ))
}

fn validate_openai_endpoint(endpoint: &str, allow_local_test_endpoint: bool) -> Result<(), String> {
    validate_provider_endpoint(
        endpoint,
        DEFAULT_OPENAI_ENDPOINT,
        "OpenAI",
        allow_local_test_endpoint,
    )
}

fn validate_deepseek_endpoint(
    endpoint: &str,
    allow_local_test_endpoint: bool,
) -> Result<(), String> {
    validate_provider_endpoint(
        endpoint,
        DEFAULT_DEEPSEEK_ENDPOINT,
        "DeepSeek",
        allow_local_test_endpoint,
    )
}

fn brokered_input(request: &ProviderRequest) -> String {
    let mut input = String::with_capacity(request.prompt.len() + 256);
    input.push_str("Prompt:\n");
    input.push_str(&request.prompt);
    if !request.context.included.is_empty() {
        input.push_str("\n\nBrokered context:\n");
        for item in &request.context.included {
            input.push_str("[Source: ");
            input.push_str(&item.source);
            input.push_str("]\n");
            input.push_str(&item.text);
            input.push('\n');
        }
    }
    input
}

fn map_provider_error(provider_name: &str, error: reqwest::Error) -> GatewayError {
    if error.is_timeout() {
        GatewayError::ProviderTimeout
    } else {
        GatewayError::ProviderUnavailable(format!("{provider_name} network request failed"))
    }
}

fn read_provider_body(response: reqwest::blocking::Response) -> Result<Vec<u8>, GatewayError> {
    if response
        .content_length()
        .is_some_and(|length| length > MAX_PROVIDER_RESPONSE_BYTES as u64)
    {
        return Err(GatewayError::ProviderUnavailable(
            "OpenAI response exceeded the response budget".into(),
        ));
    }
    let mut body = Vec::new();
    response
        .take((MAX_PROVIDER_RESPONSE_BYTES + 1) as u64)
        .read_to_end(&mut body)
        .map_err(|_| GatewayError::ProviderUnavailable("could not read OpenAI response".into()))?;
    if body.len() > MAX_PROVIDER_RESPONSE_BYTES {
        return Err(GatewayError::ProviderUnavailable(
            "OpenAI response exceeded the response budget".into(),
        ));
    }
    Ok(body)
}

fn response_text(value: &serde_json::Value) -> Option<String> {
    if let Some(text) = value.get("output_text").and_then(serde_json::Value::as_str) {
        if !text.trim().is_empty() {
            return Some(text.to_string());
        }
    }
    let mut chunks = Vec::new();
    collect_output_text(value, &mut chunks);
    if chunks.is_empty() {
        None
    } else {
        Some(chunks.join("\n"))
    }
}

fn collect_output_text(value: &serde_json::Value, chunks: &mut Vec<String>) {
    match value {
        serde_json::Value::Array(values) => {
            for value in values {
                collect_output_text(value, chunks);
            }
        }
        serde_json::Value::Object(object) => {
            if object.get("type").and_then(serde_json::Value::as_str) == Some("output_text") {
                if let Some(text) = object.get("text").and_then(serde_json::Value::as_str) {
                    if !text.trim().is_empty() {
                        chunks.push(text.to_string());
                    }
                }
            }
            for value in object.values() {
                collect_output_text(value, chunks);
            }
        }
        _ => {}
    }
}

pub trait AuthorizationValidator: Send + Sync {
    fn authorize(&self, bearer: &str) -> Result<String, GatewayError>;
}

impl AuthorizationValidator for Box<dyn AuthorizationValidator> {
    fn authorize(&self, bearer: &str) -> Result<String, GatewayError> {
        self.as_ref().authorize(bearer)
    }
}

pub struct DevelopmentAuth {
    expected_hash: [u8; 32],
}

pub struct CloudSessionAuth {
    endpoint: reqwest::Url,
    client: reqwest::blocking::Client,
}

impl CloudSessionAuth {
    pub fn new(endpoint: &str) -> Result<Self, String> {
        let endpoint = reqwest::Url::parse(endpoint)
            .map_err(|_| "RELINTOR_CLOUD_API_ENDPOINT must be a valid URL".to_string())?;
        let host = endpoint.host_str().unwrap_or_default();
        if endpoint.scheme() != "https"
            && !(endpoint.scheme() == "http" && matches!(host, "localhost" | "127.0.0.1" | "::1"))
        {
            return Err("cloud auth endpoint must use HTTPS; HTTP is limited to loopback".into());
        }
        let client = reqwest::blocking::Client::builder()
            .connect_timeout(Duration::from_secs(3))
            .timeout(Duration::from_secs(5))
            .build()
            .map_err(|_| "could not construct the cloud auth client".to_string())?;
        Ok(Self { endpoint, client })
    }
}

impl AuthorizationValidator for CloudSessionAuth {
    fn authorize(&self, bearer: &str) -> Result<String, GatewayError> {
        if bearer.trim().is_empty() || bearer.len() > 512 {
            return Err(GatewayError::Unauthorized);
        }
        let url = self
            .endpoint
            .join("/v1/account")
            .map_err(|_| GatewayError::Unauthorized)?;
        let response = self
            .client
            .get(url)
            .bearer_auth(bearer)
            .send()
            .map_err(|_| GatewayError::Unauthorized)?;
        if !response.status().is_success() {
            return Err(GatewayError::Unauthorized);
        }
        let body = response.bytes().map_err(|_| GatewayError::Unauthorized)?;
        let value: serde_json::Value =
            serde_json::from_slice(&body).map_err(|_| GatewayError::Unauthorized)?;
        value
            .get("user")
            .and_then(|user| user.get("id"))
            .and_then(serde_json::Value::as_str)
            .filter(|subject| !subject.trim().is_empty())
            .map(str::to_string)
            .ok_or(GatewayError::Unauthorized)
    }
}

impl DevelopmentAuth {
    pub fn new(token: &str) -> Result<Self, String> {
        if token.len() < 24 {
            return Err("RELINTOR_AI_DEV_BEARER_TOKEN must be at least 24 characters".into());
        }
        Ok(Self {
            expected_hash: sha256_bytes(token.as_bytes()),
        })
    }
}

impl AuthorizationValidator for DevelopmentAuth {
    fn authorize(&self, bearer: &str) -> Result<String, GatewayError> {
        let actual = sha256_bytes(bearer.as_bytes());
        if constant_time_eq(&actual, &self.expected_hash) {
            Ok("development-user".into())
        } else {
            Err(GatewayError::Unauthorized)
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct UsageEntry {
    used: u64,
    limit: u64,
}

pub struct UsageMeter {
    default_limit: u64,
    entries: Mutex<BTreeMap<String, UsageEntry>>,
}

impl UsageMeter {
    pub fn new(default_limit: u64) -> Self {
        Self {
            default_limit,
            entries: Mutex::new(BTreeMap::new()),
        }
    }

    pub fn check_and_commit(&self, subject: &str, units: u64) -> Result<u64, GatewayError> {
        let mut entries = self
            .entries
            .lock()
            .map_err(|_| GatewayError::ProviderUnavailable("usage meter lock poisoned".into()))?;
        let entry = entries.entry(subject.into()).or_insert(UsageEntry {
            used: 0,
            limit: self.default_limit,
        });

        if entry.used.saturating_add(units) > entry.limit {
            return Err(GatewayError::BudgetExceeded);
        }
        entry.used = entry.used.saturating_add(units);
        Ok(entry.limit.saturating_sub(entry.used))
    }

    pub fn used(&self, subject: &str) -> u64 {
        self.entries
            .lock()
            .ok()
            .and_then(|entries| entries.get(subject).copied())
            .map(|entry| entry.used)
            .unwrap_or(0)
    }
}

pub struct Gateway<P: ProviderAdapter> {
    pub config: GatewayConfig,
    pub provider: Arc<P>,
    pub broker: ContextBroker,
    pub usage: UsageMeter,
}

impl<P: ProviderAdapter> Gateway<P> {
    pub fn handle(
        &self,
        subject: &str,
        request: GatewayRequest,
    ) -> Result<GatewayResponse, GatewayError> {
        validate_request_id(&request.request_id)?;

        if request.prompt.trim().is_empty() || request.prompt.chars().count() > 12_000 {
            return Err(GatewayError::InvalidRequest(
                "prompt is empty or exceeds the request budget".into(),
            ));
        }
        if contains_secret(&request.prompt) {
            return Err(GatewayError::InvalidRequest(
                "prompt contains secret-like material and was not sent".into(),
            ));
        }

        let context = self.broker.prepare(&request.context);
        let usage_units = usage_units(&request.prompt, &context);

        // Check against a local foundation meter before provider dispatch.
        // Production persistence/synchronization is a later adapter.
        let already_used = self.usage.used(subject);
        if already_used.saturating_add(usage_units) > self.config.dev_budget_units {
            return Err(GatewayError::BudgetExceeded);
        }

        let provider_request = ProviderRequest {
            model: self.config.model.clone(),
            prompt: request.prompt,
            context: context.clone(),
        };

        let provider = Arc::clone(&self.provider);
        let (tx, rx) = mpsc::sync_channel(1);
        std::thread::spawn(move || {
            let _ = tx.send(provider.complete(provider_request));
        });

        let response = match rx.recv_timeout(self.config.timeout) {
            Ok(result) => result?,
            Err(mpsc::RecvTimeoutError::Timeout) => return Err(GatewayError::ProviderTimeout),
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err(GatewayError::ProviderUnavailable(
                    "provider worker disconnected".into(),
                ))
            }
        };

        let remaining_budget_units = self.usage.check_and_commit(subject, usage_units)?;

        Ok(GatewayResponse {
            request_id: request.request_id,
            model: self.config.model.clone(),
            text: response.text,
            included_context_items: context.included.len(),
            excluded_context_items: context.excluded_count,
            redacted_context_items: context.redacted_count,
            usage_units,
            remaining_budget_units,
            provenance: response.provenance,
        })
    }
}

fn usage_units(prompt: &str, context: &BrokeredContext) -> u64 {
    let context_chars: usize = context
        .included
        .iter()
        .map(|item| item.text.chars().count())
        .sum();
    prompt
        .chars()
        .count()
        .saturating_add(context_chars)
        .try_into()
        .unwrap_or(u64::MAX)
}

pub fn run_from_env() -> Result<(), String> {
    let config = GatewayConfig::from_env()?;

    let development_mode = env::var("RELINTOR_AI_AUTH_MODE").as_deref() == Ok("development");
    let allow_mock = development_mode && env::var("RELINTOR_AI_ALLOW_MOCK").as_deref() == Ok("1");
    let allow_local_test_endpoint =
        development_mode && env::var("RELINTOR_AI_ALLOW_LOCAL_PROVIDER_TEST").as_deref() == Ok("1");
    let endpoint = match config.provider_name.as_str() {
        "deepseek" => env::var("RELINTOR_DEEPSEEK_ENDPOINT").ok(),
        _ => env::var("RELINTOR_OPENAI_ENDPOINT").ok(),
    };
    let provider = select_provider(
        &config,
        env::var("RELINTOR_PROVIDER_API_KEY").ok().as_deref(),
        endpoint.as_deref(),
        allow_mock,
        allow_local_test_endpoint,
    )?;

    let auth: Box<dyn AuthorizationValidator> = if development_mode {
        let dev_token = env::var("RELINTOR_AI_DEV_BEARER_TOKEN").map_err(|_| {
            "RELINTOR_AI_DEV_BEARER_TOKEN is required in development mode".to_string()
        })?;
        Box::new(DevelopmentAuth::new(&dev_token)?)
    } else {
        let cloud_endpoint = env::var("RELINTOR_CLOUD_API_ENDPOINT").map_err(|_| {
            "RELINTOR_CLOUD_API_ENDPOINT is required outside development mode".to_string()
        })?;
        Box::new(CloudSessionAuth::new(&cloud_endpoint)?)
    };

    let listener = TcpListener::bind(&config.bind_addr)
        .map_err(|error| format!("bind AI gateway: {error}"))?;
    eprintln!(
        "Relintor AI gateway listening on {} using {}",
        config.bind_addr, config.provider_name
    );

    let gateway = Gateway {
        broker: ContextBroker::new(config.max_context_chars),
        provider: Arc::new(provider),
        usage: UsageMeter::new(config.dev_budget_units),
        config,
    };

    for mut stream in listener.incoming().flatten() {
        handle_connection(&mut stream, &gateway, &auth);
    }
    Ok(())
}

/// Client-side Rust boundary used by the P8 verifier. The caller supplies a
/// server-owned bearer token; renderer code has no access to this function or
/// the credential. The protocol is deliberately small and uses the same
/// authenticated gateway endpoint as the service listener.
pub fn request_completion(
    endpoint: &str,
    bearer_token: &str,
    request: GatewayRequest,
) -> Result<GatewayResponse, String> {
    let url = reqwest::Url::parse(endpoint)
        .map_err(|_| "AI gateway endpoint must be a valid URL".to_string())?;
    let host = url.host_str().unwrap_or_default();
    if url.scheme() != "https"
        && !(url.scheme() == "http" && matches!(host, "localhost" | "127.0.0.1" | "::1"))
    {
        return Err(
            "AI gateway HTTP is restricted to loopback; remote endpoints require HTTPS".into(),
        );
    }
    let client = reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|_| "could not construct the AI gateway HTTPS client".to_string())?;
    let response = client
        .post(url)
        .bearer_auth(bearer_token)
        .json(&request)
        .send()
        .map_err(|error| {
            if error.is_timeout() {
                "AI gateway request timed out".to_string()
            } else {
                "AI gateway request failed".to_string()
            }
        })?;
    let status = response.status();
    if status != reqwest::StatusCode::OK {
        return Err(format!("AI gateway returned HTTP {}", status.as_u16()));
    }
    let body = read_provider_body(response).map_err(|error| error.to_string())?;
    serde_json::from_slice(&body).map_err(|_| "AI gateway returned malformed JSON".to_string())
}

fn handle_connection<P: ProviderAdapter, A: AuthorizationValidator>(
    stream: &mut TcpStream,
    gateway: &Gateway<P>,
    auth: &A,
) {
    let request = match read_request(stream) {
        Ok(request) => request,
        Err(error) => {
            let _ = write_response(
                stream,
                400,
                &new_request_id(),
                &serde_json::json!({"error": error}).to_string(),
            );
            return;
        }
    };

    let request_id = sanitized_request_id(request.headers.get("x-request-id"));

    if request.method == "GET" && request.path == "/healthz" {
        let _ = write_response(
            stream,
            200,
            &request_id,
            &serde_json::json!({"status":"ok","service":"ai-gateway"}).to_string(),
        );
        return;
    }

    if request.method != "POST" || request.path != "/v1/ai/complete" {
        let _ = write_response(
            stream,
            404,
            &request_id,
            &serde_json::json!({"error":"route not found"}).to_string(),
        );
        return;
    }

    let result = bearer(&request)
        .and_then(|token| auth.authorize(&token))
        .and_then(|subject| {
            serde_json::from_slice::<GatewayRequest>(&request.body)
                .map_err(|_| GatewayError::InvalidRequest("request body is invalid JSON".into()))
                .and_then(|body| gateway.handle(&subject, body))
        });

    match result {
        Ok(body) => {
            let _ = write_response(
                stream,
                200,
                &request_id,
                &serde_json::to_string(&body).unwrap_or_else(|_| "{}".into()),
            );
        }
        Err(error) => {
            let status = match error {
                GatewayError::Unauthorized => 401,
                GatewayError::BudgetExceeded => 429,
                GatewayError::ProviderTimeout | GatewayError::ProviderUnavailable(_) => 503,
                _ => 400,
            };
            let _ = write_response(
                stream,
                status,
                &request_id,
                &serde_json::json!({"error": error.to_string()}).to_string(),
            );
        }
    }
}

struct HttpRequest {
    method: String,
    path: String,
    headers: BTreeMap<String, String>,
    body: Vec<u8>,
}

fn read_request(stream: &mut TcpStream) -> Result<HttpRequest, String> {
    let mut bytes = Vec::new();
    let mut buffer = [0u8; 4096];

    let header_end = loop {
        let count = stream
            .read(&mut buffer)
            .map_err(|error| error.to_string())?;
        if count == 0 {
            return Err("HTTP request ended before headers completed".into());
        }
        bytes.extend_from_slice(&buffer[..count]);
        if bytes.len() > MAX_HTTP_BYTES {
            return Err("request exceeds 1 MiB".into());
        }
        if let Some(position) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            break position;
        }
    };

    let header_text =
        std::str::from_utf8(&bytes[..header_end]).map_err(|_| "HTTP headers are not UTF-8")?;
    let mut lines = header_text.lines();
    let request_line = lines.next().ok_or("HTTP request line is missing")?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next().ok_or("HTTP method is missing")?.to_string();
    let path = parts.next().ok_or("HTTP path is missing")?.to_string();
    let version = parts.next().ok_or("HTTP version is missing")?;

    if version != "HTTP/1.1" && version != "HTTP/1.0" {
        return Err("unsupported HTTP version".into());
    }
    if parts.next().is_some() {
        return Err("malformed HTTP request line".into());
    }

    let mut headers = BTreeMap::new();
    for line in lines {
        let (name, value) = line.split_once(':').ok_or("malformed HTTP header")?;
        let name = name.trim().to_ascii_lowercase();
        if name.is_empty() || value.contains('\r') || value.contains('\n') {
            return Err("malformed HTTP header".into());
        }
        headers.insert(name, value.trim().to_string());
    }

    let content_length = match headers.get("content-length") {
        Some(value) => value
            .parse::<usize>()
            .map_err(|_| "invalid Content-Length")?,
        None => 0,
    };

    let body_start = header_end + 4;
    if body_start.saturating_add(content_length) > MAX_HTTP_BYTES {
        return Err("request exceeds 1 MiB".into());
    }

    while bytes.len() < body_start + content_length {
        let count = stream
            .read(&mut buffer)
            .map_err(|error| error.to_string())?;
        if count == 0 {
            return Err("HTTP request body ended early".into());
        }
        bytes.extend_from_slice(&buffer[..count]);
        if bytes.len() > MAX_HTTP_BYTES {
            return Err("request exceeds 1 MiB".into());
        }
    }

    Ok(HttpRequest {
        method,
        path,
        headers,
        body: bytes[body_start..body_start + content_length].to_vec(),
    })
}

fn bearer(request: &HttpRequest) -> Result<String, GatewayError> {
    request
        .headers
        .get("authorization")
        .and_then(|value| value.strip_prefix("Bearer "))
        .filter(|value| !value.is_empty() && value.len() <= 512)
        .map(str::to_string)
        .ok_or(GatewayError::Unauthorized)
}

fn validate_request_id(value: &str) -> Result<(), GatewayError> {
    if value.is_empty()
        || value.len() > 80
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
    {
        Err(GatewayError::InvalidRequest("request ID is invalid".into()))
    } else {
        Ok(())
    }
}

fn sanitized_request_id(header: Option<&String>) -> String {
    header
        .filter(|value| validate_request_id(value).is_ok())
        .cloned()
        .unwrap_or_else(new_request_id)
}

fn new_request_id() -> String {
    format!("req_{}", Uuid::new_v4().simple())
}

fn write_response(
    stream: &mut TcpStream,
    status: u16,
    request_id: &str,
    body: &str,
) -> std::io::Result<()> {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        429 => "Too Many Requests",
        503 => "Service Unavailable",
        _ => "Internal Server Error",
    };
    let response = format!(
        "HTTP/1.1 {status} {reason}\r\n\
         Content-Type: application/json\r\n\
         Content-Length: {}\r\n\
         X-Request-Id: {request_id}\r\n\
         X-Content-Type-Options: nosniff\r\n\
         X-Frame-Options: DENY\r\n\
         Referrer-Policy: no-referrer\r\n\
         Cache-Control: no-store\r\n\
         Connection: close\r\n\r\n\
         {body}",
        body.len()
    );
    stream.write_all(response.as_bytes())
}

fn sha256_bytes(bytes: &[u8]) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(bytes);
    digest.finalize().into()
}

fn constant_time_eq(left: &[u8; 32], right: &[u8; 32]) -> bool {
    let mut difference = 0u8;
    for (a, b) in left.iter().zip(right.iter()) {
        difference |= a ^ b;
    }
    difference == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::Receiver;

    fn request() -> GatewayRequest {
        GatewayRequest {
            request_id: "req-test".into(),
            prompt: "summarize the evidence".into(),
            context: vec![
                ContextItem {
                    source: "included.md".into(),
                    text: "safe context".into(),
                    excluded: false,
                },
                ContextItem {
                    source: "ignored.md".into(),
                    text: "must not leave the broker".into(),
                    excluded: true,
                },
                ContextItem {
                    source: "secret.env".into(),
                    text: "OPENAI_API_KEY=sk-secret".into(),
                    excluded: false,
                },
            ],
        }
    }

    fn gateway() -> Gateway<MockProvider> {
        let config = GatewayConfig::test_config();
        Gateway {
            broker: ContextBroker::new(config.max_context_chars),
            provider: Arc::new(MockProvider),
            usage: UsageMeter::new(config.dev_budget_units),
            config,
        }
    }

    fn provider_request() -> ProviderRequest {
        ProviderRequest {
            model: "test-model-from-config".into(),
            prompt: "return a concise answer".into(),
            context: BrokeredContext {
                included: vec![ContextItem {
                    source: "safe.md".into(),
                    text: "safe brokered context".into(),
                    excluded: false,
                }],
                excluded_count: 1,
                redacted_count: 1,
            },
        }
    }

    fn mock_openai_server(
        status: u16,
        response_body: &'static str,
        delay: Duration,
    ) -> (String, Receiver<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}/v1/responses", listener.local_addr().unwrap());
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut bytes = Vec::new();
            let mut buffer = [0u8; 4096];
            let header_end = loop {
                let count = stream.read(&mut buffer).unwrap();
                if count == 0 {
                    return;
                }
                bytes.extend_from_slice(&buffer[..count]);
                if let Some(position) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
                    break position;
                }
            };
            let headers = String::from_utf8_lossy(&bytes[..header_end]);
            let content_length = headers
                .lines()
                .find_map(|line| {
                    line.strip_prefix("Content-Length:")
                        .or_else(|| line.strip_prefix("content-length:"))
                })
                .and_then(|value| value.trim().parse::<usize>().ok())
                .unwrap_or(0);
            let body_start = header_end + 4;
            while bytes.len() < body_start + content_length {
                let count = stream.read(&mut buffer).unwrap();
                if count == 0 {
                    return;
                }
                bytes.extend_from_slice(&buffer[..count]);
            }
            let request =
                String::from_utf8_lossy(&bytes[..body_start + content_length]).to_string();
            let _ = sender.send(request);
            std::thread::sleep(delay);
            let reason = if status == 200 { "OK" } else { "Error" };
            let response = format!(
                "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response_body}",
                response_body.len()
            );
            let _ = stream.write_all(response.as_bytes());
        });
        (endpoint, receiver)
    }

    #[test]
    fn mock_provider_is_deterministic_and_secrets_are_removed() {
        let response = gateway().handle("user-1", request()).unwrap();
        assert_eq!(response.included_context_items, 1);
        assert_eq!(response.excluded_context_items, 1);
        assert_eq!(response.redacted_context_items, 1);
        assert!(!response.text.contains("sk-secret"));
        assert!(response.usage_units > 0);
    }

    #[test]
    fn secret_in_prompt_fails_before_provider_dispatch() {
        let mut request = request();
        request.prompt = "use password=hunter2".into();
        assert!(matches!(
            gateway().handle("user-1", request),
            Err(GatewayError::InvalidRequest(_))
        ));
    }

    #[test]
    fn secret_in_source_name_is_redacted() {
        let broker = ContextBroker::new(100);
        let result = broker.prepare(&[ContextItem {
            source: "postgresql://user:pass@host/db".into(),
            text: "otherwise safe".into(),
            excluded: false,
        }]);
        assert!(result.included.is_empty());
        assert_eq!(result.redacted_count, 1);
    }

    #[test]
    fn provider_timeout_is_a_real_response_deadline() {
        struct SlowProvider;
        impl ProviderAdapter for SlowProvider {
            fn complete(
                &self,
                _request: ProviderRequest,
            ) -> Result<ProviderResponse, GatewayError> {
                std::thread::sleep(Duration::from_millis(50));
                Ok(ProviderResponse {
                    text: "late".into(),
                    provenance: vec![],
                })
            }
        }

        let mut config = GatewayConfig::test_config();
        config.timeout = Duration::from_millis(1);
        let gateway = Gateway {
            broker: ContextBroker::new(100),
            provider: Arc::new(SlowProvider),
            usage: UsageMeter::new(1_000),
            config,
        };

        assert!(matches!(
            gateway.handle("user-1", request()),
            Err(GatewayError::ProviderTimeout)
        ));
    }

    #[test]
    fn provider_outage_is_normalized() {
        struct DownProvider;
        impl ProviderAdapter for DownProvider {
            fn complete(
                &self,
                _request: ProviderRequest,
            ) -> Result<ProviderResponse, GatewayError> {
                Err(GatewayError::ProviderUnavailable("offline".into()))
            }
        }

        let config = GatewayConfig::test_config();
        let gateway = Gateway {
            broker: ContextBroker::new(100),
            provider: Arc::new(DownProvider),
            usage: UsageMeter::new(1_000),
            config,
        };

        assert!(matches!(
            gateway.handle("user-1", request()),
            Err(GatewayError::ProviderUnavailable(_))
        ));
    }

    #[test]
    fn usage_budget_is_counted_and_enforced() {
        let mut config = GatewayConfig::test_config();
        config.dev_budget_units = 30;
        let gateway = Gateway {
            broker: ContextBroker::new(100),
            provider: Arc::new(MockProvider),
            usage: UsageMeter::new(30),
            config,
        };

        let mut first = request();
        first.prompt = "1234567890".into();
        first.context.clear();
        let response = gateway.handle("user-1", first).unwrap();
        assert_eq!(response.usage_units, 10);
        assert_eq!(gateway.usage.used("user-1"), 10);

        let mut second = request();
        second.prompt = "1234567890123456789012345".into();
        second.context.clear();
        assert!(matches!(
            gateway.handle("user-1", second),
            Err(GatewayError::BudgetExceeded)
        ));
        assert_eq!(gateway.usage.used("user-1"), 10);
    }

    #[test]
    fn development_auth_rejects_arbitrary_bearer_tokens() {
        let auth = DevelopmentAuth::new("this-is-a-long-development-token").unwrap();
        assert!(auth.authorize("this-is-a-long-development-token").is_ok());
        assert_eq!(
            auth.authorize("wrong-token"),
            Err(GatewayError::Unauthorized)
        );
    }

    #[test]
    fn production_cloud_session_auth_accepts_only_a_valid_relintor_session() {
        let (endpoint, requests) =
            mock_openai_server(200, r#"{"user":{"id":"user-from-cloud"}}"#, Duration::ZERO);
        let auth = CloudSessionAuth::new(&endpoint).unwrap();
        assert_eq!(
            auth.authorize("relintor-access-token").unwrap(),
            "user-from-cloud"
        );
        let request = requests.recv().unwrap();
        assert!(request
            .to_ascii_lowercase()
            .contains("authorization: bearer relintor-access-token"));
        assert!(CloudSessionAuth::new("http://example.com").is_err());
    }

    #[test]
    fn response_request_id_cannot_contain_header_injection() {
        assert_ne!(
            sanitized_request_id(Some(&"ok\r\nInjected: yes".into())),
            "ok\r\nInjected: yes"
        );
    }

    #[test]
    fn non_mock_provider_configuration_fails_closed_at_runtime_boundary() {
        let mut config = GatewayConfig::test_config();
        config.provider_name = "real-provider-not-yet-implemented".into();
        assert!(select_provider(&config, Some("server-test-key"), None, false, false).is_err());
    }

    #[test]
    fn explicit_mock_selection_is_the_only_mock_runtime_path() {
        let config = GatewayConfig::test_config();
        assert!(select_provider(&config, None, None, true, false).is_ok());
        assert!(select_provider(&config, None, None, false, false).is_err());
    }

    #[test]
    fn openai_selection_requires_non_empty_server_key() {
        let mut config = GatewayConfig::test_config();
        config.provider_name = "openai".into();
        assert!(select_provider(&config, None, None, false, false).is_err());
        assert!(select_provider(&config, Some(""), None, false, false).is_err());
        assert!(select_provider(&config, Some("server-test-key"), None, false, false).is_ok());
    }

    #[test]
    fn deepseek_selection_is_explicit_and_requires_non_empty_server_key() {
        let mut config = GatewayConfig::test_config();
        config.provider_name = "deepseek".into();
        assert!(select_provider(&config, None, None, false, false).is_err());
        assert!(select_provider(&config, Some(""), None, false, false).is_err());
        assert!(matches!(
            select_provider(&config, Some("server-test-key"), None, false, false),
            Ok(RuntimeProvider::DeepSeek(_))
        ));
    }

    #[test]
    fn unknown_and_unconfigured_provider_names_fail_closed() {
        let mut config = GatewayConfig::test_config();
        config.provider_name = "unknown".into();
        assert!(select_provider(&config, None, None, false, false).is_err());
        config.provider_name = "unconfigured".into();
        assert!(select_provider(&config, None, None, false, false).is_err());
    }

    #[test]
    fn openai_request_uses_model_bearer_store_false_and_brokered_context() {
        let (endpoint, requests) = mock_openai_server(
            200,
            r#"{"output":[{"type":"message","content":[{"type":"output_text","text":"safe answer"}]}]}"#,
            Duration::ZERO,
        );
        let provider = OpenAiProvider::new(
            endpoint,
            ProviderCredentials::from_value(Some("server-test-key")).unwrap(),
            Duration::from_secs(1),
            true,
        )
        .unwrap();
        let response = provider.complete(provider_request()).unwrap();
        assert_eq!(response.text, "safe answer");
        let request = requests.recv_timeout(Duration::from_secs(1)).unwrap();
        assert!(request.contains("authorization: Bearer server-test-key"));
        assert!(request.contains("test-model-from-config"));
        assert!(request.contains("\"store\":false"));
        assert!(request.contains("safe brokered context"));
        assert!(!request.contains("ignored.md"));
    }

    #[test]
    fn deepseek_request_uses_configured_model_messages_and_bearer() {
        let (endpoint, requests) = mock_openai_server(
            200,
            r#"{"choices":[{"message":{"content":"deep answer"}}]}"#,
            Duration::ZERO,
        );
        let provider = DeepSeekProvider::new(
            endpoint,
            ProviderCredentials::from_value(Some("server-test-key")).unwrap(),
            Duration::from_secs(1),
            true,
        )
        .unwrap();
        let mut request = provider_request();
        request.model = "deepseek-v4-flash".into();
        let response = provider.complete(request).unwrap();
        assert_eq!(response.text, "deep answer");
        assert!(response.provenance.contains(&"provider:deepseek".into()));
        let captured = requests.recv_timeout(Duration::from_secs(1)).unwrap();
        assert!(captured.contains("authorization: Bearer server-test-key"));
        assert!(captured.contains("\"model\":\"deepseek-v4-flash\""));
        assert!(captured.contains("\"messages\":[{"));
        assert!(captured.contains("\"role\":\"user\""));
        assert!(captured.contains("return a concise answer"));
        assert!(captured.contains("\"stream\":false"));
    }

    #[test]
    fn openai_key_is_absent_from_debug_and_provider_errors() {
        let (endpoint, _) =
            mock_openai_server(401, r#"{"error":"server-test-key"}"#, Duration::ZERO);
        let provider = OpenAiProvider::new(
            endpoint,
            ProviderCredentials::from_value(Some("server-test-key")).unwrap(),
            Duration::from_secs(1),
            true,
        )
        .unwrap();
        let debug = format!("{provider:?}");
        let error = provider
            .complete(provider_request())
            .unwrap_err()
            .to_string();
        assert!(!debug.contains("server-test-key"));
        assert!(!error.contains("server-test-key"));
    }

    #[test]
    fn secret_prompt_is_rejected_before_openai_dispatch() {
        let (endpoint, requests) =
            mock_openai_server(200, r#"{"output_text":"must not run"}"#, Duration::ZERO);
        let provider = OpenAiProvider::new(
            endpoint,
            ProviderCredentials::from_value(Some("server-test-key")).unwrap(),
            Duration::from_secs(1),
            true,
        )
        .unwrap();
        let mut request = provider_request();
        request.prompt = "password=do-not-send".into();
        assert!(matches!(
            provider.complete(request),
            Err(GatewayError::InvalidRequest(_))
        ));
        assert!(requests.recv_timeout(Duration::from_millis(50)).is_err());
    }

    #[test]
    fn provider_timeout_and_connection_failure_are_normalized() {
        let (endpoint, _) =
            mock_openai_server(200, r#"{"output_text":"late"}"#, Duration::from_millis(100));
        let provider = OpenAiProvider::new(
            endpoint,
            ProviderCredentials::from_value(Some("server-test-key")).unwrap(),
            Duration::from_millis(10),
            true,
        )
        .unwrap();
        assert!(matches!(
            provider.complete(provider_request()),
            Err(GatewayError::ProviderTimeout)
        ));

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let unavailable = format!("http://{}/v1/responses", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            drop(stream);
        });
        let provider = OpenAiProvider::new(
            unavailable,
            ProviderCredentials::from_value(Some("server-test-key")).unwrap(),
            Duration::from_secs(1),
            true,
        )
        .unwrap();
        assert!(matches!(
            provider.complete(provider_request()),
            Err(GatewayError::ProviderUnavailable(_))
        ));
    }

    #[test]
    fn non_success_and_malformed_responses_never_become_success() {
        let (endpoint, _) =
            mock_openai_server(500, r#"{"output_text":"not success"}"#, Duration::ZERO);
        let provider = OpenAiProvider::new(
            endpoint,
            ProviderCredentials::from_value(Some("server-test-key")).unwrap(),
            Duration::from_secs(1),
            true,
        )
        .unwrap();
        assert!(matches!(
            provider.complete(provider_request()),
            Err(GatewayError::ProviderUnavailable(_))
        ));

        let (endpoint, _) = mock_openai_server(200, "not-json", Duration::ZERO);
        let provider = OpenAiProvider::new(
            endpoint,
            ProviderCredentials::from_value(Some("server-test-key")).unwrap(),
            Duration::from_secs(1),
            true,
        )
        .unwrap();
        assert!(matches!(
            provider.complete(provider_request()),
            Err(GatewayError::ProviderUnavailable(_))
        ));
    }

    #[test]
    fn deepseek_non_success_malformed_and_missing_content_fail_closed() {
        let cases = [
            (
                500,
                r#"{"choices":[{"message":{"content":"not success"}}]}"#,
            ),
            (200, "not-json"),
            (200, r#"{"choices":[]}"#),
            (200, r#"{"choices":[{"message":{}}]}"#),
            (200, r#"{"choices":[{"message":{"content":""}}]}"#),
        ];
        for (status, body) in cases {
            let (endpoint, _) = mock_openai_server(status, body, Duration::ZERO);
            let provider = DeepSeekProvider::new(
                endpoint,
                ProviderCredentials::from_value(Some("server-test-key")).unwrap(),
                Duration::from_secs(1),
                true,
            )
            .unwrap();
            assert!(provider.complete(provider_request()).is_err());
        }
    }

    #[test]
    fn deepseek_key_is_absent_from_debug_and_network_errors() {
        let (endpoint, _) =
            mock_openai_server(401, r#"{"error":"server-test-key"}"#, Duration::ZERO);
        let provider = DeepSeekProvider::new(
            endpoint,
            ProviderCredentials::from_value(Some("server-test-key")).unwrap(),
            Duration::from_secs(1),
            true,
        )
        .unwrap();
        let debug = format!("{provider:?}");
        let error = provider
            .complete(provider_request())
            .unwrap_err()
            .to_string();
        assert!(!debug.contains("server-test-key"));
        assert!(!error.contains("server-test-key"));
    }

    #[test]
    fn response_parser_accepts_nested_output_text_and_rejects_empty_output() {
        let valid = serde_json::json!({
            "output": [{"type": "message", "content": [
                {"type": "output_text", "text": "first"},
                {"type": "output_text", "text": "second"}
            ]}]
        });
        assert_eq!(response_text(&valid).as_deref(), Some("first\nsecond"));
        assert!(response_text(&serde_json::json!({"output": []})).is_none());
    }

    #[test]
    fn endpoint_policy_rejects_remote_plaintext_and_allows_loopback_test_http() {
        assert!(validate_openai_endpoint("http://example.com/v1/responses", true).is_err());
        assert!(validate_openai_endpoint("http://127.0.0.1:1234/v1/responses", true).is_ok());
        assert!(validate_openai_endpoint(DEFAULT_OPENAI_ENDPOINT, false).is_ok());
        let request = request();
        assert!(request_completion("http://example.com", "server-test-key", request).is_err());
    }

    #[test]
    fn deepseek_endpoint_policy_requires_https_except_explicit_loopback_tests() {
        assert!(validate_deepseek_endpoint(DEFAULT_DEEPSEEK_ENDPOINT, false).is_ok());
        assert!(validate_deepseek_endpoint("http://example.com/chat/completions", true).is_err());
        assert!(
            validate_deepseek_endpoint("http://127.0.0.1:1234/chat/completions", false).is_err()
        );
        assert!(validate_deepseek_endpoint("http://127.0.0.1:1234/chat/completions", true).is_ok());
    }

    #[test]
    #[ignore = "LIVE BILLABLE: requires explicit RELINTOR_LIVE_OPENAI_TEST=1 and a real provider key"]
    fn live_openai_response_is_non_empty_when_explicitly_enabled() {
        if env::var("RELINTOR_LIVE_OPENAI_TEST").as_deref() != Ok("1") {
            return;
        }
        let model = env::var("RELINTOR_AI_MODEL").expect("RELINTOR_AI_MODEL");
        let provider =
            OpenAiProvider::from_env(Duration::from_secs(30)).expect("OpenAI configuration");
        let response = provider
            .complete(ProviderRequest {
                model,
                prompt: "Reply with exactly the word READY.".into(),
                context: BrokeredContext {
                    included: vec![],
                    excluded_count: 0,
                    redacted_count: 0,
                },
            })
            .expect("live OpenAI request");
        assert!(!response.text.trim().is_empty());
    }

    #[test]
    #[ignore = "LIVE BILLABLE: requires explicit RELINTOR_LIVE_DEEPSEEK_TEST=1 and a real provider key"]
    fn live_deepseek_response_is_non_empty_when_explicitly_enabled() {
        if env::var("RELINTOR_LIVE_DEEPSEEK_TEST").as_deref() != Ok("1") {
            return;
        }
        let model = env::var("RELINTOR_AI_MODEL").expect("RELINTOR_AI_MODEL");
        let provider =
            DeepSeekProvider::from_env(Duration::from_secs(30)).expect("DeepSeek configuration");
        let response = provider
            .complete(ProviderRequest {
                model,
                prompt: "Reply with exactly the word READY.".into(),
                context: BrokeredContext {
                    included: vec![],
                    excluded_count: 0,
                    redacted_count: 0,
                },
            })
            .expect("live DeepSeek request");
        assert!(!response.text.trim().is_empty());
    }
}
