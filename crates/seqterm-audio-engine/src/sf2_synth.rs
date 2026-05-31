//! SF2 SoundFont synthesizer wrapper around oxisynth.
//!
//! REALTIME CONTRACT: `render()`, `note_on()`, `note_off()`, `control_change()`
//! must be allocation-free. All resources are pre-allocated in `load()`.

use std::any::Any;
use std::path::Path;

use anyhow::{Result, anyhow};
use soundfont::SoundFont2;
use oxisynth::{MidiEvent, SoundFont, Synth, SynthDescriptor};

use seqterm_ports::realtime::{AudioSource, AudioSynthPort};

/// Maximum polyphony (voices) per synth instance (matches FluidSynth GM default).
const MAX_VOICES: u16 = 256;

/// Pre-allocated render buffer size (frames).
const RENDER_BUF_FRAMES: usize = 4096;

/// A SoundFont2 synthesizer slot.
///
/// Loaded once (non-RT), then driven by lock-free MIDI events from the callback.
pub struct SoundFontSynth {
    synth: Synth,
    sample_rate: u32,
    /// Pre-allocated left / right render buffers.
    render_buf_l: Vec<f32>,
    render_buf_r: Vec<f32>,
    active: bool,
    /// Fade-out envelope: (remaining_frames, total_frames).
    fade_out: Option<(usize, usize)>,
}

impl SoundFontSynth {
    /// Load an SF2 file and select bank/preset on channel 0. Called from non-RT thread.
    pub fn load(path: &Path, bank: u8, preset: u8, sample_rate: u32) -> Result<Self> {
        Self::load_multi(path, &[(0, bank, preset)], sample_rate)
    }

    /// Load one SF2 file and configure multiple MIDI channels in a single synth.
    /// `channels` is `[(midi_channel_0based, bank, preset)]`.
    /// All clips sharing the same SF2 path reuse this single instance — one file load,
    /// one block of decoded sample memory, N independent MIDI channels.
    pub fn load_multi(path: &Path, channels: &[(u8, u8, u8)], sample_rate: u32) -> Result<Self> {
        let desc = SynthDescriptor {
            sample_rate: sample_rate as f32,
            gain: 1.0,
            polyphony: MAX_VOICES,
            ..Default::default()
        };

        let mut synth = Synth::new(desc)
            .map_err(|e| anyhow!("oxisynth init failed: {e:?}"))?;

        let sf_data = std::fs::read(path)
            .map_err(|e| anyhow!("SF2 read error {:?}: {e}", path))?;

        let sf = SoundFont::load(&mut std::io::Cursor::new(sf_data))
            .map_err(|e| anyhow!("SF2 parse error: {e:?}"))?;

        let sfont_id = synth.add_font(sf, true);

        // Configure each channel's bank/preset. Failures are non-fatal: some SF2 files
        // don't expose every bank (e.g. bank 128 percussion in a non-GM file) — skip
        // those channels rather than aborting the entire load.
        for &(ch, bank, preset) in channels {
            if let Err(e) = synth.select_program(ch, sfont_id, bank as u32, preset) {
                tracing::warn!("SF2 select_program ch={ch} bank={bank} preset={preset} skipped: {e:?}");
                // Fall back to bank 0 preset 0 on this channel so it still produces sound.
                let _ = synth.select_program(ch, sfont_id, 0, 0);
            }
        }

        // GM-compatible reverb/chorus defaults on all 16 channels.
        // CC91 = reverb send (40 ≈ FluidSynth default), CC93 = chorus send (0 = off by default).
        for ch in 0u8..16 {
            let _ = synth.send_event(MidiEvent::ControlChange { channel: ch, ctrl: 91, value: 40 });
            let _ = synth.send_event(MidiEvent::ControlChange { channel: ch, ctrl: 93, value: 0  });
            // CC7 = channel volume (100 = GM default), CC10 = pan (64 = center)
            let _ = synth.send_event(MidiEvent::ControlChange { channel: ch, ctrl:  7, value: 100 });
            let _ = synth.send_event(MidiEvent::ControlChange { channel: ch, ctrl: 10, value:  64 });
        }

        Ok(Self {
            synth,
            sample_rate,
            render_buf_l: vec![0.0f32; RENDER_BUF_FRAMES],
            render_buf_r: vec![0.0f32; RENDER_BUF_FRAMES],
            active: true,
            fade_out: None,
        })
    }
}

