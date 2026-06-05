//! Offline audio renderer — produces WAV files without a live CPAL stream.
//!
//! Drives the sequencer logic and Mixer synchronously, block by block.
//! No realtime clock — each step's worth of samples is rendered before advancing.

use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result};
use seqterm_core::{PatternSource, Project};
use tracing::warn;

use crate::{
    assets::AssetCache,
    audio_clip::{AudioClipPlayer, LoopMode},
    mixer::Mixer,
    sf2_synth::SoundFontSynth,
};
use seqterm_ports::realtime::AudioSynthPort;

/// Frames per render block used during offline export.
const OFFLINE_BLOCK_FRAMES: usize = 512;

fn write_samples(
    writer: &mut hound::WavWriter<std::io::BufWriter<std::fs::File>>,
    samples: &[f32],
    bit_depth: u8,
) -> Result<()> {
    match bit_depth {
        24 => {
            for &s in samples {
                writer.write_sample((s.clamp(-1.0, 1.0) * 8_388_607.0) as i32)?;
            }
        }
        32 => {
            for &s in samples {
                writer.write_sample((s.clamp(-1.0, 1.0) * 2_147_483_647.0) as i32)?;
            }
        }
        _ => {
            for &s in samples {
                writer.write_sample((s.clamp(-1.0, 1.0) * 32_767.0) as i16)?;
            }
        }
    }
    Ok(())
}

struct PendingNoteOff {
    slot_id: u32,
    channel: u8,
    note: u8,
    at_step: usize,
}

/// Event collected during the read pass over the project, applied to the mixer afterward.
enum SlotEvent {
    NoteOn { slot_id: u32, channel: u8, note: u8, velocity: u8, gate_steps: usize },
    ClipTrigger { slot_id: u32 },
}

/// Offline renderer: synchronously drives sequencer + Mixer to produce PCM.
pub struct OfflineRenderer {
    project: Project,
    sample_rate: u32,
    bit_depth: u8,
    /// Optional row filter ("A".."H"): when set, only clips in that row are rendered.
    row_filter: Option<String>,
    mixer: Mixer,
    slot_map: HashMap<String, u32>,
    next_slot: u32,
    buf: Vec<f32>,
}

impl OfflineRenderer {
    pub fn new(project: Project, sample_rate: u32, bit_depth: u8, row_filter: Option<String>) -> Self {
        Self {
            project,
            sample_rate,
            bit_depth,
            row_filter,
            mixer: Mixer::new(OFFLINE_BLOCK_FRAMES * 2),
            slot_map: HashMap::new(),
            next_slot: 0,
            buf: vec![0.0f32; OFFLINE_BLOCK_FRAMES * 2],
        }
    }

    /// Load all matching SF2 / AudioFile sources into the mixer (synchronous, non-RT).
    fn load_sources(&mut self) -> Result<()> {
        let cache = AssetCache::new();

        // Snapshot the clip assignments before borrowing self.mixer.
        let clips: Vec<(String, PatternSource)> = self.project.matrix.iter()
            .filter(|(row_key, _)| {
                self.row_filter.as_ref().is_none_or(|f| f == *row_key)
            })
            .flat_map(|(row_key, slots)| {
                slots.iter().enumerate().filter_map(|(col, opt)| {
                    let clip = opt.as_ref().filter(|c| c.enabled)?;
                    let key = format!("{row_key}{col}");
                    Some((key, clip.source.clone()))
                }).collect::<Vec<_>>()
            })
            .collect();

        for (clip_key, source) in clips {
            let slot_id = self.next_slot;
            match source {
                PatternSource::Sf2 { path, bank, preset, .. } => {
                    match cache.load_sf2(&path, bank, preset, self.sample_rate) {
                        Ok(synth) => {
                            self.mixer.set_slot(slot_id as usize, Box::new(synth), 1.0);
                            self.mixer.slots[slot_id as usize].active = false;
                            self.slot_map.insert(clip_key, slot_id);
                            self.next_slot += 1;
                        }
                        Err(e) => warn!("Offline render: SF2 load failed {}: {e}", path.display()),
                    }
                }
                PatternSource::AudioFile { path, looping, .. } => {
                    match cache.get_or_load_audio(&path) {
                        Ok(loaded) => {
                            let mut player = AudioClipPlayer::new(loaded, self.sample_rate);
                            if looping { player.set_loop_mode(LoopMode::Loop); }
                            self.mixer.set_slot(slot_id as usize, Box::new(player), 1.0);
                            self.mixer.slots[slot_id as usize].active = false;
                            self.slot_map.insert(clip_key, slot_id);
                            self.next_slot += 1;
                        }
                        Err(e) => warn!("Offline render: AudioFile load failed {}: {e}", path.display()),
                    }
                }
                // External synth plugins are not rendered offline (audio is stubbed).
                PatternSource::Midi | PatternSource::Plugin { .. } => {}
            }
        }
        Ok(())
    }

