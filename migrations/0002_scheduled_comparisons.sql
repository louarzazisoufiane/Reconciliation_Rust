CREATE EXTENSION IF NOT EXISTS pgcrypto;

CREATE TABLE scheduled (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name TEXT NOT NULL,
    frequency TEXT NOT NULL CHECK (frequency IN ('one_time', 'daily', 'weekly', 'monthly')),
    run_at TIMESTAMPTZ NOT NULL,
    old_path TEXT NOT NULL,
    new_path TEXT NOT NULL,
    old_layout_id UUID NOT NULL REFERENCES layouts(id),
    new_layout_id UUID NOT NULL REFERENCES layouts(id),
    archive_path TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'running', 'completed', 'failed')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_run_at TIMESTAMPTZ,
    error_message TEXT
);

CREATE INDEX scheduled_due_idx ON scheduled (run_at) WHERE status = 'pending';

CREATE TABLE scheduled_runs (
    id BIGSERIAL PRIMARY KEY,
    scheduled_id UUID NOT NULL REFERENCES scheduled(id) ON DELETE CASCADE,
    comparison_id UUID NOT NULL REFERENCES comparison_runs(id) ON DELETE CASCADE,
    old_filename TEXT NOT NULL,
    new_filename TEXT NOT NULL,
    processed_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (scheduled_id, old_filename, new_filename)
);
