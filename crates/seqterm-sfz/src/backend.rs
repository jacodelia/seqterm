//! `InstrumentBackend` adapter for SFZ instruments.
//!
//! Each note-on selects a matching region, decodes the sample with `symphonia`,
//! and mixes its output into the render buffer.  Up to 32 simultaneous voices
//! are supported; oldest voice is stolen when the limit is exceeded.

use std::path::PathBuf;
use seqterm_ports::realtime::{AudioSource, AudioSynthPort, InstrumentBackend, PresetInfo};
use crate::{SfzInstrument, SfzRegion};

const MAX_VOICES: usize = 32;

// ─── Voice ────────────────────────────────────────────────────────────────────

struct Voice {
    note:     u8,
    channel:  u8,
    gain:     f32,
    rate:     f32,   // playback rate multiplier (for pitch transposition)
    samples:  Vec<f32>, // decoded stereo f32 PCM
    pos:      f64,   // read head (fractional sample index)
    active:   bool,
}


/// Decode a WAV/FLAC/MP3 sample file to interleaved stereo f32 at `target_sr`.
fn load_samples(path: &PathBuf, target_sr: u32) -> anyhow::Result<Vec<f32>> {
    use symphonia::core::audio::SampleBuffer;
    use symphonia::core::codecs::DecoderOptions;
    use symphonia::core::formats::FormatOptions;
    use symphonia::core::io::MediaSourceStream;
    use symphonia::core::meta::MetadataOptions;
    use symphonia::core::probe::Hint;

    let file = std::fs::File::open(path)?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let probed = symphonia::default::get_probe()
        .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())?;
    let mut format = probed.format;
    let track = format.default_track().ok_or_else(|| anyhow::anyhow!("no default track"))?;
    let track_id = track.id;
    let n_channels = track.codec_params.channels.map(|c| c.count()).unwrap_or(1);
    let file_sr = track.codec_params.sample_rate.unwrap_or(target_sr);
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())?;
    let mut raw: Vec<f32> = Vec::new();
    loop {
        let packet = match format.next_packet() {
            Ok(p) if p.track_id() == track_id => p,
            Ok(_) => continue,
            Err(_) => break,
        };
        if let Ok(decoded) = decoder.decode(&packet) {
            let spec = *decoded.spec();
            let mut buf: SampleBuffer<f32> = SampleBuffer::new(decoded.capacity() as u64, spec);
            buf.copy_interleaved_ref(decoded);
            raw.extend_from_slice(buf.samples());
        }
    }
    // Downmix to stereo.
    let mut stereo: Vec<f32> = Vec::with_capacity(raw.len() / n_channels * 2);
    let ch = n_channels.max(1);
    for frame in raw.chunks_exact(ch) {
        let l = frame[0];
        let r = if ch > 1 { frame[1] } else { l };
        stereo.push(l);
        stereo.push(r);
    }
    // Resample if needed (simple linear interpolation).
    if file_sr != target_sr && !stereo.is_empty() {
        let ratio = file_sr as f64 / target_sr as f64;
        let out_frames = (stereo.len() / 2) as f64 / ratio;
        let mut resampled = Vec::with_capacity(out_frames as usize * 2);
        let mut pos = 0.0f64;
        let in_frames = stereo.len() / 2;
        while pos < in_frames as f64 - 1.0 {
            let i0 = pos as usize;
            let i1 = (i0 + 1).min(in_frames - 1);
            let t = (pos - i0 as f64) as f32;
            let l = stereo[i0 * 2]     + t * (stereo[i1 * 2]     - stereo[i0 * 2]);
            let r = stereo[i0 * 2 + 1] + t * (stereo[i1 * 2 + 1] - stereo[i0 * 2 + 1]);
            resampled.push(l);
            resampled.push(r);
            pos += ratio;
        }
        return Ok(resampled);
    }
    Ok(stereo)
}

// ─── SfzBackend ───────────────────────────────────────────────────────────────

