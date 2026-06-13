use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::note::NoteEvent;
use crate::rational::{RationalTime, Resolution};
use crate::Note;

// ─── PatternSource ────────────────────────────────────────────────────────────

/// Defines what drives a clip's audio output.
///
/// - `Midi`: classic mode — MIDI notes routed to an external port/synth.
/// - `Sf2`: built-in SF2 synthesis via seqterm-audio-engine (oxisynth).
/// - `AudioFile`: sample playback (WAV/FLAC/MP3/OGG).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PatternSource {
    /// Route MIDI to an external port (default, backwards-compatible).
    Midi,
    /// Use an SF2 SoundFont bank+preset for synthesis.
    Sf2 {
        /// Path to the .sf2 file (stored relative to project root on save).
        path: PathBuf,
        /// SF2 bank number (0-127).
        bank: u8,
        /// SF2 preset / program number (0-127).
        preset: u8,
        /// Human-readable preset name (cached, not authoritative).
        #[serde(default)]
        preset_name: String,
    },
    /// Play back an audio file (WAV, FLAC, MP3, OGG).
    AudioFile {
        /// Path to the audio file (stored relative to project root on save).
        path: PathBuf,
        /// Whether to loop the sample.
        #[serde(default)]
        looping: bool,
        /// BPM hint for time-stretch sync (0.0 = no stretch).
        #[serde(default)]
        original_bpm: f64,
        /// Volume gain multiplier (1.0 = unity).
        #[serde(default = "default_audio_gain")]
        gain: f32,
    },
    /// Use an external synthesizer plugin (VST2/VST3/CLAP/LV2/DSSI/SFZ instrument)
    /// discovered by the plugin host as the note source.
    Plugin {
        /// Registry plugin id (filesystem path / uid).
        id: String,
        /// Format tag, e.g. "VST3", "LV2", "DSSI".
        format: String,
        /// Human-readable plugin name (cached, not authoritative).
        #[serde(default)]
        name: String,
    },
}

fn default_audio_gain() -> f32 { 1.0 }

impl Default for PatternSource {
    fn default() -> Self {
        PatternSource::Midi
    }
}

impl PatternSource {
    /// Short display label for UI cells.
    pub fn kind_label(&self) -> &'static str {
        match self {
            PatternSource::Midi => "MIDI",
            PatternSource::Sf2 { .. } => "SF2",
            PatternSource::AudioFile { .. } => "AUDIO",
            PatternSource::Plugin { .. } => "SYNTH",
        }
    }

    /// Icon character for matrix cell display.
    pub fn icon(&self) -> char {
        match self {
            PatternSource::Midi => ' ',
            PatternSource::Sf2 { .. } => '♪',
            PatternSource::AudioFile { .. } => '▶',
            PatternSource::Plugin { .. } => '◇',
        }
    }

    pub fn is_midi(&self) -> bool { matches!(self, PatternSource::Midi) }
    pub fn is_sf2(&self) -> bool { matches!(self, PatternSource::Sf2 { .. }) }
    pub fn is_audio(&self) -> bool { matches!(self, PatternSource::AudioFile { .. }) }
    pub fn is_plugin(&self) -> bool { matches!(self, PatternSource::Plugin { .. }) }
}

fn default_euclid_fill() -> usize { 3 }
fn default_euclid_len() -> usize { 16 }
fn default_humanization() -> u8 { 0 }
fn default_time_sig_num() -> u8 { 4 }
fn default_time_sig_den() -> u8 { 4 }

/// A sequence of up to 128 steps.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Pattern {
    pub name: String,
    /// The actual step data.
    pub steps: Vec<Note>,
    /// Active step count (1-128).
    pub length: usize,
    /// Swing percentage (50 = no swing, 54 = light swing).
    pub swing: u8,
    /// Random variation amount (0-100).
    pub random: u8,
    /// Pattern trigger probability (0-100).
    pub prob: u8,
    /// Euclidean rhythm: number of active pulses.
    #[serde(default = "default_euclid_fill")]
    pub euclid_fill: usize,
    /// Euclidean rhythm: total step count for the pattern.
    #[serde(default = "default_euclid_len")]
    pub euclid_len: usize,
    /// Humanization amount 0-100%.
    #[serde(default = "default_humanization")]
    pub humanization: u8,
    /// Evolution speed: 0=off, 1=slow, 2=medium, 3=fast.
    #[serde(default)]
    pub evolution: u8,
    /// Probability lock (freezes randomization).
    #[serde(default)]
    pub prob_lock: bool,
    /// Pattern-level microshift (-99..+99 ticks).
    #[serde(default)]
    pub microshift: i8,
    /// Time signature numerator (beats per measure, 1-128).
    #[serde(default = "default_time_sig_num")]
    pub time_sig_num: u8,
    /// Time signature denominator (beat unit, 1-128).
    #[serde(default = "default_time_sig_den")]
    pub time_sig_den: u8,
    /// Beat grouping within a measure, e.g. [3,2,2] for 7/8 → 3+2+2.
    /// Empty means one undivided group equal to time_sig_num.
    #[serde(default)]
    pub beat_groups: Vec<u8>,
    /// Per-pattern edit/grid resolution (Phase 2 rational-time model).
    /// One step on the legacy grid equals one step of this resolution.
    /// Defaults to `1/16` so existing projects keep identical step timing.
    #[serde(default)]
    pub resolution: Resolution,
}

