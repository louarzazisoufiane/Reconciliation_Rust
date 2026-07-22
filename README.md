# Fixed-width reconciliation

This application compares two user-uploaded fixed-width files through the embedded React UI.

## Local development

```bash
# Set DATABASE_URL in .env to your locally installed PostgreSQL instance.
cargo run -p recon-web
```

The server loads `.env`, creates the target database when it does not exist, and runs the SQL migrations on startup. Visit `http://127.0.0.1:3000`.

## Storage strategy

`old_rows`, `new_rows`, and `delta_rows` are stable PostgreSQL tables. Every parsed record stores its variable layout fields in a JSONB `data` payload, while its row ID, comparison ID, and composite primary key are normal typed columns with indexes. Uploads are parsed incrementally and written in bounded multi-row insert batches. Download timestamp and source filename are load-level metadata held once on `comparison_runs`. This supports a different layout for every load without generating or migrating arbitrary database columns.

The composite key uses all layout fields marked as primary-key fields in their layout order. The delta records `added`, `removed`, and `modified` records; modified rows retain both complete payloads plus a per-field `{ old, new }` JSON object for only changed values.
