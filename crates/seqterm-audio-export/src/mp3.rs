use std::path::Path;
use anyhow::{Context, Result};
use mp3lame_encoder::{Builder, FlushNoGap, Id3Tag, MonoPcm};

use crate::AudioExportOpts;

/// Write a silent MP3 file of the given duration using libmp3lame.
///
/// Enabled only when the `mp3` feature is active.  Requires `libmp3lame-dev`
/// (Ubuntu/Debian) or the equivalent native library at build time.
pub fn export_mp3_stub(path: &Path, duration_secs: f32, opts: &AudioExportOpts) -> Result<()> {
    let sample_rate = opts.sample_rate;
    let n_samples   = (duration_secs * sample_rate as f32).ceil() as usize;

    let mut builder = Builder::new().context("mp3lame: failed to create encoder")?;
    builder.set_sample_rate(sample_rate).context("mp3lame: set_sample_rate")?;
    builder.set_brate(mp3lame_encoder::Birtate::Kbps192).context("mp3lame: set_brate")?;
    builder.set_quality(mp3lame_encoder::Quality::Best).context("mp3lame: set_quality")?;
    builder.set_id3_tag(Id3Tag {
        title:   b"SeqTerm Export",
        artist:  b"SeqTerm",
        album:   b"",
        year:    b"",
        comment: b"",
    });

    let mut encoder = builder.build().context("mp3lame: build encoder")?;

    // Silent input: all zeroes.
    let silence: Vec<i16> = vec![0i16; n_samples];

    // Encode in one pass (mono silence).
    let input = MonoPcm(&silence);
    let mut out_buf = vec![0u8; mp3lame_encoder::max_required_buffer_size(n_samples)];
    let encoded = encoder.encode(input, &mut out_buf)
        .context("mp3lame: encode")?;
    let flush_buf_size = 7200;
    let mut flush_buf = vec![0u8; flush_buf_size];
    let flushed = encoder.flush::<FlushNoGap>(&mut flush_buf)
        .context("mp3lame: flush")?;

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).context("creating output directory")?;
    }
    let mut file = std::fs::File::create(path)
        .with_context(|| format!("creating {}", path.display()))?;
    use std::io::Write;
    file.write_all(&out_buf[..encoded]).context("writing mp3 frames")?;
    file.write_all(&flush_buf[..flushed]).context("writing mp3 flush")?;

    tracing::info!("MP3 stub written to {} ({:.1}s, {}kHz)", path.display(), duration_secs, sample_rate / 1000);
    Ok(())
}
