ALTER TABLE comparison_runs ADD COLUMN run_name TEXT;

UPDATE comparison_runs
SET run_name = 'Comparison ' || to_char(created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD HH24:MI:SS UTC')
WHERE run_name IS NULL;

ALTER TABLE comparison_runs
    ALTER COLUMN run_name SET NOT NULL;
