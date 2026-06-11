//! Real CLAP hosting via the safe [`clack_host`] crate.
//!
//! Replaces the earlier hand-rolled C-ABI FFI: `clack-host` provides a safe,
//! correct CLAP host (entry loading, factory enumeration, instancing and the
//! audio/event marshaling), so we no longer maintain unsafe `#[repr(C)]` ABI
//! structs ourselves. Used for both discovery ([`read_descriptors`]) and live
//! instrument playback ([`ClapAudioInstance`]).

#![cfg(feature = "clap")]

use std::ffi::CString;
use std::path::Path;

use clack_host::events::event_types::{
    MidiEvent, NoteExpressionEvent, NoteExpressionType, NoteOffEvent, NoteOnEvent,
};
use clack_host::prelude::*;
use clack_extensions::state::PluginState;

use crate::ClapPluginInfo;

// ── Host handler (no host extensions; all callbacks are no-ops) ─────────────

struct SeqShared;
impl<'a> SharedHandler<'a> for SeqShared {
    fn request_restart(&self) {}
    fn request_process(&self) {}
    fn request_callback(&self) {}
}

struct SeqHost;
impl HostHandlers for SeqHost {
    type Shared<'a> = SeqShared;
    type MainThread<'a> = ();
    type AudioProcessor<'a> = ();
}

fn host_info() -> HostInfo {
    HostInfo::new("SeqTerm", "SeqTerm", "https://github.com/jacodelia/seqterm", "0.1.0")
        .expect("static host info has no interior nul")
}

// ── Discovery ───────────────────────────────────────────────────────────────

/// Enumerate every plugin a `.clap` file exposes, with real metadata. Returns an
/// empty vec on any failure so a directory scan never aborts on one bad file.
pub fn read_descriptors(path: &Path) -> Vec<ClapPluginInfo> {
    // SAFETY: loading an external library is inherently unsafe; clack handles the
    // ABI. We only read descriptors and drop the entry immediately after.
    let entry = match unsafe { PluginEntry::load(path) } {
        Ok(e) => e,
        Err(e) => { tracing::debug!("CLAP load {} failed: {e}", path.display()); return Vec::new(); }
    };
    let Some(factory) = entry.get_plugin_factory() else { return Vec::new() };

    let mut out = Vec::new();
    for desc in factory.plugin_descriptors() {
        let id = desc.id().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        if id.is_empty() { continue; }
        let (mut is_instrument, mut is_effect) = (false, false);
        for f in desc.features() {
            match f.to_string_lossy().as_ref() {
                "instrument" => is_instrument = true,
                "audio-effect" | "note-effect" => is_effect = true,
                _ => {}
            }
        }
        out.push(ClapPluginInfo {
            path: path.to_path_buf(),
            name: desc.name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(),
            vendor: desc.vendor().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(),
            id,
            version: desc.version().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(),
            is_instrument,
            is_effect: is_effect || !is_instrument,
        });
    }
    out
}

// ── Live instrument ─────────────────────────────────────────────────────────

/// An input event queued from the audio thread, flushed each `render`. Notes use
/// CLAP's typed note ports (with a unique `note_id` so the plugin can address
/// individual voices). Per-note expression (tuning/brightness) is delivered as
/// `NoteExpressionEvent`s targeting that `note_id` — this is the polyphonic
/// expression path. Plain channel CCs still go through raw MIDI 1.0 (`MidiEvent`).
///
/// `channel`/`key`/`note_id` are stored raw (`-1` = wildcard / `Match::All`) and
/// rebuilt into a [`Pckn`] in `render` via [`Pckn::from_raw`].
enum QueuedEvent {
    NoteOn { channel: i16, key: i16, note_id: i32, velocity: f64 },
    NoteOff { channel: i16, key: i16, note_id: i32 },
    Expr { channel: i16, key: i16, note_id: i32, ty: NoteExpressionType, value: f64 },
    Midi([u8; 3]),
}

const CHANNELS: usize = 2;

/// Default per-note pitch-bend range in semitones (the MPE specification default).
const DEFAULT_MPE_BEND_SEMITONES: f64 = 48.0;

/// Convert a signed 14-bit MIDI pitch-bend (`-8192..=8191`, 0 = centre) into a
/// tuning offset in semitones given a per-note bend `range`.
fn bend_to_semitones(value: i16, range: f64) -> f64 {
    (value as f64 / 8192.0) * range
}

/// Tracks sounding voices so each gets a unique CLAP `note_id` and per-channel
/// expression can be fanned out to the right voices. Pure (no clack types), so
/// it is unit-tested without a real plugin. Linear-scanned (voice counts small)
/// to avoid allocation on the audio thread.
#[derive(Default)]
struct NoteRegistry {
    next: u32,
    active: Vec<((u8, u8), u32)>,
}

