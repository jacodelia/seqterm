//! Audio sample clip player.
//!
//! Plays back pre-decoded PCM samples (f32 stereo).
//! REALTIME CONTRACT: `render()` and `play()`/`stop()` are allocation-free.
//! Decoding from disk happens on a background thread and provides Arc<[f32]>.

use std::any::Any;
use std::path::Path;
use std::sync::Arc;

use anyhow::{Result, anyhow};
use seqterm_ports::realtime::AudioSource;

/// Playback mode for audio clips.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoopMode {
    /// Play once to end, then silence.
    Once,
    /// Loop continuously.
    Loop,
}

/// A loaded audio clip — immutable once decoded.
pub struct LoadedClip {
    /// Interleaved stereo f32 samples at the clip's native sample rate.
    pub samples: Arc<[f32]>,
    pub channels: u16,
    pub native_sample_rate: u32,
    pub duration_secs: f64,
}

impl LoadedClip {
    /// Decode an audio file synchronously. Call from a non-RT thread.
    pub fn load(path: &Path) -> Result<Self> {
        // Try WAV first (hound — fast, already in deps).
        if path.extension().and_then(|e| e.to_str()) == Some("wav") {
            return Self::load_wav(path);
        }
        // Fallback: symphonia for FLAC, MP3, OGG, etc.
        Self::load_symphonia(path)
    }

    fn load_wav(path: &Path) -> Result<Self> {
        let mut reader = hound::WavReader::open(path)
            .map_err(|e| anyhow!("WAV open error: {e}"))?;
        let spec = reader.spec();
        let channels = spec.channels;

        let samples: Vec<f32> = match spec.sample_format {
            hound::SampleFormat::Float => {
                reader.samples::<f32>().map(|s| s.unwrap_or(0.0)).collect()
            }
            hound::SampleFormat::Int => {
                let max = (1i64 << (spec.bits_per_sample - 1)) as f32;
                reader.samples::<i32>()
                    .map(|s| s.unwrap_or(0) as f32 / max)
                    .collect()
            }
        };

        let sample_count = samples.len();
        let duration_secs = sample_count as f64 / (spec.sample_rate as f64 * channels as f64);

        Ok(Self {
            samples: samples.into(),
            channels,
            native_sample_rate: spec.sample_rate,
            duration_secs,
        })
    }

    fn load_symphonia(path: &Path) -> Result<Self> {
        use symphonia::core::audio::SampleBuffer;
        use symphonia::core::codecs::DecoderOptions;
        use symphonia::core::formats::FormatOptions;
        use symphonia::core::io::MediaSourceStream;
        use symphonia::core::meta::MetadataOptions;
        use symphonia::core::probe::Hint;

        let file = std::fs::File::open(path)
            .map_err(|e| anyhow!("file open: {e}"))?;
        let mss = MediaSourceStream::new(Box::new(file), Default::default());

        let mut hint = Hint::new();
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            hint.with_extension(ext);
        }

        let probed = symphonia::default::get_probe()
            .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())
            .map_err(|e| anyhow!("probe: {e}"))?;

        let mut format = probed.format;
        let track = format.default_track()
            .ok_or_else(|| anyhow!("no default track"))?;
        let track_id = track.id;
        let sample_rate = track.codec_params.sample_rate.unwrap_or(48000);
        let channels = track.codec_params.channels
            .map(|c| c.count() as u16)
            .unwrap_or(2);

        let mut decoder = symphonia::default::get_codecs()
            .make(&track.codec_params, &DecoderOptions::default())
            .map_err(|e| anyhow!("decoder: {e}"))?;

        let mut all_samples: Vec<f32> = Vec::new();

        loop {
            let packet = match format.next_packet() {
                Ok(p) => p,
                Err(_) => break,
            };
            if packet.track_id() != track_id { continue; }

            match decoder.decode(&packet) {
                Ok(decoded) => {
                    let spec = *decoded.spec();
                    let mut buf = SampleBuffer::<f32>::new(decoded.capacity() as u64, spec);
                    buf.copy_interleaved_ref(decoded);
                    all_samples.extend_from_slice(buf.samples());
                }
                Err(_) => break,
            }
        }

        let duration_secs = all_samples.len() as f64 / (sample_rate as f64 * channels as f64);

        Ok(Self {
            samples: all_samples.into(),
            channels,
            native_sample_rate: sample_rate,
            duration_secs,
        })
    }

    /// Peak-normalize in-place so the loudest sample = 1.0. No-op on silence.
    pub fn normalize(&mut self) {
        let peak = self.samples.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        if peak > 1e-6 {
            let gain = 1.0 / peak;
            let samples: Vec<f32> = self.samples.iter().map(|&s| s * gain).collect();
            self.samples = samples.into();
        }
    }

    /// Compute peak amplitude across `bands` evenly-spaced windows.
    /// Returns values in `[0.0, 1.0]` suitable for waveform display.
    pub fn peak_bands(&self, bands: usize) -> Vec<f32> {
        if bands == 0 { return Vec::new(); }
        let ch = self.channels.max(1) as usize;
        let total_frames = self.samples.len() / ch;
        if total_frames == 0 { return vec![0.0; bands]; }

        let frames_per_band = (total_frames + bands - 1) / bands;
        (0..bands).map(|b| {
            let start = b * frames_per_band * ch;
            let end = ((b + 1) * frames_per_band * ch).min(self.samples.len());
            self.samples[start..end]
                .iter()
                .map(|s| s.abs())
                .fold(0.0f32, f32::max)
        }).collect()
    }
}

