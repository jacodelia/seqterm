//! SF2 SoundFont synthesizer wrapper around oxisynth.
//!
//! REALTIME CONTRACT: `render()`, `note_on()`, `note_off()`, `control_change()`
//! must be allocation-free. All resources are pre-allocated in `load()`.

use std::any::Any;
use std::path::Path;
use std::sync::atomic::{AtomicU8, Ordering};

use anyhow::{Result, anyhow};
use soundfont::SoundFont2;
use oxisynth::{MidiEvent, SoundFont, Synth, SynthDescriptor};

use seqterm_fluidsynth::FluidSynthBackend;
use seqterm_ports::realtime::{AudioSource, AudioSynthPort, InstrumentBackend, PresetInfo};

/// Maximum polyphony (voices) per synth instance (matches FluidSynth GM default).
const MAX_VOICES: u16 = 256;

/// Pre-allocated render buffer size (frames).
const RENDER_BUF_FRAMES: usize = 4096;

/// Process-wide SF2 engine preference.
/// `0` = auto (consult `SEQTERM_SF2_BACKEND` env), `1` = oxisynth, `2` = fluidsynth.
static SF2_BACKEND: AtomicU8 = AtomicU8::new(0);

/// Select the SF2 engine used by subsequently-loaded [`SoundFontSynth`] instances.
///
/// `prefer_fluid` only takes effect when the crate is built with the `fluidsynth`
/// feature *and* a linkable libfluidsynth is present; otherwise loading falls back
/// to the built-in oxisynth engine. Call once at startup (e.g. from settings).
pub fn set_sf2_prefer_fluidsynth(prefer_fluid: bool) {
    SF2_BACKEND.store(if prefer_fluid { 2 } else { 1 }, Ordering::Relaxed);
}

/// Whether this build can actually run FluidSynth (the `fluidsynth` feature was
/// compiled in and a libfluidsynth was linked). When `false`, selecting the
/// FluidSynth engine transparently falls back to oxisynth.
pub fn fluidsynth_available() -> bool {
    cfg!(any(feature = "fluidsynth", feature = "fluidsynth-system"))
}

/// Whether new SF2 instances should try the FluidSynth engine first.
/// Falls back to the `SEQTERM_SF2_BACKEND=fluidsynth` environment variable when
/// no explicit preference has been set via [`set_sf2_prefer_fluidsynth`].
pub fn sf2_prefer_fluidsynth() -> bool {
    match SF2_BACKEND.load(Ordering::Relaxed) {
        2 => true,
        1 => false,
        _ => std::env::var("SEQTERM_SF2_BACKEND")
            .map(|v| v.eq_ignore_ascii_case("fluidsynth") || v.eq_ignore_ascii_case("fluid"))
            .unwrap_or(false),
    }
}

/// The concrete sample engine driving a [`SoundFontSynth`].
enum Sf2Engine {
    /// Built-in pure-Rust engine (default).
    Oxi(Synth),
    /// External libfluidsynth engine (when requested and available).
    Fluid(FluidSynthBackend),
}

