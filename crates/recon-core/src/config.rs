//! The per-source-pair run configuration (decisions 6–10, CONFIGURATION section).
//!
//! Deserialized into strongly-typed structs with `deny_unknown_fields` so an
//! unknown key is a hard error (fail closed). A run config references its two
//! schemas by name + version rather than inlining field layouts.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::schema::SchemaRef;

/// A full run configuration for one source pair.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunConfig {
    /// Human-readable run name (appears in report + manifest).
    pub run_name: String,
    /// The single primary key column (decision 6).
    pub key: String,
    /// Duplicate-key handling; only `report_continue` is supported (decision 7).
    #[serde(default)]
    pub duplicate_policy: DuplicatePolicy,
    /// Explicit allow-list of columns to compare (decision 8).
    pub compare_columns: Vec<String>,
    /// Per-column normalization toggles (decision 10). Columns absent here use
    /// the defaults from [`Normalization::resolved`].
    #[serde(default)]
    pub normalization: BTreeMap<String, Normalization>,
    /// Source A: landing path + schema reference.
    pub source_a: SourceConfig,
    /// Source B: landing path + schema reference (may equal A's).
    pub source_b: SourceConfig,
    /// Report / sidecar output settings.
    pub report: ReportConfig,
    /// Completion-detection settings (decision 11).
    #[serde(default)]
    pub completion_detection: CompletionDetection,
}

/// A single source: where the fixed-width file lands and which schema decodes it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceConfig {
    /// Landing path of the fixed-width file.
    pub path: PathBuf,
    /// Reference to the schema (name + version) used to decode this file.
    pub schema_ref: SchemaRef,
}

/// Duplicate-key policy. Only report-and-continue is in scope (decision 7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DuplicatePolicy {
    /// Detect duplicates, set them aside, and compare the de-duplicated
    /// remainder.
    #[default]
    ReportContinue,
}

/// Per-column normalization toggles (decision 10). Each is `Option`-typed so it
/// can be individually disabled; `None` falls back to the per-toggle default.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Normalization {
    /// Trim surrounding whitespace (default ON — essential for padded fields).
    pub trim: Option<bool>,
    /// Strip leading zeros on numeric fields, e.g. `00042` -> `42` (default OFF).
    pub strip_leading_zeros: Option<bool>,
    /// Collapse NULL / empty / all-spaces / low-values to one canonical empty
    /// (default OFF).
    pub unify_null: Option<bool>,
    /// Case-fold to lowercase (default OFF; enable for names/text).
    pub case_fold: Option<bool>,
}

/// Normalization toggles with every `Option` resolved to a concrete `bool`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolvedNorm {
    /// See [`Normalization::trim`].
    pub trim: bool,
    /// See [`Normalization::strip_leading_zeros`].
    pub strip_leading_zeros: bool,
    /// See [`Normalization::unify_null`].
    pub unify_null: bool,
    /// See [`Normalization::case_fold`].
    pub case_fold: bool,
}

impl Normalization {
    /// Resolve toggles to concrete booleans using the documented defaults
    /// (`trim` OFF, everything else OFF).
    pub fn resolved(&self) -> ResolvedNorm {
        ResolvedNorm {
            trim: self.trim.unwrap_or(false),
            strip_leading_zeros: self.strip_leading_zeros.unwrap_or(false),
            unify_null: self.unify_null.unwrap_or(false),
            case_fold: self.case_fold.unwrap_or(false),
        }
    }
}

impl Default for ResolvedNorm {
    fn default() -> Self {
        Normalization::default().resolved()
    }
}

fn default_embed_row_cap() -> usize {
    5000
}

/// Report / sidecar output settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReportConfig {
    /// Max rows per category embedded inline in the HTML (decision 13).
    #[serde(default = "default_embed_row_cap")]
    pub embed_row_cap: usize,
    /// Sidecar format; only `parquet` is supported.
    #[serde(default)]
    pub sidecar_format: SidecarFormat,
    /// Directory that reports + sidecars + manifest live under.
    pub output_dir: PathBuf,
}

