CREATE TABLE api_usage_event (
    id BIGSERIAL PRIMARY KEY,
    client_id TEXT NOT NULL,
    target_id TEXT NOT NULL,
    route_prefix TEXT NOT NULL,
    method TEXT NOT NULL,
    status TEXT NOT NULL CHECK (
        status IN ('accepted', 'success', 'rejected', 'rate_limited', 'upstream_error')
    ),
    response_code INTEGER,
    upstream_host TEXT,
    occurred_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_api_usage_event_client_target_time
    ON api_usage_event (client_id, target_id, occurred_at DESC);

CREATE INDEX idx_api_usage_event_target_time
    ON api_usage_event (target_id, occurred_at DESC);

CREATE INDEX idx_api_usage_event_occurred_at
    ON api_usage_event (occurred_at DESC);