impl NoteRegistry {
    /// Allocate a fresh note id for `(channel, key)` and record it as sounding.
    fn alloc(&mut self, ch: u8, key: u8) -> u32 {
        let id = self.next;
        self.next = self.next.wrapping_add(1);
        self.active.push(((ch, key), id));
        id
    }

    /// Remove and return the note id for `(channel, key)`, if sounding.
    fn take(&mut self, ch: u8, key: u8) -> Option<u32> {
        self.active.iter().position(|((c, k), _)| *c == ch && *k == key)
            .map(|pos| self.active.swap_remove(pos).1)
    }

    fn clear(&mut self) { self.active.clear(); }
}

/// A live CLAP instrument: holds the (main-thread) [`PluginInstance`] and its
/// started audio processor, rendering interleaved stereo and accepting note
/// events. See `unsafe impl Send` below for the threading rationale.
pub struct ClapAudioInstance {
    // Field order matters for drop: processor before instance.
    processor: Option<StartedPluginAudioProcessor<SeqHost>>,
    instance: PluginInstance<SeqHost>,
    in_ports: AudioPorts,
    out_ports: AudioPorts,
    in_buf: Vec<Vec<f32>>,
    out_buf: Vec<Vec<f32>>,
    queue: Vec<QueuedEvent>,
    steady: u64,
    max_frames: u32,
    active: bool,
    /// Sounding-voice registry: assigns unique note ids and maps channel→voices.
    notes: NoteRegistry,
    /// Whether per-note expression is enabled (MPE). When off, pitch-bend and
    /// timbre (CC74) are sent as plain channel MIDI instead of note expression.
    mpe: bool,
    /// Per-note pitch-bend range in semitones used to map bend → tuning.
    mpe_bend_semitones: f64,
}

// SAFETY: a `PluginInstance` is `!Send` because CLAP main-thread callbacks must
// run on its creating thread. We never invoke main-thread methods after handing
// the instance to the audio thread — only the (Send) audio processor's `process`
// and our own note queue, all on that single audio thread. The instance is kept
// alive purely to own the processor and tear it down on drop. This mirrors the
// engine's other instrument sources (LV2) which make the same single-thread
// hand-off assertion.
unsafe impl Send for ClapAudioInstance {}

impl ClapAudioInstance {
    pub fn build(path: &Path, plugin_id: &str, sample_rate: u32, max_block: u32) -> Option<Self> {
        let max_block = max_block.max(1);
        // SAFETY: external library load; clack handles the ABI.
        let entry = unsafe { PluginEntry::load(path) }.ok()?;
        let id = CString::new(plugin_id).ok()?;
        let info = host_info();

        let mut instance = PluginInstance::<SeqHost>::new(
            |_| SeqShared,
            |_| (),
            &entry,
            id.as_c_str(),
            &info,
        ).ok()?;

        let config = PluginAudioConfiguration {
            sample_rate: sample_rate as f64,
            min_frames_count: 1,
            max_frames_count: max_block,
        };
        let stopped = instance.activate(|_, _| (), config).ok()?;
        let started = stopped.start_processing().ok()?;

        Some(Self {
            processor: Some(started),
            instance,
            in_ports: AudioPorts::with_capacity(CHANNELS, 1),
            out_ports: AudioPorts::with_capacity(CHANNELS, 1),
            in_buf: vec![vec![0.0; max_block as usize]; CHANNELS],
            out_buf: vec![vec![0.0; max_block as usize]; CHANNELS],
            queue: Vec::with_capacity(64),
            steady: 0,
            max_frames: max_block,
            active: true,
            notes: NoteRegistry::default(),
            mpe: false,
            mpe_bend_semitones: DEFAULT_MPE_BEND_SEMITONES,
        })
    }

    /// Enable/disable polyphonic (MPE) expression and set the per-note pitch-bend
    /// range in semitones. When enabled, channel pitch-bend and CC74 (timbre) are
    /// translated into per-note tuning/brightness expression for the voices on
    /// that channel (one note per MPE member channel).
    pub fn set_mpe(&mut self, enabled: bool, bend_semitones: f64) {
        self.mpe = enabled;
        if bend_semitones > 0.0 { self.mpe_bend_semitones = bend_semitones; }
    }

