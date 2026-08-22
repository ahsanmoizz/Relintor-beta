//! P10 server authority: organizations, team policy, entitlement grants,
//! founder administration, MFA, billing boundaries, and durable audit.
//!
//! This module deliberately keeps the renderer outside every authority
//! decision. PostgreSQL is the production store; the memory implementation is
//! compiled into deterministic tests only.

use super::{
    parse_plan, random_token, AccountView, ApiError, Config, EntitlementPolicy, EntitlementSource,
    HttpRequest, MemoryStore, PlanTier, PostgresStore, User,
};
use hmac::{Hmac, Mac};
use postgres::GenericClient;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha1::Sha1;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

const MFA_CHALLENGE_SECONDS: i64 = 120;
const MFA_PROOF_SECONDS: i64 = 300;
const TOTP_STEP_SECONDS: u64 = 30;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OrganizationRole {
    Owner,
    Admin,
    Member,
}

impl OrganizationRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Owner => "owner",
            Self::Admin => "admin",
            Self::Member => "member",
        }
    }

    fn parse(value: &str) -> Result<Self, ApiError> {
        match value {
            "owner" => Ok(Self::Owner),
            "admin" => Ok(Self::Admin),
            "member" => Ok(Self::Member),
            _ => Err(ApiError::bad_request("organization role is invalid")),
        }
    }

    fn can_manage_members(self) -> bool {
        matches!(self, Self::Owner | Self::Admin)
    }

    fn can_assign_admin(self) -> bool {
        matches!(self, Self::Owner)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TeamPolicyView {
    pub organization_id: String,
    pub version: u64,
    pub policy: Value,
    pub updated_by: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OrganizationMemberView {
    pub user: User,
    pub role: OrganizationRole,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SubscriptionView {
    pub id: String,
    pub organization_id: Option<String>,
    pub user_id: Option<String>,
    pub plan: PlanTier,
    pub billing_interval: String,
    pub state: String,
    pub seat_limit: Option<u64>,
    pub provider: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ComplimentaryGrantView {
    pub id: String,
    pub user_id: Option<String>,
    pub organization_id: Option<String>,
    pub plan: PlanTier,
    pub starts_at: String,
    pub expires_at: Option<String>,
    pub seat_limit: Option<u64>,
    pub ai_budget_override: Option<u64>,
    pub revoked: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AdminAuditEventView {
    pub id: String,
    pub actor_user_id: Option<String>,
    pub actor_role: String,
    pub organization_id: Option<String>,
    pub target_user_id: Option<String>,
    pub entity_type: String,
    pub entity_id: String,
    pub operation: String,
    pub reason: String,
    pub outcome: String,
    pub request_id: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MfaChallengeView {
    pub challenge_id: String,
    pub expires_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MfaProofView {
    pub proof: String,
    pub expires_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GrantInput {
    pub user_id: Option<String>,
    pub organization_id: Option<String>,
    pub plan: PlanTier,
    pub expires_in_seconds: Option<i64>,
    pub seat_limit: Option<u64>,
    pub ai_budget_override: Option<u64>,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PolicyUpdateRequest {
    expected_version: u64,
    policy: Value,
    reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct MemberRequest {
    user_id: String,
    role: OrganizationRole,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct MfaVerifyRequest {
    challenge_id: String,
    code: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GrantModifyRequest {
    plan: Option<PlanTier>,
    expires_in_seconds: Option<Option<i64>>,
    seat_limit: Option<Option<u64>>,
    ai_budget_override: Option<Option<u64>>,
    reason: String,
}

pub trait BillingAdapter {
    fn provider_name(&self) -> &'static str;
    fn status(&self) -> BillingStatus;
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BillingStatus {
    pub available: bool,
    pub provider: Option<String>,
    pub detail: String,
}

pub struct UnavailableBillingAdapter;

impl BillingAdapter for UnavailableBillingAdapter {
    fn provider_name(&self) -> &'static str {
        "unconfigured"
    }

    fn status(&self) -> BillingStatus {
        BillingStatus {
            available: false,
            provider: None,
            detail: "No production billing provider is configured; paid billing is fail-closed."
                .into(),
        }
    }
}

pub(crate) trait P10Repository {
    fn p10_route(
        &mut self,
        config: &Config,
        account: &AccountView,
        access_token: &str,
        request: &HttpRequest,
        request_id: &str,
    ) -> Result<Value, ApiError>;
}

pub(crate) fn is_p10_path(path: &str) -> bool {
    path == "/v1/billing/status"
        || path.starts_with("/v1/organizations")
        || path.starts_with("/v1/admin/")
}

fn now_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default()
}

fn hash_value(value: &str) -> String {
    let mut digest = Sha256::new();
    digest.update(value.as_bytes());
    digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn role_for_account(account: &AccountView) -> Result<OrganizationRole, ApiError> {
    OrganizationRole::parse(&account.role)
}

fn require_reason(reason: &str) -> Result<(), ApiError> {
    let trimmed = reason.trim();
    if trimmed.is_empty() || trimmed.len() > 500 {
        return Err(ApiError::bad_request(
            "a non-empty mutation reason of at most 500 bytes is required",
        ));
    }
    Ok(())
}

fn validate_policy(policy: &Value) -> Result<(), ApiError> {
    fn visit(value: &Value) -> bool {
        match value {
            Value::Object(map) => map.iter().any(|(key, value)| {
                let lower = key.to_ascii_lowercase();
                [
                    "p6",
                    "p7",
                    "p8",
                    "p9",
                    "sealed",
                    "completion",
                    "verified_complete",
                    "verification_authority",
                    "execution_authority",
                    "recovery_authority",
                ]
                .iter()
                .any(|marker| lower.contains(marker))
                    || visit(value)
            }),
            Value::Array(items) => items.iter().any(visit),
            _ => false,
        }
    }
    if visit(policy) {
        return Err(ApiError::forbidden(
            "team policy cannot override sealed execution, verification, or recovery authority",
        ));
    }
    Ok(())
}

fn to_u64(value: Option<i64>) -> Option<u64> {
    value.and_then(|value| u64::try_from(value).ok())
}

fn mfa_code(secret: &[u8], timestamp: u64) -> String {
    let counter = timestamp / TOTP_STEP_SECONDS;
    let mut message = [0u8; 8];
    message.copy_from_slice(&counter.to_be_bytes());
    let mut mac = Hmac::<Sha1>::new_from_slice(secret).expect("HMAC accepts every secret length");
    mac.update(&message);
    let digest = mac.finalize().into_bytes();
    let offset = usize::from(digest[19] & 0x0f);
    let binary = (u32::from(digest[offset]) & 0x7f) << 24
        | u32::from(digest[offset + 1]) << 16
        | u32::from(digest[offset + 2]) << 8
        | u32::from(digest[offset + 3]);
    format!("{:06}", binary % 1_000_000)
}

fn valid_mfa_code(secret: &[u8], code: &str, timestamp: i64) -> bool {
    let Ok(timestamp) = u64::try_from(timestamp) else {
        return false;
    };
    let Ok(code) = code.parse::<u32>() else {
        return false;
    };
    code < 1_000_000
        && [
            timestamp.saturating_sub(TOTP_STEP_SECONDS),
            timestamp,
            timestamp.saturating_add(TOTP_STEP_SECONDS),
        ]
        .into_iter()
        .map(|value| mfa_code(secret, value))
        .any(|expected| expected == format!("{code:06}"))
}

fn parse_uuid_for_api(value: &str, message: &str) -> Result<Uuid, ApiError> {
    Uuid::parse_str(value).map_err(|_| ApiError::bad_request(message))
}

fn read_json<T: for<'de> Deserialize<'de>>(request: &HttpRequest) -> Result<T, ApiError> {
    serde_json::from_slice(&request.body)
        .map_err(|_| ApiError::bad_request("request body is invalid JSON"))
}

fn proof_header(request: &HttpRequest) -> Result<&str, ApiError> {
    request
        .headers
        .get("x-admin-mfa-proof")
        .map(String::as_str)
        .filter(|value| value.len() >= 32 && value.len() <= 512)
        .ok_or_else(|| ApiError::unauthorized("fresh admin MFA proof is required"))
}

fn json_plan(value: &str) -> Result<PlanTier, ApiError> {
    parse_plan(value)
}

fn plan_name(plan: PlanTier) -> &'static str {
    match plan {
        PlanTier::Individual => "individual",
        PlanTier::Pro => "pro",
        PlanTier::Team => "team",
        PlanTier::Enterprise => "enterprise",
    }
}

impl PostgresStore {
    fn admin_authority(
        &mut self,
        actor: &AccountView,
    ) -> Result<Option<(bool, bool, Vec<u8>)>, ApiError> {
        let user_id = super::parse_uuid(&actor.user.id)?;
        let row = self
            .client
            .query_opt(
                "SELECT is_founder, is_admin, mfa_secret FROM admin_identities
                 WHERE user_id = $1 AND (is_founder OR is_admin)",
                &[&user_id],
            )
            .map_err(|_| ApiError::internal("query admin authority"))?;
        Ok(row.map(|row| {
            (
                row.get::<_, bool>(0),
                row.get::<_, bool>(1),
                row.get::<_, Vec<u8>>(2),
            )
        }))
    }

    fn require_admin(&mut self, actor: &AccountView) -> Result<(bool, bool), ApiError> {
        let Some((founder, admin, _)) = self.admin_authority(actor)? else {
            return Err(ApiError::forbidden("admin authority is required"));
        };
        Ok((founder, admin))
    }

    fn consume_mfa_proof(
        &mut self,
        actor: &AccountView,
        access_token: &str,
        proof: &str,
    ) -> Result<(), ApiError> {
        self.require_admin(actor)?;
        let user_id = super::parse_uuid(&actor.user.id)?;
        let session_hash = super::hash_token(access_token);
        let proof_hash = hash_value(proof);
        let changed = self
            .client
            .execute(
                "UPDATE admin_mfa_proofs
                 SET used_at = CURRENT_TIMESTAMP
                 WHERE user_id = $1 AND session_hash = $2 AND proof_hash = $3
                   AND used_at IS NULL AND expires_at > CURRENT_TIMESTAMP",
                &[&user_id, &session_hash, &proof_hash],
            )
            .map_err(|_| ApiError::internal("consume MFA proof"))?;
        if changed != 1 {
            return Err(ApiError::unauthorized(
                "MFA proof is invalid, expired, or replayed",
            ));
        }
        Ok(())
    }

    pub fn begin_admin_mfa(
        &mut self,
        actor: &AccountView,
        access_token: &str,
    ) -> Result<MfaChallengeView, ApiError> {
        let Some((_, _, _)) = self.admin_authority(actor)? else {
            return Err(ApiError::forbidden("admin MFA is available only to admins"));
        };
        let user_id = super::parse_uuid(&actor.user.id)?;
        let id = Uuid::new_v4();
        let nonce = random_token("mfa");
        self.client
            .execute(
                "INSERT INTO admin_mfa_challenges(id, user_id, session_hash, nonce_hash, expires_at)
                 VALUES ($1, $2, $3, $4, CURRENT_TIMESTAMP + INTERVAL '120 seconds')",
                &[&id, &user_id, &super::hash_token(access_token), &hash_value(&nonce)],
            )
            .map_err(|_| ApiError::internal("create admin MFA challenge"))?;
        Ok(MfaChallengeView {
            challenge_id: id.to_string(),
            expires_at: now_seconds() + MFA_CHALLENGE_SECONDS,
        })
    }

    pub fn verify_admin_mfa(
        &mut self,
        actor: &AccountView,
        access_token: &str,
        challenge_id: &str,
        code: &str,
    ) -> Result<MfaProofView, ApiError> {
        let Some((_, _, secret)) = self.admin_authority(actor)? else {
            return Err(ApiError::forbidden("admin MFA is available only to admins"));
        };
        if code.len() != 6 || !code.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(ApiError::unauthorized("MFA code is invalid"));
        }
        let user_id = super::parse_uuid(&actor.user.id)?;
        let challenge_id = parse_uuid_for_api(challenge_id, "MFA challenge identifier is invalid")?;
        let session_hash = super::hash_token(access_token);
        let row = self
            .client
            .query_opt(
                "SELECT id, nonce_hash
                 FROM admin_mfa_challenges
                 WHERE id = $1 AND user_id = $2 AND session_hash = $3
                   AND used_at IS NULL AND expires_at > CURRENT_TIMESTAMP",
                &[&challenge_id, &user_id, &session_hash],
            )
            .map_err(|_| ApiError::internal("query admin MFA challenge"))?
            .ok_or_else(|| ApiError::unauthorized("MFA challenge is invalid"))?;
        let _nonce_hash = row.get::<_, String>(1);
        if !valid_mfa_code(&secret, code, now_seconds()) {
            return Err(ApiError::unauthorized("MFA code is invalid"));
        }
        let proof = random_token("mfa-proof");
        let proof_id = Uuid::new_v4();
        let changed = self
            .client
            .execute(
                "UPDATE admin_mfa_challenges SET used_at = CURRENT_TIMESTAMP
                 WHERE id = $1 AND used_at IS NULL",
                &[&challenge_id],
            )
            .map_err(|_| ApiError::internal("consume admin MFA challenge"))?;
        if changed != 1 {
            return Err(ApiError::unauthorized(
                "MFA challenge is expired or replayed",
            ));
        }
        self.client
            .execute(
                "INSERT INTO admin_mfa_proofs(id, user_id, session_hash, proof_hash, expires_at)
                 VALUES ($1, $2, $3, $4, CURRENT_TIMESTAMP + INTERVAL '300 seconds')",
                &[&proof_id, &user_id, &session_hash, &hash_value(&proof)],
            )
            .map_err(|_| ApiError::internal("store admin MFA proof"))?;
        Ok(MfaProofView {
            proof,
            expires_at: now_seconds() + MFA_PROOF_SECONDS,
        })
    }

    fn actor_org_role(&mut self, actor: &AccountView) -> Result<OrganizationRole, ApiError> {
        role_for_account(actor)
    }

    #[allow(clippy::too_many_arguments)]
    fn audit<C: GenericClient>(
        client: &mut C,
        actor: &AccountView,
        actor_role: &str,
        organization_id: Option<Uuid>,
        target_user_id: Option<Uuid>,
        entity_type: &str,
        entity_id: &str,
        operation: &str,
        reason: &str,
        before_state: Option<Value>,
        after_state: Option<Value>,
        request_id: &str,
    ) -> Result<(), ApiError> {
        let actor_id = super::parse_uuid(&actor.user.id)?;
        client
            .execute(
                "INSERT INTO p10_admin_audit_events(
                    id, actor_user_id, actor_role, organization_id, target_user_id,
                    entity_type, entity_id, operation, reason, before_state,
                    after_state, outcome, request_id
                 ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,'success',$12)",
                &[
                    &Uuid::new_v4(),
                    &actor_id,
                    &actor_role,
                    &organization_id,
                    &target_user_id,
                    &entity_type,
                    &entity_id,
                    &operation,
                    &reason,
                    &before_state,
                    &after_state,
                    &request_id,
                ],
            )
            .map_err(|_| ApiError::internal("mandatory admin audit persistence failed"))?;
        Ok(())
    }

    pub fn team_policy(&mut self, actor: &AccountView) -> Result<TeamPolicyView, ApiError> {
        let organization_id = super::parse_uuid(&actor.organization.id)?;
        let row = self
            .client
            .query_opt(
                "SELECT version, policy, updated_by FROM team_policies WHERE organization_id = $1",
                &[&organization_id],
            )
            .map_err(|_| ApiError::internal("query team policy"))?;
        match row {
            Some(row) => Ok(TeamPolicyView {
                organization_id: actor.organization.id.clone(),
                version: row.get::<_, i64>(0).max(0) as u64,
                policy: row.get(1),
                updated_by: row.get::<_, Uuid>(2).to_string(),
            }),
            None => Ok(TeamPolicyView {
                organization_id: actor.organization.id.clone(),
                version: 0,
                policy: json!({}),
                updated_by: actor.user.id.clone(),
            }),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn update_team_policy(
        &mut self,
        actor: &AccountView,
        access_token: &str,
        proof: &str,
        expected_version: u64,
        policy: Value,
        reason: &str,
        request_id: &str,
    ) -> Result<TeamPolicyView, ApiError> {
        require_reason(reason)?;
        validate_policy(&policy)?;
        self.consume_mfa_proof(actor, access_token, proof)?;
        let role = self.actor_org_role(actor)?;
        if !role.can_manage_members() {
            return Err(ApiError::forbidden("organization admin role is required"));
        }
        let organization_id = super::parse_uuid(&actor.organization.id)?;
        let actor_id = super::parse_uuid(&actor.user.id)?;
        let mut tx = self
            .client
            .transaction()
            .map_err(|_| ApiError::internal("begin team policy transaction"))?;
        let current = tx
            .query_opt(
                "SELECT version, policy FROM team_policies WHERE organization_id = $1 FOR UPDATE",
                &[&organization_id],
            )
            .map_err(|_| ApiError::internal("lock team policy"))?;
        let current_version = current
            .as_ref()
            .map(|row| row.get::<_, i64>(0).max(0) as u64)
            .unwrap_or(0);
        if current_version != expected_version {
            return Err(ApiError::conflict("team policy version is stale"));
        }
        let next_version = current_version + 1;
        if current.is_some() {
            tx.execute(
                "UPDATE team_policies SET version=$1, policy=$2, updated_by=$3, updated_at=CURRENT_TIMESTAMP
                 WHERE organization_id=$4",
                &[&next_version.saturating_cast_i64(), &policy, &actor_id, &organization_id],
            )
            .map_err(|_| ApiError::internal("update team policy"))?;
        } else {
            tx.execute(
                "INSERT INTO team_policies(organization_id,version,policy,updated_by)
                 VALUES($1,$2,$3,$4)",
                &[
                    &organization_id,
                    &next_version.saturating_cast_i64(),
                    &policy,
                    &actor_id,
                ],
            )
            .map_err(|_| ApiError::internal("create team policy"))?;
        }
        Self::audit(
            &mut tx,
            actor,
            role.as_str(),
            Some(organization_id),
            None,
            "team_policy",
            &organization_id.to_string(),
            "update",
            reason,
            current.map(|row| row.get(1)),
            Some(policy.clone()),
            request_id,
        )?;
        tx.commit()
            .map_err(|_| ApiError::internal("commit team policy"))?;
        Ok(TeamPolicyView {
            organization_id: organization_id.to_string(),
            version: next_version,
            policy,
            updated_by: actor_id.to_string(),
        })
    }

    pub(crate) fn p10_active_policy(
        &mut self,
        account: &AccountView,
    ) -> Result<Option<EntitlementPolicy>, ApiError> {
        let user_id = super::parse_uuid(&account.user.id)?;
        let organization_id = super::parse_uuid(&account.organization.id)?;

        if let Some(row) = self
            .client
            .query_opt(
                "SELECT 1 FROM admin_identities
                 WHERE user_id = $1 AND is_founder AND (is_founder OR is_admin)",
                &[&user_id],
            )
            .map_err(|_| ApiError::internal("query founder entitlement"))?
        {
            let _ = row;
            let mut limits = BTreeMap::new();
            limits.insert("team_members".into(), u64::MAX);
            limits.insert("monthly_ai_units".into(), u64::MAX);
            return Ok(Some(EntitlementPolicy {
                plan: PlanTier::Enterprise,
                source: EntitlementSource::Founder,
                capabilities: super::capability_names(PlanTier::Enterprise),
                limits,
                expires_at: None,
                grace_until: None,
            }));
        }

        if let Some(row) = self
            .client
            .query_opt(
                "SELECT plan_template, seat_limit, ai_budget_override,
                        EXTRACT(EPOCH FROM expires_at)::BIGINT
                 FROM complimentary_grants
                 WHERE starts_at <= CURRENT_TIMESTAMP
                   AND revoked_at IS NULL
                   AND (expires_at IS NULL OR expires_at >= CURRENT_TIMESTAMP)
                   AND (user_id = $1 OR organization_id = $2)
                 ORDER BY CASE WHEN organization_id = $2 THEN 0 ELSE 1 END, starts_at DESC
                 LIMIT 1",
                &[&user_id, &organization_id],
            )
            .map_err(|_| ApiError::internal("query active complimentary grant"))?
        {
            let plan = parse_plan(&row.get::<_, String>(0))?;
            let mut limits = BTreeMap::new();
            if let Some(seat_limit) = row.get::<_, Option<i64>>(1) {
                limits.insert("team_members".into(), seat_limit.max(0) as u64);
            }
            if let Some(ai_budget) = row.get::<_, Option<i64>>(2) {
                limits.insert("monthly_ai_units".into(), ai_budget.max(0) as u64);
            }
            let expires_at: Option<i64> = row.get(3);
            return Ok(Some(EntitlementPolicy {
                plan,
                source: EntitlementSource::Complimentary,
                capabilities: super::capability_names(plan),
                limits,
                expires_at,
                grace_until: expires_at.map(|value| value + super::DEFAULT_GRACE_SECONDS),
            }));
        }

        if let Some(row) = self
            .client
            .query_opt(
                "SELECT plan_id, status,
                        EXTRACT(EPOCH FROM COALESCE(trial_ends_at, current_period_end))::BIGINT,
                        seat_limit
                 FROM subscriptions
                 WHERE (user_id = $1 OR organization_id = $2)
                   AND ((status = 'active' AND (current_period_end IS NULL OR current_period_end >= CURRENT_TIMESTAMP))
                     OR (status = 'trial' AND trial_ends_at >= CURRENT_TIMESTAMP))
                 ORDER BY starts_at DESC LIMIT 1",
                &[&user_id, &organization_id],
            )
            .map_err(|_| ApiError::internal("query active subscription"))?
        {
            let plan = parse_plan(&row.get::<_, String>(0))?;
            let source = if row.get::<_, String>(1) == "trial" {
                EntitlementSource::Trial
            } else {
                EntitlementSource::Plan
            };
            let mut limits = BTreeMap::new();
            if let Some(seat_limit) = row.get::<_, Option<i64>>(3) {
                limits.insert("team_members".into(), seat_limit.max(0) as u64);
            }
            let expires_at: Option<i64> = row.get(2);
            return Ok(Some(EntitlementPolicy {
                plan,
                source,
                capabilities: super::capability_names(plan),
                limits,
                expires_at,
                grace_until: expires_at.map(|value| value + super::DEFAULT_GRACE_SECONDS),
            }));
        }

        if self
            .client
            .query_opt(
                "SELECT 1 FROM subscriptions
                 WHERE (user_id = $1 OR organization_id = $2)
                   AND ((status = 'trial' AND trial_ends_at < CURRENT_TIMESTAMP)
                     OR (status = 'active' AND current_period_end IS NOT NULL AND current_period_end < CURRENT_TIMESTAMP))
                 ORDER BY starts_at DESC LIMIT 1",
                &[&user_id, &organization_id],
            )
            .map_err(|_| ApiError::internal("query expired subscription"))?
            .is_some()
        {
            return Err(ApiError::forbidden("subscription or trial has expired"));
        }

        Ok(None)
    }

    fn organization_for_user(&mut self, user_id: Uuid) -> Result<Uuid, ApiError> {
        self.client
            .query_opt(
                "SELECT organization_id FROM organization_memberships
                 WHERE user_id = $1 ORDER BY created_at LIMIT 1",
                &[&user_id],
            )
            .map_err(|_| ApiError::internal("query target organization"))?
            .map(|row| row.get(0))
            .ok_or_else(|| ApiError::not_found("target user has no organization"))
    }

    fn require_target_scope(
        &mut self,
        actor: &AccountView,
        target_user: Option<Uuid>,
        target_org: Option<Uuid>,
        founder: bool,
    ) -> Result<(), ApiError> {
        if founder {
            return Ok(());
        }
        let actor_org = super::parse_uuid(&actor.organization.id)?;
        let role = self.actor_org_role(actor)?;
        if !role.can_manage_members() {
            return Err(ApiError::forbidden("organization admin role is required"));
        }
        let scope = match (target_user, target_org) {
            (_, Some(org)) => org,
            (Some(user), None) => self.organization_for_user(user)?,
            (None, None) => return Err(ApiError::bad_request("grant target is required")),
        };
        if scope != actor_org {
            return Err(ApiError::forbidden("cross-tenant administration is denied"));
        }
        Ok(())
    }

    fn insert_grant(
        &mut self,
        actor: &AccountView,
        access_token: &str,
        proof: &str,
        input: GrantInput,
        user_target: bool,
        request_id: &str,
    ) -> Result<ComplimentaryGrantView, ApiError> {
        require_reason(&input.reason)?;
        if let Some(seconds) = input.expires_in_seconds {
            if seconds <= 0 {
                return Err(ApiError::bad_request("grant expiry must be positive"));
            }
        }
        if user_target == input.user_id.is_none()
            || (!user_target && input.organization_id.is_none())
        {
            return Err(ApiError::bad_request("grant target shape is invalid"));
        }
        let (founder, _) = self.require_admin(actor)?;
        self.require_target_scope(
            actor,
            input
                .user_id
                .as_deref()
                .map(super::parse_uuid)
                .transpose()?,
            input
                .organization_id
                .as_deref()
                .map(super::parse_uuid)
                .transpose()?,
            founder,
        )?;
        self.consume_mfa_proof(actor, access_token, proof)?;

        let user_id = input
            .user_id
            .as_deref()
            .map(super::parse_uuid)
            .transpose()?;
        let organization_id = input
            .organization_id
            .as_deref()
            .map(super::parse_uuid)
            .transpose()?;
        if let Some(user_id) = user_id {
            let exists = self
                .client
                .query_opt("SELECT 1 FROM users WHERE id=$1", &[&user_id])
                .map_err(|_| ApiError::internal("query target user"))?
                .is_some();
            if !exists {
                return Err(ApiError::not_found("target user does not exist"));
            }
        }
        if let Some(organization_id) = organization_id {
            let exists = self
                .client
                .query_opt(
                    "SELECT 1 FROM organizations WHERE id=$1",
                    &[&organization_id],
                )
                .map_err(|_| ApiError::internal("query target organization"))?
                .is_some();
            if !exists {
                return Err(ApiError::not_found("target organization does not exist"));
            }
        }

        let audit_organization_id =
            organization_id.or_else(|| user_id.and_then(|id| self.organization_for_user(id).ok()));
        let id = Uuid::new_v4();
        let plan = plan_name(input.plan);
        let expires = input.expires_in_seconds;
        let mut tx = self
            .client
            .transaction()
            .map_err(|_| ApiError::internal("begin grant transaction"))?;
        tx.execute(
            "INSERT INTO complimentary_grants(
                id,user_id,organization_id,reason,granted_by,starts_at,expires_at,
                plan_template,seat_limit,ai_budget_override,notes
             ) VALUES ($1,$2,$3,$4,$5,CURRENT_TIMESTAMP,
                CASE WHEN $6::BIGINT IS NULL THEN NULL
                     ELSE CURRENT_TIMESTAMP + ($6::BIGINT * INTERVAL '1 second') END,
                $7,$8,$9,$10)",
            &[
                &id,
                &user_id,
                &organization_id,
                &input.reason,
                &super::parse_uuid(&actor.user.id)?,
                &expires,
                &plan,
                &input.seat_limit.map(|value| value.saturating_cast_i64()),
                &input
                    .ai_budget_override
                    .map(|value| value.saturating_cast_i64()),
                &Some(input.reason.clone()),
            ],
        )
        .map_err(|_| ApiError::internal("create complimentary grant"))?;
        let actor_role = if founder {
            "founder".to_string()
        } else {
            OrganizationRole::parse(&actor.role)?.as_str().to_string()
        };
        Self::audit(
            &mut tx,
            actor,
            &actor_role,
            audit_organization_id,
            user_id,
            "complimentary_grant",
            &id.to_string(),
            "create",
            &input.reason,
            None,
            Some(
                json!({"plan": plan, "seat_limit": input.seat_limit, "ai_budget_override": input.ai_budget_override}),
            ),
            request_id,
        )?;
        tx.commit()
            .map_err(|_| ApiError::internal("commit complimentary grant"))?;
        self.grant_by_id(id)
    }

    pub fn create_complimentary_user_grant(
        &mut self,
        actor: &AccountView,
        access_token: &str,
        proof: &str,
        input: GrantInput,
        request_id: &str,
    ) -> Result<ComplimentaryGrantView, ApiError> {
        self.insert_grant(actor, access_token, proof, input, true, request_id)
    }

    pub fn create_complimentary_company_grant(
        &mut self,
        actor: &AccountView,
        access_token: &str,
        proof: &str,
        input: GrantInput,
        request_id: &str,
    ) -> Result<ComplimentaryGrantView, ApiError> {
        self.insert_grant(actor, access_token, proof, input, false, request_id)
    }

    fn grant_by_id(&mut self, id: Uuid) -> Result<ComplimentaryGrantView, ApiError> {
        let row = self
            .client
            .query_opt(
                "SELECT id,user_id,organization_id,plan_template,
                        to_char(starts_at,'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"'),
                        CASE WHEN expires_at IS NULL THEN NULL ELSE to_char(expires_at,'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') END,
                        seat_limit,ai_budget_override,revoked_at IS NOT NULL
                 FROM complimentary_grants WHERE id=$1",
                &[&id],
            )
            .map_err(|_| ApiError::internal("query complimentary grant"))?
            .ok_or_else(|| ApiError::not_found("complimentary grant does not exist"))?;
        let plan = parse_plan(&row.get::<_, String>(3))?;
        Ok(ComplimentaryGrantView {
            id: row.get::<_, Uuid>(0).to_string(),
            user_id: row.get::<_, Option<Uuid>>(1).map(|value| value.to_string()),
            organization_id: row.get::<_, Option<Uuid>>(2).map(|value| value.to_string()),
            plan,
            starts_at: row.get(4),
            expires_at: row.get(5),
            seat_limit: to_u64(row.get(6)),
            ai_budget_override: to_u64(row.get(7)),
            revoked: row.get(8),
        })
    }

    pub fn modify_complimentary_grant(
        &mut self,
        actor: &AccountView,
        access_token: &str,
        proof: &str,
        grant_id: &str,
        change: GrantModifyRequest,
        request_id: &str,
    ) -> Result<ComplimentaryGrantView, ApiError> {
        require_reason(&change.reason)?;
        let id = parse_uuid_for_api(grant_id, "grant identifier is invalid")?;
        let before = self
            .client
            .query_opt(
                "SELECT user_id,organization_id,plan_template,seat_limit,ai_budget_override,revoked_at IS NOT NULL
                 FROM complimentary_grants WHERE id=$1",
                &[&id],
            )
            .map_err(|_| ApiError::internal("query grant for modification"))?
            .ok_or_else(|| ApiError::not_found("complimentary grant does not exist"))?;
        let target_user = before.get::<_, Option<Uuid>>(0);
        let target_org = before.get::<_, Option<Uuid>>(1);
        let (founder, _) = self.require_admin(actor)?;
        self.require_target_scope(actor, target_user, target_org, founder)?;
        self.consume_mfa_proof(actor, access_token, proof)?;
        if before.get::<_, bool>(5) {
            return Err(ApiError::conflict("revoked grant cannot be modified"));
        }
        let old_value = json!({
            "plan": before.get::<_, String>(2),
            "seat_limit": before.get::<_, Option<i64>>(3),
            "ai_budget_override": before.get::<_, Option<i64>>(4),
        });
        let plan = change
            .plan
            .map(|value| plan_name(value).to_string())
            .unwrap_or_else(|| before.get::<_, String>(2));
        let seat_limit = match change.seat_limit {
            Some(value) => value.map(|value| value.saturating_cast_i64()),
            None => before.get::<_, Option<i64>>(3),
        };
        let ai_budget = match change.ai_budget_override {
            Some(value) => value.map(|value| value.saturating_cast_i64()),
            None => before.get::<_, Option<i64>>(4),
        };
        let expiry = change.expires_in_seconds.unwrap_or(None);
        if let Some(seconds) = expiry {
            if seconds <= 0 {
                return Err(ApiError::bad_request("grant expiry must be positive"));
            }
        }
        let mut tx = self
            .client
            .transaction()
            .map_err(|_| ApiError::internal("begin grant modification"))?;
        tx.execute(
            "UPDATE complimentary_grants SET plan_template=$1,seat_limit=$2,ai_budget_override=$3,
                expires_at=CASE WHEN $4::BIGINT IS NULL THEN NULL
                    ELSE CURRENT_TIMESTAMP + ($4::BIGINT * INTERVAL '1 second') END,
                updated_at=CURRENT_TIMESTAMP WHERE id=$5 AND revoked_at IS NULL",
            &[&plan, &seat_limit, &ai_budget, &expiry, &id],
        )
        .map_err(|_| ApiError::internal("modify complimentary grant"))?;
        let actor_role = if founder {
            "founder".to_string()
        } else {
            OrganizationRole::parse(&actor.role)?.as_str().to_string()
        };
        Self::audit(
            &mut tx,
            actor,
            &actor_role,
            target_org,
            target_user,
            "complimentary_grant",
            grant_id,
            "modify",
            &change.reason,
            Some(old_value),
            Some(json!({"plan": plan, "seat_limit": seat_limit, "ai_budget_override": ai_budget})),
            request_id,
        )?;
        tx.commit()
            .map_err(|_| ApiError::internal("commit grant modification"))?;
        self.grant_by_id(id)
    }

    pub fn revoke_complimentary_grant(
        &mut self,
        actor: &AccountView,
        access_token: &str,
        proof: &str,
        grant_id: &str,
        reason: &str,
        request_id: &str,
    ) -> Result<ComplimentaryGrantView, ApiError> {
        require_reason(reason)?;
        let id = parse_uuid_for_api(grant_id, "grant identifier is invalid")?;
        let row = self
            .client
            .query_opt(
                "SELECT user_id,organization_id,revoked_at IS NOT NULL FROM complimentary_grants WHERE id=$1",
                &[&id],
            )
            .map_err(|_| ApiError::internal("query grant for revocation"))?
            .ok_or_else(|| ApiError::not_found("complimentary grant does not exist"))?;
        let target_user = row.get::<_, Option<Uuid>>(0);
        let target_org = row.get::<_, Option<Uuid>>(1);
        let (founder, _) = self.require_admin(actor)?;
        self.require_target_scope(actor, target_user, target_org, founder)?;
        self.consume_mfa_proof(actor, access_token, proof)?;
        if row.get::<_, bool>(2) {
            return self.grant_by_id(id);
        }
        let mut tx = self
            .client
            .transaction()
            .map_err(|_| ApiError::internal("begin grant revocation"))?;
        tx.execute(
            "UPDATE complimentary_grants SET revoked_at=CURRENT_TIMESTAMP,updated_at=CURRENT_TIMESTAMP WHERE id=$1 AND revoked_at IS NULL",
            &[&id],
        )
        .map_err(|_| ApiError::internal("revoke complimentary grant"))?;
        let actor_role = if founder {
            "founder".to_string()
        } else {
            OrganizationRole::parse(&actor.role)?.as_str().to_string()
        };
        Self::audit(
            &mut tx,
            actor,
            &actor_role,
            target_org,
            target_user,
            "complimentary_grant",
            grant_id,
            "revoke",
            reason,
            Some(json!({"revoked": false})),
            Some(json!({"revoked": true})),
            request_id,
        )?;
        tx.commit()
            .map_err(|_| ApiError::internal("commit grant revocation"))?;
        self.grant_by_id(id)
    }

    pub fn organization_members(
        &mut self,
        actor: &AccountView,
    ) -> Result<Vec<OrganizationMemberView>, ApiError> {
        let organization_id = super::parse_uuid(&actor.organization.id)?;
        self.client
            .query(
                "SELECT u.id,u.email,m.role FROM organization_memberships m
                 JOIN users u ON u.id=m.user_id WHERE m.organization_id=$1 ORDER BY u.email",
                &[&organization_id],
            )
            .map_err(|_| ApiError::internal("query organization members"))?
            .into_iter()
            .map(|row| {
                Ok(OrganizationMemberView {
                    user: User {
                        id: row.get::<_, Uuid>(0).to_string(),
                        email: row.get(1),
                    },
                    role: OrganizationRole::parse(row.get::<_, String>(2).as_str())?,
                })
            })
            .collect()
    }

    pub fn add_organization_member(
        &mut self,
        actor: &AccountView,
        access_token: &str,
        proof: &str,
        user_id: &str,
        role: OrganizationRole,
        request_id: &str,
    ) -> Result<OrganizationMemberView, ApiError> {
        let actor_role = self.actor_org_role(actor)?;
        if !actor_role.can_manage_members()
            || (role == OrganizationRole::Admin && !actor_role.can_assign_admin())
        {
            return Err(ApiError::forbidden(
                "organization role cannot add this member",
            ));
        }
        self.consume_mfa_proof(actor, access_token, proof)?;
        let organization_id = super::parse_uuid(&actor.organization.id)?;
        let user_id = parse_uuid_for_api(user_id, "member user identifier is invalid")?;
        let mut tx = self
            .client
            .transaction()
            .map_err(|_| ApiError::internal("begin member transaction"))?;
        tx.query_one(
            "SELECT id FROM organizations WHERE id=$1 FOR UPDATE",
            &[&organization_id],
        )
        .map_err(|_| ApiError::not_found("organization does not exist"))?;
        let exists = tx
            .query_opt("SELECT 1 FROM users WHERE id=$1", &[&user_id])
            .map_err(|_| ApiError::internal("query member user"))?
            .is_some();
        if !exists {
            return Err(ApiError::not_found("member user does not exist"));
        }
        let limit = tx
            .query_opt(
                "SELECT seat_limit FROM complimentary_grants
                 WHERE organization_id=$1 AND starts_at<=CURRENT_TIMESTAMP AND revoked_at IS NULL
                   AND (expires_at IS NULL OR expires_at>=CURRENT_TIMESTAMP)
                 ORDER BY starts_at DESC LIMIT 1",
                &[&organization_id],
            )
            .map_err(|_| ApiError::internal("query organization seat grant"))?
            .and_then(|row| row.get::<_, Option<i64>>(0));
        let count: i64 = tx
            .query_one(
                "SELECT COUNT(*)::BIGINT FROM organization_memberships WHERE organization_id=$1",
                &[&organization_id],
            )
            .map_err(|_| ApiError::internal("count organization seats"))?
            .get(0);
        if let Some(limit) = limit {
            if count >= limit {
                return Err(ApiError::forbidden(
                    "organization seat limit has been reached",
                ));
            }
        }
        tx.execute(
            "INSERT INTO organization_memberships(organization_id,user_id,role,updated_at)
             VALUES($1,$2,$3,CURRENT_TIMESTAMP)",
            &[&organization_id, &user_id, &role.as_str()],
        )
        .map_err(|_| ApiError::conflict("user is already an organization member"))?;
        Self::audit(
            &mut tx,
            actor,
            actor_role.as_str(),
            Some(organization_id),
            Some(user_id),
            "organization_membership",
            &user_id.to_string(),
            "add",
            "member added",
            None,
            Some(json!({"role": role.as_str()})),
            request_id,
        )?;
        let row = tx
            .query_one("SELECT email FROM users WHERE id=$1", &[&user_id])
            .map_err(|_| ApiError::internal("query added member"))?;
        tx.commit()
            .map_err(|_| ApiError::internal("commit member transaction"))?;
        Ok(OrganizationMemberView {
            user: User {
                id: user_id.to_string(),
                email: row.get(0),
            },
            role,
        })
    }

    pub fn remove_organization_member(
        &mut self,
        actor: &AccountView,
        access_token: &str,
        proof: &str,
        user_id: &str,
        reason: &str,
        request_id: &str,
    ) -> Result<bool, ApiError> {
        require_reason(reason)?;
        let role = self.actor_org_role(actor)?;
        if !role.can_manage_members() {
            return Err(ApiError::forbidden("organization admin role is required"));
        }
        self.consume_mfa_proof(actor, access_token, proof)?;
        let organization_id = super::parse_uuid(&actor.organization.id)?;
        let user_id = parse_uuid_for_api(user_id, "member user identifier is invalid")?;
        if user_id == super::parse_uuid(&actor.user.id)? {
            return Err(ApiError::bad_request(
                "an administrator cannot remove their active membership",
            ));
        }
        let mut tx = self
            .client
            .transaction()
            .map_err(|_| ApiError::internal("begin member removal"))?;
        let changed = tx
            .execute(
                "DELETE FROM organization_memberships WHERE organization_id=$1 AND user_id=$2 AND role <> 'owner'",
                &[&organization_id, &user_id],
            )
            .map_err(|_| ApiError::internal("remove organization member"))?;
        if changed != 1 {
            return Err(ApiError::not_found(
                "removable organization member not found",
            ));
        }
        Self::audit(
            &mut tx,
            actor,
            role.as_str(),
            Some(organization_id),
            Some(user_id),
            "organization_membership",
            &user_id.to_string(),
            "remove",
            reason,
            Some(json!({"member": user_id})),
            None,
            request_id,
        )?;
        tx.commit()
            .map_err(|_| ApiError::internal("commit member removal"))?;
        Ok(true)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn change_organization_member_role(
        &mut self,
        actor: &AccountView,
        access_token: &str,
        proof: &str,
        user_id: &str,
        role: OrganizationRole,
        reason: &str,
        request_id: &str,
    ) -> Result<OrganizationMemberView, ApiError> {
        require_reason(reason)?;
        let actor_role = self.actor_org_role(actor)?;
        if !actor_role.can_assign_admin() {
            return Err(ApiError::forbidden("organization owner role is required"));
        }
        self.consume_mfa_proof(actor, access_token, proof)?;
        let organization_id = super::parse_uuid(&actor.organization.id)?;
        let user_id = parse_uuid_for_api(user_id, "member user identifier is invalid")?;
        if user_id == super::parse_uuid(&actor.user.id)? {
            return Err(ApiError::bad_request(
                "the owner role cannot be changed through this route",
            ));
        }
        let mut tx = self
            .client
            .transaction()
            .map_err(|_| ApiError::internal("begin role change"))?;
        let old = tx
            .query_opt(
                "SELECT role FROM organization_memberships WHERE organization_id=$1 AND user_id=$2 FOR UPDATE",
                &[&organization_id, &user_id],
            )
            .map_err(|_| ApiError::internal("query member role"))?
            .ok_or_else(|| ApiError::not_found("organization member does not exist"))?;
        let old_role: String = old.get(0);
        tx.execute(
            "UPDATE organization_memberships SET role=$1,updated_at=CURRENT_TIMESTAMP
             WHERE organization_id=$2 AND user_id=$3",
            &[&role.as_str(), &organization_id, &user_id],
        )
        .map_err(|_| ApiError::internal("update member role"))?;
        Self::audit(
            &mut tx,
            actor,
            actor_role.as_str(),
            Some(organization_id),
            Some(user_id),
            "organization_membership",
            &user_id.to_string(),
            "role_change",
            reason,
            Some(json!({"role": old_role})),
            Some(json!({"role": role.as_str()})),
            request_id,
        )?;
        let email: String = tx
            .query_one("SELECT email FROM users WHERE id=$1", &[&user_id])
            .map_err(|_| ApiError::internal("query role-changed member"))?
            .get(0);
        tx.commit()
            .map_err(|_| ApiError::internal("commit role change"))?;
        Ok(OrganizationMemberView {
            user: User {
                id: user_id.to_string(),
                email,
            },
            role,
        })
    }

    pub fn subscriptions(
        &mut self,
        actor: &AccountView,
    ) -> Result<Vec<SubscriptionView>, ApiError> {
        let user_id = super::parse_uuid(&actor.user.id)?;
        let organization_id = super::parse_uuid(&actor.organization.id)?;
        self.client
            .query(
                "SELECT id,organization_id,user_id,plan_id,billing_interval,status,
                        CASE WHEN status='trial' AND trial_ends_at < CURRENT_TIMESTAMP THEN 'TRIAL_EXPIRED'
                             WHEN status='trial' THEN 'TRIAL_ACTIVE'
                             WHEN status='active' AND current_period_end IS NOT NULL AND current_period_end < CURRENT_TIMESTAMP THEN 'EXPIRED'
                             ELSE upper(status) END,
                        seat_limit,provider
                 FROM subscriptions WHERE user_id=$1 OR organization_id=$2 ORDER BY starts_at DESC",
                &[&user_id, &organization_id],
            )
            .map_err(|_| ApiError::internal("query subscriptions"))?
            .into_iter()
            .map(|row| {
                Ok(SubscriptionView {
                    id: row.get::<_, Uuid>(0).to_string(),
                    organization_id: row.get::<_, Option<Uuid>>(1).map(|value| value.to_string()),
                    user_id: row.get::<_, Option<Uuid>>(2).map(|value| value.to_string()),
                    plan: json_plan(row.get::<_, String>(3).as_str())?,
                    billing_interval: row.get(4),
                    state: row.get(6),
                    seat_limit: to_u64(row.get(7)),
                    provider: row.get(8),
                })
            })
            .collect()
    }

    pub fn audit_events(
        &mut self,
        actor: &AccountView,
        organization_id: Option<&str>,
    ) -> Result<Vec<AdminAuditEventView>, ApiError> {
        let (founder, _) = self.require_admin(actor)?;
        let actor_org = super::parse_uuid(&actor.organization.id)?;
        let requested_org = organization_id
            .map(|value| parse_uuid_for_api(value, "organization identifier is invalid"))
            .transpose()?;
        if !founder && requested_org.is_some_and(|value| value != actor_org) {
            return Err(ApiError::forbidden("cross-tenant audit access is denied"));
        }
        let scope = requested_org.unwrap_or(actor_org);
        self.client
            .query(
                "SELECT id,actor_user_id,actor_role,organization_id,target_user_id,
                        entity_type,entity_id,operation,reason,outcome,request_id,
                        to_char(created_at,'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"')
                 FROM p10_admin_audit_events WHERE organization_id=$1
                 ORDER BY created_at DESC LIMIT 200",
                &[&scope],
            )
            .map_err(|_| ApiError::internal("query admin audit events"))?
            .into_iter()
            .map(|row| {
                Ok(AdminAuditEventView {
                    id: row.get::<_, Uuid>(0).to_string(),
                    actor_user_id: row.get::<_, Option<Uuid>>(1).map(|value| value.to_string()),
                    actor_role: row.get(2),
                    organization_id: row.get::<_, Option<Uuid>>(3).map(|value| value.to_string()),
                    target_user_id: row.get::<_, Option<Uuid>>(4).map(|value| value.to_string()),
                    entity_type: row.get(5),
                    entity_id: row.get(6),
                    operation: row.get(7),
                    reason: row.get(8),
                    outcome: row.get(9),
                    request_id: row.get(10),
                    created_at: row.get(11),
                })
            })
            .collect()
    }

    fn p10_route_impl(
        &mut self,
        config: &Config,
        account: &AccountView,
        access_token: &str,
        request: &HttpRequest,
        request_id: &str,
    ) -> Result<Value, ApiError> {
        let path = request.path.as_str();
        match (request.method.as_str(), path) {
            ("GET", "/v1/organizations/members") => Ok(json!(self.organization_members(account)?)),
            ("POST", "/v1/organizations/members") => {
                let body: MemberRequest = read_json(request)?;
                let proof = proof_header(request)?;
                Ok(json!(self.add_organization_member(
                    account,
                    access_token,
                    proof,
                    &body.user_id,
                    body.role,
                    request_id,
                )?))
            }
            ("GET", "/v1/organizations/policy") => Ok(json!(self.team_policy(account)?)),
            ("PUT", "/v1/organizations/policy") => {
                let body: PolicyUpdateRequest = read_json(request)?;
                require_reason(&body.reason)?;
                let proof = proof_header(request)?;
                Ok(json!(self.update_team_policy(
                    account,
                    access_token,
                    proof,
                    body.expected_version,
                    body.policy,
                    &body.reason,
                    request_id,
                )?))
            }
            ("GET", "/v1/billing/status") => {
                let adapter = UnavailableBillingAdapter;
                Ok(
                    json!({"billing": adapter.status(), "subscriptions": self.subscriptions(account)?}),
                )
            }
            ("POST", "/v1/admin/mfa/challenge") => {
                Ok(json!(self.begin_admin_mfa(account, access_token)?))
            }
            ("POST", "/v1/admin/mfa/verify") => {
                let body: MfaVerifyRequest = read_json(request)?;
                Ok(json!(self.verify_admin_mfa(
                    account,
                    access_token,
                    &body.challenge_id,
                    &body.code,
                )?))
            }
            ("POST", "/v1/admin/grants/user") => {
                let proof = proof_header(request)?;
                let body: GrantInput = read_json(request)?;
                Ok(json!(self.create_complimentary_user_grant(
                    account,
                    access_token,
                    proof,
                    body,
                    request_id,
                )?))
            }
            ("POST", "/v1/admin/grants/company") => {
                let proof = proof_header(request)?;
                let body: GrantInput = read_json(request)?;
                Ok(json!(self.create_complimentary_company_grant(
                    account,
                    access_token,
                    proof,
                    body,
                    request_id,
                )?))
            }
            ("GET", "/v1/admin/audit") => Ok(json!(self.audit_events(account, None)?)),
            ("PATCH", value) if value.starts_with("/v1/admin/grants/") => {
                let id = value.trim_start_matches("/v1/admin/grants/");
                let proof = proof_header(request)?;
                let body: GrantModifyRequest = read_json(request)?;
                Ok(json!(self.modify_complimentary_grant(
                    account,
                    access_token,
                    proof,
                    id,
                    body,
                    request_id,
                )?))
            }
            ("POST", value)
                if value.ends_with("/revoke") && value.starts_with("/v1/admin/grants/") =>
            {
                let id = value
                    .trim_start_matches("/v1/admin/grants/")
                    .trim_end_matches("/revoke")
                    .trim_end_matches('/');
                let body: GrantReasonRequest = read_json(request)?;
                let proof = proof_header(request)?;
                Ok(json!(self.revoke_complimentary_grant(
                    account,
                    access_token,
                    proof,
                    id,
                    &body.reason,
                    request_id,
                )?))
            }
            ("DELETE", value) if value.starts_with("/v1/organizations/members/") => {
                let member_id = value.trim_start_matches("/v1/organizations/members/");
                let body: GrantReasonRequest = read_json(request)?;
                let proof = proof_header(request)?;
                Ok(json!({"removed": self.remove_organization_member(
                    account,
                    access_token,
                    proof,
                    member_id,
                    &body.reason,
                    request_id,
                )?}))
            }
            ("PATCH", value) if value.starts_with("/v1/organizations/members/") => {
                let member_id = value.trim_start_matches("/v1/organizations/members/");
                let body: RoleChangeRequest = read_json(request)?;
                let proof = proof_header(request)?;
                Ok(json!(self.change_organization_member_role(
                    account,
                    access_token,
                    proof,
                    member_id,
                    body.role,
                    &body.reason,
                    request_id,
                )?))
            }
            _ => {
                let _ = config;
                Err(ApiError::not_found("P10 route not found"))
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct GrantReasonRequest {
    reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RoleChangeRequest {
    role: OrganizationRole,
    reason: String,
}

impl P10Repository for PostgresStore {
    fn p10_route(
        &mut self,
        config: &Config,
        account: &AccountView,
        access_token: &str,
        request: &HttpRequest,
        request_id: &str,
    ) -> Result<Value, ApiError> {
        self.p10_route_impl(config, account, access_token, request, request_id)
    }
}

#[derive(Debug, Default)]
pub(crate) struct MemoryP10State {
    admins: BTreeMap<String, (bool, bool, Vec<u8>)>,
    challenges: BTreeMap<String, MemoryChallenge>,
    proofs: BTreeMap<String, MemoryProof>,
    policies: BTreeMap<String, (u64, Value, String)>,
    grants: BTreeMap<String, MemoryGrant>,
    subscriptions: Vec<MemorySubscription>,
    audit: Vec<AdminAuditEventView>,
}

#[derive(Debug, Clone)]
struct MemoryChallenge {
    user_id: String,
    session_hash: String,
    expires_at: i64,
    used: bool,
}

#[derive(Debug, Clone)]
struct MemoryProof {
    user_id: String,
    session_hash: String,
    expires_at: i64,
    used: bool,
}

#[derive(Debug, Clone)]
struct MemoryGrant {
    id: String,
    user_id: Option<String>,
    organization_id: Option<String>,
    plan: PlanTier,
    starts_at: i64,
    expires_at: Option<i64>,
    seat_limit: Option<u64>,
    ai_budget_override: Option<u64>,
    revoked: bool,
}

#[derive(Debug, Clone)]
struct MemorySubscription {
    _id: String,
    organization_id: Option<String>,
    user_id: Option<String>,
    plan: PlanTier,
    _billing_interval: String,
    status: String,
    expires_at: Option<i64>,
    seat_limit: Option<u64>,
}

impl MemoryStore {
    fn memory_admin(&self, actor: &AccountView) -> Result<(bool, bool, Vec<u8>), ApiError> {
        self.p10
            .admins
            .get(&actor.user.id)
            .cloned()
            .ok_or_else(|| ApiError::forbidden("admin authority is required"))
    }

    fn consume_memory_proof(
        &mut self,
        actor: &AccountView,
        access_token: &str,
        proof: &str,
    ) -> Result<(), ApiError> {
        let _ = self.memory_admin(actor)?;
        let key = hash_value(proof);
        let Some(value) = self.p10.proofs.get_mut(&key) else {
            return Err(ApiError::unauthorized(
                "MFA proof is invalid, expired, or replayed",
            ));
        };
        if value.used
            || value.user_id != actor.user.id
            || value.session_hash != super::hash_token(access_token)
            || value.expires_at < now_seconds()
        {
            return Err(ApiError::unauthorized(
                "MFA proof is invalid, expired, or replayed",
            ));
        }
        value.used = true;
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn seed_admin_for_tests(
        &mut self,
        actor: &AccountView,
        founder: bool,
        secret: &[u8],
    ) {
        self.p10
            .admins
            .insert(actor.user.id.clone(), (founder, true, secret.to_vec()));
    }

    #[cfg(test)]
    pub(crate) fn seed_trial_for_tests(
        &mut self,
        actor: &AccountView,
        plan: PlanTier,
        expires_at: i64,
        seat_limit: Option<u64>,
    ) {
        self.p10.subscriptions.push(MemorySubscription {
            _id: random_token("sub"),
            organization_id: Some(actor.organization.id.clone()),
            user_id: Some(actor.user.id.clone()),
            plan,
            _billing_interval: "monthly".into(),
            status: "trial".into(),
            expires_at: Some(expires_at),
            seat_limit,
        });
    }

    pub(crate) fn p10_active_policy(
        &mut self,
        account: &AccountView,
    ) -> Result<Option<EntitlementPolicy>, ApiError> {
        let now = now_seconds();
        if let Some((true, _, _)) = self.p10.admins.get(&account.user.id) {
            let mut limits = BTreeMap::new();
            limits.insert("team_members".into(), u64::MAX);
            limits.insert("monthly_ai_units".into(), u64::MAX);
            return Ok(Some(EntitlementPolicy {
                plan: PlanTier::Enterprise,
                source: EntitlementSource::Founder,
                capabilities: super::capability_names(PlanTier::Enterprise),
                limits,
                expires_at: None,
                grace_until: None,
            }));
        }
        if let Some(grant) = self
            .p10
            .grants
            .values()
            .filter(|grant| {
                !grant.revoked
                    && grant.starts_at <= now
                    && grant.expires_at.is_none_or(|value| value >= now)
                    && (grant.user_id.as_deref() == Some(account.user.id.as_str())
                        || grant.organization_id.as_deref()
                            == Some(account.organization.id.as_str()))
            })
            .max_by_key(|grant| (grant.organization_id.is_some(), grant.starts_at))
        {
            let mut limits = BTreeMap::new();
            if let Some(value) = grant.seat_limit {
                limits.insert("team_members".into(), value);
            }
            if let Some(value) = grant.ai_budget_override {
                limits.insert("monthly_ai_units".into(), value);
            }
            return Ok(Some(EntitlementPolicy {
                plan: grant.plan,
                source: EntitlementSource::Complimentary,
                capabilities: super::capability_names(grant.plan),
                limits,
                expires_at: grant.expires_at,
                grace_until: grant
                    .expires_at
                    .map(|value| value + super::DEFAULT_GRACE_SECONDS),
            }));
        }
        if let Some(subscription) = self
            .p10
            .subscriptions
            .iter()
            .filter(|subscription| {
                (subscription.user_id.as_deref() == Some(account.user.id.as_str())
                    || subscription.organization_id.as_deref()
                        == Some(account.organization.id.as_str()))
                    && ((subscription.status == "active"
                        && subscription.expires_at.is_none_or(|value| value >= now))
                        || (subscription.status == "trial"
                            && subscription.expires_at.is_some_and(|value| value >= now)))
            })
            .max_by_key(|subscription| subscription.expires_at.unwrap_or(i64::MAX))
        {
            let source = if subscription.status == "trial" {
                EntitlementSource::Trial
            } else {
                EntitlementSource::Plan
            };
            let mut limits = BTreeMap::new();
            if let Some(value) = subscription.seat_limit {
                limits.insert("team_members".into(), value);
            }
            return Ok(Some(EntitlementPolicy {
                plan: subscription.plan,
                source,
                capabilities: super::capability_names(subscription.plan),
                limits,
                expires_at: subscription.expires_at,
                grace_until: subscription
                    .expires_at
                    .map(|value| value + super::DEFAULT_GRACE_SECONDS),
            }));
        }
        if self.p10.subscriptions.iter().any(|subscription| {
            (subscription.user_id.as_deref() == Some(account.user.id.as_str())
                || subscription.organization_id.as_deref()
                    == Some(account.organization.id.as_str()))
                && subscription.expires_at.is_some_and(|value| value < now)
                && (subscription.status == "trial" || subscription.status == "active")
        }) {
            return Err(ApiError::forbidden("subscription or trial has expired"));
        }
        Ok(None)
    }

    pub fn begin_memory_admin_mfa(
        &mut self,
        actor: &AccountView,
        access_token: &str,
    ) -> Result<MfaChallengeView, ApiError> {
        let _ = self.memory_admin(actor)?;
        let id = random_token("challenge");
        self.p10.challenges.insert(
            id.clone(),
            MemoryChallenge {
                user_id: actor.user.id.clone(),
                session_hash: super::hash_token(access_token),
                expires_at: now_seconds() + MFA_CHALLENGE_SECONDS,
                used: false,
            },
        );
        Ok(MfaChallengeView {
            challenge_id: id,
            expires_at: now_seconds() + MFA_CHALLENGE_SECONDS,
        })
    }

    pub fn verify_memory_admin_mfa(
        &mut self,
        actor: &AccountView,
        access_token: &str,
        challenge_id: &str,
        code: &str,
    ) -> Result<MfaProofView, ApiError> {
        let (_, _, secret) = self.memory_admin(actor)?;
        let Some(challenge) = self.p10.challenges.get_mut(challenge_id) else {
            return Err(ApiError::unauthorized("MFA challenge is invalid"));
        };
        if challenge.used
            || challenge.expires_at < now_seconds()
            || challenge.user_id != actor.user.id
            || challenge.session_hash != super::hash_token(access_token)
            || !valid_mfa_code(&secret, code, now_seconds())
        {
            return Err(ApiError::unauthorized("MFA challenge or code is invalid"));
        }
        challenge.used = true;
        let proof = random_token("mfa-proof");
        self.p10.proofs.insert(
            hash_value(&proof),
            MemoryProof {
                user_id: actor.user.id.clone(),
                session_hash: super::hash_token(access_token),
                expires_at: now_seconds() + MFA_PROOF_SECONDS,
                used: false,
            },
        );
        Ok(MfaProofView {
            proof,
            expires_at: now_seconds() + MFA_PROOF_SECONDS,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn memory_audit(
        &mut self,
        actor: &AccountView,
        role: &str,
        entity_type: &str,
        entity_id: &str,
        operation: &str,
        reason: &str,
        request_id: &str,
    ) {
        self.p10.audit.push(AdminAuditEventView {
            id: random_token("audit"),
            actor_user_id: Some(actor.user.id.clone()),
            actor_role: role.into(),
            organization_id: Some(actor.organization.id.clone()),
            target_user_id: None,
            entity_type: entity_type.into(),
            entity_id: entity_id.into(),
            operation: operation.into(),
            reason: reason.into(),
            outcome: "success".into(),
            request_id: request_id.into(),
            created_at: now_seconds().to_string(),
        });
    }

    fn memory_grant_view(grant: &MemoryGrant) -> ComplimentaryGrantView {
        ComplimentaryGrantView {
            id: grant.id.clone(),
            user_id: grant.user_id.clone(),
            organization_id: grant.organization_id.clone(),
            plan: grant.plan,
            starts_at: grant.starts_at.to_string(),
            expires_at: grant.expires_at.map(|value| value.to_string()),
            seat_limit: grant.seat_limit,
            ai_budget_override: grant.ai_budget_override,
            revoked: grant.revoked,
        }
    }

    fn memory_insert_grant(
        &mut self,
        actor: &AccountView,
        access_token: &str,
        proof: &str,
        input: GrantInput,
        user_target: bool,
        request_id: &str,
    ) -> Result<ComplimentaryGrantView, ApiError> {
        require_reason(&input.reason)?;
        let (founder, _, _) = self.memory_admin(actor)?;
        let role = OrganizationRole::parse(&actor.role)?;
        if !founder && !role.can_manage_members() {
            return Err(ApiError::forbidden("organization admin role is required"));
        }
        let target_scope = input
            .organization_id
            .clone()
            .or_else(|| Some(actor.organization.id.clone()));
        if !founder && target_scope.as_deref() != Some(actor.organization.id.as_str()) {
            return Err(ApiError::forbidden("cross-tenant administration is denied"));
        }
        if !founder {
            if let Some(target_user) = input.user_id.as_deref() {
                let target_scope = self
                    .memberships
                    .keys()
                    .find(|(user_id, _)| user_id == target_user)
                    .map(|(_, organization_id)| organization_id.as_str());
                if target_scope != Some(actor.organization.id.as_str()) {
                    return Err(ApiError::forbidden("cross-tenant administration is denied"));
                }
            }
        }
        if user_target == input.user_id.is_none()
            || (!user_target && input.organization_id.is_none())
        {
            return Err(ApiError::bad_request("grant target shape is invalid"));
        }
        self.consume_memory_proof(actor, access_token, proof)?;
        let id = random_token("grant");
        let grant = MemoryGrant {
            id: id.clone(),
            user_id: input.user_id,
            organization_id: if user_target { None } else { target_scope },
            plan: input.plan,
            starts_at: now_seconds(),
            expires_at: input.expires_in_seconds.map(|value| now_seconds() + value),
            seat_limit: input.seat_limit,
            ai_budget_override: input.ai_budget_override,
            revoked: false,
        };
        self.p10.grants.insert(id.clone(), grant.clone());
        self.memory_audit(
            actor,
            if founder { "founder" } else { role.as_str() },
            "complimentary_grant",
            &id,
            "create",
            &input.reason,
            request_id,
        );
        Ok(Self::memory_grant_view(&grant))
    }

    pub fn create_memory_user_grant(
        &mut self,
        actor: &AccountView,
        access_token: &str,
        proof: &str,
        input: GrantInput,
        request_id: &str,
    ) -> Result<ComplimentaryGrantView, ApiError> {
        self.memory_insert_grant(actor, access_token, proof, input, true, request_id)
    }

    pub fn create_memory_company_grant(
        &mut self,
        actor: &AccountView,
        access_token: &str,
        proof: &str,
        input: GrantInput,
        request_id: &str,
    ) -> Result<ComplimentaryGrantView, ApiError> {
        self.memory_insert_grant(actor, access_token, proof, input, false, request_id)
    }

    pub fn add_memory_member(
        &mut self,
        actor: &AccountView,
        access_token: &str,
        proof: &str,
        user_id: &str,
        role: OrganizationRole,
        request_id: &str,
    ) -> Result<OrganizationMemberView, ApiError> {
        let actor_role = OrganizationRole::parse(&actor.role)?;
        if !actor_role.can_manage_members()
            || (role == OrganizationRole::Admin && !actor_role.can_assign_admin())
        {
            return Err(ApiError::forbidden(
                "organization role cannot add this member",
            ));
        }
        self.consume_memory_proof(actor, access_token, proof)?;
        let target = self
            .users
            .get(user_id)
            .cloned()
            .ok_or_else(|| ApiError::not_found("member user does not exist"))?;
        let count = self
            .memberships
            .keys()
            .filter(|(_, organization_id)| organization_id == &actor.organization.id)
            .count() as u64;
        let now = now_seconds();
        let limit = self
            .p10
            .grants
            .values()
            .filter(|grant| {
                !grant.revoked
                    && grant.organization_id.as_deref() == Some(actor.organization.id.as_str())
                    && grant.starts_at <= now
                    && grant.expires_at.is_none_or(|value| value >= now)
            })
            .filter_map(|grant| grant.seat_limit)
            .max();
        if limit.is_some_and(|value| count >= value) {
            return Err(ApiError::forbidden(
                "organization seat limit has been reached",
            ));
        }
        if self
            .memberships
            .contains_key(&(user_id.to_string(), actor.organization.id.clone()))
        {
            return Err(ApiError::conflict("user is already an organization member"));
        }
        self.memberships.insert(
            (user_id.to_string(), actor.organization.id.clone()),
            role.as_str().into(),
        );
        self.memory_audit(
            actor,
            actor_role.as_str(),
            "organization_membership",
            user_id,
            "add",
            "member added",
            request_id,
        );
        Ok(OrganizationMemberView { user: target, role })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn change_memory_member_role(
        &mut self,
        actor: &AccountView,
        access_token: &str,
        proof: &str,
        user_id: &str,
        role: OrganizationRole,
        reason: &str,
        request_id: &str,
    ) -> Result<OrganizationMemberView, ApiError> {
        require_reason(reason)?;
        let actor_role = OrganizationRole::parse(&actor.role)?;
        if !actor_role.can_assign_admin() {
            return Err(ApiError::forbidden("organization owner role is required"));
        }
        self.consume_memory_proof(actor, access_token, proof)?;
        let key = (user_id.to_string(), actor.organization.id.clone());
        if user_id == actor.user.id {
            return Err(ApiError::bad_request(
                "the owner role cannot be changed through this route",
            ));
        }
        let membership = self
            .memberships
            .get_mut(&key)
            .ok_or_else(|| ApiError::not_found("organization member does not exist"))?;
        *membership = role.as_str().into();
        let user = self
            .users
            .get(user_id)
            .cloned()
            .ok_or_else(|| ApiError::not_found("member user does not exist"))?;
        self.memory_audit(
            actor,
            actor_role.as_str(),
            "organization_membership",
            user_id,
            "role_change",
            reason,
            request_id,
        );
        Ok(OrganizationMemberView { user, role })
    }

    pub fn revoke_memory_grant(
        &mut self,
        actor: &AccountView,
        access_token: &str,
        proof: &str,
        grant_id: &str,
        reason: &str,
        request_id: &str,
    ) -> Result<ComplimentaryGrantView, ApiError> {
        require_reason(reason)?;
        let _ = self.memory_admin(actor)?;
        self.consume_memory_proof(actor, access_token, proof)?;
        let grant = self
            .p10
            .grants
            .get_mut(grant_id)
            .ok_or_else(|| ApiError::not_found("complimentary grant does not exist"))?;
        grant.revoked = true;
        let view = Self::memory_grant_view(grant);
        self.memory_audit(
            actor,
            &actor.role,
            "complimentary_grant",
            grant_id,
            "revoke",
            reason,
            request_id,
        );
        Ok(view)
    }

    pub fn modify_memory_grant(
        &mut self,
        actor: &AccountView,
        access_token: &str,
        proof: &str,
        grant_id: &str,
        change: GrantModifyRequest,
        request_id: &str,
    ) -> Result<ComplimentaryGrantView, ApiError> {
        require_reason(&change.reason)?;
        self.consume_memory_proof(actor, access_token, proof)?;
        let grant = self
            .p10
            .grants
            .get_mut(grant_id)
            .ok_or_else(|| ApiError::not_found("complimentary grant does not exist"))?;
        if grant.revoked {
            return Err(ApiError::conflict("revoked grant cannot be modified"));
        }
        if let Some(plan) = change.plan {
            grant.plan = plan;
        }
        if let Some(expiry) = change.expires_in_seconds {
            grant.expires_at = expiry.map(|value| now_seconds() + value);
        }
        if let Some(seat_limit) = change.seat_limit {
            grant.seat_limit = seat_limit;
        }
        if let Some(ai_budget) = change.ai_budget_override {
            grant.ai_budget_override = ai_budget;
        }
        let view = Self::memory_grant_view(grant);
        self.memory_audit(
            actor,
            &actor.role,
            "complimentary_grant",
            grant_id,
            "modify",
            &change.reason,
            request_id,
        );
        Ok(view)
    }

    pub fn memory_team_policy(&self, actor: &AccountView) -> TeamPolicyView {
        let (version, policy, updated_by) = self
            .p10
            .policies
            .get(&actor.organization.id)
            .cloned()
            .unwrap_or((0, json!({}), actor.user.id.clone()));
        TeamPolicyView {
            organization_id: actor.organization.id.clone(),
            version,
            policy,
            updated_by,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn update_memory_team_policy(
        &mut self,
        actor: &AccountView,
        access_token: &str,
        proof: &str,
        expected_version: u64,
        policy: Value,
        reason: &str,
        request_id: &str,
    ) -> Result<TeamPolicyView, ApiError> {
        require_reason(reason)?;
        validate_policy(&policy)?;
        let role = OrganizationRole::parse(&actor.role)?;
        if !role.can_manage_members() {
            return Err(ApiError::forbidden("organization admin role is required"));
        }
        self.consume_memory_proof(actor, access_token, proof)?;
        let current = self
            .p10
            .policies
            .get(&actor.organization.id)
            .map(|value| value.0)
            .unwrap_or(0);
        if current != expected_version {
            return Err(ApiError::conflict("team policy version is stale"));
        }
        let next = current + 1;
        self.p10.policies.insert(
            actor.organization.id.clone(),
            (next, policy.clone(), actor.user.id.clone()),
        );
        self.memory_audit(
            actor,
            role.as_str(),
            "team_policy",
            &actor.organization.id,
            "update",
            reason,
            request_id,
        );
        Ok(TeamPolicyView {
            organization_id: actor.organization.id.clone(),
            version: next,
            policy,
            updated_by: actor.user.id.clone(),
        })
    }

    pub fn memory_audit_events(
        &self,
        actor: &AccountView,
    ) -> Result<Vec<AdminAuditEventView>, ApiError> {
        let _ = self.memory_admin(actor)?;
        Ok(self
            .p10
            .audit
            .iter()
            .filter(|event| {
                event.organization_id.as_deref() == Some(actor.organization.id.as_str())
            })
            .cloned()
            .collect())
    }

    fn p10_route_impl(
        &mut self,
        _config: &Config,
        account: &AccountView,
        access_token: &str,
        request: &HttpRequest,
        request_id: &str,
    ) -> Result<Value, ApiError> {
        match (request.method.as_str(), request.path.as_str()) {
            ("GET", "/v1/organizations/policy") => Ok(json!(self.memory_team_policy(account))),
            ("PUT", "/v1/organizations/policy") => {
                let body: PolicyUpdateRequest = read_json(request)?;
                let proof = proof_header(request)?;
                Ok(json!(self.update_memory_team_policy(
                    account,
                    access_token,
                    proof,
                    body.expected_version,
                    body.policy,
                    &body.reason,
                    request_id,
                )?))
            }
            ("POST", "/v1/admin/mfa/challenge") => {
                Ok(json!(self.begin_memory_admin_mfa(account, access_token)?))
            }
            ("POST", "/v1/admin/mfa/verify") => {
                let body: MfaVerifyRequest = read_json(request)?;
                Ok(json!(self.verify_memory_admin_mfa(
                    account,
                    access_token,
                    &body.challenge_id,
                    &body.code
                )?))
            }
            ("POST", "/v1/admin/grants/user") => {
                let body: GrantInput = read_json(request)?;
                let proof = proof_header(request)?;
                Ok(json!(self.create_memory_user_grant(
                    account,
                    access_token,
                    proof,
                    body,
                    request_id
                )?))
            }
            ("POST", "/v1/admin/grants/company") => {
                let body: GrantInput = read_json(request)?;
                let proof = proof_header(request)?;
                Ok(json!(self.create_memory_company_grant(
                    account,
                    access_token,
                    proof,
                    body,
                    request_id
                )?))
            }
            ("PATCH", value) if value.starts_with("/v1/admin/grants/") => {
                let id = value.trim_start_matches("/v1/admin/grants/");
                let body: GrantModifyRequest = read_json(request)?;
                let proof = proof_header(request)?;
                Ok(json!(self.modify_memory_grant(
                    account,
                    access_token,
                    proof,
                    id,
                    body,
                    request_id
                )?))
            }
            ("PATCH", value) if value.starts_with("/v1/organizations/members/") => {
                let member_id = value.trim_start_matches("/v1/organizations/members/");
                let body: RoleChangeRequest = read_json(request)?;
                let proof = proof_header(request)?;
                Ok(json!(self.change_memory_member_role(
                    account,
                    access_token,
                    proof,
                    member_id,
                    body.role,
                    &body.reason,
                    request_id
                )?))
            }
            ("GET", "/v1/admin/audit") => Ok(json!(self.memory_audit_events(account)?)),
            _ => Err(ApiError::not_found("P10 route not found")),
        }
    }
}

impl P10Repository for MemoryStore {
    fn p10_route(
        &mut self,
        config: &Config,
        account: &AccountView,
        access_token: &str,
        request: &HttpRequest,
        request_id: &str,
    ) -> Result<Value, ApiError> {
        self.p10_route_impl(config, account, access_token, request, request_id)
    }
}

// A small checked conversion keeps SQL parameters i64 without allowing a
// renderer-supplied value to wrap into a negative seat/version count.
trait SaturatingCastI64 {
    fn saturating_cast_i64(self) -> i64;
}

impl SaturatingCastI64 for u64 {
    fn saturating_cast_i64(self) -> i64 {
        i64::try_from(self).unwrap_or(i64::MAX)
    }
}

#[cfg(test)]
mod p10_acceptance {
    use super::*;
    use crate::{issue_entitlement, CloudRepository};

    const SECRET: &[u8] = b"relintor-p10-test-mfa-secret";

    fn admin_session(founder: bool) -> (MemoryStore, super::super::SessionView) {
        let mut store = MemoryStore::default();
        let session = store
            .dev_sign_in("admin@example.test", "p10-test")
            .expect("development session");
        store.seed_admin_for_tests(&session.account, founder, SECRET);
        (store, session)
    }

    fn proof(store: &mut MemoryStore, session: &super::super::SessionView) -> String {
        let challenge = store
            .begin_memory_admin_mfa(&session.account, &session.access_token)
            .expect("MFA challenge");
        let code = mfa_code(SECRET, now_seconds() as u64);
        store
            .verify_memory_admin_mfa(
                &session.account,
                &session.access_token,
                &challenge.challenge_id,
                &code,
            )
            .expect("MFA proof")
            .proof
    }

    fn grant_input(
        user_id: Option<String>,
        organization_id: Option<String>,
        seat_limit: Option<u64>,
    ) -> GrantInput {
        GrantInput {
            user_id,
            organization_id,
            plan: PlanTier::Team,
            expires_in_seconds: None,
            seat_limit,
            ai_budget_override: Some(10_000),
            reason: "P10 acceptance grant".into(),
        }
    }

    #[test]
    fn founder_gets_server_authoritative_free_unlimited_entitlement() {
        let (mut store, session) = admin_session(true);
        let policy = store
            .entitlement_policy(&session.account)
            .expect("founder policy");
        assert_eq!(policy.source, EntitlementSource::Founder);
        assert_eq!(policy.limits["team_members"], u64::MAX);
        let document = issue_entitlement(&Config::test_config(), &session.account, policy);
        assert_eq!(document.claims.source, EntitlementSource::Founder);
        assert!(document.claims.expires_at > now_seconds());
    }

    #[test]
    fn ordinary_admin_cannot_forge_founder_status_in_payload_or_state() {
        let (mut store, session) = admin_session(false);
        let policy = store
            .entitlement_policy(&session.account)
            .expect("ordinary policy");
        assert_ne!(policy.source, EntitlementSource::Founder);
        assert_ne!(policy.limits.get("team_members"), Some(&u64::MAX));
    }

    #[test]
    fn mfa_requires_valid_session_bound_totp_and_replay_is_denied() {
        let (mut store, session) = admin_session(true);
        let challenge = store
            .begin_memory_admin_mfa(&session.account, &session.access_token)
            .expect("challenge");
        assert!(store
            .verify_memory_admin_mfa(
                &session.account,
                &session.access_token,
                &challenge.challenge_id,
                "000000"
            )
            .is_err());
        let code = mfa_code(SECRET, now_seconds() as u64);
        let proof = store
            .verify_memory_admin_mfa(
                &session.account,
                &session.access_token,
                &challenge.challenge_id,
                &code,
            )
            .expect("valid TOTP");
        let input = grant_input(None, Some(session.account.organization.id.clone()), Some(3));
        store
            .create_memory_company_grant(
                &session.account,
                &session.access_token,
                &proof.proof,
                input.clone(),
                "mfa-1",
            )
            .expect("first proof use");
        assert!(store
            .create_memory_company_grant(
                &session.account,
                &session.access_token,
                &proof.proof,
                input,
                "mfa-replay",
            )
            .is_err());
    }

    #[test]
    fn complimentary_user_and_company_grants_are_durable_and_audited() {
        let (mut store, session) = admin_session(true);
        let target = store
            .dev_sign_in("target@example.test", "target")
            .expect("target session");
        let user_proof = proof(&mut store, &session);
        let user_grant = store
            .create_memory_user_grant(
                &session.account,
                &session.access_token,
                &user_proof,
                grant_input(Some(target.account.user.id.clone()), None, None),
                "grant-user",
            )
            .expect("user grant");
        let target_policy = store
            .entitlement_policy(&target.account)
            .expect("target grant policy");
        assert_eq!(target_policy.source, EntitlementSource::Complimentary);
        let company_proof = proof(&mut store, &session);
        let company_grant = store
            .create_memory_company_grant(
                &session.account,
                &session.access_token,
                &company_proof,
                grant_input(None, Some(session.account.organization.id.clone()), Some(5)),
                "grant-company",
            )
            .expect("company grant");
        let revoke_proof = proof(&mut store, &session);
        store
            .revoke_memory_grant(
                &session.account,
                &session.access_token,
                &revoke_proof,
                &user_grant.id,
                "revoke acceptance grant",
                "grant-revoke",
            )
            .expect("revoke grant");
        assert!(
            store
                .memory_audit_events(&session.account)
                .expect("audit events")
                .iter()
                .filter(|event| event.entity_type == "complimentary_grant")
                .count()
                >= 3
        );
        assert!(company_grant.seat_limit.is_some());
    }

    #[test]
    fn expired_trial_is_not_an_active_entitlement() {
        let mut store = MemoryStore::default();
        let session = store
            .dev_sign_in("trial@example.test", "trial")
            .expect("trial session");
        store.seed_trial_for_tests(&session.account, PlanTier::Pro, now_seconds() - 1, Some(2));
        assert!(store.entitlement_policy(&session.account).is_err());
    }

    #[test]
    fn seat_limit_is_server_authoritative_and_final_seat_is_deterministic() {
        let (mut store, session) = admin_session(true);
        let first = store
            .dev_sign_in("member-one@example.test", "member-one")
            .expect("first member");
        let second = store
            .dev_sign_in("member-two@example.test", "member-two")
            .expect("second member");
        let grant_proof = proof(&mut store, &session);
        let _grant = store
            .create_memory_company_grant(
                &session.account,
                &session.access_token,
                &grant_proof,
                grant_input(None, Some(session.account.organization.id.clone()), Some(2)),
                "seat-grant",
            )
            .expect("seat grant");
        let first_member_proof = proof(&mut store, &session);
        store
            .add_memory_member(
                &session.account,
                &session.access_token,
                &first_member_proof,
                &first.account.user.id,
                OrganizationRole::Member,
                "seat-one",
            )
            .expect("final available seat");
        let second_member_proof = proof(&mut store, &session);
        assert!(store
            .add_memory_member(
                &session.account,
                &session.access_token,
                &second_member_proof,
                &second.account.user.id,
                OrganizationRole::Member,
                "seat-two",
            )
            .is_err());
    }

    #[test]
    fn policy_is_tenant_scoped_versioned_and_cannot_override_sealed_authority() {
        let (mut store, session) = admin_session(true);
        let policy_proof = proof(&mut store, &session);
        let first = store
            .update_memory_team_policy(
                &session.account,
                &session.access_token,
                &policy_proof,
                0,
                json!({"shared_project": true}),
                "policy acceptance",
                "policy-1",
            )
            .expect("policy update");
        assert_eq!(first.version, 1);
        let stale_proof = proof(&mut store, &session);
        assert!(store
            .update_memory_team_policy(
                &session.account,
                &session.access_token,
                &stale_proof,
                0,
                json!({"shared_project": false}),
                "stale policy",
                "policy-stale",
            )
            .is_err());
        let forbidden_proof = proof(&mut store, &session);
        assert!(store
            .update_memory_team_policy(
                &session.account,
                &session.access_token,
                &forbidden_proof,
                1,
                json!({"p8": {"verified_complete": true}}),
                "forbidden override",
                "policy-forbidden",
            )
            .is_err());
    }

    #[test]
    fn cross_tenant_grant_is_denied_for_non_founder_admin() {
        let (mut store, session) = admin_session(false);
        let other = store
            .dev_sign_in("other-tenant@example.test", "other")
            .expect("other tenant");
        let cross_tenant_proof = proof(&mut store, &session);
        assert!(store
            .create_memory_user_grant(
                &session.account,
                &session.access_token,
                &cross_tenant_proof,
                grant_input(Some(other.account.user.id), None, None),
                "cross-tenant",
            )
            .is_err());
    }

    #[test]
    #[ignore = "requires RELINTOR_TEST_DATABASE_URL pointing to a disposable local PostgreSQL database"]
    fn mandatory_p10_postgres_authority_round_trip() {
        let database_url = std::env::var("RELINTOR_TEST_DATABASE_URL")
            .expect("RELINTOR_TEST_DATABASE_URL required");
        let mut store =
            PostgresStore::connect(&database_url).expect("connect disposable PostgreSQL");
        let email = format!("p10-{}@example.test", Uuid::new_v4());
        let session = store.dev_sign_in(&email, "p10-live").expect("live sign-in");
        store
            .client
            .execute(
                "INSERT INTO admin_identities(user_id,is_founder,is_admin,mfa_secret)
                 VALUES($1,TRUE,TRUE,$2)
                 ON CONFLICT(user_id) DO UPDATE SET is_founder=TRUE,is_admin=TRUE,mfa_secret=$2",
                &[
                    &super::super::parse_uuid(&session.account.user.id).unwrap(),
                    &SECRET.to_vec(),
                ],
            )
            .expect("seed disposable founder authority");
        let challenge = store
            .begin_admin_mfa(&session.account, &session.access_token)
            .expect("live MFA challenge");
        let proof = store
            .verify_admin_mfa(
                &session.account,
                &session.access_token,
                &challenge.challenge_id,
                &mfa_code(SECRET, now_seconds() as u64),
            )
            .expect("live MFA proof");
        let grant = store
            .create_complimentary_company_grant(
                &session.account,
                &session.access_token,
                &proof.proof,
                grant_input(None, Some(session.account.organization.id.clone()), Some(3)),
                "p10-live-grant",
            )
            .expect("live company grant");
        assert_eq!(grant.plan, PlanTier::Team);
        let policy = store
            .entitlement_policy(&session.account)
            .expect("live grant policy");
        assert_eq!(policy.source, EntitlementSource::Founder);
        assert_eq!(
            store
                .team_policy(&session.account)
                .expect("live team policy")
                .version,
            0
        );
        assert!(!store
            .audit_events(&session.account, None)
            .expect("live audit")
            .is_empty());
    }
}
