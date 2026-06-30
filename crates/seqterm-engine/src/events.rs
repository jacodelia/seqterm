/// Commands sent from the UI / host to the scheduler thread.
#[derive(Debug, Clone)]
pub enum EngineCommand {
    Play,
    Stop,
    /// Pause: freeze position, silence notes. Resume with Play.
    Pause,
    /// Rewind: reset position to bar 0 step 0 (stays in current play/pause state).
    Rewind,
    Record,
    SetBpm(f64),
    SetPattern(String),
    Tick,
    /// Fire an immediate NoteOn+NoteOff for a note preview (MIDI audition).
    /// Fields: (midi_note, velocity, dest_port_name, midi_channel_0indexed)
    PreviewNote(u8, u8, Option<String>, u8),
    /// Replace the entire per-pattern MIDI port map (used after project load).
    SetMidiPorts(std::collections::HashMap<String, flume::Sender<Vec<u8>>>),
    /// Extend the per-pattern MIDI port map with new entries (used after MIDI import).
    AddMidiPorts(std::collections::HashMap<String, flume::Sender<Vec<u8>>>),
    /// Set the audio engine slot map: clip_key (e.g. "A3") → audio_engine slot_id.
    /// Used for SF2 and AudioFile sources so the scheduler emits AudioNoteOn events.
    SetAudioSlots(std::collections::HashMap<String, u32>),
    /// Set the arrangement audio-clip slot map: clip id → audio_engine slot_id
    /// (Milestone B, Phase B). Lets the scheduler trigger a clip's loaded sample
    /// as SONG playback crosses the clip's start.
    SetArrangementAudioSlots(std::collections::HashMap<u64, u32>),
    /// Notify the scheduler of the audio buffer config so it can pre-schedule
    /// audio events `ceil(buffer_latency_ms / tick_ms)` ticks ahead.
    SetAudioLatency { buffer_size: u32, sample_rate: u32 },
    /// Set the list of raw-byte senders that should receive MIDI clock (0xF8),
    /// Start (0xFA), and Stop (0xFC) messages.
    SetClockPorts(Vec<flume::Sender<Vec<u8>>>),
    /// Enable or disable MIDI clock output on the currently configured clock ports.
    SetMidiClockOut(bool),
    /// Hot-swap the project Arc so the scheduler reads patterns from the new project.
    SwapProject(std::sync::Arc<parking_lot::Mutex<seqterm_core::Project>>),
    /// Enable/disable song-mode pattern chain following.
    SetChainMode(bool),
    /// Override chain position (UI-driven seek within the chain).
    SeekChain(usize),
    /// Enable/disable playback of the rational `Arrangement` timeline (Milestone B).
    /// When on, the scheduler plays each routed arrangement track's active clips
    /// through the matrix-row instrument named by `ArrangementTrack.source_row`.
    SetArrangementPlayback(bool),
}

/// Events emitted by the scheduler thread back to the UI / host.
#[derive(Debug, Clone)]
pub enum EngineEvent {
    StepAdvanced(usize),
    BarAdvanced(usize),
    NoteOn { note: u8, vel: u8, ch: u8 },
    NoteOff { note: u8, ch: u8 },
    BpmChanged(f64),
    XRun,
    /// Incoming MIDI CC from a live input port (forwarded for MIDI Learn and automation).
    MidiCc { ch: u8, cc: u8, val: u8 },
    /// A note event that must be routed to the audio engine (SF2 slot) instead of MIDI out.
    /// `slot_id` corresponds to the AudioEngine slot assigned to this clip's source.
    AudioNoteOn  { slot_id: u32, channel: u8, note: u8, velocity: u8 },
    AudioNoteOff { slot_id: u32, channel: u8, note: u8 },
    /// CC event for an SF2 slot — fired before the note-on when the step has explicit CC data.
    AudioControlChange { slot_id: u32, channel: u8, cc: u8, value: u8 },
    /// Channel pitch-bend for an audio slot (`value` = signed 14-bit, -8192..=8191).
    /// Used by the internal-MPE path to deliver per-note bend to plugin instruments.
    AudioPitchBend { slot_id: u32, channel: u8, value: i16 },
    /// Channel pressure (aftertouch, `0..=127`) for an audio slot. Internal-MPE
    /// path → per-note pressure expression on plugin instruments.
    AudioChannelPressure { slot_id: u32, channel: u8, value: u8 },
    /// Trigger an audio clip (PatternSource::AudioFile) at the given slot.
    AudioClipTrigger { slot_id: u32 },
    /// The pattern chain advanced to a new scene (song-mode).
    ChainAdvanced { chain_pos: usize, scene_idx: usize },
    /// Set a parameter on an FX processor in an audio slot (automation target).
    AudioFxParam { slot_id: u32, fx_idx: usize, param_idx: usize, value: f32 },
}