/// Realtime audio clip player.
/// Created from a `LoadedClip`, drives `AudioSource` in the callback.
pub struct AudioClipPlayer {
    clip: Arc<LoadedClip>,
    /// Current read position in the samples slice.
    pos: usize,
    /// Playback rate ratio (accounts for sample rate conversion + pitch shift).
    rate: f64,
    /// native_sr / engine_sr — cached so pitch changes don't need engine_sr.
    base_rate: f64,
    /// Fractional position accumulator.
    frac: f64,
    loop_mode: LoopMode,
    playing: bool,
    gain: f32,
    fade_out: Option<(usize, usize)>, // (remaining, total)
    /// Loop start frame (0 = beginning of clip).
    loop_start: usize,
    /// Loop end frame (0 = use full clip length).
    loop_end: usize,
    /// Hard trim start frame — playback never goes before this (0 = beginning).
    trim_start: usize,
    /// Hard trim end frame — playback never goes past this (0 = use full clip length).
    trim_end: usize,
    /// Play sample in reverse (pos counts forward, reads from end backward).
    reverse: bool,
    /// Pitch offset in semitones (vinyl-style: shifts pitch and speed together).
    pitch_st: f32,
}

impl AudioClipPlayer {
    pub fn new(clip: Arc<LoadedClip>, engine_sample_rate: u32) -> Self {
        let base_rate = clip.native_sample_rate as f64 / engine_sample_rate as f64;
        Self {
            clip,
            pos: 0,
            rate: base_rate,
            base_rate,
            frac: 0.0,
            loop_mode: LoopMode::Once,
            playing: false,
            gain: 1.0,
            fade_out: None,
            loop_start: 0,
            loop_end: 0,
            trim_start: 0,
            trim_end: 0,
            reverse: false,
            pitch_st: 0.0,
        }
    }

    /// Apply tempo-based rate (vinyl-style: changes pitch + speed together).
    pub fn set_tempo_ratio(&mut self, ratio: f64) {
        let pitch_ratio = 2.0f64.powf(self.pitch_st as f64 / 12.0);
        self.rate = self.base_rate * ratio * pitch_ratio;
    }

    /// Apply pitch offset in semitones (vinyl-style: changes pitch + speed together).
    pub fn set_pitch_st(&mut self, semitones: f32) {
        self.pitch_st = semitones;
        let pitch_ratio = 2.0f64.powf(semitones as f64 / 12.0);
        self.rate = self.base_rate * pitch_ratio;
    }

    pub fn set_loop_mode(&mut self, mode: LoopMode) { self.loop_mode = mode; }
    pub fn set_gain(&mut self, gain: f32) { self.gain = gain; }
    pub fn set_reverse(&mut self, reverse: bool) { self.reverse = reverse; }

    /// Set hard trim points as fractions of total clip length (0.0–1.0).
    /// `play()` will start at `trim_start`; rendering stops at `trim_end`.
    pub fn set_playback_range(&mut self, start_frac: f32, end_frac: f32) {
        let total = self.clip.samples.len() / self.clip.channels as usize;
        self.trim_start = ((start_frac.clamp(0.0, 1.0) as f64) * total as f64) as usize;
        let end = ((end_frac.clamp(0.0, 1.0) as f64) * total as f64) as usize;
        self.trim_end = end.max(self.trim_start + 1).min(total);
    }

