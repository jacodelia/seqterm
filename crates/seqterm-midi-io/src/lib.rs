//! MIDI file import, export, and OSC server for SeqTerm.
//!
//! Import: SMF Type 0/1 → SeqTerm Patterns/Clips.
//! Export: Project arrangement → SMF Type 1.
//! OSC: incoming UDP OSC → OscMsg events for the App loop.

pub mod osc;
pub use osc::{OscArg, OscMsg, OscServer};

use std::{
    collections::HashMap,
    hash::{Hash, Hasher},
    path::Path,
};

use anyhow::{Context, Result, bail};
use seqterm_core::{Clip, Note, Pattern, Project};
use seqterm_core::project::{AutomationLane, Track};
use tracing::info;

pub use seqterm_audio_export::export_wav_stub;

// ─── Public options / results ────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub struct MidiImportOptions {
    /// How many bars per pattern slice (1, 2, 4, or 8).
    pub bars_per_pattern: usize,
    /// Steps per beat: 4 = 16th notes (default), 8 = 32nd notes.
    pub steps_per_beat: u32,
    /// Treat channel 9 (0-indexed) as percussion and flag accordingly.
    pub detect_drums: bool,
}

impl Default for MidiImportOptions {
    fn default() -> Self {
        Self { bars_per_pattern: 4, steps_per_beat: 4, detect_drums: true }
    }
}

#[derive(Debug)]
pub struct ImportedMidi {
    pub patterns:   HashMap<String, Pattern>,
    pub matrix:     HashMap<String, Vec<Option<Clip>>>,
    pub tracks:     Vec<Track>,
    pub automation: Vec<AutomationLane>,
    pub bpm:        f64,
    pub summary:    String,
}

// ─── Track probe (fast pre-import scan) ──────────────────────────────────────

/// Lightweight track metadata extracted without full pattern conversion.
#[derive(Debug, Clone)]
pub struct MidiTrackInfo {
    pub name:       String,
    pub channel:    u8,
    pub note_count: usize,
    pub is_drum:    bool,
}

/// Quickly scan a MIDI file and return one `MidiTrackInfo` per non-empty track.
pub fn probe_midi(path: &Path) -> Result<Vec<MidiTrackInfo>> {
    let bytes = std::fs::read(path)
        .with_context(|| format!("reading {}", path.display()))?;
    let smf = midly::Smf::parse(&bytes).context("parsing MIDI file")?;

    let mut infos = Vec::new();
    for (idx, track) in smf.tracks.iter().enumerate() {
        let mut name      = String::new();
        let mut channel   = 0u8;
        let mut note_count = 0usize;
        let mut is_drum   = false;

        for ev in track {
            match ev.kind {
                midly::TrackEventKind::Meta(midly::MetaMessage::TrackName(n)) => {
                    name = String::from_utf8_lossy(n).trim().to_string();
                }
                midly::TrackEventKind::Midi { channel: ch, message } => {
                    let ch_v = ch.as_int();
                    if ch_v == 9 { is_drum = true; }
                    channel = ch_v;
                    if let midly::MidiMessage::NoteOn { vel, .. } = message {
                        if vel.as_int() > 0 { note_count += 1; }
                    }
                }
                _ => {}
            }
        }
        if note_count == 0 { continue; } // skip tempo/meta-only tracks
        if name.is_empty() { name = format!("TRK{:02}", idx + 1); }
        infos.push(MidiTrackInfo { name, channel, note_count, is_drum });
    }
    Ok(infos)
}

// ─── Import ──────────────────────────────────────────────────────────────────

