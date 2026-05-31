//! Commands and events for the audio engine.

use std::path::PathBuf;

use seqterm_core::{GrainParams, GranularZone};
use seqterm_ports::realtime::AudioSource;
use crate::fx::FxProcessor;

/// Commands sent from non-RT world → audio engine (via rtrb ring buffer).
///
/// Not `Clone` because `InstallSource` carries a `Box<dyn AudioSource>`.
pub enum AudioCommand {
    /// Load an SF2 file into a named slot. Non-RT: actual load happens on asset thread.
    LoadSf2 { slot_id: u32, path: PathBuf, bank: u8, preset: u8 },
    /// Load an audio file into a named slot (background decode).
    LoadAudioFile { slot_id: u32, path: PathBuf, looping: bool, original_bpm: f64 },
    /// Unload a slot (frees synth / sample memory).
    UnloadSlot { slot_id: u32 },
    /// Trigger NoteOn on a synth slot.
    NoteOn { slot_id: u32, channel: u8, note: u8, velocity: u8 },
    /// Trigger NoteOff on a synth slot.
    NoteOff { slot_id: u32, channel: u8, note: u8 },
    /// Send AllNotesOff on every MIDI channel (0-15) for a synth slot.
    /// Use on transport Stop to silence stuck notes.
    AllNotesOff { slot_id: u32 },
    /// Send MIDI CC to a synth slot.
    ControlChange { slot_id: u32, channel: u8, cc: u8, value: u8 },
    /// Trigger audio clip playback.
    PlayAudioClip { slot_id: u32 },
    /// Stop audio clip playback (fade-out).
    StopAudioClip { slot_id: u32 },
    /// Set master output volume (0.0 - 2.0).
    SetMasterVolume(f32),
    /// Set per-slot volume (0.0 - 2.0).
    SetSlotVolume { slot_id: u32, volume: f32 },
    /// Set per-slot post-fader send levels to bus A and bus B (0.0 = off, 1.0 = full).
    SetSlotSends { slot_id: u32, send_a: f32, send_b: f32 },
    /// Set return volume for a bus (0.0 - 2.0).
    SetBusVolume { bus_idx: usize, volume: f32 },
    /// Mute or unmute a bus return.
    SetBusMuted { bus_idx: usize, muted: bool },
    /// Shutdown the audio engine.
    Shutdown,
    /// Install a loaded AudioSource into a mixer slot.
    /// Sent from the non-RT asset-loading thread → RT callback via the ring buffer.
    InstallSource { slot_id: u32, source: Box<dyn AudioSource> },
    /// Start capturing mixed stereo output to WAV.
    /// `done` is set to `true` by `StopCapture`; the writer thread exits once set + ring drained.
    StartCapture {
        capture_tx: rtrb::Producer<f32>,
        done: std::sync::Arc<std::sync::atomic::AtomicBool>,
    },
    /// Signal the RT callback to stop writing capture data.
    StopCapture,
    /// Replace the FX chain on a slot. Pre-constructed processors, no RT alloc.
    SetSlotFxChain { slot_id: u32, chain: Vec<Box<dyn FxProcessor>> },
    /// Clear all FX from a slot (drops the chain on the RT thread — acceptable for rare ops).
    ClearSlotFx { slot_id: u32 },
    /// Freeze the GranularEngine in the given slot (snapshot source into freeze buffer).
    FreezeGranular { slot_id: u32 },
    /// Unfreeze the GranularEngine in the given slot (revert to live source).
    UnfreezeGranular { slot_id: u32 },
    /// Set loop region on an AudioClipPlayer slot (fractions of total clip length, 0.0–1.0).
    SetLoopPoints { slot_id: u32, start_frac: f32, end_frac: f32 },
    /// Update grain parameters on a GranularEngine slot.
    SetGranularParams { slot_id: u32, params: GrainParams },
    /// Update the granular zone (position, range, scan) on a GranularEngine slot.
    SetGranularZone { slot_id: u32, zone: GranularZone },
    /// Update the modulation matrix on a GranularEngine slot.
    SetGranularMod { slot_id: u32, mod_matrix: seqterm_core::GranularMod },
    /// Connect a mixer slot as live audio input to a granular engine slot.
    /// `source_slot_id = None` disables live mode and restores loaded-sample mode.
    SetGranularLiveSource { granular_slot_id: u32, source_slot_id: Option<u32> },
    /// Enable or disable reverse playback on an AudioClipPlayer slot.
    SetReverse { slot_id: u32, reverse: bool },
    /// Set pitch offset in semitones on an AudioClipPlayer slot (vinyl-style: shifts pitch + speed).
    SetPitchSt { slot_id: u32, semitones: f32 },
    /// Set hard trim points on an AudioClipPlayer slot (fractions of total clip length, 0.0–1.0).
    /// Trim constrains the absolute playback range; loop points operate within trim.
    SetPlaybackRange { slot_id: u32, start_frac: f32, end_frac: f32 },
    /// Replace the master bus FX chain. Pre-constructed processors, no RT alloc.
    SetMasterFxChain { chain: Vec<Box<dyn FxProcessor>> },
    /// Clear all FX from the master bus.
    ClearMasterFx,
}

