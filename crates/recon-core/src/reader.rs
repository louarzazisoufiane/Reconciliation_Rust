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
//! The reader is out-of-core: the input is decoded in fixed row-count batches
//! and staged to a temporary Parquet file, which is then scanned lazily. At no
//! point does the whole input sit in RAM — peak reader memory is one batch.
//! The staged temp file (owned by [`FixedWidthSource`]) is deleted on drop; it
//! is created in the system temp directory (honouring `TMPDIR`).

use std::io::{BufRead, BufReader};
use std::path::Path;
use std::sync::Arc;

use polars::prelude::{
    Column, DataFrame, DataType, LazyFrame, ParquetWriter, PlPath, PlSmallStr, PolarsResult,
    ScanArgsParquet, Schema as PlSchema,
};

use crate::error::{ReconError, ReconResult};
use crate::schema::Schema;

/// Rows decoded per staged batch. Bounds reader memory to one batch of
/// `String`s regardless of input size.
const STAGE_BATCH_ROWS: usize = 100_000;

/// A lazily scannable fixed-width source.
///
/// Owns the staged temporary Parquet file backing the [`LazyFrame`]; the file
/// is deleted when this value is dropped, so every query obtained via
/// [`FixedWidthSource::lazy`] must be executed while the source is alive.
pub struct FixedWidthSource {
    lazy: LazyFrame,
    /// Staged temp file; removed from disk on drop.
    staged: tempfile::TempPath,
}

impl FixedWidthSource {
    /// A lazy scan over the staged data. Cheap to clone per query.
    pub fn lazy(&self) -> LazyFrame {
        self.lazy.clone()
    }

    /// Execute the scan and materialize the full source (small inputs/tests).
    pub fn collect(self) -> PolarsResult<DataFrame> {
        self.lazy.collect()
    }

    /// Path of the staged temp file (diagnostics; the file disappears when
    /// this source is dropped).
    pub fn staged_path(&self) -> &Path {
        &self.staged
    }
}

/// Read a fixed-width file into a canonical [`FixedWidthSource`] per `schema`.
///
/// Every column is typed as `Utf8` (`String`). One record layout per file — no
/// header/detail/trailer discrimination (decision 4). Blank trailing lines are
/// ignored; a trailing `\r` (CRLF input) is stripped.
pub fn read_fixed_width(path: &Path, schema: &Schema) -> ReconResult<FixedWidthSource> {
    read_fixed_width_batched(path, schema, STAGE_BATCH_ROWS)
}

/// [`read_fixed_width`] with an explicit batch size (exercised by tests to
/// prove multi-batch staging is byte-identical to single-batch).
fn read_fixed_width_batched(
    path: &Path,
    schema: &Schema,
    batch_rows: usize,
) -> ReconResult<FixedWidthSource> {
    debug_assert!(batch_rows > 0);
    let file = std::fs::File::open(path)
        .map_err(|e| ReconError::Io(format!("reading {}: {e}", path.display())))?;
    let mut reader = BufReader::new(file);

    let mut tmp = tempfile::Builder::new()
        .prefix("recon-stage-")
        .suffix(".parquet")
        .tempfile()
        .map_err(|e| ReconError::Io(format!("creating staging file: {e}")))?;

    let pl_schema: PlSchema = schema
        .fields
        .iter()
        .map(|f| {
            polars::prelude::Field::new(PlSmallStr::from_str(&f.name), DataType::String)
        })
        .collect();

    let ncols = schema.fields.len();
    let mut writer = ParquetWriter::new(tmp.as_file_mut()).batched(&pl_schema)?;
    let mut columns: Vec<Vec<String>> = vec![Vec::new(); ncols];
    let mut pending = 0usize;
    let mut buf: Vec<u8> = Vec::new();

    loop {
        buf.clear();
        let n = reader
            .read_until(b'\n', &mut buf)
            .map_err(|e| ReconError::Io(format!("reading {}: {e}", path.display())))?;
        if n == 0 {
            break;
        }
        // Strip the delimiter, then a single trailing CR for CRLF inputs. A
        // file ending in `\n` naturally yields no blank trailing row here.
        let mut line = &buf[..];
        if let Some(b'\n') = line.last() {
            line = &line[..line.len() - 1];
        }
        if let Some(b'\r') = line.last() {
            line = &line[..line.len() - 1];
        }
        for (idx, field) in schema.fields.iter().enumerate() {
            let range = field.byte_range(schema.index_base);
            let start = range.start.min(line.len());
            let end = range.end.min(line.len());
            let slice = if start < end { &line[start..end] } else { &[][..] };
            columns[idx].push(String::from_utf8_lossy(slice).into_owned());
        }
        pending += 1;
        if pending == batch_rows {
            write_batch(&mut writer, schema, &mut columns)?;
            pending = 0;
        }
    }
    if pending > 0 {
        write_batch(&mut writer, schema, &mut columns)?;
    }
    writer.finish()?;
    drop(writer);

    let staged = tmp.into_temp_path();
    let lazy = LazyFrame::scan_parquet(
        PlPath::Local(Arc::from(&*staged)),
        ScanArgsParquet::default(),
    )?;
    Ok(FixedWidthSource { lazy, staged })
}

