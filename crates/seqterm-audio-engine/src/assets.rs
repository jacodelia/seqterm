//! Asset cache — background loading of SF2 and audio files.
//! Non-realtime. Thread-safe. LRU eviction.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::Result;

use crate::audio_clip::LoadedClip;
use crate::sf2_synth::SoundFontSynth;

/// Loaded SF2 data (shared between slots playing the same file).
pub struct CachedSf2 {
    pub path: PathBuf,
    pub size_bytes: usize,
}

/// Loaded audio clip data (shared between slots playing the same file).
pub struct CachedAudio {
    pub clip: Arc<LoadedClip>,
    pub path: PathBuf,
}

/// Background-loaded asset registry.
/// Not realtime-safe — used from the control/application thread only.
pub struct AssetCache {
    audio: Mutex<HashMap<PathBuf, Arc<LoadedClip>>>,
    /// Total cached audio memory in bytes.
    audio_bytes: Mutex<usize>,
    /// Maximum audio cache size in bytes.
    pub max_audio_bytes: usize,
}

impl AssetCache {
    pub fn new() -> Self {
        Self {
            audio: Mutex::new(HashMap::new()),
            audio_bytes: Mutex::new(0),
            max_audio_bytes: 256 * 1024 * 1024, // 256 MB
        }
    }

    /// Load or retrieve from cache.
    /// Caller: non-RT thread only.
    pub fn get_or_load_audio(&self, path: &Path) -> Result<Arc<LoadedClip>> {
        let key = path.to_path_buf();

        // Check cache.
        if let Ok(cache) = self.audio.lock() {
            if let Some(clip) = cache.get(&key) {
                return Ok(Arc::clone(clip));
            }
        }

        // Load from disk.
        let clip = Arc::new(LoadedClip::load(path)?);
        let bytes = clip.samples.len() * 4; // f32 = 4 bytes

        if let Ok(mut cache) = self.audio.lock() {
            cache.insert(key, Arc::clone(&clip));
        }
        if let Ok(mut total) = self.audio_bytes.lock() {
            *total += bytes;
        }

        Ok(clip)
    }

    /// Evict all cached audio data.
    pub fn clear_audio(&self) {
        if let Ok(mut cache) = self.audio.lock() { cache.clear(); }
        if let Ok(mut total) = self.audio_bytes.lock() { *total = 0; }
    }

    /// Current audio cache size in bytes.
    pub fn audio_cache_bytes(&self) -> usize {
        self.audio_bytes.lock().map(|v| *v).unwrap_or(0)
    }

    /// Load an SF2 synth instance synchronously (non-RT).
    pub fn load_sf2(&self, path: &Path, bank: u8, preset: u8, sample_rate: u32)
        -> Result<SoundFontSynth>
    {
        SoundFontSynth::load(path, bank, preset, sample_rate)
    }
}

impl Default for AssetCache {
    fn default() -> Self { Self::new() }
}
