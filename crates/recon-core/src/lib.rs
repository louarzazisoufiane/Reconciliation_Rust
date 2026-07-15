//! `recon-core`: pure library for fixed-width reconciliation.
//!
//! Contains the configuration model, schema model, the single fixed-width
//! reader, normalization (vectorized Polars expressions), the storage seam,
//! and the comparison engine. No I/O orchestration or reporting lives here.

pub mod config;
pub mod error;
pub mod schema;
pub mod reader;
pub mod normalize;
pub mod engine;
pub mod storage;

pub use error::{ReconError, ReconResult};
pub use schema::{Field, Schema, SchemaRef};
pub use config::RunConfig;
