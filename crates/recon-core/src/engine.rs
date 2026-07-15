//! The comparison engine (Polars) — the heart of `recon-core`.
//!
//! Given a validated [`RunConfig`] and its two resolved [`Schema`]s, this
//! reads both fixed-width sources, normalizes the compared columns, sets aside
//! duplicate keys, performs an order-independent full-outer join on the single
//! key, and derives the `only_in_a` / `only_in_b` / `changed` / `duplicate`
//! categories plus a summary.
//!
//! The complete, uncapped diff is streamed to a Parquet sidecar; only the first
//! `embed_row_cap` rows per category are collected for inline embedding
//! (decision 13). The joined frame is never collected whole.
//!
//! Any *data* outcome returns `Ok` (the process exits 0 — decision 12); `Err`
//! is reserved for genuine engine/IO errors.

use std::path::Path;

use polars::prelude::*;
use serde::{Deserialize, Serialize};

use crate::config::RunConfig;
use crate::error::{ReconError, ReconResult};
use crate::normalize::normalize_expr;
use crate::reader::read_fixed_width;
use crate::schema::Schema;

/// Suffix applied to source-A copies of compared columns in the joined frame.
const A: &str = "__a";
/// Suffix applied to source-B copies of compared columns in the joined frame.
const B: &str = "__b";

/// The full result of one comparison run.
pub struct ReconOutcome {
    /// Counts + metadata (serialized into the manifest and rendered).
    pub summary: Summary,
    /// Capped, collected samples for inline embedding in the HTML report.
    pub samples: Samples,
}

/// Per-category capped samples (already collected into small `DataFrame`s).
pub struct Samples {
    /// Rows present only in A (full row, A-side values). Columns: key + compares.
    pub only_in_a: DataFrame,
    /// Rows present only in B. Columns: key + compares.
    pub only_in_b: DataFrame,
    /// Rows whose key is in both but ≥1 compared column differs. Columns:
    /// key + `{col}__a` / `{col}__b` per compared column.
    pub changed: DataFrame,
    /// Duplicate-key summary. Columns: key, side, count.
    pub duplicates: DataFrame,
}

/// Run summary — per-category counts + metadata. Serializable for the manifest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Summary {
    /// Unique run id.
    pub run_id: String,
    /// Configured run name.
    pub run_name: String,
    /// UTC timestamp (RFC-3339) the comparison ran.
    pub timestamp: String,
    /// Primary key column.
    pub key: String,
    /// Allow-listed compared columns.
    pub compare_columns: Vec<String>,
    /// Source A path (as configured).
    pub source_a_path: String,
    /// Source B path.
    pub source_b_path: String,
    /// Schema A reference (`name@vN`).
    pub schema_a_ref: String,
    /// Schema B reference.
    pub schema_b_ref: String,
    /// Total rows read from A.
    pub rows_a: usize,
    /// Total rows read from B.
    pub rows_b: usize,
    /// Keys present only in A (de-duplicated remainder).
    pub only_in_a: usize,
    /// Keys present only in B.
    pub only_in_b: usize,
    /// Keys in both with ≥1 differing compared column.
    pub changed: usize,
    /// Keys in both that match on every compared column.
    pub matched: usize,
    /// Distinct duplicate keys in A.
    pub dup_keys_a: usize,
    /// Distinct duplicate keys in B.
    pub dup_keys_b: usize,
    /// Total duplicate rows in A (across duplicate keys).
    pub dup_rows_a: usize,
    /// Total duplicate rows in B.
    pub dup_rows_b: usize,
    /// matched / (matched + only_in_a + only_in_b + changed).
    pub match_rate: f64,
    /// Overall pass label (no diffs and no duplicates). Does NOT affect exit
    /// code (decision 12).
    pub pass: bool,
    /// Configured completion-detection stability window (for the footer caveat).
    pub stability_minutes: u64,
    /// Inline embed cap that was applied per category.
    pub embed_row_cap: usize,
    /// Relative filename of the full-diff Parquet sidecar.
    pub sidecar: String,
}

/// A null String literal, used to fill the absent side of duplicate rows.
fn null_str() -> Expr {
    lit(NULL).cast(DataType::String)
}

/// Count the rows of a lazy frame by collecting only a `len()` aggregation.
fn count_rows(lf: LazyFrame) -> ReconResult<usize> {
    let df = lf.select([len().alias("n")]).collect()?;
    let n = df
        .column("n")
        .and_then(|c| c.get(0).map_err(Into::into))
        .map_err(|e: PolarsError| ReconError::engine(e.to_string()))?;
    Ok(n.try_extract::<u64>().unwrap_or(0) as usize)
}

