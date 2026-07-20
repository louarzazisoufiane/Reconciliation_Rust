//! `recon-orch`: pure-Rust orchestration (replaces Airflow).
//!
//! Implements the task graph
//! `wait_source_A + wait_source_B → run_comparison → generate_report →
//! update_index → report_out`. The oneshot [`pipeline`] is the unit the
//! daemon, the CLI, and the web UI all invoke; the [`Daemon`] is a long-running
//! watcher over that same chain. Completion detection and the (no-op) notifier
//! live behind their own seams.

pub mod daemon;
pub mod dconfig;
pub mod detector;
pub mod notifier;
pub mod pipeline;

pub use daemon::Daemon;
pub use dconfig::DaemonConfig;
pub use detector::{CompletionDetector, SizeStabilityDetector};
pub use notifier::{NoopNotifier, Notifier};
pub use pipeline::{OneshotResult, generate_run_id, run_oneshot, run_oneshot_with_notifier};
