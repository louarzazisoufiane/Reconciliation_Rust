//! The oneshot pipeline: run_comparison → generate_report → update_index →
//! report_out. This is the unit invoked by the `recon` CLI, the daemon, and the
//! web UI (decision, ORCHESTRATION section).

use std::time::{SystemTime, UNIX_EPOCH};

use recon_core::config::RunConfig;
use recon_core::engine::{ReconOutcome, run_comparison};
use recon_core::error::ReconResult;
use recon_report::{PublishedPaths, artifact_stem, publish};
use recon_schema::SchemaStore;

use crate::notifier::{NoopNotifier, Notifier};

/// The result of a completed oneshot run.
pub struct OneshotResult {
    /// Full engine outcome (summary + capped samples).
    pub outcome: ReconOutcome,
    /// Written artifact paths.
    pub paths: PublishedPaths,
}

/// Generate a compact, dependency-free run id from the wall clock.
pub fn generate_run_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{nanos:x}")
}

/// Run the full chain once: resolve the two schema refs, compare, stream the
/// sidecar, write the self-contained report, append the manifest, regenerate
/// the index, and fire the (no-op) notifier.
///
/// Returns `Ok` on any *data* outcome (decision 12); `Err` only on a genuine
/// engine/IO/config failure.
pub fn run_oneshot(
    config: &RunConfig,
    store: &dyn SchemaStore,
    run_id: &str,
) -> ReconResult<OneshotResult> {
    run_oneshot_with_notifier(config, store, run_id, &NoopNotifier)
}

/// [`run_oneshot`] with an explicit notifier (for the [FUTURE] alerting seam).
pub fn run_oneshot_with_notifier(
    config: &RunConfig,
    store: &dyn SchemaStore,
    run_id: &str,
    notifier: &dyn Notifier,
) -> ReconResult<OneshotResult> {
    config.validate()?;
    let schema_a = store.resolve(&config.source_a.schema_ref)?;
    let schema_b = store.resolve(&config.source_b.schema_ref)?;

    let now = jiff::Timestamp::now();
    let stem = artifact_stem(&now.to_string(), run_id);
    let output_dir = &config.report.output_dir;
    let sidecar = output_dir.join(format!("{stem}.parquet"));

    let outcome = run_comparison(config, &schema_a, &schema_b, run_id, now, &sidecar)?;
    let paths = publish(output_dir, &outcome)?;
    notifier.notify(&outcome.summary)?;

    Ok(OneshotResult { outcome, paths })
}
