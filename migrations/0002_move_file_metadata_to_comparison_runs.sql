-- A source file's metadata describes the load, not an individual parsed row.
-- Keep one old and one new metadata record on the comparison run and remove
-- the duplicated values from the variable-layout row tables.
ALTER TABLE comparison_runs
    ADD COLUMN old_date_of_download DATE,
    ADD COLUMN old_origin_file_name TEXT,
    ADD COLUMN new_date_of_download DATE,
    ADD COLUMN new_origin_file_name TEXT;

UPDATE comparison_runs AS run
SET old_date_of_download = (
        SELECT date_of_download
        FROM old_rows
        WHERE comparison_id = run.id
        ORDER BY id
        LIMIT 1
    ),
    old_origin_file_name = (
        SELECT origin_file_name
        FROM old_rows
        WHERE comparison_id = run.id
        ORDER BY id
        LIMIT 1
    )
WHERE EXISTS (SELECT 1 FROM old_rows WHERE comparison_id = run.id);

UPDATE comparison_runs AS run
SET new_date_of_download = (
        SELECT date_of_download
        FROM new_rows
        WHERE comparison_id = run.id
        ORDER BY id
        LIMIT 1
    ),
    new_origin_file_name = (
        SELECT origin_file_name
        FROM new_rows
        WHERE comparison_id = run.id
        ORDER BY id
        LIMIT 1
    )
WHERE EXISTS (SELECT 1 FROM new_rows WHERE comparison_id = run.id);

ALTER TABLE old_rows
    DROP COLUMN date_of_download,
    DROP COLUMN origin_file_name;

ALTER TABLE new_rows
    DROP COLUMN date_of_download,
    DROP COLUMN origin_file_name;
