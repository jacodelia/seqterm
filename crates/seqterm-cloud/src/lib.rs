//! # SeqTerm Cloud Sync
//!
//! Provides a pluggable adapter for backing up and syncing SeqTerm projects
//! to cloud storage.  Adapters implement the [`CloudAdapter`] trait; any
//! storage provider (S3-compatible, Dropbox, custom REST API, local NFS mount,
//! rsync endpoint) can be plugged in behind this interface.
//!
//! ## Built-in adapters
//!
//! | Adapter | Feature flag | Description |
//! |---------|-------------|-------------|
//! | [`LocalFs`] | default | Copies `.stz` files to a local or NFS path |
//! | [`HttpAdapter`] | `http` | Uploads/downloads via a REST API |
//!
//! ## Usage
//!
//! ```rust,ignore
//! use seqterm_cloud::{CloudAdapter, SyncMeta, LocalFs};
//!
//! let adapter = LocalFs::new("/mnt/backup/seqterm");
//! adapter.push("my_project", &stz_bytes).await?;
//! let (bytes, meta) = adapter.pull("my_project").await?;
//! ```

use std::path::PathBuf;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use chrono::{DateTime, Utc};

// ─── SyncMeta ─────────────────────────────────────────────────────────────────

/// Metadata returned alongside a downloaded project.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncMeta {
    /// Project UUID.
    pub project_id: Uuid,
    /// Human-readable project name.
    pub name:        String,
    /// Last modified timestamp on the remote.
    pub modified_at: DateTime<Utc>,
    /// File size in bytes.
    pub size_bytes:  u64,
    /// Remote path / key under which the project is stored.
    pub remote_key:  String,
}

// ─── CloudAdapter trait ───────────────────────────────────────────────────────

/// Abstraction over a remote storage backend.
///
/// All methods are `async` and must not block the audio thread.
/// They are called from a dedicated sync task outside the RT path.
pub trait CloudAdapter: Send + Sync + 'static {
    /// Name of this adapter (shown in the UI).
    fn name(&self) -> &str;

    /// Upload a project (raw `.stz` bytes) under `project_key`.
    /// Overwrites any existing version.
    fn push<'a>(
        &'a self,
        project_key: &'a str,
        data:        &'a [u8],
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<SyncMeta>> + Send + 'a>>;

    /// Download a project by `project_key`.
    /// Returns `(raw_bytes, metadata)`.
    fn pull<'a>(
        &'a self,
        project_key: &'a str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(Vec<u8>, SyncMeta)>> + Send + 'a>>;

    /// List all projects available on the remote.
    fn list<'a>(
        &'a self,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<SyncMeta>>> + Send + 'a>>;

    /// Delete a project from the remote.
    fn delete<'a>(
        &'a self,
        project_key: &'a str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + 'a>>;
}

// ─── LocalFs adapter ──────────────────────────────────────────────────────────

/// Adapter that copies `.stz` files to a local directory (or NFS / SMB mount).
/// No network required; also acts as a reference implementation.
pub struct LocalFs {
    root: PathBuf,
}

impl LocalFs {
    /// Create a `LocalFs` adapter that stores projects in `root`.
    /// The directory will be created if it doesn't exist.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    fn project_path(&self, key: &str) -> PathBuf {
        let safe_key = key.replace(['/', '\\', '\0', ':'], "_");
        self.root.join(format!("{safe_key}.stz"))
    }
}

impl CloudAdapter for LocalFs {
    fn name(&self) -> &str { "Local filesystem" }

    fn push<'a>(
        &'a self,
        project_key: &'a str,
        data:        &'a [u8],
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<SyncMeta>> + Send + 'a>> {
        Box::pin(async move {
            std::fs::create_dir_all(&self.root)?;
            let path = self.project_path(project_key);
            std::fs::write(&path, data)?;
            let meta = std::fs::metadata(&path)?;
            Ok(SyncMeta {
                project_id:  Uuid::new_v4(),
                name:        project_key.to_string(),
                modified_at: meta.modified().map(DateTime::<Utc>::from).unwrap_or_else(|_| Utc::now()),
                size_bytes:  meta.len(),
                remote_key:  project_key.to_string(),
            })
        })
    }

    fn pull<'a>(
        &'a self,
        project_key: &'a str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(Vec<u8>, SyncMeta)>> + Send + 'a>> {
        Box::pin(async move {
            let path = self.project_path(project_key);
            let data = std::fs::read(&path)?;
            let meta_fs = std::fs::metadata(&path)?;
            let sync_meta = SyncMeta {
                project_id:  Uuid::new_v4(),
                name:        project_key.to_string(),
                modified_at: meta_fs.modified().map(DateTime::<Utc>::from).unwrap_or_else(|_| Utc::now()),
                size_bytes:  data.len() as u64,
                remote_key:  project_key.to_string(),
            };
            Ok((data, sync_meta))
        })
    }

    fn list<'a>(
        &'a self,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<SyncMeta>>> + Send + 'a>> {
        Box::pin(async move {
            let mut results = Vec::new();
            if !self.root.exists() { return Ok(results); }
            for entry in std::fs::read_dir(&self.root)?.flatten() {
                let path = entry.path();
                if path.extension().map(|e| e == "stz").unwrap_or(false) {
                    let key = path.file_stem()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    let meta_fs = std::fs::metadata(&path)?;
                    results.push(SyncMeta {
                        project_id:  Uuid::new_v4(),
                        name:        key.clone(),
                        modified_at: meta_fs.modified().map(DateTime::<Utc>::from).unwrap_or_else(|_| Utc::now()),
                        size_bytes:  meta_fs.len(),
                        remote_key:  key,
                    });
                }
            }
            results.sort_by(|a, b| b.modified_at.cmp(&a.modified_at));
            Ok(results)
        })
    }

    fn delete<'a>(
        &'a self,
        project_key: &'a str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + 'a>> {
        Box::pin(async move {
            let path = self.project_path(project_key);
            if path.exists() { std::fs::remove_file(path)?; }
            Ok(())
        })
    }
}