/// A SoundFont2 synthesizer slot.
///
/// Loaded once (non-RT), then driven by lock-free MIDI events from the callback.
/// Internally backed by either oxisynth or libfluidsynth (see [`Sf2Engine`]),
/// chosen at load time — the public surface is identical either way.
pub struct SoundFontSynth {
    engine: Sf2Engine,
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
        // Try the FluidSynth engine first when requested. It only "counts" if a
        // real libfluidsynth is linked in (is_real); the silent stub falls back.
        if sf2_prefer_fluidsynth() {
            match FluidSynthBackend::new(path.to_path_buf(), channels, sample_rate) {
                Ok(fluid) if fluid.is_real() => {
                    tracing::info!("SF2 loaded via FluidSynth: {}", path.display());
                    return Ok(Self {
                        engine: Sf2Engine::Fluid(fluid),
                        sample_rate,
                        render_buf_l: vec![0.0f32; RENDER_BUF_FRAMES],
                        render_buf_r: vec![0.0f32; RENDER_BUF_FRAMES],
                        active: true,
                        fade_out: None,
                    });
                }
                Ok(_) => tracing::warn!(
                    "FluidSynth backend requested but not compiled in; using oxisynth for {}",
                    path.display()
                ),
                Err(e) => tracing::warn!(
                    "FluidSynth load failed ({e}); using oxisynth for {}",
                    path.display()
                ),
            }
        }

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
            engine: Sf2Engine::Oxi(synth),
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
            match &mut self.engine {
                Sf2Engine::Oxi(synth) => synth.write((l, r)),
                Sf2Engine::Fluid(fluid) => fluid.render_into(l, r),
            }
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
        self.all_notes_off();
    }

    fn as_any_mut(&mut self) -> &mut dyn Any { self }

    fn as_synth(&mut self) -> Option<&mut dyn AudioSynthPort> { Some(self) }
}

impl SoundFontSynth {
    /// Send AllNotesOff on every MIDI channel — use on transport Stop.
    pub fn all_notes_off(&mut self) {
        match &mut self.engine {
            Sf2Engine::Oxi(synth) => {
                for ch in 0..16u8 {
                    let _ = synth.send_event(MidiEvent::AllNotesOff { channel: ch });
                }
            }
            Sf2Engine::Fluid(fluid) => fluid.all_notes_off(),
        }
    }
}

impl AudioSynthPort for SoundFontSynth {
    fn note_on(&mut self, channel: u8, note: u8, velocity: u8) {
        // Re-activate the synth and clear any pending fade-out so that notes
        // playing after a transport Stop are rendered at full gain.
        self.active   = true;
        self.fade_out = None;
        match &mut self.engine {
            Sf2Engine::Oxi(synth) => {
                let _ = synth.send_event(MidiEvent::NoteOn { channel, key: note, vel: velocity });
            }
            Sf2Engine::Fluid(fluid) => fluid.note_on(channel, note, velocity),
        }
    }

    fn note_off(&mut self, channel: u8, note: u8) {
        match &mut self.engine {
            Sf2Engine::Oxi(synth) => {
                let _ = synth.send_event(MidiEvent::NoteOff { channel, key: note });
            }
            Sf2Engine::Fluid(fluid) => fluid.note_off(channel, note),
        }
    }

    fn control_change(&mut self, channel: u8, cc: u8, value: u8) {
        match &mut self.engine {
            Sf2Engine::Oxi(synth) => {
                let _ = synth.send_event(MidiEvent::ControlChange { channel, ctrl: cc, value });
            }
            Sf2Engine::Fluid(fluid) => fluid.control_change(channel, cc, value),
        }
    }

    fn pitch_bend(&mut self, channel: u8, value: i16) {
        match &mut self.engine {
            Sf2Engine::Oxi(synth) => {
                // AudioSynthPort uses signed -8192..+8191; oxisynth uses u16 0..16383.
                let u14 = (value as i32 + 8192).clamp(0, 16383) as u16;
                let _ = synth.send_event(MidiEvent::PitchBend { channel, value: u14 });
            }
            Sf2Engine::Fluid(fluid) => fluid.pitch_bend(channel, value),
        }
    }

    fn all_notes_off(&mut self) {
        // Delegate to the native per-engine path (inherent method).
        SoundFontSynth::all_notes_off(self);
    }
}

impl InstrumentBackend for SoundFontSynth {
    fn backend_name(&self) -> &str {
        match &self.engine {
            Sf2Engine::Oxi(_) => "SF2 (oxisynth)",
            Sf2Engine::Fluid(_) => "SF2 (FluidSynth)",
        }
    }

