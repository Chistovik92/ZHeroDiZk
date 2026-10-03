-- Stage 0.5.1: session grants issued to operators. The signed token itself is never stored.
CREATE TABLE grants (
    id uuid PRIMARY KEY,
    org_id uuid NOT NULL,
    operator_id uuid NOT NULL,
    device_id uuid NOT NULL,
    capabilities text[] NOT NULL
        CHECK (cardinality(capabilities) > 0
               AND capabilities <@ ARRAY['view', 'input', 'file_transfer', 'clipboard']::text[]),
    mode text NOT NULL CHECK (mode IN ('attended', 'unattended')),
    issued_at timestamptz NOT NULL DEFAULT now(),
    not_before timestamptz NOT NULL,
    expires_at timestamptz NOT NULL CHECK (expires_at > not_before),
    revoked_at timestamptz,
    FOREIGN KEY (device_id, org_id) REFERENCES devices (id, org_id) ON DELETE CASCADE,
    FOREIGN KEY (org_id, operator_id) REFERENCES memberships (org_id, user_id) ON DELETE CASCADE
);
CREATE INDEX grants_device_idx ON grants (device_id, expires_at);
CREATE INDEX grants_operator_idx ON grants (operator_id, issued_at);
