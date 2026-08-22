-- Relintor Milestone 2 independent-audit reconciliation.
-- This migration is additive because 002 may already have been applied by a
-- development environment. Do not rewrite migration history to repair it.
BEGIN;

ALTER TABLE sessions
    ADD COLUMN IF NOT EXISTS organization_id UUID REFERENCES organizations(id) ON DELETE CASCADE;

ALTER TABLE sessions
    ADD COLUMN IF NOT EXISTS access_token_hash TEXT;

ALTER TABLE sessions
    ADD COLUMN IF NOT EXISTS access_expires_at TIMESTAMPTZ;

CREATE UNIQUE INDEX IF NOT EXISTS sessions_access_token_hash_unique
    ON sessions(access_token_hash)
    WHERE access_token_hash IS NOT NULL;

CREATE TABLE IF NOT EXISTS usage_buckets (
    id UUID PRIMARY KEY,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    organization_id UUID REFERENCES organizations(id) ON DELETE CASCADE,
    metric TEXT NOT NULL,
    period_start TIMESTAMPTZ NOT NULL,
    period_end TIMESTAMPTZ NOT NULL,
    limit_value BIGINT NOT NULL CHECK (limit_value >= 0),
    used_value BIGINT NOT NULL DEFAULT 0 CHECK (used_value >= 0),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CHECK (period_end > period_start),
    CHECK (used_value <= limit_value),
    UNIQUE (user_id, organization_id, metric, period_start, period_end)
);

CREATE TABLE IF NOT EXISTS complimentary_grants (
    id UUID PRIMARY KEY,
    user_id UUID REFERENCES users(id) ON DELETE CASCADE,
    organization_id UUID REFERENCES organizations(id) ON DELETE CASCADE,
    reason TEXT NOT NULL,
    granted_by UUID REFERENCES users(id) ON DELETE SET NULL,
    starts_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    expires_at TIMESTAMPTZ,
    plan_template TEXT NOT NULL REFERENCES plans(id),
    seat_limit BIGINT CHECK (seat_limit IS NULL OR seat_limit >= 0),
    ai_budget_override BIGINT CHECK (ai_budget_override IS NULL OR ai_budget_override >= 0),
    notes TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CHECK (user_id IS NOT NULL OR organization_id IS NOT NULL),
    CHECK (expires_at IS NULL OR expires_at > starts_at)
);

CREATE INDEX IF NOT EXISTS complimentary_grants_org_idx
    ON complimentary_grants(organization_id, starts_at, expires_at);

CREATE INDEX IF NOT EXISTS complimentary_grants_user_idx
    ON complimentary_grants(user_id, starts_at, expires_at);

CREATE TABLE IF NOT EXISTS admin_actions (
    id UUID PRIMARY KEY,
    actor_user_id UUID REFERENCES users(id) ON DELETE SET NULL,
    target_user_id UUID REFERENCES users(id) ON DELETE SET NULL,
    target_organization_id UUID REFERENCES organizations(id) ON DELETE SET NULL,
    action_type TEXT NOT NULL,
    reason TEXT NOT NULL,
    old_value JSONB,
    new_value JSONB,
    request_id TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);

INSERT INTO cloud_schema_migrations (version, name)
VALUES (3, 'milestone2_reconciliation')
ON CONFLICT (version) DO NOTHING;

COMMIT;
