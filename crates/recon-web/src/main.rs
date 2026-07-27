mod assets;

use std::net::SocketAddr;
use std::str::FromStr;
use std::{
    collections::HashSet,
    path::{Path as FsPath, PathBuf},
};

use anyhow::Context;
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Multipart, Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use sqlx::{
    FromRow, PgPool, Postgres, QueryBuilder,
    postgres::{PgConnectOptions, PgPoolOptions},
};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    sync::mpsc,
    time::{self, Duration},
};
use tower_http::trace::TraceLayer;
use uuid::Uuid;
use xxhash_rust::xxh3::xxh3_64;

const MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("../../migrations");
const INSERT_BATCH_SIZE: usize = 5_000;
const PARSE_WORK_QUEUE_CAPACITY: usize = 2_048;
const PARSE_RESULT_QUEUE_CAPACITY: usize = 2_048;

// The JSON payload is serialized once while parsing. Its bytes are both the
// row-hash input and the value bound to PostgreSQL as JSONB during insertion.
type ParsedRow = (String, String, String);

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
    tokio::spawn(scheduled_worker(state.clone()));
    let api = Router::new()
        .route("/layouts", get(list_layouts).post(create_layout))
        .route(
            "/comparisons",
            get(list_comparisons).post(create_comparison),
        )
        .route("/comparisons/{id}/delta", get(list_delta))
        .route("/scheduled", get(list_scheduled).post(create_scheduled))
        .route(
            "/scheduled/{id}",
            get(get_scheduled)
                .put(update_scheduled)
                .delete(delete_scheduled),
        )
        .route("/scheduled/{id}/run-now", post(run_scheduled_now))
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
    // The run's start time is also its creation time: a run is created only
    // when processing begins, so no redundant database column is needed.
    created_at: String,
    processing_duration_ms: Option<i64>,
    processing_started_at: Option<String>,
    old_layout_name: String,
    new_layout_name: String,
    old_date_of_download: Option<String>,
    new_date_of_download: Option<String>,
    old_origin_file_name: Option<String>,
    new_origin_file_name: Option<String>,
}

