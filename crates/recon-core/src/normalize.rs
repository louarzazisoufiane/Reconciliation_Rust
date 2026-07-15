//! Per-column normalization as vectorized Polars string expressions (decision 10).
//!
//! Raw string equality on fixed-width data yields massive false diffs from
//! padding, leading zeros, null representation, and casing. Normalization is
//! what makes "exact equality" usable. Every toggle is expressed as a Polars
//! expression — never a per-row Rust closure — so it runs vectorized/streaming.
//!
//! Application order (each step optional): `unify_null` → `trim` →
//! `strip_leading_zeros` → `case_fold`.

use polars::prelude::*;

use crate::config::ResolvedNorm;

/// Characters treated as padding/whitespace when trimming or detecting
/// null-like values (spaces, tabs, CR/LF, and mainframe low-values).
const PAD_CHARS: &str = " \t\r\n\u{0}";

/// Build the normalization expression for `column` under the resolved toggles.
///
/// The returned `Expr` is aliased back to `column` so it can be dropped into a
/// `with_columns` call.
pub fn normalize_expr(column: &str, n: ResolvedNorm) -> Expr {
    let mut e = col(column);

    if n.unify_null {
        // Strip padding/low-values to a "core"; map empty or literal NULL to "".
        let core = e.clone().str().strip_chars(lit(PAD_CHARS));
        let is_null_like = core
            .clone()
            .eq(lit(""))
            .or(core.str().to_uppercase().eq(lit("NULL")));
        e = when(is_null_like).then(lit("")).otherwise(e);
    }

    if n.trim {
        e = e.str().strip_chars(lit(PAD_CHARS));
    }

    if n.strip_leading_zeros {
        let stripped = e.clone().str().strip_chars_start(lit("0"));
        // Preserve a single "0" for an all-zeros field so it does not vanish.
        e = when(
            stripped
                .clone()
                .eq(lit(""))
                .and(e.clone().neq(lit(""))),
        )
        .then(lit("0"))
        .otherwise(stripped);
    }

    if n.case_fold {
        e = e.str().to_lowercase();
    }

    e.alias(column)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn norm(values: &[&str], n: ResolvedNorm) -> Vec<String> {
        let s: Vec<String> = values.iter().map(|v| v.to_string()).collect();
        let df = DataFrame::new(vec![Column::new("v".into(), s)]).unwrap();
        let out = df
            .lazy()
            .with_columns([normalize_expr("v", n)])
            .collect()
            .unwrap();
        out.column("v")
            .unwrap()
            .str()
            .unwrap()
            .into_iter()
            .map(|o| o.unwrap_or("").to_string())
            .collect()
    }

    fn off() -> ResolvedNorm {
        ResolvedNorm {
            trim: false,
            strip_leading_zeros: false,
            unify_null: false,
            case_fold: false,
        }
    }

    #[test]
    fn trim_toggle() {
        let n = ResolvedNorm { trim: true, ..off() };
        assert_eq!(norm(&["  hi  ", "x"], n), vec!["hi", "x"]);
        // Off => padding preserved.
        assert_eq!(norm(&["  hi  "], off()), vec!["  hi  "]);
    }

    #[test]
    fn strip_leading_zeros_toggle() {
        let n = ResolvedNorm { strip_leading_zeros: true, ..off() };
        assert_eq!(norm(&["00042", "0", "00000", "100"], n), vec!["42", "0", "0", "100"]);
        assert_eq!(norm(&["00042"], off()), vec!["00042"]);
    }

    #[test]
    fn unify_null_toggle() {
        let n = ResolvedNorm { unify_null: true, ..off() };
        assert_eq!(norm(&["NULL", "   ", "\u{0}\u{0}", "", "keep"], n), vec!["", "", "", "", "keep"]);
    }

    #[test]
    fn case_fold_toggle() {
        let n = ResolvedNorm { case_fold: true, ..off() };
        assert_eq!(norm(&["ABc"], n), vec!["abc"]);
        assert_eq!(norm(&["ABc"], off()), vec!["ABc"]);
    }

    #[test]
    fn combined_padded_leading_zero() {
        let n = ResolvedNorm { trim: true, strip_leading_zeros: true, ..off() };
        assert_eq!(norm(&[" 00042 "], n), vec!["42"]);
    }
}