    fn select_preset(&mut self, bank: u16, program: u8) -> anyhow::Result<()> {
        match &mut self.engine {
            Sf2Engine::Oxi(synth) => {
                // oxisynth: select on channel 0 (single-channel use case).
                synth.send_event(oxisynth::MidiEvent::ControlChange { channel: 0, ctrl: 0,  value: (bank >> 7) as u8 })
                    .map_err(|e| anyhow::anyhow!("{e:?}"))?;
                synth.send_event(oxisynth::MidiEvent::ControlChange { channel: 0, ctrl: 32, value: (bank & 0x7F) as u8 })
                    .map_err(|e| anyhow::anyhow!("{e:?}"))?;
                synth.send_event(oxisynth::MidiEvent::ProgramChange { channel: 0, program_id: program })
                    .map_err(|e| anyhow::anyhow!("{e:?}"))?;
            }
            Sf2Engine::Fluid(fluid) => fluid.select_preset(bank, program),
        }
        Ok(())
    }

    fn list_presets(&self) -> Vec<PresetInfo> {
        match &self.engine {
            // oxisynth doesn't expose a preset list after load.
            Sf2Engine::Oxi(_) => Vec::new(),
            Sf2Engine::Fluid(fluid) => fluid.list_presets(),
        }
    }

    fn all_notes_off(&mut self) {
        SoundFontSynth::all_notes_off(self);
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
            // Allow bank 128 (GM percussion). Values above 128 are non-standard; clamp to 128.
            let bank = p.header.bank.min(128) as u8;
            let num  = p.header.preset.min(127) as u8;
            let name = p.header.name.clone();
            (bank, num, name)
        })
        .collect();
    presets.sort_by_key(|(b, p, _)| *b as u16 * 128 + *p as u16);
    presets.dedup_by_key(|(b, p, _)| (*b, *p));
    presets
}

#[cfg(test)]
mod tests {
    #[test]
    fn bank_select_msb_lsb_encoding() {
        // The select_preset implementation encodes bank as CC0 (MSB) + CC32 (LSB).
        // Verify the bit-math for the key cases.

        // bank 0 (GM melodic): MSB=0, LSB=0
        let bank: u16 = 0;
        assert_eq!((bank >> 7) as u8, 0, "bank 0 MSB");
        assert_eq!((bank & 0x7F) as u8, 0, "bank 0 LSB");

        // bank 128 (GM2 / XG percussion): MSB=1, LSB=0
        let bank: u16 = 128;
        assert_eq!((bank >> 7) as u8, 1, "bank 128 MSB should be 1");
        assert_eq!((bank & 0x7F) as u8, 0, "bank 128 LSB should be 0");

        // bank 1 (variation bank): MSB=0, LSB=1
        let bank: u16 = 1;
        assert_eq!((bank >> 7) as u8, 0, "bank 1 MSB");
        assert_eq!((bank & 0x7F) as u8, 1, "bank 1 LSB");

        // bank 256 (XG high bank): MSB=2, LSB=0
        let bank: u16 = 256;
        assert_eq!((bank >> 7) as u8, 2, "bank 256 MSB should be 2");
        assert_eq!((bank & 0x7F) as u8, 0, "bank 256 LSB should be 0");

        // bank 129 (MSB=1, LSB=1): used by some GS/XG percussion variations
        let bank: u16 = 129;
        assert_eq!((bank >> 7) as u8, 1, "bank 129 MSB");
        assert_eq!((bank & 0x7F) as u8, 1, "bank 129 LSB");
    }

    #[test]
    fn bank_select_roundtrip() {
        // Verify MSB<<7|LSB reconstructs the original bank number.
        for bank in [0u16, 1, 64, 127, 128, 129, 255, 256, 300] {
            let msb = (bank >> 7) as u16;
            let lsb = (bank & 0x7F) as u16;
            let reconstructed = (msb << 7) | lsb;
            assert_eq!(reconstructed, bank,
                "bank {bank} should roundtrip via MSB={msb} LSB={lsb}");
        }
    }
}
