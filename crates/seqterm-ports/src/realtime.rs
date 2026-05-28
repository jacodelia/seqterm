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
