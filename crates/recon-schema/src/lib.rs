//! `recon-schema`: the versioned schema library (decision 5).
//!
//! Schemas are first-class, saved entities. This crate defines the
//! [`SchemaStore`] trait (list / get / save / exists) and a filesystem-backed
//! implementation that is *immutable by version*: saving under an existing name
//! creates a NEW version (`schemas/<name>/v<N>.yml` plus a `latest` pointer)
//! rather than silently overwriting. SQLite backing is [FUTURE].

use std::path::{Path, PathBuf};

use recon_core::error::{ReconError, ReconResult};
use recon_core::schema::{Schema, SchemaRef};
use serde::Serialize;

/// Lightweight listing entry for the schema library screen.
#[derive(Debug, Clone, Serialize)]
pub struct SchemaInfo {
    /// Schema name (library key).
    pub name: String,
    /// Highest stored version.
    pub latest_version: u32,
    /// Field count of the latest version.
    pub field_count: usize,
    /// Created-at of the latest version (RFC-3339), best-effort from mtime.
    pub created_at: String,
}

/// Persistence seam for named, versioned fixed-width layouts.
pub trait SchemaStore: Send + Sync {
    /// List every stored schema (latest version of each).
    fn list(&self) -> ReconResult<Vec<SchemaInfo>>;
    /// Whether any version of `name` exists.
    fn exists(&self, name: &str) -> bool;
    /// The highest stored version of `name`, if any.
    fn latest_version(&self, name: &str) -> ReconResult<Option<u32>>;
    /// Fetch a specific version.
    fn get(&self, name: &str, version: u32) -> ReconResult<Schema>;
    /// Fetch the latest version.
    fn get_latest(&self, name: &str) -> ReconResult<Schema>;
    /// Save `schema` as a NEW version (never overwrites). Returns the assigned
    /// version. The schema is validated first; hard validation errors abort.
    fn save(&self, schema: &Schema) -> ReconResult<u32>;
    /// Delete a schema and ALL of its versions. Returns `true` if it existed,
    /// `false` if there was nothing to remove.
    fn delete(&self, name: &str) -> ReconResult<bool>;
    /// Resolve a [`SchemaRef`] to a concrete schema.
    fn resolve(&self, r: &SchemaRef) -> ReconResult<Schema> {
        self.get(&r.name, r.version)
    }
}

/// Filesystem-backed [`SchemaStore`] rooted at a `schemas/` directory.
#[derive(Debug, Clone)]
pub struct FsSchemaStore {
    root: PathBuf,
}

impl FsSchemaStore {
    /// Create a store rooted at `root` (e.g. `schemas/`).
    pub fn new(root: impl Into<PathBuf>) -> Self {
        FsSchemaStore { root: root.into() }
    }

    /// The root directory this store reads/writes.
    pub fn root(&self) -> &Path {
        &self.root
    }

    fn schema_dir(&self, name: &str) -> PathBuf {
        self.root.join(name)
    }

    fn version_file(&self, name: &str, version: u32) -> PathBuf {
        self.schema_dir(name).join(format!("v{version}.yml"))
    }

    fn latest_file(&self, name: &str) -> PathBuf {
        self.schema_dir(name).join("latest.txt")
    }

    fn read_latest_pointer(&self, name: &str) -> ReconResult<Option<u32>> {
        let p = self.latest_file(name);
        if !p.exists() {
            return Ok(None);
        }
        let txt = std::fs::read_to_string(&p)?;
        txt.trim()
            .parse::<u32>()
            .map(Some)
            .map_err(|e| ReconError::config(format!("bad latest pointer for '{name}': {e}")))
    }
}

fn created_at(path: &Path) -> String {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| jiff::Timestamp::try_from(t).ok())
        .map(|ts| ts.to_string())
        .unwrap_or_default()
}

impl SchemaStore for FsSchemaStore {
    fn list(&self) -> ReconResult<Vec<SchemaInfo>> {
        let mut out = Vec::new();
        if !self.root.exists() {
            return Ok(out);
        }
        for entry in std::fs::read_dir(&self.root)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            if let Some(version) = self.latest_version(&name)? {
                let schema = self.get(&name, version)?;
                out.push(SchemaInfo {
                    name: name.clone(),
                    latest_version: version,
                    field_count: schema.fields.len(),
                    created_at: created_at(&self.version_file(&name, version)),
                });
            }
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
    }

    fn exists(&self, name: &str) -> bool {
        self.latest_version(name).ok().flatten().is_some()
    }

