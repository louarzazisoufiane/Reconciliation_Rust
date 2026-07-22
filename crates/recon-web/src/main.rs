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
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use sqlx::{
    FromRow, PgPool,
    postgres::{PgConnectOptions, PgPoolCopyExt, PgPoolOptions},
};
use tower_http::trace::TraceLayer;
use uuid::Uuid;

const MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("../../migrations");
const COPY_BUFFER_SIZE: usize = 64 * 1024;

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
        .route("/comparisons", post(create_comparison))
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
    let mut comparison_id = None;
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
                        sqlx::query("INSERT INTO comparison_runs (id, old_layout_id, new_layout_id, old_date_of_download, old_origin_file_name, new_date_of_download, new_origin_file_name) VALUES ($1, $2, $3, $4::timestamptz, $5, $6::timestamptz, $7)")
                            .bind(id).bind(old_id).bind(new_id).bind(old_date).bind(old_file_name).bind(new_date).bind(new_file_name)
                            .execute(&state.pool).await?;
                        comparison_id = Some(id);
                        id
                    }
                };
                let is_old = name == "old_file";
                let layout =
                    fetch_layout(&state.pool, if is_old { old_id } else { new_id }).await?;
                let count = stream_load(
                    &state.pool,
                    field,
                    if is_old { "old_rows" } else { "new_rows" },
                    id,
                    &layout,
                )
                .await?;
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
    let old_rows = old_rows.ok_or_else(|| anyhow::anyhow!("old file is required"))?;
    let new_rows = new_rows.ok_or_else(|| anyhow::anyhow!("new file is required"))?;
    compute_delta(&state.pool, comparison_id).await?;
    let counts = sqlx::query_as::<_, DeltaCount>("SELECT count(*) FILTER (WHERE change_type = 'added') AS added, count(*) FILTER (WHERE change_type = 'removed') AS removed, count(*) FILTER (WHERE change_type = 'modified') AS modified FROM delta_rows WHERE comparison_id = $1").bind(comparison_id).fetch_one(&state.pool).await?;
    Ok(Json(ComparisonResponse {
        id: comparison_id,
        old_rows,
        new_rows,
        added: counts.added.unwrap_or(0),
        removed: counts.removed.unwrap_or(0),
        modified: counts.modified.unwrap_or(0),
    }))
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

async fn stream_load(
    pool: &PgPool,
    mut file: axum::extract::multipart::Field<'_>,
    table: &str,
    comparison_id: Uuid,
    fields: &[LayoutField],
) -> anyhow::Result<u64> {
    let mut pending = Vec::new();
    let copy_statement = match table {
        "old_rows" => {
            "COPY old_rows (comparison_id, composite_primary_key, data) FROM STDIN WITH (FORMAT text)"
        }
        "new_rows" => {
            "COPY new_rows (comparison_id, composite_primary_key, data) FROM STDIN WITH (FORMAT text)"
        }
        _ => anyhow::bail!("invalid source table"),
    };
    let mut copy = pool.copy_in_raw(copy_statement).await?;
    let mut copy_buffer = Vec::with_capacity(COPY_BUFFER_SIZE);
    let mut count = 0;
    while let Some(chunk) = file.chunk().await? {
        pending.extend_from_slice(&chunk);
        while let Some(newline) = pending.iter().position(|b| *b == b'\n') {
            let mut line: Vec<u8> = pending.drain(..=newline).collect();
            if line.last() == Some(&b'\n') {
                line.pop();
            }
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            if let Some((key, data)) = parse_row(&line, fields)? {
                write_copy_row(&mut copy_buffer, comparison_id, &key, &data)?;
                count += 1;
            }
            if copy_buffer.len() >= COPY_BUFFER_SIZE {
                copy.send(copy_buffer.as_slice()).await?;
                copy_buffer.clear();
            }
        }
    }
    if !pending.is_empty() {
        if let Some((key, data)) = parse_row(&pending, fields)? {
            write_copy_row(&mut copy_buffer, comparison_id, &key, &data)?;
            count += 1;
        }
    }
    if !copy_buffer.is_empty() {
        copy.send(copy_buffer.as_slice()).await?;
    }
    copy.finish().await?;
    Ok(count)
}

fn parse_row(line: &[u8], fields: &[LayoutField]) -> anyhow::Result<Option<(String, Value)>> {
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
    Ok(Some((key_parts.join("\u{1f}"), Value::Object(data))))
}

fn write_copy_row(
    buffer: &mut Vec<u8>,
    comparison_id: Uuid,
    key: &str,
    data: &Value,
) -> anyhow::Result<()> {
    copy_text_field(buffer, &comparison_id.to_string());
    buffer.push(b'\t');
    copy_text_field(buffer, key);
    buffer.push(b'\t');
    copy_text_field(buffer, &serde_json::to_string(data)?);
    buffer.push(b'\n');
    Ok(())
}

/// Escape a PostgreSQL COPY text field. The parser has already bounded memory
/// to a line and `copy_buffer` is flushed regularly, so source size does not
/// determine resident memory.
fn copy_text_field(buffer: &mut Vec<u8>, value: &str) {
    for byte in value.bytes() {
        match byte {
            b'\\' => buffer.extend_from_slice(b"\\\\"),
            b'\t' => buffer.extend_from_slice(b"\\t"),
            b'\n' => buffer.extend_from_slice(b"\\n"),
            b'\r' => buffer.extend_from_slice(b"\\r"),
            _ => buffer.push(byte),
        }
    }
}

async fn compute_delta(pool: &PgPool, id: Uuid) -> anyhow::Result<()> {
    sqlx::query("INSERT INTO delta_rows (comparison_id, composite_primary_key, change_type, old_data, new_data, changed_fields) SELECT $1, o.composite_primary_key, 'modified', o.data, n.data, COALESCE(jsonb_object_agg(k, jsonb_build_object('old', o.data -> k, 'new', n.data -> k)) FILTER (WHERE (o.data -> k) IS DISTINCT FROM (n.data -> k)), '{}'::jsonb) FROM old_rows o JOIN new_rows n ON n.comparison_id = o.comparison_id AND n.composite_primary_key = o.composite_primary_key CROSS JOIN LATERAL jsonb_object_keys(o.data || n.data) AS k WHERE o.comparison_id = $1 AND o.data IS DISTINCT FROM n.data GROUP BY o.composite_primary_key, o.data, n.data").bind(id).execute(pool).await?;
    sqlx::query("INSERT INTO delta_rows (comparison_id, composite_primary_key, change_type, old_data, changed_fields) SELECT $1, o.composite_primary_key, 'removed', o.data, '{}'::jsonb FROM old_rows o LEFT JOIN new_rows n ON n.comparison_id = o.comparison_id AND n.composite_primary_key = o.composite_primary_key WHERE o.comparison_id = $1 AND n.id IS NULL").bind(id).execute(pool).await?;
    sqlx::query("INSERT INTO delta_rows (comparison_id, composite_primary_key, change_type, new_data, changed_fields) SELECT $1, n.composite_primary_key, 'added', n.data, '{}'::jsonb FROM new_rows n LEFT JOIN old_rows o ON o.comparison_id = n.comparison_id AND o.composite_primary_key = n.composite_primary_key WHERE n.comparison_id = $1 AND o.id IS NULL").bind(id).execute(pool).await?;
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
        assert_eq!(row.1["name"], "ALICE");
    }

    #[test]
    fn copy_text_escapes_copy_control_characters() {
        let mut encoded = Vec::new();
        copy_text_field(&mut encoded, "a\tb\nc\\d");
        assert_eq!(encoded, b"a\\tb\\nc\\\\d");
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
