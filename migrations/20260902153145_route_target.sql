-- Add migration script here
CREATE TABLE route_target (
    path_prefix        TEXT PRIMARY KEY,
    upstream_base_url  TEXT NOT NULL,
    rate_limit_per_min  INTEGER NOT NULL CHECK (rate_limit_per_min >= 0),
    required_role       TEXT,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now()
);

