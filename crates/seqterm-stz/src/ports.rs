use std::path::Path;

use uuid::Uuid;

use crate::{
    domain::{AssetEntry, AssetType, Manifest, ProjectSnapshot, StzContainer},
    error::StzResult,
};

// ─── Storage port ─────────────────────────────────────────────────────────────

pub trait ProjectStoragePort: Send + Sync {
    fn save(&self, container: &StzContainer, path: &Path) -> StzResult<()>;
    fn load(&self, path: &Path) -> StzResult<StzContainer>;
    fn read_manifest(&self, path: &Path) -> StzResult<Manifest>;
    fn list_snapshots(&self, path: &Path) -> StzResult<Vec<ProjectSnapshot>>;
}

// ─── Asset port ───────────────────────────────────────────────────────────────

pub trait AssetStoragePort: Send + Sync {
    fn add_asset(&mut self, data: &[u8], name: &str, asset_type: AssetType) -> StzResult<AssetEntry>;
    fn load_asset_data(&self, uuid: &Uuid) -> StzResult<Vec<u8>>;
    fn remove_asset(&mut self, uuid: &Uuid) -> StzResult<()>;
    fn list_assets(&self) -> &[AssetEntry];
}

// ─── Validator port ───────────────────────────────────────────────────────────

pub trait ProjectValidatorPort: Send + Sync {
    fn validate_manifest(&self, manifest: &Manifest) -> StzResult<()>;
    fn validate_container(&self, container: &StzContainer) -> StzResult<()>;
}

// ─── Archive port ─────────────────────────────────────────────────────────────

pub struct ArchiveEntry {
    pub name: String,
    pub data: Vec<u8>,
}

pub trait ArchivePort: Send + Sync {
    fn pack(&self, entries: Vec<ArchiveEntry>, dest: &Path) -> StzResult<()>;
    fn unpack(&self, archive: &Path) -> StzResult<Vec<ArchiveEntry>>;
    fn read_entry(&self, archive: &Path, name: &str) -> StzResult<Vec<u8>>;
}

// ─── Migration port ───────────────────────────────────────────────────────────

/// A single format-version migration step.
pub trait Migration: Send + Sync {
    fn from_version(&self) -> u32;
    fn to_version(&self) -> u32;
    /// Mutate the raw ZIP entry map in-place.
    fn migrate(&self, entries: &mut std::collections::HashMap<String, Vec<u8>>) -> StzResult<()>;
}
