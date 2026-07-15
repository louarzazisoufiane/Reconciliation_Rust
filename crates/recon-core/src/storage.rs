//! The `Storage` seam (decision 1).
//!
//! Every artifact path — reports, sidecars, manifest, schemas — flows through
//! this trait so an S3/blob backend is a drop-in later [FUTURE]. Only a local
//! filesystem implementation ships today; no cloud SDKs are pulled in.

use std::path::{Path, PathBuf};

use crate::error::ReconResult;

/// Abstract byte/metadata storage behind which a cloud backend can slot in.
pub trait Storage: Send + Sync {
    /// Read the full contents of a path.
    fn read(&self, path: &Path) -> ReconResult<Vec<u8>>;
    /// Write bytes to a path, creating parent directories as needed.
    fn write(&self, path: &Path, bytes: &[u8]) -> ReconResult<()>;
    /// Whether a path exists.
    fn exists(&self, path: &Path) -> bool;
    /// Size in bytes of a path (used by completion detection — decision 11).
    fn size(&self, path: &Path) -> ReconResult<u64>;
    /// List immediate entries of a directory (empty if it does not exist).
    fn list_dir(&self, path: &Path) -> ReconResult<Vec<PathBuf>>;
}

/// The local-filesystem implementation of [`Storage`].
#[derive(Debug, Clone, Default)]
pub struct LocalFs;

impl Storage for LocalFs {
    fn read(&self, path: &Path) -> ReconResult<Vec<u8>> {
        Ok(std::fs::read(path)?)
    }

    fn write(&self, path: &Path, bytes: &[u8]) -> ReconResult<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, bytes)?;
        Ok(())
    }

    fn exists(&self, path: &Path) -> bool {
        path.exists()
    }

    fn size(&self, path: &Path) -> ReconResult<u64> {
        Ok(std::fs::metadata(path)?.len())
    }

    fn list_dir(&self, path: &Path) -> ReconResult<Vec<PathBuf>> {
        if !path.exists() {
            return Ok(Vec::new());
        }
        let mut out = Vec::new();
        for entry in std::fs::read_dir(path)? {
            out.push(entry?.path());
        }
        out.sort();
        Ok(out)
    }
}
