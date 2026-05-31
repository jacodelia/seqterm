use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::domain::AssetEntry;

/// Tracks all audio/MIDI/plugin asset files embedded in the archive.
///
/// Stored at `registry/assets.json`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AssetRegistry {
    pub assets: Vec<AssetEntry>,
}

impl AssetRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(&mut self, entry: AssetEntry) {
        self.assets.retain(|a| a.uuid != entry.uuid);
        self.assets.push(entry);
    }

    pub fn remove(&mut self, uuid: &Uuid) {
        self.assets.retain(|a| &a.uuid != uuid);
    }

    pub fn find_by_uuid(&self, uuid: &Uuid) -> Option<&AssetEntry> {
        self.assets.iter().find(|a| &a.uuid == uuid)
    }

    /// Returns an existing entry whose hash matches, enabling deduplication.
    pub fn find_by_hash(&self, hash: &str) -> Option<&AssetEntry> {
        self.assets.iter().find(|a| a.hash == hash)
    }
}

/// Tracks all project objects by UUID for fast loading and validation.
///
/// Stored at `registry/objects.json`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ObjectRegistry {
    pub tracks: Vec<Uuid>,
    pub patterns: Vec<Uuid>,
    pub clips: Vec<Uuid>,
    pub mixer_channels: Vec<Uuid>,
    pub buses: Vec<Uuid>,
    pub automation: Vec<Uuid>,
    pub plugins: Vec<Uuid>,
    pub routing_graphs: Vec<Uuid>,
}

impl ObjectRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn contains(&self, uuid: &Uuid) -> bool {
        self.tracks.contains(uuid)
            || self.patterns.contains(uuid)
            || self.clips.contains(uuid)
            || self.mixer_channels.contains(uuid)
            || self.buses.contains(uuid)
            || self.automation.contains(uuid)
            || self.plugins.contains(uuid)
            || self.routing_graphs.contains(uuid)
    }
}
