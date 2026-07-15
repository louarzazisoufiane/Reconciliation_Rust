//! Regenerate the static, server-less history index (decision 14).

use std::path::Path;

use maud::{DOCTYPE, Markup, PreEscaped, html};
use recon_core::error::ReconResult;

use crate::assets;
use crate::manifest::{self, ManifestEntry};

/// Render `index.html` from the full set of manifest entries (newest first).
pub fn render_index(entries: &[ManifestEntry]) -> String {
    let mut rows: Vec<&ManifestEntry> = entries.iter().collect();
    rows.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));

    let doc = html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                title { "Reconciliation history" }
                style { (PreEscaped(assets::STYLE)) }
            }
            body {
                header {
                    h1 { "Reconciliation history" }
                    div.sub { (entries.len()) " run(s) · newest first" }
                }
                main {
                    input #q.filter placeholder="Filter runs…";
                    @if rows.is_empty() {
                        p.empty { "No runs yet." }
                    } @else {
                        div.tablewrap {
                            table #runs {
                                thead {
                                    tr {
                                        th { "When" }
                                        th { "Run" }
                                        th { "Key" }
                                        th { "Result" }
                                        th { "Changed" }
                                        th { "Only A" }
                                        th { "Only B" }
                                        th { "Matched" }
                                        th { "Match rate" }
                                        th { "Report" }
                                    }
                                }
                                tbody {
                                    @for e in &rows {
                                        (run_row(e))
                                    }
                                }
                            }
                        }
                    }
                }
                footer {
                    p { "Static index — no server required. Regenerated from manifest.json on every run." }
                }
                script { (PreEscaped(assets::INDEX_JS)) }
            }
        }
    };
    doc.into_string()
}

fn run_row(e: &ManifestEntry) -> Markup {
    let (cls, label) = if e.pass { ("pass", "PASS") } else { ("fail", "DIFF") };
    let rate = format!("{:.2}%", e.match_rate * 100.0);
    html! {
        tr {
            td { (e.timestamp) }
            td { (e.run_name) " " span.sub { (e.run_id) } }
            td { (e.key) }
            td { span.badge.(cls) { (label) } }
            td data-v=(e.changed) { (e.changed) }
            td data-v=(e.only_in_a) { (e.only_in_a) }
            td data-v=(e.only_in_b) { (e.only_in_b) }
            td data-v=(e.matched) { (e.matched) }
            td data-v=(e.match_rate) { (rate) }
            td { a href=(e.report_html) { "open" } }
        }
    }
}

/// Load the manifest from `output_dir` and (re)write `index.html`.
pub fn regenerate_index(output_dir: &Path) -> ReconResult<()> {
    let entries = manifest::load(output_dir)?;
    let html = render_index(&entries);
    std::fs::create_dir_all(output_dir)?;
    std::fs::write(output_dir.join("index.html"), html)?;
    Ok(())
}