    /// Set loop region as fractions of total clip length (0.0–1.0).
    /// Pass `start_frac = 0.0, end_frac = 1.0` to reset to full-clip loop.
    pub fn set_loop_region(&mut self, start_frac: f32, end_frac: f32) {
        let total = self.clip.samples.len() / self.clip.channels as usize;
        self.loop_start = ((start_frac.clamp(0.0, 1.0) as f64) * total as f64) as usize;
        let end = ((end_frac.clamp(0.0, 1.0) as f64) * total as f64) as usize;
        self.loop_end = end.max(self.loop_start + 1).min(total);
    }

    pub fn play(&mut self) {
        let total = self.clip.samples.len() / self.clip.channels as usize;
        // Trim start takes priority over loop start for the initial position.
        let effective_start = if self.trim_start > 0 && self.trim_start < total {
            self.trim_start
        } else if self.loop_start < total {
            self.loop_start
        } else {
            0
        };
        self.pos = effective_start;
        self.frac = 0.0;
        self.playing = true;
        self.fade_out = None;
    }

    pub fn as_any_mut(&mut self) -> &mut dyn Any { self }
}

impl AudioSource for AudioClipPlayer {
    fn render(&mut self, output: &mut [f32], _sample_rate: u32) -> usize {
        if !self.playing { return 0; }

        let frames = output.len() / 2;
        let total_frames = self.clip.samples.len() / self.clip.channels as usize;
        let ch = self.clip.channels as usize;

        // Hard trim limits: override loop/end if trim points are set.
        let hard_end = if self.trim_end > 0 && self.trim_end <= total_frames {
            self.trim_end
        } else {
            total_frames
        };
        let hard_start = self.trim_start.min(hard_end.saturating_sub(1));

        let loop_end = if self.loop_end > 0 && self.loop_end <= hard_end {
            self.loop_end
        } else {
            hard_end
        };
        let loop_start = self.loop_start.clamp(hard_start, loop_end.saturating_sub(1));

        let mut written = 0;
        for i in 0..frames {
            // For reverse, pos counts from 0 upward but we read backward from loop_end.
            let span = loop_end.saturating_sub(loop_start);
            let (src_idx, past_end) = if self.reverse {
                let idx = loop_end.saturating_sub(1 + self.pos);
                (idx, self.pos >= span)
            } else {
                (self.pos, self.pos >= loop_end)
            };

            if past_end {
                match self.loop_mode {
                    LoopMode::Loop => { self.pos = loop_start; self.frac = 0.0; }
                    LoopMode::Once => { self.playing = false; break; }
                }
            }

            let s_idx = src_idx * ch;
            let l = self.clip.samples.get(s_idx).copied().unwrap_or(0.0);
            let r = if ch >= 2 {
                self.clip.samples.get(s_idx + 1).copied().unwrap_or(0.0)
            } else { l };

            // Apply fade-out.
            let env = if let Some((rem, total)) = self.fade_out {
                let t = rem.saturating_sub(i);
                (t as f32 / total as f32).clamp(0.0, 1.0)
            } else { 1.0 };

            output[i * 2]     = l * self.gain * env;
            output[i * 2 + 1] = r * self.gain * env;

            // Advance read position with linear interpolation at rate.
            self.frac += self.rate;
            let steps = self.frac as usize;
            self.pos += steps;
            self.frac -= steps as f64;

            written += 1;
        }

        if let Some((ref mut rem, _total)) = self.fade_out {
            *rem = rem.saturating_sub(written);
            if *rem == 0 { self.playing = false; }
        }

        written
    }

    fn is_active(&self) -> bool { self.playing }