    /// Collect events for `global_step` (read-only pass over project + slot_map).
    fn collect_events(&self, global_step: usize) -> Vec<SlotEvent> {
        let mut events = Vec::new();

        for (row_key, slots) in &self.project.matrix {
            if self.row_filter.as_ref().is_some_and(|f| f != row_key) { continue; }
            for (col, clip_opt) in slots.iter().enumerate() {
                let clip = match clip_opt.as_ref().filter(|c| c.enabled) {
                    Some(c) => c,
                    None => continue,
                };
                let pat_key = match &clip.pattern_key {
                    Some(k) => k,
                    None => continue,
                };
                let pat = match self.project.patterns.get(pat_key) {
                    Some(p) => p,
                    None => continue,
                };
                if pat.length == 0 { continue; }

                let pos = global_step % pat.length;
                let note = match pat.steps.get(pos).filter(|n| !n.is_empty()) {
                    Some(n) => n,
                    None => continue,
                };

                let clip_key = format!("{row_key}{col}");
                let slot_id = match self.slot_map.get(&clip_key).copied() {
                    Some(id) => id,
                    None => continue,
                };

                let ch = clip.midi_channel.saturating_sub(1) & 0x0F;

                match &clip.source {
                    PatternSource::AudioFile { .. } => {
                        events.push(SlotEvent::ClipTrigger { slot_id });
                    }
                    PatternSource::Sf2 { .. } => {
                        let gate_steps = (note.gate as usize).div_ceil(100).max(1);
                        for (midi_note, vel) in note.all_note_ons() {
                            events.push(SlotEvent::NoteOn {
                                slot_id, channel: ch, note: midi_note, velocity: vel, gate_steps,
                            });
                        }
                    }
                    PatternSource::Midi | PatternSource::Plugin { .. } => {}
                }
            }
        }
        events
    }

    /// Apply collected events to the mixer and update the pending NoteOff list.
    fn apply_events(&mut self, events: Vec<SlotEvent>, absolute_step: usize, pending: &mut Vec<PendingNoteOff>) {
        for ev in events {
            match ev {
                SlotEvent::NoteOn { slot_id, channel, note, velocity, gate_steps } => {
                    if let Some(slot) = self.mixer.slots.get_mut(slot_id as usize) {
                        slot.active = true;
                        if let Some(src) = slot.source.as_mut()
                            && let Some(synth) = src.as_any_mut().downcast_mut::<SoundFontSynth>()
                        {
                            synth.note_on(channel, note, velocity);
                        }
                        pending.push(PendingNoteOff {
                            slot_id, channel, note, at_step: absolute_step + gate_steps,
                        });
                    }
                }
                SlotEvent::ClipTrigger { slot_id } => {
                    if let Some(slot) = self.mixer.slots.get_mut(slot_id as usize) {
                        slot.active = true;
                        if let Some(src) = slot.source.as_mut()
                            && let Some(player) = src.as_any_mut().downcast_mut::<AudioClipPlayer>()
                        {
                            player.play();
                        }
                    }
                }
            }
        }
    }

