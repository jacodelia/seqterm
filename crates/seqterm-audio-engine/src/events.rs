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
    /// Send a channel pitch-bend to a synth slot (`value` is the signed 14-bit
    /// bend, -8192..=8191). Used for per-note (MPE) and channel-wide bend.
    PitchBend { slot_id: u32, channel: u8, value: i16 },
    /// Send a channel pressure (aftertouch) `0..=127` to a synth slot. Used for
    /// MPE per-note pressure expression.
    ChannelPressure { slot_id: u32, channel: u8, value: u8 },
    /// Configure polyphonic (MPE) expression on a synth slot: per-channel
    /// pitch-bend / CC74 become per-note expression with the given bend range.
    SetSlotMpe { slot_id: u32, enabled: bool, bend_semitones: f64 },
    /// Request the slot's instrument to serialize its opaque state (e.g. a CLAP
    /// plugin's `state` blob) and send the bytes back on `reply`. Empty vec if
    /// the instrument has no state. Used to persist plugin presets/parameters.
    SaveSlotState { slot_id: u32, reply: flume::Sender<Vec<u8>> },
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
    /// Route a slot's output to a group bus (0 = master mix, 1-8 = group bus).
    SetSlotGroupBus { slot_id: u32, group_bus: u8 },
    /// Set return volume for a bus (0.0 - 2.0).
    SetBusVolume { bus_idx: usize, volume: f32 },
    /// Mute or unmute a bus return.
    SetBusMuted { bus_idx: usize, muted: bool },
    /// Shutdown the audio engine.
    Shutdown,
    /// Install a loaded AudioSource into a mixer slot.
    /// Sent from the non-RT asset-loading thread → RT callback via the ring buffer.
    InstallSource { slot_id: u32, source: Box<dyn AudioSource> },
    /// Update the editable instrument params of a running [`crate::Sf2Sampler`]
    /// in a slot (from the EDITOR). Keeps the sample pool + sounding voices; only
    /// new notes pick up the edited zones. No-op if the slot isn't an Sf2Sampler.
    UpdateSf2Instrument { slot_id: u32, instrument: Box<seqterm_core::Sf2Instrument> },
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
    /// Set stereo pan on an AudioClipPlayer slot (-1.0 = L, 0.0 = C, +1.0 = R).
    SetSlotPan { slot_id: u32, pan: f32 },
    /// Set the per-pad biquad filter on an AudioClipPlayer slot.
    /// `cutoff`/`resonance` are normalised 0–1 (cutoff → 20–20000 Hz, Q → 0.5–10).
    SetSlotFilter { slot_id: u32, kind: seqterm_core::FilterKind, cutoff: f32, resonance: f32 },
    /// Set the per-pad ADSR voice envelope on an AudioClipPlayer slot.
    SetSlotEnvelope { slot_id: u32, env: seqterm_core::AdsrEnvelope },
    /// Set pitch offset in semitones on an AudioClipPlayer slot (vinyl-style: shifts pitch + speed).
    SetPitchSt { slot_id: u32, semitones: f32 },
    /// Set hard trim points on an AudioClipPlayer slot (fractions of total clip length, 0.0–1.0).
    /// Trim constrains the absolute playback range; loop points operate within trim.
    SetPlaybackRange { slot_id: u32, start_frac: f32, end_frac: f32 },
    /// Set a parameter on one FX processor within a slot's chain (for automation).
    SetSlotFxParam { slot_id: u32, fx_idx: usize, param_idx: usize, value: f32 },
    /// Replace the master bus FX chain. Pre-constructed processors, no RT alloc.
    SetMasterFxChain { chain: Vec<Box<dyn FxProcessor>> },
    /// Set a parameter on one FX processor in the master chain (for automation).
    SetMasterFxParam { fx_idx: usize, param_idx: usize, value: f32 },
    /// Clear all FX from the master bus.
    ClearMasterFx,
    /// Send a MIDI program change to a synth slot (changes bank+preset on the given channel).
    ProgramChange { slot_id: u32, channel: u8, program: u8 },
    /// Route live audio input into the output mix at the given gain (0.0=mute, 1.0=unity).
    /// `input_rx` is the Consumer end of the ring buffer written by the input stream thread.
    StartInputMonitor {
        input_rx: rtrb::Consumer<f32>,
        monitor_gain: f32,
    },
    /// Stop mixing live audio input into the output.
    StopInputMonitor,
    /// Begin recording live audio input into a capture buffer.
    /// Pattern mirrors StartCapture: non-RT owns the writer thread; RT writes to the ring.
    StartInputRecord {
        record_tx: rtrb::Producer<f32>,
        done: std::sync::Arc<std::sync::atomic::AtomicBool>,
    },
    /// Signal the RT callback to stop writing input record data.
    StopInputRecord,
    /// Adjust live-input monitor gain without stopping the stream.
    SetInputMonitorGain(f32),
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
            Self::PitchBend    { slot_id, value, .. } => write!(f, "PitchBend(slot={slot_id}, value={value})"),
            Self::ChannelPressure { slot_id, value, .. } => write!(f, "ChannelPressure(slot={slot_id}, value={value})"),
            Self::SetSlotMpe   { slot_id, enabled, .. } => write!(f, "SetSlotMpe(slot={slot_id}, on={enabled})"),
            Self::SaveSlotState{ slot_id, .. } => write!(f, "SaveSlotState(slot={slot_id})"),
            Self::PlayAudioClip{ slot_id }     => write!(f, "PlayAudioClip(slot={slot_id})"),
            Self::StopAudioClip{ slot_id }     => write!(f, "StopAudioClip(slot={slot_id})"),
            Self::SetMasterVolume(v)           => write!(f, "SetMasterVolume({v:.2})"),
            Self::SetSlotVolume{ slot_id, volume } => write!(f, "SetSlotVolume(slot={slot_id}, vol={volume:.2})"),
            Self::SetSlotSends     { slot_id, send_a, send_b } => write!(f, "SetSlotSends(slot={slot_id}, a={send_a:.2}, b={send_b:.2})"),
            Self::SetSlotGroupBus  { slot_id, group_bus }      => write!(f, "SetSlotGroupBus(slot={slot_id}, gb={group_bus})"),
            Self::SetBusVolume { bus_idx, volume } => write!(f, "SetBusVolume(bus={bus_idx}, vol={volume:.2})"),
            Self::SetBusMuted  { bus_idx, muted }  => write!(f, "SetBusMuted(bus={bus_idx}, muted={muted})"),
            Self::Shutdown                     => write!(f, "Shutdown"),
            Self::InstallSource{ slot_id, .. } => write!(f, "InstallSource(slot={slot_id})"),
            Self::UpdateSf2Instrument { slot_id, .. } => write!(f, "UpdateSf2Instrument(slot={slot_id})"),
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
            Self::SetSlotPan { slot_id, pan }       => write!(f, "SetSlotPan(slot={slot_id}, pan={pan:.2})"),
            Self::SetSlotFilter { slot_id, kind, cutoff, resonance } =>
                write!(f, "SetSlotFilter(slot={slot_id}, {}, fc={cutoff:.2}, q={resonance:.2})", kind.label()),
            Self::SetSlotEnvelope { slot_id, env } =>
                write!(f, "SetSlotEnvelope(slot={slot_id}, en={}, a={:.0} d={:.0} s={:.2} r={:.0})",
                    env.enabled, env.attack_ms, env.decay_ms, env.sustain, env.release_ms),
            Self::SetPitchSt { slot_id, semitones } => write!(f, "SetPitchSt(slot={slot_id}, st={semitones:.1})"),
            Self::SetPlaybackRange { slot_id, start_frac, end_frac } =>
                write!(f, "SetPlaybackRange(slot={slot_id}, {start_frac:.2}–{end_frac:.2})"),
            Self::SetSlotFxParam { slot_id, fx_idx, param_idx, value } =>
                write!(f, "SetSlotFxParam(slot={slot_id}, fx={fx_idx}, param={param_idx}, val={value:.3})"),
            Self::SetMasterFxChain { chain } => write!(f, "SetMasterFxChain(len={})", chain.len()),
            Self::SetMasterFxParam { fx_idx, param_idx, value } =>
                write!(f, "SetMasterFxParam(fx={fx_idx}, param={param_idx}, val={value:.3})"),
            Self::ClearMasterFx                => write!(f, "ClearMasterFx"),
            Self::ProgramChange { slot_id, channel, program } =>
                write!(f, "ProgramChange(slot={slot_id}, ch={channel}, prog={program})"),
            Self::StartInputMonitor { monitor_gain, .. } => write!(f, "StartInputMonitor(gain={monitor_gain:.2})"),
            Self::StopInputMonitor  => write!(f, "StopInputMonitor"),
            Self::StartInputRecord { .. } => write!(f, "StartInputRecord"),
            Self::StopInputRecord   => write!(f, "StopInputRecord"),
            Self::SetInputMonitorGain(g) => write!(f, "SetInputMonitorGain({g:.2})"),
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
    /// Live audio input stream started successfully.
    InputStreamStarted { sample_rate: u32 },
    /// Live audio input stream stopped.
    InputStreamStopped,
    /// Input recording finished; WAV written successfully.
    InputRecordStopped { path: std::path::PathBuf, duration_secs: f64 },
    /// Input recording failed.
    InputRecordFailed(String),
    /// Describes an available audio input device.
    InputDevicesListed(Vec<crate::cpal_backend::AudioInputDeviceInfo>),
}
