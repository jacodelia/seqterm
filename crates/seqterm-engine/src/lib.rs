pub mod events;
pub mod scheduler;
pub mod transport;

use std::{collections::HashMap, sync::Arc};

use parking_lot::Mutex;
use seqterm_core::Project;
use triple_buffer::Output;

pub use events::{EngineCommand, EngineEvent};
pub use scheduler::Scheduler;
pub use transport::TransportState;

/// High-level handle to the playback engine, used by the UI layer.
pub struct PlaybackEngine {
    pub cmd_tx: flume::Sender<EngineCommand>,
    pub event_rx: flume::Receiver<EngineEvent>,
    /// Lock-free reader for the scheduler's latest transport state.
    transport_rx: Output<TransportState>,
}

impl PlaybackEngine {
    /// Create a new engine with no MIDI output wired.
    pub fn start(project: Arc<Mutex<Project>>) -> Self {
        Self::start_with_midi(project, HashMap::new())
    }

    /// Create engine with per-pattern MIDI virtual ports already created.
    /// `midi_ports` maps pattern key → raw-byte sender to that pattern's ALSA port.
    pub fn start_with_midi(
        project: Arc<Mutex<Project>>,
        midi_ports: HashMap<String, flume::Sender<Vec<u8>>>,
    ) -> Self {
        let (cmd_tx, cmd_rx) = flume::unbounded();
        let (event_tx, event_rx) = flume::unbounded();
        let (transport_tx, transport_rx) = triple_buffer::triple_buffer(&TransportState::default());

        let scheduler = Scheduler::with_midi_ports(cmd_rx, event_tx, project, midi_ports, transport_tx);
        std::thread::Builder::new()
            .name("seqterm-scheduler".to_string())
            .spawn(move || scheduler.run())
            .expect("failed to spawn scheduler thread");

        Self { cmd_tx, event_rx, transport_rx }
    }

    /// Read the latest transport state published by the scheduler (lock-free).
    pub fn transport_snapshot(&mut self) -> &TransportState {
        self.transport_rx.read()
    }

    pub fn play(&self) {
        let _ = self.cmd_tx.send(EngineCommand::Play);
    }

    pub fn stop(&self) {
        let _ = self.cmd_tx.send(EngineCommand::Stop);
    }

    /// Pause: freeze transport position; resume with play().
    pub fn pause(&self) {
        let _ = self.cmd_tx.send(EngineCommand::Pause);
    }

    /// Rewind: jump to bar 0 step 0 without changing play/pause state.
    pub fn rewind(&self) {
        let _ = self.cmd_tx.send(EngineCommand::Rewind);
    }

    pub fn toggle_record(&self) {
        let _ = self.cmd_tx.send(EngineCommand::Record);
    }

    pub fn set_bpm(&self, bpm: f64) {
        let _ = self.cmd_tx.send(EngineCommand::SetBpm(bpm));
    }

    pub fn set_pattern(&self, key: String) {
        let _ = self.cmd_tx.send(EngineCommand::SetPattern(key));
    }

    /// Fire an immediate MIDI note preview (NoteOn + NoteOff sent to the engine).
    /// `dest` is the midi_ports key (clip's midi_out), `ch` is the 0-indexed MIDI channel.
    pub fn preview_note(&self, midi: u8, vel: u8, dest: Option<String>, ch: u8) {
        let _ = self.cmd_tx.send(EngineCommand::PreviewNote(midi, vel, dest, ch));
    }

    /// Replace all per-pattern virtual MIDI port senders (used after project load).
    /// Old senders are dropped, closing the previous ports.
    pub fn set_midi_ports(&self, ports: HashMap<String, flume::Sender<Vec<u8>>>) {
        let _ = self.cmd_tx.send(EngineCommand::SetMidiPorts(ports));
    }

    /// Extend the per-pattern MIDI port map with additional entries (used after MIDI import).
    pub fn add_midi_ports(&self, ports: HashMap<String, flume::Sender<Vec<u8>>>) {
        let _ = self.cmd_tx.send(EngineCommand::AddMidiPorts(ports));
    }

    /// Set the audio engine slot map (clip_key → slot_id) so the scheduler
    /// routes SF2 / AudioFile clips to the audio engine instead of MIDI out.
    pub fn set_audio_slots(&self, slots: HashMap<String, u32>) {
        let _ = self.cmd_tx.send(EngineCommand::SetAudioSlots(slots));
    }

    /// Inform the scheduler of the audio buffer config so it can compute
    /// how many steps to pre-schedule audio events (latency compensation).
    pub fn set_audio_latency(&self, buffer_size: u32, sample_rate: u32) {
        let _ = self.cmd_tx.send(EngineCommand::SetAudioLatency { buffer_size, sample_rate });
    }

    /// Enable or disable MIDI clock output (0xF8 per PPQN tick, 0xFA/0xFC on play/stop).
    pub fn set_midi_clock_out(&self, enabled: bool) {
        let _ = self.cmd_tx.send(EngineCommand::SetMidiClockOut(enabled));
    }

    /// Set the raw-byte senders that receive MIDI clock messages.
    pub fn set_clock_ports(&self, ports: Vec<flume::Sender<Vec<u8>>>) {
        let _ = self.cmd_tx.send(EngineCommand::SetClockPorts(ports));
    }

    /// Hot-swap the project the scheduler reads patterns from (used for tab switching).
    pub fn set_project(&self, project: Arc<Mutex<Project>>) {
        let _ = self.cmd_tx.send(EngineCommand::SwapProject(project));
    }

    /// Enable or disable song-mode pattern chain following.
    pub fn set_chain_mode(&self, enabled: bool) {
        let _ = self.cmd_tx.send(EngineCommand::SetChainMode(enabled));
    }

    /// Seek to a specific chain position (0-based entry index).
    pub fn seek_chain(&self, pos: usize) {
        let _ = self.cmd_tx.send(EngineCommand::SeekChain(pos));
    }

    /// Drain all pending events, returning them.
    pub fn drain_events(&self) -> Vec<EngineEvent> {
        let mut out = Vec::new();
        while let Ok(ev) = self.event_rx.try_recv() {
            out.push(ev);
        }
        out
    }
}
