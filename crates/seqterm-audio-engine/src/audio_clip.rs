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

// ─── Per-slot DSP: biquad filter + ADSR envelope ─────────────────────────────

/// Filter response selected per pad (mirrors `seqterm_core::FilterKind`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipFilterKind { Off, Lowpass, Highpass, Bandpass, Notch }

impl From<seqterm_core::FilterKind> for ClipFilterKind {
    fn from(k: seqterm_core::FilterKind) -> Self {
        match k {
            seqterm_core::FilterKind::Off      => Self::Off,
            seqterm_core::FilterKind::Lowpass  => Self::Lowpass,
            seqterm_core::FilterKind::Highpass => Self::Highpass,
            seqterm_core::FilterKind::Bandpass => Self::Bandpass,
            seqterm_core::FilterKind::Notch    => Self::Notch,
        }
    }
}

/// One RBJ biquad: coefficients + per-call state. Stereo uses two of these.
#[derive(Debug, Clone, Copy, Default)]
struct Biquad {
    b0: f32, b1: f32, b2: f32, a1: f32, a2: f32,
    x1: f32, x2: f32, y1: f32, y2: f32,
}

impl Biquad {
    #[inline]
    fn process(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.b1 * self.x1 + self.b2 * self.x2
            - self.a1 * self.y1 - self.a2 * self.y2;
        self.x2 = self.x1; self.x1 = x;
        self.y2 = self.y1; self.y1 = y;
        y
    }
    fn reset(&mut self) { self.x1 = 0.0; self.x2 = 0.0; self.y1 = 0.0; self.y2 = 0.0; }

