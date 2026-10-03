-- Stage 0.4.4: organisations (tenants), memberships, enrolment tokens and devices.
CREATE TABLE organizations (
    id uuid PRIMARY KEY,
    name text NOT NULL CHECK (length(name) BETWEEN 1 AND 100),
    created_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE memberships (
    org_id uuid NOT NULL REFERENCES organizations (id) ON DELETE CASCADE,
    user_id uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    role text NOT NULL CHECK (role IN ('owner', 'admin', 'member')),
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (org_id, user_id)
);
CREATE INDEX memberships_user_idx ON memberships (user_id);

-- One-time tokens that let a device enrol into one organisation; only the digest is stored.
CREATE TABLE enrollment_tokens (
    id uuid PRIMARY KEY,
    org_id uuid NOT NULL REFERENCES organizations (id) ON DELETE CASCADE,
    token_hash bytea NOT NULL CHECK (length(token_hash) = 32),
    created_by uuid NOT NULL REFERENCES users (id),
    created_at timestamptz NOT NULL DEFAULT now(),
    expires_at timestamptz NOT NULL,
    used_at timestamptz
);
CREATE UNIQUE INDEX enrollment_tokens_hash_key ON enrollment_tokens (token_hash);

CREATE TABLE devices (
    id uuid PRIMARY KEY,
    org_id uuid NOT NULL REFERENCES organizations (id) ON DELETE CASCADE,
    name text NOT NULL CHECK (length(name) BETWEEN 1 AND 100),
    platform text NOT NULL CHECK (platform IN ('windows', 'linux', 'android', 'macos', 'ios')),
    public_key bytea NOT NULL CHECK (length(public_key) = 32),
    status text NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'revoked')),
    created_at timestamptz NOT NULL DEFAULT now(),
    revoked_at timestamptz
);
-- A device key can belong to exactly one device in the whole installation.
CREATE UNIQUE INDEX devices_public_key_key ON devices (public_key);
CREATE INDEX devices_org_idx ON devices (org_id);
