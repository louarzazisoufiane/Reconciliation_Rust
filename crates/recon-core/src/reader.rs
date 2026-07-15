//! The single fixed-width reader (decisions 3 & 4).
//!
//! This is the ONLY reader in the system — there is no CSV/Parquet/JDBC fan-out.
//! Fields are sliced by BYTE offset (`[start, start + length)` in the schema's
//! `index_base`). Bytes are decoded lossily as UTF-8.
//!
//! ## UTF-8 caveat
//! Offsets are byte offsets, not character offsets. For pure ASCII (the common
//! mainframe case) byte == char. For multi-byte UTF-8 a field boundary that
//! falls mid-codepoint will decode via [`String::from_utf8_lossy`] and may
//! introduce a replacement character. Layouts are expected to be byte-aligned.
//!
//! ## Memory note
//! The reader materializes the file into a Polars `DataFrame` and returns it as
//! a `LazyFrame`. A truly out-of-core chunked reader is a [FUTURE] enhancement;
//! the engine downstream keeps everything lazy/streaming from here on.

use std::path::Path;

use polars::prelude::*;

use crate::error::{ReconError, ReconResult};
use crate::schema::Schema;

/// Read a fixed-width file into a canonical `LazyFrame` per `schema`.
///
/// Every column is typed as `Utf8` (`String`). One record layout per file — no
/// header/detail/trailer discrimination (decision 4). Blank trailing lines are
/// ignored; a trailing `\r` (CRLF input) is stripped.
pub fn read_fixed_width(path: &Path, schema: &Schema) -> ReconResult<LazyFrame> {
    let bytes = std::fs::read(path).map_err(|e| {
        ReconError::Io(format!("reading {}: {e}", path.display()))
    })?;

    let ncols = schema.fields.len();
    let mut columns: Vec<Vec<String>> = vec![Vec::new(); ncols];

    for raw_line in split_lines(&bytes) {
        // Strip a single trailing CR for CRLF inputs.
        let line = match raw_line.last() {
            Some(b'\r') => &raw_line[..raw_line.len() - 1],
            _ => raw_line,
        };
        for (idx, field) in schema.fields.iter().enumerate() {
            let range = field.byte_range(schema.index_base);
            let start = range.start.min(line.len());
            let end = range.end.min(line.len());
            let slice = if start < end { &line[start..end] } else { &[][..] };
            columns[idx].push(String::from_utf8_lossy(slice).into_owned());
        }
    }

    let series: Vec<Column> = schema
        .fields
        .iter()
        .zip(columns)
        .map(|(field, values)| {
            Column::new(PlSmallStr::from_str(&field.name), values)
        })
        .collect();

    let df = DataFrame::new(series)?;
    Ok(df.lazy())
}

/// Split a byte buffer into lines on `\n`, dropping a trailing empty line.
fn split_lines(bytes: &[u8]) -> impl Iterator<Item = &[u8]> {
    let trimmed = match bytes.last() {
        Some(b'\n') => &bytes[..bytes.len() - 1],
        _ => bytes,
    };
    // If the file was entirely empty, yield nothing.
    let empty = trimmed.is_empty();
    trimmed
        .split(|&b| b == b'\n')
        .filter(move |_| !empty)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::Field;
    use std::io::Write;

    fn schema() -> Schema {
        Schema {
            name: "customers".into(),
            version: 1,
            encoding: "utf-8".into(),
            index_base: 0,
            fields: vec![
                Field { name: "id".into(), start: 0, length: 3 },
                Field { name: "name".into(), start: 3, length: 5 },
            ],
        }
    }

    fn write_tmp(content: &str) -> tempfile::NamedTempFile {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(content.as_bytes()).unwrap();
        f.flush().unwrap();
        f
    }

    #[test]
    fn slices_by_byte_offset() {
        let f = write_tmp("001Alice\n002Bob  \n");
        let lf = read_fixed_width(f.path(), &schema()).unwrap();
        let df = lf.collect().unwrap();
        assert_eq!(df.height(), 2);
        let ids: Vec<_> = df.column("id").unwrap().str().unwrap().into_iter().collect();
        assert_eq!(ids, vec![Some("001"), Some("002")]);
        let names: Vec<_> = df.column("name").unwrap().str().unwrap().into_iter().collect();
        assert_eq!(names, vec![Some("Alice"), Some("Bob  ")]);
    }

    #[test]
    fn short_line_is_padded_empty() {
        let f = write_tmp("01\n");
        let lf = read_fixed_width(f.path(), &schema()).unwrap();
        let df = lf.collect().unwrap();
        assert_eq!(df.height(), 1);
        let names: Vec<_> = df.column("name").unwrap().str().unwrap().into_iter().collect();
        assert_eq!(names, vec![Some("")]);
    }

    #[test]
    fn empty_file_zero_rows() {
        let f = write_tmp("");
        let df = read_fixed_width(f.path(), &schema()).unwrap().collect().unwrap();
        assert_eq!(df.height(), 0);
    }
}