impl Pattern {
    /// Maximum pattern length (steps). Long enough for full-piece MIDI imports
    /// while keeping alloc bounded.
    pub const MAX_STEPS: usize = 8192;

    pub fn new(name: impl Into<String>, length: usize) -> Self {
        let length = length.clamp(1, Self::MAX_STEPS);
        Self {
            name: name.into(),
            steps: vec![Note::default(); length],
            length,
            swing: 50,
            random: 0,
            prob: 0,
            euclid_fill: 3,
            euclid_len: length.min(16).max(2),
            humanization: 0,
            evolution: 0,
            prob_lock: false,
            microshift: 0,
            time_sig_num: 4,
            time_sig_den: 4,
            beat_groups: vec![],
            resolution: Resolution::default_edit(),
        }
    }

    /// Return an immutable reference to the step at `index`, if in range.
    pub fn step(&self, index: usize) -> Option<&Note> {
        self.steps.get(index)
    }

    /// Return a mutable reference to the step at `index`, if in range.
    pub fn step_mut(&mut self, index: usize) -> Option<&mut Note> {
        self.steps.get_mut(index)
    }

    /// Set a note at the given step index.
    pub fn set_step(&mut self, index: usize, note: Note) {
        if index < self.steps.len() {
            self.steps[index] = note;
        }
    }

    /// Clear (silence) a step.
    pub fn clear_step(&mut self, index: usize) {
        if index < self.steps.len() {
            self.steps[index] = Note::default();
        }
    }

    /// Calculate the swing offset in ticks for a given step.
    /// `ppqn` is the pulse-per-quarter-note resolution.
    pub fn swing_offset(&self, step: usize, ppqn: u32) -> i32 {
        if step % 2 == 1 {
            ((self.swing as i32 - 50) * ppqn as i32) / 100
        } else {
            0
        }
    }

    /// Rounded-up step count to fit complete measures of the pattern's time signature.
    /// e.g. length=32, time_sig_num=7 → ceil(32/7)*7 = 35.
    pub fn effective_length(&self) -> usize {
        let num = self.time_sig_num.max(1) as usize;
        ((self.length + num - 1) / num) * num
    }

    /// Quantize all note `micro` fields toward zero.
    ///
    /// - `strength` 0-100: how much to pull micro toward the grid (100 = snap fully).
    /// - `grid_divs` 1-32: subdivision of each step (1 = step-level, 2 = half-step, 4 = quarter-step…).
    ///   At grid_divs > 1 each step is divided into `grid_divs` slots; micro is snapped to the
    ///   nearest slot boundary.
    /// - `swing_aware`: if true and `self.swing != 50`, even steps are left untouched (preserving swing groove).
    pub fn quantize(&mut self, strength: u8, grid_divs: usize, swing_aware: bool) {
        let s = (strength as f32 / 100.0).clamp(0.0, 1.0);
        let divs = grid_divs.max(1) as i8;
        for (step_idx, note) in self.steps.iter_mut().enumerate() {
            if note.is_empty() { continue; }
            // Skip even steps when swing-aware (swing offsets them intentionally).
            if swing_aware && self.swing != 50 && step_idx % 2 == 1 { continue; }

            if divs <= 1 {
                // Simple: pull micro straight to zero.
                note.micro = (note.micro as f32 * (1.0 - s)).round() as i8;
            } else {
                // Snap to nearest 1/divs boundary within [-99, 99].
                let slot_size = 100i8 / divs;
                let nearest_slot = ((note.micro as f32 / slot_size as f32).round() as i8)
                    .clamp(-divs + 1, divs - 1);
                let target = nearest_slot * slot_size;
                note.micro = (note.micro as f32 + (target - note.micro) as f32 * s).round() as i8;
            }
        }
    }

    /// Humanize timing: add random micro-offsets up to `amount` percent.
    /// Preserves existing micro values by adding to them (clamped to ±99).
    pub fn humanize_timing(&mut self, amount: u8) {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let range = amount.min(99) as i8;
        for (i, note) in self.steps.iter_mut().enumerate() {
            if note.is_empty() { continue; }
            // Deterministic pseudo-random from step index + current micro.
            let mut h = DefaultHasher::new();
            (i, note.micro).hash(&mut h);
            let r = ((h.finish() as i8).wrapping_add(note.micro)) % (range + 1);
            note.micro = (note.micro + r).clamp(-99, 99);
        }
    }

    // ── Rational-time view (Phase 2) ──────────────────────────────────────────
    //
    // The `steps` grid remains the editing source of truth during the transition;
    // these accessors derive the exact rational-time representation the scheduler
    // and persistence consume. Conversion is lossless for the legacy `1/16` grid
    // and for any resolution: a step's start is `index * resolution.step_beats()`,
    // `micro` shifts the start by `micro%` of one step, and `gate%` of one step is
    // the sounding duration.

    /// Beats spanned by one step at this pattern's resolution.
    pub fn step_beats(&self) -> RationalTime {
        self.resolution.step_beats()
    }

