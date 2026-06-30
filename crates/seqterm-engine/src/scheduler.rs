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

/// Convert a rational duration/offset in **beats** to whole transport ticks
/// (`ppqn` ticks per beat), flooring. Used to place rational note events on the
/// tick clock; for a `1/16` grid event this is exact (a multiple of `ppqn/4`).
fn ev_offset_ticks(beats: seqterm_core::RationalTime, ppqn: i64) -> i64 {
    (beats * ppqn).floor()
}

/// Resolve the instrument an arrangement track routes through (Milestone B):
/// the first configured clip in matrix `row` supplies the source / MIDI out /
/// channel, and `audio_slot_map` supplies that cell's audio slot (if any). The
/// arrangement plays its *own* clip's pattern — only the *instrument* comes from
/// the matrix row. Returns `None` if the row has no configured clip.
fn resolve_row_instrument(
    proj: &Project,
    audio_slot_map: &HashMap<String, u32>,
    row: &str,
) -> Option<(seqterm_core::PatternSource, Option<String>, u8, Option<u32>)> {
    let slots = proj.matrix.get(row)?;
    for clip in slots.iter().flatten() {
        let clip_key = format!("{}{}", (b'A' + clip.row as u8) as char, clip.col);
        return Some((
            clip.source.clone(),
            clip.midi_out.clone(),
            clip.midi_channel,
            audio_slot_map.get(&clip_key).copied(),
        ));
    }
    None
}

