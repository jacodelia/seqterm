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
    built_synth::BuiltinSynth,
    mixer::Mixer,
};
use seqterm_ports::realtime::AudioSource;
#[allow(unused_imports)]
use seqterm_ports::realtime::AudioSynthPort;

/// Frames per render block used during offline export.
const OFFLINE_BLOCK_FRAMES: usize = 512;

/// Factory that instantiates a standalone plugin audio source (e.g. an LV2
/// instrument) for a given plugin id. Supplied by the caller (which owns the
/// plugin registry) so the offline renderer can host real plugins on export.
/// Returns `None` if the plugin can't be instantiated as a sounding source.
pub type PluginSourceFactory<'a> =
    Box<dyn FnMut(&str, u32, u32) -> Option<Box<dyn AudioSource>> + 'a>;

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
pub struct OfflineRenderer<'a> {
    project: Project,
    sample_rate: u32,
    bit_depth: u8,
    /// Optional row filter ("A".."H"): when set, only clips in that row are rendered.
    row_filter: Option<String>,
    mixer: Mixer,
    slot_map: HashMap<String, u32>,
    next_slot: u32,
    buf: Vec<f32>,
    /// Optional plugin instrument factory (hosts real LV2/VST instruments on export).
    plugin_factory: Option<PluginSourceFactory<'a>>,
    /// Display names of plugin clips that could not be hosted offline and fell
    /// back to the built-in synth (surfaced to the user after the render).
    fallback_plugins: Vec<String>,
}

