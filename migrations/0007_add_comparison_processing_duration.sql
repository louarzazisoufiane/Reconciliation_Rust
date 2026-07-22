ALTER TABLE comparison_runs
    ADD COLUMN processing_started_at TIMESTAMPTZ,
    ADD COLUMN processing_completed_at TIMESTAMPTZ,
    ADD COLUMN processing_duration_ms BIGINT;
