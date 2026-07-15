//! `recon-report`: self-contained HTML report + Parquet sidecar reference +
//! append-only manifest + static history index (decisions 13 & 14).
//!
//! The engine writes the uncapped Parquet sidecar; this crate embeds the capped
//! samples inline in a single self-contained `.html`, appends the run to
//! `manifest.json`, and regenerates `index.html` — all viewable with no server.

pub mod assets;
pub mod index;
pub mod manifest;
pub mod report;
pub mod table;

pub use index::{regenerate_index, render_index};
pub use manifest::{ManifestEntry, load as load_manifest};
pub use report::{PublishedPaths, artifact_stem, publish, render_report};
pub use table::Table;
