//! Relintor cloud account/organization/entitlement authority.
//!
//! P10 extends the PostgreSQL-backed P2 foundation with durable organization,
//! policy, grant, subscription, founder/admin, MFA, and audit authority. The
//! deterministic memory repository exists only for unit tests. External
//! identity provisioning and a production billing provider remain explicit
//! environment dependencies and are fail-closed at runtime.

use base64::{
    engine::general_purpose::{STANDARD as BASE64, URL_SAFE_NO_PAD},
    Engine as _,
};
use postgres::{Client, NoTls};
use relintor_contracts::{
    capability_names, sign_entitlement, EntitlementClaims, EntitlementSource, PlanTier,
    SignedEntitlement, ENTITLEMENT_SCHEMA_VERSION,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::env;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use uuid::Uuid;

mod p10;
mod p11;
pub use p10::{BillingAdapter, BillingStatus, UnavailableBillingAdapter};
pub use p11::{
    service_status, standards_distribution_status, RolloutStatusView, ServiceStatusView,
    StandardsDistributionStatusView,
};

pub const REQUEST_ID_HEADER: &str = "x-request-id";
const DEFAULT_BIND_ADDR: &str = "127.0.0.1:8787";
const DEFAULT_GRACE_SECONDS: i64 = 7 * 24 * 60 * 60;
const MAX_HTTP_BYTES: usize = 1024 * 1024;
const GOOGLE_ISSUER: &str = "https://accounts.google.com";
const DEFAULT_GOOGLE_JWKS_URL: &str = "https://www.googleapis.com/oauth2/v3/certs";
const MAX_GOOGLE_JWKS_BYTES: usize = 512 * 1024;

const MIGRATION_002: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../db/migrations/002_cloud_account_foundation.sql"
));
const MIGRATION_003: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../db/migrations/003_milestone2_reconciliation.sql"
));
const MIGRATION_009: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../db/migrations/009_p10_teams_billing_admin.sql"
));
const MIGRATION_010: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../db/migrations/010_google_external_identities.sql"
));

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthAdapter {
    Development,
    GoogleOidc,
}