    pub fn note_on(&mut self, channel: u8, note: u8, velocity: u8) {
        let ch = channel & 0x0F;
        let id = self.notes.alloc(ch, note);
        self.queue.push(QueuedEvent::NoteOn {
            channel: ch as i16, key: note as i16, note_id: id as i32, velocity: velocity as f64 / 127.0,
        });
    }
    pub fn note_off(&mut self, channel: u8, note: u8) {
        let ch = channel & 0x0F;
        let note_id = self.notes.take(ch, note).map(|id| id as i32).unwrap_or(-1);
        self.queue.push(QueuedEvent::NoteOff { channel: ch as i16, key: note as i16, note_id });
    }
    pub fn all_notes_off(&mut self) {
        self.notes.clear();
        // Wildcard note-off (all ports/channels/keys/ids) …
        self.queue.push(QueuedEvent::NoteOff { channel: -1, key: -1, note_id: -1 });
        // … plus CC 123 per channel for plugins that only honor the MIDI form.
        for ch in 0..16u8 {
            self.queue.push(QueuedEvent::Midi([0xB0 | ch, 123, 0]));
        }
    }

    /// Push a per-note expression for every voice currently sounding on `ch`.
    /// Returns true if at least one voice was targeted.
    fn push_expr_for_channel(&mut self, ch: u8, ty: NoteExpressionType, value: f64) -> bool {
        let mut any = false;
        for i in 0..self.notes.active.len() {
            let ((c, k), id) = self.notes.active[i];
            if c == ch {
                self.queue.push(QueuedEvent::Expr {
                    channel: ch as i16, key: k as i16, note_id: id as i32, ty, value,
                });
                any = true;
            }
        }
        any
    }

    /// Queue a MIDI Control Change. In MPE mode, CC74 (timbre) becomes a
    /// per-note `Brightness` expression for the channel's voices.
    pub fn control_change(&mut self, channel: u8, cc: u8, value: u8) {
        let ch = channel & 0x0F;
        if self.mpe && cc == 74
            && self.push_expr_for_channel(ch, NoteExpressionType::Brightness, value as f64 / 127.0)
        {
            return;
        }
        self.queue.push(QueuedEvent::Midi([0xB0 | ch, cc & 0x7F, value & 0x7F]));
    }

    /// Queue a channel pressure (aftertouch). In MPE mode this becomes per-note
    /// `Pressure` expression for the channel's voices; otherwise a raw MIDI 1.0
    /// channel pressure message.
    pub fn channel_pressure(&mut self, channel: u8, value: u8) {
        let ch = channel & 0x0F;
        if self.mpe && self.push_expr_for_channel(ch, NoteExpressionType::Pressure, value as f64 / 127.0) {
            return;
        }
        self.queue.push(QueuedEvent::Midi([0xD0 | ch, value & 0x7F, 0]));
    }

    /// Queue a channel pitch-bend. `value` is the signed 14-bit bend
    /// (-8192..=8191, 0 = centre). In MPE mode this becomes per-note `Tuning`
    /// expression (in semitones) for the channel's voices; otherwise a raw MIDI
    /// 1.0 channel pitch-bend.
    pub fn pitch_bend(&mut self, channel: u8, value: i16) {
        let ch = channel & 0x0F;
        if self.mpe {
            let semis = bend_to_semitones(value, self.mpe_bend_semitones);
            if self.push_expr_for_channel(ch, NoteExpressionType::Tuning, semis) {
                return;
            }
        }
        let bend = (value as i32 + 8192).clamp(0, 16383) as u16;
        let lsb = (bend & 0x7F) as u8;
        let msb = ((bend >> 7) & 0x7F) as u8;
        self.queue.push(QueuedEvent::Midi([0xE0 | ch, lsb, msb]));
    }

    pub fn is_active(&self) -> bool { self.active }
    pub fn stop(&mut self) { self.active = false; }

    /// Serialize the plugin's opaque state via the CLAP `state` extension.
    /// Returns `None` if the plugin doesn't support state or the save fails.
    /// Main-thread operation.
    pub fn save_state(&mut self) -> Option<Vec<u8>> {
        let state: PluginState = self.instance.plugin_shared_handle().get_extension()?;
        let mut buf = Vec::new();
        state.save(&mut self.instance.plugin_handle(), &mut buf).ok()?;
        Some(buf)
    }

    /// Restore the plugin's opaque state from bytes produced by [`Self::save_state`]
    /// (CLAP `state` extension). Returns `true` on success. Main-thread operation.
    pub fn load_state(&mut self, bytes: &[u8]) -> bool {
        let Some(state): Option<PluginState> = self.instance.plugin_shared_handle().get_extension() else {
            return false;
        };
        let mut reader = bytes;
        state.load(&mut self.instance.plugin_handle(), &mut reader).is_ok()
    }

