-- Stage 0.4.3: TOTP multi-factor authentication.
ALTER TABLE users
    ADD COLUMN mfa_secret_enc bytea,
    ADD COLUMN mfa_enabled boolean NOT NULL DEFAULT false,
    ADD COLUMN mfa_last_step bigint NOT NULL DEFAULT 0;

-- A password-only login of an MFA user yields a short-lived session of kind 'mfa_pending',
-- which is accepted only by the second login step.
ALTER TABLE sessions
    ADD COLUMN kind text NOT NULL DEFAULT 'full' CHECK (kind IN ('full', 'mfa_pending'));

CREATE TABLE recovery_codes (
    id uuid PRIMARY KEY,
    user_id uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    code_hash bytea NOT NULL CHECK (length(code_hash) = 32),
    used_at timestamptz
);
CREATE UNIQUE INDEX recovery_codes_hash_key ON recovery_codes (user_id, code_hash);
