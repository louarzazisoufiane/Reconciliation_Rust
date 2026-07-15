//! The `Notifier` seam (decision 15).
//!
//! No alerting today — people open `index.html` / the web UI. Email/Slack
//! fail-only alerting is [FUTURE]; this leaves the hook point with a no-op impl.

use recon_core::engine::Summary;
use recon_core::error::ReconResult;

/// Post-run notification hook.
pub trait Notifier: Send + Sync {
    /// Called after a run completes with its summary.
    fn notify(&self, summary: &Summary) -> ReconResult<()>;
}

/// The default no-op notifier.
#[derive(Debug, Clone, Default)]
pub struct NoopNotifier;

impl Notifier for NoopNotifier {
    fn notify(&self, _summary: &Summary) -> ReconResult<()> {
        Ok(())
    }
}
