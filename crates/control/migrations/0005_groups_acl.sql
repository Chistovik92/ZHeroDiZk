-- Stage 0.4.5: device groups, access rules (ACL) and personal address books.
-- Composite foreign keys (id, org_id) make the database itself refuse cross-tenant links.
ALTER TABLE devices ADD CONSTRAINT devices_id_org_key UNIQUE (id, org_id);

CREATE TABLE device_groups (
    id uuid PRIMARY KEY,
    org_id uuid NOT NULL REFERENCES organizations (id) ON DELETE CASCADE,
    name text NOT NULL CHECK (length(name) BETWEEN 1 AND 100),
    created_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT device_groups_id_org_key UNIQUE (id, org_id),
    CONSTRAINT device_groups_name_key UNIQUE (org_id, name)
);

CREATE TABLE device_group_members (
    group_id uuid NOT NULL,
    device_id uuid NOT NULL,
    org_id uuid NOT NULL,
    PRIMARY KEY (group_id, device_id),
    FOREIGN KEY (group_id, org_id) REFERENCES device_groups (id, org_id) ON DELETE CASCADE,
    FOREIGN KEY (device_id, org_id) REFERENCES devices (id, org_id) ON DELETE CASCADE
);

-- "user may use these capabilities on every device of the group"; nothing is allowed by default.
CREATE TABLE acl_rules (
    org_id uuid NOT NULL,
    user_id uuid NOT NULL,
    group_id uuid NOT NULL,
    capabilities text[] NOT NULL
        CHECK (cardinality(capabilities) > 0
               AND capabilities <@ ARRAY['view', 'input', 'file_transfer', 'clipboard']::text[]),
    updated_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, group_id),
    FOREIGN KEY (group_id, org_id) REFERENCES device_groups (id, org_id) ON DELETE CASCADE,
    FOREIGN KEY (org_id, user_id) REFERENCES memberships (org_id, user_id) ON DELETE CASCADE
);

CREATE TABLE address_book_entries (
    user_id uuid NOT NULL,
    device_id uuid NOT NULL,
    org_id uuid NOT NULL,
    alias text NOT NULL CHECK (length(alias) BETWEEN 1 AND 100),
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, device_id),
    FOREIGN KEY (org_id, user_id) REFERENCES memberships (org_id, user_id) ON DELETE CASCADE,
    FOREIGN KEY (device_id, org_id) REFERENCES devices (id, org_id) ON DELETE CASCADE
);