pub fn import_midi(path: &Path, opts: &MidiImportOptions) -> Result<ImportedMidi> {
    let bytes = std::fs::read(path)
        .with_context(|| format!("reading {}", path.display()))?;
    let smf = midly::Smf::parse(&bytes)
        .context("parsing MIDI file")?;

    let ppq = match smf.header.timing {
        midly::Timing::Metrical(t) => t.as_int() as u32,
        midly::Timing::Timecode(..) => bail!("SMPTE timecode MIDI files are not supported"),
    };

    // ── Tempo map (tick → µs/beat) ────────────────────────────────────────
    let tempo_map = build_tempo_map(&smf.tracks, smf.header.format);
    let bpm = tempo_map
        .first()
        .map(|(_, us)| 60_000_000.0 / *us as f64)
        .unwrap_or(120.0);

    // ── Time signature map ────────────────────────────────────────────────
    let time_sig_map = build_time_sig_map(&smf.tracks, smf.header.format);
    let (initial_num, _initial_den) = time_sig_map
        .first()
        .map(|(_, n, d)| (*n, *d))
        .unwrap_or((4, 4));

    let ticks_per_step = ppq / opts.steps_per_beat;
    let steps_per_bar  = opts.steps_per_beat * initial_num as u32;
    let steps_per_pat  = (opts.bars_per_pattern as u32 * steps_per_bar) as usize;

    let mut all_patterns: HashMap<String, Pattern> = HashMap::new();
    let mut matrix:       HashMap<String, Vec<Option<Clip>>> = HashMap::new();
    let mut arr_tracks:   Vec<Track> = Vec::new();
    // Fingerprint → first pattern name that used those steps (for deduplication).
    let mut fingerprints: HashMap<u64, String> = HashMap::new();

    // ── Detect which track list to use ────────────────────────────────────
    let tracks_iter: Vec<_> = smf.tracks.iter().enumerate().collect();

    let mut row_index = 0usize;

    for (track_idx, track) in tracks_iter {
        // Collect note events for this track.
        let (track_name, note_events, ch_hint) =
            extract_notes(track, ppq, ticks_per_step, opts.detect_drums);

        if note_events.is_empty() {
            continue;
        }

        let row_label = (b'A' + row_index as u8) as char;
        let base_name = if track_name.is_empty() {
            format!("TRK{:02}", track_idx + 1)
        } else {
            sanitize_name(&track_name)
        };

        // Split note events into pattern-length slices.
        let max_step = note_events.iter().map(|(s, ..)| *s).max().unwrap_or(0);
        let num_pats = (max_step / steps_per_pat + 1).max(1);

        let mut col_slots: Vec<Option<Clip>> = vec![None; num_pats];

        for pat_idx in 0..num_pats {
            let pat_start = pat_idx * steps_per_pat;
            let pat_end   = pat_start + steps_per_pat;

            let local: Vec<_> = note_events
                .iter()
                .filter(|(s, ..)| *s >= pat_start && *s < pat_end)
                .collect();

            if local.is_empty() { continue; }

            let mut pat = Pattern::new("", steps_per_pat);
            // Apply time signature active at this pattern's start tick.
            let pat_start_tick = (pat_start as u32) * ticks_per_step;
            let (ts_num, ts_den) = time_sig_at_tick(&time_sig_map, pat_start_tick);
            pat.time_sig_num = ts_num;
            pat.time_sig_den = ts_den;
            for &(abs_step, pitch, vel, gate, micro, cc01, cc74, pb) in &local {
                let local_step = abs_step - pat_start;
                if local_step >= steps_per_pat { continue; }
                if let Ok(note) = Note::from_midi(*pitch, *vel) {
                    let s = &mut pat.steps[local_step];
                    *s = note;
                    s.gate       = *gate;
                    s.micro      = *micro;
                    s.cc01       = *cc01;
                    s.cc74       = *cc74;
                    s.pitch_bend = *pb;
                }
            }

            let candidate_name = if num_pats == 1 {
                base_name.clone()
            } else {
                format!("{}{:02}", base_name, pat_idx + 1)
            };

            // Deduplication: reuse an identical pattern if we've already seen these steps.
            let fp = pattern_fingerprint(&pat);
            let actual_name = if let Some(existing) = fingerprints.get(&fp) {
                existing.clone()
            } else {
                fingerprints.insert(fp, candidate_name.clone());
                pat.name = candidate_name.clone();
                all_patterns.insert(candidate_name.clone(), pat);
                candidate_name
            };

            let clip = Clip::new(actual_name.clone(), row_index, pat_idx)
                .with_pattern(&actual_name)
                .with_channel(ch_hint + 1);
            col_slots[pat_idx] = Some(clip);
        }

        // Build arranger Track entry mirroring the MIDI arrangement timeline.
        let arr_blocks: Vec<(u32, u32, String)> = col_slots.iter().enumerate()
            .filter_map(|(col_idx, slot)| {
                slot.as_ref().map(|clip| {
                    let start_bar = col_idx as u32 * opts.bars_per_pattern as u32;
                    (start_bar, opts.bars_per_pattern as u32, clip.name.clone())
                })
            })
            .collect();
        arr_tracks.push(Track { name: base_name, blocks: arr_blocks, mute: false });

        let row_key = row_label.to_string();
        matrix.insert(row_key, col_slots);
        row_index += 1;
        if row_index >= 16 { break; } // max 16 rows (A-P)
    }

    // ── Tempo automation lane (only when the file has multiple tempos) ─────
    let mut automation: Vec<AutomationLane> = Vec::new();
    let real_tempo_events: Vec<_> = tempo_map.iter()
        .skip(1) // skip the synthetic default at tick 0
        .filter(|(tick, _)| *tick > 0)
        .collect();
    if !real_tempo_events.is_empty() {
        let mut lane = AutomationLane::new("BPM", "bpm");
        for (tick, us_per_beat) in &tempo_map {
            if *us_per_beat == 0 { continue; }
            let beat_bpm = 60_000_000.0 / *us_per_beat as f64;
            let bar = tick / (ppq * 4).max(1);
            // Remap BPM 20-300 → 0-127.
            let val = ((beat_bpm - 20.0) / 280.0 * 127.0).round().clamp(0.0, 127.0) as u8;
            lane.points.push((bar, val));
        }
        lane.points.dedup_by_key(|(bar, _)| *bar);
        automation.push(lane);
    }

    let dedup_count = fingerprints.len();
    let summary = format!(
        "Imported {} patterns ({} unique) across {} tracks @ {:.0} BPM",
        num_pats_total(&matrix),
        dedup_count,
        row_index,
        bpm,
    );
    info!("{summary}");

    Ok(ImportedMidi { patterns: all_patterns, matrix, tracks: arr_tracks, automation, bpm, summary })
}