    /// Total musical length of the active region, in beats.
    pub fn length_beats(&self) -> RationalTime {
        self.step_beats() * self.length as i64
    }

    /// Absolute start of step `index` in beats, including its `micro` offset.
    pub fn step_start(&self, index: usize) -> RationalTime {
        let base = self.step_beats() * index as i64;
        let micro = self.steps.get(index).map(|n| n.micro).unwrap_or(0);
        if micro == 0 {
            base
        } else {
            base + self.step_beats() * RationalTime::new(micro as i64, 100)
        }
    }

    /// Sounding duration of step `index` (its `gate%` of one step), in beats.
    pub fn step_duration(&self, index: usize) -> RationalTime {
        let gate = self.steps.get(index).map(|n| n.gate).unwrap_or(100);
        self.step_beats() * RationalTime::new(gate as i64, 100)
    }

    /// Express a target absolute `start` (beats) for step `index` as a `micro`
    /// offset (`%` of one step, clamped `-99..=99`) relative to the step's grid
    /// position. The step index itself is unchanged — this is the canonical-store
    /// representation used by [`quantize_to`](Self::quantize_to) and friends.
    fn micro_for_start(&self, index: usize, start: RationalTime) -> i8 {
        let base = self.step_beats() * index as i64;
        let pct = (start - base) / self.step_beats() * 100;
        // Round to nearest integer percent: floor(x + 1/2).
        let rounded = (pct + RationalTime::new(1, 2)).floor();
        rounded.clamp(-99, 99) as i8
    }

    /// Snap every note's exact rational `start` toward the nearest line of the
    /// `target` grid (a [`Resolution`] under an optional [`Tuplet`]), by
    /// `strength_pct` (0 = no change, 100 = hard snap). Exact rational rounding —
    /// no floating-point drift — so arbitrary/odd grids snap cleanly.
    ///
    /// The result is written back through each note's `micro` field (the canonical
    /// store during the Phase 2 transition); positions are therefore quantized to
    /// the existing `1%`-of-a-step `micro` granularity. UI exposure is Phase 3.
    pub fn quantize_to(
        &mut self,
        target: Resolution,
        tuplet: crate::rational::Tuplet,
        strength_pct: u8,
    ) {
        let grid = target.step_beats() * tuplet.scale();
        if grid.is_zero() {
            return;
        }
        let strength = RationalTime::new(strength_pct.min(100) as i64, 100);
        let end = self.length.min(self.steps.len());
        for i in 0..end {
            if self.steps[i].is_empty() {
                continue;
            }
            let cur = self.step_start(i);
            // Nearest grid line: round(cur / grid) * grid.
            let q = cur / grid;
            let nearest = grid * (q + RationalTime::new(1, 2)).floor();
            let new_start = cur + (nearest - cur) * strength;
            self.steps[i].micro = self.micro_for_start(i, new_start);
        }
    }

    /// Add bounded, deterministic rational jitter to each note's `start`, up to
    /// `amount_pct` percent of one step, written back through `micro`. Unlike the
    /// legacy [`humanize_timing`](Self::humanize_timing) this is expressed in the
    /// rational model and is symmetric around the grid line.
    pub fn humanize_rational(&mut self, amount_pct: u8) {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let range = amount_pct.min(99) as i64;
        if range == 0 {
            return;
        }
        let end = self.length.min(self.steps.len());
        for i in 0..end {
            if self.steps[i].is_empty() {
                continue;
            }
            // Deterministic pseudo-random offset in [-range, range] percent.
            let mut h = DefaultHasher::new();
            (i, self.steps[i].micro).hash(&mut h);
            let jitter = (h.finish() % (2 * range as u64 + 1)) as i64 - range;
            let cur = self.step_start(i);
            let new_start = cur + self.step_beats() * RationalTime::new(jitter, 100);
            self.steps[i].micro = self.micro_for_start(i, new_start);
        }
    }

    /// Change the pattern's edit resolution while **preserving the exact musical
    /// position and duration of every note** (Phase 3, item 7 — "never destroys
    /// data"). The grid is rebuilt from the rational events at the new step size.
    ///
    /// Exactly lossless when the new grid is commensurate with the old (e.g. any
    /// power-of-two change, or a change whose step size evenly divides every
    /// note's offset). For incommensurate grids each note is placed at its
    /// nearest new step with the residual folded into `micro` (1%-of-step
    /// granularity). Returns the number of notes that could not be placed because
    /// a finer original position collapsed onto an already-occupied new step.
    pub fn set_resolution(&mut self, new_res: Resolution) -> usize {
        if new_res.den() == self.resolution.den() {
            self.resolution = new_res;
            return 0;
        }
        let events = self.to_events();
        let total_beats = self.length_beats();
        let new_step = new_res.step_beats();

        // Step count that covers the same musical length (round up to a whole step).
        let n = total_beats / new_step;
        let mut new_len = n.floor();
        if !n.frac().is_zero() {
            new_len += 1;
        }
        let new_len = (new_len.max(1) as usize).min(Self::MAX_STEPS);

        let mut steps = vec![Note::default(); new_len];
        let mut dropped = 0usize;
        for ev in &events {
            // Nearest new step index, residual offset folded into micro.
            let q = ev.start / new_step;
            let j = (q + RationalTime::new(1, 2)).floor();
            if j < 0 || j as usize >= new_len {
                dropped += 1;
                continue;
            }
            let j = j as usize;
            if !steps[j].is_empty() {
                dropped += 1;
                continue;
            }
            let base = new_step * j as i64;
            let micro = {
                let pct = (ev.start - base) / new_step * 100;
                (pct + RationalTime::new(1, 2)).floor().clamp(-99, 99) as i8
            };
            let gate = {
                let pct = ev.duration / new_step * 100;
                (pct + RationalTime::new(1, 2)).floor().clamp(1, u16::MAX as i64) as u16
            };
            let mut note = ev.note.clone();
            note.micro = micro;
            note.gate = gate;
            steps[j] = note;
        }

        self.steps = steps;
        self.length = new_len;
        self.resolution = new_res;
        dropped
    }

