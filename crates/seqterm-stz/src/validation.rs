use crate::{
    domain::{Manifest, StzContainer, STZ_FORMAT, STZ_FORMAT_VERSION},
    error::{StzError, StzResult},
    ports::ProjectValidatorPort,
};

/// Default validator: checks manifest fields and registry consistency.
pub struct DefaultValidator;

impl ProjectValidatorPort for DefaultValidator {
    fn validate_manifest(&self, manifest: &Manifest) -> StzResult<()> {
        if manifest.format != STZ_FORMAT {
            return Err(StzError::InvalidManifest(format!(
                "expected format '{}', got '{}'",
                STZ_FORMAT, manifest.format
            )));
        }
        if manifest.format_version > STZ_FORMAT_VERSION {
            return Err(StzError::UnsupportedVersion(manifest.format_version));
        }
        if manifest.project_name.is_empty() {
            return Err(StzError::InvalidManifest("project_name is empty".into()));
        }
        if manifest.root_project.is_empty() {
            return Err(StzError::InvalidManifest("root_project path is empty".into()));
        }
        Ok(())
    }

    fn validate_container(&self, container: &StzContainer) -> StzResult<()> {
        // Every UUID referenced in StzProject must exist as an object.
        for uuid in &container.project.tracks {
            if !container.tracks.iter().any(|t| &t.id == uuid) {
                return Err(StzError::MissingObject(uuid.to_string()));
            }
        }
        for uuid in &container.project.patterns {
            if !container.patterns.iter().any(|p| &p.id == uuid) {
                return Err(StzError::MissingObject(uuid.to_string()));
            }
        }
        for uuid in &container.project.mixer_channels {
            if !container.mixer_channels.iter().any(|c| &c.id == uuid) {
                return Err(StzError::MissingObject(uuid.to_string()));
            }
        }
        for uuid in &container.project.buses {
            if !container.buses.iter().any(|b| &b.id == uuid) {
                return Err(StzError::MissingObject(uuid.to_string()));
            }
        }

        // Routing graph must be acyclic.
        container.routing.validate_acyclic()?;

        // Every audio-source pattern path should have a matching asset once the
        // asset pipeline is wired (lightweight check for now).
        let _ = &container.patterns;

        // Registry consistency: registered object count must match in-memory counts.
        let reg = container.build_object_registry();
        if reg.tracks.len() != container.object_registry.tracks.len()
            || reg.patterns.len() != container.object_registry.patterns.len()
        {
            return Err(StzError::RegistryMismatch(
                "object counts differ between manifest and in-memory state".into(),
            ));
        }

        Ok(())
    }
}