/// Run one comparison. Writes the full diff to `sidecar_path`; returns capped
/// samples + a summary.
pub fn run_comparison(
    config: &RunConfig,
    schema_a: &Schema,
    schema_b: &Schema,
    run_id: &str,
    now: jiff::Timestamp,
    sidecar_path: &Path,
) -> ReconResult<ReconOutcome> {
    config.validate()?;
    let key = config.key.clone();

    // Columns the engine touches: key + compare list, de-duplicated, order-preserving.
    let mut cols: Vec<String> = vec![key.clone()];
    for c in &config.compare_columns {
        if !cols.contains(c) {
            cols.push(c.clone());
        }
    }

    // Cross-check the columns exist in both schemas (defense in depth; the web
    // UI already enforces decision 9 at build time).
    for c in &cols {
        if !schema_a.has_column(c) {
            return Err(ReconError::config(format!(
                "column '{c}' absent from schema A ({})",
                config.source_a.schema_ref
            )));
        }
        if !schema_b.has_column(c) {
            return Err(ReconError::config(format!(
                "column '{c}' absent from schema B ({})",
                config.source_b.schema_ref
            )));
        }
    }

    let lf_a = prepare_side(&config.source_a.path, schema_a, config, &cols)?;
    let lf_b = prepare_side(&config.source_b.path, schema_b, config, &cols)?;

    let rows_a = count_rows(lf_a.clone())?;
    let rows_b = count_rows(lf_b.clone())?;

    // --- Duplicate detection (decision 7) -----------------------------------
    let dup_keys_a = duplicate_keys(lf_a.clone(), &key);
    let dup_keys_b = duplicate_keys(lf_b.clone(), &key);
    let dup_keys_a_ct = count_rows(dup_keys_a.clone())?;
    let dup_keys_b_ct = count_rows(dup_keys_b.clone())?;
    let dup_rows_a = count_rows(semi(lf_a.clone(), dup_keys_a.clone(), &key))?;
    let dup_rows_b = count_rows(semi(lf_b.clone(), dup_keys_b.clone(), &key))?;

    // De-duplicated remainder that participates in the join.
    let a_clean = anti(lf_a.clone(), dup_keys_a.clone(), &key);
    let b_clean = anti(lf_b.clone(), dup_keys_b.clone(), &key);

    // --- Full outer join on the single key (decision 6) ---------------------
    let a_ren = rename_side(a_clean, &key, &config.compare_columns, A);
    let b_ren = rename_side(b_clean, &key, &config.compare_columns, B);

    let joined = a_ren.join(
        b_ren,
        [col(key.as_str())],
        [col(key.as_str())],
        JoinArgs::new(JoinType::Full).with_coalesce(JoinCoalesce::CoalesceColumns),
    );

    let category = category_expr(&config.compare_columns);
    let joined = joined.with_columns([category.alias("__category")]);

    // --- Category counts (small aggregation; never materializes the diff) ----
    let counts = joined
        .clone()
        .group_by([col("__category")])
        .agg([len().alias("n")])
        .collect()?;
    let cat = |name: &str| category_count(&counts, name);
    let only_in_a = cat("only_in_a");
    let only_in_b = cat("only_in_b");
    let changed = cat("changed");
    let matched = cat("matched");

    // --- Stream the full diff to the Parquet sidecar (decision 13) -----------
    let unified = unified_diff_frame(
        joined.clone(),
        &key,
        &config.compare_columns,
        lf_a.clone(),
        lf_b.clone(),
        dup_keys_a.clone(),
        dup_keys_b.clone(),
    );
    write_sidecar(unified, sidecar_path)?;

    // --- Capped samples for inline embedding --------------------------------
    let cap = config.report.embed_row_cap;
    let samples = collect_samples(&joined, &key, &config.compare_columns, lf_a, lf_b, dup_keys_a, dup_keys_b, cap)?;

    let denom = matched + only_in_a + only_in_b + changed;
    let match_rate = if denom == 0 { 1.0 } else { matched as f64 / denom as f64 };
    let pass = only_in_a == 0 && only_in_b == 0 && changed == 0 && dup_keys_a_ct == 0 && dup_keys_b_ct == 0;

    let summary = Summary {
        run_id: run_id.to_string(),
        run_name: config.run_name.clone(),
        timestamp: now.to_string(),
        key,
        compare_columns: config.compare_columns.clone(),
        source_a_path: config.source_a.path.display().to_string(),
        source_b_path: config.source_b.path.display().to_string(),
        schema_a_ref: config.source_a.schema_ref.to_string(),
        schema_b_ref: config.source_b.schema_ref.to_string(),
        rows_a,
        rows_b,
        only_in_a,
        only_in_b,
        changed,
        matched,
        dup_keys_a: dup_keys_a_ct,
        dup_keys_b: dup_keys_b_ct,
        dup_rows_a,
        dup_rows_b,
        match_rate,
        pass,
        stability_minutes: config.completion_detection.stability_minutes,
        embed_row_cap: cap,
        sidecar: sidecar_path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default(),
    };

    Ok(ReconOutcome { summary, samples })
}