    /// Render one interleaved-stereo block; returns frames written.
    pub fn render(&mut self, output: &mut [f32]) -> usize {
        let frames = (output.len() / 2).min(self.max_frames as usize);
        for s in output.iter_mut() { *s = 0.0; }
        if frames == 0 { return output.len() / 2; }

        let Self { processor, in_ports, out_ports, in_buf, out_buf, queue, steady, .. } = self;
        let Some(proc) = processor.as_mut() else { return frames };

        // Clear scratch buffers.
        for ch in in_buf.iter_mut() { for v in ch[..frames].iter_mut() { *v = 0.0; } }
        for ch in out_buf.iter_mut() { for v in ch[..frames].iter_mut() { *v = 0.0; } }

        // Build the input event list from the queued events. Notes (with their
        // unique ids) and per-note expression use typed events; plain channel CCs
        // use raw MIDI 1.0.
        let mut in_ev = EventBuffer::new();
        for q in queue.iter() {
            match q {
                QueuedEvent::NoteOn { channel, key, note_id, velocity } => {
                    let pckn = Pckn::from_raw(0, *channel, *key, *note_id);
                    in_ev.push(&NoteOnEvent::new(0, pckn, *velocity));
                }
                QueuedEvent::NoteOff { channel, key, note_id } => {
                    let pckn = Pckn::from_raw(0, *channel, *key, *note_id);
                    in_ev.push(&NoteOffEvent::new(0, pckn, 0.0));
                }
                QueuedEvent::Expr { channel, key, note_id, ty, value } => {
                    let pckn = Pckn::from_raw(0, *channel, *key, *note_id);
                    in_ev.push(&NoteExpressionEvent::new(0, pckn, *ty, *value));
                }
                QueuedEvent::Midi(data) => {
                    in_ev.push(&MidiEvent::new(0, 0, *data));
                }
            }
        }
        let input_events = InputEvents::from(&in_ev);
        let mut out_ev = EventBuffer::new();
        let mut output_events = OutputEvents::from(&mut out_ev);

        let input_audio = in_ports.with_input_buffers([AudioPortBuffer {
            latency: 0,
            channels: AudioPortBufferType::f32_input_only(
                in_buf.iter_mut().map(|b| InputChannel::constant(b)),
            ),
        }]);
        let mut output_audio = out_ports.with_output_buffers([AudioPortBuffer {
            latency: 0,
            channels: AudioPortBufferType::f32_output_only(
                out_buf.iter_mut().map(|b| b.as_mut_slice()),
            ),
        }]);

        let _ = proc.process(
            &input_audio,
            &mut output_audio,
            &input_events,
            &mut output_events,
            Some(*steady),
            None,
        );
        *steady += frames as u64;
        queue.clear();

        // Interleave planar stereo → output.
        for i in 0..frames {
            output[i * 2] = out_buf[0][i];
            output[i * 2 + 1] = out_buf[1][i];
        }
        frames
    }
}

impl Drop for ClapAudioInstance {
    fn drop(&mut self) {
        // Stop processing and deactivate on the main-thread instance so clack
        // tears the plugin down cleanly.
        if let Some(started) = self.processor.take() {
            let stopped = started.stop_processing();
            self.instance.deactivate(stopped);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn note_registry_unique_ids_and_lookup() {
        let mut r = NoteRegistry::default();
        let a = r.alloc(0, 60);
        let b = r.alloc(0, 64); // same channel, different key → distinct voice
        let c = r.alloc(1, 60); // same key, different channel → distinct voice
        assert_ne!(a, b);
        assert_ne!(a, c);
        assert_ne!(b, c);
        // take returns the matching id and removes only that voice.
        assert_eq!(r.take(0, 64), Some(b));
        assert_eq!(r.take(0, 64), None);
        assert_eq!(r.take(0, 60), Some(a));
        assert_eq!(r.take(1, 60), Some(c));
        assert!(r.active.is_empty());
    }

    #[test]
    fn channel_voices_are_isolated() {
        // MPE: one note per member channel — expression on a channel targets only
        // that channel's voice(s).
        let mut r = NoteRegistry::default();
        r.alloc(1, 60);
        r.alloc(2, 67);
        let on_ch1: Vec<_> = r.active.iter().filter(|((c, _), _)| *c == 1).collect();
        assert_eq!(on_ch1.len(), 1);
        assert_eq!((on_ch1[0].0).1, 60);
    }

    #[test]
    fn bend_to_semitones_maps_range() {
        // Full positive bend → +range; full negative → -range; centre → 0.
        assert!((bend_to_semitones(8191, 48.0) - 48.0).abs() < 0.01);
        assert!((bend_to_semitones(-8192, 48.0) + 48.0).abs() < 0.01);
        assert_eq!(bend_to_semitones(0, 48.0), 0.0);
        // Half bend with a ±2 range → +1 semitone.
        assert!((bend_to_semitones(4096, 2.0) - 1.0).abs() < 0.01);
    }
}