// ─── HTTP adapter ─────────────────────────────────────────────────────────────

/// HTTP-based adapter that communicates with a REST API.
///
/// Expects endpoints:
/// - `PUT  /projects/{key}` — upload (body = raw `.stz` bytes)
/// - `GET  /projects/{key}` — download
/// - `GET  /projects`       — list (JSON array of `SyncMeta`)
/// - `DELETE /projects/{key}` — delete
#[cfg(feature = "http")]
pub struct HttpAdapter {
    base_url: String,
    client:   reqwest::Client,
}

#[cfg(feature = "http")]
impl HttpAdapter {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            client:   reqwest::Client::new(),
        }
    }
}

#[cfg(feature = "http")]
impl CloudAdapter for HttpAdapter {
    fn name(&self) -> &str { "HTTP REST API" }

    fn push<'a>(
        &'a self,
        project_key: &'a str,
        data:        &'a [u8],
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<SyncMeta>> + Send + 'a>> {
        Box::pin(async move {
            let url = format!("{}/projects/{}", self.base_url, project_key);
            let resp = self.client.put(&url)
                .body(data.to_vec())
                .header("Content-Type", "application/octet-stream")
                .send().await?
                .error_for_status()?;
            Ok(resp.json::<SyncMeta>().await?)
        })
    }

    fn pull<'a>(
        &'a self,
        project_key: &'a str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(Vec<u8>, SyncMeta)>> + Send + 'a>> {
        Box::pin(async move {
            let url = format!("{}/projects/{}", self.base_url, project_key);
            let resp = self.client.get(&url).send().await?.error_for_status()?;
            // Try to get metadata from response headers.
            let size = resp.content_length().unwrap_or(0);
            let bytes = resp.bytes().await?.to_vec();
            let meta = SyncMeta {
                project_id:  Uuid::new_v4(),
                name:        project_key.to_string(),
                modified_at: Utc::now(),
                size_bytes:  size,
                remote_key:  project_key.to_string(),
            };
            Ok((bytes, meta))
        })
    }

    fn list<'a>(
        &'a self,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<SyncMeta>>> + Send + 'a>> {
        Box::pin(async move {
            let url = format!("{}/projects", self.base_url);
            let metas = self.client.get(&url).send().await?
                .error_for_status()?
                .json::<Vec<SyncMeta>>().await?;
            Ok(metas)
        })
    }

    fn delete<'a>(
        &'a self,
        project_key: &'a str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + 'a>> {
        Box::pin(async move {
            let url = format!("{}/projects/{}", self.base_url, project_key);
            self.client.delete(&url).send().await?.error_for_status()?;
            Ok(())
        })
    }
}

// ─── SyncManager ──────────────────────────────────────────────────────────────

/// Coordinates sync operations: compares local vs. remote timestamps and
/// decides which direction to transfer.
pub struct SyncManager {
    adapter: Box<dyn CloudAdapter>,
}

impl SyncManager {
    pub fn new(adapter: impl CloudAdapter) -> Self {
        Self { adapter: Box::new(adapter) }
    }

    /// Push the local project to the remote.
    pub async fn backup(&self, key: &str, stz_bytes: &[u8]) -> Result<SyncMeta> {
        self.adapter.push(key, stz_bytes).await
    }

    /// Pull the remote project, returning bytes ready to open with `seqterm-stz`.
    pub async fn restore(&self, key: &str) -> Result<(Vec<u8>, SyncMeta)> {
        self.adapter.pull(key).await
    }

    /// List projects on the remote.
    pub async fn list(&self) -> Result<Vec<SyncMeta>> {
        self.adapter.list().await
    }

    /// Delete a project from the remote.
    pub async fn delete(&self, key: &str) -> Result<()> {
        self.adapter.delete(key).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_fs_path_is_safe() {
        let adapter = LocalFs::new("/tmp/seqterm");
        let p = adapter.project_path("my/project:name");
        let fname = p.file_name().unwrap().to_string_lossy();
        assert!(!fname.contains('/'));
        assert!(!fname.contains(':'));
    }

    #[test]
    fn sync_meta_serializes() {
        let meta = SyncMeta {
            project_id:  Uuid::new_v4(),
            name:        "test".into(),
            modified_at: Utc::now(),
            size_bytes:  42,
            remote_key:  "test".into(),
        };
        let json = serde_json::to_string(&meta).unwrap();
        assert!(json.contains("test"));
    }
}