// ─── Export ──────────────────────────────────────────────────────────────────

/// Export the full project arrangement as SMF Type 1.
pub fn export_midi(project: &Project, path: &Path) -> Result<()> {
    const PPQ: u32 = 480;
    let ticks_per_step = PPQ / 4; // 16th note steps

    // Collect tracks: one per pattern row in the matrix.
    let mut track_data: Vec<Vec<u8>> = Vec::new();

    // Track 0 = tempo track.
    let us_per_beat = (60_000_000.0 / project.bpm) as u32;
    // Derive time signature from the first non-empty pattern (or default 4/4).
    let (ts_num, ts_den) = project.patterns.values().next()
        .map(|p| (p.time_sig_num.max(1), p.time_sig_den.max(1)))
        .unwrap_or((4, 4));
    let ts_den_pow2 = (ts_den as f64).log2().round() as u8; // 4→2, 8→3, 16→4

    let mut tempo_track = Vec::new();
    // Time signature meta: FF 58 04 <num> <den_pow2> <midi_clocks_per_click> <32nds_per_quarter>
    write_varlen(&mut tempo_track, 0);
    tempo_track.extend_from_slice(&[0xFF, 0x58, 0x04, ts_num, ts_den_pow2, 24, 8]);
    // Tempo meta: FF 51 03 <µs/beat>
    write_varlen(&mut tempo_track, 0);
    tempo_track.extend_from_slice(&[0xFF, 0x51, 0x03]);
    tempo_track.extend_from_slice(&us_per_beat.to_be_bytes()[1..]); // 3 bytes
    write_varlen(&mut tempo_track, 0);
    tempo_track.extend_from_slice(&[0xFF, 0x2F, 0x00]); // end of track
    track_data.push(tempo_track);

    // One MIDI track per matrix row (A–P).
    let mut sorted_rows: Vec<_> = project.matrix.iter().collect();
    sorted_rows.sort_by_key(|(k, _)| k.as_str());

    for (row_key, slots) in &sorted_rows {
        // Gather all patterns for this row in column order.
        let mut events: Vec<(u32, u8, u8, u8, u16)> = Vec::new(); // (tick, ch, pitch, vel, gate_ticks)
        let mut col_tick_offset = 0u32;

        for slot in slots.iter() {
            if let Some(clip) = slot {
                if let Some(pat) = clip
                    .pattern_key
                    .as_ref()
                    .and_then(|k| project.patterns.get(k))
                {
                    let ch = (clip.midi_channel.saturating_sub(1)) & 0x0F;
                    for (step, note) in pat.steps.iter().enumerate() {
                        if note.is_empty() { continue; }
                        if let Some(pitch) = seqterm_core::note::parse_note_name(&note.note) {
                            let tick = col_tick_offset + step as u32 * ticks_per_step;
                            let gate = (note.gate as u32 * ticks_per_step / 100).max(10);
                            events.push((tick, ch, pitch, note.velocity, gate as u16));
                        }
                    }
                    col_tick_offset += pat.length as u32 * ticks_per_step;
                }
            }
        }

        if events.is_empty() { continue; }

        // Sort by tick.
        events.sort_unstable_by_key(|(t, ..)| *t);

        // Build note-on / note-off pairs.
        let mut raw_events: Vec<(u32, Vec<u8>)> = Vec::new();
        for (tick, ch, pitch, vel, gate) in events {
            raw_events.push((tick, vec![0x90 | ch, pitch, vel]));
            raw_events.push((tick + gate as u32, vec![0x80 | ch, pitch, 0]));
        }
        raw_events.sort_unstable_by_key(|(t, _)| *t);

        // Encode as delta-time events.
        let mut track = Vec::new();
        // Track name meta event.
        write_varlen(&mut track, 0);
        track.push(0xFF); track.push(0x03);
        write_varlen(&mut track, row_key.len() as u32);
        track.extend_from_slice(row_key.as_bytes());

        let mut last_tick = 0u32;
        for (tick, msg) in raw_events {
            let delta = tick.saturating_sub(last_tick);
            write_varlen(&mut track, delta);
            track.extend_from_slice(&msg);
            last_tick = tick;
        }
        // End of track.
        write_varlen(&mut track, 0);
        track.extend_from_slice(&[0xFF, 0x2F, 0x00]);
        track_data.push(track);
    }

    // Automation tracks: one SMF track per enabled CC automation lane.
    // Target format: "channel.N.ccM" → channel N (1-indexed), CC number M.
    let ticks_per_bar = 16 * ticks_per_step; // 4/4 at 16th-note resolution
    for lane in &project.automation {
        if !lane.enabled || lane.points.is_empty() { continue; }
        let Some((ch_idx, cc_num)) = parse_cc_target(&lane.target) else { continue };
        let ch = ch_idx & 0x0F;

        let mut events: Vec<(u32, Vec<u8>)> = lane.points.iter().map(|(bar, val)| {
            let tick = *bar * ticks_per_bar;
            (tick, vec![0xB0 | ch, cc_num, *val])
        }).collect();
        events.sort_unstable_by_key(|(t, _)| *t);

        let mut track = Vec::new();
        write_varlen(&mut track, 0);
        track.push(0xFF); track.push(0x03);
        write_varlen(&mut track, lane.name.len() as u32);
        track.extend_from_slice(lane.name.as_bytes());

        let mut last_tick = 0u32;
        for (tick, msg) in events {
            let delta = tick.saturating_sub(last_tick);
            write_varlen(&mut track, delta);
            track.extend_from_slice(&msg);
            last_tick = tick;
        }
        write_varlen(&mut track, 0);
        track.extend_from_slice(&[0xFF, 0x2F, 0x00]);
        track_data.push(track);
    }

    // Assemble SMF.
    let mut out: Vec<u8> = Vec::new();
    write_midi_header(&mut out, track_data.len() as u16, PPQ as u16);
    for track in track_data {
        out.extend_from_slice(b"MTrk");
        out.extend_from_slice(&(track.len() as u32).to_be_bytes());
        out.extend_from_slice(&track);
    }

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, &out)
        .with_context(|| format!("writing {}", path.display()))?;
    info!("Exported MIDI to {}", path.display());
    Ok(())
}

