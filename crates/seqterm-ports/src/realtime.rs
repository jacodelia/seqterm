//! Realtime-safe port traits.
//!
//! RULE: All methods on these traits MUST be:
//! - Allocation-free (no Vec, Box, String creation)
//! - Lock-free (no Mutex, RwLock)
//! - Non-blocking (no sleep, no I/O)
//! - Deterministic-time (bounded execution)

/// A source of interleaved stereo f32 audio samples.
/// Called from the audio callback — must be realtime-safe.
pub trait AudioSource: Send + 'static {
    /// Fill `output` (interleaved stereo f32) with the next block of audio.
    /// `sample_rate`: current engine sample rate.
    /// Returns the number of frames written (may be < output.len()/2 when finished).
    fn render(&mut self, output: &mut [f32], sample_rate: u32) -> usize;

    /// Whether this source is still producing audio.
    fn is_active(&self) -> bool;

    /// Graceful stop: fade out over next block instead of hard-cutting.
    fn stop(&mut self);

    /// Downcast support — allows the audio callback to recover the concrete type
    /// for type-specific operations (e.g. note_on on SoundFontSynth).
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any;

    /// If this source is also a MIDI-driven synth, return it as one. Lets the
    /// audio callback route note/CC/pitch-bend events to any synth (SF2, LV2, …)
    /// without knowing the concrete type. Non-synth sources return `None`.
    fn as_synth(&mut self) -> Option<&mut dyn AudioSynthPort> {
        None
    }
}

/// A synthesizer that can receive MIDI events and render audio.
/// Called from the audio callback — must be realtime-safe.
pub trait AudioSynthPort: AudioSource {
    /// Trigger a note-on event.
    fn note_on(&mut self, channel: u8, note: u8, velocity: u8);

    /// Trigger a note-off event.
    fn note_off(&mut self, channel: u8, note: u8);

    /// Send a CC message.
    fn control_change(&mut self, channel: u8, cc: u8, value: u8);

    /// Send a pitch-bend. `value` is -8192..+8191.
    fn pitch_bend(&mut self, channel: u8, value: i16);

    /// Send a channel pressure (aftertouch) value `0..=127`. Default no-op; used
    /// for MPE per-note pressure by backends with native note expression (CLAP).
    fn channel_pressure(&mut self, _channel: u8, _value: u8) {}

    /// Silence all sounding voices on every channel. The default sends an
    /// "All Notes Off" CC (123) on all 16 channels; backends with a faster
    /// native path (e.g. SF2) may override.
    fn all_notes_off(&mut self) {
        for ch in 0..16u8 {
            self.control_change(ch, 123, 0);
        }
    }

    /// Configure polyphonic (MPE) expression: when enabled, the backend should
    /// treat per-channel pitch-bend / timbre as per-note expression, using
    /// `bend_semitones` as the per-note pitch-bend range. Default: no-op (the
    /// backend keeps treating these as plain channel messages). Implemented by
    /// backends with native per-note expression (e.g. the CLAP host).
    fn set_mpe(&mut self, _enabled: bool, _bend_semitones: f64) {}

    /// Serialize the backend's opaque instrument state for persistence (e.g. a
    /// hosted plugin's preset/parameter blob). Default: `None` (no state).
    fn save_state(&mut self) -> Option<Vec<u8>> { None }

    /// Restore opaque instrument state previously produced by [`Self::save_state`].
    /// Returns `true` if applied. Default: no-op.
    fn load_state(&mut self, _bytes: &[u8]) -> bool { false }
}

/// Sink for realtime events flowing from the audio callback back to non-RT world.
/// Must be lock-free (ring buffer backed).
pub trait RealtimeEventSink: Send + Sync {
    /// Push an event. Must never block or allocate. May silently drop if full.
    fn push_note_on(&self, channel: u8, note: u8, velocity: u8);
    fn push_note_off(&self, channel: u8, note: u8);
    fn push_xrun(&self);
    fn push_dsp_load(&self, percent: f32);
}

/// Preset descriptor returned by `InstrumentBackend::list_presets`.
#[derive(Debug, Clone)]
pub struct PresetInfo {
    pub bank: u16,
    pub program: u8,
    pub name: String,
}

/// Abstract instrument engine — a realtime synthesizer that can be configured
/// with presets and driven via MIDI-style events.
///
/// Adapters: `SoundFontSynth` (SF2), future `SfzSynth` (SFZ), `Vst3Instrument` (VST3), etc.
///
/// REALTIME CONTRACT: Only `note_on`, `note_off`, `control_change`, `pitch_bend`,
/// `render`, `stop` are called from the audio callback. All other methods are non-RT.
pub trait InstrumentBackend: AudioSynthPort {
    /// Human-readable name of this backend (e.g. "SF2 (oxisynth)", "SFZ", "VST3").
    fn backend_name(&self) -> &str;

    /// Select a preset (non-RT). Called on a loader thread before the synth is
    /// installed into the mixer slot.
    fn select_preset(&mut self, bank: u16, program: u8) -> anyhow::Result<()>;

    /// Return all available presets (non-RT).
    fn list_presets(&self) -> Vec<PresetInfo>;

    /// Send all-notes-off on all channels (may be called from either context).
    fn all_notes_off(&mut self);
}