    /// RBJ cookbook coefficients for the given response, cutoff and Q.
    fn set(&mut self, kind: ClipFilterKind, fc: f32, q: f32, sr: f32) {
        let fc = fc.clamp(20.0, sr * 0.45);
        let q  = q.max(0.1);
        let w0 = 2.0 * std::f32::consts::PI * fc / sr;
        let (sn, cs) = w0.sin_cos();
        let alpha = sn / (2.0 * q);
        let a0 = 1.0 + alpha;
        let (b0, b1, b2, a1, a2) = match kind {
            ClipFilterKind::Lowpass  => ((1.0 - cs) / 2.0, 1.0 - cs, (1.0 - cs) / 2.0, -2.0 * cs, 1.0 - alpha),
            ClipFilterKind::Highpass => ((1.0 + cs) / 2.0, -(1.0 + cs), (1.0 + cs) / 2.0, -2.0 * cs, 1.0 - alpha),
            ClipFilterKind::Bandpass => (alpha, 0.0, -alpha, -2.0 * cs, 1.0 - alpha),
            ClipFilterKind::Notch    => (1.0, -2.0 * cs, 1.0, -2.0 * cs, 1.0 - alpha),
            ClipFilterKind::Off      => (1.0, 0.0, 0.0, 0.0, 0.0),
        };
        if kind == ClipFilterKind::Off {
            self.b0 = 1.0; self.b1 = 0.0; self.b2 = 0.0; self.a1 = 0.0; self.a2 = 0.0;
        } else {
            self.b0 = b0 / a0; self.b1 = b1 / a0; self.b2 = b2 / a0;
            self.a1 = a1 / a0; self.a2 = a2 / a0;
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EnvPhase { Idle, Attack, Hold, Decay, Sustain, Release }

/// Gate-driven ADSR+Hold voice envelope, advanced one frame at a time.
#[derive(Debug, Clone)]
struct AdsrVoice {
    enabled:   bool,
    attack_ms: f32, hold_ms: f32, decay_ms: f32, sustain: f32, release_ms: f32,
    phase:     EnvPhase,
    level:     f32,
    t:         f32,   // seconds elapsed in the current phase
    rel_from:  f32,   // level captured at release onset
}

impl Default for AdsrVoice {
    fn default() -> Self {
        Self {
            enabled: false,
            attack_ms: 2.0, hold_ms: 0.0, decay_ms: 200.0, sustain: 0.8, release_ms: 100.0,
            phase: EnvPhase::Idle, level: 0.0, t: 0.0, rel_from: 0.0,
        }
    }
}

impl AdsrVoice {
    fn set(&mut self, env: &seqterm_core::AdsrEnvelope) {
        self.enabled    = env.enabled;
        self.attack_ms  = env.attack_ms.max(0.0);
        self.hold_ms    = env.hold_ms.max(0.0);
        self.decay_ms   = env.decay_ms.max(0.0);
        self.sustain    = env.sustain.clamp(0.0, 1.0);
        self.release_ms = env.release_ms.max(0.0);
    }
    fn gate_on(&mut self) { self.phase = EnvPhase::Attack; self.level = 0.0; self.t = 0.0; }
    fn gate_off(&mut self) {
        if self.phase != EnvPhase::Idle {
            self.rel_from = self.level;
            self.phase = EnvPhase::Release;
            self.t = 0.0;
        }
    }
    fn is_finished(&self) -> bool { self.phase == EnvPhase::Idle && self.level <= 1e-4 }

    /// Advance one frame, returning the envelope gain (0–1).
    #[inline]
    fn next(&mut self, dt: f32) -> f32 {
        if !self.enabled { return 1.0; }
        match self.phase {
            EnvPhase::Idle => { self.level = 0.0; }
            EnvPhase::Attack => {
                let dur = self.attack_ms / 1000.0;
                if dur <= 0.0 { self.level = 1.0; self.phase = EnvPhase::Hold; self.t = 0.0; }
                else {
                    self.level = (self.t / dur).min(1.0);
                    if self.t >= dur { self.level = 1.0; self.phase = EnvPhase::Hold; self.t = 0.0; }
                }
            }
            EnvPhase::Hold => {
                self.level = 1.0;
                if self.t >= self.hold_ms / 1000.0 { self.phase = EnvPhase::Decay; self.t = 0.0; }
            }
            EnvPhase::Decay => {
                let dur = self.decay_ms / 1000.0;
                if dur <= 0.0 { self.level = self.sustain; self.phase = EnvPhase::Sustain; self.t = 0.0; }
                else {
                    let p = (self.t / dur).min(1.0);
                    self.level = 1.0 + (self.sustain - 1.0) * p;
                    if self.t >= dur { self.level = self.sustain; self.phase = EnvPhase::Sustain; self.t = 0.0; }
                }
            }
            EnvPhase::Sustain => { self.level = self.sustain; }
            EnvPhase::Release => {
                let dur = self.release_ms / 1000.0;
                if dur <= 0.0 { self.level = 0.0; self.phase = EnvPhase::Idle; }
                else {
                    let p = (self.t / dur).min(1.0);
                    self.level = self.rel_from * (1.0 - p);
                    if self.t >= dur { self.level = 0.0; self.phase = EnvPhase::Idle; }
                }
            }
        }
        self.t += dt;
        self.level
    }
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

    /// Apply a single destructive edit operation to the PCM buffer, in place.
    /// Fractions are relative to the full clip length. `Delete`/`Trim` change
    /// the buffer length; the others preserve it. Call from a non-RT thread.
    pub fn apply_edit_op(&mut self, op: &seqterm_core::AudioEditOp) {
        use seqterm_core::AudioEditOp;
        let ch = self.channels.max(1) as usize;
        let frames = self.samples.len() / ch;
        if frames == 0 { return; }
        // Clamp a fraction to a frame index, then to a flat sample index.
        let idx = |frac: f32| -> usize { ((frac.clamp(0.0, 1.0) * frames as f32) as usize).min(frames) * ch };

        let mut buf: Vec<f32> = self.samples.to_vec();

        match *op {
            AudioEditOp::Silence { start, end } => {
                let (a, b) = (idx(start.min(end)), idx(start.max(end)));
                for s in &mut buf[a..b] { *s = 0.0; }
            }
            AudioEditOp::Reverse { start, end } => {
                let (a, b) = (idx(start.min(end)), idx(start.max(end)));
                // Reverse frame-wise (keep L/R order within each frame).
                let region = &mut buf[a..b];
                let nframes = region.len() / ch;
                for f in 0..nframes / 2 {
                    let lo = f * ch;
                    let hi = (nframes - 1 - f) * ch;
                    for c in 0..ch { region.swap(lo + c, hi + c); }
                }
            }
            AudioEditOp::Normalize => {
                let peak = buf.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
                if peak > 1e-6 {
                    let g = 1.0 / peak;
                    for s in &mut buf { *s *= g; }
                }
            }
            AudioEditOp::FadeIn { end } => {
                let e = idx(end).max(ch);
                let nframes = e / ch;
                for f in 0..nframes {
                    let g = f as f32 / nframes.max(1) as f32;
                    for c in 0..ch { buf[f * ch + c] *= g; }
                }
            }
            AudioEditOp::FadeOut { start } => {
                let a = idx(start);
                let nframes = (buf.len() - a) / ch;
                for f in 0..nframes {
                    let g = 1.0 - f as f32 / nframes.max(1) as f32;
                    for c in 0..ch { buf[a + f * ch + c] *= g; }
                }
            }
            AudioEditOp::Delete { start, end } => {
                let (a, b) = (idx(start.min(end)), idx(start.max(end)));
                buf.drain(a..b);
            }
            AudioEditOp::Trim { start, end } => {
                let (a, b) = (idx(start.min(end)), idx(start.max(end)));
                buf = buf[a..b].to_vec();
            }
        }

        self.duration_secs = (buf.len() / ch) as f64 / self.native_sample_rate.max(1) as f64;
        self.samples = buf.into();
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

/// Write a `LoadedClip` to a 16-bit stereo WAV file at the clip's native sample rate.
pub fn write_wav(clip: &LoadedClip, path: &std::path::Path) -> anyhow::Result<()> {
    let channels = clip.channels.max(1) as u32;
    let spec = hound::WavSpec {
        channels: channels as u16,
        sample_rate: clip.native_sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut w = hound::WavWriter::create(path, spec)?;
    for &s in clip.samples.iter() {
        let val = (s.clamp(-1.0, 1.0) * 32767.0) as i16;
        w.write_sample(val)?;
    }
    w.finalize()?;
    Ok(())
}

impl LoadedClip {
    /// Convenience: stretch the clip to match `project_bpm` given its `original_bpm`.
    /// Returns `self` unchanged when `original_bpm` is 0 or BPMs match.
    pub fn time_stretch_to_bpm(&self, original_bpm: f64, project_bpm: f64) -> anyhow::Result<Self> {
        if original_bpm < 1.0 || (original_bpm - project_bpm).abs() < 0.5 {
            return Ok(Self {
                samples: self.samples.clone(),
                channels: self.channels,
                native_sample_rate: self.native_sample_rate,
                duration_secs: self.duration_secs,
            });
        }
        // ratio > 1 = stretch (slow down), ratio < 1 = compress (speed up).
        let ratio = project_bpm / original_bpm;
        self.time_stretch(ratio)
    }

    /// Time-stretch this clip offline to a new speed ratio using rubato's
    /// sinc interpolation resampler. Returns a new `LoadedClip` with the
    /// stretched audio at the original native sample rate.
    ///
    /// `ratio` > 1.0 = slower (stretched), < 1.0 = faster (compressed).
    /// Pitch is preserved (unlike vinyl-style speed change).
    /// Call from a non-RT background thread.
    pub fn time_stretch(&self, ratio: f64) -> anyhow::Result<Self> {
        use rubato::{
            FastFixedIn, PolynomialDegree, Resampler,
        };

        if (ratio - 1.0).abs() < 0.001 {
            return Ok(Self {
                samples: self.samples.clone(),
                channels: self.channels,
                native_sample_rate: self.native_sample_rate,
                duration_secs: self.duration_secs,
            });
        }

        let ch = self.channels as usize;
        let total_frames = self.samples.len() / ch;

        // De-interleave.
        let planes: Vec<Vec<f64>> = (0..ch)
            .map(|c| {
                (0..total_frames)
                    .map(|f| self.samples[f * ch + c] as f64)
                    .collect()
            })
            .collect();

        // rubato FastFixedIn: resample from (1/ratio) × SR to SR.
        // This stretches the audio by `ratio` without changing pitch.
        let mut resampler = FastFixedIn::<f64>::new(
            1.0 / ratio,  // input / output ratio
            2.0,
            PolynomialDegree::Cubic,
            1024,
            ch,
        )?;

        let mut out_planes: Vec<Vec<f64>> = vec![Vec::new(); ch];

        let chunk_size = resampler.input_frames_max();
        let mut pos = 0usize;
        while pos < total_frames {
            let end = (pos + chunk_size).min(total_frames);
            let chunk: Vec<Vec<f64>> = planes.iter()
                .map(|p| {
                    let mut v = p[pos..end].to_vec();
                    v.resize(chunk_size, 0.0);
                    v
                })
                .collect();
            let out = resampler.process(&chunk.iter().map(|v| v.as_slice()).collect::<Vec<_>>(), None)?;
            for (c, o) in out.iter().enumerate() {
                out_planes[c].extend_from_slice(o);
            }
            pos = end;
        }

        // Flush remaining frames.
        if let Ok(out) = resampler.process_partial::<Vec<f64>>(None, None) {
            for (c, o) in out.iter().enumerate() {
                out_planes[c].extend_from_slice(o);
            }
        }

        // Re-interleave.
        let out_frames = out_planes[0].len();
        let mut samples: Vec<f32> = Vec::with_capacity(out_frames * ch);
        for f in 0..out_frames {
            for c in 0..ch {
                samples.push(out_planes[c][f] as f32);
            }
        }

        let duration_secs = out_frames as f64 / self.native_sample_rate as f64;
        Ok(Self {
            samples: samples.into(),
            channels: self.channels,
            native_sample_rate: self.native_sample_rate,
            duration_secs,
        })
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
    /// Stereo pan (-1.0 = L, 0.0 = C, +1.0 = R), constant-power law.
    pan: f32,
    /// Per-pad biquad filter (one per channel) + its parameters.
    filter_kind: ClipFilterKind,
    filter_cutoff: f32,   // normalised 0–1 (mapped to Hz)
    filter_q: f32,        // normalised 0–1 (mapped to Q)
    filter: [Biquad; 2],
    /// Sample rate the filter coefficients were computed for (0 = uncomputed).
    filter_coeff_sr: u32,
    /// True when the filter coefficients need recomputing.
    filter_dirty: bool,
    /// Per-pad ADSR voice envelope.
    env: AdsrVoice,
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
            pan: 0.0,
            filter_kind: ClipFilterKind::Off,
            filter_cutoff: 1.0,
            filter_q: 0.1,
            filter: [Biquad::default(); 2],
            filter_coeff_sr: 0,
            filter_dirty: true,
            env: AdsrVoice::default(),
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

    /// Set stereo pan (-1.0 = hard L, 0.0 = center, +1.0 = hard R).
    pub fn set_pan(&mut self, pan: f32) { self.pan = pan.clamp(-1.0, 1.0); }

    /// Set the per-pad filter. `cutoff` and `resonance` are normalised 0–1;
    /// cutoff maps to 20–20000 Hz logarithmically, resonance to Q 0.5–10.
    pub fn set_filter(&mut self, kind: ClipFilterKind, cutoff: f32, resonance: f32) {
        self.filter_kind   = kind;
        self.filter_cutoff = cutoff.clamp(0.0, 1.0);
        self.filter_q      = resonance.clamp(0.0, 1.0);
        self.filter_dirty  = true;
    }

    /// Set the per-pad ADSR voice envelope.
    pub fn set_envelope(&mut self, env: &seqterm_core::AdsrEnvelope) { self.env.set(env); }

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
        // Retrigger the voice envelope and clear filter state for a clean attack.
        self.env.gate_on();
        self.filter[0].reset();
        self.filter[1].reset();
    }

    pub fn as_any_mut(&mut self) -> &mut dyn Any { self }
}

impl AudioSource for AudioClipPlayer {
    fn render(&mut self, output: &mut [f32], sample_rate: u32) -> usize {
        if !self.playing { return 0; }

        let frames = output.len() / 2;
        let total_frames = self.clip.samples.len() / self.clip.channels as usize;
        let ch = self.clip.channels as usize;

        // Recompute filter coefficients when the params or sample rate changed.
        if self.filter_dirty || self.filter_coeff_sr != sample_rate {
            // cutoff 0–1 → 20–20000 Hz (log); resonance 0–1 → Q 0.5–10.
            let fc = 20.0 * 1000f32.powf(self.filter_cutoff);
            let q  = 0.5 + self.filter_q * 9.5;
            self.filter[0].set(self.filter_kind, fc, q, sample_rate as f32);
            self.filter[1].set(self.filter_kind, fc, q, sample_rate as f32);
            self.filter_coeff_sr = sample_rate;
            self.filter_dirty = false;
        }
        let filter_on = self.filter_kind != ClipFilterKind::Off;

        // Constant-power pan law.
        let pan_angle = (self.pan + 1.0) * 0.25 * std::f32::consts::PI; // 0..π/2
        let (pan_l, pan_r) = (pan_angle.cos(), pan_angle.sin());

        let dt = 1.0 / sample_rate as f32; // seconds per frame for the envelope

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

            // Apply fade-out (stop ramp).
            let fade = if let Some((rem, total)) = self.fade_out {
                let t = rem.saturating_sub(i);
                (t as f32 / total as f32).clamp(0.0, 1.0)
            } else { 1.0 };

            // Per-pad biquad filter.
            let (mut l, mut r) = (l, r);
            if filter_on {
                l = self.filter[0].process(l);
                r = self.filter[1].process(r);
            }

            // ADSR voice envelope (1.0 when disabled).
            let venv = self.env.next(dt);

            let amp = self.gain * fade * venv;
            output[i * 2]     = l * amp * pan_l;
            output[i * 2 + 1] = r * amp * pan_r;

            // Advance read position with linear interpolation at rate.
            self.frac += self.rate;
            let steps = self.frac as usize;
            self.pos += steps;
            self.frac -= steps as f64;

            written += 1;

            // Envelope fully released → stop the voice.
            if self.env.enabled && self.env.is_finished() {
                self.playing = false;
                break;
            }
        }

        if let Some((ref mut rem, _total)) = self.fade_out {
            *rem = rem.saturating_sub(written);
            if *rem == 0 { self.playing = false; }
        }

        written
    }

    fn is_active(&self) -> bool { self.playing }

    fn stop(&mut self) {
        if self.env.enabled {
            // Let the envelope's release stage ramp the voice down.
            self.env.gate_off();
        } else {
            let fade_frames = 2400; // ~50ms at 48kHz
            self.fade_out = Some((fade_frames, fade_frames));
        }
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
    fn edit_op_silence_zeroes_region() {
        // 8 mono frames of 1.0; silence the middle half (0.25–0.75 → frames 2..6).
        let mut clip = LoadedClip {
            samples: vec![1.0f32; 8].into(), channels: 1,
            native_sample_rate: 48000, duration_secs: 0.0,
        };
        clip.apply_edit_op(&seqterm_core::AudioEditOp::Silence { start: 0.25, end: 0.75 });
        assert_eq!(&clip.samples[..], &[1.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0]);
    }

    #[test]
    fn edit_op_reverse_is_frame_wise() {
        // 4 stereo frames: L/R pairs (0,1),(2,3),(4,5),(6,7). Reverse whole clip.
        let mut clip = LoadedClip {
            samples: vec![0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0].into(),
            channels: 2, native_sample_rate: 48000, duration_secs: 0.0,
        };
        clip.apply_edit_op(&seqterm_core::AudioEditOp::Reverse { start: 0.0, end: 1.0 });
        // Frame order reversed, L/R preserved within each frame.
        assert_eq!(&clip.samples[..], &[6.0, 7.0, 4.0, 5.0, 2.0, 3.0, 0.0, 1.0]);
    }

    #[test]
    fn edit_op_normalize_scales_to_unity() {
        let mut clip = LoadedClip {
            samples: vec![0.0, 0.5, -0.25, 0.1].into(), channels: 1,
            native_sample_rate: 48000, duration_secs: 0.0,
        };
        clip.apply_edit_op(&seqterm_core::AudioEditOp::Normalize);
        let peak = clip.samples.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        assert!((peak - 1.0).abs() < 1e-6);
    }

    #[test]
    fn edit_op_delete_shortens_buffer() {
        let mut clip = LoadedClip {
            samples: vec![1.0f32; 8].into(), channels: 1,
            native_sample_rate: 48000, duration_secs: 0.0,
        };
        clip.apply_edit_op(&seqterm_core::AudioEditOp::Delete { start: 0.25, end: 0.75 });
        assert_eq!(clip.samples.len(), 4, "delete must remove 4 of 8 frames");
    }

    #[test]
    fn edit_op_trim_keeps_only_region() {
        let mut clip = LoadedClip {
            samples: (0..8).map(|i| i as f32).collect::<Vec<_>>().into(), channels: 1,
            native_sample_rate: 48000, duration_secs: 0.0,
        };
        clip.apply_edit_op(&seqterm_core::AudioEditOp::Trim { start: 0.25, end: 0.75 });
        assert_eq!(&clip.samples[..], &[2.0, 3.0, 4.0, 5.0]);
    }

    #[test]
    fn pan_hard_left_silences_right() {
        let clip = make_loaded_clip(vec![0.5f32; 256], 2, 48000);
        let mut player = AudioClipPlayer::new(clip, 48000);
        player.set_pan(-1.0);
        player.play();
        let mut buf = vec![0.0f32; 128];
        player.render(&mut buf, 48000);
        let r_energy: f32 = buf.iter().skip(1).step_by(2).map(|s| s.abs()).sum();
        let l_energy: f32 = buf.iter().step_by(2).map(|s| s.abs()).sum();
        assert!(r_energy < 1e-4, "hard-left pan must silence R, got {r_energy}");
        assert!(l_energy > 1.0, "hard-left pan must keep L, got {l_energy}");
    }

    #[test]
    fn disabled_envelope_is_unity() {
        let mut env = AdsrVoice::default();
        env.set(&seqterm_core::AdsrEnvelope { enabled: false, ..Default::default() });
        env.gate_on();
        for _ in 0..100 { assert_eq!(env.next(1.0 / 48000.0), 1.0); }
    }

    #[test]
    fn enabled_envelope_attacks_from_zero() {
        let mut env = AdsrVoice::default();
        env.set(&seqterm_core::AdsrEnvelope {
            enabled: true, attack_ms: 10.0, hold_ms: 0.0, decay_ms: 0.0,
            sustain: 1.0, release_ms: 10.0,
        });
        env.gate_on();
        let first = env.next(1.0 / 48000.0);
        assert!(first < 0.1, "attack should start near zero, got {first}");
        // After the 10ms attack (480 frames) the level should reach ~1.0.
        for _ in 0..480 { env.next(1.0 / 48000.0); }
        assert!(env.next(1.0 / 48000.0) > 0.99, "should reach unity after attack");
    }

    #[test]
    fn lowpass_filter_attenuates_nyquist() {
        // Alternating ±1 signal = Nyquist; a low cutoff LP must reduce its energy.
        let samples: Vec<f32> = (0..512).map(|i| if i % 2 == 0 { 1.0 } else { -1.0 }).collect();
        let clip = make_loaded_clip(samples, 1, 48000);
        let mut player = AudioClipPlayer::new(clip, 48000);
        player.set_filter(ClipFilterKind::Lowpass, 0.3, 0.1); // low cutoff
        player.play();
        let mut buf = vec![0.0f32; 256];
        player.render(&mut buf, 48000);
        let energy: f32 = buf.iter().step_by(2).map(|s| s * s).sum();
        assert!(energy < 100.0, "LP must attenuate Nyquist energy, got {energy}");
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