/// Parse "channel.N.ccM" → (channel_index_0based, cc_number).
fn parse_cc_target(target: &str) -> Option<(u8, u8)> {
    let parts: Vec<&str> = target.splitn(3, '.').collect();
    if parts.len() != 3 || parts[0] != "channel" { return None; }
    let ch: u8 = parts[1].parse().ok()?;
    let cc_str = parts[2].strip_prefix("cc")?;
    let cc: u8 = cc_str.parse().ok()?;
    Some((ch.saturating_sub(1), cc))
}

/// Export only the matrix rows that have at least one placed clip.
///
/// Patterns not referenced by any matrix slot are omitted.
pub fn export_midi_active_only(project: &Project, path: &Path) -> Result<()> {
    // Collect the set of row keys that have at least one Some clip.
    let active_rows: std::collections::HashSet<String> = project.matrix.iter()
        .filter(|(_, slots)| slots.iter().any(|s| s.is_some()))
        .map(|(k, _)| k.clone())
        .collect();

    if active_rows.is_empty() {
        anyhow::bail!("No active matrix rows to export");
    }

    // Build a temporary project with only the active rows.
    let mut filtered = project.clone();
    filtered.matrix.retain(|k, _| active_rows.contains(k));

    // Collect all pattern keys still referenced.
    let used_patterns: std::collections::HashSet<String> = filtered.matrix.values()
        .flat_map(|slots| slots.iter().flatten())
        .filter_map(|c| c.pattern_key.as_deref())
        .map(|s| s.to_owned())
        .collect();
    filtered.patterns.retain(|k, _| used_patterns.contains(k));

    export_midi(&filtered, path)
}

// ─── Internals ────────────────────────────────────────────────────────────────

