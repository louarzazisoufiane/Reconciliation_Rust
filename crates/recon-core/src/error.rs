//! Typed error enum mapping to distinct process exit codes (decision 12).
//!
//! The `recon` binary exits 0 on any *data* outcome; a non-zero exit only ever
//! signals a genuine engine/IO/config failure. Each variant carries a stable
//! exit code so shells and schedulers can distinguish causes.

use thiserror::Error;

/// The crate-wide error type.
#[derive(Debug, Error)]
pub enum ReconError {
    /// Bad or inconsistent configuration (exit code 2).
    #[error("config error: {0}")]
    Config(String),

    /// Filesystem / read failure while accessing an input or artifact (exit 3).
    #[error("io error: {0}")]
    Io(String),

    /// Failure inside the compute engine, e.g. a Polars error (exit 4).
    #[error("engine error: {0}")]
    Engine(String),
}

impl ReconError {
    /// The process exit code associated with this error (decision 12).
    ///
    /// `0` is reserved for success (any data outcome); this method never
    /// returns `0`.
    pub fn exit_code(&self) -> i32 {
        match self {
            ReconError::Config(_) => 2,
            ReconError::Io(_) => 3,
            ReconError::Engine(_) => 4,
        }
    }

    /// Convenience constructor for a config error.
    pub fn config(msg: impl Into<String>) -> Self {
        ReconError::Config(msg.into())
    }

    /// Convenience constructor for an engine error.
    pub fn engine(msg: impl Into<String>) -> Self {
        ReconError::Engine(msg.into())
    }
}

impl From<std::io::Error> for ReconError {
    fn from(e: std::io::Error) -> Self {
        ReconError::Io(e.to_string())
    }
}

impl From<polars::prelude::PolarsError> for ReconError {
    fn from(e: polars::prelude::PolarsError) -> Self {
        ReconError::Engine(e.to_string())
    }
}

/// The crate-wide result alias.
pub type ReconResult<T> = Result<T, ReconError>;
