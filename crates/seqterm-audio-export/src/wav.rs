use std::path::Path;

use anyhow::{Context, Result};
use tracing::info;

use crate::AudioExportOpts;

/// Write a silent WAV file as a placeholder for offline render.
/// Respects `opts.sample_rate` and `opts.bit_depth` (16 / 24 / 32).
/// Full offline render (P2) will replace silence with actual audio synthesis.
pub fn export_wav_stub(path: &Path, duration_secs: f32, opts: &AudioExportOpts) -> Result<()> {
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: opts.sample_rate,
        bits_per_sample: opts.bit_depth as u16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(path, spec)
        .context("creating WAV file")?;
    let n_samples = (duration_secs * opts.sample_rate as f32) as u32;
    for _ in 0..n_samples * 2 {
        writer.write_sample(0i32)?;
    }
    writer.finalize().context("finalizing WAV")?;
    info!(
        "WAV stub written: {} ({} Hz, {}-bit, {:.1}s)",
        path.display(), opts.sample_rate, opts.bit_depth, duration_secs
    );
    Ok(())
}