/// Map an arrangement automation destination id to a MIDI CC number. Returns
/// `None` for destinations that aren't expressible as a channel CC. (Milestone F.)
fn cc_for_destination(dest: &str) -> Option<u8> {
    match dest {
        "volume" => Some(7),
        "pan" => Some(10),
        "cutoff" => Some(74),
        "resonance" => Some(71),
        "send_a" | "reverb" => Some(91),
        "send_b" | "chorus" => Some(93),
        other => other.strip_prefix("cc").and_then(|n| n.parse::<u8>().ok()),
    }
}

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
    /// Maps arrangement audio clip id → audio engine slot_id (Milestone B,
    /// Phase B). Set by the application via EngineCommand::SetArrangementAudioSlots.
    /// The scheduler edge-triggers the slot as the playhead crosses the clip start.
    arrangement_audio_slots: HashMap<u64, u32>,
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
    /// When true, the rational `Arrangement` timeline is played alongside the
    /// matrix (Milestone B). Routed via `ArrangementTrack.source_row`.
    arrangement_playback: bool,
    /// Last automation value emitted per `(track_idx, destination)`, so the
    /// arrangement automation driver only sends a CC when the value actually
    /// changes (avoids flooding the port/audio engine every tick). (Milestone F.)
    last_automation_values: HashMap<(usize, String), u8>,
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
            arrangement_audio_slots: HashMap::new(),
            audio_lookahead_steps: 0,
            clock_ports: Vec::new(),
            midi_clock_out: false,
            mpe_maps: HashMap::new(),
            chain_mode: false,
            chain_pos: 0,
            chain_bars: 0,
            arrangement_playback: false,
            last_automation_values: HashMap::new(),
        }
    }

    /// Run the scheduler loop. This should be called on a dedicated thread.
    ///
    /// Timing uses a **phase accumulator**: the next tick's target time advances by
    /// exactly one tick duration (`next_tick += tick_dur`) rather than resetting to
    /// "now" on each fire. Resetting to now discards the overshoot every tick, so
    /// ticks land progressively late and jittery and the tempo never stays square.
    /// Advancing the target keeps the average period exact (locked to BPM) and lets
    /// the loop catch up cleanly after an OS scheduling hiccup.
    pub fn run(mut self) {
        let mut next_tick = Instant::now();

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
                // Re-anchor the clock so resuming doesn't fire a backlog burst.
                next_tick = Instant::now();
                continue;
            }

            // Nanosecond precision + recomputed each iteration so tempo changes take
            // effect immediately and fractional-tick error doesn't accumulate.
            let tick_dur = Duration::from_nanos(self.transport.tick_duration_ns());
            let now = Instant::now();

            if now >= next_tick {
                // Spiral-of-death guard: if we fell badly behind (lock stall / OS
                // preemption) snap the clock to now and drop the backlog instead of
                // firing a burst. Otherwise advance by exactly one tick so timing
                // stays phase-locked to the ideal grid.
                let behind = now - next_tick;
                if behind > tick_dur * 4 {
                    warn!("Scheduler overrun: {}µs late", behind.as_micros());
                    let _ = self.event_tx.send(EngineEvent::XRun);
                    next_tick = now + tick_dur;
                } else {
                    next_tick += tick_dur;
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
                // Sleep most of the remaining time; a small guard avoids overshooting
                // (OS sleep granularity ~1ms), the final approach is a tight re-check.
                let remaining = next_tick - now;
                if remaining > Duration::from_micros(200) {
                    thread::sleep(remaining - Duration::from_micros(200));
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
            EngineCommand::SetArrangementAudioSlots(slots) => {
                self.arrangement_audio_slots = slots;
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
            EngineCommand::SetArrangementPlayback(enabled) => {
                self.arrangement_playback = enabled;
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
        if self.arrangement_playback {
            // Cycle (loop) playback: wrap the arrangement clock back to the cycle
            // start once it reaches the end. Only `absolute_step` is wrapped — the
            // matrix transport (`current_step`) is untouched, so the two stay
            // independent. (Phase 5, Fase 8.)
            self.maybe_loop_arrangement();
            self.fire_arrangement_clips(self.absolute_step);
            self.process_arrangement_automation(self.absolute_step);
        }
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

    /// Play the rational `Arrangement` timeline (Milestone B). For each routed
    /// track's active clip at the current timeline beat, resolve the matrix-row
    /// instrument (`source_row`) and emit the clip's pattern events that fall in
    /// this master-step window.
    ///
    /// Compact emit: note-ons through the row's audio slot (internal synth / SF2 /
    /// sampler) and/or its external MIDI out; off-grid (tuplet / sub-step) hits go
    /// through the same `__audio__{slot}` pending queue the matrix path uses.
    /// MPE, drum-map and per-step CC parity with the matrix path are a follow-up;
    /// this delivers "place clip → press play → hear it".
    fn fire_arrangement_clips(&mut self, master_step: usize) {
        use seqterm_core::{PatternSource, RationalTime};
        if !self.arrangement_playback {
            return;
        }
        // `cached_project` is refreshed by `fire_all_clips` earlier this tick.
        let ppqn = self.transport.ppqn as i64;
        let step_w = RationalTime::new(1, 4); // master 1/16 = 1/4 beat
        let now_beat = step_w * master_step as i64;

        // One note to emit, resolved under the immutable project borrow.
        struct Emit {
            slot: Option<u32>,
            midi_out: Option<String>,
            ch: u8,
            note: u8,
            vel: u8,
            offset_ticks: u64,
            gate_ticks: u64,
            audio_oneshot: bool,
        }
        let mut emits: Vec<Emit> = Vec::new();
        {
            let proj = &self.cached_project;
            for hit in proj.arrangement.playback_hits(now_beat) {
                let Some(pat) = proj.patterns.get(&hit.pattern_key) else { continue };
                if pat.length == 0 {
                    continue;
                }
                // Resolve the routed row's instrument from its first matrix clip.
                let Some((source, midi_out, midi_channel, slot)) =
                    resolve_row_instrument(proj, &self.audio_slot_map, &hit.source_row)
                else {
                    continue;
                };
                let ch = midi_channel.saturating_sub(1) & 0x0F;
                let audio_oneshot = matches!(source, PatternSource::AudioFile { .. });
                let events = pat.to_events();
                let win = seqterm_core::hits_in_window(
                    &events,
                    pat.length_beats(),
                    pat.step_beats(),
                    hit.local_beat,
                    step_w,
                );
                for wh in &win {
                    let ev = &events[wh.event_index];
                    let note = &ev.note;
                    let offset_ticks = ev_offset_ticks(wh.offset, ppqn).max(0) as u64;
                    let gate_ticks = ev_offset_ticks(ev.duration, ppqn).max(1) as u64;
                    for (midi_note, vel) in note.all_note_ons() {
                        emits.push(Emit {
                            slot,
                            midi_out: midi_out.clone(),
                            ch,
                            note: midi_note,
                            vel,
                            offset_ticks,
                            gate_ticks,
                            audio_oneshot,
                        });
                    }
                }
            }
        }

        let now_tick = self.transport.elapsed_ticks;
        for e in emits {
            // Audio-slot path (internal synth / SF2 / sampler) or one-shot sample.
            if let Some(slot_id) = e.slot {
                if e.audio_oneshot {
                    let _ = self.event_tx.send(EngineEvent::AudioClipTrigger { slot_id });
                } else if e.offset_ticks == 0 {
                    let _ = self.event_tx.send(EngineEvent::AudioNoteOn {
                        slot_id, channel: e.ch, note: e.note, velocity: e.vel,
                    });
                    self.pending_note_offs.push(PendingNoteOff {
                        dest_name: format!("__audio__{slot_id}"),
                        ch: e.ch, note: e.note,
                        at_tick: now_tick + e.gate_ticks,
                        clip_key: None,
                    });
                } else {
                    // Off-grid hit: defer the on (and its off) to the exact tick.
                    self.pending_note_ons.push(PendingNoteOn {
                        dest_name: format!("__audio__{slot_id}"),
                        ch: e.ch, note: e.note, vel: e.vel,
                        at_tick: now_tick + e.offset_ticks,
                        gate_ticks: e.gate_ticks,
                        clip_key: None,
                        pitch_bend: 0, pressure: 0, timbre: 64, use_mpe: false,
                    });
                    self.pending_note_offs.push(PendingNoteOff {
                        dest_name: format!("__audio__{slot_id}"),
                        ch: e.ch, note: e.note,
                        at_tick: now_tick + e.offset_ticks + e.gate_ticks,
                        clip_key: None,
                    });
                }
            }
            // External MIDI out, if the row is routed to a port.
            if let Some(tx) = e.midi_out.as_ref().and_then(|d| self.midi_ports.get(d)) {
                let _ = tx.send(vec![0x90 | e.ch, e.note, e.vel]);
                let _ = tx.send(vec![0x80 | e.ch, e.note, 0]);
            }
        }

        // Audio clips: edge-trigger the loaded sample once as the playhead crosses
        // the clip start. Each 1/16 step window `[now_beat, now_beat + 1/4)` is
        // visited exactly once, so a clip whose start lands in it fires once.
        // ponytail: triggered at step granularity from the sample head; sub-step
        // offset, content_offset trim, length-trim and per-clip gain are deferred
        // (need an extended PlayAudioClip + RT-mixer params — Phase F territory).
        let starts = self
            .cached_project
            .arrangement
            .audio_clip_starts_in(now_beat, now_beat + step_w);
        for (clip_id, _gain) in starts {
            if let Some(&slot_id) = self.arrangement_audio_slots.get(&clip_id) {
                let _ = self.event_tx.send(EngineEvent::AudioClipTrigger { slot_id });
            }
        }
    }

    /// If a cycle (loop) span is set, wrap the arrangement clock to the cycle
    /// start once it reaches the end. The cycle is in beats; the master clock is
    /// 1/16 steps (4 steps/beat). When the value changes it is force-published so
    /// pending automation re-evaluates from the loop start. (Phase 5, Fase 8.)
    fn maybe_loop_arrangement(&mut self) {
        let Some((s, e)) = self.cached_project.arrangement.cycle else { return };
        const STEPS_PER_BEAT: f64 = 4.0;
        let start_step = (s.to_f64() * STEPS_PER_BEAT).round() as usize;
        let end_step = (e.to_f64() * STEPS_PER_BEAT).round() as usize;
        if end_step > start_step && self.absolute_step >= end_step {
            self.absolute_step = start_step;
            // Force automation to re-send at the loop start (values likely differ
            // from where the playhead just was).
            self.last_automation_values.clear();
        }
    }

    /// Evaluate each arrangement track's automation lanes at the current beat and
    /// apply changed values as CCs to the track's routed instrument (Milestone F).
    /// Destinations map to standard CC numbers (`volume`→7, `pan`→10, `cutoff`→74,
    /// `send_a`/`reverb`→91, `send_b`→92, or `ccNN` literal). A CC is sent only
    /// when the quantised `0..=127` value changes, so this is cheap per tick.
    fn process_arrangement_automation(&mut self, master_step: usize) {
        use seqterm_core::RationalTime;
        let step_w = RationalTime::new(1, 4); // master 1/16 = 1/4 beat
        let now_beat = (step_w * master_step as i64).to_f64();

        // Resolve (track_idx, cc, value, slot, midi_out, ch) under the borrow.
        struct CcApply {
            key: (usize, String),
            cc: u8,
            val: u8,
            slot: Option<u32>,
            midi_out: Option<String>,
            ch: u8,
        }
        let mut applies: Vec<CcApply> = Vec::new();
        {
            let proj = &self.cached_project;
            for (ti, track) in proj.arrangement.tracks.iter().enumerate() {
                if track.mute || track.automation.is_empty() {
                    continue;
                }
                let Some(row) = track.source_row.as_deref() else { continue };
                let Some((_, midi_out, midi_channel, slot)) =
                    resolve_row_instrument(proj, &self.audio_slot_map, row)
                else {
                    continue;
                };
                let ch = midi_channel.saturating_sub(1) & 0x0F;
                for lane in &track.automation {
                    let Some(cc) = cc_for_destination(&lane.destination) else { continue };
                    let Some(v) = lane.value_at(now_beat) else { continue };
                    let val = (v.clamp(0.0, 1.0) * 127.0).round() as u8;
                    applies.push(CcApply {
                        key: (ti, lane.destination.clone()),
                        cc, val, slot,
                        midi_out: midi_out.clone(),
                        ch,
                    });
                }
            }
        }

        for a in applies {
            // Skip if the quantised value is unchanged since the last tick.
            if self.last_automation_values.get(&a.key) == Some(&a.val) {
                continue;
            }
            self.last_automation_values.insert(a.key, a.val);
            if let Some(slot_id) = a.slot {
                let _ = self.event_tx.send(EngineEvent::AudioControlChange {
                    slot_id, channel: a.ch, cc: a.cc, value: a.val,
                });
            }
            if let Some(tx) = a.midi_out.as_ref().and_then(|d| self.midi_ports.get(d)) {
                let _ = tx.send(vec![0xB0 | a.ch, a.cc, a.val]);
            }
        }
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
                // Rational-time playback: scan the beat window covered by this
                // master 1/16 step (1/4 beat wide) of the pattern's looping
                // timeline. For a 1/16 pattern this yields exactly the legacy
                // `global_step % length` step at offset 0 (bit-identical timing);
                // odd resolutions / tuplets place multiple sub-step hits at exact
                // offsets, handled by the existing pending-note tick machinery.
                let is_audio_source = matches!(
                    &clip.source,
                    PatternSource::Sf2 { .. }
                        | PatternSource::AudioFile { .. }
                        | PatternSource::Plugin { .. }
                );
                // Audio sources pre-schedule by lookahead steps to hide buffer latency.
                let base_step = if is_audio_source {
                    global_step + self.audio_lookahead_steps
                } else {
                    global_step
                };
                let ppqn = self.transport.ppqn as i64;
                let master_step = seqterm_core::RationalTime::new(1, 4); // 1/16 note
                let w0 = master_step * base_step as i64;
                // Loop counter salts the probability engine so unlocked
                // generation re-rolls each pass (prob_lock ignores it).
                let loop_salt = (base_step as u64) / (pat.length.max(1) as u64);
                let events = pat.generated_events(loop_salt);
                let hits = seqterm_core::hits_in_window(
                    &events,
                    pat.length_beats(),
                    pat.step_beats(),
                    w0,
                    master_step,
                );
                // Euclidean rhythm mask: when enabled, only steps that fall on a
                // pulse of the euclid(fill, len) pattern may trigger.
                let euclid_mask: Option<Vec<bool>> = if pat.euclid_enabled {
                    Some(seqterm_generative::euclidean_rhythm(
                        pat.euclid_fill.max(1),
                        pat.euclid_len.max(2),
                    ))
                } else {
                    None
                };
                for hit in &hits {
                    let ev = &events[hit.event_index];
                    let note = &ev.note;
                    let pos = hit.local_step;
                    // Sub-step placement (beats → ticks) within this master step.
                    // Pattern-level groove: swing (odd steps) + global microshift +
                    // optional humanization jitter, all in ticks.
                    let step_ticks = (ppqn / 4).max(1);
                    let mut off_i = ev_offset_ticks(hit.offset, ppqn);
                    off_i += pat.swing_offset(pos, self.transport.ppqn) as i64;
                    off_i += pat.microshift as i64;
                    if pat.humanization > 0 {
                        // Deterministic per (global_step, pos) jitter in
                        // [-range, +range] ticks, range scaled by humanization%.
                        let range = (step_ticks * pat.humanization as i64) / 200;
                        if range > 0 {
                            let h = (base_step as u64)
                                .wrapping_mul(2654435761)
                                .wrapping_add(pos as u64)
                                .wrapping_mul(40503);
                            let j = (h % (2 * range as u64 + 1)) as i64 - range;
                            off_i += j;
                        }
                    }
                    let offset_ticks = off_i.max(0) as u64;
                    let gate_ticks = ev_offset_ticks(ev.duration, ppqn).max(1) as u64;
                    {
                        // Euclidean gate.
                        if let Some(mask) = &euclid_mask {
                            if !mask.get(pos % mask.len().max(1)).copied().unwrap_or(true) {
                                continue;
                            }
                        }
                        // Per-note probability gate (default 100 = always). The
                        // pattern-level `pat.prob` engine *generates* notes
                        // (see `Pattern::generated_events`) rather than gating,
                        // so it is applied upstream, not here.
                        if note.prob < 100 {
                            let seed = if pat.prob_lock {
                                (pos as u64).wrapping_mul(2246822519).wrapping_add(7)
                            } else {
                                use std::time::{SystemTime, UNIX_EPOCH};
                                SystemTime::now()
                                    .duration_since(UNIX_EPOCH)
                                    .unwrap_or_default()
                                    .subsec_nanos() as u64
                            };
                            if (seed % 100) >= note.prob as u64 {
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
                                    // Timing comes from the rational event: sub-step
                                    // `offset_ticks` and `gate_ticks` were derived above.
                                    let note_on_tick = self.transport.elapsed_ticks + offset_ticks;

                                    // For drum channels, play only the mapped GM note; otherwise all chord voices.
                                    let effective_notes: Vec<(u8, u8)> = if let Some(dc) = drum_channel {
                                        vec![(dc.drum_map[pos % 16], note.velocity)]
                                    } else {
                                        note.all_note_ons()
                                    };
                                    // For MPE clips, deliver this step's expression
                                    // (per-note pitch-bend + timbre) to the slot so
                                    // plugin instruments with note expression respond.
                                    // Bend is always sent (incl. 0) so non-bent notes
                                    // reset the channel.
                                    let mpe_expr = clip.mpe_zone.is_some();
                                    for (midi_note, vel) in effective_notes {
                                        let noff_tick = note_on_tick + gate_ticks;
                                        if offset_ticks == 0 {
                                            if mpe_expr {
                                                let _ = self.event_tx.send(EngineEvent::AudioPitchBend {
                                                    slot_id, channel: ch, value: note.pitch_bend,
                                                });
                                                if note.timbre != 64 {
                                                    let _ = self.event_tx.send(EngineEvent::AudioControlChange {
                                                        slot_id, channel: ch, cc: 74, value: note.timbre,
                                                    });
                                                }
                                                if note.pressure > 0 {
                                                    let _ = self.event_tx.send(EngineEvent::AudioChannelPressure {
                                                        slot_id, channel: ch, value: note.pressure,
                                                    });
                                                }
                                            }
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
                            PatternSource::Midi => {
                                // Internal synth: if this MIDI clip has an allocated
                                // audio slot (BuiltinSynth), route notes there so it
                                // sounds and passes through the mixer / audio export.
                                // External MIDI-out, if configured, still fires below.
                                if let Some(slot_id) = audio_slot {
                                    let note_on_tick = self.transport.elapsed_ticks + offset_ticks;
                                    let synth_notes: Vec<(u8, u8)> = if let Some(dc) = drum_channel {
                                        vec![(dc.drum_map[pos % 16], note.velocity)]
                                    } else {
                                        note.all_note_ons()
                                    };
                                    for (midi_note, vel) in synth_notes {
                                        if offset_ticks == 0 {
                                            let _ = self.event_tx.send(EngineEvent::AudioNoteOn {
                                                slot_id, channel: ch, note: midi_note, velocity: vel,
                                            });
                                        } else {
                                            // Off-grid (tuplet/free-time) hit: defer to its tick.
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
                                            at_tick: note_on_tick + gate_ticks,
                                            clip_key: None,
                                        });
                                    }
                                }
                            }
                        }

                        // ── MIDI path (default / MPE) ─────────────────────────────
                        let dest_name = clip.midi_out.clone().unwrap_or_default();
                        let midi_tx: Option<flume::Sender<Vec<u8>>> = clip.midi_out.as_deref()
                            .and_then(|dst| self.midi_ports.get(dst))
                            .cloned();
                        // Timing from the rational event (sub-step offset + gate).
                        let note_on_tick = self.transport.elapsed_ticks + offset_ticks;
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
                                if offset_ticks == 0 {
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

    /// Build a scheduler with one SF2 clip (slot 5) playing pattern "P".
    fn sched_with_pattern(pat: seqterm_core::Pattern) -> (Scheduler, flume::Receiver<EngineEvent>) {
        use seqterm_core::{Clip, PatternSource};
        let mut proj = Project::blank("test");
        proj.patterns.insert("P".to_string(), pat);
        let mut clip = Clip::new("P", 0, 0).with_pattern("P");
        clip.enabled = true;
        clip.source = PatternSource::Sf2 {
            path: "x.sf2".into(), bank: 0, preset: 0, preset_name: String::new(),
        };
        proj.matrix.insert("A".to_string(), vec![Some(clip)]);
        let (mut sched, rx) = make_scheduler(proj);
        let mut slots = HashMap::new();
        slots.insert("A0".to_string(), 5u32);
        sched.handle_command(EngineCommand::SetAudioSlots(slots));
        (sched, rx)
    }

    /// Rational-time parity: a 1/16 pattern fires exactly the legacy step each
    /// master step, immediately (offset 0), with identical note selection.
    #[test]
    fn rational_scheduler_parity_on_sixteenth_grid() {
        use seqterm_core::{Note, Pattern};
        let mut pat = Pattern::new("P", 4);
        pat.set_step(0, Note::from_midi(60, 100).unwrap());
        pat.set_step(2, Note::from_midi(64, 100).unwrap());
        let (mut sched, rx) = sched_with_pattern(pat);

        // Fire master steps 0..8 (two loops of the 4-step pattern).
        for gs in 0..8usize {
            sched.transport.current_step = gs;
            sched.fire_all_clips(gs);
        }
        let ons: Vec<u8> = rx.try_iter().filter_map(|e| match e {
            EngineEvent::AudioNoteOn { slot_id: 5, note, .. } => Some(note),
            _ => None,
        }).collect();
        // Steps 0 and 2 fire each loop: notes 60,64,60,64.
        assert_eq!(ons, vec![60, 64, 60, 64]);
    }

    /// A 1/12 (triplet) pattern fires its off-grid steps via the pending-note
    /// queue at the correct sub-step tick, not on the master 1/16 boundary.
    #[test]
    fn rational_scheduler_triplet_defers_offbeat_hits() {
        use seqterm_core::{Note, Pattern, Resolution};
        let mut pat = Pattern::new("P", 12);
        pat.resolution = Resolution::Whole(12); // 1/12 grid, 3 steps per beat
        pat.set_step(1, Note::from_midi(60, 100).unwrap()); // at 1/3 beat — off-grid
        let (mut sched, rx) = sched_with_pattern(pat);

        // Master step 1 covers beats [1/4, 1/2); the 1/3-beat triplet hit lands here.
        sched.transport.elapsed_ticks = 480; // one beat in, arbitrary base
        sched.transport.current_step = 1;
        sched.fire_all_clips(1);

        // It must NOT fire immediately (off the 1/16 grid) ...
        let immediate = rx.try_iter().any(|e| matches!(
            e, EngineEvent::AudioNoteOn { slot_id: 5, note: 60, .. }
        ));
        assert!(!immediate, "triplet hit should be deferred, not immediate");
        // ... it must be queued to a sub-step tick after the window start.
        assert!(
            sched.pending_note_ons.iter().any(|p| p.note == 60 && p.at_tick > 480),
            "triplet hit should be queued at its sub-step tick"
        );
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

    /// Milestone B: an arrangement clip routed to a matrix-row instrument plays
    /// that row's instrument when arrangement playback is enabled. "Place clip →
    /// play → hear it."
    #[test]
    fn arrangement_clip_plays_through_routed_row() {
        use seqterm_core::{
            Arrangement, ArrangementClip, ArrangementTrack, Clip, ClipKind, Note, Pattern,
            PatternSource, RationalTime, TrackKind,
        };
        let mut proj = Project::blank("test");
        // Pattern "P": a single note at step 0 on a 1/16 grid.
        let mut pat = Pattern::new("P", 4);
        pat.set_step(0, Note::from_midi(60, 100).unwrap());
        proj.patterns.insert("P".to_string(), pat);
        // Matrix row "A" carries the instrument (SF2 → audio slot 5).
        let mut row_clip = Clip::new("inst", 0, 0).with_pattern("P");
        row_clip.source = PatternSource::Sf2 {
            path: "x.sf2".into(), bank: 0, preset: 0, preset_name: String::new(),
        };
        proj.matrix.insert("A".to_string(), vec![Some(row_clip)]);
        // Arrangement track routed to row "A" with a clip referencing "P".
        let mut arr = Arrangement::default();
        let mut track = ArrangementTrack::new("Lead", TrackKind::Midi);
        track.source_row = Some("A".to_string());
        track.primary_lane_mut().clips.push(ArrangementClip::new(
            0, "clipP",
            ClipKind::Pattern { pattern_key: "P".into() },
            RationalTime::ZERO, RationalTime::whole(4),
        ));
        arr.tracks.push(track);
        proj.arrangement = arr;

        let (mut sched, rx) = make_scheduler(proj);
        let mut slots = HashMap::new();
        slots.insert("A0".to_string(), 5u32);
        sched.handle_command(EngineCommand::SetAudioSlots(slots));

        // Disabled by default → no arrangement notes.
        sched.fire_arrangement_clips(0);
        assert!(
            !rx.try_iter().any(|e| matches!(e, EngineEvent::AudioNoteOn { slot_id: 5, .. })),
            "arrangement must be silent until playback is enabled"
        );

        // Enable and fire the first master step (timeline beat 0).
        sched.handle_command(EngineCommand::SetArrangementPlayback(true));
        sched.fire_arrangement_clips(0);
        let ons: Vec<u8> = rx.try_iter().filter_map(|e| match e {
            EngineEvent::AudioNoteOn { slot_id: 5, note, .. } => Some(note),
            _ => None,
        }).collect();
        assert_eq!(ons, vec![60], "clip's pattern plays through row A's instrument");
    }

    /// Phase B: an arrangement audio clip edge-triggers its loaded slot exactly
    /// once, on the master step whose window contains the clip start.
    #[test]
    fn arrangement_audio_clip_edge_triggers_slot() {
        use seqterm_core::{Arrangement, ArrangementClip, ArrangementTrack, ClipKind, RationalTime, TrackKind};
        let mut proj = Project::blank("test");
        let mut arr = Arrangement::default();
        let mut track = ArrangementTrack::new("Drums", TrackKind::Audio);
        // Audio clip id 7, starts at beat 1 (master step 4), length 4 beats.
        track.primary_lane_mut().clips.push(ArrangementClip::new(
            7, "loop.wav",
            ClipKind::Audio { path: "loop.wav".into(), gain: 1.0 },
            RationalTime::whole(1), RationalTime::whole(4),
        ));
        arr.tracks.push(track);
        proj.arrangement = arr;

        let (mut sched, rx) = make_scheduler(proj);
        let mut map = HashMap::new();
        map.insert(7u64, 9u32);
        sched.handle_command(EngineCommand::SetArrangementAudioSlots(map));
        sched.handle_command(EngineCommand::SetArrangementPlayback(true));

        // Step 3 (beat 0.75) — before the clip start window → no trigger.
        sched.fire_arrangement_clips(3);
        assert!(!rx.try_iter().any(|e| matches!(e, EngineEvent::AudioClipTrigger { slot_id: 9 })));

        // Step 4 (beat 1.0) — clip start lands in [1.0, 1.25) → one trigger.
        sched.fire_arrangement_clips(4);
        let n = rx.try_iter().filter(|e| matches!(e, EngineEvent::AudioClipTrigger { slot_id: 9 })).count();
        assert_eq!(n, 1, "audio clip must trigger exactly once at its start step");

        // Step 5 (beat 1.25) — inside the clip but past its start → no re-trigger.
        sched.fire_arrangement_clips(5);
        assert!(!rx.try_iter().any(|e| matches!(e, EngineEvent::AudioClipTrigger { slot_id: 9 })));
    }

    #[test]
    fn arrangement_automation_emits_changed_cc() {
        use seqterm_core::{
            Arrangement, ArrangementTrack, AutomationCurve, PatternSource, RationalTime, TrackKind,
        };
        let mut proj = Project::blank("test");
        // Matrix row "A" → audio slot 5 (so the lane resolves to an instrument).
        let mut row_clip = seqterm_core::Clip::new("inst", 0, 0).with_pattern("P");
        row_clip.source = PatternSource::Sf2 {
            path: "x.sf2".into(), bank: 0, preset: 0, preset_name: String::new(),
        };
        proj.matrix.insert("A".to_string(), vec![Some(row_clip)]);
        // Track routed to A with a "volume" ramp: beat 0 → 0.0, beat 8 → 1.0.
        let mut arr = Arrangement::default();
        let mut track = ArrangementTrack::new("Lead", TrackKind::Midi);
        track.source_row = Some("A".to_string());
        arr.tracks.push(track);
        proj.arrangement = arr;
        proj.arrangement.set_automation_point(0, "volume", RationalTime::ZERO, 0.0, AutomationCurve::Linear);
        proj.arrangement.set_automation_point(0, "volume", RationalTime::whole(8), 1.0, AutomationCurve::Linear);

        let (mut sched, rx) = make_scheduler(proj);
        let mut slots = HashMap::new();
        slots.insert("A0".to_string(), 5u32);
        sched.handle_command(EngineCommand::SetAudioSlots(slots));

        let cc7 = |rx: &flume::Receiver<EngineEvent>| -> Vec<u8> {
            rx.try_iter().filter_map(|e| match e {
                EngineEvent::AudioControlChange { slot_id: 5, cc: 7, value, .. } => Some(value),
                _ => None,
            }).collect()
        };

        // Beat 0 (master step 0): value 0.0 → CC 7 = 0.
        sched.process_arrangement_automation(0);
        assert_eq!(cc7(&rx), vec![0], "volume CC at beat 0");

        // Same beat again: value unchanged → no duplicate CC.
        sched.process_arrangement_automation(0);
        assert!(cc7(&rx).is_empty(), "unchanged value is not resent");

        // Beat 8 (master step 32): value 1.0 → CC 7 = 127.
        sched.process_arrangement_automation(32);
        assert_eq!(cc7(&rx), vec![127], "volume ramps to full at beat 8");

        // Muting the track suppresses automation output.
        sched.cached_project.arrangement.tracks[0].mute = true;
        sched.process_arrangement_automation(16);
        assert!(cc7(&rx).is_empty(), "muted track emits no automation");
    }

    #[test]
    fn cycle_wraps_arrangement_clock() {
        use seqterm_core::RationalTime;
        let mut proj = Project::blank("test");
        // Cycle over beats [0, 8) → master steps [0, 32) at 4 steps/beat.
        proj.arrangement.cycle = Some((RationalTime::ZERO, RationalTime::whole(8)));
        let (mut sched, _rx) = make_scheduler(proj);

        // Just before the end: no wrap.
        sched.absolute_step = 31;
        sched.maybe_loop_arrangement();
        assert_eq!(sched.absolute_step, 31);

        // At the end (exclusive): wrap to the cycle start.
        sched.absolute_step = 32;
        sched.maybe_loop_arrangement();
        assert_eq!(sched.absolute_step, 0, "clock wraps to cycle start at the end");

        // Far past the end also wraps.
        sched.absolute_step = 1000;
        sched.maybe_loop_arrangement();
        assert_eq!(sched.absolute_step, 0);

        // A non-zero cycle start wraps to that start.
        sched.cached_project.arrangement.cycle =
            Some((RationalTime::whole(4), RationalTime::whole(8)));
        sched.absolute_step = 40;
        sched.maybe_loop_arrangement();
        assert_eq!(sched.absolute_step, 16, "wraps to beat 4 = step 16");

        // No cycle → never wraps.
        sched.cached_project.arrangement.cycle = None;
        sched.absolute_step = 9999;
        sched.maybe_loop_arrangement();
        assert_eq!(sched.absolute_step, 9999);
    }

    /// Song + Phase 6: an arrangement clip whose pattern carries an *exact*
    /// off-grid rational event (a tuplet note that does not land on the 1/16
    /// master grid) plays through the routed row with sub-step tick precision.
    /// Proves the canonical `events` layer survives the whole Song playback path.
    #[test]
    fn arrangement_clip_plays_exact_offgrid_tuplet_event() {
        use seqterm_core::{
            Arrangement, ArrangementClip, ArrangementTrack, ClipKind, Note, Pattern,
            PatternSource, RationalTime, TrackKind,
        };
        let mut proj = Project::blank("test");
        // Pattern "P": one EXACT event at beat 2/7 (a 7-tuplet position, never on
        // the 1/16 grid) — no step notes, so only the canonical layer can fire it.
        let mut pat = Pattern::new("P", 4);
        pat.add_event(
            RationalTime::new(2, 7),
            RationalTime::new(1, 7),
            Note::from_midi(67, 100).unwrap(),
        );
        proj.patterns.insert("P".to_string(), pat);
        // Matrix row "A" carries the instrument (SF2 → audio slot 5).
        let mut row_clip = seqterm_core::Clip::new("inst", 0, 0).with_pattern("P");
        row_clip.source = PatternSource::Sf2 {
            path: "x.sf2".into(), bank: 0, preset: 0, preset_name: String::new(),
        };
        proj.matrix.insert("A".to_string(), vec![Some(row_clip)]);
        // Arrangement track routed to "A" with a clip referencing "P" at beat 0.
        let mut arr = Arrangement::default();
        let mut track = ArrangementTrack::new("Lead", TrackKind::Midi);
        track.source_row = Some("A".to_string());
        track.primary_lane_mut().clips.push(ArrangementClip::new(
            0, "clipP",
            ClipKind::Pattern { pattern_key: "P".into() },
            RationalTime::ZERO, RationalTime::whole(4),
        ));
        arr.tracks.push(track);
        proj.arrangement = arr;

        let (mut sched, _rx) = make_scheduler(proj);
        let mut slots = HashMap::new();
        slots.insert("A0".to_string(), 5u32);
        sched.handle_command(EngineCommand::SetAudioSlots(slots));
        sched.handle_command(EngineCommand::SetArrangementPlayback(true));

        // Master step 0 covers beats [0, 1/4): the event at 2/7 ≈ 0.286 is NOT in
        // this window, so nothing is scheduled yet.
        sched.fire_arrangement_clips(0);
        assert!(sched.pending_note_ons.is_empty(), "2/7 is past the first 1/16 window");

        // Master step 1 covers beats [1/4, 1/2): the event lands here, off-grid,
        // so it is deferred to its exact tick rather than emitted immediately.
        sched.fire_arrangement_clips(1);
        assert_eq!(sched.pending_note_ons.len(), 1, "exact event scheduled in its window");
        let on = &sched.pending_note_ons[0];
        assert_eq!(on.note, 67);
        assert_eq!(on.dest_name, "__audio__5", "routed through row A's audio slot");
        // Offset = 2/7 - 1/4 = 1/28 beat → floor(480/28) = 17 ticks past now.
        assert_eq!(on.at_tick, 17, "sub-step tick precision preserved (1/28 beat @ 480 ppqn)");
        // Gate = 1/7 beat → floor(480/7) = 68 ticks.
        assert_eq!(on.gate_ticks, 68, "exact rational duration preserved");
    }
}
