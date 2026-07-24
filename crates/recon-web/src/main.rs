mod assets;

use std::collections::HashSet;
use std::net::SocketAddr;
use std::str::FromStr;

use anyhow::Context;
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Multipart, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use sqlx::{
    FromRow, PgPool, Postgres, QueryBuilder,
    postgres::{PgConnectOptions, PgPoolOptions},
};
use tokio::sync::mpsc;
use tower_http::trace::TraceLayer;
use uuid::Uuid;
use xxhash_rust::xxh3::xxh3_64;

const MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("../../migrations");
const INSERT_BATCH_SIZE: usize = 5_000;
const PARSE_WORK_QUEUE_CAPACITY: usize = 2_048;
const PARSE_RESULT_QUEUE_CAPACITY: usize = 2_048;

type ParsedRow = (String, String, Value);

#[derive(Clone)]
struct AppState {
    pool: PgPool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    let database_url = std::env::var("DATABASE_URL").context(
        "DATABASE_URL is required (copy or use the supplied .env for local development)",
    )?;
    ensure_database_exists(&database_url).await?;
    let pool = PgPoolOptions::new()
        .max_connections(10)
        .connect(&database_url)
        .await?;
    MIGRATOR.run(&pool).await?;
    let state = AppState { pool };
    let api = Router::new()
        .route("/layouts", get(list_layouts).post(create_layout))
        .route(
            "/comparisons",
            get(list_comparisons).post(create_comparison),
        )
        .route("/comparisons/{id}/delta", get(list_delta))
        .with_state(state);
    // Multipart is streamed and rows are batched below; do not impose Axum's
    // small default request-body cap on large fixed-width source files.
    let app = Router::new()
        .nest("/api", api)
        .fallback(assets::static_handler)
        .layer(DefaultBodyLimit::disable())
        .layer(TraceLayer::new_for_http());
    let addr: SocketAddr = std::env::var("RECON_WEB_ADDR")
        .unwrap_or_else(|_| "127.0.0.1:3000".into())
        .parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!(%addr, "reconciliation web listening");
    axum::serve(listener, app).await?;
    Ok(())
}

async fn ensure_database_exists(url: &str) -> anyhow::Result<()> {
    let options = PgConnectOptions::from_str(url)?;
    let name = options.get_database().unwrap_or("postgres").to_owned();
    if name == "postgres" {
        return Ok(());
    }
    let admin_options = options.clone().database("postgres");
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect_with(admin_options)
        .await?;
    let exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_database WHERE datname = $1)")
            .bind(&name)
            .fetch_one(&pool)
            .await?;
    if !exists {
        let quoted = format!("\"{}\"", name.replace('"', "\"\""));
        sqlx::query(&format!("CREATE DATABASE {quoted}"))
            .execute(&pool)
            .await?;
        tracing::info!(database = %name, "created database");
    }
    Ok(())
}

#[derive(Debug)]
struct AppError(anyhow::Error);
impl<E: Into<anyhow::Error>> From<E> for AppError {
    fn from(e: E) -> Self {
        Self(e.into())
    }
}
impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        tracing::warn!(error = %self.0, "API request failed");
        (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": self.0.to_string()})),
        )
            .into_response()
    }
}
type ApiResult<T> = Result<Json<T>, AppError>;

#[derive(Clone, Debug, Serialize, Deserialize)]
struct LayoutField {
    name: String,
    start: usize,
    end: usize,
    is_primary_key: bool,
}
#[derive(Debug, Deserialize)]
struct CreateLayout {
    name: String,
    fields: Vec<LayoutField>,
}
#[derive(Debug, Serialize, FromRow)]
struct Layout {
    id: Uuid,
    name: String,
    fields: sqlx::types::Json<Vec<LayoutField>>,
}

fn validate_layout(fields: &[LayoutField]) -> anyhow::Result<()> {
    if fields.is_empty() {
        anyhow::bail!("a layout needs at least one field");
    }
    if !fields.iter().any(|field| field.is_primary_key) {
        anyhow::bail!("select at least one primary-key field");
    }
    let mut names = HashSet::new();
    for field in fields {
        if field.name.trim().is_empty() || field.start == 0 || field.end < field.start {
            anyhow::bail!("each field needs a name and valid 1-based start/end positions");
        }
        if !names.insert(field.name.trim().to_owned()) {
            anyhow::bail!("field names must be unique");
        }
    }
    Ok(())
}