impl<'a> OfflineRenderer<'a> {
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
            plugin_factory: None,
            fallback_plugins: Vec::new(),
        }
    }

    /// Attach a plugin instrument factory so assigned LV2/VST plugins are
    /// rendered with their real sound (rather than falling back to the built-in
    /// synth) during export.
    pub fn with_plugin_factory(mut self, factory: PluginSourceFactory<'a>) -> Self {
        self.plugin_factory = Some(factory);
        self
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
                    // If the user edited this preset in the EDITOR, render it with
                    // SeqTerm's own sampler (the edited zones) so the export matches
                    // live playback. Otherwise use fluidsynth.
                    let edit_key = format!("{}|{}|{}", path.display(), bank, preset);
                    let edited = self.project.sf2_edits.get(&edit_key).cloned();
                    let installed: Option<Box<dyn AudioSource>> = if let Some(inst) = edited {
                        match crate::load_sf2_instrument(&path, bank, preset) {
                            Ok(mut loaded) => {
                                loaded.instrument = inst;
                                Some(Box::new(crate::Sf2Sampler::new(loaded)))
                            }
                            Err(e) => {
                                warn!("Offline render: edited SF2 load failed {}: {e}", path.display());
                                None
                            }
                        }
                    } else {
                        match cache.load_sf2(&path, bank, preset, self.sample_rate) {
                            Ok(synth) => Some(Box::new(synth)),
                            Err(e) => { warn!("Offline render: SF2 load failed {}: {e}", path.display()); None }
                        }
                    };
                    if let Some(source) = installed {
                        self.mixer.set_slot(slot_id as usize, source, 1.0);
                        self.mixer.slots[slot_id as usize].active = false;
                        self.slot_map.insert(clip_key, slot_id);
                        self.next_slot += 1;
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
                // Assigned plugin (LV2/VST): honour the real plugin sound if a
                // factory can instantiate it; otherwise fall back to the built-in
                // synth so the pattern still sounds in the export.
                PatternSource::Plugin { id, name, .. } => {
                    let block = OFFLINE_BLOCK_FRAMES as u32;
                    let source: Box<dyn AudioSource> = match self.plugin_factory
                        .as_mut()
                        .and_then(|f| f(&id, self.sample_rate, block))
                    {
                        Some(src) => src,
                        None => {
                            // Plugin couldn't be hosted offline → built-in synth.
                            let label = if name.is_empty() { id.clone() } else { name.clone() };
                            if !self.fallback_plugins.contains(&label) {
                                self.fallback_plugins.push(label);
                            }
                            Box::new(BuiltinSynth::new())
                        }
                    };
                    self.mixer.set_slot(slot_id as usize, source, 1.0);
                    self.mixer.slots[slot_id as usize].active = false;
                    self.slot_map.insert(clip_key, slot_id);
                    self.next_slot += 1;
                }
                // Pure MIDI patterns have no assigned instrument → render with the
                // built-in internal synth so they still sound in the export.
                PatternSource::Midi => {
                    self.mixer.set_slot(slot_id as usize, Box::new(BuiltinSynth::new()), 1.0);
                    self.mixer.slots[slot_id as usize].active = false;
                    self.slot_map.insert(clip_key, slot_id);
                    self.next_slot += 1;
                }
            }
        }
        Ok(())
    }

    /// Apply the project's persisted mixer FX chains to the loaded slots and the
    /// master bus, so the export sounds like the live mixer ("everything through
    /// the mixer"). Per-slot inserts are keyed by clip_key; the master chain is
    /// applied only for full mixdowns (not per-row stems, which stay pre-master).
    fn apply_fx_chains(&mut self) {
        let sr = self.sample_rate;
        for (clip_key, specs) in &self.project.slot_fx {
            if let Some(&slot_id) = self.slot_map.get(clip_key) {
                let chain = crate::fx_chain::build_chain_from_specs(specs, sr);
                if let Some(slot) = self.mixer.slots.get_mut(slot_id as usize) {
                    slot.fx_chain = chain;
                }
            }
        }
        if self.row_filter.is_none() && !self.project.master_fx.is_empty() {
            self.mixer.master_fx = crate::fx_chain::build_chain_from_specs(&self.project.master_fx, sr);
        }
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
                    // SF2, plus MIDI / Plugin patterns (which render through the
                    // built-in synth installed in load_sources) are all note-driven.
                    PatternSource::Sf2 { .. }
                    | PatternSource::Midi
                    | PatternSource::Plugin { .. } => {
                        let gate_steps = (note.gate as usize).div_ceil(100).max(1);
                        for (midi_note, vel) in note.all_note_ons() {
                            events.push(SlotEvent::NoteOn {
                                slot_id, channel: ch, note: midi_note, velocity: vel, gate_steps,
                            });
                        }
                    }
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
                            && let Some(synth) = src.as_synth()
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
                    && let Some(synth) = src.as_synth()
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
    /// Returns the display names of any assigned plugins that could not be
    /// hosted offline and fell back to the built-in synth.
    pub fn render_to_wav<F>(&mut self, path: &Path, progress: F) -> Result<Vec<String>>
    where
        F: Fn(f32, &str),
    {
        self.load_sources()?;
        self.apply_fx_chains();

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
        Ok(std::mem::take(&mut self.fallback_plugins))
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
    render_offline_mixdown_with(project, path, sample_rate, bit_depth, None, progress).map(|_| ())
}

/// Like [`render_offline_mixdown`], but with an optional plugin instrument
/// factory so assigned LV2/VST plugins are rendered with their real sound.
/// Returns the names of any plugins that fell back to the built-in synth.
pub fn render_offline_mixdown_with<F>(
    project: Project,
    path: &Path,
    sample_rate: u32,
    bit_depth: u8,
    plugin_factory: Option<PluginSourceFactory<'_>>,
    progress: F,
) -> Result<Vec<String>>
where
    F: Fn(f32, &str),
{
    let mut renderer = OfflineRenderer::new(project, sample_rate, bit_depth, None);
    if let Some(f) = plugin_factory { renderer = renderer.with_plugin_factory(f); }
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
    render_offline_stem_with(project, row_key, path, sample_rate, bit_depth, None, progress).map(|_| ())
}

/// Like [`render_offline_stem`], but with an optional plugin instrument factory.
/// Returns the names of any plugins that fell back to the built-in synth.
pub fn render_offline_stem_with<F>(
    project: Project,
    row_key: &str,
    path: &Path,
    sample_rate: u32,
    bit_depth: u8,
    plugin_factory: Option<PluginSourceFactory<'_>>,
    progress: F,
) -> Result<Vec<String>>
where
    F: Fn(f32, &str),
{
    let mut renderer = OfflineRenderer::new(project, sample_rate, bit_depth, Some(row_key.to_string()));
    if let Some(f) = plugin_factory { renderer = renderer.with_plugin_factory(f); }
    renderer.render_to_wav(path, progress)
}