    /// Set a note's sounding duration to an exact rational `dur` (beats) — a
    /// graphical "resize end". Stored through `gate` (% of one step); arbitrary
    /// rational targets are kept to 1%-of-step granularity. No-op for empty steps.
    pub fn set_note_duration(&mut self, index: usize, dur: RationalTime) {
        let step = self.step_beats();
        if step.is_zero() {
            return;
        }
        if let Some(note) = self.steps.get_mut(index) {
            if note.is_empty() {
                return;
            }
            let pct = dur / step * 100;
            note.gate = (pct + RationalTime::new(1, 2)).floor().clamp(1, u16::MAX as i64) as u16;
        }
    }

    /// Move a note's onset to an exact rational `new_start` (beats) while keeping
    /// its END fixed — a graphical "resize start". The onset is expressed via
    /// `micro` within the note's own step (clamped to ±99%), and `gate` grows or
    /// shrinks to preserve the end position. No-op for empty steps.
    pub fn resize_note_start(&mut self, index: usize, new_start: RationalTime) {
        let step = self.step_beats();
        if step.is_zero() {
            return;
        }
        let old_start = self.step_start(index);
        let old_end = old_start + self.step_duration(index);
        if let Some(note) = self.steps.get_mut(index) {
            if note.is_empty() {
                return;
            }
            let base = step * index as i64;
            let micro_pct = (new_start - base) / step * 100;
            note.micro = (micro_pct + RationalTime::new(1, 2)).floor().clamp(-99, 99) as i8;
            // Recompute the effective start after micro clamping, keep end fixed.
            let eff_start = base + step * RationalTime::new(note.micro as i64, 100);
            let new_dur = old_end - eff_start;
            let dur = if new_dur.is_negative() || new_dur.is_zero() {
                step / 100 // a sliver; never zero/negative
            } else {
                new_dur
            };
            let pct = dur / step * 100;
            note.gate = (pct + RationalTime::new(1, 2)).floor().clamp(1, u16::MAX as i64) as u16;
        }
    }

    /// Derive the exact rational [`NoteEvent`] list for the active region.
    ///
    /// Timing (`micro`, `gate`) is folded into `start`/`duration`; the embedded
    /// note carries the remaining expressive payload with its timing fields
    /// normalized (`micro = 0`, `gate = 100`) so timing has one source of truth.
    pub fn to_events(&self) -> Vec<NoteEvent> {
        let mut events = Vec::new();
        let end = self.length.min(self.steps.len());
        for i in 0..end {
            let note = &self.steps[i];
            if note.is_empty() {
                continue;
            }
            let mut payload = note.clone();
            payload.micro = 0;
            payload.gate = 100;
            events.push(NoteEvent::new(
                self.step_start(i),
                self.step_duration(i),
                payload,
            ));
        }
        events
    }

    /// Returns the active beat grouping, validating that groups sum to time_sig_num.
    /// Falls back to [time_sig_num] if unset or the sum is inconsistent.
    pub fn effective_groups(&self) -> Vec<u8> {
        let n = self.time_sig_num.max(1);
        let sum: u8 = self.beat_groups.iter().copied().fold(0u8, |a, b| a.saturating_add(b));
        if self.beat_groups.is_empty() || sum != n {
            vec![n]
        } else {
            self.beat_groups.clone()
        }
    }
}

impl Default for Pattern {
    fn default() -> Self {
        Self::new("EMPTY", 64)
    }
}

/// One event hit produced by scanning a beat window of a looping pattern.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowHit {
    /// Index into the `events` slice that was scanned.
    pub event_index: usize,
    /// The event's grid step index (`start / step_beats`), for drum-map/display.
    pub local_step: usize,
    /// Offset of the hit from the window start, in beats (`>= 0`, `< width`).
    pub offset: RationalTime,
}

