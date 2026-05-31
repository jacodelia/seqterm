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
        m.register(V0ToV1);
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
