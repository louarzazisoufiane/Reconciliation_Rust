-- COPY lands here first. These tables deliberately have no indexes or foreign
-- keys, so PostgreSQL can bulk-load a source file before validation and
-- promotion into the durable, indexed old/new tables.
CREATE UNLOGGED TABLE old_staging_rows (
    comparison_id UUID NOT NULL,
    composite_primary_key TEXT NOT NULL,
    data JSONB NOT NULL
);

CREATE UNLOGGED TABLE new_staging_rows (
    comparison_id UUID NOT NULL,
    composite_primary_key TEXT NOT NULL,
    data JSONB NOT NULL
);