    /// Flush any NoteOffs whose step has arrived.
    fn flush_note_offs(&mut self, absolute_step: usize, pending: &mut Vec<PendingNoteOff>) {
        let mut i = 0;
        while i < pending.len() {
            if pending[i].at_step <= absolute_step {
                let noff = pending.swap_remove(i);
                if let Some(slot) = self.mixer.slots.get_mut(noff.slot_id as usize)
                    && let Some(src) = slot.source.as_mut()
                    && let Some(synth) = src.as_any_mut().downcast_mut::<SoundFontSynth>()
                {
                    synth.note_off(noff.channel, noff.note);
                }
            } else {
                i += 1;
            }
        }
    }

    /// Run the full offline render and write the result to `path`.
    ///
    /// `progress(fraction, message)` is called approximately once per bar.
    pub fn render_to_wav<F>(&mut self, path: &Path, progress: F) -> Result<()>
    where
        F: Fn(f32, &str),
    {
        self.load_sources()?;

        let max_bars = self.project.tracks.iter()
            .flat_map(|t| t.blocks.iter())
            .map(|(start, len, _)| start + len)
            .max()
            .unwrap_or(32);

        let bpm = self.project.bpm;
        // Duration of a 16th-note step in samples.
        let step_samples = ((60.0 / (bpm * 4.0)) * self.sample_rate as f64) as usize;
        let total_steps = max_bars as usize * 16;
        let bit_depth = self.bit_depth;
        let sample_rate = self.sample_rate;

        let spec = hound::WavSpec {
            channels: 2,
            sample_rate,
            bits_per_sample: bit_depth as u16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(path, spec)
            .context("creating WAV file")?;

        let mut pending: Vec<PendingNoteOff> = Vec::new();

        for step in 0..total_steps {
            self.flush_note_offs(step, &mut pending);
            let events = self.collect_events(step);
            self.apply_events(events, step, &mut pending);

            // Render this step's worth of audio in OFFLINE_BLOCK_FRAMES chunks.
            let mut remaining = step_samples;
            while remaining > 0 {
                let frames = remaining.min(OFFLINE_BLOCK_FRAMES);
                {
                    let chunk = &mut self.buf[..frames * 2];
                    self.mixer.mix(chunk, sample_rate);
                }
                write_samples(&mut writer, &self.buf[..frames * 2], bit_depth)?;
                remaining -= frames;
            }

            if step % 16 == 0 {
                let bar = step / 16 + 1;
                progress(step as f32 / total_steps as f32, &format!("Bar {bar}/{max_bars}"));
            }
        }

        // Render one extra bar to flush reverb tails.
        for step in 0..16usize {
            self.flush_note_offs(total_steps + step, &mut pending);
            let mut remaining = step_samples;
            while remaining > 0 {
                let frames = remaining.min(OFFLINE_BLOCK_FRAMES);
                {
                    let chunk = &mut self.buf[..frames * 2];
                    self.mixer.mix(chunk, sample_rate);
                }
                write_samples(&mut writer, &self.buf[..frames * 2], bit_depth)?;
                remaining -= frames;
            }
        }

        writer.finalize().context("finalizing WAV")?;
        Ok(())
    }
}

/// Render the entire project to a single stereo WAV mixdown.
///
/// `progress(fraction, message)` is called once per bar.
pub fn render_offline_mixdown<F>(
    project: Project,
    path: &Path,
    sample_rate: u32,
    bit_depth: u8,
    progress: F,
) -> Result<()>
where
    F: Fn(f32, &str),
{
    let mut renderer = OfflineRenderer::new(project, sample_rate, bit_depth, None);
    renderer.render_to_wav(path, progress)
}

/// Render a single row (stem) to a WAV file.
pub fn render_offline_stem<F>(
    project: Project,
    row_key: &str,
    path: &Path,
    sample_rate: u32,
    bit_depth: u8,
    progress: F,
) -> Result<()>
where
    F: Fn(f32, &str),
{
    let mut renderer = OfflineRenderer::new(project, sample_rate, bit_depth, Some(row_key.to_string()));
    renderer.render_to_wav(path, progress)
}