/// Scan the half-open beat window `[w0, w0 + width)` of a pattern's looping
/// timeline (loop length `loop_len` beats) and return every event that fires in
/// it, each with its in-window `offset` and grid `local_step`.
///
/// This is the rational-time replacement for the legacy `global_step % length`
/// step selection: with a `1/16` pattern and a `1/4`-beat (one master-step)
/// window it yields exactly one hit at offset 0 — identical to the old grid —
/// while odd resolutions, tuplets and polyrhythms place multiple sub-step hits
/// at exact offsets. Pure and allocation-light; unit-tested for parity.
pub fn hits_in_window(
    events: &[NoteEvent],
    loop_len: RationalTime,
    step_beats: RationalTime,
    w0: RationalTime,
    width: RationalTime,
) -> Vec<WindowHit> {
    let mut hits = Vec::new();
    if loop_len.is_zero() || loop_len.is_negative() || width.is_negative() {
        return hits;
    }
    let w1 = w0 + width;
    for (event_index, ev) in events.iter().enumerate() {
        let s = ev.start.rem_euclid(loop_len);
        let local_step = if step_beats.is_zero() {
            0
        } else {
            s.div_floor(step_beats).max(0) as usize
        };
        // Smallest k with s + k*loop_len >= w0, then walk forward through the
        // window (normally a single iteration since width < loop_len).
        let mut k = (w0 - s).div_floor(loop_len);
        loop {
            let cand = s + loop_len * k;
            if cand >= w1 {
                break;
            }
            if cand >= w0 {
                hits.push(WindowHit {
                    event_index,
                    local_step,
                    offset: cand - w0,
                });
            }
            k += 1;
        }
    }
    hits
}

/// All ordered compositions of `n` into parts of 2 and 3 (plus the trivial [n]).
/// For n > 16 only [[n]] is returned to avoid combinatorial explosion.
pub fn musical_groupings(n: u8) -> Vec<Vec<u8>> {
    let n = n.max(1);
    if n > 16 {
        return vec![vec![n]];
    }
    let mut comps: Vec<Vec<u8>> = Vec::new();
    fn compose(rem: u8, cur: &mut Vec<u8>, out: &mut Vec<Vec<u8>>) {
        if rem == 0 { out.push(cur.clone()); return; }
        for p in [2u8, 3u8] {
            if p <= rem { cur.push(p); compose(rem - p, cur, out); cur.pop(); }
        }
    }
    let mut buf = Vec::new();
    compose(n, &mut buf, &mut comps);
    comps.sort();
    comps.dedup();
    comps.retain(|g| g.as_slice() != [n]);
    let mut result = vec![vec![n]];
    result.extend(comps);
    result
}

fn default_clip_enabled() -> bool { true }
fn default_midi_channel() -> u8 { 1 }

/// A clip is a (row, col) slot in the session matrix that references a pattern.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Clip {
    pub name: String,
    /// The pattern key this clip references (e.g. "KCK01").
    pub pattern_key: Option<String>,
    /// Whether the clip is currently scheduled to play.
    pub playing: bool,
    /// Current playback step within the pattern.
    pub current_step: usize,
    /// Matrix row (0-7, A-H).
    pub row: usize,
    /// Matrix column (0-7).
    pub col: usize,
    /// Whether the clip is enabled (false = muted/disabled).
    #[serde(default = "default_clip_enabled")]
    pub enabled: bool,
    /// MIDI output port name to route this clip's pattern to (None = unrouted).
    #[serde(default)]
    pub midi_out: Option<String>,
    /// MIDI output channel (1-16).
    #[serde(default = "default_midi_channel")]
    pub midi_channel: u8,
    /// Audio source type: MIDI (default), SF2, or AudioFile.
    #[serde(default)]
    pub source: PatternSource,
    /// When Some, MIDI output uses MPE channel allocation for this clip.
    #[serde(default)]
    pub mpe_zone: Option<crate::mpe::MpeZone>,
    /// Frozen state: when true, this clip is playing a rendered audio stem.
    /// The original source (MIDI/SF2) is preserved in `freeze_source` for unfreeze.
    #[serde(default)]
    pub frozen: bool,
    /// The original source saved before freezing. Restored on unfreeze.
    #[serde(default)]
    pub freeze_source: Option<Box<PatternSource>>,
}

impl Clip {
    pub fn new(name: impl Into<String>, row: usize, col: usize) -> Self {
        Self {
            name: name.into(),
            pattern_key: None,
            playing: false,
            current_step: 0,
            row,
            col,
            enabled: true,
            midi_out: None,
            midi_channel: 1,
            source: PatternSource::default(),
            mpe_zone: None,
            frozen: false,
            freeze_source: None,
        }
    }

    /// Assign an SF2 source to this clip.
    pub fn with_sf2(mut self, path: impl Into<PathBuf>, bank: u8, preset: u8) -> Self {
        self.source = PatternSource::Sf2 {
            path: path.into(),
            bank,
            preset,
            preset_name: String::new(),
        };
        self
    }

    /// Assign an audio file source to this clip.
    pub fn with_audio(mut self, path: impl Into<PathBuf>, looping: bool) -> Self {
        self.source = PatternSource::AudioFile {
            path: path.into(),
            looping,
            original_bpm: 0.0,
            gain: 1.0,
        };
        self
    }

    pub fn with_pattern(mut self, key: impl Into<String>) -> Self {
        self.pattern_key = Some(key.into());
        self
    }

    pub fn with_channel(mut self, channel: u8) -> Self {
        self.midi_channel = channel.clamp(1, 16);
        self
    }