async fn list_layouts(State(state): State<AppState>) -> ApiResult<Vec<Layout>> {
    Ok(Json(
        sqlx::query_as("SELECT id, name, fields FROM layouts ORDER BY name")
            .fetch_all(&state.pool)
            .await?,
    ))
}
async fn create_layout(
    State(state): State<AppState>,
    Json(mut request): Json<CreateLayout>,
) -> ApiResult<Layout> {
    request.name = request.name.trim().to_owned();
    validate_layout(&request.fields)?;
    if request.name.is_empty() {
        return Err(AppError(anyhow::anyhow!("layout name is required")));
    }
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO layouts (id, name, fields) VALUES ($1, $2, $3)")
        .bind(id)
        .bind(&request.name)
        .bind(sqlx::types::Json(&request.fields))
        .execute(&state.pool)
        .await?;
    Ok(Json(Layout {
        id,
        name: request.name,
        fields: sqlx::types::Json(request.fields),
    }))
}

#[derive(Serialize)]
struct ComparisonResponse {
    id: Uuid,
    old_rows: u64,
    new_rows: u64,
    added: i64,
    removed: i64,
    modified: i64,
}

#[derive(Serialize, FromRow)]
struct ComparisonHistoryRow {
    id: Uuid,
    run_index: i64,
    run_name: String,
    created_at: String,
    processing_duration_ms: Option<i64>,
    processing_started_at: Option<String>,
    processing_completed_at: Option<String>,
    old_layout_name: String,
    new_layout_name: String,
    old_date_of_download: Option<String>,
    new_date_of_download: Option<String>,
    old_origin_file_name: Option<String>,
    new_origin_file_name: Option<String>,
}

async fn list_comparisons(State(state): State<AppState>) -> ApiResult<Vec<ComparisonHistoryRow>> {
    Ok(Json(sqlx::query_as(
        "SELECT run.id, run.run_index, run.run_name, run.created_at::text AS created_at, run.processing_duration_ms, run.processing_started_at::text AS processing_started_at, run.processing_completed_at::text AS processing_completed_at, old_layout.name AS old_layout_name, new_layout.name AS new_layout_name, run.old_date_of_download::text AS old_date_of_download, run.new_date_of_download::text AS new_date_of_download, run.old_origin_file_name, run.new_origin_file_name FROM comparison_runs run JOIN layouts old_layout ON old_layout.id = run.old_layout_id JOIN layouts new_layout ON new_layout.id = run.new_layout_id ORDER BY run.created_at DESC",
    )
    .fetch_all(&state.pool)
    .await?))
}

