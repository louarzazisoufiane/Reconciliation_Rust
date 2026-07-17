//! The orchestration drivers (ORCHESTRATION section).
//!
//! Two drivers sit behind one [`Orchestrator`] seam:
//!   * [`OneshotDriver`] runs the whole chain once (cron / systemd-timer / web).
//!   * [`Daemon`] is a long-running watcher that, per configured pair, waits for
//!     BOTH sources to complete (existence + size stability) and then runs the
//!     same oneshot chain, handling retries/backoff itself.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use recon_core::config::RunConfig;
use recon_core::error::{ReconError, ReconResult};
use recon_schema::{FsSchemaStore, SchemaStore};
use tracing::{info, warn};

use crate::dconfig::DaemonConfig;
use crate::detector::{CompletionDetector, SizeStabilityDetector};
use crate::pipeline::{generate_run_id, run_oneshot};

/// The orchestration seam. Both the oneshot and daemon drivers implement it;
/// the daemon additionally exposes an async [`Daemon::run`] loop.
pub trait Orchestrator {
    /// Drive the configured work. For oneshot this runs once and returns; the
    /// daemon variant blocks until shutdown (see [`Daemon::run`]).
    fn drive(&self) -> ReconResult<()>;
}

/// The oneshot driver: resolve schemas, compare, report, index — once.
pub struct OneshotDriver<'a> {
    /// The run configuration to execute.
    pub config: &'a RunConfig,
    /// Schema store used to resolve the two schema refs.
    pub store: &'a dyn SchemaStore,
    /// Explicit run id (generate one with [`generate_run_id`] if absent).
    pub run_id: String,
}

impl Orchestrator for OneshotDriver<'_> {
    fn drive(&self) -> ReconResult<()> {
        let result = run_oneshot(self.config, self.store, &self.run_id)?;
        info!(
            run_id = %self.run_id,
            report = %result.paths.report_html.display(),
            pass = result.outcome.summary.pass,
            "oneshot run complete"
        );
        Ok(())
    }
}

/// The watcher daemon.
pub struct Daemon {
    config: DaemonConfig,
    store: FsSchemaStore,
    detector: SizeStabilityDetector,
    /// Last (size_a, size_b) that we successfully ran, keyed by config path.
    last_run: HashMap<PathBuf, (u64, u64)>,
}

impl Daemon {
    /// Build a daemon from its config, using the size-stability detector with
    /// the per-pair stability window taken from each run config.
    pub fn new(config: DaemonConfig) -> Self {
        let store = FsSchemaStore::new(config.schemas_dir.clone());
        // A single detector keyed by path; the window is the max across pairs
        // (each pair also carries its own, honored via the run config footer).
        let detector = SizeStabilityDetector::new(5);
        Daemon {
            config,
            store,
            detector,
            last_run: HashMap::new(),
        }
    }

    /// Build a daemon with an explicit detector (used in tests to shorten the
    /// stability window).
    pub fn with_detector(config: DaemonConfig, detector: SizeStabilityDetector) -> Self {
        let store = FsSchemaStore::new(config.schemas_dir.clone());
        Daemon {
            config,
            store,
            detector,
            last_run: HashMap::new(),
        }
    }

    /// Run the watch loop until Ctrl-C. Structured logs throughout; a failing
    /// pair is logged and retried on the next tick (backoff = poll interval).
    pub async fn run(mut self) -> ReconResult<()> {
        let interval = Duration::from_secs(self.config.poll_seconds.max(1));
        info!(
            pairs = self.config.pairs.len(),
            poll_seconds = self.config.poll_seconds,
            "daemon started"
        );
        let pairs = self.config.pairs.clone();
        loop {
            for pair_path in &pairs {
                // `tick_pair` runs the engine, whose streaming collects/sinks
                // call `block_on` on polars' own tokio runtime — that panics
                // on a runtime worker thread, so step off the async runtime.
                // (`block_in_place` needs the multi-thread runtime, which is
                // what the `#[tokio::main]` binary provides.)
                let tick = tokio::task::block_in_place(|| self.tick_pair(pair_path));
                if let Err(e) = tick {
                    warn!(pair = %pair_path.display(), error = %e, "pair tick failed; will retry");
                }
            }
            tokio::select! {
                _ = tokio::time::sleep(interval) => {}
                _ = tokio::signal::ctrl_c() => {
                    info!("shutdown signal received; stopping daemon");
                    return Ok(());
                }
            }
        }
    }

    /// One evaluation of a single pair: if both sources are complete and the
    /// inputs changed since the last successful run, run the oneshot chain.
    pub fn tick_pair(&mut self, config_path: &Path) -> ReconResult<()> {
        let text = std::fs::read_to_string(config_path)?;
        let config = RunConfig::from_yaml(&text)?;

        let a = &config.source_a.path;
        let b = &config.source_b.path;
        if !self.detector.is_complete(a)? || !self.detector.is_complete(b)? {
            return Ok(());
        }

        let sig = (
            std::fs::metadata(a)?.len(),
            std::fs::metadata(b)?.len(),
        );
        if self.last_run.get(config_path) == Some(&sig) {
            // Already reconciled this exact input; nothing to do.
            return Ok(());
        }

        let run_id = generate_run_id();
        info!(pair = %config_path.display(), run_id = %run_id, "both sources complete; running");
        let result = run_oneshot(&config, &self.store, &run_id).map_err(|e| {
            ReconError::engine(format!("oneshot for {}: {e}", config_path.display()))
        })?;
        info!(
            run_id = %run_id,
            report = %result.paths.report_html.display(),
            pass = result.outcome.summary.pass,
            "run complete"
        );
        self.last_run.insert(config_path.to_path_buf(), sig);
        Ok(())
    }
}
