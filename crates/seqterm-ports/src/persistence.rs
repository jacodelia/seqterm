//! Persistence ports — project save/load repository.

use std::path::PathBuf;
use anyhow::Result;
use seqterm_core::Project;

/// Lightweight metadata about a saved project (for project browser).
#[derive(Debug, Clone)]
pub struct ProjectMetadata {
    pub name: String,
    pub path: PathBuf,
    pub bpm: f64,
    pub version: u32,
    pub modified_at: Option<std::time::SystemTime>,
}

/// Port: project persistence.
/// Implemented by JsonProjectRepository, BinaryProjectRepository, etc.
pub trait ProjectRepository: Send + Sync {
    /// Load a project from the given path.
    fn load(&self, path: &std::path::Path) -> Result<Project>;

    /// Save a project to the given path.
    fn save(&self, project: &Project, path: &std::path::Path) -> Result<()>;

    /// Read lightweight metadata without loading the full project.
    fn read_metadata(&self, path: &std::path::Path) -> Result<ProjectMetadata>;

    /// List all projects in a directory.
    fn list(&self, dir: &std::path::Path) -> Result<Vec<ProjectMetadata>>;

    /// Create a backup of the given project file.
    fn backup(&self, path: &std::path::Path) -> Result<PathBuf>;
}
