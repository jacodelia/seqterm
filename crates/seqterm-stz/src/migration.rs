use std::collections::HashMap;

use crate::{
    domain::STZ_FORMAT_VERSION,
    error::{StzError, StzResult},
    ports::Migration,
};

// ─── Migrator ─────────────────────────────────────────────────────────────────

/// Applies a chain of format-version migrations in order.
pub struct ProjectMigrator {
    migrations: Vec<Box<dyn Migration>>,
}

impl ProjectMigrator {
    pub fn new() -> Self {
        let mut m = Self { migrations: Vec::new() };
        m.migrations.push(Box::new(V0ToV1));
        m.migrations.push(Box::new(V1ToV2));
        m
    }

    pub fn register(&mut self, migration: impl Migration + 'static) {
        self.migrations.push(Box::new(migration));
    }

    /// Migrate a ZIP entry map from `from_version` up to the current version.
    pub fn migrate_to_current(
        &self,
        entries: &mut HashMap<String, Vec<u8>>,
        from_version: u32,
    ) -> StzResult<()> {
        let mut version = from_version;
        while version < STZ_FORMAT_VERSION {
            let step = self
                .migrations
                .iter()
                .find(|m| m.from_version() == version)
                .ok_or_else(|| StzError::MigrationFailed {
                    from: version,
                    to: STZ_FORMAT_VERSION,
                    reason: format!("no migration registered for v{version} → v{}", version + 1),
                })?;
            step.migrate(entries)?;
            version = step.to_version();
        }
        Ok(())
    }
}

impl Default for ProjectMigrator {
    fn default() -> Self {
        Self::new()
    }
}

// ─── v0 → v1 ─────────────────────────────────────────────────────────────────

/// Stamps `format_version: 1` in the manifest; no structural changes.
struct V0ToV1;

// ─── v1 → v2 ─────────────────────────────────────────────────────────────────

/// v2 additions:
/// - `project/project.json` gains `track_colors`, `track_heights`, `track_types`,
///   `markers`, `loop_region` (all default-empty/null if absent — no data lost).
/// - `snapshots/` directory support (no migration needed: old files just have none).
/// - `plugins/state/` for plugin state blobs.
///
/// This is a non-breaking forward migration: old v1 files load fine in v2 because
/// all new fields have `#[serde(default)]` in `seqterm_core::Project`.
struct V1ToV2;

impl Migration for V0ToV1 {
    fn from_version(&self) -> u32 { 0 }
    fn to_version(&self) -> u32 { 1 }

    fn migrate(&self, entries: &mut HashMap<String, Vec<u8>>) -> StzResult<()> {
        if let Some(bytes) = entries.get_mut("manifest.json") {
            let mut value: serde_json::Value = serde_json::from_slice(bytes)?;
            value["format_version"] = serde_json::json!(1);
            *bytes = serde_json::to_vec_pretty(&value)?;
            tracing::debug!("STZ migration: v0 → v1 (stamped format_version)");
        }
        Ok(())
    }
}

impl Migration for V1ToV2 {
    fn from_version(&self) -> u32 { 1 }
    fn to_version(&self) -> u32 { 2 }

    fn migrate(&self, entries: &mut HashMap<String, Vec<u8>>) -> StzResult<()> {
        // Stamp the new version in manifest.json.
        if let Some(bytes) = entries.get_mut("manifest.json") {
            let mut value: serde_json::Value = serde_json::from_slice(bytes)?;
            value["format_version"] = serde_json::json!(2);
            *bytes = serde_json::to_vec_pretty(&value)?;
        }
        // project/project.json: add default values for new fields if absent.
        // All new fields have serde defaults, so this is a no-op for deserialization,
        // but we stamp them explicitly so the file is self-documenting.
        if let Some(bytes) = entries.get("project/project.json") {
            let mut value: serde_json::Value = serde_json::from_slice(bytes)?;
            if value.get("track_colors").is_none() { value["track_colors"] = serde_json::json!({}); }
            if value.get("track_heights").is_none() { value["track_heights"] = serde_json::json!({}); }
            if value.get("track_types").is_none()  { value["track_types"]  = serde_json::json!({}); }
            if value.get("markers").is_none()      { value["markers"]      = serde_json::json!([]); }
            if value.get("loop_region").is_none()  { value["loop_region"]  = serde_json::Value::Null; }
            let new_bytes = serde_json::to_vec_pretty(&value)?;
            entries.insert("project/project.json".to_string(), new_bytes);
        }
        tracing::debug!("STZ migration: v1 → v2 (track metadata + snapshot support)");
        Ok(())
    }
}