    pub fn row_label(&self) -> char {
        (b'A' + self.row as u8) as char
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::note::Note;

    fn make_pattern(len: usize) -> Pattern {
        Pattern::new("TEST", len)
    }

    #[test]
    fn pattern_default_steps_empty() {
        let p = make_pattern(16);
        assert_eq!(p.length, 16);
        assert!(p.steps.iter().all(|s| s.is_empty()));
    }

    #[test]
    fn plugin_source_serde_roundtrip() {
        let src = PatternSource::Plugin {
            id: "/usr/lib/lv2/Amp.lv2".into(),
            format: "LV2".into(),
            name: "Amp".into(),
        };
        let json = serde_json::to_string(&src).unwrap();
        assert!(json.contains("\"type\":\"plugin\""), "got {json}");
        let back: PatternSource = serde_json::from_str(&json).unwrap();
        assert_eq!(src, back);
        assert!(back.is_plugin());
        assert_eq!(back.kind_label(), "SYNTH");
    }

    #[test]
    fn legacy_sources_still_deserialize() {
        // Older projects only had midi/sf2/audio_file — they must still load.
        let midi: PatternSource = serde_json::from_str(r#"{"type":"midi"}"#).unwrap();
        assert!(midi.is_midi());
        let audio: PatternSource =
            serde_json::from_str(r#"{"type":"audio_file","path":"x.wav"}"#).unwrap();
        assert!(audio.is_audio());
    }

    #[test]
    fn set_and_get_step() {
        let mut p = make_pattern(8);
        let note = Note::from_midi(60, 100).unwrap();
        p.set_step(0, note.clone());
        assert_eq!(p.step(0).unwrap().to_midi(), Some(60));
    }

    #[test]
    fn clear_step_makes_it_empty() {
        let mut p = make_pattern(8);
        let note = Note::from_midi(64, 80).unwrap();
        p.set_step(3, note);
        assert!(!p.step(3).unwrap().is_empty());
        p.clear_step(3);
        assert!(p.step(3).unwrap().is_empty());
    }

    #[test]
    fn step_out_of_bounds_returns_none() {
        let p = make_pattern(8);
        assert!(p.step(8).is_none());
        assert!(p.step(100).is_none());
    }

    #[test]
    fn effective_length_defaults_to_length() {
        let p = make_pattern(16);
        assert_eq!(p.effective_length(), 16);
    }

    #[test]
    fn pattern_polymeter_phase() {
        let p = make_pattern(6);
        // The scheduler uses `global_step % pat.length`.
        for global_step in 0..18usize {
            let pos = global_step % p.length;
            assert!(pos < p.length);
        }
    }

    #[test]
    fn clip_row_label() {
        let c = Clip::new("PAT1", 0, 0);
        assert_eq!(c.row_label(), 'A');
        let c2 = Clip::new("PAT2", 7, 0);
        assert_eq!(c2.row_label(), 'H');
    }

    #[test]
    fn clip_with_sf2_builder() {
        let c = Clip::new("X", 0, 0)
            .with_sf2("/usr/share/sounds/bank.sf2", 0, 1);
        assert!(c.source.is_sf2());
    }

    #[test]
    fn clip_with_audio_builder() {
        let c = Clip::new("X", 0, 0)
            .with_audio("/tmp/loop.wav", true);
        assert!(c.source.is_audio());
    }

    // ── Rational-time view ────────────────────────────────────────────────────

    #[test]
    fn default_resolution_is_sixteenth() {
        let p = make_pattern(16);
        assert_eq!(p.resolution, Resolution::Whole(16));
        // 16 sixteenths = 4 beats = one 4/4 bar.
        assert_eq!(p.length_beats(), RationalTime::whole(4));
        assert_eq!(p.step_beats(), RationalTime::new(1, 4));
    }

    #[test]
    fn to_events_default_grid_positions() {
        let mut p = make_pattern(16);
        p.set_step(0, Note::from_midi(60, 100).unwrap());
        p.set_step(4, Note::from_midi(64, 100).unwrap()); // one beat in
        let events = p.to_events();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].start, RationalTime::ZERO);
        assert_eq!(events[0].duration, RationalTime::new(1, 4)); // gate 100 = one 1/16
        assert_eq!(events[0].to_midi(), Some(60));
        assert_eq!(events[1].start, RationalTime::whole(1)); // step 4 = 1 beat
    }

    #[test]
    fn to_events_gate_and_micro_fold_into_timing() {
        let mut p = make_pattern(8);
        let mut n = Note::from_midi(60, 100).unwrap();
        n.gate = 50; // half a step
        n.micro = 25; // +25% of a step
        p.set_step(2, n);
        let events = p.to_events();
        assert_eq!(events.len(), 1);
        // step 2 base = 2/4 = 1/2 beat; micro +25% of 1/4 beat = 1/16 beat.
        assert_eq!(events[0].start, RationalTime::new(1, 2) + RationalTime::new(1, 16));
        // gate 50% of 1/4 beat = 1/8 beat.
        assert_eq!(events[0].duration, RationalTime::new(1, 8));
        // Timing fields normalized in the payload.
        assert_eq!(events[0].note.micro, 0);
        assert_eq!(events[0].note.gate, 100);
        // Expressive payload preserved.
        assert_eq!(events[0].note.velocity, 100);
    }

    #[test]
    fn to_events_triplet_resolution() {
        // 1/8 triplet grid: each step is a 1/8 note scaled into a triplet — but
        // resolution alone is Whole(12) (a 1/12 note = 1/3 beat).
        let mut p = make_pattern(12);
        p.resolution = Resolution::Whole(12);
        p.set_step(0, Note::from_midi(60, 100).unwrap());
        p.set_step(3, Note::from_midi(60, 100).unwrap()); // 3 twelfths = 1 beat
        let events = p.to_events();
        assert_eq!(events[0].start, RationalTime::ZERO);
        assert_eq!(events[1].start, RationalTime::whole(1));
        assert_eq!(events[0].duration, RationalTime::new(1, 3));
        assert_eq!(p.length_beats(), RationalTime::whole(4)); // 12 * 1/3 = 4 beats
    }

    #[test]
    fn hits_in_window_matches_legacy_grid_selection() {
        // A 1/16 pattern, 8 steps (2 beats). Scanning each master 1/4-beat window
        // must select exactly the legacy `global_step % length` step at offset 0.
        let mut p = make_pattern(8);
        for i in 0..8 {
            p.set_step(i, Note::from_midi(60 + i as u8, 100).unwrap());
        }
        let events = p.to_events();
        let loop_len = p.length_beats(); // 2 beats
        let step = p.step_beats(); // 1/4
        for global_step in 0..24usize {
            let w0 = step * global_step as i64;
            let hits = hits_in_window(&events, loop_len, step, w0, step);
            assert_eq!(hits.len(), 1, "exactly one hit per window at step {global_step}");
            assert_eq!(hits[0].local_step, global_step % 8);
            assert_eq!(hits[0].offset, RationalTime::ZERO, "1/16 grid fires on-beat");
        }
    }

    #[test]
    fn hits_in_window_triplet_subdivision() {
        // 1/12 grid (eighth-note triplets), one beat = 3 steps. Scanning the first
        // master 1/4-beat window [0, 1/4) catches only the triplet hit at 0; the
        // next two triplet hits (1/3, 2/3 beat) land in later windows.
        let mut p = make_pattern(12);
        p.resolution = Resolution::Whole(12);
        for i in 0..12 {
            p.set_step(i, Note::from_midi(60, 100).unwrap());
        }
        let events = p.to_events();
        let loop_len = p.length_beats(); // 4 beats
        let master = RationalTime::new(1, 4);
        // Collect all hits across a full bar of master windows; every triplet step
        // must fire exactly once with the correct sub-step offset.
        let mut fired = Vec::new();
        for gs in 0..16usize {
            let w0 = master * gs as i64;
            for h in hits_in_window(&events, loop_len, p.step_beats(), w0, master) {
                fired.push((h.local_step, w0 + h.offset));
            }
        }
        assert_eq!(fired.len(), 12, "all 12 triplet steps fire once each");
        // Triplet step 1 sits at 1/3 beat — between master windows, fired with offset.
        assert!(fired.iter().any(|(ls, at)| *ls == 1 && *at == RationalTime::new(1, 3)));
        assert!(fired.iter().any(|(ls, at)| *ls == 2 && *at == RationalTime::new(2, 3)));
    }

    #[test]
    fn hits_in_window_polyrhythm_independent_loops() {
        // Two patterns of different lengths loop independently (polymeter): a
        // 3-step and a 4-step 1/16 pattern realign only every 12 master steps.
        let mut a = make_pattern(3);
        let mut b = make_pattern(4);
        a.set_step(0, Note::from_midi(60, 100).unwrap());
        b.set_step(0, Note::from_midi(67, 100).unwrap());
        let (ea, eb) = (a.to_events(), b.to_events());
        let step = RationalTime::new(1, 4);
        let mut coincidences = 0;
        for gs in 0..12usize {
            let w0 = step * gs as i64;
            let ha = hits_in_window(&ea, a.length_beats(), step, w0, step);
            let hb = hits_in_window(&eb, b.length_beats(), step, w0, step);
            if !ha.is_empty() && !hb.is_empty() {
                coincidences += 1;
            }
        }
        // Both fire on step 0 only — coincide at gs 0 (the bar start). Pattern A
        // (len 3) fires at 0,3,6,9; B (len 4) at 0,4,8 → overlap only at 0.
        assert_eq!(coincidences, 1);
    }

    #[test]
    fn quantize_to_snaps_micro_to_grid() {
        use crate::rational::Tuplet;
        let mut p = make_pattern(8);
        let mut n = Note::from_midi(60, 100).unwrap();
        n.micro = 25; // step 2 (0.5 beat) + 25% of 1/4 = 0.5625 beat
        p.set_step(2, n);
        // Hard-snap to the 1/16 grid: 0.5625 → nearest 1/4 line = 0.5 → micro 0.
        p.quantize_to(Resolution::Whole(16), Tuplet::NONE, 100);
        assert_eq!(p.steps[2].micro, 0);
        assert_eq!(p.step_start(2), RationalTime::new(1, 2));
    }

    #[test]
    fn quantize_to_partial_strength_pulls_halfway() {
        use crate::rational::Tuplet;
        let mut p = make_pattern(8);
        let mut n = Note::from_midi(60, 100).unwrap();
        n.micro = 40; // 40% off the grid
        p.set_step(0, n);
        // 50% strength pulls the offset toward 0 by half → ~20%.
        p.quantize_to(Resolution::Whole(16), Tuplet::NONE, 50);
        assert_eq!(p.steps[0].micro, 20);
    }

    #[test]
    fn quantize_to_triplet_grid() {
        use crate::rational::Tuplet;
        // A 1/16 note near the first triplet line snaps onto it.
        let mut p = make_pattern(8);
        let mut n = Note::from_midi(60, 100).unwrap();
        // step 1 = 0.25 beat; nudge toward 1/3 beat (first eighth-triplet line).
        n.micro = 30; // 0.25 + 0.075 = 0.325 beat, near 1/3 = 0.3333
        p.set_step(1, n);
        p.quantize_to(Resolution::Whole(8), Tuplet::new(3, 2), 100);
        // 1/8-triplet grid = 1/3 beat. The snap target is exactly 1/3, but the
        // canonical `micro` store rounds to 1%-of-step → 33% (start 133/400 ≈
        // 0.3325), the nearest representable position to 1/3 ≈ 0.3333.
        assert_eq!(p.steps[1].micro, 33);
        assert!((p.step_start(1).to_f64() - 1.0 / 3.0).abs() < 0.003);
    }

    #[test]
    fn humanize_rational_is_bounded_and_deterministic() {
        let mut a = make_pattern(8);
        let mut b = make_pattern(8);
        for p in [&mut a, &mut b] {
            for i in 0..8 {
                p.set_step(i, Note::from_midi(60, 100).unwrap());
            }
        }
        a.humanize_rational(20);
        b.humanize_rational(20);
        for i in 0..8 {
            assert!(a.steps[i].micro.abs() <= 20, "jitter within ±20%");
            assert_eq!(a.steps[i].micro, b.steps[i].micro, "deterministic");
        }
    }

    #[test]
    fn set_resolution_preserves_positions_power_of_two() {
        // 1/16 → 1/32 is commensurate: every note's exact start/duration survives.
        let mut p = make_pattern(8); // 2 beats
        p.set_step(0, Note::from_midi(60, 100).unwrap());
        p.set_step(3, Note::from_midi(64, 100).unwrap());
        let mut n = Note::from_midi(67, 100).unwrap();
        n.micro = 50; // +50% of a 1/16 step = +1/8 beat
        p.set_step(5, n);
        let before = p.to_events();

        let dropped = p.set_resolution(Resolution::Whole(32));
        assert_eq!(dropped, 0);
        assert_eq!(p.resolution, Resolution::Whole(32));
        assert_eq!(p.length, 16); // 2 beats / (1/8 beat per 1/32 step)
        let after = p.to_events();

        assert_eq!(after.len(), before.len());
        for (a, b) in after.iter().zip(before.iter()) {
            assert_eq!(a.start, b.start, "position preserved exactly");
            assert_eq!(a.duration, b.duration, "duration preserved exactly");
            assert_eq!(a.to_midi(), b.to_midi());
        }
    }

    #[test]
    fn set_resolution_preserves_musical_length() {
        let mut p = make_pattern(16); // 4 beats at 1/16
        assert_eq!(p.length_beats(), RationalTime::whole(4));
        p.set_resolution(Resolution::Whole(8)); // coarser: 1/8 = 1/2 beat steps
        assert_eq!(p.length, 8); // 4 beats / (1/2 beat) = 8 steps
        assert_eq!(p.length_beats(), RationalTime::whole(4));
    }

    #[test]
    fn set_note_duration_resizes_end() {
        let mut p = make_pattern(8);
        p.set_step(0, Note::from_midi(60, 100).unwrap());
        // Resize to 3/4 beat = three 1/16 steps.
        p.set_note_duration(0, RationalTime::new(3, 4));
        assert_eq!(p.steps[0].gate, 300);
        assert_eq!(p.step_duration(0), RationalTime::new(3, 4));
    }

    #[test]
    fn resize_note_start_keeps_end_fixed() {
        let mut p = make_pattern(8);
        let mut n = Note::from_midi(60, 100).unwrap();
        n.gate = 100; // one step long: start 0, end 1/4 beat
        p.set_step(0, n);
        let end_before = p.step_start(0) + p.step_duration(0);
        // Pull the onset later, to +1/8 beat (50% of the 1/16 step).
        p.resize_note_start(0, RationalTime::new(1, 8));
        assert_eq!(p.steps[0].micro, 50);
        // End is unchanged; duration shrank to 1/8 beat.
        let end_after = p.step_start(0) + p.step_duration(0);
        assert_eq!(end_after, end_before);
        assert_eq!(p.step_duration(0), RationalTime::new(1, 8));
    }

    #[test]
    fn resolution_defaults_when_absent_in_json() {
        // Older patterns serialized without `resolution` must default to 1/16.
        let json = r#"{"name":"P","steps":[],"length":16,"swing":50,"random":0,"prob":0}"#;
        let p: Pattern = serde_json::from_str(json).unwrap();
        assert_eq!(p.resolution, Resolution::Whole(16));
    }
}

