//! Daemon configuration: which pairs to watch and how often to poll.

use std::path::PathBuf;

use recon_core::error::{ReconError, ReconResult};
use serde::{Deserialize, Serialize};

fn default_poll_seconds() -> u64 {
    30
}

/// Configuration for the watcher daemon.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DaemonConfig {
    /// Root of the schema library (`schemas/`), resolving run-config schema refs.
    pub schemas_dir: PathBuf,
    /// Poll interval in seconds.
    #[serde(default = "default_poll_seconds")]
    pub poll_seconds: u64,
    /// Run-config file paths, one per watched source pair.
    pub pairs: Vec<PathBuf>,
}

impl DaemonConfig {
    /// Parse a daemon config from YAML.
    pub fn from_yaml(text: &str) -> ReconResult<Self> {
        serde_norway::from_str(text)
            .map_err(|e| ReconError::config(format!("daemon config: {e}")))
    }

    /// Load a daemon config from a file.
    pub fn load(path: &std::path::Path) -> ReconResult<Self> {
        let text = std::fs::read_to_string(path)?;
        Self::from_yaml(&text)
    }
}
