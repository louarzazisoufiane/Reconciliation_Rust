//! `recon-web`: the operator entry point (spec's WEB INTERFACE section).
//!
//! A JSON API served by axum, with the React SPA (`web/`) embedded into the
//! same binary (see `assets.rs`) for a single-binary deploy. Endpoints cover
//! the schema library (list/get/create, versioned via `SchemaStore`), building
//! a run (validate the two chosen schemas' shared columns, then build + trigger
//! with decision-9 cross-schema validation), and recent runs. Building a run
//! WRITES its config as YAML under `run-configs/` (git-versionable,
//! daemon-loadable) and immediately triggers it through the same oneshot
//! pipeline the `recon` CLI and the daemon use — this crate does no comparison
//! itself.

mod assets;

use std::collections::{BTreeMap, HashSet};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use axum::Router;
use axum::extract::{Path as AxumPath, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Json;
use recon_core::config::{
    CompletionDetection, DuplicatePolicy, Normalization, ReportConfig, RunConfig, SidecarFormat,
    SourceConfig,
};
use recon_core::engine::Summary;
use recon_core::error::ReconError;
use recon_core::schema::{Field, Schema, SchemaRef, SchemaWarning};
use recon_orch::{generate_run_id, run_oneshot};
use recon_report::ManifestEntry;
use recon_schema::{FsSchemaStore, SchemaInfo, SchemaStore};
use serde::{Deserialize, Serialize};
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;

#[derive(Clone)]
struct AppState {
    store: Arc<FsSchemaStore>,
    reports_dir: PathBuf,
    run_configs_dir: PathBuf,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let schemas_dir: PathBuf = std::env::var("RECON_SCHEMAS_DIR")
        .unwrap_or_else(|_| "schemas".into())
        .into();
    let reports_dir: PathBuf = std::env::var("RECON_REPORTS_DIR")
        .unwrap_or_else(|_| "reports".into())
        .into();
    let run_configs_dir: PathBuf = std::env::var("RECON_RUN_CONFIGS_DIR")
        .unwrap_or_else(|_| "run-configs".into())
        .into();
    let addr: SocketAddr = std::env::var("RECON_WEB_ADDR")
        .unwrap_or_else(|_| "127.0.0.1:3000".into())
        .parse()?;

    std::fs::create_dir_all(&schemas_dir)?;
    std::fs::create_dir_all(&reports_dir)?;
    std::fs::create_dir_all(&run_configs_dir)?;

    let state = AppState {
        store: Arc::new(FsSchemaStore::new(schemas_dir)),
        reports_dir: reports_dir.clone(),
        run_configs_dir,
    };

    let api = Router::new()
        .route("/schemas", get(schemas_list).post(schemas_create))
        .route("/schemas/infer", post(schemas_infer))
        .route("/schemas/{name}", get(schemas_get))
        .route("/runs", get(runs_list).post(runs_create))
        .route("/runs/validate", post(runs_validate))
        .with_state(state.clone());

    let app = Router::new()
        .nest("/api", api)
        .nest_service("/reports", ServeDir::new(reports_dir))
        .fallback(assets::static_handler)
        .layer(TraceLayer::new_for_http());

    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!(%addr, "recon-web listening");
    axum::serve(listener, app).await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Error handling — every handler returns JSON, including errors.
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct ErrorResponse {
    error: String,
}

struct AppError(ReconError);

impl From<ReconError> for AppError {
    fn from(e: ReconError) -> Self {
        AppError(e)
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = match &self.0 {
            ReconError::Config(_) => StatusCode::BAD_REQUEST,
            ReconError::Io(_) | ReconError::Engine(_) => StatusCode::INTERNAL_SERVER_ERROR,
        };
        (
            status,
            Json(ErrorResponse {
                error: self.0.to_string(),
            }),
        )
            .into_response()
    }
}

fn describe_warning(w: &SchemaWarning) -> String {
    match w {
        SchemaWarning::Overlap { a, b } => format!("'{a}' and '{b}' overlap"),
        SchemaWarning::Gap {
            after,
            before,
            bytes,
        } => format!("{bytes} byte gap between '{after}' and '{before}'"),
    }
}

// ---------------------------------------------------------------------------
// Schema library
// ---------------------------------------------------------------------------

async fn schemas_list(State(state): State<AppState>) -> Result<Json<Vec<SchemaInfo>>, AppError> {
    Ok(Json(state.store.list()?))
}

#[derive(Debug, Deserialize)]
struct SchemaQuery {
    v: Option<u32>,
}

async fn schemas_get(
    State(state): State<AppState>,
    AxumPath(name): AxumPath<String>,
    Query(q): Query<SchemaQuery>,
) -> Result<Json<Schema>, AppError> {
    let schema = match q.v {
        Some(v) => state.store.get(&name, v)?,
        None => state.store.get_latest(&name)?,
    };
    Ok(Json(schema))
}

#[derive(Debug, Deserialize)]
struct InferRequest {
    sample: String,
}

async fn schemas_infer(Json(req): Json<InferRequest>) -> Json<Schema> {
    Json(infer_draft(&req.sample))
}

/// Draft a schema from a pasted sample line: each run of non-space bytes
/// becomes a field, offsets taken from the byte position in the sample. The
/// operator still reviews/edits before saving (decision 5's "draft" state).
fn infer_draft(sample: &str) -> Schema {
    let bytes = sample.as_bytes();
    let mut fields = Vec::new();
    let mut i = 0;
    let mut n = 1;
    while i < bytes.len() {
        while i < bytes.len() && bytes[i] == b' ' {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        let start = i;
        while i < bytes.len() && bytes[i] != b' ' {
            i += 1;
        }
        fields.push(Field {
            name: format!("field{n}"),
            start,
            length: i - start,
        });
        n += 1;
    }
    Schema {
        name: String::new(),
        version: 0,
        encoding: "utf-8".into(),
        index_base: 0,
        fields,
    }
}

#[derive(Serialize)]
struct SchemaSaveResponse {
    schema: Schema,
    warnings: Vec<String>,
}

async fn schemas_create(
    State(state): State<AppState>,
    Json(mut draft): Json<Schema>,
) -> Result<Json<SchemaSaveResponse>, AppError> {
    if draft.name.trim().is_empty() {
        return Err(AppError(ReconError::config("schema name is required")));
    }
    draft.version = 0; // ignored on input — `SchemaStore::save` assigns the next version.
    let warnings = draft.validate()?;
    let version = state.store.save(&draft)?;
    draft.version = version;
    Ok(Json(SchemaSaveResponse {
        schema: draft,
        warnings: warnings.iter().map(describe_warning).collect(),
    }))
}

// ---------------------------------------------------------------------------
// Build a run
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct ValidateRunRequest {
    schema_a: SchemaRef,
    schema_b: SchemaRef,
}

#[derive(Serialize)]
struct ValidateRunResponse {
    common_columns: Vec<String>,
}

/// Decision 9 (interactive half): resolve both chosen schemas and return the
/// column names present, by name, in BOTH — the only columns the UI should
/// offer for key/compare selection.
async fn runs_validate(
    State(state): State<AppState>,
    Json(req): Json<ValidateRunRequest>,
) -> Result<Json<ValidateRunResponse>, AppError> {
    let schema_a = state.store.resolve(&req.schema_a)?;
    let schema_b = state.store.resolve(&req.schema_b)?;

    let names_a: HashSet<&str> = schema_a.field_names().into_iter().collect();
    let common: Vec<String> = schema_b
        .field_names()
        .into_iter()
        .filter(|n| names_a.contains(n))
        .map(String::from)
        .collect();

    if common.is_empty() {
        return Err(AppError(ReconError::config(
            "the two chosen schemas share no column names",
        )));
    }

    Ok(Json(ValidateRunResponse {
        common_columns: common,
    }))
}

#[derive(Debug, Deserialize)]
struct RunBuildRequest {
    run_name: String,
    path_a: String,
    path_b: String,
    schema_a: SchemaRef,
    schema_b: SchemaRef,
    key: String,
    compare_columns: Vec<String>,
    #[serde(default)]
    normalization: BTreeMap<String, Normalization>,
}

#[derive(Serialize)]
struct RunCreateResponse {
    run_id: String,
    report_url: String,
    summary: Summary,
}

/// Decision 9 (hard half): building a run whose key or a compare column is
/// absent (by name) from either chosen schema is REFUSED, not just warned.
async fn runs_create(
    State(state): State<AppState>,
    Json(req): Json<RunBuildRequest>,
) -> Result<Json<RunCreateResponse>, AppError> {
    if req.run_name.trim().is_empty() || req.key.trim().is_empty() || req.compare_columns.is_empty()
    {
        return Err(AppError(ReconError::config(
            "run name, key, and at least one compare column are required",
        )));
    }

    let schema_a = state.store.resolve(&req.schema_a)?;
    let schema_b = state.store.resolve(&req.schema_b)?;

    for col in std::iter::once(&req.key).chain(req.compare_columns.iter()) {
        if !schema_a.has_column(col) || !schema_b.has_column(col) {
            return Err(AppError(ReconError::config(format!(
                "column '{col}' is not present in both chosen schemas"
            ))));
        }
    }

    let cfg = RunConfig {
        run_name: req.run_name.clone(),
        key: req.key,
        duplicate_policy: DuplicatePolicy::default(),
        compare_columns: req.compare_columns,
        normalization: req.normalization,
        source_a: SourceConfig {
            path: PathBuf::from(req.path_a),
            schema_ref: req.schema_a,
        },
        source_b: SourceConfig {
            path: PathBuf::from(req.path_b),
            schema_ref: req.schema_b,
        },
        report: ReportConfig {
            embed_row_cap: 5000,
            sidecar_format: SidecarFormat::default(),
            output_dir: state.reports_dir.clone(),
        },
        completion_detection: CompletionDetection::default(),
    };

    cfg.validate()?;

    // WRITE the run config as YAML (git-versionable, and loadable later by the
    // daemon's `pairs` list) before triggering the oneshot run.
    let safe_name: String = cfg
        .run_name
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();
    let cfg_path = state.run_configs_dir.join(format!("{safe_name}.yml"));
    let yaml = serde_norway::to_string(&cfg)
        .map_err(|e| AppError(ReconError::config(format!("serializing run config: {e}"))))?;
    std::fs::write(&cfg_path, yaml)
        .map_err(|e| AppError(ReconError::Io(format!("writing {}: {e}", cfg_path.display()))))?;

    let run_id = generate_run_id();
    let result = run_oneshot(&cfg, state.store.as_ref(), &run_id)?;
    let file = result
        .paths
        .report_html
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or_default()
        .to_string();

    Ok(Json(RunCreateResponse {
        run_id,
        report_url: format!("/reports/{file}"),
        summary: result.outcome.summary,
    }))
}

// ---------------------------------------------------------------------------
// Runs
// ---------------------------------------------------------------------------

async fn runs_list(State(state): State<AppState>) -> Result<Json<Vec<ManifestEntry>>, AppError> {
    let mut entries = recon_report::load_manifest(&state.reports_dir)?;
    entries.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    Ok(Json(entries))
}