/// Realtime SFZ instrument backend.  Holds one loaded instrument and manages voices.
pub struct SfzBackend {
    instrument:  SfzInstrument,
    voices:      Vec<Voice>,
    sample_rate: u32,
    active:      bool,
}

impl SfzBackend {
    pub fn new(instrument: SfzInstrument) -> Self {
        Self {
            instrument,
            voices: Vec::with_capacity(MAX_VOICES),
            sample_rate: 48000,
            active: true,
        }
    }

    fn find_region(&self, note: u8, vel: u8) -> Option<&SfzRegion> {
        self.instrument.regions.iter().find(|r| r.matches(note, vel))
    }
}

// ── AudioSource ───────────────────────────────────────────────────────────────

impl AudioSource for SfzBackend {
    fn render(&mut self, output: &mut [f32], sample_rate: u32) -> usize {
        self.sample_rate = sample_rate;
        let frames = output.len() / 2;
        for v in &mut self.voices {
            if !v.active { continue; }
            for f in 0..frames {
                if v.pos as usize * 2 + 1 >= v.samples.len() {
                    v.active = false;
                    break;
                }
                let i0 = v.pos as usize;
                let i1 = (i0 + 1).min(v.samples.len() / 2 - 1);
                let t  = (v.pos - i0 as f64) as f32;
                let l  = v.samples[i0 * 2]     + t * (v.samples[i1 * 2]     - v.samples[i0 * 2]);
                let r  = v.samples[i0 * 2 + 1] + t * (v.samples[i1 * 2 + 1] - v.samples[i0 * 2 + 1]);
                output[f * 2]     += l * v.gain;
                output[f * 2 + 1] += r * v.gain;
                v.pos += v.rate as f64;
            }
        }
        self.voices.retain(|v| v.active);
        frames
    }

    fn is_active(&self) -> bool { self.active }

    fn stop(&mut self) {
        self.voices.clear();
        self.active = false;
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any { self }
}

// ── AudioSynthPort ────────────────────────────────────────────────────────────

impl AudioSynthPort for SfzBackend {
    fn note_on(&mut self, channel: u8, note: u8, velocity: u8) {
        if velocity == 0 {
            self.note_off(channel, note);
            return;
        }
        // Clone the region data we need before borrowing voices mutably.
        let region_data = self.find_region(note, velocity).map(|r| {
            (r.sample.clone(), r.rate_for_note(note), r.gain)
        });
        if let Some((sample_path, rate, region_gain)) = region_data {
            if self.voices.len() >= MAX_VOICES {
                self.voices.remove(0);
            }
            let samples = load_samples(&sample_path, self.sample_rate).unwrap_or_default();
            let vel_gain = velocity as f32 / 127.0;
            let is_active = !samples.is_empty();
            self.voices.push(Voice {
                note, channel,
                gain: region_gain * vel_gain,
                rate,
                samples,
                pos: 0.0,
                active: is_active,
            });
        }
    }

    fn note_off(&mut self, channel: u8, note: u8) {
        // SFZ one-shot behaviour: note-off stops the voice immediately.
        self.voices.retain(|v| !(v.channel == channel && v.note == note));
    }

    fn control_change(&mut self, _channel: u8, _cc: u8, _value: u8) {}

    fn pitch_bend(&mut self, _channel: u8, _value: i16) {}
}

// ── InstrumentBackend ─────────────────────────────────────────────────────────

impl InstrumentBackend for SfzBackend {
    fn backend_name(&self) -> &str { "SFZ" }

    fn select_preset(&mut self, _bank: u16, _program: u8) -> anyhow::Result<()> {
        Ok(()) // SFZ has no bank/program concept; instrument is fixed at load
    }

    fn list_presets(&self) -> Vec<PresetInfo> {
        vec![PresetInfo {
            bank: 0,
            program: 0,
            name: self.instrument.base_dir
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("SFZ Instrument")
                .to_string(),
        }]
    }

    fn all_notes_off(&mut self) {
        self.voices.clear();
    }
}
