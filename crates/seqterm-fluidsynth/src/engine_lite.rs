//! Embedded FluidLite engine.
//!
//! FluidLite is the FluidSynth synthesis core with GLib and all OS/driver code
//! stripped out, so it compiles straight into the SeqTerm binary (bundled C,
//! built by the `cc` crate) — no system libfluidsynth, no shared library, no
//! runtime dependency. It keeps SF2/SF3 support, velocity layers, modulators
//! and the FluidSynth reverb/chorus.

use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard, Once};

use fluidlite::{FnLogger, IsSettings, Log, LogLevel, Settings, Synth};
use seqterm_ports::realtime::PresetInfo;

/// This engine produces real sound.
pub const REAL: bool = true;

static LOG_INIT: Once = Once::new();

/// FluidLite (unlike full FluidSynth 2.x) is **not** internally thread-safe: it
/// keeps process-global C state (error buffer, soundfont loader, log handler).
/// SeqTerm loads SoundFonts on background threads while the audio thread renders
/// other synths, so concurrent FluidLite calls would race that global state and
/// crash (SIGSEGV). This global lock serializes *every* FluidLite C call across
/// all engine instances. Contention is normally nil; during a (rare) soundfont
/// load the audio thread may briefly wait — far preferable to a hard crash.
static FLUID_LOCK: Mutex<()> = Mutex::new(());

/// Acquire the global FluidLite lock, ignoring poisoning (a previous panic while
/// holding it must not deadlock subsequent audio rendering).
#[inline]
fn fluid_guard() -> MutexGuard<'static, ()> {
    FLUID_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

/// Redirect FluidLite's C log output away from stderr/stdout (its default),
/// which would otherwise corrupt the ratatui TUI's alternate-screen rendering.
/// Messages are forwarded to `tracing` instead. Called once before any synth.
fn install_log_redirect() {
    LOG_INIT.call_once(|| {
        Log::set(
            LogLevel::DEBUG, // [Panic, Error, Warning, Info, Debug]
            FnLogger::from(|level: LogLevel, message: &str| match level {
                LogLevel::Panic | LogLevel::Error => tracing::error!("fluidlite: {message}"),
                LogLevel::Warning => tracing::warn!("fluidlite: {message}"),
                LogLevel::Info => tracing::info!("fluidlite: {message}"),
                LogLevel::Debug => tracing::debug!("fluidlite: {message}"),
            }),
        );
    });
}

/// Wraps a FluidLite `Synth` so that its (C-state-touching) destructor runs while
/// the global [`FLUID_LOCK`] is held — otherwise dropping a synth on the audio
/// thread could race a soundfont load on a background thread.
struct LockedSynth(std::mem::ManuallyDrop<Synth>);

impl std::ops::Deref for LockedSynth {
    type Target = Synth;
    fn deref(&self) -> &Synth { &self.0 }
}

impl Drop for LockedSynth {
    fn drop(&mut self) {
        let _guard = fluid_guard();
        // SAFETY: dropped exactly once, here, while holding the global lock.
        unsafe { std::mem::ManuallyDrop::drop(&mut self.0); }
    }
}

pub struct Engine {
    synth: LockedSynth,
    #[allow(dead_code)]
    sf2_path: PathBuf,
    font_id: u32,
}

impl Engine {
    pub fn new(
        sf2_path: PathBuf,
        channels: &[(u8, u8, u8)],
        sample_rate: u32,
    ) -> anyhow::Result<Self> {
        // Hold the global lock for the entire (non-RT) construction: log setup,
        // settings, synth creation and sfload all touch FluidLite's global C state.
        let _guard = fluid_guard();
        install_log_redirect();
        let settings = Settings::new()
            .map_err(|e| anyhow::anyhow!("fluidlite settings: {e}"))?;
        if let Some(s) = settings.num("synth.sample-rate") { s.set(sample_rate as f64); }
        if let Some(s) = settings.int("synth.polyphony")   { s.set(512); }

        let synth = Synth::new(settings)
            .map_err(|e| anyhow::anyhow!("fluidlite synth: {e}"))?;
        synth.set_gain(1.0);
        synth.set_reverb_on(true);
        synth.set_chorus_on(true);

        let font_id = synth.sfload(&sf2_path, true)
            .map_err(|e| anyhow::anyhow!("fluidlite sfload {}: {e}", sf2_path.display()))?;

        // Per-channel bank/preset, with a graceful fallback to 0/0.
        for &(ch, bank, preset) in channels {
            if synth.program_select(ch as u32, font_id, bank as u32, preset as u32).is_err() {
                tracing::warn!(
                    "FluidLite program_select ch={ch} bank={bank} preset={preset} failed; falling back to 0/0"
                );
                let _ = synth.program_select(ch as u32, font_id, 0, 0);
            }
        }

        // GM-compatible reverb/chorus/volume/pan defaults on all channels.
        for ch in 0..16u32 {
            let _ = synth.cc(ch, 91, 40);  // reverb send
            let _ = synth.cc(ch, 93, 0);   // chorus send
            let _ = synth.cc(ch, 7, 100);  // channel volume
            let _ = synth.cc(ch, 10, 64);  // pan center
        }

        Ok(Self {
            synth: LockedSynth(std::mem::ManuallyDrop::new(synth)),
            sf2_path,
            font_id,
        })
    }

    pub fn render_into(&mut self, l: &mut [f32], r: &mut [f32]) {
        let _guard = fluid_guard();
        let _ = self.synth.write((l, r));
    }

    pub fn note_on(&mut self, channel: u8, note: u8, velocity: u8) {
        let _guard = fluid_guard();
        let _ = self.synth.note_on(channel as u32, note as u32, velocity as u32);
    }

    pub fn note_off(&mut self, channel: u8, note: u8) {
        let _guard = fluid_guard();
        let _ = self.synth.note_off(channel as u32, note as u32);
    }

    pub fn control_change(&mut self, channel: u8, cc: u8, value: u8) {
        let _guard = fluid_guard();
        let _ = self.synth.cc(channel as u32, cc as u32, value as u32);
    }

    pub fn pitch_bend(&mut self, channel: u8, value: i16) {
        // AudioSynthPort uses signed -8192..+8191; fluidlite uses 0..16383.
        let v = (value as i32 + 8192).clamp(0, 16383) as u32;
        let _guard = fluid_guard();
        let _ = self.synth.pitch_bend(channel as u32, v);
    }

    pub fn all_notes_off(&mut self) {
        // FluidLite has no all-notes-off helper; CC 123 (All Notes Off) per channel.
        let _guard = fluid_guard();
        for ch in 0..16u32 {
            let _ = self.synth.cc(ch, 123, 0);
        }
    }

    pub fn select_preset(&mut self, bank: u16, program: u8) {
        let _guard = fluid_guard();
        let _ = self.synth.program_select(0, self.font_id, bank as u32, program as u32);
    }

    pub fn list_presets(&self) -> Vec<PresetInfo> {
        // Preset listing for the SF2 browser comes from a file-parse path
        // (enumerate_sf2_presets); the engine doesn't need to enumerate.
        Vec::new()
    }
}
