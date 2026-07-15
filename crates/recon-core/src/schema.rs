//! The fixed-width schema model (decision 5).
//!
//! A schema is a named, reusable fixed-width layout: an ordered list of fields
//! (`{ name, start, length }`) plus an `encoding` and `index_base`. Schemas are
//! first-class, versioned entities; persistence lives in `recon-schema`, this
//! module only defines the model and its validation rules.

use serde::{Deserialize, Serialize};

use crate::error::{ReconError, ReconResult};

/// A single fixed-width field: a byte slice `[start, start + length)`.
///
/// `start` is expressed in the schema's `index_base` (0 by default). Slicing is
/// by BYTE offset — see [`crate::reader`] for the UTF-8 caveat.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Field {
    /// Column name; must be unique within a schema.
    pub name: String,
    /// Zero-based (or `index_base`-based) start offset in bytes.
    pub start: usize,
    /// Field width in bytes; must be `> 0`.
    pub length: usize,
}

impl Field {
    /// The half-open byte range of this field, normalized to a zero-based
    /// offset using the supplied `index_base`.
    pub fn byte_range(&self, index_base: usize) -> std::ops::Range<usize> {
        let start = self.start.saturating_sub(index_base);
        start..start + self.length
    }
}

fn default_encoding() -> String {
    "utf-8".to_string()
}

/// A named, versioned fixed-width layout.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Schema {
    /// Logical schema name (the library key).
    pub name: String,
    /// Monotonic version; a new save under an existing name bumps this.
    pub version: u32,
    /// Character encoding label (informational; the reader slices bytes).
    #[serde(default = "default_encoding")]
    pub encoding: String,
    /// The base that `Field::start` is expressed in (0 = zero-indexed).
    #[serde(default)]
    pub index_base: usize,
    /// Ordered fields making up one record.
    pub fields: Vec<Field>,
}

/// A non-fatal warning surfaced by [`Schema::validate`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SchemaWarning {
    /// Two fields' byte ranges overlap (allowed but flagged — decision, web UI).
    Overlap {
        /// First field name.
        a: String,
        /// Second field name.
        b: String,
    },
    /// A gap exists between two consecutive fields (allowed, informational).
    Gap {
        /// Field before the gap.
        after: String,
        /// Field after the gap.
        before: String,
        /// Number of unmapped bytes.
        bytes: usize,
    },
}

impl Schema {
    /// All field names in declaration order.
    pub fn field_names(&self) -> Vec<&str> {
        self.fields.iter().map(|f| f.name.as_str()).collect()
    }

    /// Whether a column with the given name exists in this schema.
    pub fn has_column(&self, name: &str) -> bool {
        self.fields.iter().any(|f| f.name == name)
    }

    /// Validate structural invariants (decision 5 / web UI rules).
    ///
    /// Hard errors (returned as `Err`): zero-length field, `start < index_base`,
    /// duplicate field name. Soft issues (overlaps, gaps) are returned as
    /// warnings so callers — notably the web UI — can surface them without
    /// refusing the save.
    pub fn validate(&self) -> ReconResult<Vec<SchemaWarning>> {
        if self.fields.is_empty() {
            return Err(ReconError::config(format!(
                "schema '{}' has no fields",
                self.name
            )));
        }

        // Unique names.
        let mut seen = std::collections::HashSet::new();
        for f in &self.fields {
            if f.length == 0 {
                return Err(ReconError::config(format!(
                    "schema '{}': field '{}' has length 0 (must be > 0)",
                    self.name, f.name
                )));
            }
            if f.start < self.index_base {
                return Err(ReconError::config(format!(
                    "schema '{}': field '{}' start {} < index_base {}",
                    self.name, f.name, f.start, self.index_base
                )));
            }
            if !seen.insert(f.name.as_str()) {
                return Err(ReconError::config(format!(
                    "schema '{}': duplicate field name '{}'",
                    self.name, f.name
                )));
            }
        }

        // Overlap / gap detection over fields sorted by start offset.
        let mut warnings = Vec::new();
        let mut ordered: Vec<&Field> = self.fields.iter().collect();
        ordered.sort_by_key(|f| f.start);
        for pair in ordered.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            let a_end = a.byte_range(self.index_base).end;
            let b_start = b.byte_range(self.index_base).start;
            if b_start < a_end {
                warnings.push(SchemaWarning::Overlap {
                    a: a.name.clone(),
                    b: b.name.clone(),
                });
            } else if b_start > a_end {
                warnings.push(SchemaWarning::Gap {
                    after: a.name.clone(),
                    before: b.name.clone(),
                    bytes: b_start - a_end,
                });
            }
        }
        Ok(warnings)
    }

    /// Total byte width implied by the right-most field.
    pub fn record_width(&self) -> usize {
        self.fields
            .iter()
            .map(|f| f.byte_range(self.index_base).end)
            .max()
            .unwrap_or(0)
    }
}

/// A reference to a stored schema by name + version (used in run configs).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SchemaRef {
    /// Schema library name.
    pub name: String,
    /// Specific version to resolve.
    pub version: u32,
}

impl std::fmt::Display for SchemaRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}@v{}", self.name, self.version)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field(name: &str, start: usize, length: usize) -> Field {
        Field {
            name: name.into(),
            start,
            length,
        }
    }

    fn schema(fields: Vec<Field>) -> Schema {
        Schema {
            name: "t".into(),
            version: 1,
            encoding: "utf-8".into(),
            index_base: 0,
            fields,
        }
    }

    #[test]
    fn valid_schema_no_warnings() {
        let s = schema(vec![field("a", 0, 3), field("b", 3, 2)]);
        assert_eq!(s.validate().unwrap(), vec![]);
        assert_eq!(s.record_width(), 5);
    }

    #[test]
    fn zero_length_is_error() {
        let s = schema(vec![field("a", 0, 0)]);
        assert!(s.validate().is_err());
    }

    #[test]
    fn duplicate_name_is_error() {
        let s = schema(vec![field("a", 0, 3), field("a", 3, 2)]);
        assert!(s.validate().is_err());
    }

    #[test]
    fn overlap_and_gap_warn() {
        let overlap = schema(vec![field("a", 0, 4), field("b", 2, 2)]);
        assert!(matches!(
            overlap.validate().unwrap().as_slice(),
            [SchemaWarning::Overlap { .. }]
        ));
        let gap = schema(vec![field("a", 0, 2), field("b", 5, 2)]);
        assert!(matches!(
            gap.validate().unwrap().as_slice(),
            [SchemaWarning::Gap { bytes: 3, .. }]
        ));
    }
}