async fn list_comparisons(State(state): State<AppState>) -> ApiResult<Vec<ComparisonHistoryRow>> {
    Ok(Json(sqlx::query_as(
        "SELECT run.id, run.run_index, run.run_name, run.processing_started_at::text AS created_at, run.processing_duration_ms, run.processing_started_at::text AS processing_started_at, old_layout.name AS old_layout_name, new_layout.name AS new_layout_name, run.old_date_of_download::text AS old_date_of_download, run.new_date_of_download::text AS new_date_of_download, run.old_origin_file_name, run.new_origin_file_name FROM comparison_runs run JOIN layouts old_layout ON old_layout.id = run.old_layout_id JOIN layouts new_layout ON new_layout.id = run.new_layout_id ORDER BY run.processing_started_at DESC",
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
                let count = stream_load(&state.pool, field, &table, id, &layout).await?;
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
    sqlx::query("UPDATE comparison_runs SET processing_duration_ms = GREATEST(0, (EXTRACT(EPOCH FROM (now() - processing_started_at)) * 1000)::BIGINT) WHERE id = $1")
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

#[derive(Debug, Deserialize)]
struct CreateScheduledRequest {
    name: String,
    frequency: String,
    run_at: String,
    old_path: String,
    new_path: String,
    old_layout_id: Uuid,
    new_layout_id: Uuid,
    archive_path: String,
}

#[derive(Debug, Serialize, FromRow)]
struct ScheduledTask {
    id: Uuid,
    name: String,
    frequency: String,
    run_at: String,
    old_path: String,
    new_path: String,
    old_layout_id: Uuid,
    new_layout_id: Uuid,
    archive_path: String,
    status: String,
    created_at: String,
    last_run_at: Option<String>,
    error_message: Option<String>,
}

#[derive(Deserialize)]
struct ScheduledFilter {
    status: Option<String>,
}

async fn validate_schedule_request(request: &mut CreateScheduledRequest) -> anyhow::Result<()> {
    request.name = request.name.trim().to_owned();
    for value in [
        &mut request.old_path,
        &mut request.new_path,
        &mut request.archive_path,
    ] {
        *value = value.trim().to_owned();
    }
    if request.name.is_empty() {
        anyhow::bail!("schedule name is required");
    }
    if !matches!(
        request.frequency.as_str(),
        "one_time" | "daily" | "weekly" | "monthly"
    ) {
        anyhow::bail!("frequency must be one_time, daily, weekly, or monthly");
    }
    if request.archive_path.is_empty() {
        anyhow::bail!("archive path is required");
    }
    for (label, path) in [
        ("old path", &request.old_path),
        ("new path", &request.new_path),
    ] {
        let metadata = tokio::fs::metadata(path)
            .await
            .with_context(|| format!("{label} does not exist or cannot be read"))?;
        if !metadata.is_dir() {
            anyhow::bail!("{label} must be a directory");
        }
        let _directory = tokio::fs::read_dir(path)
            .await
            .with_context(|| format!("{label} is not readable"))?;
    }
    Ok(())
}

const SCHEDULE_FIELDS: &str = "id, name, frequency, run_at::text AS run_at, old_path, new_path, old_layout_id, new_layout_id, archive_path, status, created_at::text AS created_at, last_run_at::text AS last_run_at, error_message";
const SCHEDULE_SELECT: &str = "SELECT id, name, frequency, run_at::text AS run_at, old_path, new_path, old_layout_id, new_layout_id, archive_path, status, created_at::text AS created_at, last_run_at::text AS last_run_at, error_message FROM scheduled";

async fn list_scheduled(
    State(state): State<AppState>,
    Query(filter): Query<ScheduledFilter>,
) -> ApiResult<Vec<ScheduledTask>> {
    let rows = match filter.status.as_deref() {
        Some("all") => {
            sqlx::query_as(&format!("{SCHEDULE_SELECT} ORDER BY run_at"))
                .fetch_all(&state.pool)
                .await?
        }
        Some("pending") | None => {
            sqlx::query_as(&format!(
                "{SCHEDULE_SELECT} WHERE status IN ('pending', 'failed') ORDER BY run_at"
            ))
            .fetch_all(&state.pool)
            .await?
        }
        Some(status) => {
            sqlx::query_as(&format!(
                "{SCHEDULE_SELECT} WHERE status = $1 ORDER BY run_at"
            ))
            .bind(status)
            .fetch_all(&state.pool)
            .await?
        }
    };
    Ok(Json(rows))
}

async fn get_scheduled(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> ApiResult<ScheduledTask> {
    let task = sqlx::query_as(&format!("{SCHEDULE_SELECT} WHERE id = $1"))
        .bind(id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| anyhow::anyhow!("scheduled task not found"))?;
    Ok(Json(task))
}

async fn create_scheduled(
    State(state): State<AppState>,
    Json(mut request): Json<CreateScheduledRequest>,
) -> ApiResult<ScheduledTask> {
    validate_schedule_request(&mut request).await?;
    let id = Uuid::new_v4();
    let task = sqlx::query_as(&format!("INSERT INTO scheduled (id, name, frequency, run_at, old_path, new_path, old_layout_id, new_layout_id, archive_path) VALUES ($1, $2, $3, $4::timestamptz, $5, $6, $7, $8, $9) RETURNING {SCHEDULE_FIELDS}"))
        .bind(id).bind(&request.name).bind(&request.frequency).bind(&request.run_at)
        .bind(&request.old_path).bind(&request.new_path).bind(request.old_layout_id)
        .bind(request.new_layout_id).bind(&request.archive_path)
        .fetch_one(&state.pool).await?;
    Ok(Json(task))
}

async fn update_scheduled(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(mut request): Json<CreateScheduledRequest>,
) -> ApiResult<ScheduledTask> {
    validate_schedule_request(&mut request).await?;
    let task = sqlx::query_as(&format!("UPDATE scheduled SET name = $2, frequency = $3, run_at = $4::timestamptz, old_path = $5, new_path = $6, old_layout_id = $7, new_layout_id = $8, archive_path = $9, status = 'pending', error_message = NULL WHERE id = $1 RETURNING {SCHEDULE_FIELDS}"))
        .bind(id).bind(&request.name).bind(&request.frequency).bind(&request.run_at)
        .bind(&request.old_path).bind(&request.new_path).bind(request.old_layout_id)
        .bind(request.new_layout_id).bind(&request.archive_path)
        .fetch_optional(&state.pool).await?
        .ok_or_else(|| anyhow::anyhow!("scheduled task not found"))?;
    Ok(Json(task))
}

async fn delete_scheduled(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    let result = sqlx::query("DELETE FROM scheduled WHERE id = $1")
        .bind(id)
        .execute(&state.pool)
        .await?;
    if result.rows_affected() == 0 {
        return Err(AppError(anyhow::anyhow!("scheduled task not found")));
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn run_scheduled_now(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    let task = claim_scheduled(&state.pool, Some(id))
        .await?
        .ok_or_else(|| AppError(anyhow::anyhow!("scheduled task is not pending or failed")))?;
    tokio::spawn(execute_scheduled(state, task));
    Ok(StatusCode::ACCEPTED)
}

async fn scheduled_worker(state: AppState) {
    let mut interval = time::interval(Duration::from_secs(60));
    loop {
        interval.tick().await;
        while let Ok(Some(task)) = claim_scheduled(&state.pool, None).await {
            execute_scheduled(state.clone(), task).await;
        }
    }
}

async fn claim_scheduled(
    pool: &PgPool,
    requested_id: Option<Uuid>,
) -> anyhow::Result<Option<ScheduledTask>> {
    let condition = if requested_id.is_some() {
        "id = $1 AND status IN ('pending', 'failed')"
    } else {
        "status = 'pending' AND run_at <= now()"
    };
    // The lock is taken inside the CTE and the status update occurs in the
    // same statement, so multiple application instances cannot claim a run.
    let sql = format!(
        "WITH candidate AS (SELECT id FROM scheduled WHERE {condition} ORDER BY run_at FOR UPDATE SKIP LOCKED LIMIT 1) UPDATE scheduled s SET status = 'running', error_message = NULL FROM candidate WHERE s.id = candidate.id RETURNING s.id, s.name, s.frequency, s.run_at::text AS run_at, s.old_path, s.new_path, s.old_layout_id, s.new_layout_id, s.archive_path, s.status, s.created_at::text AS created_at, s.last_run_at::text AS last_run_at, s.error_message"
    );
    let query = sqlx::query_as::<_, ScheduledTask>(&sql);
    let task = if let Some(id) = requested_id {
        query.bind(id).fetch_optional(pool).await?
    } else {
        query.fetch_optional(pool).await?
    };
    Ok(task)
}

#[derive(Clone)]
struct ScheduledFile {
    path: PathBuf,
    relative: PathBuf,
    filename: String,
}

async fn scan_schedule_files(root: &FsPath) -> anyhow::Result<Vec<ScheduledFile>> {
    let mut result = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        let mut entries = tokio::fs::read_dir(&directory).await?;
        while let Some(entry) = entries.next_entry().await? {
            let file_type = entry.file_type().await?;
            if file_type.is_dir() {
                pending.push(entry.path());
            } else if file_type.is_file() {
                let path = entry.path();
                let relative = path
                    .strip_prefix(root)
                    .context("file escaped scheduled root")?
                    .to_path_buf();
                let filename = entry.file_name().to_string_lossy().into_owned();
                result.push(ScheduledFile {
                    path,
                    relative,
                    filename,
                });
            }
        }
    }
    Ok(result)
}

#[derive(Clone)]
struct MatchedFiles {
    old: ScheduledFile,
    new: ScheduledFile,
    chunks: Vec<String>,
    score: usize,
}

fn common_chunks(left: &str, right: &str) -> Vec<String> {
    // Repeatedly take the longest contiguous shared block and mask it in both
    // names. This produces non-overlapping chunks while retaining every block
    // used to calculate the resemblance score.
    let mut a = left.as_bytes().to_vec();
    let mut b = right.as_bytes().to_vec();
    let mut found: Vec<(usize, String)> = Vec::new();
    loop {
        let mut best = (0usize, 0usize, 0usize);
        for i in 0..a.len() {
            if a[i] == 0 {
                continue;
            }
            for j in 0..b.len() {
                if a[i] != b[j] || b[j] == 0 {
                    continue;
                }
                let mut length = 0;
                while i + length < a.len()
                    && j + length < b.len()
                    && a[i + length] != 0
                    && a[i + length] == b[j + length]
                {
                    length += 1;
                }
                if length > best.2 {
                    best = (i, j, length);
                }
            }
        }
        if best.2 == 0 {
            break;
        }
        found.push((
            best.0,
            String::from_utf8_lossy(&a[best.0..best.0 + best.2]).into_owned(),
        ));
        a[best.0..best.0 + best.2].fill(0);
        b[best.1..best.1 + best.2].fill(0);
    }
    found.sort_by_key(|(position, _)| *position);
    found.into_iter().map(|(_, chunk)| chunk).collect()
}

fn match_scheduled_files(
    old: Vec<ScheduledFile>,
    new: Vec<ScheduledFile>,
    processed: &HashSet<(String, String)>,
) -> Vec<MatchedFiles> {
    let mut old: Vec<_> = old.into_iter().collect();
    let mut new: Vec<_> = new.into_iter().collect();
    let mut matches = Vec::new();
    for old_file in &old {
        if new
            .iter()
            .all(|new_file| common_chunks(&old_file.filename, &new_file.filename).is_empty())
        {
            tracing::warn!(file = %old_file.filename, "skipping old scheduled file with no filename resemblance");
        }
    }
    for new_file in &new {
        if old
            .iter()
            .all(|old_file| common_chunks(&old_file.filename, &new_file.filename).is_empty())
        {
            tracing::warn!(file = %new_file.filename, "skipping new scheduled file with no filename resemblance");
        }
    }
    loop {
        let mut best: Option<(usize, usize, Vec<String>)> = None;
        for (old_index, old_file) in old.iter().enumerate() {
            for (new_index, new_file) in new.iter().enumerate() {
                if processed.contains(&(
                    old_file.relative.to_string_lossy().into_owned(),
                    new_file.relative.to_string_lossy().into_owned(),
                )) {
                    continue;
                }
                let chunks = common_chunks(&old_file.filename, &new_file.filename);
                let score = chunks.iter().map(String::len).sum::<usize>();
                if score
                    > best
                        .as_ref()
                        .map_or(0, |item| item.2.iter().map(String::len).sum())
                {
                    best = Some((old_index, new_index, chunks));
                }
            }
        }
        let Some((old_index, new_index, chunks)) = best else {
            break;
        };
        let score = chunks.iter().map(String::len).sum();
        if score == 0 {
            break;
        }
        let new_file = new.remove(new_index);
        let old_file = old.remove(old_index);
        matches.push(MatchedFiles {
            old: old_file,
            new: new_file,
            chunks,
            score,
        });
    }
    matches
}

fn scheduled_run_name(id: Uuid, chunks: &[String]) -> String {
    let suffix: String = chunks
        .concat()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    let mut name = format!("scheduled_{id}_{suffix}");
    name.truncate(100);
    name
}

async fn archive_file(source: &FsPath, destination: &FsPath) -> anyhow::Result<()> {
    let parent = destination
        .parent()
        .context("archive destination has no parent")?;
    tokio::fs::create_dir_all(parent).await?;
    match tokio::fs::rename(source, destination).await {
        Ok(()) => Ok(()),
        Err(rename_error) => {
            tracing::debug!(%rename_error, source = %source.display(), "rename failed; using copy/delete archive fallback");
            tokio::fs::copy(source, destination).await?;
            tokio::fs::remove_file(source).await?;
            Ok(())
        }
    }
}

async fn execute_scheduled(state: AppState, task: ScheduledTask) {
    let result = async {
        let processed: HashSet<(String, String)> = sqlx::query_as::<_, (String, String)>("SELECT old_filename, new_filename FROM scheduled_runs WHERE scheduled_id = $1")
            .bind(task.id).fetch_all(&state.pool).await?.into_iter().collect();
        let old_files = scan_schedule_files(FsPath::new(&task.old_path)).await?;
        let new_files = scan_schedule_files(FsPath::new(&task.new_path)).await?;
        let pairs = match_scheduled_files(old_files, new_files, &processed);
        for pair in pairs {
            tracing::info!(scheduled_id = %task.id, score = pair.score, old = %pair.old.filename, new = %pair.new.filename, "processing scheduled file pair");
            let comparison_id = create_comparison_from_paths(&state.pool, &task, &pair).await?;
            let old_archive = FsPath::new(&task.archive_path).join("matching/old").join(&pair.old.relative);
            let new_archive = FsPath::new(&task.archive_path).join("matching/new").join(&pair.new.relative);
            // Record only after both files are archived. If the second move
            // fails, restore the first file where possible for a later retry.
            let old_moved = match archive_file(&pair.old.path, &old_archive).await {
                Ok(()) => true,
                Err(error) => {
                    tracing::warn!(%error, file = %pair.old.path.display(), "could not archive old file");
                    false
                }
            };
            let new_moved = match archive_file(&pair.new.path, &new_archive).await {
                Ok(()) => true,
                Err(error) => {
                    tracing::warn!(%error, file = %pair.new.path.display(), "could not archive new file");
                    false
                }
            };
            if old_moved && new_moved {
                sqlx::query("INSERT INTO scheduled_runs (scheduled_id, comparison_id, old_filename, new_filename) SELECT $1, $2, $3, $4 WHERE EXISTS (SELECT 1 FROM scheduled WHERE id = $1) ON CONFLICT (scheduled_id, old_filename, new_filename) DO NOTHING")
                    .bind(task.id).bind(comparison_id).bind(pair.old.relative.to_string_lossy().as_ref()).bind(pair.new.relative.to_string_lossy().as_ref()).execute(&state.pool).await?;
            } else {
                if old_moved { if let Err(error) = archive_file(&old_archive, &pair.old.path).await { tracing::warn!(%error, "could not restore partially archived old file"); } }
                if new_moved { if let Err(error) = archive_file(&new_archive, &pair.new.path).await { tracing::warn!(%error, "could not restore partially archived new file"); } }
            }
        }
        Ok::<(), anyhow::Error>(())
    }.await;
    match result {
        Ok(()) => {
            let completion = "UPDATE scheduled SET status = CASE WHEN frequency = 'one_time' THEN 'completed' ELSE 'pending' END, run_at = CASE frequency WHEN 'daily' THEN run_at + interval '1 day' WHEN 'weekly' THEN run_at + interval '7 days' WHEN 'monthly' THEN run_at + interval '1 month' ELSE run_at END, last_run_at = now(), error_message = NULL WHERE id = $1";
            if let Err(error) = sqlx::query(completion)
                .bind(task.id)
                .execute(&state.pool)
                .await
            {
                tracing::error!(%error, scheduled_id = %task.id, "could not complete scheduled task");
            }
        }
        Err(error) => {
            tracing::error!(%error, scheduled_id = %task.id, "scheduled task failed");
            if let Err(update_error) = sqlx::query(
                "UPDATE scheduled SET status = 'failed', error_message = $2 WHERE id = $1",
            )
            .bind(task.id)
            .bind(error.to_string())
            .execute(&state.pool)
            .await
            {
                tracing::error!(%update_error, scheduled_id = %task.id, "could not store scheduled task failure");
            }
        }
    }
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
            "CREATE TABLE {table} (id BIGSERIAL PRIMARY KEY, comparison_id UUID NOT NULL REFERENCES comparison_runs(id) ON DELETE CASCADE, composite_primary_key TEXT NOT NULL, row_hash CHAR(16) NOT NULL, data JSONB NOT NULL)"
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

async fn create_comparison_from_paths(
    pool: &PgPool,
    task: &ScheduledTask,
    pair: &MatchedFiles,
) -> anyhow::Result<Uuid> {
    let old_layout = fetch_layout(pool, task.old_layout_id).await?;
    let new_layout = fetch_layout(pool, task.new_layout_id).await?;
    let id = Uuid::new_v4();
    let run_name = scheduled_run_name(task.id, &pair.chunks);
    let run_index: i64 = sqlx::query_scalar("INSERT INTO comparison_runs (id, run_name, old_layout_id, new_layout_id, old_date_of_download, old_origin_file_name, new_date_of_download, new_origin_file_name, processing_started_at) VALUES ($1, $2, $3, $4, now(), $5, now(), $6, now()) RETURNING run_index")
        .bind(id).bind(run_name).bind(task.old_layout_id).bind(task.new_layout_id)
        .bind(&pair.old.filename).bind(&pair.new.filename).fetch_one(pool).await?;
    create_source_tables(pool, run_index).await?;
    let old_file = tokio::fs::File::open(&pair.old.path)
        .await
        .with_context(|| format!("cannot read old file {}", pair.old.path.display()))?;
    let old_rows = stream_load_reader(
        pool,
        old_file,
        &source_table_name(true, run_index),
        id,
        &old_layout,
    )
    .await?;
    let new_file = tokio::fs::File::open(&pair.new.path)
        .await
        .with_context(|| format!("cannot read new file {}", pair.new.path.display()))?;
    let _new_rows = stream_load_reader(
        pool,
        new_file,
        &source_table_name(false, run_index),
        id,
        &new_layout,
    )
    .await?;
    compute_delta(pool, id, run_index).await?;
    sqlx::query("UPDATE comparison_runs SET processing_duration_ms = GREATEST(0, (EXTRACT(EPOCH FROM (now() - processing_started_at)) * 1000)::BIGINT) WHERE id = $1")
        .bind(id).execute(pool).await?;
    tracing::info!(comparison_id = %id, old_rows, "scheduled comparison completed");
    Ok(id)
}

async fn stream_load_reader(
    pool: &PgPool,
    file: tokio::fs::File,
    table: &str,
    comparison_id: Uuid,
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
        let sender = row_sender.clone();
        let fields = fields.to_vec();
        workers.push(tokio::task::spawn_blocking(move || {
            while let Some(line) = line_receiver.blocking_recv() {
                if sender.blocking_send(parse_row(&line, &fields)).is_err() {
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
                    count += 1;
                    batch.push(row);
                    if batch.len() == INSERT_BATCH_SIZE {
                        insert_batch(&writer_pool, &writer_table, comparison_id, &batch).await?;
                        batch.clear();
                    }
                }
                Ok(_) => {}
                Err(error) => {
                    parse_error.get_or_insert(error);
                }
            }
        }
        if let Some(error) = parse_error {
            return Err(error);
        }
        if !batch.is_empty() {
            insert_batch(&writer_pool, &writer_table, comparison_id, &batch).await?;
        }
        Ok::<u64, anyhow::Error>(count)
    });
    let mut reader = BufReader::new(file);
    let mut line = Vec::new();
    let mut next_worker = 0;
    let read_result = async {
        loop {
            line.clear();
            let read = reader.read_until(b'\n', &mut line).await?;
            if read == 0 {
                break;
            }
            if line.last() == Some(&b'\n') {
                line.pop();
            }
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            line_senders[next_worker]
                .send(std::mem::take(&mut line))
                .await
                .map_err(|_| anyhow::anyhow!("parsing workers stopped unexpectedly"))?;
            next_worker = (next_worker + 1) % worker_count;
        }
        Ok::<(), anyhow::Error>(())
    }
    .await;
    drop(line_senders);
    for worker in workers {
        worker.await.context("parsing worker panicked")?;
    }
    let write_result = writer.await.context("database writer task panicked")?;
    let count = write_result?;
    read_result?;
    Ok(count)
}

async fn stream_load(
    pool: &PgPool,
    mut file: axum::extract::multipart::Field<'_>,
    table: &str,
    comparison_id: Uuid,
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
                        insert_batch(&writer_pool, &writer_table, comparison_id, &batch).await?;
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
            insert_batch(&writer_pool, &writer_table, comparison_id, &batch).await?;
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
    // A database insertion failure closes the row receiver, which then makes
    // parser workers exit. Prefer that original failure over the subsequent
    // "workers stopped" symptom from the multipart reader.
    let count = write_result?;
    read_result?;
    Ok(count)
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
    let json = serde_json::to_string(&Value::Object(data))?;
    let row_hash = format!("{:016x}", xxh3_64(json.as_bytes()));
    Ok(Some((composite_key, row_hash, json)))
}

async fn insert_batch(
    pool: &PgPool,
    table: &str,
    comparison_id: Uuid,
    rows: &[ParsedRow],
) -> anyhow::Result<()> {
    let mut builder = QueryBuilder::<Postgres>::new(format!(
        "INSERT INTO {table} (comparison_id, composite_primary_key, row_hash, data) "
    ));
    builder.push_values(rows, |mut row, (key, row_hash, json)| {
        row.push_bind(comparison_id)
            .push_bind(key)
            .push_bind(row_hash)
            .push_bind(json)
            .push_unseparated("::jsonb");
    });
    builder.build().execute(pool).await?;
    Ok(())
}

async fn compute_delta(pool: &PgPool, id: Uuid, run_index: i64) -> anyhow::Result<()> {
    let old_table = source_table_name(true, run_index);
    let new_table = source_table_name(false, run_index);
    // Aggregate the JSON field diff per modified row. This preserves the
    // indexed source-table join while avoiding the former global aggregate
    // across every changed row and every JSON field.
    sqlx::query(
        &format!("WITH changed AS MATERIALIZED (
            SELECT o.composite_primary_key, o.data AS old_data, n.data AS new_data
            FROM {old_table} o
            JOIN {new_table} n
                ON n.composite_primary_key = o.composite_primary_key
            WHERE o.comparison_id = $1
                AND o.row_hash <> n.row_hash
                AND o.data IS DISTINCT FROM n.data
        )
        INSERT INTO delta_rows (comparison_id, composite_primary_key, change_type, old_data, new_data, changed_fields)
        SELECT
            $1,
            changed.composite_primary_key,
            'modified',
            changed.old_data,
            changed.new_data,
            COALESCE(diff.changed_fields, '{{}}'::jsonb)
        FROM changed
        CROSS JOIN LATERAL (
            SELECT jsonb_object_agg(
                key,
                jsonb_build_object('old', changed.old_data -> key, 'new', changed.new_data -> key)
            ) AS changed_fields
            FROM jsonb_object_keys(changed.old_data || changed.new_data) AS key
            WHERE (changed.old_data -> key) IS DISTINCT FROM (changed.new_data -> key)
        ) AS diff"),
    )
    .bind(id)
    .execute(pool)
    .await?;
    sqlx::query(&format!("INSERT INTO delta_rows (comparison_id, composite_primary_key, change_type, old_data, changed_fields) SELECT $1, o.composite_primary_key, 'removed', o.data, '{{}}'::jsonb FROM {old_table} o LEFT JOIN {new_table} n ON n.composite_primary_key = o.composite_primary_key WHERE o.comparison_id = $1 AND n.id IS NULL"))
        .bind(id)
        .execute(pool)
        .await?;
    sqlx::query(&format!("INSERT INTO delta_rows (comparison_id, composite_primary_key, change_type, new_data, changed_fields) SELECT $1, n.composite_primary_key, 'added', n.data, '{{}}'::jsonb FROM {new_table} n LEFT JOIN {old_table} o ON o.composite_primary_key = n.composite_primary_key WHERE n.comparison_id = $1 AND o.id IS NULL"))
        .bind(id)
        .execute(pool)
        .await?;
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
        let data: Value = serde_json::from_str(&row.2).unwrap();
        assert_eq!(data["name"], "ALICE");
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

    #[test]
    fn filename_chunks_are_non_overlapping_and_ordered() {
        let chunks = common_chunks("ABC_DEF_001.dat", "ABC_DEF_002.dat");
        assert_eq!(chunks.concat(), "ABC_DEF_00.dat");
        assert_eq!(
            scheduled_run_name(Uuid::nil(), &chunks),
            "scheduled_00000000-0000-0000-0000-000000000000_ABC_DEF_00_dat"
        );
    }
}