impl AuthAdapter {
    fn from_env(value: &str) -> Result<Self, String> {
        match value {
            "development" => Ok(Self::Development),
            "google_oidc" => Ok(Self::GoogleOidc),
            _ => Err("RELINTOR_AUTH_ADAPTER must be development or google_oidc".into()),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Config {
    pub bind_addr: String,
    pub database_url: String,
    pub issuer: String,
    pub grace_seconds: i64,
    pub dev_auth_adapter: bool,
    pub auth_adapter: AuthAdapter,
    pub google_client_id: Option<String>,
    pub google_jwks_url: String,
    pub signing_key: [u8; 32],
    pub key_id: String,
    pub release_channel: String,
    pub supported_versions: Vec<String>,
    pub standards_distribution_manifest: Option<String>,
}

impl Config {
    pub fn from_env() -> Result<Self, String> {
        let database_url = required_env("RELINTOR_DATABASE_URL")?;
        validate_database_url(&database_url)?;

        let signing_key_text = required_env("RELINTOR_ENTITLEMENT_SIGNING_KEY")?;
        let signing_key = BASE64
            .decode(signing_key_text)
            .map_err(|_| "RELINTOR_ENTITLEMENT_SIGNING_KEY must be base64".to_string())?;
        let signing_key: [u8; 32] = signing_key
            .try_into()
            .map_err(|_| "RELINTOR_ENTITLEMENT_SIGNING_KEY must decode to 32 bytes".to_string())?;

        let grace_seconds = env::var("RELINTOR_ENTITLEMENT_GRACE_SECONDS")
            .unwrap_or_else(|_| DEFAULT_GRACE_SECONDS.to_string())
            .parse::<i64>()
            .map_err(|_| "RELINTOR_ENTITLEMENT_GRACE_SECONDS must be an integer".to_string())?;
        if grace_seconds < 0 {
            return Err("RELINTOR_ENTITLEMENT_GRACE_SECONDS cannot be negative".into());
        }

        let key_id = required_env("RELINTOR_ENTITLEMENT_KEY_ID")?;
        if key_id.trim().is_empty() || key_id.len() > 120 {
            return Err("RELINTOR_ENTITLEMENT_KEY_ID is invalid".into());
        }

        let release_channel =
            env::var("RELINTOR_RELEASE_CHANNEL").unwrap_or_else(|_| "unconfigured".into());
        if !matches!(
            release_channel.as_str(),
            "stable" | "beta" | "canary" | "unconfigured"
        ) {
            return Err(
                "RELINTOR_RELEASE_CHANNEL must be stable, beta, canary, or unconfigured".into(),
            );
        }
        let supported_versions = env::var("RELINTOR_SUPPORTED_VERSIONS")
            .unwrap_or_else(|_| env!("CARGO_PKG_VERSION").into())
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .collect::<Vec<_>>();
        if supported_versions.is_empty() {
            return Err("RELINTOR_SUPPORTED_VERSIONS must contain a version".into());
        }

        let auth_adapter = AuthAdapter::from_env(
            &env::var("RELINTOR_AUTH_ADAPTER").unwrap_or_else(|_| "unconfigured".into()),
        )?;
        let google_client_id = env::var("RELINTOR_GOOGLE_CLIENT_ID").ok();
        if auth_adapter == AuthAdapter::GoogleOidc
            && google_client_id.as_deref().is_none_or(str::is_empty)
        {
            return Err("RELINTOR_GOOGLE_CLIENT_ID is required for google_oidc auth".into());
        }

        Ok(Self {
            bind_addr: env::var("RELINTOR_BIND_ADDR").unwrap_or_else(|_| DEFAULT_BIND_ADDR.into()),
            database_url,
            issuer: required_env("RELINTOR_ENTITLEMENT_ISSUER")?,
            grace_seconds,
            dev_auth_adapter: auth_adapter == AuthAdapter::Development,
            auth_adapter,
            google_client_id,
            google_jwks_url: env::var("RELINTOR_GOOGLE_JWKS_URL")
                .unwrap_or_else(|_| DEFAULT_GOOGLE_JWKS_URL.into()),
            signing_key,
            key_id,
            release_channel,
            supported_versions,
            standards_distribution_manifest: env::var("RELINTOR_STANDARDS_DISTRIBUTION_MANIFEST")
                .ok(),
        })
    }

    pub fn test_config() -> Self {
        Self {
            bind_addr: "127.0.0.1:0".into(),
            database_url: "postgresql://localhost/relintor_test".into(),
            issuer: "relintor-cloud-test".into(),
            grace_seconds: 60,
            dev_auth_adapter: true,
            auth_adapter: AuthAdapter::Development,
            google_client_id: None,
            google_jwks_url: DEFAULT_GOOGLE_JWKS_URL.into(),
            signing_key: [7u8; 32],
            key_id: "test-2026-01".into(),
            release_channel: "stable".into(),
            supported_versions: vec![env!("CARGO_PKG_VERSION").into()],
            standards_distribution_manifest: None,
        }
    }
}

fn required_env(name: &str) -> Result<String, String> {
    env::var(name).map_err(|_| format!("{name} is required"))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GoogleIdentity {
    pub provider_subject: String,
    pub email: Option<String>,
    pub email_verified: bool,
}

#[derive(Debug, Deserialize)]
struct GoogleJwtHeader {
    alg: String,
    kid: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GoogleClaims {
    iss: String,
    aud: String,
    exp: i64,
    sub: String,
    email: Option<String>,
    #[serde(default)]
    email_verified: bool,
}

#[derive(Debug, Clone)]
struct JwksCache {
    expires_at: SystemTime,
    value: serde_json::Value,
}

#[derive(Clone)]
pub struct GoogleOidcValidator {
    client_id: String,
    jwks_url: String,
    client: reqwest::blocking::Client,
    cache: Arc<Mutex<Option<JwksCache>>>,
}

impl GoogleOidcValidator {
    pub fn new(client_id: String, jwks_url: String) -> Result<Self, String> {
        validate_google_jwks_url(&jwks_url, false)?;
        let client = reqwest::blocking::Client::builder()
            .connect_timeout(Duration::from_secs(3))
            .timeout(Duration::from_secs(5))
            .build()
            .map_err(|_| "could not construct the Google OIDC client".to_string())?;
        Ok(Self {
            client_id,
            jwks_url,
            client,
            cache: Arc::new(Mutex::new(None)),
        })
    }

    #[cfg(test)]
    fn for_test(client_id: &str, jwks_url: &str) -> Result<Self, String> {
        validate_google_jwks_url(jwks_url, true)?;
        let client = reqwest::blocking::Client::builder()
            .connect_timeout(Duration::from_secs(1))
            .timeout(Duration::from_secs(2))
            .build()
            .map_err(|_| "could not construct the test OIDC client".to_string())?;
        Ok(Self {
            client_id: client_id.into(),
            jwks_url: jwks_url.into(),
            client,
            cache: Arc::new(Mutex::new(None)),
        })
    }

    pub fn validate_id_token(&self, token: &str) -> Result<GoogleIdentity, ApiError> {
        let parts = token.split('.').collect::<Vec<_>>();
        if parts.len() != 3 {
            return Err(ApiError::unauthorized(
                "Google authentication could not be verified",
            ));
        }
        let header: GoogleJwtHeader = decode_jwt_json(parts[0])?;
        if header.alg != "RS256" {
            return Err(ApiError::unauthorized(
                "Google authentication could not be verified",
            ));
        }
        let kid = header
            .kid
            .as_deref()
            .ok_or_else(|| ApiError::unauthorized("Google authentication could not be verified"))?;
        let decoding_key = self.decoding_key(kid)?;
        let signature = URL_SAFE_NO_PAD
            .decode(parts[2])
            .map_err(|_| ApiError::unauthorized("Google authentication could not be verified"))?;
        ring::signature::UnparsedPublicKey::new(
            &ring::signature::RSA_PKCS1_2048_8192_SHA256,
            decoding_key,
        )
        .verify(format!("{}.{}", parts[0], parts[1]).as_bytes(), &signature)
        .map_err(|_| ApiError::unauthorized("Google authentication could not be verified"))?;
        let claims: GoogleClaims = decode_jwt_json(parts[1])?;
        if claims.iss != GOOGLE_ISSUER
            || claims.aud != self.client_id
            || claims.exp <= now_seconds()
            || claims.sub.trim().is_empty()
        {
            return Err(ApiError::unauthorized(
                "Google authentication could not be verified",
            ));
        }
        if claims.email.is_some() && !claims.email_verified {
            return Err(ApiError::unauthorized(
                "Google authentication could not be verified",
            ));
        }
        Ok(GoogleIdentity {
            provider_subject: claims.sub,
            email: claims.email.map(|email| email.to_ascii_lowercase()),
            email_verified: claims.email_verified,
        })
    }

    fn decoding_key(&self, kid: &str) -> Result<Vec<u8>, ApiError> {
        if let Some(value) = self.cached_key(kid) {
            return jwk_to_rsa_public_der(&value);
        }
        let value = self.fetch_jwks()?;
        let key = jwk_for_kid(&value, kid)
            .ok_or_else(|| ApiError::unauthorized("Google authentication could not be verified"))?;
        jwk_to_rsa_public_der(&key)
    }

    fn cached_key(&self, kid: &str) -> Option<serde_json::Value> {
        let guard = self.cache.lock().ok()?;
        let cache = guard.as_ref()?;
        if cache.expires_at <= SystemTime::now() {
            return None;
        }
        jwk_for_kid(&cache.value, kid)
    }

    fn fetch_jwks(&self) -> Result<serde_json::Value, ApiError> {
        let response =
            self.client.get(&self.jwks_url).send().map_err(|_| {
                ApiError::service_unavailable("Google signing keys are unavailable")
            })?;
        if !response.status().is_success()
            || response
                .content_length()
                .is_some_and(|length| length > MAX_GOOGLE_JWKS_BYTES as u64)
        {
            return Err(ApiError::service_unavailable(
                "Google signing keys are unavailable",
            ));
        }
        let bytes = response
            .bytes()
            .map_err(|_| ApiError::service_unavailable("Google signing keys are unavailable"))?;
        if bytes.len() > MAX_GOOGLE_JWKS_BYTES {
            return Err(ApiError::service_unavailable(
                "Google signing keys are unavailable",
            ));
        }
        let value: serde_json::Value = serde_json::from_slice(&bytes)
            .map_err(|_| ApiError::service_unavailable("Google signing keys are unavailable"))?;
        if value
            .get("keys")
            .and_then(serde_json::Value::as_array)
            .is_none()
        {
            return Err(ApiError::service_unavailable(
                "Google signing keys are unavailable",
            ));
        }
        if let Ok(mut cache) = self.cache.lock() {
            *cache = Some(JwksCache {
                expires_at: SystemTime::now() + Duration::from_secs(15 * 60),
                value: value.clone(),
            });
        }
        Ok(value)
    }
}

fn decode_jwt_json<T: for<'de> Deserialize<'de>>(part: &str) -> Result<T, ApiError> {
    let bytes = URL_SAFE_NO_PAD
        .decode(part)
        .map_err(|_| ApiError::unauthorized("Google authentication could not be verified"))?;
    serde_json::from_slice(&bytes)
        .map_err(|_| ApiError::unauthorized("Google authentication could not be verified"))
}

fn jwk_for_kid(value: &serde_json::Value, kid: &str) -> Option<serde_json::Value> {
    value
        .get("keys")
        .and_then(serde_json::Value::as_array)
        .and_then(|keys| {
            keys.iter()
                .find(|key| key.get("kid").and_then(serde_json::Value::as_str) == Some(kid))
        })
        .cloned()
}

fn jwk_to_rsa_public_der(value: &serde_json::Value) -> Result<Vec<u8>, ApiError> {
    if value.get("kty").and_then(serde_json::Value::as_str) != Some("RSA") {
        return Err(ApiError::service_unavailable(
            "Google signing keys are unavailable",
        ));
    }
    let modulus = value
        .get("n")
        .and_then(serde_json::Value::as_str)
        .and_then(|value| URL_SAFE_NO_PAD.decode(value).ok())
        .ok_or_else(|| ApiError::service_unavailable("Google signing keys are unavailable"))?;
    let exponent = value
        .get("e")
        .and_then(serde_json::Value::as_str)
        .and_then(|value| URL_SAFE_NO_PAD.decode(value).ok())
        .ok_or_else(|| ApiError::service_unavailable("Google signing keys are unavailable"))?;
    Ok(der_sequence(
        &[der_integer(&modulus), der_integer(&exponent)].concat(),
    ))
}

fn der_integer(bytes: &[u8]) -> Vec<u8> {
    let first_nonzero = bytes
        .iter()
        .position(|byte| *byte != 0)
        .unwrap_or(bytes.len().saturating_sub(1));
    let value = &bytes[first_nonzero..];
    let mut body = Vec::with_capacity(value.len() + 1);
    if value.first().is_some_and(|byte| byte & 0x80 != 0) {
        body.push(0);
    }
    body.extend_from_slice(value);
    let mut output = vec![0x02];
    output.extend_from_slice(&der_length(body.len()));
    output.extend(body);
    output
}

fn der_sequence(body: &[u8]) -> Vec<u8> {
    let mut output = vec![0x30];
    output.extend_from_slice(&der_length(body.len()));
    output.extend_from_slice(body);
    output
}

fn der_length(length: usize) -> Vec<u8> {
    if length < 128 {
        vec![length as u8]
    } else {
        let bytes = (length as u64).to_be_bytes();
        let first_nonzero = bytes
            .iter()
            .position(|byte| *byte != 0)
            .unwrap_or(bytes.len() - 1);
        let value = &bytes[first_nonzero..];
        let mut output = vec![0x80 | value.len() as u8];
        output.extend_from_slice(value);
        output
    }
}

fn validate_google_jwks_url(value: &str, allow_local_test: bool) -> Result<(), String> {
    let url = reqwest::Url::parse(value).map_err(|_| "Google JWKS URL is invalid".to_string())?;
    let host = url.host_str().unwrap_or_default();
    let loopback = matches!(host, "localhost" | "127.0.0.1" | "::1");
    if value == DEFAULT_GOOGLE_JWKS_URL && url.scheme() == "https"
        || allow_local_test && loopback && matches!(url.scheme(), "http" | "https")
    {
        Ok(())
    } else {
        Err("Google JWKS URL must use HTTPS; only explicit loopback tests may use HTTP".into())
    }
}

fn validate_database_url(value: &str) -> Result<(), String> {
    if !(value.starts_with("postgres://") || value.starts_with("postgresql://")) {
        return Err(
            "RELINTOR_DATABASE_URL must use the PostgreSQL postgres:// or postgresql:// scheme"
                .into(),
        );
    }

    // This repaired Milestone 2 adapter intentionally uses NoTls for local
    // development verification only. A remote/hosted database must not silently
    // downgrade to plaintext; a TLS adapter is a later production dependency.
    if !(value.contains("localhost") || value.contains("127.0.0.1") || value.contains("[::1]")) {
        return Err(
            "Milestone 2 PostgreSQL adapter permits NoTls only for a local database; hosted PostgreSQL requires the later TLS production adapter"
                .into(),
        );
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct User {
    pub id: String,
    pub email: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Organization {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AccountView {
    pub user: User,
    pub organization: Organization,
    pub role: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionView {
    pub session_id: String,
    pub access_token: String,
    pub refresh_token: String,
    pub account: AccountView,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Device {
    pub id: String,
    pub user_id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntitlementPolicy {
    pub plan: PlanTier,
    pub source: EntitlementSource,
    pub capabilities: Vec<String>,
    pub limits: BTreeMap<String, u64>,
    pub expires_at: Option<i64>,
    pub grace_until: Option<i64>,
}

pub trait CloudRepository {
    fn readiness(&mut self) -> Result<(), ApiError>;
    fn dev_sign_in(&mut self, email: &str, device_name: &str) -> Result<SessionView, ApiError>;
    fn google_sign_in(
        &mut self,
        identity: &GoogleIdentity,
        device_name: &str,
    ) -> Result<SessionView, ApiError>;
    fn sign_out(&mut self, access_token: &str) -> Result<bool, ApiError>;
    fn refresh_session(&mut self, refresh_token: &str) -> Result<SessionView, ApiError>;
    fn account_for_access_token(&mut self, access_token: &str) -> Result<AccountView, ApiError>;
    fn register_device(&mut self, user_id: &str, name: &str) -> Result<Device, ApiError>;
    fn entitlement_policy(&mut self, account: &AccountView) -> Result<EntitlementPolicy, ApiError>;
}

pub struct PostgresStore {
    client: Client,
}

impl PostgresStore {
    pub fn connect(database_url: &str) -> Result<Self, String> {
        validate_database_url(database_url)?;
        let mut client = Client::connect(database_url, NoTls)
            .map_err(|error| format!("connect PostgreSQL: {error}"))?;
        apply_cloud_migrations(&mut client)?;
        Ok(Self { client })
    }

    fn account_for_ids(
        &mut self,
        user_id: Uuid,
        organization_id: Uuid,
    ) -> Result<AccountView, ApiError> {
        let row = self
            .client
            .query_opt(
                "SELECT u.email, o.name, m.role
                 FROM users u
                 JOIN organization_memberships m ON m.user_id = u.id
                 JOIN organizations o ON o.id = m.organization_id
                 WHERE u.id = $1 AND o.id = $2",
                &[&user_id, &organization_id],
            )
            .map_err(|_| ApiError::internal("query account"))?
            .ok_or_else(|| ApiError::unauthorized("account organization is unavailable"))?;

        Ok(AccountView {
            user: User {
                id: user_id.to_string(),
                email: row.get::<_, String>(0),
            },
            organization: Organization {
                id: organization_id.to_string(),
                name: row.get::<_, String>(1),
            },
            role: row.get::<_, String>(2),
        })
    }

    fn create_session(
        &mut self,
        user_id: Uuid,
        organization_id: Uuid,
        device_id: Uuid,
    ) -> Result<SessionView, ApiError> {
        let session_id = Uuid::new_v4();
        let access_token = random_token("rat");
        let refresh_token = random_token("rrt");
        let access_hash = hash_token(&access_token);
        let refresh_hash = hash_token(&refresh_token);

        self.client
            .execute(
                "INSERT INTO sessions (
                    id, user_id, organization_id, device_id,
                    access_token_hash, refresh_token_hash,
                    access_expires_at, expires_at
                 ) VALUES (
                    $1, $2, $3, $4, $5, $6,
                    CURRENT_TIMESTAMP + INTERVAL '15 minutes',
                    CURRENT_TIMESTAMP + INTERVAL '30 days'
                 )",
                &[
                    &session_id,
                    &user_id,
                    &organization_id,
                    &device_id,
                    &access_hash,
                    &refresh_hash,
                ],
            )
            .map_err(|_| ApiError::internal("create session"))?;

        Ok(SessionView {
            session_id: session_id.to_string(),
            access_token,
            refresh_token,
            account: self.account_for_ids(user_id, organization_id)?,
        })
    }

    fn active_complimentary_policy(
        &mut self,
        account: &AccountView,
    ) -> Result<Option<EntitlementPolicy>, ApiError> {
        let user_id = parse_uuid(&account.user.id)?;
        let organization_id = parse_uuid(&account.organization.id)?;
        let row = self
            .client
            .query_opt(
                "SELECT plan_template, seat_limit, ai_budget_override,
                        EXTRACT(EPOCH FROM expires_at)::BIGINT
                 FROM complimentary_grants
                 WHERE starts_at <= CURRENT_TIMESTAMP
                   AND (expires_at IS NULL OR expires_at >= CURRENT_TIMESTAMP)
                   AND (user_id = $1 OR organization_id = $2)
                 ORDER BY CASE WHEN organization_id = $2 THEN 0 ELSE 1 END, starts_at DESC
                 LIMIT 1",
                &[&user_id, &organization_id],
            )
            .map_err(|_| ApiError::internal("query complimentary entitlement"))?;

        let Some(row) = row else {
            return Ok(None);
        };

        let plan = parse_plan(&row.get::<_, String>(0))?;
        let mut limits = BTreeMap::new();
        if let Some(seat_limit) = row.get::<_, Option<i64>>(1) {
            limits.insert("team_members".into(), seat_limit.max(0) as u64);
        }
        if let Some(ai_budget) = row.get::<_, Option<i64>>(2) {
            limits.insert("monthly_ai_units".into(), ai_budget.max(0) as u64);
        }

        Ok(Some(EntitlementPolicy {
            plan,
            source: EntitlementSource::Complimentary,
            capabilities: capability_names(plan),
            limits,
            expires_at: row.get(3),
            grace_until: row
                .get::<_, Option<i64>>(3)
                .map(|value| value + DEFAULT_GRACE_SECONDS),
        }))
    }
}

impl CloudRepository for PostgresStore {
    fn readiness(&mut self) -> Result<(), ApiError> {
        self.client
            .is_valid(Duration::from_secs(2))
            .map_err(|_| ApiError::service_unavailable("PostgreSQL is not ready"))
    }

    fn dev_sign_in(&mut self, email: &str, device_name: &str) -> Result<SessionView, ApiError> {
        let normalized = normalize_email(email)?;
        validate_device_name(device_name)?;

        let mut transaction = self
            .client
            .transaction()
            .map_err(|_| ApiError::internal("begin sign-in transaction"))?;

        let user_id = if let Some(row) = transaction
            .query_opt(
                "SELECT id FROM users WHERE lower(email) = lower($1)",
                &[&normalized],
            )
            .map_err(|_| ApiError::internal("query development account"))?
        {
            row.get::<_, Uuid>(0)
        } else {
            let id = Uuid::new_v4();
            transaction
                .execute(
                    "INSERT INTO users(id, email) VALUES ($1, $2)",
                    &[&id, &normalized],
                )
                .map_err(|_| ApiError::internal("create development account"))?;
            id
        };

        let organization_id = if let Some(row) = transaction
            .query_opt(
                "SELECT organization_id
                 FROM organization_memberships
                 WHERE user_id = $1
                 ORDER BY created_at
                 LIMIT 1",
                &[&user_id],
            )
            .map_err(|_| ApiError::internal("query organization membership"))?
        {
            row.get::<_, Uuid>(0)
        } else {
            let id = Uuid::new_v4();
            let org_name = format!(
                "{}'s workspace",
                normalized.split('@').next().unwrap_or("Relintor")
            );
            transaction
                .execute(
                    "INSERT INTO organizations(id, name) VALUES ($1, $2)",
                    &[&id, &org_name],
                )
                .map_err(|_| ApiError::internal("create organization"))?;
            transaction
                .execute(
                    "INSERT INTO organization_memberships(organization_id, user_id, role)
                     VALUES ($1, $2, 'owner')",
                    &[&id, &user_id],
                )
                .map_err(|_| ApiError::internal("create owner membership"))?;
            id
        };

        let device_id = Uuid::new_v4();
        transaction
            .execute(
                "INSERT INTO devices(id, user_id, device_name, last_seen_at)
                 VALUES ($1, $2, $3, CURRENT_TIMESTAMP)",
                &[&device_id, &user_id, &device_name.trim()],
            )
            .map_err(|_| ApiError::internal("register device"))?;

        let has_entitlement = transaction
            .query_opt(
                "SELECT id FROM entitlements
                 WHERE user_id = $1
                   AND organization_id = $2
                   AND status IN ('active','trial','complimentary')
                   AND (expires_at IS NULL OR expires_at >= CURRENT_TIMESTAMP)
                 LIMIT 1",
                &[&user_id, &organization_id],
            )
            .map_err(|_| ApiError::internal("query entitlement"))?
            .is_some();

        if !has_entitlement {
            let entitlement_id = Uuid::new_v4();
            transaction
                .execute(
                    "INSERT INTO entitlements(
                        id, user_id, organization_id, plan_id, status, issued_at
                     ) VALUES ($1, $2, $3, 'individual', 'active', CURRENT_TIMESTAMP)",
                    &[&entitlement_id, &user_id, &organization_id],
                )
                .map_err(|_| ApiError::internal("create development entitlement"))?;
        }

        transaction
            .execute(
                "INSERT INTO audit_events(
                    id, organization_id, actor_user_id, event_type, request_id, metadata
                 ) VALUES ($1, $2, $3, 'dev_sign_in', $4, $5)",
                &[
                    &Uuid::new_v4(),
                    &organization_id,
                    &user_id,
                    &"dev-auth",
                    &serde_json::json!({"device_id": device_id}),
                ],
            )
            .map_err(|_| ApiError::internal("record sign-in audit event"))?;

        transaction
            .commit()
            .map_err(|_| ApiError::internal("commit sign-in transaction"))?;

        self.create_session(user_id, organization_id, device_id)
    }

    fn google_sign_in(
        &mut self,
        identity: &GoogleIdentity,
        device_name: &str,
    ) -> Result<SessionView, ApiError> {
        validate_device_name(device_name)?;
        if identity.email.is_some() && !identity.email_verified {
            return Err(ApiError::unauthorized(
                "Google account does not provide a verified email",
            ));
        }
        let email = identity.email.as_deref().ok_or_else(|| {
            ApiError::unauthorized("Google account does not provide a verified email")
        })?;
        let mut transaction = self
            .client
            .transaction()
            .map_err(|_| ApiError::internal("begin Google sign-in transaction"))?;
        let existing = transaction
            .query_opt(
                "SELECT user_id FROM external_identities
                 WHERE provider = 'google' AND provider_subject = $1",
                &[&identity.provider_subject],
            )
            .map_err(|_| ApiError::internal("query Google identity"))?;
        let user_id = if let Some(row) = existing {
            row.get::<_, Uuid>(0)
        } else if let Some(row) = transaction
            .query_opt(
                "SELECT id FROM users WHERE lower(email) = lower($1)",
                &[&email],
            )
            .map_err(|_| ApiError::internal("query Google account"))?
        {
            row.get::<_, Uuid>(0)
        } else {
            let id = Uuid::new_v4();
            transaction
                .execute(
                    "INSERT INTO users(id, email) VALUES ($1, $2)",
                    &[&id, &email],
                )
                .map_err(|_| ApiError::internal("create Google account"))?;
            id
        };
        let inserted = transaction
            .execute(
                "INSERT INTO external_identities(
                    id, provider, provider_subject, user_id, verified_email
                 ) VALUES ($1, 'google', $2, $3, $4)
                 ON CONFLICT (provider, provider_subject) DO NOTHING",
                &[
                    &Uuid::new_v4(),
                    &identity.provider_subject,
                    &user_id,
                    &email,
                ],
            )
            .map_err(|_| ApiError::internal("bind Google identity"))?;
        if inserted == 0 {
            let bound_user = transaction
                .query_one(
                    "SELECT user_id FROM external_identities
                     WHERE provider = 'google' AND provider_subject = $1",
                    &[&identity.provider_subject],
                )
                .map_err(|_| ApiError::internal("verify Google identity binding"))?
                .get::<_, Uuid>(0);
            if bound_user != user_id {
                return Err(ApiError::conflict("Google identity is already bound"));
            }
        }
        let organization_id = if let Some(row) = transaction
            .query_opt(
                "SELECT organization_id FROM organization_memberships
                 WHERE user_id = $1 ORDER BY created_at LIMIT 1",
                &[&user_id],
            )
            .map_err(|_| ApiError::internal("query Google organization"))?
        {
            row.get::<_, Uuid>(0)
        } else {
            let id = Uuid::new_v4();
            transaction
                .execute(
                    "INSERT INTO organizations(id, name) VALUES ($1, $2)",
                    &[
                        &id,
                        &format!(
                            "{}'s workspace",
                            email.split('@').next().unwrap_or("Relintor")
                        ),
                    ],
                )
                .map_err(|_| ApiError::internal("create Google organization"))?;
            transaction
                .execute(
                    "INSERT INTO organization_memberships(organization_id, user_id, role)
                     VALUES ($1, $2, 'owner')",
                    &[&id, &user_id],
                )
                .map_err(|_| ApiError::internal("create Google owner membership"))?;
            id
        };
        let device_id = Uuid::new_v4();
        transaction
            .execute(
                "INSERT INTO devices(id, user_id, device_name, last_seen_at)
                 VALUES ($1, $2, $3, CURRENT_TIMESTAMP)",
                &[&device_id, &user_id, &device_name.trim()],
            )
            .map_err(|_| ApiError::internal("register Google device"))?;
        transaction
            .execute(
                "INSERT INTO audit_events(
                    id, organization_id, actor_user_id, event_type, request_id, metadata
                 ) VALUES ($1, $2, $3, 'google_oidc_sign_in', $4, $5)",
                &[
                    &Uuid::new_v4(),
                    &organization_id,
                    &user_id,
                    &"google-oidc",
                    &serde_json::json!({"provider": "google", "subject_bound": true}),
                ],
            )
            .map_err(|_| ApiError::internal("record Google sign-in audit event"))?;
        transaction
            .commit()
            .map_err(|_| ApiError::internal("commit Google sign-in transaction"))?;
        self.create_session(user_id, organization_id, device_id)
    }

    fn sign_out(&mut self, access_token: &str) -> Result<bool, ApiError> {
        let access_hash = hash_token(access_token);
        let changed = self
            .client
            .execute(
                "UPDATE sessions
                 SET revoked_at = CURRENT_TIMESTAMP
                 WHERE access_token_hash = $1 AND revoked_at IS NULL",
                &[&access_hash],
            )
            .map_err(|_| ApiError::internal("revoke session"))?;
        Ok(changed > 0)
    }

    fn refresh_session(&mut self, refresh_token: &str) -> Result<SessionView, ApiError> {
        let refresh_hash = hash_token(refresh_token);
        let row = self
            .client
            .query_opt(
                "SELECT id, user_id, organization_id
                 FROM sessions
                 WHERE refresh_token_hash = $1
                   AND revoked_at IS NULL
                   AND expires_at > CURRENT_TIMESTAMP",
                &[&refresh_hash],
            )
            .map_err(|_| ApiError::internal("query refresh session"))?
            .ok_or_else(|| ApiError::unauthorized("refresh session is missing or expired"))?;

        let session_id = row.get::<_, Uuid>(0);
        let user_id = row.get::<_, Uuid>(1);
        let organization_id = row
            .get::<_, Option<Uuid>>(2)
            .ok_or_else(|| ApiError::unauthorized("session organization is missing"))?;
        let access_token = random_token("rat");
        let next_refresh = random_token("rrt");
        let access_hash = hash_token(&access_token);
        let next_refresh_hash = hash_token(&next_refresh);

        self.client
            .execute(
                "UPDATE sessions
                 SET access_token_hash = $1,
                     refresh_token_hash = $2,
                     access_expires_at = CURRENT_TIMESTAMP + INTERVAL '15 minutes',
                     expires_at = CURRENT_TIMESTAMP + INTERVAL '30 days'
                 WHERE id = $3",
                &[&access_hash, &next_refresh_hash, &session_id],
            )
            .map_err(|_| ApiError::internal("rotate refresh session"))?;

        Ok(SessionView {
            session_id: session_id.to_string(),
            access_token,
            refresh_token: next_refresh,
            account: self.account_for_ids(user_id, organization_id)?,
        })
    }

    fn account_for_access_token(&mut self, access_token: &str) -> Result<AccountView, ApiError> {
        let access_hash = hash_token(access_token);
        let row = self
            .client
            .query_opt(
                "SELECT user_id, organization_id
                 FROM sessions
                 WHERE access_token_hash = $1
                   AND revoked_at IS NULL
                   AND access_expires_at > CURRENT_TIMESTAMP",
                &[&access_hash],
            )
            .map_err(|_| ApiError::internal("query access session"))?
            .ok_or_else(|| ApiError::unauthorized("session is missing or expired"))?;

        let user_id = row.get::<_, Uuid>(0);
        let organization_id = row
            .get::<_, Option<Uuid>>(1)
            .ok_or_else(|| ApiError::unauthorized("session organization is missing"))?;
        self.account_for_ids(user_id, organization_id)
    }

    fn register_device(&mut self, user_id: &str, name: &str) -> Result<Device, ApiError> {
        validate_device_name(name)?;
        let user_uuid = parse_uuid(user_id)?;
        let id = Uuid::new_v4();
        self.client
            .execute(
                "INSERT INTO devices(id, user_id, device_name, last_seen_at)
                 VALUES ($1, $2, $3, CURRENT_TIMESTAMP)",
                &[&id, &user_uuid, &name.trim()],
            )
            .map_err(|_| ApiError::internal("register device"))?;
        Ok(Device {
            id: id.to_string(),
            user_id: user_id.into(),
            name: name.trim().into(),
        })
    }

    fn entitlement_policy(&mut self, account: &AccountView) -> Result<EntitlementPolicy, ApiError> {
        if let Some(policy) = self.p10_active_policy(account)? {
            return Ok(policy);
        }
        if let Some(policy) = self.active_complimentary_policy(account)? {
            return Ok(policy);
        }

        let user_id = parse_uuid(&account.user.id)?;
        let organization_id = parse_uuid(&account.organization.id)?;
        let row = self
            .client
            .query_opt(
                "SELECT e.plan_id
                 FROM entitlements e
                 WHERE e.user_id = $1
                   AND e.organization_id = $2
                   AND e.status IN ('active','trial','complimentary')
                   AND (e.expires_at IS NULL OR e.expires_at >= CURRENT_TIMESTAMP)
                 ORDER BY e.created_at DESC
                 LIMIT 1",
                &[&user_id, &organization_id],
            )
            .map_err(|_| ApiError::internal("query active entitlement"))?
            .ok_or_else(|| ApiError::forbidden("no active entitlement"))?;

        let plan = parse_plan(&row.get::<_, String>(0))?;
        let mut limits = BTreeMap::new();

        for row in self
            .client
            .query(
                "SELECT capability, limit_value
                 FROM entitlement_grants g
                 JOIN entitlements e ON e.id = g.entitlement_id
                 WHERE e.user_id = $1 AND e.organization_id = $2
                   AND e.status IN ('active','trial','complimentary')",
                &[&user_id, &organization_id],
            )
            .map_err(|_| ApiError::internal("query entitlement grants"))?
        {
            let capability = row.get::<_, String>(0);
            if let Some(limit) = row.get::<_, Option<i64>>(1) {
                limits.insert(capability, limit.max(0) as u64);
            }
        }

        Ok(EntitlementPolicy {
            plan,
            source: EntitlementSource::Plan,
            capabilities: capability_names(plan),
            limits,
            expires_at: None,
            grace_until: None,
        })
    }
}

/// Test-only memory adapter. Runtime startup never selects this repository.
#[derive(Debug, Default)]
pub struct MemoryStore {
    users: BTreeMap<String, User>,
    organizations: BTreeMap<String, Organization>,
    memberships: BTreeMap<(String, String), String>,
    access_sessions: BTreeMap<String, (String, String)>,
    refresh_sessions: BTreeMap<String, (String, String)>,
    devices: BTreeMap<String, Device>,
    external_identities: BTreeMap<(String, String), String>,
    p10: p10::MemoryP10State,
}

impl MemoryStore {
    fn account_for_ids(
        &self,
        user_id: &str,
        organization_id: &str,
    ) -> Result<AccountView, ApiError> {
        let user = self
            .users
            .get(user_id)
            .cloned()
            .ok_or_else(|| ApiError::internal("test account missing"))?;
        let organization = self
            .organizations
            .get(organization_id)
            .cloned()
            .ok_or_else(|| ApiError::internal("test organization missing"))?;
        let role = self
            .memberships
            .get(&(user_id.into(), organization_id.into()))
            .cloned()
            .ok_or_else(|| ApiError::internal("test membership missing"))?;
        Ok(AccountView {
            user,
            organization,
            role,
        })
    }
}

impl CloudRepository for MemoryStore {
    fn readiness(&mut self) -> Result<(), ApiError> {
        Ok(())
    }

    fn dev_sign_in(&mut self, email: &str, device_name: &str) -> Result<SessionView, ApiError> {
        let normalized = normalize_email(email)?;
        validate_device_name(device_name)?;
        let user_id = format!("user-{}", Uuid::new_v4());
        let org_id = format!("org-{}", Uuid::new_v4());
        self.users.insert(
            user_id.clone(),
            User {
                id: user_id.clone(),
                email: normalized.clone(),
            },
        );
        self.organizations.insert(
            org_id.clone(),
            Organization {
                id: org_id.clone(),
                name: format!(
                    "{}'s workspace",
                    normalized.split('@').next().unwrap_or("Relintor")
                ),
            },
        );
        self.memberships
            .insert((user_id.clone(), org_id.clone()), "owner".into());

        let _device = self.register_device(&user_id, device_name)?;
        let access = random_token("rat");
        let refresh = random_token("rrt");
        self.access_sessions
            .insert(access.clone(), (user_id.clone(), org_id.clone()));
        self.refresh_sessions
            .insert(refresh.clone(), (user_id.clone(), org_id.clone()));

        Ok(SessionView {
            session_id: Uuid::new_v4().to_string(),
            access_token: access,
            refresh_token: refresh,
            account: self.account_for_ids(&user_id, &org_id)?,
        })
    }

    fn google_sign_in(
        &mut self,
        identity: &GoogleIdentity,
        device_name: &str,
    ) -> Result<SessionView, ApiError> {
        validate_device_name(device_name)?;
        if identity.email.is_some() && !identity.email_verified {
            return Err(ApiError::unauthorized(
                "Google account does not provide a verified email",
            ));
        }
        let key = ("google".into(), identity.provider_subject.clone());
        let user_id = if let Some(user_id) = self.external_identities.get(&key) {
            user_id.clone()
        } else if let Some(email) = identity.email.as_deref() {
            self.users
                .values()
                .find(|user| user.email == email)
                .map(|user| user.id.clone())
                .unwrap_or_else(|| format!("user-{}", Uuid::new_v4()))
        } else {
            return Err(ApiError::unauthorized(
                "Google account does not provide a verified email",
            ));
        };
        if !self.users.contains_key(&user_id) {
            let email = identity.email.clone().ok_or_else(|| {
                ApiError::unauthorized("Google account does not provide a verified email")
            })?;
            self.users.insert(
                user_id.clone(),
                User {
                    id: user_id.clone(),
                    email: email.clone(),
                },
            );
            self.external_identities.insert(key, user_id.clone());
        } else {
            let bound = self
                .external_identities
                .entry(key)
                .or_insert(user_id.clone());
            if *bound != user_id {
                return Err(ApiError::conflict("Google identity is already bound"));
            }
        }
        let org_id = self
            .memberships
            .keys()
            .find(|(candidate, _)| candidate == &user_id)
            .map(|(_, org)| org.clone())
            .unwrap_or_else(|| {
                let org = format!("org-{}", Uuid::new_v4());
                let name = identity
                    .email
                    .as_deref()
                    .and_then(|email| email.split('@').next())
                    .unwrap_or("Relintor");
                self.organizations.insert(
                    org.clone(),
                    Organization {
                        id: org.clone(),
                        name: format!("{name}'s workspace"),
                    },
                );
                self.memberships
                    .insert((user_id.clone(), org.clone()), "owner".into());
                org
            });
        self.register_device(&user_id, device_name)?;
        let access = random_token("rat");
        let refresh = random_token("rrt");
        self.access_sessions
            .insert(access.clone(), (user_id.clone(), org_id.clone()));
        self.refresh_sessions
            .insert(refresh.clone(), (user_id.clone(), org_id.clone()));
        Ok(SessionView {
            session_id: Uuid::new_v4().to_string(),
            access_token: access,
            refresh_token: refresh,
            account: self.account_for_ids(&user_id, &org_id)?,
        })
    }

    fn sign_out(&mut self, access_token: &str) -> Result<bool, ApiError> {
        Ok(self.access_sessions.remove(access_token).is_some())
    }

    fn refresh_session(&mut self, refresh_token: &str) -> Result<SessionView, ApiError> {
        let (user_id, org_id) = self
            .refresh_sessions
            .remove(refresh_token)
            .ok_or_else(|| ApiError::unauthorized("refresh session is missing or expired"))?;
        let access = random_token("rat");
        let refresh = random_token("rrt");
        self.access_sessions
            .insert(access.clone(), (user_id.clone(), org_id.clone()));
        self.refresh_sessions
            .insert(refresh.clone(), (user_id.clone(), org_id.clone()));
        Ok(SessionView {
            session_id: Uuid::new_v4().to_string(),
            access_token: access,
            refresh_token: refresh,
            account: self.account_for_ids(&user_id, &org_id)?,
        })
    }

    fn account_for_access_token(&mut self, access_token: &str) -> Result<AccountView, ApiError> {
        let (user_id, org_id) = self
            .access_sessions
            .get(access_token)
            .cloned()
            .ok_or_else(|| ApiError::unauthorized("session is missing or expired"))?;
        self.account_for_ids(&user_id, &org_id)
    }

    fn register_device(&mut self, user_id: &str, name: &str) -> Result<Device, ApiError> {
        validate_device_name(name)?;
        let id = format!("device-{}", Uuid::new_v4());
        let device = Device {
            id: id.clone(),
            user_id: user_id.into(),
            name: name.trim().into(),
        };
        self.devices.insert(id, device.clone());
        Ok(device)
    }

    fn entitlement_policy(&mut self, account: &AccountView) -> Result<EntitlementPolicy, ApiError> {
        if let Some(policy) = self.p10_active_policy(account)? {
            return Ok(policy);
        }
        Ok(EntitlementPolicy {
            plan: PlanTier::Individual,
            source: EntitlementSource::Plan,
            capabilities: capability_names(PlanTier::Individual),
            limits: BTreeMap::new(),
            expires_at: None,
            grace_until: None,
        })
    }
}

pub fn apply_cloud_migrations(client: &mut Client) -> Result<(), String> {
    client
        .batch_execute(MIGRATION_002)
        .map_err(|error| format!("apply migration 002: {error}"))?;
    client
        .batch_execute(MIGRATION_003)
        .map_err(|error| format!("apply migration 003: {error}"))?;
    client
        .batch_execute(MIGRATION_009)
        .map_err(|error| format!("apply migration 009: {error}"))?;
    client
        .batch_execute(MIGRATION_010)
        .map_err(|error| format!("apply migration 010: {error}"))?;

    let rows = client
        .query(
            "SELECT version, name
             FROM cloud_schema_migrations
             WHERE version IN (2, 3, 9, 10)
             ORDER BY version",
            &[],
        )
        .map_err(|error| format!("verify cloud migrations: {error}"))?;

    let actual: Vec<(i64, String)> = rows
        .into_iter()
        .map(|row| (row.get::<_, i64>(0), row.get::<_, String>(1)))
        .collect();

    let expected = vec![
        (2, "cloud_account_foundation".to_string()),
        (3, "milestone2_reconciliation".to_string()),
        (9, "p10_teams_billing_admin".to_string()),
        (10, "google_external_identities".to_string()),
    ];

    if actual != expected {
        return Err(format!(
            "cloud migration identity mismatch: expected {expected:?}, found {actual:?}"
        ));
    }
    Ok(())
}

pub fn issue_entitlement(
    config: &Config,
    account: &AccountView,
    policy: EntitlementPolicy,
) -> SignedEntitlement {
    let signing_key = ed25519_dalek::SigningKey::from_bytes(&config.signing_key);
    let now = now_seconds();
    let expires_at = policy.expires_at.unwrap_or_else(|| {
        if policy.source == EntitlementSource::Founder {
            now.saturating_add(100 * 365 * 24 * 60 * 60)
        } else {
            now.saturating_add(24 * 60 * 60)
        }
    });
    let grace_until = policy
        .grace_until
        .unwrap_or_else(|| expires_at.saturating_add(config.grace_seconds));

    sign_entitlement(
        EntitlementClaims {
            subject: account.user.id.clone(),
            organization: Some(account.organization.id.clone()),
            plan: policy.plan,
            source: policy.source,
            capabilities: policy.capabilities,
            limits: policy.limits,
            issued_at: now,
            expires_at,
            grace_until,
            schema_version: ENTITLEMENT_SCHEMA_VERSION,
            issuer: config.issuer.clone(),
            id: Uuid::new_v4().to_string(),
            key_id: config.key_id.clone(),
        },
        &signing_key,
    )
}

fn now_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default()
}

fn random_token(prefix: &str) -> String {
    format!(
        "{prefix}_{}{}",
        Uuid::new_v4().simple(),
        Uuid::new_v4().simple()
    )
}

fn hash_token(token: &str) -> String {
    let mut digest = Sha256::new();
    digest.update(token.as_bytes());
    digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn parse_uuid(value: &str) -> Result<Uuid, ApiError> {
    Uuid::parse_str(value).map_err(|_| ApiError::unauthorized("account identifier is invalid"))
}

fn normalize_email(email: &str) -> Result<String, ApiError> {
    let normalized = email.trim().to_ascii_lowercase();
    if normalized.len() > 320
        || !normalized.contains('@')
        || normalized.starts_with('@')
        || normalized.ends_with('@')
    {
        return Err(ApiError::bad_request(
            "email must be a valid development identity",
        ));
    }
    Ok(normalized)
}

fn validate_device_name(name: &str) -> Result<(), ApiError> {
    if name.trim().is_empty() || name.chars().count() > 120 {
        Err(ApiError::bad_request(
            "device name is required and must be at most 120 characters",
        ))
    } else {
        Ok(())
    }
}

fn parse_plan(value: &str) -> Result<PlanTier, ApiError> {
    match value {
        "individual" => Ok(PlanTier::Individual),
        "pro" => Ok(PlanTier::Pro),
        "team" => Ok(PlanTier::Team),
        "enterprise" => Ok(PlanTier::Enterprise),
        _ => Err(ApiError::internal("unknown entitlement plan")),
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ApiError {
    pub code: String,
    pub message: String,
    #[serde(skip)]
    pub status: u16,
}

impl ApiError {
    fn bad_request(message: &str) -> Self {
        Self {
            code: "bad_request".into(),
            message: message.into(),
            status: 400,
        }
    }
    fn unauthorized(message: &str) -> Self {
        Self {
            code: "unauthorized".into(),
            message: message.into(),
            status: 401,
        }
    }
    fn forbidden(message: &str) -> Self {
        Self {
            code: "forbidden".into(),
            message: message.into(),
            status: 403,
        }
    }
    fn conflict(message: &str) -> Self {
        Self {
            code: "conflict".into(),
            message: message.into(),
            status: 409,
        }
    }
    fn not_found(message: &str) -> Self {
        Self {
            code: "not_found".into(),
            message: message.into(),
            status: 404,
        }
    }
    fn service_unavailable(message: &str) -> Self {
        Self {
            code: "service_unavailable".into(),
            message: message.into(),
            status: 503,
        }
    }
    fn internal(message: &str) -> Self {
        Self {
            code: "internal_error".into(),
            message: message.into(),
            status: 500,
        }
    }
}

#[derive(Debug, Deserialize)]
struct DevSignInRequest {
    email: String,
    device_name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GoogleExchangeRequest {
    id_token: String,
    device_name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DeviceRequest {
    name: String,
}

pub struct ApiState<R: CloudRepository> {
    pub config: Config,
    pub accounts: R,
    google_oidc: Option<GoogleOidcValidator>,
}

impl<R: CloudRepository> ApiState<R> {
    pub fn new(config: Config, accounts: R) -> Self {
        Self {
            config,
            accounts,
            google_oidc: None,
        }
    }
}

pub fn run_from_env() -> Result<(), String> {
    let config = Config::from_env()?;
    let google_oidc = match config.auth_adapter {
        AuthAdapter::Development => None,
        AuthAdapter::GoogleOidc => Some(GoogleOidcValidator::new(
            config.google_client_id.clone().ok_or_else(|| {
                "RELINTOR_GOOGLE_CLIENT_ID is required for google_oidc auth".to_string()
            })?,
            config.google_jwks_url.clone(),
        )?),
    };
    let store = PostgresStore::connect(&config.database_url)?;
    let listener =
        TcpListener::bind(&config.bind_addr).map_err(|error| format!("bind cloud API: {error}"))?;
    eprintln!("Relintor cloud API listening on {}", config.bind_addr);
    let mut state = ApiState::new(config, store);
    state.google_oidc = google_oidc;

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => handle_connection(stream, &mut state),
            Err(error) => eprintln!("cloud API accept error: {error}"),
        }
    }
    Ok(())
}

fn handle_connection<R: CloudRepository + p10::P10Repository>(
    mut stream: TcpStream,
    state: &mut ApiState<R>,
) {
    let request = match read_request(&mut stream) {
        Ok(request) => request,
        Err(error) => {
            let _ = write_response(
                &mut stream,
                &new_request_id(),
                &ApiError::bad_request(&error),
            );
            return;
        }
    };

    let id = sanitized_request_id(request.headers.get(REQUEST_ID_HEADER));
    let response = route(&request, state);
    let _ = write_json_response(&mut stream, &id, response);
}

fn route<R: CloudRepository + p10::P10Repository>(
    request: &HttpRequest,
    state: &mut ApiState<R>,
) -> Result<serde_json::Value, ApiError> {
    match (request.method.as_str(), request.path.as_str()) {
        ("GET", "/healthz") => {
            let status = p11::service_status(&mut state.accounts, &state.config);
            Ok(serde_json::json!({
                "status": if status.status == "healthy" { "ok" } else { "unavailable" },
                "service": status.service,
                "version": status.version,
                "detail": status.detail
            }))
        }
        ("GET", "/readyz") => {
            state.accounts.readiness()?;
            Ok(serde_json::json!({"status":"ready","database":"postgresql"}))
        }
        ("GET", "/v1/service/status") => {
            serde_json::to_value(p11::service_status(&mut state.accounts, &state.config))
                .map_err(|_| ApiError::internal("serialize service status"))
        }
        ("GET", "/v1/standards/distribution") => {
            serde_json::to_value(p11::standards_distribution_status(&state.config))
                .map_err(|_| ApiError::internal("serialize standards distribution status"))
        }
        ("POST", "/v1/auth/dev/sign-in") => {
            if !state.config.dev_auth_adapter {
                return Err(ApiError::not_found("development auth adapter is disabled"));
            }
            let body: DevSignInRequest = parse_json(request)?;
            let session = state.accounts.dev_sign_in(
                &body.email,
                body.device_name.as_deref().unwrap_or("Relintor desktop"),
            )?;
            serde_json::to_value(session).map_err(|_| ApiError::internal("serialize session"))
        }
        ("POST", "/v1/auth/google/exchange") => {
            if state.config.auth_adapter != AuthAdapter::GoogleOidc {
                return Err(ApiError::not_found("Google authentication is disabled"));
            }
            let validator = state.google_oidc.as_ref().ok_or_else(|| {
                ApiError::service_unavailable("Google authentication is unavailable")
            })?;
            let body: GoogleExchangeRequest = parse_json(request)?;
            let identity = validator.validate_id_token(&body.id_token)?;
            let session = state.accounts.google_sign_in(
                &identity,
                body.device_name.as_deref().unwrap_or("Relintor desktop"),
            )?;
            serde_json::to_value(session)
                .map_err(|_| ApiError::internal("serialize Google session"))
        }
        ("POST", "/v1/auth/sign-out") => {
            let access_token = bearer(request)?;
            Ok(serde_json::json!({
                "signed_out": state.accounts.sign_out(&access_token)?
            }))
        }
        ("POST", "/v1/auth/refresh") => {
            let refresh_token = bearer(request)?;
            let session = state.accounts.refresh_session(&refresh_token)?;
            serde_json::to_value(session)
                .map_err(|_| ApiError::internal("serialize refreshed session"))
        }
        ("GET", "/v1/account") => {
            let account = state.accounts.account_for_access_token(&bearer(request)?)?;
            serde_json::to_value(account).map_err(|_| ApiError::internal("serialize account"))
        }
        ("GET", "/v1/entitlements") => {
            let access_token = bearer(request)?;
            let account = state.accounts.account_for_access_token(&access_token)?;
            let policy = state.accounts.entitlement_policy(&account)?;
            let document = issue_entitlement(&state.config, &account, policy);
            serde_json::to_value(document).map_err(|_| ApiError::internal("serialize entitlement"))
        }
        ("POST", "/v1/devices/register") => {
            let access_token = bearer(request)?;
            let account = state.accounts.account_for_access_token(&access_token)?;
            let body: DeviceRequest = parse_json(request)?;
            let device = state
                .accounts
                .register_device(&account.user.id, &body.name)?;
            serde_json::to_value(device).map_err(|_| ApiError::internal("serialize device"))
        }
        (method, path) if p10::is_p10_path(path) => {
            let access_token = bearer(request)?;
            let account = state.accounts.account_for_access_token(&access_token)?;
            let _ = method;
            state.accounts.p10_route(
                &state.config,
                &account,
                &access_token,
                request,
                &sanitized_request_id(request.headers.get(REQUEST_ID_HEADER)),
            )
        }
        _ => Err(ApiError::not_found("route not found")),
    }
}

fn bearer(request: &HttpRequest) -> Result<String, ApiError> {
    let value = request
        .headers
        .get("authorization")
        .ok_or_else(|| ApiError::unauthorized("bearer session is required"))?;
    value
        .strip_prefix("Bearer ")
        .filter(|token| !token.trim().is_empty() && token.len() <= 512)
        .map(str::to_string)
        .ok_or_else(|| ApiError::unauthorized("bearer session is required"))
}

fn parse_json<T: for<'de> Deserialize<'de>>(request: &HttpRequest) -> Result<T, ApiError> {
    serde_json::from_slice(&request.body)
        .map_err(|_| ApiError::bad_request("request body is invalid JSON"))
}

#[derive(Debug)]
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

fn sanitized_request_id(header: Option<&String>) -> String {
    header
        .filter(|value| {
            !value.is_empty()
                && value.len() <= 80
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
        })
        .cloned()
        .unwrap_or_else(new_request_id)
}

fn new_request_id() -> String {
    format!("req_{}", Uuid::new_v4().simple())
}

fn write_json_response(
    stream: &mut TcpStream,
    request_id: &str,
    response: Result<serde_json::Value, ApiError>,
) -> std::io::Result<()> {
    match response {
        Ok(body) => write_raw_response(stream, 200, request_id, &body.to_string()),
        Err(error) => write_raw_response(
            stream,
            error.status,
            request_id,
            &serde_json::json!({"error": error}).to_string(),
        ),
    }
}

fn write_response(
    stream: &mut TcpStream,
    request_id: &str,
    error: &ApiError,
) -> std::io::Result<()> {
    write_raw_response(
        stream,
        error.status,
        request_id,
        &serde_json::json!({"error": error}).to_string(),
    )
}

fn write_raw_response(
    stream: &mut TcpStream,
    status: u16,
    request_id: &str,
    body: &str,
) -> std::io::Result<()> {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_rejects_non_postgres_and_remote_notls_database() {
        assert!(validate_database_url("sqlite://local").is_err());
        assert!(validate_database_url("postgresql://localhost/relintor").is_ok());
        assert!(validate_database_url("postgresql://db.example.com/relintor").is_err());
    }

    #[test]
    fn development_account_is_tenant_scoped() {
        let mut store = MemoryStore::default();
        let first = store.dev_sign_in("one@example.test", "first").unwrap();
        let second = store.dev_sign_in("two@example.test", "second").unwrap();
        assert_ne!(
            first.account.organization.id,
            second.account.organization.id
        );
        assert_eq!(
            store
                .account_for_access_token(&first.access_token)
                .unwrap()
                .user
                .email,
            "one@example.test"
        );
    }

    #[test]
    fn refresh_token_is_separate_and_rotated() {
        let mut store = MemoryStore::default();
        let first = store.dev_sign_in("refresh@example.test", "test").unwrap();
        let second = store.refresh_session(&first.refresh_token).unwrap();

        assert_ne!(first.access_token, first.refresh_token);
        assert_ne!(first.refresh_token, second.refresh_token);
        assert!(store.refresh_session(&first.refresh_token).is_err());
    }

    #[test]
    fn entitlement_grace_is_signed_into_claims() {
        let config = Config::test_config();
        let mut store = MemoryStore::default();
        let session = store.dev_sign_in("owner@example.test", "test").unwrap();
        let policy = store.entitlement_policy(&session.account).unwrap();
        let document = issue_entitlement(&config, &session.account, policy);

        assert_eq!(document.claims.subject, session.account.user.id);
        assert_eq!(
            document.claims.grace_until - document.claims.expires_at,
            config.grace_seconds
        );
        assert!(!document.signature.is_empty());
    }

    #[test]
    fn request_id_rejects_header_control_characters() {
        assert_ne!(
            sanitized_request_id(Some(&"ok\r\nInjected: yes".into())),
            "ok\r\nInjected: yes"
        );
        assert_eq!(
            sanitized_request_id(Some(&"valid-id_123".into())),
            "valid-id_123"
        );
    }

    #[test]
    fn migration_sources_are_additive_and_versioned() {
        for marker in [
            "CREATE TABLE IF NOT EXISTS users",
            "CREATE TABLE IF NOT EXISTS entitlements",
        ] {
            assert!(
                MIGRATION_002.contains(marker),
                "migration 002 missing {marker}"
            );
        }
        for marker in [
            "usage_buckets",
            "complimentary_grants",
            "admin_actions",
            "milestone2_reconciliation",
        ] {
            assert!(
                MIGRATION_003.contains(marker),
                "migration 003 missing {marker}"
            );
        }
        for marker in [
            "team_policies",
            "subscriptions",
            "admin_identities",
            "admin_mfa_challenges",
            "p10_admin_audit_events",
            "p10_teams_billing_admin",
        ] {
            assert!(
                MIGRATION_009.contains(marker),
                "migration 009 missing {marker}"
            );
        }
        assert!(MIGRATION_010.contains("external_identities"));
        assert!(MIGRATION_010.contains("google_external_identities"));
    }

    #[test]
    fn auth_adapter_selection_is_explicit_and_unknown_fails_closed() {
        assert_eq!(
            AuthAdapter::from_env("development"),
            Ok(AuthAdapter::Development)
        );
        assert_eq!(
            AuthAdapter::from_env("google_oidc"),
            Ok(AuthAdapter::GoogleOidc)
        );
        assert!(AuthAdapter::from_env("anything_else").is_err());
    }

    #[test]
    fn google_oidc_validates_signature_claims_and_caches_mocked_jwks() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut request = [0u8; 1024];
                let _ = stream.read(&mut request);
                let body = format!(
                    "{{\"keys\":[{{\"kty\":\"RSA\",\"kid\":\"test-key\",\"n\":\"{}\",\"e\":\"AQAB\"}}]}}",
                    TEST_RSA_N
                );
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{}",
                    body.len(), body
                );
                let _ = stream.write_all(response.as_bytes());
            }
        });
        let validator =
            GoogleOidcValidator::for_test("client-123", &format!("http://{address}/jwks")).unwrap();
        let identity = validator.validate_id_token(TEST_GOOGLE_TOKEN).unwrap();
        assert_eq!(identity.provider_subject, "google-sub-001");
        assert_eq!(identity.email.as_deref(), Some("friend@example.test"));
        assert!(identity.email_verified);
        assert!(validator
            .validate_id_token(TEST_WRONG_ISSUER_TOKEN)
            .is_err());
        assert!(validator
            .validate_id_token(TEST_WRONG_AUDIENCE_TOKEN)
            .is_err());
        assert!(validator.validate_id_token(TEST_EXPIRED_TOKEN).is_err());
        assert!(validator
            .validate_id_token(TEST_UNVERIFIED_EMAIL_TOKEN)
            .is_err());
    }

    #[test]
    fn google_subject_is_authority_and_email_cannot_rebind_identity() {
        let mut store = MemoryStore::default();
        let first = store
            .google_sign_in(
                &GoogleIdentity {
                    provider_subject: "stable-subject".into(),
                    email: Some("first@example.test".into()),
                    email_verified: true,
                },
                "Windows beta",
            )
            .unwrap();
        let same_subject = store
            .google_sign_in(
                &GoogleIdentity {
                    provider_subject: "stable-subject".into(),
                    email: Some("forged@example.test".into()),
                    email_verified: true,
                },
                "Windows beta",
            )
            .unwrap();
        assert_eq!(first.account.user.id, same_subject.account.user.id);
        assert_eq!(same_subject.account.user.email, "first@example.test");
        assert_ne!(
            first.account.user.id,
            store
                .google_sign_in(
                    &GoogleIdentity {
                        provider_subject: "another-subject".into(),
                        email: Some("forged@example.test".into()),
                        email_verified: true,
                    },
                    "Windows beta",
                )
                .unwrap()
                .account
                .user
                .id
        );
    }

    const TEST_RSA_N: &str = "urKiZB3JEkM91e6ob9Umyhzc8rN5G-XcNcyAuX2CKizcCqEDn_XKWmMCIy4EfzVp4WI12D_Kaju30Jk74q9EPtexAjGzCZwLBM0cernl1qHJ0Hly9E4zOBKMfbAKyEqjbd3laUojVpw7obv-dPup1Sslmw0vAqjFtlRZZO4XE2_wHRn1oW_WbC5450lnlZwLj2ZbP0RPBtUMwFxRxtm1cyxB93ntgeBaGzOqIwLohs6mUqAtyRDp3e3XLzdi7UgrX1NQHxxfSCil3IbS-DmWhxDmEBZ6riHTycvuF11XDG6qWzHiqbhgq7IJcOf4CyWDjb4xC_D1wa_ftoMECWp7vQ";
    const TEST_GOOGLE_TOKEN: &str = "eyJhbGciOiJSUzI1NiIsImtpZCI6InRlc3Qta2V5IiwidHlwIjoiSldUIn0.eyJpc3MiOiJodHRwczovL2FjY291bnRzLmdvb2dsZS5jb20iLCJhdWQiOiJjbGllbnQtMTIzIiwic3ViIjoiZ29vZ2xlLXN1Yi0wMDEiLCJlbWFpbCI6ImZyaWVuZEBleGFtcGxlLnRlc3QiLCJlbWFpbF92ZXJpZmllZCI6dHJ1ZSwiZXhwIjo0MTAyNDQ0ODAwfQ.rwaHbW-a8NaG6Ghi-yQWZJKlMaWY8P_mdPU8ntBqbNXpGpaLbm0pEginoVAhAJNbdGd-IMC7jll53eMZ0pcNPuHTLKDScv8XvJYUWS84SVCUZqDZUq-RhPQCOWPpPADsnUk5doYEP7kvMFBKGexfUgFoH3W3fZFDY2Q_Xdk7r72JxuCblORp7MaJA09sPrONu-I0FfBE497_FjBiN5ADGYBxyZzNDQPgSCD2QNu6C7jhymSEQxon02cQxxT_AgppTTpEIVRuAu-f315nVV3SwjdaH4_tUOi4nqY4DCPXylnDxBaLrhHxdGwfGtDpe8cMuhTDxu2W2B4_GBtntwq4Ew";
    const TEST_WRONG_ISSUER_TOKEN: &str = "eyJhbGciOiJSUzI1NiIsImtpZCI6InRlc3Qta2V5IiwidHlwIjoiSldUIn0.eyJpc3MiOiJodHRwczovL2V2aWwuZXhhbXBsZSIsImF1ZCI6ImNsaWVudC0xMjMiLCJzdWIiOiJnb29nbGUtc3ViLTAwMSIsImVtYWlsIjoiZnJpZW5kQGV4YW1wbGUudGVzdCIsImVtYWlsX3ZlcmlmaWVkIjp0cnVlLCJleHAiOjQxMDI0NDQ4MDB9.Ijz3gEL7Dyf3tPz1oF2_gm1LHW3L3qIjL-EM5t7zNklW6kSPt0sZV78xFWIwZHJNg0XNr6VhdFbFpbWoXTMkHxXe0C3q84hAcKZ8CDk1hCqSNuVD3oCjqCavkLowWhZH5jsGOobMe3XXGHL7TXeoaXMksw3lT8yIAHmJhBUZSv3L356Iaw9e8gIvVWqoHsKFVsK10N0qwDhkqAKBB1aO5N4LIdShVTrmz5FacgIh6Fb0mr6qgv3n2G9dMoa67szzpGwWLxW4xsRNWOffSeD0ouc2BkqNGW5R64USA1nbtLv-h3FWpgBDY_C3RVkBf-cJmgKJ9rJ_y3ZJWIhyB3oEvQ";
    const TEST_WRONG_AUDIENCE_TOKEN: &str = "eyJhbGciOiJSUzI1NiIsImtpZCI6InRlc3Qta2V5IiwidHlwIjoiSldUIn0.eyJpc3MiOiJodHRwczovL2FjY291bnRzLmdvb2dsZS5jb20iLCJhdWQiOiJvdGhlci1jbGllbnQiLCJzdWIiOiJnb29nbGUtc3ViLTAwMSIsImVtYWlsIjoiZnJpZW5kQGV4YW1wbGUudGVzdCIsImVtYWlsX3ZlcmlmaWVkIjp0cnVlLCJleHAiOjQxMDI0NDQ4MDB9.VETkz7Sv6eF9xIk9si7Org8c84lNQV32-YdKzOFbXOiBa1EGQGs6gkT9QgYDt-f7anC990Faww6BOIxwjH7T1NK_NnR7-83tagkJUkbi3A1jusohAAOU8N5JZH3A0P-JVRgBqJ7nJCAat7Wd8710lDQiGDxODnXL5-Z_FKzYiS7YVzZX_DfLidoz_TUAheRahEZoAXsvpQufVxpElAoxXigBUKQ0pQH26u9gAjcTmZP5OhpRs5XpVKYMeYhT4OoBHbjhuVP8SWqluMFeOpvTWICYQchbJEUv7erqRGJbGYyjMZUrL-RcWBrCcCYmYUukMLYuWj8KGqgJwrDHeFWwYQ";
    const TEST_EXPIRED_TOKEN: &str = "eyJhbGciOiJSUzI1NiIsImtpZCI6InRlc3Qta2V5IiwidHlwIjoiSldUIn0.eyJpc3MiOiJodHRwczovL2FjY291bnRzLmdvb2dsZS5jb20iLCJhdWQiOiJjbGllbnQtMTIzIiwic3ViIjoiZ29vZ2xlLXN1Yi0wMDEiLCJlbWFpbCI6ImZyaWVuZEBleGFtcGxlLnRlc3QiLCJlbWFpbF92ZXJpZmllZCI6dHJ1ZSwiZXhwIjoxfQ.nbHE11htU_daaIlmyryitbjQvNS0sb-6r5Tu2TtoMcuLPi2NKArt4wBSBILdycmK3Hl1tlzlG4bBVh5OmsPKBt9GzaAUokku_X-gxDtbcxx4kMWYSMMLMrwUT8eS8DcdbNr7lACD48eNCg9pjbO7_yomvCoqUiYw-HAi7ISTLO4k3VfEUtMaYlfeH4yIHcJ8yKGAK5--vSrNbV4ulZeiJPjLLwFuJyt9Es5VfjGUyGwMCZW5dPxwKvvDOBuGXmHgsdu7ow7ruWjZ7tCL7xM7eUnQjm-uhEfZEPHvuanpot1F4N9BLX9OAtVOCFDmDGSVv9pqipAKdRpT234uvetvlw";
    const TEST_UNVERIFIED_EMAIL_TOKEN: &str = "eyJhbGciOiJSUzI1NiIsImtpZCI6InRlc3Qta2V5IiwidHlwIjoiSldUIn0.eyJpc3MiOiJodHRwczovL2FjY291bnRzLmdvb2dsZS5jb20iLCJhdWQiOiJjbGllbnQtMTIzIiwic3ViIjoiZ29vZ2xlLXN1Yi0wMDEiLCJlbWFpbCI6ImZyaWVuZEBleGFtcGxlLnRlc3QiLCJlbWFpbF92ZXJpZmllZCI6ZmFsc2UsImV4cCI6NDEwMjQ0NDgwMH0.h-l4JLReSjo7bax4JWbxfgOVj5fyyiJJsGJ397TFa2A9tje5yBqP0jWmxpGZq3e5fBdVANmahfPuSUQBOR055I6gc-l6dMoImjfK5SI8nY65eNwbagbElQNXsTFABTg4crKE0oiK_A99GqI9qkwrHxK0XoHesXfoWiuth4it-13GH77UEkENutzhA7leC4h3et1Bzj5E9CEHwGiWvwUuHeVJ6wt5WLeJjJTYzXLJHgrSw3EBGNwHjEv5R9qo72zcv5sub6Upm0NkcGD1-14baXuvNkPTvAi_TaP3oz3wU6NJ8soFIc67US4c8aLzccQ_NtebQEVbptOxwj0DoU1SeQ";

    #[test]
    fn health_route_is_available_but_readiness_uses_repository() {
        let mut state = ApiState::new(Config::test_config(), MemoryStore::default());
        let request = HttpRequest {
            method: "GET".into(),
            path: "/healthz".into(),
            headers: BTreeMap::new(),
            body: Vec::new(),
        };
        assert_eq!(route(&request, &mut state).unwrap()["status"], "ok");

        let ready_request = HttpRequest {
            method: "GET".into(),
            path: "/readyz".into(),
            headers: BTreeMap::new(),
            body: Vec::new(),
        };
        assert_eq!(
            route(&ready_request, &mut state).unwrap()["status"],
            "ready"
        );
    }

    #[test]
    fn postgres_integration_runs_only_when_explicitly_requested() {
        let Ok(database_url) = env::var("RELINTOR_TEST_DATABASE_URL") else {
            // This unit test does not pretend a live database ran. The mandatory
            // verification command below is a separate ignored integration test.
            return;
        };
        let mut store = PostgresStore::connect(&database_url).expect("live PostgreSQL integration");
        store.readiness().expect("PostgreSQL readiness");
    }

    #[test]
    #[ignore = "requires RELINTOR_TEST_DATABASE_URL pointing to a disposable local PostgreSQL database"]
    fn mandatory_live_postgres_migration_and_account_round_trip() {
        let database_url =
            env::var("RELINTOR_TEST_DATABASE_URL").expect("RELINTOR_TEST_DATABASE_URL required");
        let mut store =
            PostgresStore::connect(&database_url).expect("connect and migrate PostgreSQL");
        store.readiness().expect("PostgreSQL ready");

        let session = store
            .dev_sign_in("integration@example.test", "integration-device")
            .expect("PostgreSQL dev sign-in");
        let account = store
            .account_for_access_token(&session.access_token)
            .expect("access session round trip");
        assert_eq!(account.user.email, "integration@example.test");

        let refreshed = store
            .refresh_session(&session.refresh_token)
            .expect("refresh round trip");
        assert_ne!(session.refresh_token, refreshed.refresh_token);

        let policy = store
            .entitlement_policy(&refreshed.account)
            .expect("policy");
        let document = issue_entitlement(&Config::test_config(), &refreshed.account, policy);
        assert_eq!(
            document.claims.organization.as_deref(),
            Some(refreshed.account.organization.id.as_str())
        );
    }
}
