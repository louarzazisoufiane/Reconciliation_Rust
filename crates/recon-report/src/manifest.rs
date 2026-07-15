//! The append-only run history manifest (decision 14).
//!
//! `reports/manifest.json` is an append-only JSON array — every run is kept,
//! nothing is auto-pruned. `index.html` is regenerated from it so history is
//! viewable with no server running. SQLite backing is [FUTURE].

use std::path::{Path, PathBuf};

use recon_core::engine::Summary;
use recon_core::error::{ReconError, ReconResult};
use serde::{Deserialize, Serialize};

/// One row of run history — a flattened projection of a [`Summary`] plus the
/// artifact filenames.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestEntry {
    /// Run id.
    pub run_id: String,
    /// Run name.
    pub run_name: String,
    /// RFC-3339 timestamp.
    pub timestamp: String,
    /// Self-contained report filename (relative to `reports/`).
    pub report_html: String,
    /// Full-diff sidecar filename (relative to `reports/`).
    pub sidecar: String,
    /// Primary key column.
    pub key: String,
    /// Rows read from A.
    pub rows_a: usize,
    /// Rows read from B.
    pub rows_b: usize,
    /// only-in-A count.
    pub only_in_a: usize,
    /// only-in-B count.
    pub only_in_b: usize,
    /// changed count.
    pub changed: usize,
    /// matched count.
    pub matched: usize,
    /// Distinct duplicate keys in A.
    pub dup_keys_a: usize,
    /// Distinct duplicate keys in B.
    pub dup_keys_b: usize,
    /// Match rate.
    pub match_rate: f64,
    /// Pass label (does not affect exit code).
    pub pass: bool,
}

impl ManifestEntry {
    /// Build a manifest entry from a run summary + report filename.
    pub fn from_summary(summary: &Summary, report_html: &str) -> ManifestEntry {
        ManifestEntry {
            run_id: summary.run_id.clone(),
            run_name: summary.run_name.clone(),
            timestamp: summary.timestamp.clone(),
            report_html: report_html.to_string(),
            sidecar: summary.sidecar.clone(),
            key: summary.key.clone(),
            rows_a: summary.rows_a,
            rows_b: summary.rows_b,
            only_in_a: summary.only_in_a,
            only_in_b: summary.only_in_b,
            changed: summary.changed,
            matched: summary.matched,
            dup_keys_a: summary.dup_keys_a,
            dup_keys_b: summary.dup_keys_b,
            match_rate: summary.match_rate,
            pass: summary.pass,
        }
    }
}

/// Path to the manifest file within an output directory.
pub fn manifest_path(output_dir: &Path) -> PathBuf {
    output_dir.join("manifest.json")
}

/// Load the manifest, returning an empty list if it does not exist yet.
pub fn load(output_dir: &Path) -> ReconResult<Vec<ManifestEntry>> {
    let path = manifest_path(output_dir);
    if !path.exists() {
        return Ok(Vec::new());
    }
    let text = std::fs::read_to_string(&path)?;
    if text.trim().is_empty() {
        return Ok(Vec::new());
    }
    serde_json::from_str(&text)
        .map_err(|e| ReconError::Io(format!("parsing manifest: {e}")))
}

/// Append an entry to the manifest (creating it if needed).
pub fn append(output_dir: &Path, entry: ManifestEntry) -> ReconResult<Vec<ManifestEntry>> {
    std::fs::create_dir_all(output_dir)?;
    let mut entries = load(output_dir)?;
    entries.push(entry);
    let text = serde_json::to_string_pretty(&entries)
        .map_err(|e| ReconError::Io(format!("serializing manifest: {e}")))?;
    std::fs::write(manifest_path(output_dir), text)?;
    Ok(entries)
}