/// Full-diff sidecar format (decision 13).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SidecarFormat {
    /// Streamed Parquet (the complete, uncapped diff).
    #[default]
    Parquet,
}

fn default_stability_minutes() -> u64 {
    5
}

/// Completion-detection settings (decision 11).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompletionDetection {
    /// Detection method; only `size_stability` is supported today.
    #[serde(default)]
    pub method: CompletionMethod,
    /// Minutes a file's size must be unchanged before it is considered written.
    #[serde(default = "default_stability_minutes")]
    pub stability_minutes: u64,
}

impl Default for CompletionDetection {
    fn default() -> Self {
        CompletionDetection {
            method: CompletionMethod::default(),
            stability_minutes: default_stability_minutes(),
        }
    }
}

/// Completion-detection method (decision 11). Marker-file / control-table
/// strategies are [FUTURE] behind the `CompletionDetector` trait.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompletionMethod {
    /// Existence + file-size stability over a window.
    #[default]
    SizeStability,
}

impl RunConfig {
    /// Parse a run config from YAML text.
    pub fn from_yaml(text: &str) -> crate::error::ReconResult<Self> {
        serde_norway::from_str(text)
            .map_err(|e| crate::error::ReconError::config(format!("run config: {e}")))
    }

    /// Resolve normalization for a column, applying defaults when unspecified.
    pub fn norm_for(&self, column: &str) -> ResolvedNorm {
        self.normalization
            .get(column)
            .map(Normalization::resolved)
            .unwrap_or_default()
    }

    /// Structural validation independent of the schemas (decision 8/6).
    ///
    /// Cross-schema column-existence checks (decision 9) live in `recon-schema`
    /// / the web UI where the resolved schemas are available.
    pub fn validate(&self) -> crate::error::ReconResult<()> {
        use crate::error::ReconError;
        if self.compare_columns.is_empty() {
            return Err(ReconError::config("compare_columns must not be empty"));
        }
        // The key need not appear in `compare_columns`, but it must never be
        // absent from the pipeline; that is validated against schemas elsewhere.
        // Normalization keys must reference real compare columns or the key.
        for col in self.normalization.keys() {
            if col != &self.key && !self.compare_columns.contains(col) {
                return Err(ReconError::config(format!(
                    "normalization references unknown column '{col}' (not the key or a compare column)"
                )));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
run_name: customers_daily_recon
key: customer_id
duplicate_policy: report_continue
compare_columns: [customer_id, name, city, balance]
normalization:
  name:        { trim: true, case_fold: true }
  balance:     { trim: true, strip_leading_zeros: true }
  customer_id: { trim: true }
source_a:
  path: /landing/a/customers.txt
  schema_ref: { name: customers_layout, version: 3 }
source_b:
  path: /landing/b/customers.txt
  schema_ref: { name: customers_layout, version: 3 }
report:
  embed_row_cap: 5000
  sidecar_format: parquet
  output_dir: reports/
completion_detection:
  method: size_stability
  stability_minutes: 5
"#;

    #[test]
    fn parses_sample() {
        let cfg = RunConfig::from_yaml(SAMPLE).unwrap();
        assert_eq!(cfg.key, "customer_id");
        assert_eq!(cfg.compare_columns.len(), 4);
        assert_eq!(cfg.source_a.schema_ref.version, 3);
        cfg.validate().unwrap();
    }

    #[test]
    fn unknown_key_rejected() {
        let bad = format!("{SAMPLE}\nsurprise: 1\n");
        assert!(RunConfig::from_yaml(&bad).is_err());
    }

    #[test]
    fn norm_defaults() {
        let cfg = RunConfig::from_yaml(SAMPLE).unwrap();
        // Unlisted column -> trim on, others off.
        let city_norm = cfg.norm_for("nonexistent");
        assert!(city_norm.trim);
        assert!(!city_norm.case_fold);
        // Listed column.
        assert!(cfg.norm_for("name").case_fold);
        assert!(cfg.norm_for("balance").strip_leading_zeros);
    }
}