impl std::fmt::Debug for AudioCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::LoadSf2      { slot_id, .. } => write!(f, "LoadSf2(slot={slot_id})"),
            Self::LoadAudioFile{ slot_id, .. } => write!(f, "LoadAudioFile(slot={slot_id})"),
            Self::UnloadSlot   { slot_id }     => write!(f, "UnloadSlot(slot={slot_id})"),
            Self::NoteOn       { slot_id, note, .. } => write!(f, "NoteOn(slot={slot_id}, note={note})"),
            Self::NoteOff      { slot_id, note, .. } => write!(f, "NoteOff(slot={slot_id}, note={note})"),
            Self::AllNotesOff  { slot_id }           => write!(f, "AllNotesOff(slot={slot_id})"),
            Self::ControlChange{ slot_id, cc, .. }   => write!(f, "CC(slot={slot_id}, cc={cc})"),
            Self::PlayAudioClip{ slot_id }     => write!(f, "PlayAudioClip(slot={slot_id})"),
            Self::StopAudioClip{ slot_id }     => write!(f, "StopAudioClip(slot={slot_id})"),
            Self::SetMasterVolume(v)           => write!(f, "SetMasterVolume({v:.2})"),
            Self::SetSlotVolume{ slot_id, volume } => write!(f, "SetSlotVolume(slot={slot_id}, vol={volume:.2})"),
            Self::SetSlotSends { slot_id, send_a, send_b } => write!(f, "SetSlotSends(slot={slot_id}, a={send_a:.2}, b={send_b:.2})"),
            Self::SetBusVolume { bus_idx, volume } => write!(f, "SetBusVolume(bus={bus_idx}, vol={volume:.2})"),
            Self::SetBusMuted  { bus_idx, muted }  => write!(f, "SetBusMuted(bus={bus_idx}, muted={muted})"),
            Self::Shutdown                     => write!(f, "Shutdown"),
            Self::InstallSource{ slot_id, .. } => write!(f, "InstallSource(slot={slot_id})"),
            Self::StartCapture { .. }          => write!(f, "StartCapture"),
            Self::StopCapture                  => write!(f, "StopCapture"),
            Self::SetSlotFxChain { slot_id, chain } => write!(f, "SetSlotFxChain(slot={slot_id}, len={})", chain.len()),
            Self::ClearSlotFx { slot_id }      => write!(f, "ClearSlotFx(slot={slot_id})"),
            Self::FreezeGranular   { slot_id } => write!(f, "FreezeGranular(slot={slot_id})"),
            Self::UnfreezeGranular { slot_id } => write!(f, "UnfreezeGranular(slot={slot_id})"),
            Self::SetLoopPoints { slot_id, start_frac, end_frac } =>
                write!(f, "SetLoopPoints(slot={slot_id}, {start_frac:.2}–{end_frac:.2})"),
            Self::SetGranularParams { slot_id, .. } => write!(f, "SetGranularParams(slot={slot_id})"),
            Self::SetGranularZone   { slot_id, .. } => write!(f, "SetGranularZone(slot={slot_id})"),
            Self::SetGranularMod    { slot_id, .. } => write!(f, "SetGranularMod(slot={slot_id})"),
            Self::SetGranularLiveSource { granular_slot_id, source_slot_id } =>
                write!(f, "SetGranularLiveSource(gran={granular_slot_id}, src={source_slot_id:?})"),
            Self::SetReverse { slot_id, reverse }   => write!(f, "SetReverse(slot={slot_id}, rev={reverse})"),
            Self::SetPitchSt { slot_id, semitones } => write!(f, "SetPitchSt(slot={slot_id}, st={semitones:.1})"),
            Self::SetPlaybackRange { slot_id, start_frac, end_frac } =>
                write!(f, "SetPlaybackRange(slot={slot_id}, {start_frac:.2}–{end_frac:.2})"),
            Self::SetMasterFxChain { chain } => write!(f, "SetMasterFxChain(len={})", chain.len()),
            Self::ClearMasterFx                => write!(f, "ClearMasterFx"),
        }
    }
}

/// Events emitted by the audio engine back to non-RT world.
#[derive(Debug, Clone)]
pub enum AudioEngineEvent {
    /// Audio stream started successfully.
    StreamStarted { sample_rate: u32, buffer_size: u32 },
    /// Audio stream stopped.
    StreamStopped,
    /// An xrun (buffer underrun/overrun) occurred.
    Xrun,
    /// DSP CPU load report (percent, 0-100).
    DspLoad(f32),
    /// An SF2 slot was loaded and is ready.
    Sf2Loaded { slot_id: u32, preset_name: String },
    /// An audio file slot was loaded and is ready.
    AudioFileLoaded { slot_id: u32, duration_secs: f64, sample_rate: u32 },
    /// A slot load failed.
    LoadFailed { slot_id: u32, error: String },
    /// Audio engine error (non-fatal).
    Error(String),
    /// Realtime capture started; WAV is being written to this path.
    CaptureStarted(std::path::PathBuf),
    /// Realtime capture finished; WAV written successfully.
    CaptureStopped { path: std::path::PathBuf, duration_secs: f64 },
    /// Realtime capture failed (e.g., file I/O error).
    CaptureFailed(String),
}
