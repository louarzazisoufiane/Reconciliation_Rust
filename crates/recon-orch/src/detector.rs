//! Completion detection (decision 11).
//!
//! A source is "done" only when it both EXISTS and its file size has been
//! unchanged for a stability window — never on existence alone. The behavior is
//! behind the [`CompletionDetector`] trait so marker-file / control-table
//! strategies can drop in later [FUTURE].
//!
//! ## Caveat (documented in every report footer)
//! A writer that stalls longer than the stability window mid-write can look
//! "done" and cause a partial-file comparison.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use recon_core::error::ReconResult;

/// The completion-detection seam.
pub trait CompletionDetector: Send + Sync {
    /// Whether `path` is finished being written. Implementations that need
    /// history (like size stability) are called repeatedly by the poll loop.
    fn is_complete(&self, path: &Path) -> ReconResult<bool>;
}

/// Existence + file-size stability detector (decision 11).
///
/// The size of each watched path is recorded on first sight and whenever it
/// changes; `is_complete` returns `true` only once the size has held steady for
/// at least `min_stable`.
pub struct SizeStabilityDetector {
    min_stable: Duration,
    seen: Mutex<HashMap<PathBuf, (u64, Instant)>>,
}

impl SizeStabilityDetector {
    /// Build a detector requiring `minutes` of unchanged size.
    pub fn new(minutes: u64) -> Self {
        SizeStabilityDetector {
            min_stable: Duration::from_secs(minutes * 60),
            seen: Mutex::new(HashMap::new()),
        }
    }

    /// Build a detector with an explicit stability duration (used in tests).
    pub fn with_duration(min_stable: Duration) -> Self {
        SizeStabilityDetector {
            min_stable,
            seen: Mutex::new(HashMap::new()),
        }
    }
}

impl CompletionDetector for SizeStabilityDetector {
    fn is_complete(&self, path: &Path) -> ReconResult<bool> {
        if !path.exists() {
            return Ok(false);
        }
        let size = std::fs::metadata(path)?.len();
        let mut seen = self.seen.lock().expect("detector mutex poisoned");
        match seen.get(path) {
            // Size held steady since we last recorded it.
            Some((prev, since)) if *prev == size => Ok(since.elapsed() >= self.min_stable),
            // First sighting or a size change: (re)start the stability clock.
            _ => {
                seen.insert(path.to_path_buf(), (size, Instant::now()));
                Ok(false)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn missing_file_never_complete() {
        let d = SizeStabilityDetector::with_duration(Duration::ZERO);
        assert!(!d.is_complete(Path::new("/no/such/file")).unwrap());
    }

    #[test]
    fn stable_becomes_complete_changing_does_not() {
        let d = SizeStabilityDetector::with_duration(Duration::ZERO);
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(b"hello").unwrap();
        f.flush().unwrap();

        // First sighting records size, not yet complete.
        assert!(!d.is_complete(f.path()).unwrap());
        // Unchanged since last sighting (window is zero) => complete.
        assert!(d.is_complete(f.path()).unwrap());

        // Grow the file: no longer complete on the next check.
        f.write_all(b" world").unwrap();
        f.flush().unwrap();
        assert!(!d.is_complete(f.path()).unwrap());
        // Stable again => complete.
        assert!(d.is_complete(f.path()).unwrap());
    }
}