impl AudioSource for SoundFontSynth {
    fn render(&mut self, output: &mut [f32], _sample_rate: u32) -> usize {
        let frames = output.len() / 2;
        if frames == 0 { return 0; }

        let buf_frames = frames.min(RENDER_BUF_FRAMES);

        // Write synth output into render bufs, then release the borrows.
        {
            let l = &mut self.render_buf_l[..buf_frames];
            let r = &mut self.render_buf_r[..buf_frames];
            self.synth.write((l, r));
        }

        // Apply fade-out if stopping.
        if let Some((ref mut remaining, total)) = self.fade_out {
            for i in 0..buf_frames {
                let t = remaining.saturating_sub(i);
                let gain = (t as f32 / total as f32).clamp(0.0, 1.0);
                self.render_buf_l[i] *= gain;
                self.render_buf_r[i] *= gain;
            }
            *remaining = remaining.saturating_sub(buf_frames);
            if *remaining == 0 {
                self.active = false;
            }
        }

        // Interleave L/R into the output buffer.
        for i in 0..buf_frames {
            output[i * 2]     = self.render_buf_l[i];
            output[i * 2 + 1] = self.render_buf_r[i];
        }

        buf_frames
    }

    fn is_active(&self) -> bool { self.active }

    fn stop(&mut self) {
        let fade_frames = (self.sample_rate as usize * 50) / 1000;
        self.fade_out = Some((fade_frames, fade_frames));
        for ch in 0..16u8 {
            let _ = self.synth.send_event(MidiEvent::AllNotesOff { channel: ch });
        }
    }

    fn as_any_mut(&mut self) -> &mut dyn Any { self }
}

impl SoundFontSynth {
    /// Send AllNotesOff on every MIDI channel — use on transport Stop.
    pub fn all_notes_off(&mut self) {
        for ch in 0..16u8 {
            let _ = self.synth.send_event(MidiEvent::AllNotesOff { channel: ch });
        }
    }
}

impl AudioSynthPort for SoundFontSynth {
    fn note_on(&mut self, channel: u8, note: u8, velocity: u8) {
        // Re-activate the synth and clear any pending fade-out so that notes
        // playing after a transport Stop are rendered at full gain.
        self.active   = true;
        self.fade_out = None;
        let _ = self.synth.send_event(MidiEvent::NoteOn {
            channel,
            key: note,
            vel: velocity,
        });
    }

    fn note_off(&mut self, channel: u8, note: u8) {
        let _ = self.synth.send_event(MidiEvent::NoteOff {
            channel,
            key: note,
        });
    }

    fn control_change(&mut self, channel: u8, cc: u8, value: u8) {
        let _ = self.synth.send_event(MidiEvent::ControlChange {
            channel,
            ctrl: cc,
            value,
        });
    }

    fn pitch_bend(&mut self, channel: u8, value: i16) {
        // AudioSynthPort uses signed -8192..+8191; oxisynth PitchBend uses u16 0..16383.
        let u14 = (value as i32 + 8192).clamp(0, 16383) as u16;
        let _ = self.synth.send_event(MidiEvent::PitchBend {
            channel,
            value: u14,
        });
    }
}

/// Read the preset list from an SF2 file without loading sample data.
/// Returns `(bank, preset_num, name)` sorted by bank then preset.
/// Call from a non-RT background thread only.
pub fn enumerate_sf2_presets(path: &Path) -> Vec<(u8, u8, String)> {
    let file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return Vec::new(),
    };
    let sf = match SoundFont2::load(&mut std::io::BufReader::new(file)) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let mut presets: Vec<(u8, u8, String)> = sf.presets
        .into_iter()
        .filter(|p| p.header.name != "EOP")
        .map(|p| {
            let bank   = p.header.bank.min(127) as u8;
            let num    = p.header.preset.min(127) as u8;
            let name   = p.header.name.clone();
            (bank, num, name)
        })
        .collect();
    presets.sort_by_key(|(b, p, _)| *b as u16 * 128 + *p as u16);
    presets.dedup_by_key(|(b, p, _)| (*b, *p));
    presets
}
