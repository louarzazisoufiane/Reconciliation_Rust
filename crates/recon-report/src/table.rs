//! Convert a (capped) Polars `DataFrame` into a JSON-friendly table for
//! inline embedding in the self-contained HTML report.

use polars::prelude::*;
use serde::Serialize;

/// A small table ready to be embedded as inline JSON.
#[derive(Debug, Clone, Serialize)]
pub struct Table {
    /// Column headers in order.
    pub columns: Vec<String>,
    /// Row-major cell values; `null` for absent cells.
    pub rows: Vec<Vec<Option<String>>>,
}

impl Table {
    /// Build a [`Table`] from a `DataFrame`, stringifying every cell.
    pub fn from_df(df: &DataFrame) -> Table {
        let columns: Vec<String> = df
            .get_column_names()
            .iter()
            .map(|s| s.to_string())
            .collect();

        let cols: Vec<&Column> = df.get_columns().iter().collect();
        let height = df.height();
        let mut rows = Vec::with_capacity(height);
        for i in 0..height {
            let mut row = Vec::with_capacity(cols.len());
            for c in &cols {
                row.push(match c.get(i) {
                    Ok(av) => stringify(av),
                    Err(_) => None,
                });
            }
            rows.push(row);
        }
        Table { columns, rows }
    }

    /// Number of rows in the (capped) table.
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// Whether the table has no rows.
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}

/// Render an `AnyValue` as an optional owned string (`None` == SQL null).
fn stringify(av: AnyValue) -> Option<String> {
    match av {
        AnyValue::Null => None,
        AnyValue::String(s) => Some(s.to_string()),
        AnyValue::StringOwned(s) => Some(s.to_string()),
        other => Some(other.to_string()),
    }
}
