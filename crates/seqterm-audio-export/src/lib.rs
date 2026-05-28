pub mod wav;

pub use wav::export_wav_stub;

#[cfg(feature = "mp3")]
pub mod mp3;

#[cfg(feature = "mp3")]
pub use mp3::export_mp3_stub;

use std::path::{Path, PathBuf};
use anyhow::Result;
use seqterm_core::Project;

/// Export one WAV stub per active matrix row into the same directory as `mixdown_path`.
/// Files are named `<stem>_<ROW>.wav` (e.g. `demo_A.wav`, `demo_B.wav`).
/// Returns paths of files written.
pub fn export_stems_stub(
    project: &Project,
    mixdown_path: &Path,
    opts: &AudioExportOpts,
) -> Result<Vec<PathBuf>> {
    let dir  = mixdown_path.parent().unwrap_or(Path::new("."));
    let base = mixdown_path.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "export".to_string());

    let duration = {
        let max_bars = project.tracks.iter()
            .flat_map(|t| t.blocks.iter())
            .map(|(start, len, _)| start + len)
            .max()
            .unwrap_or(32);
        let beats_per_bar = 4.0_f32;
        let secs_per_beat = (60.0 / project.bpm) as f32;
        (max_bars as f32 * beats_per_bar * secs_per_beat).max(1.0)
    };

    let mut written = Vec::new();
    for row in 0u8..8 {
        let row_key = ((b'A' + row) as char).to_string();
        let has_clips = project.matrix.get(&row_key)
            .map(|slots| slots.iter().any(|s| s.is_some()))
            .unwrap_or(false);
        if !has_clips { continue; }

        let path = dir.join(format!("{base}_{row_key}.wav"));
        wav::export_wav_stub(&path, duration, opts)?;
        written.push(path);
    }
    Ok(written)
}

/// Mix mode for audio export.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportMode {
    /// Single stereo mixdown.
    Mixdown,
    /// One WAV file per active pattern row (stems).
    Stems,
}

/// Container format for audio export.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ExportFormat {
    #[default]
    Wav,
    #[cfg(feature = "mp3")]
    Mp3,
}

/// User-selected audio export settings (captured from the export options dialog).
#[derive(Debug, Clone)]
pub struct AudioExportOpts {
    pub sample_rate: u32,
    pub bit_depth:   u8,
    pub mode:        ExportMode,
}

impl Default for AudioExportOpts {
    fn default() -> Self {
        Self { sample_rate: 48000, bit_depth: 16, mode: ExportMode::Mixdown }
    }
}

/// Write a single audio stub file, selecting the encoder from the path extension.
/// Falls back to WAV for unknown extensions. Returns `Err` if the format requires
/// an optional feature (`mp3`) that was not compiled in.
pub fn export_audio_stub(path: &Path, duration_secs: f32, opts: &AudioExportOpts) -> Result<()> {
    match path.extension().and_then(|e| e.to_str()) {
        #[cfg(feature = "mp3")]
        Some("mp3") => mp3::export_mp3_stub(path, duration_secs, opts),
        Some("mp3") => anyhow::bail!(
            "MP3 export requires the `mp3` feature flag — rebuild with `--features seqterm-audio-export/mp3`"
        ),
        _ => wav::export_wav_stub(path, duration_secs, opts),
    }
}