type NoteEvent = (usize, u8, u8, u16, i8, u8, u8, i16);
// (step, pitch, vel, gate_steps*100, micro, cc01, cc74, pitch_bend)

fn extract_notes(
    track: &[midly::TrackEvent<'_>],
    _ppq: u32,
    ticks_per_step: u32,
    detect_drums: bool,
) -> (String, Vec<NoteEvent>, u8) {
    let mut name = String::new();
    let mut tick = 0u32;
    // pitch → (on_tick, vel)
    let mut active: HashMap<u8, (u32, u8)> = HashMap::new();
    let mut events: Vec<NoteEvent> = Vec::new();
    let mut ch_hint = 0u8;
    // Per-step accumulators
    let mut cc01_at: HashMap<usize, u8>  = HashMap::new();
    let mut cc74_at: HashMap<usize, u8>  = HashMap::new();
    let mut pb_at:   HashMap<usize, i16> = HashMap::new();

    for ev in track {
        tick += ev.delta.as_int();
        match ev.kind {
            midly::TrackEventKind::Meta(m) => {
                if let midly::MetaMessage::TrackName(n) = m {
                    name = String::from_utf8_lossy(n).trim().to_string();
                }
            }
            midly::TrackEventKind::Midi { channel, message } => {
                let ch = channel.as_int();
                if detect_drums && ch == 9 { ch_hint = 9; }

                let step_idx = (tick / ticks_per_step.max(1)) as usize;
                let micro_raw = (tick % ticks_per_step.max(1)) as f32
                    / ticks_per_step.max(1) as f32 * 198.0 - 99.0;
                let micro = micro_raw.round().clamp(-99.0, 99.0) as i8;

                match message {
                    midly::MidiMessage::NoteOn { key, vel } => {
                        let pitch = key.as_int();
                        let vel_v = vel.as_int();
                        if vel_v == 0 {
                            // NoteOn vel=0 treated as NoteOff
                            if let Some((on_tick, on_vel)) = active.remove(&pitch) {
                                let dur_ticks = tick.saturating_sub(on_tick).max(1);
                                let gate = ((dur_ticks * 100 / ticks_per_step.max(1))
                                    .clamp(10, 400)) as u16;
                                let s = (on_tick / ticks_per_step.max(1)) as usize;
                                let c1 = cc01_at.get(&s).copied().unwrap_or(0);
                                let c74 = cc74_at.get(&s).copied().unwrap_or(0);
                                let pb  = pb_at.get(&s).copied().unwrap_or(0);
                                events.push((s, pitch, on_vel, gate, micro, c1, c74, pb));
                            }
                        } else {
                            // Overlapping note: truncate earlier occurrence before starting new one.
                            if let Some((on_tick, on_vel)) = active.remove(&pitch) {
                                let dur_ticks = tick.saturating_sub(on_tick).max(1);
                                let gate = ((dur_ticks * 100 / ticks_per_step.max(1))
                                    .clamp(10, 400)) as u16;
                                let s = (on_tick / ticks_per_step.max(1)) as usize;
                                let c1 = cc01_at.get(&s).copied().unwrap_or(0);
                                let c74 = cc74_at.get(&s).copied().unwrap_or(0);
                                let pb  = pb_at.get(&s).copied().unwrap_or(0);
                                events.push((s, pitch, on_vel, gate, micro, c1, c74, pb));
                            }
                            active.insert(pitch, (tick, vel_v));
                            if ch_hint == 0 { ch_hint = ch; }
                        }
                    }
                    midly::MidiMessage::NoteOff { key, .. } => {
                        let pitch = key.as_int();
                        if let Some((on_tick, on_vel)) = active.remove(&pitch) {
                            let dur_ticks = tick.saturating_sub(on_tick).max(1);
                            let gate = ((dur_ticks * 100 / ticks_per_step.max(1))
                                .clamp(10, 400)) as u16;
                            let s = (on_tick / ticks_per_step.max(1)) as usize;
                            let c1 = cc01_at.get(&s).copied().unwrap_or(0);
                            let c74 = cc74_at.get(&s).copied().unwrap_or(0);
                            let pb  = pb_at.get(&s).copied().unwrap_or(0);
                            events.push((s, pitch, on_vel, gate, micro, c1, c74, pb));
                        }
                    }
                    midly::MidiMessage::Controller { controller, value } => {
                        match controller.as_int() {
                            1  => { cc01_at.insert(step_idx, value.as_int()); }
                            74 => { cc74_at.insert(step_idx, value.as_int()); }
                            _ => {}
                        }
                    }
                    midly::MidiMessage::PitchBend { bend } => {
                        // midly uses PitchBend with a signed 14-bit value in -8192..=8191.
                        pb_at.insert(step_idx, bend.as_int());
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    // Flush any still-active notes (missing NoteOff).
    for (pitch, (on_tick, on_vel)) in active {
        let s   = (on_tick / ticks_per_step.max(1)) as usize;
        let c1  = cc01_at.get(&s).copied().unwrap_or(0);
        let c74 = cc74_at.get(&s).copied().unwrap_or(0);
        let pb  = pb_at.get(&s).copied().unwrap_or(0);
        events.push((s, pitch, on_vel, 100, 0, c1, c74, pb));
    }
    events.sort_unstable_by_key(|(s, ..)| *s);

    (name, events, ch_hint)
}

fn build_tempo_map(
    tracks: &[Vec<midly::TrackEvent<'_>>],
    format: midly::Format,
) -> Vec<(u32, u32)> {
    let tempo_track_idx = match format {
        midly::Format::SingleTrack => 0,
        _                          => 0, // Track 0 holds tempo in Format 1
    };
    let mut map = vec![(0u32, 500_000u32)]; // default 120 BPM
    if let Some(track) = tracks.get(tempo_track_idx) {
        let mut tick = 0u32;
        for ev in track {
            tick += ev.delta.as_int();
            if let midly::TrackEventKind::Meta(midly::MetaMessage::Tempo(us)) = ev.kind {
                map.push((tick, us.as_int()));
            }
        }
    }
    map
}

/// Extract a time signature map: `Vec<(tick, numerator, denominator)>`.
/// Denominator is stored as a power-of-two exponent per MIDI spec (2^n), so
/// denom=2 means quarter notes, denom=3 means eighth notes, etc.
/// Falls back to 4/4 at tick 0 if no time signature events are found.
pub fn build_time_sig_map(
    tracks: &[Vec<midly::TrackEvent<'_>>],
    format: midly::Format,
) -> Vec<(u32, u8, u8)> {
    let tempo_track_idx = match format {
        midly::Format::SingleTrack => 0,
        _                          => 0,
    };
    let mut map: Vec<(u32, u8, u8)> = Vec::new();
    if let Some(track) = tracks.get(tempo_track_idx) {
        let mut tick = 0u32;
        for ev in track {
            tick += ev.delta.as_int();
            if let midly::TrackEventKind::Meta(midly::MetaMessage::TimeSignature(num, den, _, _)) = ev.kind {
                let real_den = 1u8 << den; // 2^den
                map.push((tick, num, real_den));
            }
        }
    }
    if map.is_empty() {
        map.push((0, 4, 4)); // default 4/4
    }
    map
}

/// Compute a content fingerprint for a pattern's steps (used for deduplication).
fn pattern_fingerprint(pat: &Pattern) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for step in &pat.steps {
        step.note.hash(&mut h);
        step.velocity.hash(&mut h);
        step.gate.hash(&mut h);
        step.micro.hash(&mut h);
        step.cc01.hash(&mut h);
        step.cc74.hash(&mut h);
    }
    h.finish()
}

/// Count total placed clips across all matrix rows.
fn num_pats_total(matrix: &HashMap<String, Vec<Option<Clip>>>) -> usize {
    matrix.values().flat_map(|v| v.iter()).filter(|s| s.is_some()).count()
}

/// Look up which time signature is active at the given absolute tick.
fn time_sig_at_tick(map: &[(u32, u8, u8)], tick: u32) -> (u8, u8) {
    map.iter()
        .filter(|(t, _, _)| *t <= tick)
        .last()
        .map(|(_, n, d)| (*n, *d))
        .unwrap_or((4, 4))
}

fn sanitize_name(s: &str) -> String {
    let upper: String = s
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
        .map(|c| c.to_ascii_uppercase())
        .take(8)
        .collect();
    if upper.is_empty() { "TRK".to_string() } else { upper }
}

// ─── SMF write helpers ────────────────────────────────────────────────────────

fn write_varlen(buf: &mut Vec<u8>, mut n: u32) {
    let mut bytes = [0u8; 4];
    let mut count = 0;
    loop {
        bytes[count] = (n & 0x7F) as u8;
        n >>= 7;
        count += 1;
        if n == 0 { break; }
    }
    for i in (0..count).rev() {
        if i > 0 {
            buf.push(bytes[i] | 0x80);
        } else {
            buf.push(bytes[i]);
        }
    }
}

fn write_midi_header(buf: &mut Vec<u8>, num_tracks: u16, ppq: u16) {
    buf.extend_from_slice(b"MThd");
    buf.extend_from_slice(&6u32.to_be_bytes());
    buf.extend_from_slice(&(if num_tracks == 1 { 0u16 } else { 1u16 }).to_be_bytes());
    buf.extend_from_slice(&num_tracks.to_be_bytes());
    buf.extend_from_slice(&ppq.to_be_bytes());
}

// ─── WAV stub ────────────────────────────────────────────────────────────────

/// Write a silent WAV file as a placeholder for offline render (P2 feature).
// ─── MusicXML export ─────────────────────────────────────────────────────────

/// Export the project as a MusicXML partwise score.
///
/// Each named pattern becomes a `<part>`. Steps are encoded as quarter-note
/// divisions (divisions=4, so one 16th-note step = 1 division).
/// Empty steps become `<rest>` elements.
pub fn export_musicxml(project: &Project, path: &Path) -> Result<()> {
    use std::fmt::Write as FmtWrite;

    const DIVISIONS: u32 = 4; // quarter-note = 4 divisions → 16th-note = 1

    // Returns (step_letter, alter, octave) for a MIDI note number.
    fn midi_to_pitch(midi: u8) -> (&'static str, i32, i32) {
        const STEPS:  &[&str] = &["C","C","D","D","E","F","F","G","G","A","A","B"];
        const ALTERS: &[i32]  = &[ 0,  1,  0,  1,  0,  0,  1,  0,  1,  0,  1,  0];
        let note_idx = (midi % 12) as usize;
        let octave   = (midi as i32 / 12) - 1;
        (STEPS[note_idx], ALTERS[note_idx], octave)
    }

    fn velocity_dynamic(vel: u8) -> &'static str {
        match vel {
            0..=31   => "pp",
            32..=63  => "p",
            64..=79  => "mp",
            80..=95  => "mf",
            96..=111 => "f",
            _        => "ff",
        }
    }

    /// Map a SeqTerm FX command to a MusicXML `<direction>` string (dynamics / wedge).
    /// Returns `None` for commands handled as in-note articulations or empty commands.
    fn fx_to_direction(fx: &str) -> Option<&'static str> {
        match fx {
            "V10" => Some(r#"<direction placement="below"><direction-type><dynamics><pp/></dynamics></direction-type></direction>"#),
            "V20" => Some(r#"<direction placement="below"><direction-type><dynamics><p/></dynamics></direction-type></direction>"#),
            "V40" => Some(r#"<direction placement="below"><direction-type><dynamics><mf/></dynamics></direction-type></direction>"#),
            "V7F" => Some(r#"<direction placement="below"><direction-type><dynamics><ff/></dynamics></direction-type></direction>"#),
            "C01" => Some(r#"<direction placement="below"><direction-type><wedge type="crescendo" number="1"/></direction-type></direction>"#),
            "D10" | "D20" | "D40" => Some(r#"<direction placement="below"><direction-type><wedge type="diminuendo" number="1"/></direction-type></direction>"#),
            _ => None,
        }
    }

    /// Map a SeqTerm FX command to an inline MusicXML articulation element string.
    /// Returns `None` for commands emitted as `<direction>` or for empty commands.
    fn fx_to_articulation(fx: &str) -> Option<&'static str> {
        match fx {
            "S01" => Some("<staccatissimo/>"),
            "S04" | "S08" => Some("<staccato/>"),
            "R01" => Some("<accent/>"),
            "G01" => Some("<other-articulation>glissando</other-articulation>"),
            _ => None,
        }
    }

    // Collect patterns sorted by name for deterministic output.
    let mut pattern_list: Vec<(&String, &Pattern)> =
        project.patterns.iter().collect();
    pattern_list.sort_by_key(|(k, _)| k.as_str());

    let bpm = project.bpm;
    let mut xml = String::new();

    writeln!(xml, r#"<?xml version="1.0" encoding="UTF-8"?>"#).unwrap();
    writeln!(xml, r#"<!DOCTYPE score-partwise PUBLIC "-//Recordare//DTD MusicXML 4.0 Partwise//EN" "http://www.musicxml.org/dtds/partwise.dtd">"#).unwrap();
    writeln!(xml, r#"<score-partwise version="4.0">"#).unwrap();

    // Part list.
    writeln!(xml, "  <part-list>").unwrap();
    for (i, (key, pat)) in pattern_list.iter().enumerate() {
        let pid = format!("P{}", i + 1);
        let name = if pat.name.is_empty() { key.as_str() } else { pat.name.as_str() };
        writeln!(xml, r#"    <score-part id="{pid}"><part-name>{name}</part-name></score-part>"#).unwrap();
    }
    writeln!(xml, "  </part-list>").unwrap();

    // One <part> per pattern.
    for (i, (_, pat)) in pattern_list.iter().enumerate() {
        let pid = format!("P{}", i + 1);
        writeln!(xml, r#"  <part id="{pid}">"#).unwrap();

        // Steps per measure (4/4 at 16th-note grid → 16 steps per bar).
        let steps_per_bar: usize = 16;
        let n_measures = (pat.length + steps_per_bar - 1).max(1) / steps_per_bar;

        for measure in 0..n_measures {
            let mnum = measure + 1;
            writeln!(xml, r#"    <measure number="{mnum}">"#).unwrap();

            // Attributes only in measure 1.
            if measure == 0 {
                write!(xml, r#"      <attributes>
        <divisions>{DIVISIONS}</divisions>
        <key><fifths>0</fifths></key>
        <time><beats>4</beats><beat-type>4</beat-type></time>
        <clef><sign>G</sign><line>2</line></clef>
      </attributes>
      <direction placement="above">
        <direction-type>
          <metronome parentheses="no"><beat-unit>quarter</beat-unit><per-minute>{bpm:.0}</per-minute></metronome>
        </direction-type>
        <sound tempo="{bpm:.0}"/>
      </direction>
"#).unwrap();
            }

            let start_step = measure * steps_per_bar;
            let end_step   = (start_step + steps_per_bar).min(pat.length.max(1));

            for step_idx in start_step..end_step {
                let note = pat.steps.get(step_idx);
                let is_empty = note.map(|n| n.is_empty()).unwrap_or(true);

                if is_empty {
                    writeln!(xml, "      <note><rest/><duration>1</duration><type>16th</type></note>").unwrap();
                } else {
                    let n = note.unwrap();
                    // Emit <direction> elements (dynamics, wedges) derived from fx1/fx2
                    // before the note element so score readers see them first.
                    for fx in [&n.fx1, &n.fx2] {
                        if let Some(dir) = fx_to_direction(fx) {
                            writeln!(xml, "      {dir}").unwrap();
                        }
                    }
                    // Collect inline articulations for <notations>.
                    let articulations: Vec<&'static str> = [&n.fx1, &n.fx2]
                        .iter()
                        .filter_map(|fx| fx_to_articulation(fx))
                        .collect();

                    // Use the primary note via all_note_ons(); fall back to rest if unparseable.
                    let voices = n.all_note_ons();
                    if voices.is_empty() {
                        writeln!(xml, "      <note><rest/><duration>1</duration><type>16th</type></note>").unwrap();
                    } else {
                        let dyn_tag = velocity_dynamic(n.velocity);
                        for (midi_note, _vel) in &voices {
                            let (step_name, alter, octave) = midi_to_pitch(*midi_note);
                            writeln!(xml, "      <note>").unwrap();
                            writeln!(xml, "        <pitch>").unwrap();
                            writeln!(xml, "          <step>{step_name}</step>").unwrap();
                            if alter != 0 {
                                writeln!(xml, "          <alter>{alter}</alter>").unwrap();
                            }
                            writeln!(xml, "          <octave>{octave}</octave>").unwrap();
                            writeln!(xml, "        </pitch>").unwrap();
                            writeln!(xml, "        <duration>1</duration>").unwrap();
                            writeln!(xml, "        <type>16th</type>").unwrap();
                            writeln!(xml, "        <dynamics><{dyn_tag}/></dynamics>").unwrap();
                            if !articulations.is_empty() {
                                writeln!(xml, "        <notations>").unwrap();
                                writeln!(xml, "          <articulations>").unwrap();
                                for art in &articulations {
                                    writeln!(xml, "            {art}").unwrap();
                                }
                                writeln!(xml, "          </articulations>").unwrap();
                                writeln!(xml, "        </notations>").unwrap();
                            }
                            writeln!(xml, "      </note>").unwrap();
                        }
                    }
                }
            }

            // Pad remaining steps if pattern length isn't a multiple of steps_per_bar.
            for _ in end_step..start_step + steps_per_bar {
                writeln!(xml, "      <note><rest/><duration>1</duration><type>16th</type></note>").unwrap();
            }

            writeln!(xml, "    </measure>").unwrap();
        }

        writeln!(xml, "  </part>").unwrap();
    }

    writeln!(xml, "</score-partwise>").unwrap();

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).context("creating directory")?;
    }
    std::fs::write(path, xml.as_bytes())
        .with_context(|| format!("writing MusicXML to {}", path.display()))?;
    info!("MusicXML exported to {}", path.display());
    Ok(())
}

