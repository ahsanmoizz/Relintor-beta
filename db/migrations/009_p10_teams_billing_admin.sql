-- Relintor P10: durable organizations, team policy, entitlement administration,
-- MFA challenges, and security audit records. This migration is additive.
BEGIN;

ALTER TABLE organizations
    ADD COLUMN IF NOT EXISTS status TEXT NOT NULL DEFAULT 'active'
        CHECK (status IN ('active', 'suspended', 'archived'));

ALTER TABLE organization_memberships
    ADD COLUMN IF NOT EXISTS updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP;

ALTER TABLE complimentary_grants
    ADD COLUMN IF NOT EXISTS revoked_at TIMESTAMPTZ;

ALTER TABLE complimentary_grants
    ADD COLUMN IF NOT EXISTS updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP;

CREATE TABLE IF NOT EXISTS team_policies (
    organization_id UUID PRIMARY KEY REFERENCES organizations(id) ON DELETE CASCADE,
    version BIGINT NOT NULL DEFAULT 1 CHECK (version > 0),
    policy JSONB NOT NULL DEFAULT '{}'::jsonb,
    updated_by UUID NOT NULL REFERENCES users(id),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS subscriptions (
    id UUID PRIMARY KEY,
    user_id UUID REFERENCES users(id) ON DELETE CASCADE,
    organization_id UUID REFERENCES organizations(id) ON DELETE CASCADE,
    plan_id TEXT NOT NULL REFERENCES plans(id),
    billing_interval TEXT NOT NULL CHECK (billing_interval IN ('monthly', 'annual')),
    status TEXT NOT NULL CHECK (status IN ('active', 'trial', 'canceled', 'expired')),
    starts_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    trial_ends_at TIMESTAMPTZ,
    current_period_end TIMESTAMPTZ,
    seat_limit BIGINT CHECK (seat_limit IS NULL OR seat_limit >= 0),
    provider TEXT,
    provider_subscription_id TEXT,
    last_event_id TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CHECK (user_id IS NOT NULL OR organization_id IS NOT NULL),
    CHECK (trial_ends_at IS NULL OR trial_ends_at >= starts_at)
);

CREATE UNIQUE INDEX IF NOT EXISTS subscriptions_provider_event_unique
    ON subscriptions(provider, last_event_id)
    WHERE provider IS NOT NULL AND last_event_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS subscriptions_org_status_idx
    ON subscriptions(organization_id, status, starts_at);

CREATE TABLE IF NOT EXISTS admin_identities (
    user_id UUID PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    is_founder BOOLEAN NOT NULL DEFAULT FALSE,
    is_admin BOOLEAN NOT NULL DEFAULT FALSE,
    mfa_secret BYTEA NOT NULL,
    mfa_enabled BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CHECK (is_founder OR is_admin)
);

CREATE TABLE IF NOT EXISTS admin_mfa_challenges (
    id UUID PRIMARY KEY,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    session_hash TEXT NOT NULL,
    nonce_hash TEXT NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL,
    used_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS admin_mfa_proofs (
    id UUID PRIMARY KEY,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    session_hash TEXT NOT NULL,
    proof_hash TEXT NOT NULL UNIQUE,
    expires_at TIMESTAMPTZ NOT NULL,
    used_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS p10_admin_audit_events (
    id UUID PRIMARY KEY,
    actor_user_id UUID REFERENCES users(id) ON DELETE SET NULL,
    actor_role TEXT NOT NULL,
    organization_id UUID REFERENCES organizations(id) ON DELETE SET NULL,
    target_user_id UUID REFERENCES users(id) ON DELETE SET NULL,
    entity_type TEXT NOT NULL,
    entity_id TEXT NOT NULL,
    operation TEXT NOT NULL,
    reason TEXT NOT NULL,
    before_state JSONB,
    after_state JSONB,
    outcome TEXT NOT NULL CHECK (outcome IN ('success', 'denied')),
    request_id TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS p10_admin_audit_scope_idx
    ON p10_admin_audit_events(organization_id, created_at);

CREATE INDEX IF NOT EXISTS p10_admin_audit_actor_idx
    ON p10_admin_audit_events(actor_user_id, created_at);

INSERT INTO cloud_schema_migrations (version, name)
VALUES (9, 'p10_teams_billing_admin')
ON CONFLICT (version) DO NOTHING;

COMMIT;
