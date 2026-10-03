-- Stage 0.4.6: append-only audit log.
-- No foreign keys on purpose: entries must outlive the users and organisations they mention.
CREATE TABLE audit_events (
    id bigserial PRIMARY KEY,
    org_id uuid,
    actor_user_id uuid,
    action text NOT NULL CHECK (length(action) BETWEEN 1 AND 64),
    target text CHECK (length(target) <= 200),
    detail jsonb NOT NULL DEFAULT '{}'::jsonb,
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX audit_events_org_idx ON audit_events (org_id, id DESC);

CREATE FUNCTION audit_events_immutable() RETURNS trigger AS $$
BEGIN
    RAISE EXCEPTION 'audit_events is append-only';
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER audit_events_no_update
    BEFORE UPDATE OR DELETE ON audit_events
    FOR EACH ROW EXECUTE FUNCTION audit_events_immutable();

CREATE TRIGGER audit_events_no_truncate
    BEFORE TRUNCATE ON audit_events
    FOR EACH STATEMENT EXECUTE FUNCTION audit_events_immutable();
