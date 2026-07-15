//! Render + write the self-contained per-run HTML report (decision 13).

use std::path::{Path, PathBuf};

use maud::{DOCTYPE, Markup, PreEscaped, html};
use recon_core::engine::{ReconOutcome, Summary};
use recon_core::error::{ReconError, ReconResult};
use serde_json::json;

use crate::assets;
use crate::manifest::{self, ManifestEntry};
use crate::table::Table;

/// Filenames produced by [`publish`].
#[derive(Debug, Clone)]
pub struct PublishedPaths {
    /// Absolute path to the written HTML report.
    pub report_html: PathBuf,
    /// Absolute path to the regenerated index.
    pub index_html: PathBuf,
}

/// Compute the shared artifact stem `YYYY-MM-DD_HHMMSS_<runid>` from an
/// RFC-3339 timestamp. Used for BOTH the `.html` report and `.parquet` sidecar
/// so their names line up.
pub fn artifact_stem(timestamp: &str, run_id: &str) -> String {
    let formatted = timestamp
        .parse::<jiff::Timestamp>()
        .ok()
        .map(|ts| ts.strftime("%Y-%m-%d_%H%M%S").to_string())
        .unwrap_or_else(|| "unknown-time".to_string());
    format!("{formatted}_{run_id}")
}

/// Escape a JSON string for safe inlining inside a `<script>` element.
fn escape_script_json(s: &str) -> String {
    s.replace("</", "<\\/")
}

/// Build the inline data island (summary + capped tables) as a JSON string.
fn data_island(summary: &Summary, outcome: &ReconOutcome) -> ReconResult<String> {
    let tables = json!({
        "only_in_a": Table::from_df(&outcome.samples.only_in_a),
        "only_in_b": Table::from_df(&outcome.samples.only_in_b),
        "changed": Table::from_df(&outcome.samples.changed),
        "duplicates": Table::from_df(&outcome.samples.duplicates),
    });
    let root = json!({ "summary": summary, "tables": tables });
    serde_json::to_string(&root)
        .map(|s| escape_script_json(&s))
        .map_err(|e| ReconError::Io(format!("serializing report data: {e}")))
}

fn stat_card(label: &str, n: usize) -> Markup {
    html! {
        div.card {
            div.n { (n) }
            div.l { (label) }
        }
    }
}

/// Render the complete self-contained HTML document.
pub fn render_report(summary: &Summary, outcome: &ReconOutcome) -> ReconResult<String> {
    let island = data_island(summary, outcome)?;
    let pct = format!("{:.2}%", summary.match_rate * 100.0);
    let (badge_class, badge_text) = if summary.pass {
        ("pass", "PASS")
    } else {
        ("fail", "DIFFERENCES")
    };

    let doc = html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                title { "Recon — " (summary.run_name) }
                style { (PreEscaped(assets::STYLE)) }
            }
            body {
                header {
                    h1 { (summary.run_name) " " span.badge.(badge_class) { (badge_text) } }
                    div.sub { "run " (summary.run_id) " · " (summary.timestamp) }
                }
                main {
                    div.cards {
                        (stat_card("Rows A", summary.rows_a))
                        (stat_card("Rows B", summary.rows_b))
                        (stat_card("Matched", summary.matched))
                        (stat_card("Changed", summary.changed))
                        (stat_card("Only in A", summary.only_in_a))
                        (stat_card("Only in B", summary.only_in_b))
                        div.card {
                            div.n { (pct) }
                            div.l { "Match rate" }
                        }
                    }

                    dl.meta {
                        dt { "Key" } dd { (summary.key) }
                        dt { "Compared columns" } dd { (summary.compare_columns.join(", ")) }
                        dt { "Source A" } dd { (summary.source_a_path) " (" (summary.schema_a_ref) ")" }
                        dt { "Source B" } dd { (summary.source_b_path) " (" (summary.schema_b_ref) ")" }
                        dt { "Duplicates A" } dd { (summary.dup_keys_a) " keys / " (summary.dup_rows_a) " rows" }
                        dt { "Duplicates B" } dd { (summary.dup_keys_b) " keys / " (summary.dup_rows_b) " rows" }
                        dt { "Full diff" } dd { a href=(summary.sidecar) { (summary.sidecar) } " (uncapped Parquet sidecar)" }
                        dt { "Embed cap" } dd { (summary.embed_row_cap) " rows/category" }
                    }

                    h2 { "Changed " span.count { "(" (summary.changed) ")" } }
                    div #t-changed {}

                    h2 { "Only in A " span.count { "(" (summary.only_in_a) ")" } }
                    div #t-only-a {}

                    h2 { "Only in B " span.count { "(" (summary.only_in_b) ")" } }
                    div #t-only-b {}

                    h2 { "Duplicate keys " span.count { "(A: " (summary.dup_keys_a) ", B: " (summary.dup_keys_b) ")" } }
                    div #t-duplicates {}
                }
                footer {
                    p {
                        "Completion detection: file-size stability over "
                        (summary.stability_minutes)
                        " min. CAVEAT — a writer stalled longer than this window mid-write can look \"done\" and cause a partial-file comparison."
                    }
                    p {
                        "Embedded rows are capped at " (summary.embed_row_cap)
                        " per category; the linked Parquet sidecar holds the complete, uncapped diff."
                    }
                }
                script #recon-data type="application/json" { (PreEscaped(island)) }
                script { (PreEscaped(assets::REPORT_JS)) }
            }
        }
    };
    Ok(doc.into_string())
}

/// Render + write the report, append the manifest, and regenerate the index.
///
/// The Parquet sidecar is written by the engine; this only references it.
pub fn publish(output_dir: &Path, outcome: &ReconOutcome) -> ReconResult<PublishedPaths> {
    std::fs::create_dir_all(output_dir)?;
    let summary = &outcome.summary;
    let stem = artifact_stem(&summary.timestamp, &summary.run_id);
    let html_name = format!("{stem}.html");
    let html_path = output_dir.join(&html_name);

    let doc = render_report(summary, outcome)?;
    std::fs::write(&html_path, doc)?;

    let entry = ManifestEntry::from_summary(summary, &html_name);
    let entries = manifest::append(output_dir, entry)?;

    let index_path = output_dir.join("index.html");
    let index_html = crate::index::render_index(&entries);
    std::fs::write(&index_path, index_html)?;

    Ok(PublishedPaths {
        report_html: html_path,
        index_html: index_path,
    })
}