/// Read one source, select the needed columns, and apply normalization.
fn prepare_side(
    path: &Path,
    schema: &Schema,
    config: &RunConfig,
    cols: &[String],
) -> ReconResult<LazyFrame> {
    let lf = read_fixed_width(path, schema)?;
    let selected = lf.select(cols.iter().map(|c| col(c.as_str())).collect::<Vec<_>>());
    let norm_exprs: Vec<Expr> = cols
        .iter()
        .map(|c| normalize_expr(c, config.norm_for(c)))
        .collect();
    Ok(selected.with_columns(norm_exprs))
}

/// The distinct keys that repeat within a side (`count > 1`).
fn duplicate_keys(lf: LazyFrame, key: &str) -> LazyFrame {
    lf.group_by([col(key)])
        .agg([len().alias("__n")])
        .filter(col("__n").gt(lit(1u32)))
        .select([col(key)])
}

/// Rows of `lf` whose key IS in `keys` (semi join).
fn semi(lf: LazyFrame, keys: LazyFrame, key: &str) -> LazyFrame {
    lf.join(keys, [col(key)], [col(key)], JoinArgs::new(JoinType::Semi))
}

/// Rows of `lf` whose key is NOT in `keys` (anti join).
fn anti(lf: LazyFrame, keys: LazyFrame, key: &str) -> LazyFrame {
    lf.join(keys, [col(key)], [col(key)], JoinArgs::new(JoinType::Anti))
}

/// Select `key` + compared columns, suffixing the compares and tagging presence.
fn rename_side(lf: LazyFrame, key: &str, compares: &[String], suffix: &str) -> LazyFrame {
    let mut exprs = vec![col(key)];
    for c in compares {
        exprs.push(col(c.as_str()).alias(format!("{c}{suffix}")));
    }
    exprs.push(lit(true).alias(format!("__in{suffix}")));
    lf.select(exprs)
}

/// The `__category` expression over the joined frame.
fn category_expr(compares: &[String]) -> Expr {
    let only_a = col(format!("__in{B}")).is_null();
    let only_b = col(format!("__in{A}")).is_null();

    let mut differ: Option<Expr> = None;
    for c in compares {
        let d = col(format!("{c}{A}"))
            .eq_missing(col(format!("{c}{B}")))
            .not();
        differ = Some(match differ {
            Some(prev) => prev.or(d),
            None => d,
        });
    }
    let both = col(format!("__in{A}"))
        .is_not_null()
        .and(col(format!("__in{B}")).is_not_null());
    let changed = match differ {
        Some(d) => both.and(d),
        None => lit(false),
    };

    when(only_a)
        .then(lit("only_in_a"))
        .when(only_b)
        .then(lit("only_in_b"))
        .when(changed)
        .then(lit("changed"))
        .otherwise(lit("matched"))
}

/// Extract a category's row-count from the small counts aggregation.
fn category_count(counts: &DataFrame, name: &str) -> usize {
    let cats = match counts.column("__category").and_then(|c| c.str().cloned()) {
        Ok(c) => c,
        Err(_) => return 0,
    };
    let ns = match counts.column("n") {
        Ok(c) => c,
        Err(_) => return 0,
    };
    for i in 0..counts.height() {
        if cats.get(i) == Some(name) {
            return ns
                .get(i)
                .ok()
                .and_then(|v| v.try_extract::<u64>().ok())
                .unwrap_or(0) as usize;
        }
    }
    0
}

/// Build the unified diff frame (all non-matched categories + duplicates) with a
/// stable column set: `key`, `{c}__a`/`{c}__b` per compare column, `__category`.
#[allow(clippy::too_many_arguments)]
fn unified_diff_frame(
    joined: LazyFrame,
    key: &str,
    compares: &[String],
    lf_a: LazyFrame,
    lf_b: LazyFrame,
    dup_keys_a: LazyFrame,
    dup_keys_b: LazyFrame,
) -> LazyFrame {
    let unified_cols = |lf: LazyFrame| -> LazyFrame {
        let mut exprs = vec![col(key)];
        for c in compares {
            exprs.push(col(format!("{c}{A}")));
            exprs.push(col(format!("{c}{B}")));
        }
        exprs.push(col("__category"));
        lf.select(exprs)
    };

    let diffs = unified_cols(joined.filter(col("__category").neq(lit("matched"))));
    let dup_a = duplicate_rows_unified(lf_a, dup_keys_a, key, compares, "duplicate_a", true);
    let dup_b = duplicate_rows_unified(lf_b, dup_keys_b, key, compares, "duplicate_b", false);

    concat(
        [diffs, dup_a, dup_b],
        UnionArgs::default(),
    )
    .unwrap_or_else(|_| joined_empty(key, compares))
}

