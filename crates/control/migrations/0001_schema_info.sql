-- Stage 0.4.1: the first migration only records the schema generation.
CREATE TABLE schema_info (
    id integer PRIMARY KEY CHECK (id = 1),
    created_at timestamptz NOT NULL DEFAULT now()
);
INSERT INTO schema_info (id) VALUES (1);
