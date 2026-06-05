use std::{
    collections::HashMap,
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

use seqterm_core::PatternSource;

use parking_lot::Mutex;
use seqterm_core::Project;
use triple_buffer::Input;
use tracing::{debug, warn};

use crate::{
    events::{EngineCommand, EngineEvent},
    transport::TransportState,
};

/// A NoteOff that fires at a specific absolute tick (sub-step precision).
struct PendingNoteOff {
    dest_name: String,
    ch: u8,
    note: u8,
    /// Absolute elapsed-tick count when this NoteOff should fire.
    at_tick: u64,
    /// Clip key for MPE channel release.
    clip_key: Option<String>,
}

/// A NoteOn deferred by micro-shift to fire at a specific absolute tick.
struct PendingNoteOn {
    dest_name: String,
    ch: u8,
    note: u8,
    vel: u8,
    at_tick: u64,
    /// Stored for potential future use (NoteOff is already in pending_note_offs).
    #[allow(dead_code)]
    gate_ticks: u64,
    #[allow(dead_code)]
    clip_key: Option<String>,
    pitch_bend: i16,
    pressure: u8,
    timbre: u8,
    use_mpe: bool,
}

/// The realtime scheduler that drives the sequencer clock.
pub struct Scheduler {
    pub transport: TransportState,
    cmd_rx: flume::Receiver<EngineCommand>,
    event_tx: flume::Sender<EngineEvent>,
    project: Arc<Mutex<Project>>,
    /// Lock-free local snapshot of the project, refreshed opportunistically when
    /// the shared mutex is free. The note-firing path reads from THIS, never the
    /// shared mutex, so the scheduler and the UI render never block each other —
    /// eliminating the timing jitter / dropped steps from lock contention.
    cached_project: Project,
    /// Destination-keyed MIDI senders: midi_out name → raw-byte sender.
    midi_ports: HashMap<String, flume::Sender<Vec<u8>>>,
    /// Lock-free triple-buffer writer for UI transport reads.
    transport_tx: Input<TransportState>,
    /// Monotonically increasing step counter (never resets on loop).
    absolute_step: usize,
    /// NoteOffs scheduled at tick-level precision (sub-step).
    pending_note_offs: Vec<PendingNoteOff>,
    /// NoteOns deferred by micro-shift — fired at their precise sub-step tick.
    pending_note_ons: Vec<PendingNoteOn>,
    /// Maps clip key (row+col) → audio engine slot_id for SF2 / AudioFile sources.
    /// Set by the application layer via EngineCommand::SetAudioSlots.
    audio_slot_map: HashMap<String, u32>,
    /// How many steps ahead to fire audio-engine events to compensate for
    /// the audio buffer output latency.  0 = no compensation (default).
    /// Recomputed by SetAudioLatency: ceil(buffer_latency_ms / tick_ms).
    audio_lookahead_steps: usize,
    /// Senders that receive MIDI clock (0xF8), Start (0xFA), Stop (0xFC).
    clock_ports: Vec<flume::Sender<Vec<u8>>>,
    /// When true, clock messages are sent every tick while playing.
    midi_clock_out: bool,
    /// Per-clip MPE channel allocators (clip_key → MpeChannelMap).
    mpe_maps: HashMap<String, seqterm_core::MpeChannelMap>,
    /// When true, song-mode chain following is active.
    chain_mode: bool,
    /// Current position in `project.chain` (index of the current entry).
    chain_pos: usize,
    /// Bars elapsed in the current chain entry.
    chain_bars: u32,
}

impl Scheduler {
    pub fn new(
        cmd_rx: flume::Receiver<EngineCommand>,
        event_tx: flume::Sender<EngineEvent>,
        project: Arc<Mutex<Project>>,
        transport_tx: Input<TransportState>,
    ) -> Self {
        Self::with_midi_ports(cmd_rx, event_tx, project, HashMap::new(), transport_tx)
    }

    pub fn with_midi_ports(
        cmd_rx: flume::Receiver<EngineCommand>,
        event_tx: flume::Sender<EngineEvent>,
        project: Arc<Mutex<Project>>,
        midi_ports: HashMap<String, flume::Sender<Vec<u8>>>,
        transport_tx: Input<TransportState>,
    ) -> Self {
        let cached_project = project.lock().clone();
        Self {
            transport: TransportState::default(),
            cmd_rx,
            event_tx,
            project,
            cached_project,
            midi_ports,
            transport_tx,
            absolute_step: 0,
            pending_note_offs: Vec::new(),
            pending_note_ons:  Vec::new(),
            audio_slot_map: HashMap::new(),
            audio_lookahead_steps: 0,
            clock_ports: Vec::new(),
            midi_clock_out: false,
            mpe_maps: HashMap::new(),
            chain_mode: false,
            chain_pos: 0,
            chain_bars: 0,
        }
    }

    /// Run the scheduler loop. This should be called on a dedicated thread.
    pub fn run(mut self) {
        let mut last_tick = Instant::now();

        loop {
            // Exit cleanly when all PlaybackEngine handles are dropped.
            if self.cmd_rx.is_disconnected() { break; }

            // Drain all pending commands (non-blocking).
            while let Ok(cmd) = self.cmd_rx.try_recv() {
                self.handle_command(cmd);
            }

            if !self.transport.playing {
                // Sleep briefly to avoid busy-wait when stopped.
                thread::sleep(Duration::from_millis(5));
                last_tick = Instant::now();
                continue;
            }

            let tick_us = self.transport.tick_duration_us();
            let tick_dur = Duration::from_micros(tick_us);

            let elapsed = last_tick.elapsed();
            if elapsed >= tick_dur {
                last_tick = Instant::now();
                if elapsed > tick_dur * 3 {
                    warn!("Scheduler overrun: {}µs late", elapsed.as_micros());
                    let _ = self.event_tx.send(EngineEvent::XRun);
                }
                // MIDI clock: send 0xF8 every ppqn/24 ticks (= 24 pulses per beat).
                if self.midi_clock_out {
                    let clock_div = (self.transport.ppqn / 24).max(1);
                    if self.transport.elapsed_ticks % clock_div as u64 == 0 {
                        let msg = vec![0xF8u8];
                        self.clock_ports.retain(|tx| tx.send(msg.clone()).is_ok());
                    }
                }
                self.process_tick();
            } else {
                // Sleep for the remaining time (with a small guard to avoid overshooting).
                let remaining = tick_dur - elapsed;
                if remaining > Duration::from_micros(100) {
                    thread::sleep(remaining - Duration::from_micros(100));
                }
            }
        }
    }

    fn handle_command(&mut self, cmd: EngineCommand) {
        match cmd {
            EngineCommand::Play => {
                if self.midi_clock_out {
                    let msg = vec![0xFAu8];
                    self.clock_ports.retain(|tx| tx.send(msg.clone()).is_ok());
                }
                self.transport.playing = true;
                self.transport.paused  = false;
                debug!("Transport: PLAY");
            }
            EngineCommand::Pause => {
                // Flush pending NoteOffs so no stuck MIDI notes during pause.
                for noff in self.pending_note_offs.drain(..) {
                    if let Some(tx) = self.midi_ports.get(&noff.dest_name) {
                        let _ = tx.send(vec![0x80 | noff.ch, noff.note, 0]);
                    }
                }
                self.pending_note_ons.clear();
                self.transport.playing = false;
                self.transport.paused  = true;
                // Position (current_step / current_bar) is intentionally preserved.
                debug!("Transport: PAUSE at step {}", self.transport.current_step);
            }
            EngineCommand::Rewind => {
                // Reset position to beginning; keep play/pause state unchanged.
                let was_playing = self.transport.playing;
                let was_paused  = self.transport.paused;
                self.transport.reset();
                self.transport.playing = was_playing;
                self.transport.paused  = was_paused;
                for noff in self.pending_note_offs.drain(..) {
                    if let Some(tx) = self.midi_ports.get(&noff.dest_name) {
                        let _ = tx.send(vec![0x80 | noff.ch, noff.note, 0]);
                    }
                }
                self.pending_note_ons.clear();
                debug!("Transport: REWIND");
            }
            EngineCommand::Stop => {
                for noff in self.pending_note_offs.drain(..) {
                    if let Some(tx) = self.midi_ports.get(&noff.dest_name) {
                        let _ = tx.send(vec![0x80 | noff.ch, noff.note, 0]);
                    }
                }
                self.pending_note_ons.clear();
                if self.midi_clock_out {
                    let msg = vec![0xFCu8];
                    self.clock_ports.retain(|tx| tx.send(msg.clone()).is_ok());
                }
                self.transport.playing = false;
                self.transport.reset();
                debug!("Transport: STOP");
            }
            EngineCommand::Record => {
                self.transport.recording = !self.transport.recording;
                debug!("Transport: RECORD {}", self.transport.recording);
            }
            EngineCommand::SetBpm(bpm) => {
                self.transport.bpm = bpm.clamp(20.0, 300.0);
                let _ = self.event_tx.send(EngineEvent::BpmChanged(self.transport.bpm));
            }
            EngineCommand::SetPattern(key) => {
                self.transport.active_pattern = Some(key);
            }
            EngineCommand::Tick => {
                self.process_tick();
                return; // publish happens inside process_tick
            }
            EngineCommand::PreviewNote(note, vel, dest, ch) => {
                let _ = self.event_tx.send(EngineEvent::NoteOn { note, vel, ch });
                let _ = self.event_tx.send(EngineEvent::NoteOff { note, ch });
                if let Some(ref name) = dest {
                    if let Some(tx) = self.midi_ports.get(name) {
                        let _ = tx.send(vec![0x90 | ch, note, vel]);
                        let _ = tx.send(vec![0x80 | ch, note, 0]);
                    }
                }
            }
            EngineCommand::SetMidiPorts(ports) => {
                self.midi_ports = ports;
            }
            EngineCommand::AddMidiPorts(ports) => {
                self.midi_ports.extend(ports);
            }
            EngineCommand::SetAudioSlots(slots) => {
                self.audio_slot_map = slots;
            }
            EngineCommand::SetClockPorts(ports) => {
                self.clock_ports = ports;
            }
            EngineCommand::SetMidiClockOut(enabled) => {
                self.midi_clock_out = enabled;
                if !enabled {
                    // Send Stop immediately so connected devices don't hang.
                    let msg = vec![0xFCu8];
                    self.clock_ports.retain(|tx| tx.send(msg.clone()).is_ok());
                }
            }
            EngineCommand::SwapProject(new_proj) => {
                self.project = new_proj;
                self.pending_note_offs.clear();
                self.pending_note_ons.clear();
            }
            EngineCommand::SetChainMode(enabled) => {
                self.chain_mode = enabled;
                if enabled {
                    self.chain_pos  = 0;
                    self.chain_bars = 0;
                }
            }
            EngineCommand::SeekChain(pos) => {
                self.chain_pos  = pos;
                self.chain_bars = 0;
            }
            EngineCommand::SetAudioLatency { buffer_size, sample_rate } => {
                if sample_rate > 0 {
                    let ticks_per_beat = self.transport.ppqn as f64;
                    let tick_ms = 60_000.0 / (self.transport.bpm * ticks_per_beat);
                    // Steps = ticks / (ppqn/4), so step_ms = tick_ms * ppqn/4.
                    let ticks_per_step = (self.transport.ppqn / 4).max(1) as f64;
                    let step_ms = tick_ms * ticks_per_step;
                    let buffer_latency_ms =
                        buffer_size as f64 / sample_rate as f64 * 1000.0;
                    // Use round() so lookahead only activates when buffer latency
                    // exceeds half a step duration. ceil() was rounding tiny fractions
                    // (e.g. 5.8ms / 117ms = 0.05) up to 1, causing all notes to fire
                    // one full step early and making notes at step 0 inaudible for the
                    // first full pattern cycle (~7 seconds at 128 BPM / 4-bar pattern).
                    self.audio_lookahead_steps =
                        (buffer_latency_ms / step_ms).round() as usize;
                    debug!(
                        "Audio lookahead: {} step(s) ({:.1}ms buffer / {:.1}ms step)",
                        self.audio_lookahead_steps, buffer_latency_ms, step_ms
                    );
                }
            }
        }
        self.transport_tx.write(self.transport.clone());
    }

    fn process_tick(&mut self) {
        self.transport.elapsed_ticks += 1;
        self.transport.current_tick += 1;

        // Every tick: dispatch pending NoteOns (micro-shifted) and NoteOffs (gate-based).
        self.dispatch_pending_notes();

        // 16th-note grid: fire every ppqn/4 ticks.
        let ticks_per_step = self.transport.ppqn / 4;
        if ticks_per_step == 0 || self.transport.current_tick % ticks_per_step != 0 {
            self.transport_tx.write(self.transport.clone());
            return;
        }

        let step = self.transport.current_step;
        let _ = self.event_tx.send(EngineEvent::StepAdvanced(step));

        // Fire notes for ALL enabled matrix clips. Each pattern loops at its own length
        // (polymeter): position = global_step % pat.length.
        self.fire_all_clips(step);
        self.absolute_step += 1;

        // Advance global step; bar every steps_per_bar (16) steps.
        let bar_advanced = self.transport.advance_step();
        if bar_advanced {
            let _ = self
                .event_tx
                .send(EngineEvent::BarAdvanced(self.transport.current_bar));
            self.process_automation(self.transport.current_bar);
            if self.chain_mode {
                self.advance_chain();
            }
        }

        // Reset tick counter each step to avoid drift.
        self.transport.current_tick = 0;

        // Publish the latest transport state to the triple buffer (lock-free UI read).
        self.transport_tx.write(self.transport.clone());
    }

    /// Advance the pattern chain by one bar. Wraps back to the start when exhausted.
    fn advance_chain(&mut self) {
        // Read from the lock-free snapshot (refreshed in fire_all_clips).
        let chain_len = self.cached_project.chain.len();
        if chain_len == 0 { return; }

        self.chain_bars += 1;
        let entry_bars = self.cached_project.chain
            .get(self.chain_pos).map(|e| e.bars).unwrap_or(1);

        if self.chain_bars >= entry_bars {
            self.chain_bars = 0;
            self.chain_pos = (self.chain_pos + 1) % chain_len;
            let scene_idx = self.cached_project.chain
                .get(self.chain_pos).map(|e| e.scene_idx).unwrap_or(0);
            let _ = self.event_tx.send(EngineEvent::ChainAdvanced {
                chain_pos: self.chain_pos,
                scene_idx,
            });
        }
    }

    /// Interpolate and apply automation lane values for the given bar.
    pub(crate) fn process_automation(&mut self, bar: usize) {
        // Read from the lock-free snapshot (refreshed in fire_all_clips) so this
        // never contends with the UI render.
        let proj = &self.cached_project;

        for lane in &proj.automation {
            if !lane.enabled || lane.points.is_empty() {
                continue;
            }

            // Linear interpolation between the two surrounding automation points.
            let bar_u32 = bar as u32;
            let value: u8 = if bar_u32 <= lane.points[0].0 {
                lane.points[0].1
            } else if bar_u32 >= lane.points.last().unwrap().0 {
                lane.points.last().unwrap().1
            } else {
                let mut lo = lane.points[0];
                let mut hi = lane.points[lane.points.len() - 1];
                for i in 0..lane.points.len().saturating_sub(1) {
                    if lane.points[i].0 <= bar_u32 && lane.points[i + 1].0 >= bar_u32 {
                        lo = lane.points[i];
                        hi = lane.points[i + 1];
                        break;
                    }
                }
                if hi.0 == lo.0 {
                    lo.1
                } else {
                    let t = (bar_u32 - lo.0) as f32 / (hi.0 - lo.0) as f32;
                    (lo.1 as f32 + t * (hi.1 as f32 - lo.1 as f32)).round() as u8
                }
            };

            // Apply to target. Format: "project.bpm" or "bpm" (from SMF import), "channel.N.cc74", etc.
            let target = lane.target.as_str();
            if target == "project.bpm" || target == "bpm" {
                // Map 0-127 → 20-300 BPM linearly.
                let bpm = 20.0 + (value as f64 / 127.0) * 280.0;
                let _ = self.event_tx.send(EngineEvent::BpmChanged(bpm));
                self.transport.bpm = bpm;
                continue;
            }

            // "slot.N.fx.M.param.P" → set FX parameter on an audio engine slot (plugin automation).
            // value 0-127 is remapped to 0.0-1.0.
            if let Some(rest) = target.strip_prefix("slot.") {
                let parts: Vec<&str> = rest.split('.').collect();
                // Expected: ["N", "fx", "M", "param", "P"]
                if parts.len() == 5 && parts[1] == "fx" && parts[3] == "param" {
                    if let (Ok(slot_id), Ok(fx_idx), Ok(param_idx)) = (
                        parts[0].parse::<u32>(),
                        parts[2].parse::<usize>(),
                        parts[4].parse::<usize>(),
                    ) {
                        let float_val = value as f32 / 127.0;
                        let _ = self.event_tx.send(EngineEvent::AudioFxParam {
                            slot_id, fx_idx, param_idx, value: float_val,
                        });
                        continue;
                    }
                }
            }

            // "channel.N.ccXX" → send MIDI CC to the N-th output port (0-indexed).
            if let Some(rest) = target.strip_prefix("channel.") {
                let parts: Vec<&str> = rest.splitn(2, '.').collect();
                if parts.len() == 2 {
                    if let Ok(ch_idx) = parts[0].parse::<usize>() {
                        let cc_str = parts[1];
                        // Resolve MIDI channel output by index into proj.midi_outputs.
                        let port_name = proj.midi_outputs
                            .get(ch_idx)
                            .map(|p| p.name.clone());
                        if let Some(name) = port_name {
                            if let Some(tx) = self.midi_ports.get(&name) {
                                // Parse cc number: "cc74" → 74, "send_a" → CC 91 (send effect level).
                                let cc: u8 = if let Some(n) = cc_str.strip_prefix("cc") {
                                    n.parse().unwrap_or(74)
                                } else if cc_str == "send_a" {
                                    91
                                } else if cc_str == "send_b" {
                                    92
                                } else {
                                    continue;
                                };
                                let _ = tx.send(vec![0xB0, cc, value]);
                            }
                        }
                    }
                }
            }
        }
    }

    /// Dispatch pending NoteOns and NoteOffs whose tick has arrived.
    /// Called every tick for sub-step precision.
    fn dispatch_pending_notes(&mut self) {
        let now = self.transport.elapsed_ticks;

        // Dispatch micro-shifted NoteOns.
        let mut remaining_ons: Vec<PendingNoteOn> = Vec::new();
        for pon in self.pending_note_ons.drain(..) {
            if pon.at_tick > now { remaining_ons.push(pon); continue; }
            if let Some(slot_str) = pon.dest_name.strip_prefix("__audio__") {
                if let Ok(slot_id) = slot_str.parse::<u32>() {
                    let _ = self.event_tx.send(EngineEvent::AudioNoteOn {
                        slot_id, channel: pon.ch, note: pon.note, velocity: pon.vel,
                    });
                }
            } else if let Some(tx) = self.midi_ports.get(&pon.dest_name) {
                if pon.use_mpe || pon.pitch_bend != 0 {
                    let u14 = (pon.pitch_bend + 8192).clamp(0, 16383) as u16;
                    let _ = tx.send(vec![0xE0 | pon.ch, (u14 & 0x7F) as u8, ((u14 >> 7) & 0x7F) as u8]);
                }
                if pon.use_mpe && pon.pressure > 0 {
                    let _ = tx.send(vec![0xD0 | pon.ch, pon.pressure & 0x7F]);
                }
                if pon.use_mpe && pon.timbre != 64 {
                    let _ = tx.send(vec![0xB0 | pon.ch, 74, pon.timbre & 0x7F]);
                }
                let _ = tx.send(vec![0x90 | pon.ch, pon.note, pon.vel]);
            }
        }
        self.pending_note_ons = remaining_ons;

        // Dispatch tick-accurate NoteOffs.
        let mut remaining_offs: Vec<PendingNoteOff> = Vec::new();
        for noff in self.pending_note_offs.drain(..) {
            if noff.at_tick > now { remaining_offs.push(noff); continue; }
            if let Some(slot_str) = noff.dest_name.strip_prefix("__audio__") {
                if let Ok(slot_id) = slot_str.parse::<u32>() {
                    let _ = self.event_tx.send(EngineEvent::AudioNoteOff {
                        slot_id, channel: noff.ch, note: noff.note,
                    });
                }
            } else if let Some(tx) = self.midi_ports.get(&noff.dest_name) {
                let _ = tx.send(vec![0x80 | noff.ch, noff.note, 0]);
                if let Some(ref ck) = noff.clip_key {
                    if let Some(map) = self.mpe_maps.get_mut(ck) {
                        map.release(noff.note);
                    }
                }
            }
        }
        self.pending_note_offs = remaining_offs;
    }

    /// Fire notes for every enabled matrix clip at their independent phase.
    fn fire_all_clips(&mut self, global_step: usize) {
        // Refresh the local snapshot when the shared project mutex is free
        // (`try_lock` never blocks). `clone_from` reuses the snapshot's existing
        // allocations, so the refresh is cheap. The note-firing loop below then
        // reads exclusively from `self.cached_project`, so it NEVER contends with
        // the UI render — no blocking, no jitter, and no dropped steps even when
        // the UI holds the mutex (the previous `try_lock`-and-skip dropped notes
        // at random; a blocking lock added jitter; this does neither).
        if let Some(live) = self.project.try_lock() {
            self.cached_project.clone_from(&live);
        }
        let proj = &self.cached_project;
        for slots in proj.matrix.values() {
            for clip_opt in slots {
                let clip = match clip_opt.as_ref() {
                    Some(c) if c.enabled => c,
                    _ => continue,
                };
                let pat_key = match &clip.pattern_key {
                    Some(k) => k,
                    None => continue,
                };
                let pat = match proj.patterns.get(pat_key) {
                    Some(p) => p,
                    None => continue,
                };
                if pat.length == 0 {
                    continue;
                }
                // For audio-engine sources, pre-schedule by lookahead steps so
                // events arrive at the audio callback on time despite buffer latency.
                let audio_pos = (global_step + self.audio_lookahead_steps) % pat.length;
                let midi_pos  = global_step % pat.length;
                let is_audio_source = matches!(
                    &clip.source,
                    PatternSource::Sf2 { .. }
                        | PatternSource::AudioFile { .. }
                        | PatternSource::Plugin { .. }
                );
                let pos = if is_audio_source { audio_pos } else { midi_pos };
                if let Some(note) = pat.steps.get(pos) {
                    if !note.is_empty() {
                        // Probabilistic gate.
                        if note.prob < 100 {
                            use std::time::{SystemTime, UNIX_EPOCH};
                            let seed = SystemTime::now()
                                .duration_since(UNIX_EPOCH)
                                .unwrap_or_default()
                                .subsec_nanos();
                            if (seed % 100) >= note.prob as u32 {
                                continue;
                            }
                        }
                        // Drum channel: override MIDI channel to 9 (ch 10) and map step→GM note.
                        let drum_channel = proj.channels.iter().find(|c| {
                            c.is_drum && c.midi_port.as_deref() == clip.midi_out.as_deref()
                        });
                        let ch = if drum_channel.is_some() {
                            9u8 // MIDI channel 10 (0-indexed)
                        } else {
                            clip.midi_channel.saturating_sub(1) & 0x0F
                        };

                        // Route to audio engine (SF2 / AudioFile) or MIDI out.
                        let clip_key = format!("{}{}", (b'A' + clip.row as u8) as char, clip.col);
                        let audio_slot = self.audio_slot_map.get(&clip_key).copied();

                        match &clip.source {
                            PatternSource::AudioFile { .. } => {
                                // One-shot trigger per pattern step hit.
                                if let Some(slot_id) = audio_slot {
                                    let _ = self.event_tx.send(EngineEvent::AudioClipTrigger { slot_id });
                                }
                                continue;
                            }
                            PatternSource::Sf2 { .. } | PatternSource::Plugin { .. } => {
                                if let Some(slot_id) = audio_slot {
                                    // Forward CC01 (modulation) and CC74 (filter) when the step
                                    // has explicit values from MIDI import (default from import = 0).
                                    // Compare against Note::default() to skip tracker display defaults.
                                    let default_note = seqterm_core::Note::default();
                                    // Fire CC events when they differ from defaults.
                                    for (cc_num, val, def) in [
                                        (1u8, note.cc01, default_note.cc01),
                                        (11,  note.cc11, default_note.cc11),
                                        (64,  note.cc64, default_note.cc64),
                                        (74,  note.cc74, default_note.cc74),
                                        (93,  note.cc93, default_note.cc93),
                                    ] {
                                        if val != def {
                                            let _ = self.event_tx.send(EngineEvent::AudioControlChange {
                                                slot_id, channel: ch, cc: cc_num, value: val,
                                            });
                                        }
                                    }
                                    // Program change before note-on.
                                    if let Some(prog) = note.program_change {
                                        // Route via AudioCommand ring if available.
                                        // For now: emit as a special AudioControlChange with cc=0xFF (sentinel).
                                        // The audio engine detects this and calls select_preset.
                                        let _ = self.event_tx.send(EngineEvent::AudioControlChange {
                                            slot_id, channel: ch, cc: 0xFE, value: prog,
                                        });
                                    }
                                    let ticks_per_step = (self.transport.ppqn / 4) as u64;
                                    let gate_ticks = ((note.gate as u64) * ticks_per_step / 100).max(1);
                                    let micro_ticks = if note.micro != 0 {
                                        (note.micro as i64 * ticks_per_step as i64 / 100)
                                            .clamp(-(ticks_per_step as i64 / 2), ticks_per_step as i64 - 1)
                                    } else { 0 };
                                    let note_on_tick = (self.transport.elapsed_ticks as i64 + micro_ticks).max(0) as u64;

                                    // For drum channels, play only the mapped GM note; otherwise all chord voices.
                                    let effective_notes: Vec<(u8, u8)> = if let Some(dc) = drum_channel {
                                        vec![(dc.drum_map[pos % 16], note.velocity)]
                                    } else {
                                        note.all_note_ons()
                                    };
                                    for (midi_note, vel) in effective_notes {
                                        let noff_tick = note_on_tick + gate_ticks;
                                        if micro_ticks == 0 {
                                            let _ = self.event_tx.send(EngineEvent::AudioNoteOn {
                                                slot_id, channel: ch, note: midi_note, velocity: vel,
                                            });
                                        } else {
                                            self.pending_note_ons.push(PendingNoteOn {
                                                dest_name: format!("__audio__{slot_id}"),
                                                ch, note: midi_note, vel,
                                                at_tick: note_on_tick,
                                                gate_ticks,
                                                clip_key: None,
                                                pitch_bend: note.pitch_bend,
                                                pressure: note.pressure,
                                                timbre: note.timbre,
                                                use_mpe: false,
                                            });
                                        }
                                        self.pending_note_offs.push(PendingNoteOff {
                                            dest_name: format!("__audio__{slot_id}"),
                                            ch, note: midi_note,
                                            at_tick: noff_tick,
                                            clip_key: None,
                                        });
                                    }
                                }
                                continue;
                            }
                            PatternSource::Midi => {}
                        }

                        // ── MIDI path (default / MPE) ─────────────────────────────
                        let dest_name = clip.midi_out.clone().unwrap_or_default();
                        let midi_tx: Option<flume::Sender<Vec<u8>>> = clip.midi_out.as_deref()
                            .and_then(|dst| self.midi_ports.get(dst))
                            .cloned();
                        let ticks_per_step = (self.transport.ppqn / 4) as u64;
                        let gate_ticks = ((note.gate as u64) * ticks_per_step / 100).max(1);
                        let micro_ticks = if note.micro != 0 {
                            (note.micro as i64 * ticks_per_step as i64 / 100)
                                .clamp(-(ticks_per_step as i64 / 2), ticks_per_step as i64 - 1)
                        } else { 0 };
                        let note_on_tick = (self.transport.elapsed_ticks as i64 + micro_ticks).max(0) as u64;
                        let use_mpe = clip.mpe_zone.is_some();

                        // For drum channels, play only the mapped GM note; otherwise all chord voices.
                        let midi_notes_for_step: Vec<(u8, u8)> = if let Some(dc) = drum_channel {
                            vec![(dc.drum_map[pos % 16], note.velocity)]
                        } else {
                            note.all_note_ons()
                        };
                        for (midi_note, vel) in midi_notes_for_step {
                            let note_ch = if use_mpe {
                                if !self.mpe_maps.contains_key(&clip_key) {
                                    let zone = clip.mpe_zone.clone().unwrap();
                                    self.mpe_maps.insert(clip_key.clone(),
                                        seqterm_core::MpeChannelMap::new(zone));
                                }
                                let map = self.mpe_maps.get_mut(&clip_key).unwrap();
                                map.allocate(midi_note)
                            } else { ch };

                            let _ = self.event_tx.send(EngineEvent::NoteOn { note: midi_note, vel, ch: note_ch });
                            let _ = self.event_tx.send(EngineEvent::NoteOff { note: midi_note, ch: note_ch });

                            if let Some(ref tx) = midi_tx {
                                let noff_tick = note_on_tick + gate_ticks;
                                if micro_ticks == 0 {
                                    // Fire NoteOn immediately.
                                    if use_mpe || note.pitch_bend != 0 {
                                        let u14 = (note.pitch_bend + 8192).clamp(0, 16383) as u16;
                                        let _ = tx.send(vec![0xE0 | note_ch,
                                            (u14 & 0x7F) as u8, ((u14 >> 7) & 0x7F) as u8]);
                                    }
                                    if use_mpe && note.pressure > 0 {
                                        let _ = tx.send(vec![0xD0 | note_ch, note.pressure & 0x7F]);
                                    }
                                    if use_mpe && note.timbre != 64 {
                                        let _ = tx.send(vec![0xB0 | note_ch, 74, note.timbre & 0x7F]);
                                    }
                                    let _ = tx.send(vec![0x90 | note_ch, midi_note, vel]);
                                } else {
                                    // Defer NoteOn to sub-step tick.
                                    self.pending_note_ons.push(PendingNoteOn {
                                        dest_name: dest_name.clone(),
                                        ch: note_ch, note: midi_note, vel,
                                        at_tick: note_on_tick,
                                        gate_ticks,
                                        clip_key: if use_mpe { Some(clip_key.clone()) } else { None },
                                        pitch_bend: note.pitch_bend,
                                        pressure: note.pressure,
                                        timbre: note.timbre,
                                        use_mpe,
                                    });
                                }
                                self.pending_note_offs.push(PendingNoteOff {
                                    dest_name: dest_name.clone(),
                                    ch: note_ch, note: midi_note,
                                    at_tick: noff_tick,
                                    clip_key: if use_mpe { Some(clip_key.clone()) } else { None },
                                });
                            }
                        }

                        // Reset pitch bend after NoteOn (non-MPE legacy path).
                        if !use_mpe && note.pitch_bend != 0 {
                            if let Some(ref tx) = midi_tx {
                                let _ = tx.send(vec![0xE0 | ch, 0x00, 0x40]);
                            }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use seqterm_core::project::{AutomationLane, Project};
    use triple_buffer::TripleBuffer;

    fn make_scheduler(proj: Project) -> (Scheduler, flume::Receiver<EngineEvent>) {
        let (_cmd_tx, cmd_rx) = flume::unbounded();
        let (event_tx, event_rx) = flume::unbounded();
        let buf = TripleBuffer::new(&TransportState::default());
        let (transport_tx, _transport_rx) = buf.split();
        let project = Arc::new(parking_lot::Mutex::new(proj));
        let sched = Scheduler::new(cmd_rx, event_tx, project, transport_tx);
        (sched, event_rx)
    }

    #[test]
    fn bpm_automation_fires_bpm_changed_event() {
        let mut proj = Project::blank("test");
        // BPM lane: bar 0 → 120 BPM (value 45), bar 8 → 180 BPM (value 72).
        // Mapping: BPM = 20 + (val/127)*280  →  val = (BPM-20)/280*127
        let v120 = ((120.0f64 - 20.0) / 280.0 * 127.0).round() as u8; // ≈ 45
        let v180 = ((180.0f64 - 20.0) / 280.0 * 127.0).round() as u8; // ≈ 72
        let mut lane = AutomationLane::new("TEMPO", "bpm");
        lane.points = vec![(0, v120), (8, v180)];
        proj.automation = vec![lane];

        let (mut sched, event_rx) = make_scheduler(proj);

        sched.process_automation(0);

        let events: Vec<EngineEvent> = event_rx.try_iter().collect();
        let bpm_event = events.iter().find(|e| matches!(e, EngineEvent::BpmChanged(_)));
        assert!(bpm_event.is_some(), "expected BpmChanged after process_automation(0)");
        if let Some(EngineEvent::BpmChanged(bpm)) = bpm_event {
            assert!((*bpm - 120.0).abs() < 5.0, "expected ~120 BPM at bar 0, got {bpm:.1}");
        }
    }

    #[test]
    fn bpm_automation_interpolates_between_points() {
        let mut proj = Project::blank("test");
        let v_start = ((60.0f64 - 20.0) / 280.0 * 127.0).round() as u8;
        let v_end   = ((300.0f64 - 20.0) / 280.0 * 127.0).round() as u8; // = 127
        let mut lane = AutomationLane::new("TEMPO", "bpm");
        lane.points = vec![(0, v_start), (8, v_end)];
        proj.automation = vec![lane];

        let (mut sched, event_rx) = make_scheduler(proj);

        // At bar 4 (midpoint) the interpolated BPM should be roughly halfway.
        sched.process_automation(4);

        let events: Vec<EngineEvent> = event_rx.try_iter().collect();
        if let Some(EngineEvent::BpmChanged(bpm)) = events.iter().find(|e| matches!(e, EngineEvent::BpmChanged(_))) {
            assert!(*bpm > 100.0 && *bpm < 240.0,
                "midpoint BPM should be between 100 and 240, got {bpm:.1}");
        } else {
            panic!("expected BpmChanged at bar 4");
        }
    }

    /// Regression test for the "random skipping" playback bug: the scheduler and
    /// the UI render share the project `Mutex`, and `fire_all_clips` used to
    /// `try_lock` and silently drop the entire step on contention. With the UI
    /// rendering ~60 fps, that dropped notes at random. The step must fire even
    /// when the lock is briefly held by another thread (the UI render).
    #[test]
    fn fire_all_clips_does_not_drop_steps_under_lock_contention() {
        use seqterm_core::{Clip, Note, Pattern, PatternSource};

        let mut proj = Project::blank("test");
        let mut pat = Pattern::new("P", 4);
        pat.steps[0] = Note::from_midi(60, 100).unwrap(); // a note on step 0
        proj.patterns.insert("P".to_string(), pat);

        let mut clip = Clip::new("P", 0, 0).with_pattern("P");
        clip.enabled = true;
        clip.midi_channel = 1;
        clip.source = PatternSource::Sf2 {
            path: "x.sf2".into(), bank: 0, preset: 0, preset_name: String::new(),
        };
        proj.matrix.insert("A".to_string(), vec![Some(clip)]);

        let (_cmd_tx, cmd_rx) = flume::unbounded();
        let (event_tx, event_rx) = flume::unbounded();
        let buf = TripleBuffer::new(&TransportState::default());
        let (transport_tx, _rx) = buf.split();
        let project = Arc::new(parking_lot::Mutex::new(proj));
        let mut sched = Scheduler::new(cmd_rx, event_tx, Arc::clone(&project), transport_tx);
        // Map clip A0 → audio slot 5 so it emits AudioNoteOn.
        let mut slots = HashMap::new();
        slots.insert("A0".to_string(), 5u32);
        sched.handle_command(EngineCommand::SetAudioSlots(slots));

        // Another thread grabs the project lock and holds it for 40 ms — the UI
        // render holding the mutex during a frame.
        let p2 = Arc::clone(&project);
        let holder = std::thread::spawn(move || {
            let _g = p2.lock();
            std::thread::sleep(std::time::Duration::from_millis(40));
        });
        std::thread::sleep(std::time::Duration::from_millis(5)); // let it acquire

        // Fire the step while contended. Old code: returns immediately (0 events).
        // Fixed code: blocks ~35 ms, then fires the note.
        sched.fire_all_clips(0);
        holder.join().unwrap();

        let events: Vec<EngineEvent> = event_rx.try_iter().collect();
        assert!(
            events.iter().any(|e| matches!(
                e, EngineEvent::AudioNoteOn { slot_id: 5, note: 60, .. }
            )),
            "step must fire even when the project lock was contended; got {events:?}"
        );
    }
}