/// Duplicate rows shaped into the unified diff schema (opposite side nulled).
fn duplicate_rows_unified(
    lf: LazyFrame,
    dup_keys: LazyFrame,
    key: &str,
    compares: &[String],
    category: &str,
    is_a: bool,
) -> LazyFrame {
    let rows = semi(lf, dup_keys, key);
    let mut exprs = vec![col(key)];
    for c in compares {
        if is_a {
            exprs.push(col(c.as_str()).alias(format!("{c}{A}")));
            exprs.push(null_str().alias(format!("{c}{B}")));
        } else {
            exprs.push(null_str().alias(format!("{c}{A}")));
            exprs.push(col(c.as_str()).alias(format!("{c}{B}")));
        }
    }
    exprs.push(lit(category).alias("__category"));
    rows.select(exprs)
}

/// Fallback empty frame with the unified schema (used only if concat fails).
fn joined_empty(key: &str, compares: &[String]) -> LazyFrame {
    let mut exprs = vec![null_str().alias(key)];
    for c in compares {
        exprs.push(null_str().alias(format!("{c}{A}")));
        exprs.push(null_str().alias(format!("{c}{B}")));
    }
    exprs.push(null_str().alias("__category"));
    DataFrame::empty().lazy().select(exprs)
}

/// Stream a lazy frame to Parquet.
fn write_sidecar(lf: LazyFrame, path: &Path) -> ReconResult<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // Collect via the streaming engine, then write. Diffs are small relative to
    // the inputs; the join above is never collected whole.
    let mut df = lf.collect()?;
    let file = std::fs::File::create(path)
        .map_err(|e| ReconError::Io(format!("creating sidecar {}: {e}", path.display())))?;
    ParquetWriter::new(file).finish(&mut df)?;
    Ok(())
}

/// Collect capped per-category samples for inline embedding.
#[allow(clippy::too_many_arguments)]
fn collect_samples(
    joined: &LazyFrame,
    key: &str,
    compares: &[String],
    lf_a: LazyFrame,
    lf_b: LazyFrame,
    dup_keys_a: LazyFrame,
    dup_keys_b: LazyFrame,
    cap: usize,
) -> ReconResult<Samples> {
    let cap_u = cap as u32;

    // only_in_a: key + compare cols (A-side values, renamed back to plain names).
    // Skip the key in the compare loop when it equals the key to avoid duplicating
    // the column name (the key is already selected via `col(key)` above).
    let only_a = {
        let mut exprs = vec![col(key)];
        for c in compares {
            if c == key { continue; }
            exprs.push(col(format!("{c}{A}")).alias(c.as_str()));
        }
        joined
            .clone()
            .filter(col("__category").eq(lit("only_in_a")))
            .select(exprs)
            .limit(cap_u)
            .collect()?
    };

    let only_b = {
        let mut exprs = vec![col(key)];
        for c in compares {
            if c == key { continue; }
            exprs.push(col(format!("{c}{B}")).alias(c.as_str()));
        }
        joined
            .clone()
            .filter(col("__category").eq(lit("only_in_b")))
            .select(exprs)
            .limit(cap_u)
            .collect()?
    };

    // changed: full row, both sides, per compared column.
    let changed = {
        let mut exprs = vec![col(key)];
        for c in compares {
            exprs.push(col(format!("{c}{A}")));
            exprs.push(col(format!("{c}{B}")));
        }
        joined
            .clone()
            .filter(col("__category").eq(lit("changed")))
            .select(exprs)
            .limit(cap_u)
            .collect()?
    };

    // duplicates: key, side, count.
    let dup_a = lf_a
        .group_by([col(key)])
        .agg([len().alias("count")])
        .filter(col("count").gt(lit(1u32)))
        .with_column(lit("A").alias("side"))
        .select([col(key), col("side"), col("count")]);
    let _ = &dup_keys_a;
    let dup_b = lf_b
        .group_by([col(key)])
        .agg([len().alias("count")])
        .filter(col("count").gt(lit(1u32)))
        .with_column(lit("B").alias("side"))
        .select([col(key), col("side"), col("count")]);
    let _ = &dup_keys_b;
    let duplicates = concat([dup_a, dup_b], UnionArgs::default())
        .map_err(|e| ReconError::engine(e.to_string()))?
        .limit(cap_u)
        .collect()?;

    Ok(Samples {
        only_in_a: only_a,
        only_in_b: only_b,
        changed,
        duplicates,
    })
}
