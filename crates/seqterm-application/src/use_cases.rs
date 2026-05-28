//! Use cases — application business logic, one function per user action.
//!
//! Use cases:
//! - Accept a command
//! - Interact with domain via ports (ProjectRepository, AudioBackendPort, etc.)
//! - Publish domain events via EventBus
//! - Do NOT contain UI or rendering logic

use std::path::Path;
use std::sync::Arc;
use parking_lot::Mutex;
use anyhow::Result;
use tracing::info;

use seqterm_core::Project;
use seqterm_ports::{AudioEngineConfig, ProjectRepository};

use crate::{DomainEvent, EventBus};

/// Load a project from disk and publish ProjectLoaded event.
pub fn load_project(
    path: &Path,
    repo: &dyn ProjectRepository,
    project: &Arc<Mutex<Project>>,
    event_bus: &EventBus,
) -> Result<()> {
    let loaded = repo.load(path)?;
    let name = loaded.name.clone();
    *project.lock() = loaded;
    info!("Project loaded: {} from {:?}", name, path);
    event_bus.publish(DomainEvent::ProjectLoaded {
        name,
        path: Some(path.to_path_buf()),
    });
    Ok(())
}

/// Save the current project to disk and publish ProjectSaved event.
pub fn save_project(
    path: &Path,
    repo: &dyn ProjectRepository,
    project: &Arc<Mutex<Project>>,
    event_bus: &EventBus,
) -> Result<()> {
    let proj = project.lock().clone();
    repo.save(&proj, path)?;
    info!("Project saved to {:?}", path);
    event_bus.publish(DomainEvent::ProjectSaved { path: path.to_path_buf() });
    Ok(())
}

/// Start the audio engine and publish AudioEngineStarted.
pub fn start_audio_engine(
    config: AudioEngineConfig,
    backend: &mut dyn seqterm_ports::AudioBackendPort,
    event_bus: &EventBus,
) -> Result<()> {
    backend.open(config)?;
    event_bus.publish(DomainEvent::AudioEngineStarted {
        sample_rate: backend.sample_rate(),
        buffer_size: backend.buffer_size(),
    });
    Ok(())
}

/// Stop the audio engine.
pub fn stop_audio_engine(
    backend: &mut dyn seqterm_ports::AudioBackendPort,
    event_bus: &EventBus,
) {
    backend.close();
    event_bus.publish(DomainEvent::AudioEngineStopped);
}
