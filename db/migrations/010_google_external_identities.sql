-- Relintor closed-beta Google OIDC identity binding.
-- Additive: applied migrations 002, 003, and 009 remain unchanged.
BEGIN;

CREATE TABLE IF NOT EXISTS external_identities (
    id UUID PRIMARY KEY,
    provider TEXT NOT NULL CHECK (provider IN ('google')),
    provider_subject TEXT NOT NULL,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    verified_email TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE (provider, provider_subject)
);

CREATE INDEX IF NOT EXISTS external_identities_user_idx
    ON external_identities(user_id);

INSERT INTO cloud_schema_migrations (version, name)
VALUES (10, 'google_external_identities')
ON CONFLICT (version) DO NOTHING;

COMMIT;