    fn latest_version(&self, name: &str) -> ReconResult<Option<u32>> {
        // Prefer the explicit pointer; fall back to scanning version files.
        if let Some(v) = self.read_latest_pointer(name)? {
            return Ok(Some(v));
        }
        let dir = self.schema_dir(name);
        if !dir.exists() {
            return Ok(None);
        }
        let mut max = None;
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            let fname = entry.file_name().to_string_lossy().into_owned();
            if let Some(v) = fname
                .strip_prefix('v')
                .and_then(|s| s.strip_suffix(".yml"))
                .and_then(|s| s.parse::<u32>().ok())
            {
                max = Some(max.map_or(v, |m: u32| m.max(v)));
            }
        }
        Ok(max)
    }

    fn get(&self, name: &str, version: u32) -> ReconResult<Schema> {
        let p = self.version_file(name, version);
        if !p.exists() {
            return Err(ReconError::config(format!(
                "schema '{name}' version {version} not found"
            )));
        }
        let text = std::fs::read_to_string(&p)?;
        let schema: Schema = serde_norway::from_str(&text)
            .map_err(|e| ReconError::config(format!("parsing schema '{name}' v{version}: {e}")))?;
        Ok(schema)
    }

    fn get_latest(&self, name: &str) -> ReconResult<Schema> {
        let v = self
            .latest_version(name)?
            .ok_or_else(|| ReconError::config(format!("schema '{name}' not found")))?;
        self.get(name, v)
    }

    fn save(&self, schema: &Schema) -> ReconResult<u32> {
        schema.validate()?;
        let dir = self.schema_dir(&schema.name);
        std::fs::create_dir_all(&dir)?;

        let next = self.latest_version(&schema.name)?.map_or(1, |v| v + 1);
        let mut to_write = schema.clone();
        to_write.version = next;

        let path = self.version_file(&schema.name, next);
        if path.exists() {
            // Immutability guarantee: never clobber an existing version file.
            return Err(ReconError::config(format!(
                "refusing to overwrite existing {}",
                path.display()
            )));
        }
        let yaml = serde_norway::to_string(&to_write)
            .map_err(|e| ReconError::config(format!("serializing schema: {e}")))?;
        std::fs::write(&path, yaml)?;
        std::fs::write(self.latest_file(&schema.name), next.to_string())?;
        Ok(next)
    }

    fn delete(&self, name: &str) -> ReconResult<bool> {
        let dir = self.schema_dir(name);
        if !dir.exists() {
            return Ok(false);
        }
        std::fs::remove_dir_all(&dir)?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use recon_core::schema::Field;

    fn schema(name: &str, n: usize) -> Schema {
        Schema {
            name: name.into(),
            version: 1,
            encoding: "utf-8".into(),
            index_base: 0,
            fields: (0..n)
                .map(|i| Field {
                    name: format!("f{i}"),
                    start: i * 3,
                    length: 3,
                })
                .collect(),
        }
    }

    #[test]
    fn save_versions_and_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let store = FsSchemaStore::new(dir.path());

        assert!(!store.exists("customers"));
        let v1 = store.save(&schema("customers", 2)).unwrap();
        assert_eq!(v1, 1);
        // Saving under the same name bumps the version; v1 survives.
        let v2 = store.save(&schema("customers", 3)).unwrap();
        assert_eq!(v2, 2);

        assert_eq!(store.latest_version("customers").unwrap(), Some(2));
        assert_eq!(store.get("customers", 1).unwrap().fields.len(), 2);
        assert_eq!(store.get_latest("customers").unwrap().fields.len(), 3);

        let list = store.list().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].latest_version, 2);

        let resolved = store
            .resolve(&SchemaRef {
                name: "customers".into(),
                version: 1,
            })
            .unwrap();
        assert_eq!(resolved.version, 1);
    }

    #[test]
    fn delete_removes_all_versions() {
        let dir = tempfile::tempdir().unwrap();
        let store = FsSchemaStore::new(dir.path());

        store.save(&schema("customers", 2)).unwrap();
        store.save(&schema("customers", 3)).unwrap();
        assert!(store.exists("customers"));

        // Deleting an existing schema wipes every version and reports true.
        assert!(store.delete("customers").unwrap());
        assert!(!store.exists("customers"));
        assert_eq!(store.latest_version("customers").unwrap(), None);
        assert!(store.list().unwrap().is_empty());

        // Deleting again (or a never-existent name) is a no-op reporting false.
        assert!(!store.delete("customers").unwrap());
    }
}