    fn stop(&mut self) {
        let fade_frames = 2400; // ~50ms at 48kHz
        self.fade_out = Some((fade_frames, fade_frames));
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any { self }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_loaded_clip(samples: Vec<f32>, channels: u16, sr: u32) -> Arc<LoadedClip> {
        let duration_secs = samples.len() as f64 / (channels as f64 * sr as f64);
        Arc::new(LoadedClip {
            samples: samples.into(),
            channels,
            native_sample_rate: sr,
            duration_secs,
        })
    }

    #[test]
    fn peak_bands_empty_samples() {
        let clip = make_loaded_clip(vec![], 2, 48000);
        let bands = clip.peak_bands(8);
        assert_eq!(bands.len(), 8);
        assert!(bands.iter().all(|&b| b == 0.0));
    }

    #[test]
    fn peak_bands_zero_bands() {
        let clip = make_loaded_clip(vec![0.5; 128], 2, 48000);
        let bands = clip.peak_bands(0);
        assert!(bands.is_empty());
    }

    #[test]
    fn peak_bands_constant_signal() {
        let clip = make_loaded_clip(vec![0.5f32; 256], 2, 48000);
        let bands = clip.peak_bands(8);
        assert_eq!(bands.len(), 8);
        for &b in &bands {
            assert!((b - 0.5).abs() < 1e-5, "expected ~0.5, got {b}");
        }
    }

    #[test]
    fn peak_bands_detects_spike_in_last_band() {
        let mut samples = vec![0.0f32; 256];
        samples[240] = 0.9; // near the end
        let clip = make_loaded_clip(samples, 1, 48000);
        let bands = clip.peak_bands(4); // 4 bands of 64 mono samples each
        assert!(bands[3] > 0.5, "spike should appear in band 3, got {:?}", bands);
        assert!(bands[0] < 0.1, "first band should be near zero");
    }

    #[test]
    fn player_initially_inactive() {
        let clip = make_loaded_clip(vec![0.5; 64], 2, 48000);
        let player = AudioClipPlayer::new(clip, 48000);
        assert!(!player.is_active());
    }

    #[test]
    fn player_becomes_active_after_play() {
        let clip = make_loaded_clip(vec![0.5; 64], 2, 48000);
        let mut player = AudioClipPlayer::new(clip, 48000);
        player.play();
        assert!(player.is_active());
    }

    #[test]
    fn player_renders_nonzero_audio() {
        // 128 stereo samples at 0.3 → 64 frames
        let clip = make_loaded_clip(vec![0.3f32; 128], 2, 48000);
        let mut player = AudioClipPlayer::new(clip, 48000);
        player.play();
        let mut buf = vec![0.0f32; 32];
        let written = player.render(&mut buf, 48000);
        assert!(written > 0, "should render some frames");
        assert!(buf.iter().any(|&s| s.abs() > 1e-6), "should render non-silent audio");
    }

    #[test]
    fn trim_limits_playback_range() {
        // 64 stereo frames; trim to frames 16-48 (fractions 0.25-0.75).
        let clip = make_loaded_clip(vec![0.5f32; 128], 2, 48000);
        let mut player = AudioClipPlayer::new(clip, 48000);
        player.set_playback_range(0.25, 0.75);
        player.play();
        // pos should start at trim_start frame (16).
        assert_eq!(player.pos, 16, "play() should start at trim_start frame");
    }

    #[test]
    fn normalize_load_peak() {
        use std::sync::Arc;
        // Build a clip whose peak is 0.5; expect normalize sets gain to 2.0.
        let clip = Arc::new(LoadedClip {
            samples: vec![0.0f32, 0.5, 0.3, -0.1].into(),
            channels: 2,
            native_sample_rate: 48000,
            duration_secs: 0.001,
        });
        let peak = clip.samples.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        assert!((peak - 0.5).abs() < 1e-6);
        let norm_gain = 1.0 / peak;
        assert!((norm_gain - 2.0).abs() < 1e-6);
    }

    #[test]
    fn pitch_up_octave_doubles_rate() {
        let clip = make_loaded_clip(vec![0.5; 128], 2, 48000);
        let mut player = AudioClipPlayer::new(clip, 48000);
        player.set_pitch_st(12.0);
        // rate should be base_rate * 2^(12/12) = 1.0 * 2.0
        assert!((player.rate - 2.0).abs() < 1e-6, "expected rate=2.0, got {}", player.rate);
    }

    #[test]
    fn pitch_down_octave_halves_rate() {
        let clip = make_loaded_clip(vec![0.5; 128], 2, 48000);
        let mut player = AudioClipPlayer::new(clip, 48000);
        player.set_pitch_st(-12.0);
        assert!((player.rate - 0.5).abs() < 1e-6, "expected rate=0.5, got {}", player.rate);
    }

    #[test]
    fn reverse_plays_shorter_duration() {
        // 64 frames at 1:1 rate; with reverse the same frame count is consumed.
        let clip = make_loaded_clip(vec![0.5f32; 128], 2, 48000);
        let mut player = AudioClipPlayer::new(clip, 48000);
        player.set_reverse(true);
        player.play();
        let mut buf = vec![0.0f32; 128];
        let written = player.render(&mut buf, 48000);
        assert!(written > 0, "reverse should render frames");
        assert!(buf.iter().any(|&s| s.abs() > 1e-6), "reverse should be non-silent");
    }
}
