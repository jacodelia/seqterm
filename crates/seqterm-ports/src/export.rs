//! Export port — audio file rendering.

use std::path::PathBuf;
use anyhow::Result;

/// Output format for audio export.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExportFormat {
    Wav32Float,
    Wav16,
    Mp3 { bitrate_kbps: u32 },
    Flac { compression: u8 },
}

/// Configuration for an export job.
#[derive(Debug, Clone)]
pub struct ExportConfig {
    pub output_path: PathBuf,
    pub format: ExportFormat,
    pub sample_rate: u32,
    pub stems: bool,
    pub start_bar: u32,
    pub end_bar: Option<u32>,
}

/// Progress callback type.
pub type ExportProgressFn = Box<dyn Fn(f32) + Send + 'static>;

/// Port: audio export.
pub trait ExporterPort: Send + Sync {
    /// Render the current project to an audio file.
    /// `progress_fn` called with 0.0..1.0 as rendering proceeds.
    fn export(
        &self,
        config: ExportConfig,
        progress_fn: Option<ExportProgressFn>,
    ) -> Result<PathBuf>;
}