/// Flush the accumulated batch to the staging Parquet writer, leaving the
/// column buffers empty for the next batch.
fn write_batch<W: std::io::Write>(
    writer: &mut polars::io::parquet::write::BatchedWriter<W>,
    schema: &Schema,
    columns: &mut [Vec<String>],
) -> ReconResult<()> {
    let series: Vec<Column> = schema
        .fields
        .iter()
        .zip(columns.iter_mut())
        .map(|(field, values)| {
            Column::new(PlSmallStr::from_str(&field.name), std::mem::take(values))
        })
        .collect();
    let df = DataFrame::new(series)?;
    writer.write_batch(&df)?;
    Ok(())
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

    #[test]
    fn multi_batch_matches_single_batch() {
        // 10 rows staged 3 per batch (4 batches, last partial) must be
        // identical to the same file staged in one batch.
        let mut content = String::new();
        for i in 0..10 {
            content.push_str(&format!("{i:03}nm{i:03}\n"));
        }
        // CRLF row, an interior blank row, and a short row mid-stream.
        content.push_str("900x\r\n\n901name.\n");
        let f = write_tmp(&content);

        let multi = read_fixed_width_batched(f.path(), &schema(), 3)
            .unwrap()
            .collect()
            .unwrap();
        let single = read_fixed_width_batched(f.path(), &schema(), usize::MAX)
            .unwrap()
            .collect()
            .unwrap();

        assert_eq!(multi.height(), 13);
        assert_eq!(multi, single);
        // Spot-check values across batch boundaries and the edge rows.
        let ids: Vec<_> = multi.column("id").unwrap().str().unwrap().into_iter().collect();
        assert_eq!(ids[2], Some("002"));
        assert_eq!(ids[3], Some("003"));
        assert_eq!(ids[9], Some("009"));
        assert_eq!(ids[10], Some("900"));
        assert_eq!(ids[11], Some("")); // blank interior line -> empty fields
        assert_eq!(ids[12], Some("901"));
        let names: Vec<_> = multi.column("name").unwrap().str().unwrap().into_iter().collect();
        assert_eq!(names[10], Some("x")); // CR stripped, short line truncated
        assert_eq!(names[12], Some("name."));
    }

    #[test]
    fn staged_temp_file_removed_on_drop() {
        let f = write_tmp("001Alice\n");
        let src = read_fixed_width(f.path(), &schema()).unwrap();
        let staged = src.staged_path().to_path_buf();
        assert!(staged.exists());
        drop(src);
        assert!(!staged.exists());
    }
}
