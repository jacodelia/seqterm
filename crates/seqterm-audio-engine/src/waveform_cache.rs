//! Waveform overview cache.
//!
//! Pre-computes and persists per-file waveform overview data (peak per band)
//! to `~/.cache/seqterm/waveforms/{hash}.f32`. The cache avoids re-scanning
//! audio files on every project load.
//!
//! Realtime contract: none — all operations are disk I/O on background threads.

use std::path::{Path, PathBuf};

/// Resolve the cache directory: `$XDG_CACHE_HOME/seqterm/waveforms/` or
/// `~/.cache/seqterm/waveforms/`.
pub fn cache_dir() -> PathBuf {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            dirs_sys_path().unwrap_or_else(|| std::env::temp_dir().join("seqterm_cache"))
        });
    base.join("seqterm").join("waveforms")
}

fn dirs_sys_path() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        Some(PathBuf::from(std::env::var_os("HOME")?).join("Library/Caches"))
    }
    #[cfg(not(target_os = "macos"))]
    {
        Some(PathBuf::from(std::env::var_os("HOME")?).join(".cache"))
    }
}

/// Cache key: SHA-256 hex of the file path string + file size (avoids full content hash).
fn cache_key(path: &Path) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    path.hash(&mut h);
    if let Ok(meta) = std::fs::metadata(path) {
        meta.len().hash(&mut h);
    }
    format!("{:016x}", h.finish())
}

/// Load waveform overview from cache. Returns `None` on cache miss or I/O error.
pub fn load_cached(path: &Path) -> Option<Vec<f32>> {
    let key = cache_key(path);
    let cache_path = cache_dir().join(format!("{key}.f32"));
    let bytes = std::fs::read(&cache_path).ok()?;
    if bytes.len() % 4 != 0 { return None; }
    Some(
        bytes.chunks_exact(4)
            .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .collect()
    )
}

/// Write waveform overview to cache. Silently ignores I/O errors.
pub fn write_cached(path: &Path, bands: &[f32]) {
    let dir = cache_dir();
    if std::fs::create_dir_all(&dir).is_err() { return; }
    let key = cache_key(path);
    let cache_path = dir.join(format!("{key}.f32"));
    let bytes: Vec<u8> = bands.iter()
        .flat_map(|f| f.to_le_bytes())
        .collect();
    let _ = std::fs::write(&cache_path, bytes);
}

/// Scan waveform overview for an audio file, using cache when available.
/// `bands` is the number of amplitude peaks to return.
/// Returns `None` on error; call from a background thread.
pub fn waveform_bands(path: &Path, bands: usize) -> Option<Vec<f32>> {
    // Cache hit.
    if let Some(cached) = load_cached(path) {
        if cached.len() == bands {
            return Some(cached);
        }
    }
    // Cache miss: compute.
    let result = crate::scan_waveform(path, bands).ok()?;
    write_cached(path, &result);
    Some(result)
}

/// Invalidate the cached waveform for a specific file (e.g. after the file changes).
pub fn invalidate(path: &Path) {
    let key = cache_key(path);
    let cache_path = cache_dir().join(format!("{key}.f32"));
    let _ = std::fs::remove_file(cache_path);
}

/// Remove all cached waveforms older than `max_age_secs` seconds.
/// Returns the number of files deleted.
pub fn evict_old(max_age_secs: u64) -> usize {
    let dir = cache_dir();
    let Ok(entries) = std::fs::read_dir(&dir) else { return 0; };
    let threshold = std::time::SystemTime::now()
        .checked_sub(std::time::Duration::from_secs(max_age_secs))
        .unwrap_or(std::time::UNIX_EPOCH);
    let mut count = 0usize;
    for entry in entries.flatten() {
        if let Ok(meta) = entry.metadata() {
            if let Ok(modified) = meta.modified() {
                if modified < threshold {
                    if std::fs::remove_file(entry.path()).is_ok() {
                        count += 1;
                    }
                }
            }
        }
    }
    count
}