async fn create_comparison(
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> ApiResult<ComparisonResponse> {
    let mut old_layout_id = None;
    let mut new_layout_id = None;
    let mut old_date = None;
    let mut new_date = None;
    let mut old_origin_file_name = None;
    let mut new_origin_file_name = None;
    let mut run_name = None;
    let mut processing_started_at = None;
    let mut comparison_id = None;
    let mut run_index = None;
    let mut old_rows = None;
    let mut new_rows = None;
    while let Some(field) = multipart.next_field().await? {
        let name = field.name().unwrap_or_default().to_owned();
        match name.as_str() {
            "old_layout_id" => old_layout_id = Some(parse_uuid(field.text().await?)?),
            "new_layout_id" => new_layout_id = Some(parse_uuid(field.text().await?)?),
            "old_date_of_download" => old_date = Some(field.text().await?),
            "new_date_of_download" => new_date = Some(field.text().await?),
            "old_origin_file_name" => old_origin_file_name = Some(field.text().await?),
            "new_origin_file_name" => new_origin_file_name = Some(field.text().await?),
            "run_name" => run_name = Some(field.text().await?),
            "processing_started_at" => processing_started_at = Some(field.text().await?),
            "old_file" | "new_file" => {
                let old_id = old_layout_id
                    .ok_or_else(|| anyhow::anyhow!("send layout selections before files"))?;
                let new_id = new_layout_id
                    .ok_or_else(|| anyhow::anyhow!("send layout selections before files"))?;
                let id = match comparison_id {
                    Some(id) => id,
                    None => {
                        let id = Uuid::new_v4();
                        let old_date = old_date.as_deref().ok_or_else(|| {
                            anyhow::anyhow!("send old download metadata before files")
                        })?;
                        let new_date = new_date.as_deref().ok_or_else(|| {
                            anyhow::anyhow!("send new download metadata before files")
                        })?;
                        let old_file_name = old_origin_file_name.as_deref().ok_or_else(|| {
                            anyhow::anyhow!("send old file metadata before files")
                        })?;
                        let new_file_name = new_origin_file_name.as_deref().ok_or_else(|| {
                            anyhow::anyhow!("send new file metadata before files")
                        })?;
                        let run_name = run_name
                            .as_deref()
                            .map(str::trim)
                            .filter(|name| !name.is_empty())
                            .ok_or_else(|| anyhow::anyhow!("run name is required"))?;
                        let processing_started_at = processing_started_at
                            .as_deref()
                            .ok_or_else(|| anyhow::anyhow!("processing start time is required"))?;
                        let index: i64 = sqlx::query_scalar("INSERT INTO comparison_runs (id, run_name, old_layout_id, new_layout_id, old_date_of_download, old_origin_file_name, new_date_of_download, new_origin_file_name, processing_started_at) VALUES ($1, $2, $3, $4, $5::timestamptz, $6, $7::timestamptz, $8, $9::timestamptz) RETURNING run_index")
                            .bind(id).bind(run_name).bind(old_id).bind(new_id).bind(old_date).bind(old_file_name).bind(new_date).bind(new_file_name).bind(processing_started_at)
                            .fetch_one(&state.pool).await?;
                        create_source_tables(&state.pool, index).await?;
                        comparison_id = Some(id);
                        run_index = Some(index);
                        id
                    }
                };
                let is_old = name == "old_file";
                let layout =
                    fetch_layout(&state.pool, if is_old { old_id } else { new_id }).await?;
                let index = run_index.ok_or_else(|| anyhow::anyhow!("run index is missing"))?;
                let table = source_table_name(is_old, index);
                let count = stream_load(&state.pool, field, &table, id, index, &layout).await?;
                if is_old {
                    old_rows = Some(count);
                } else {
                    new_rows = Some(count);
                }
            }
            _ => {}
        }
    }
    let old_layout_id = old_layout_id.ok_or_else(|| anyhow::anyhow!("old layout is required"))?;
    let new_layout_id = new_layout_id.ok_or_else(|| anyhow::anyhow!("new layout is required"))?;
    let _ = (old_layout_id, new_layout_id);
    let comparison_id = comparison_id.ok_or_else(|| anyhow::anyhow!("both files are required"))?;
    let run_index = run_index.ok_or_else(|| anyhow::anyhow!("run index is required"))?;
    let old_rows = old_rows.ok_or_else(|| anyhow::anyhow!("old file is required"))?;
    let new_rows = new_rows.ok_or_else(|| anyhow::anyhow!("new file is required"))?;
    compute_delta(&state.pool, comparison_id, run_index).await?;
    sqlx::query("UPDATE comparison_runs SET processing_completed_at = now(), processing_duration_ms = GREATEST(0, (EXTRACT(EPOCH FROM (now() - processing_started_at)) * 1000)::BIGINT) WHERE id = $1")
        .bind(comparison_id)
        .execute(&state.pool)
        .await?;
    let counts = sqlx::query_as::<_, DeltaCount>("SELECT count(*) FILTER (WHERE change_type = 'added') AS added, count(*) FILTER (WHERE change_type = 'removed') AS removed, count(*) FILTER (WHERE change_type = 'modified') AS modified FROM delta_rows WHERE comparison_id = $1").bind(comparison_id).fetch_one(&state.pool).await?;
    let response = ComparisonResponse {
        id: comparison_id,
        old_rows,
        new_rows,
        added: counts.added.unwrap_or(0),
        removed: counts.removed.unwrap_or(0),
        modified: counts.modified.unwrap_or(0),
    };
    Ok(Json(response))
}

fn parse_uuid(value: String) -> anyhow::Result<Uuid> {
    Ok(Uuid::parse_str(&value).map_err(|_| anyhow::anyhow!("invalid layout id"))?)
}
async fn fetch_layout(pool: &PgPool, id: Uuid) -> anyhow::Result<Vec<LayoutField>> {
    let fields: sqlx::types::Json<Vec<LayoutField>> =
        sqlx::query_scalar("SELECT fields FROM layouts WHERE id = $1")
            .bind(id)
            .fetch_optional(pool)
            .await?
            .ok_or_else(|| anyhow::anyhow!("layout not found"))?;
    validate_layout(&fields.0)?;
    Ok(fields.0)
}

fn source_table_name(is_old: bool, run_index: i64) -> String {
    let prefix = if is_old { "old_rows" } else { "new_rows" };
    format!("{prefix}_{run_index}")
}

async fn create_source_tables(pool: &PgPool, run_index: i64) -> anyhow::Result<()> {
    for is_old in [true, false] {
        let table = source_table_name(is_old, run_index);
        let composite_key_index = format!("{table}_composite_key_idx");
        sqlx::query(&format!(
            "CREATE TABLE {table} (id BIGSERIAL PRIMARY KEY, comparison_id UUID NOT NULL REFERENCES comparison_runs(id) ON DELETE CASCADE, run_index BIGINT NOT NULL CHECK (run_index = {run_index}), composite_primary_key TEXT NOT NULL, row_hash CHAR(16) NOT NULL, data JSONB NOT NULL)"
        ))
        .execute(pool)
        .await?;
        sqlx::query(&format!(
            "CREATE UNIQUE INDEX {composite_key_index} ON {table} (composite_primary_key)"
        ))
        .execute(pool)
        .await?;
    }
    Ok(())
}

async fn stream_load(
    pool: &PgPool,
    mut file: axum::extract::multipart::Field<'_>,
    table: &str,
    comparison_id: Uuid,
    run_index: i64,
    fields: &[LayoutField],
) -> anyhow::Result<u64> {
    let worker_count = parsing_worker_count();
    let per_worker_queue_capacity = (PARSE_WORK_QUEUE_CAPACITY / worker_count).max(1);
    let (row_sender, mut row_receiver) = mpsc::channel(PARSE_RESULT_QUEUE_CAPACITY);
    let mut line_senders = Vec::with_capacity(worker_count);
    let mut workers = Vec::with_capacity(worker_count);

    for _ in 0..worker_count {
        let (line_sender, mut line_receiver) = mpsc::channel::<Vec<u8>>(per_worker_queue_capacity);
        line_senders.push(line_sender);
        let row_sender = row_sender.clone();
        let fields = fields.to_vec();
        workers.push(tokio::task::spawn_blocking(move || {
            loop {
                let line = line_receiver.blocking_recv();
                let Some(line) = line else {
                    break;
                };
                if row_sender.blocking_send(parse_row(&line, &fields)).is_err() {
                    break;
                }
            }
        }));
    }
    drop(row_sender);

    let writer_pool = pool.clone();
    let writer_table = table.to_owned();
    let writer = tokio::spawn(async move {
        let mut batch = Vec::with_capacity(INSERT_BATCH_SIZE);
        let mut count = 0;
        let mut parse_error = None;
        while let Some(parsed) = row_receiver.recv().await {
            match parsed {
                Ok(Some(row)) if parse_error.is_none() => {
                    batch.push(row);
                    count += 1;
                    if batch.len() == INSERT_BATCH_SIZE {
                        insert_batch(
                            &writer_pool,
                            &writer_table,
                            comparison_id,
                            run_index,
                            &batch,
                        )
                        .await?;
                        batch.clear();
                    }
                }
                Ok(_) => {}
                Err(error) => {
                    // Drain the pipeline before returning so bounded channels
                    // cannot leave workers or the multipart reader blocked.
                    parse_error.get_or_insert(error);
                }
            }
        }
        if let Some(error) = parse_error {
            return Err(error);
        }
        if !batch.is_empty() {
            insert_batch(
                &writer_pool,
                &writer_table,
                comparison_id,
                run_index,
                &batch,
            )
            .await?;
        }
        Ok::<u64, anyhow::Error>(count)
    });

    let mut pending = Vec::new();
    let mut next_worker = 0;
    let read_result = async {
        while let Some(chunk) = file.chunk().await? {
            pending.extend_from_slice(&chunk);
            while let Some(newline) = pending.iter().position(|byte| *byte == b'\n') {
                let mut line: Vec<u8> = pending.drain(..=newline).collect();
                line.pop(); // newline found above
                if line.last() == Some(&b'\r') {
                    line.pop();
                }
                line_senders[next_worker]
                    .send(line)
                    .await
                    .map_err(|_| anyhow::anyhow!("parsing workers stopped unexpectedly"))?;
                next_worker = (next_worker + 1) % worker_count;
            }
        }
        if !pending.is_empty() {
            line_senders[next_worker]
                .send(pending)
                .await
                .map_err(|_| anyhow::anyhow!("parsing workers stopped unexpectedly"))?;
        }
        Ok::<(), anyhow::Error>(())
    }
    .await;
    drop(line_senders);

    for worker in workers {
        worker.await.context("parsing worker panicked")?;
    }
    let write_result = writer.await.context("database writer task panicked")?;
    read_result?;
    write_result
}

fn parsing_worker_count() -> usize {
    let available = std::thread::available_parallelism()
        .map(|parallelism| parallelism.get())
        .unwrap_or(1);
    parsing_worker_count_for(available)
}

fn parsing_worker_count_for(available: usize) -> usize {
    (available.saturating_mul(7) / 10).max(1)
}

fn parse_row(line: &[u8], fields: &[LayoutField]) -> anyhow::Result<Option<ParsedRow>> {
    if line.is_empty() {
        return Ok(None);
    }
    let mut data = Map::new();
    let mut key_parts = Vec::new();
    for field in fields {
        let start = field.start - 1;
        let end = field.end;
        let bytes = line.get(start..end).ok_or_else(|| {
            anyhow::anyhow!(
                "line is shorter than '{}' ending at position {}",
                field.name,
                field.end
            )
        })?;
        let value = std::str::from_utf8(bytes)
            .map_err(|_| anyhow::anyhow!("fixed-width input must be UTF-8"))?
            .trim()
            .to_owned();
        if field.is_primary_key {
            key_parts.push(value.clone());
        }
        data.insert(field.name.clone(), Value::String(value));
    }
    let composite_key = key_parts.join("\u{1f}");
    let data = Value::Object(data);
    let canonical_json = serde_json::to_vec(&data)?;
    let row_hash = format!("{:016x}", xxh3_64(&canonical_json));
    Ok(Some((composite_key, row_hash, data)))
}

async fn insert_batch(
    pool: &PgPool,
    table: &str,
    comparison_id: Uuid,
    run_index: i64,
    rows: &[(String, String, Value)],
) -> anyhow::Result<()> {
    let mut builder = QueryBuilder::<Postgres>::new(format!(
        "INSERT INTO {table} (comparison_id, run_index, composite_primary_key, row_hash, data) "
    ));
    builder.push_values(rows, |mut row, (key, row_hash, data)| {
        row.push_bind(comparison_id)
            .push_bind(run_index)
            .push_bind(key)
            .push_bind(row_hash)
            .push_bind(sqlx::types::Json(data));
    });
    builder.build().execute(pool).await?;
    Ok(())
}

async fn compute_delta(pool: &PgPool, id: Uuid, run_index: i64) -> anyhow::Result<()> {
    let old_table = source_table_name(true, run_index);
    let new_table = source_table_name(false, run_index);
    sqlx::query(&format!("WITH changed AS MATERIALIZED (SELECT o.composite_primary_key, o.data AS old_data, n.data AS new_data FROM {old_table} o JOIN {new_table} n ON n.composite_primary_key = o.composite_primary_key WHERE o.comparison_id = $1 AND o.row_hash <> n.row_hash AND o.data IS DISTINCT FROM n.data) INSERT INTO delta_rows (comparison_id, composite_primary_key, change_type, old_data, new_data, changed_fields) SELECT $1, changed.composite_primary_key, 'modified', changed.old_data, changed.new_data, COALESCE(jsonb_object_agg(k, jsonb_build_object('old', changed.old_data -> k, 'new', changed.new_data -> k)) FILTER (WHERE (changed.old_data -> k) IS DISTINCT FROM (changed.new_data -> k)), '{{}}'::jsonb) FROM changed CROSS JOIN LATERAL jsonb_object_keys(changed.old_data || changed.new_data) AS k GROUP BY changed.composite_primary_key, changed.old_data, changed.new_data")).bind(id).execute(pool).await?;
    sqlx::query(&format!("INSERT INTO delta_rows (comparison_id, composite_primary_key, change_type, old_data, changed_fields) SELECT $1, o.composite_primary_key, 'removed', o.data, '{{}}'::jsonb FROM {old_table} o LEFT JOIN {new_table} n ON n.composite_primary_key = o.composite_primary_key WHERE o.comparison_id = $1 AND n.id IS NULL")).bind(id).execute(pool).await?;
    sqlx::query(&format!("INSERT INTO delta_rows (comparison_id, composite_primary_key, change_type, new_data, changed_fields) SELECT $1, n.composite_primary_key, 'added', n.data, '{{}}'::jsonb FROM {new_table} n LEFT JOIN {old_table} o ON o.composite_primary_key = n.composite_primary_key WHERE n.comparison_id = $1 AND o.id IS NULL")).bind(id).execute(pool).await?;
    Ok(())
}

#[derive(FromRow)]
struct DeltaCount {
    added: Option<i64>,
    removed: Option<i64>,
    modified: Option<i64>,
}
#[derive(Serialize, FromRow)]
struct DeltaRow {
    composite_primary_key: String,
    change_type: String,
    old_data: Option<Value>,
    new_data: Option<Value>,
    changed_fields: Value,
}
async fn list_delta(
    State(state): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<Uuid>,
) -> ApiResult<Vec<DeltaRow>> {
    Ok(Json(sqlx::query_as("SELECT composite_primary_key, change_type, old_data, new_data, changed_fields FROM delta_rows WHERE comparison_id = $1 ORDER BY id").bind(id).fetch_all(&state.pool).await?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn composite_key_uses_key_fields_in_layout_order() {
        let fields = vec![
            LayoutField {
                name: "branch".into(),
                start: 1,
                end: 2,
                is_primary_key: true,
            },
            LayoutField {
                name: "name".into(),
                start: 3,
                end: 7,
                is_primary_key: false,
            },
            LayoutField {
                name: "account".into(),
                start: 8,
                end: 10,
                is_primary_key: true,
            },
        ];
        let row = parse_row(b"01ALICE123", &fields).unwrap().unwrap();
        assert_eq!(row.0, "01\u{1f}123");
        assert_eq!(row.2["name"], "ALICE");
        assert_eq!(row.1.len(), 16);
    }

    #[test]
    fn row_fingerprint_is_stable_for_identical_parsed_data() {
        let fields = vec![LayoutField {
            name: "account".into(),
            start: 1,
            end: 3,
            is_primary_key: true,
        }];
        let first = parse_row(b"123", &fields).unwrap().unwrap();
        let second = parse_row(b"123", &fields).unwrap().unwrap();
        assert_eq!(first.1, second.1);
    }

    #[test]
    fn parsing_workers_are_capped_at_seventy_percent_of_available_threads() {
        assert_eq!(parsing_worker_count_for(10), 7);
        assert_eq!(parsing_worker_count_for(4), 2);
        assert_eq!(parsing_worker_count_for(1), 1);
    }

    #[test]
    fn layouts_require_at_least_one_primary_key_field() {
        let fields = vec![LayoutField {
            name: "value".into(),
            start: 1,
            end: 2,
            is_primary_key: false,
        }];
        assert!(validate_layout(&fields).is_err());
    }
}
