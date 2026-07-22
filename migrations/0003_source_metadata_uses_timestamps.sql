-- Preserve existing date-only metadata at midnight UTC while allowing future
-- source loads to retain the hour, minute, and second of download/upload.
ALTER TABLE comparison_runs
    ALTER COLUMN old_date_of_download TYPE TIMESTAMPTZ
        USING old_date_of_download::timestamp AT TIME ZONE 'UTC',
    ALTER COLUMN new_date_of_download TYPE TIMESTAMPTZ
        USING new_date_of_download::timestamp AT TIME ZONE 'UTC';
