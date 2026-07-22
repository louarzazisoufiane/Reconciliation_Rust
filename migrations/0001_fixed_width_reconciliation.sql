CREATE TABLE layouts (
    id UUID PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    fields JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE comparison_runs (
    id UUID PRIMARY KEY,
    old_layout_id UUID NOT NULL REFERENCES layouts(id),
    new_layout_id UUID NOT NULL REFERENCES layouts(id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Stable tables retain variable user-defined fields in JSONB, preventing DDL
-- churn while keys and load metadata remain typed and indexable.
CREATE TABLE old_rows (
    id BIGSERIAL PRIMARY KEY,
    comparison_id UUID NOT NULL REFERENCES comparison_runs(id) ON DELETE CASCADE,
    date_of_download DATE NOT NULL,
    origin_file_name TEXT NOT NULL,
    composite_primary_key TEXT NOT NULL,
    data JSONB NOT NULL
);
CREATE UNIQUE INDEX old_rows_comparison_key_idx ON old_rows (comparison_id, composite_primary_key);

CREATE TABLE new_rows (
    id BIGSERIAL PRIMARY KEY,
    comparison_id UUID NOT NULL REFERENCES comparison_runs(id) ON DELETE CASCADE,
    date_of_download DATE NOT NULL,
    origin_file_name TEXT NOT NULL,
    composite_primary_key TEXT NOT NULL,
    data JSONB NOT NULL
);
CREATE UNIQUE INDEX new_rows_comparison_key_idx ON new_rows (comparison_id, composite_primary_key);

CREATE TABLE delta_rows (
    id BIGSERIAL PRIMARY KEY,
    comparison_id UUID NOT NULL REFERENCES comparison_runs(id) ON DELETE CASCADE,
    composite_primary_key TEXT NOT NULL,
    change_type TEXT NOT NULL CHECK (change_type IN ('modified', 'added', 'removed')),
    old_data JSONB,
    new_data JSONB,
    changed_fields JSONB NOT NULL DEFAULT '{}'::jsonb
);
CREATE INDEX delta_rows_comparison_type_idx ON delta_rows (comparison_id, change_type);
