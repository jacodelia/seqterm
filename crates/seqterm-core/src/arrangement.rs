//! Rational-time arrangement model (Phase 4 — `02_songUpdate` foundation).
//!
//! Hierarchy: **Arrangement → Tracks → Lanes → Clips**. Every clip carries an
//! exact rational position and length in **beats** (Phase 2 [`RationalTime`]),
//! a stable `id` for selection/undo, and a [`ClipKind`] (Pattern / Audio / Midi).
//!
//! This model is **additive**: it lives alongside the legacy `Project.tracks`
//! bar-block arranger so existing projects keep working, and migrates the old
//! `(start_bar, length_bars, label)` blocks into rational clips losslessly.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::project::{AutomationLane, Track, TrackKind};
use crate::rational::RationalTime;

/// What a clip plays on the timeline.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ClipKind {
    /// References an internal sequencer pattern by key — non-destructive reuse:
    /// many clips can reference the same pattern and edits propagate to all.
    Pattern { pattern_key: String },
    /// An audio file clip.
    Audio {
        path: PathBuf,
        #[serde(default = "default_gain")]
        gain: f32,
    },
    /// A MIDI clip — for now it references a pattern of MIDI events by key
    /// (inline event storage is a later step).
    Midi {
        #[serde(default)]
        pattern_key: Option<String>,
    },
}

fn default_gain() -> f32 {
    1.0
}

impl ClipKind {
    /// Short label for inspectors / debugging.
    pub fn label(&self) -> &'static str {
        match self {
            ClipKind::Pattern { .. } => "PATTERN",
            ClipKind::Audio { .. } => "AUDIO",
            ClipKind::Midi { .. } => "MIDI",
        }
    }

    /// The referenced pattern key, if this clip references one.
    pub fn pattern_key(&self) -> Option<&str> {
        match self {
            ClipKind::Pattern { pattern_key } => Some(pattern_key),
            ClipKind::Midi { pattern_key } => pattern_key.as_deref(),
            ClipKind::Audio { .. } => None,
        }
    }
}

/// A clip placed on a lane: an exact rational `[start, start+length)` span.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Clip {
    /// Stable identity (unique within an [`Arrangement`]) for selection/undo.
    pub id: u64,
    pub name: String,
    pub kind: ClipKind,
    /// Onset on the timeline, in beats from the arrangement origin.
    pub start: RationalTime,
    /// Sounding length in beats (always > 0).
    pub length: RationalTime,
    /// Offset into the source content where playback begins (trim-from-start).
    #[serde(default)]
    pub content_offset: RationalTime,
    /// Loop the source within the clip span when the clip is longer than its source.
    #[serde(default)]
    pub loop_enabled: bool,
    /// Palette color index (UI only).
    #[serde(default)]
    pub color: u8,
    #[serde(default)]
    pub muted: bool,
}

impl Clip {
    pub fn new(id: u64, name: impl Into<String>, kind: ClipKind, start: RationalTime, length: RationalTime) -> Self {
        Self {
            id,
            name: name.into(),
            kind,
            start,
            length,
            content_offset: RationalTime::ZERO,
            loop_enabled: false,
            color: 0,
            muted: false,
        }
    }

    /// End position (`start + length`).
    pub fn end(&self) -> RationalTime {
        self.start + self.length
    }

    /// Whether `t` (beats) lies within `[start, end)`.
    pub fn contains(&self, t: RationalTime) -> bool {
        t >= self.start && t < self.end()
    }

    /// Whether this clip's span overlaps `other`'s (half-open).
    pub fn overlaps(&self, other: &Clip) -> bool {
        self.start < other.end() && other.start < self.end()
    }

    /// Move the clip by `delta` beats (no clamping — caller clamps to ≥ 0).
    pub fn shift(&mut self, delta: RationalTime) {
        self.start = self.start + delta;
    }

    /// Trim the **start** to `new_start` (beats), keeping the END fixed and
    /// advancing `content_offset` so the source stays aligned. Clamped so the
    /// clip keeps a positive length. No-op if `new_start >= end`.
    pub fn trim_start(&mut self, new_start: RationalTime) {
        let end = self.end();
        if new_start >= end || new_start == self.start {
            return;
        }
        let delta = new_start - self.start; // may be negative (extend left)
        // content_offset only advances when we trim inward (delta > 0); extending
        // left past the original onset cannot reveal source before offset 0.
        let new_offset = self.content_offset + delta;
        self.content_offset = if new_offset.is_negative() {
            RationalTime::ZERO
        } else {
            new_offset
        };
        self.start = new_start;
        self.length = end - new_start;
    }

    /// Trim the **end** to `new_end` (beats), keeping the start fixed. Clamped to
    /// a positive length. No-op if `new_end <= start`.
    pub fn trim_end(&mut self, new_end: RationalTime) {
        if new_end <= self.start {
            return;
        }
        self.length = new_end - self.start;
    }

    /// Split the clip at absolute position `t`. On success the clip is truncated
    /// to `[start, t)` and the **right** half `[t, end)` is returned with a fresh
    /// `id` and an advanced `content_offset`. Returns `None` if `t` is not strictly
    /// inside the clip.
    pub fn split_at(&mut self, t: RationalTime, new_id: u64) -> Option<Clip> {
        if !(t > self.start && t < self.end()) {
            return None;
        }
        let left_len = t - self.start;
        let mut right = self.clone();
        right.id = new_id;
        right.start = t;
        right.length = self.length - left_len;
        right.content_offset = self.content_offset + left_len;
        self.length = left_len;
        Some(right)
    }
}

/// A lane within a track — an ordered set of clips. Tracks may hold several lanes
/// (e.g. comp takes); the timeline renders them stacked.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Lane {
    #[serde(default)]
    pub clips: Vec<Clip>,
}

impl Lane {
    /// The clip whose span contains `t`, if any (first match).
    pub fn clip_at(&self, t: RationalTime) -> Option<&Clip> {
        self.clips.iter().find(|c| c.contains(t))
    }

    /// Keep clips sorted by start position (stable for equal starts).
    pub fn sort(&mut self) {
        self.clips.sort_by_key(|c| c.start);
    }
}

/// An arrangement track: a named, typed row of lanes plus inline automation.
/// **No mixer controls** live here (volume/EQ/sends stay in the Mixer view).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArrangementTrack {
    pub name: String,
    #[serde(default)]
    pub kind: TrackKind,
    #[serde(default)]
    pub color: u8,
    #[serde(default)]
    pub mute: bool,
    #[serde(default)]
    pub solo: bool,
    #[serde(default)]
    pub arm: bool,
    #[serde(default)]
    pub monitor: bool,
    #[serde(default = "default_one_lane")]
    pub lanes: Vec<Lane>,
    /// Per-track automation lanes rendered inline in the timeline.
    #[serde(default)]
    pub automation: Vec<AutomationLane>,
    /// Playback routing: the matrix row key (`"A"`..=`"H"`) whose configured
    /// instrument (source / MIDI out / audio slot) this track's clips play
    /// through. `None` = unrouted (silent). Keeping the instrument on the matrix
    /// row means arrangement playback reuses the existing, fully-configured
    /// routing rather than duplicating an instrument model. (Milestone B.)
    #[serde(default)]
    pub source_row: Option<String>,
}

fn default_one_lane() -> Vec<Lane> {
    vec![Lane::default()]
}

impl ArrangementTrack {
    pub fn new(name: impl Into<String>, kind: TrackKind) -> Self {
        Self {
            name: name.into(),
            kind,
            color: 0,
            mute: false,
            solo: false,
            arm: false,
            monitor: false,
            lanes: vec![Lane::default()],
            automation: Vec::new(),
            source_row: None,
        }
    }

    /// The primary (first) lane, created on demand.
    pub fn primary_lane_mut(&mut self) -> &mut Lane {
        if self.lanes.is_empty() {
            self.lanes.push(Lane::default());
        }
        &mut self.lanes[0]
    }
}

/// One active clip returned by [`Arrangement::clips_active_at`].
#[derive(Debug, Clone, PartialEq)]
pub struct ClipHit {
    pub track_idx: usize,
    pub lane_idx: usize,
    pub clip_id: u64,
    /// Referenced pattern key, if the clip plays a pattern/MIDI source.
    pub pattern_key: Option<String>,
    /// Position into the clip's source content at the queried beat (≥ 0).
    /// Consumers loop this against the source length when `loop_enabled`.
    pub local_beat: RationalTime,
}

/// One playable clip hit from [`Arrangement::playback_hits`]: which instrument
/// (matrix row) to play through, which pattern, and where in the source.
#[derive(Debug, Clone, PartialEq)]
pub struct PlaybackHit {
    pub track_idx: usize,
    /// Matrix row key carrying the instrument config to route through.
    pub source_row: String,
    pub clip_id: u64,
    pub pattern_key: String,
    /// Beats into the clip's source content at the queried beat (≥ 0).
    pub local_beat: RationalTime,
}

/// The whole arrangement: timeline tracks on rational time.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Arrangement {
    #[serde(default)]
    pub tracks: Vec<ArrangementTrack>,
    /// Monotonic clip-id allocator (never reused, survives deletes).
    #[serde(default)]
    pub next_clip_id: u64,
}

impl Arrangement {
    /// Allocate a fresh, never-reused clip id.
    pub fn alloc_id(&mut self) -> u64 {
        let id = self.next_clip_id;
        self.next_clip_id += 1;
        id
    }

    /// Whether the arrangement has any clips (used to decide migration).
    pub fn is_empty(&self) -> bool {
        self.tracks.iter().all(|t| t.lanes.iter().all(|l| l.clips.is_empty()))
    }

    /// Find a clip by id across all tracks/lanes (mutable).
    pub fn clip_mut(&mut self, id: u64) -> Option<&mut Clip> {
        for t in &mut self.tracks {
            for l in &mut t.lanes {
                if let Some(c) = l.clips.iter_mut().find(|c| c.id == id) {
                    return Some(c);
                }
            }
        }
        None
    }

    /// Add a clip to `track_idx`'s primary lane, assigning it a fresh id, and
    /// return that id. No-op (returns `None`) if the track index is out of range.
    pub fn add_clip(
        &mut self,
        track_idx: usize,
        name: impl Into<String>,
        kind: ClipKind,
        start: RationalTime,
        length: RationalTime,
    ) -> Option<u64> {
        if track_idx >= self.tracks.len() {
            return None;
        }
        let id = self.alloc_id();
        let clip = Clip::new(id, name, kind, start, length);
        self.tracks[track_idx].primary_lane_mut().clips.push(clip);
        Some(id)
    }

    /// Remove the clip with `id` from wherever it lives. Returns `true` if found.
    pub fn delete_clip(&mut self, id: u64) -> bool {
        for t in &mut self.tracks {
            for l in &mut t.lanes {
                let before = l.clips.len();
                l.clips.retain(|c| c.id != id);
                if l.clips.len() != before {
                    return true;
                }
            }
        }
        false
    }

    /// Duplicate the clip `id`, placing the copy immediately after it on the same
    /// lane (start += its length), with a fresh id. Returns the new id.
    pub fn duplicate_clip(&mut self, id: u64) -> Option<u64> {
        let new_id = self.next_clip_id;
        for t in &mut self.tracks {
            for l in &mut t.lanes {
                if let Some(src) = l.clips.iter().find(|c| c.id == id) {
                    let mut copy = src.clone();
                    copy.id = new_id;
                    copy.start = src.end();
                    l.clips.push(copy);
                    self.next_clip_id += 1;
                    return Some(new_id);
                }
            }
        }
        None
    }

    /// Split the clip `id` at absolute beat `t`; the right half gets a fresh id and
    /// is inserted on the same lane. Returns the new (right) clip id, or `None` if
    /// the clip isn't found or `t` is not strictly inside it.
    pub fn split_clip(&mut self, id: u64, t: RationalTime) -> Option<u64> {
        let new_id = self.next_clip_id;
        for track in &mut self.tracks {
            for lane in &mut track.lanes {
                if let Some(pos) = lane.clips.iter().position(|c| c.id == id) {
                    if let Some(right) = lane.clips[pos].split_at(t, new_id) {
                        lane.clips.insert(pos + 1, right);
                        self.next_clip_id += 1;
                        return Some(new_id);
                    }
                    return None;
                }
            }
        }
        None
    }

    /// Locate a clip by id, returning `(track_idx, lane_idx)` if present.
    pub fn locate_clip(&self, id: u64) -> Option<(usize, usize)> {
        for (ti, t) in self.tracks.iter().enumerate() {
            for (li, l) in t.lanes.iter().enumerate() {
                if l.clips.iter().any(|c| c.id == id) {
                    return Some((ti, li));
                }
            }
        }
        None
    }

    /// Find a clip by id across all tracks/lanes (shared).
    pub fn clip(&self, id: u64) -> Option<&Clip> {
        self.tracks
            .iter()
            .flat_map(|t| t.lanes.iter())
            .flat_map(|l| l.clips.iter())
            .find(|c| c.id == id)
    }

    /// Clip ids on `track_idx` (all lanes) ordered by start position, ties broken
    /// by id for stability. Empty if the track is out of range or has no clips.
    /// This is the order a clip cursor steps through with next/prev.
    pub fn track_clip_ids(&self, track_idx: usize) -> Vec<u64> {
        let Some(track) = self.tracks.get(track_idx) else {
            return Vec::new();
        };
        let mut clips: Vec<(RationalTime, u64)> = track
            .lanes
            .iter()
            .flat_map(|l| l.clips.iter())
            .map(|c| (c.start, c.id))
            .collect();
        clips.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
        clips.into_iter().map(|(_, id)| id).collect()
    }

    /// The first clip (earliest start) on `track_idx`, if any — the landing clip
    /// when a cursor enters a track.
    pub fn first_clip_on_track(&self, track_idx: usize) -> Option<u64> {
        self.track_clip_ids(track_idx).into_iter().next()
    }

    /// The clip on `track_idx` immediately before/after `current_id` in start
    /// order (`forward = true` → next). Returns `None` past the ends so the
    /// caller can choose whether to wrap or stop at the edge. If `current_id`
    /// isn't on the track, returns the first/last clip as an entry point.
    pub fn neighbor_clip(&self, track_idx: usize, current_id: u64, forward: bool) -> Option<u64> {
        let ids = self.track_clip_ids(track_idx);
        match ids.iter().position(|&id| id == current_id) {
            Some(pos) if forward => ids.get(pos + 1).copied(),
            Some(pos) => pos.checked_sub(1).and_then(|p| ids.get(p)).copied(),
            None if forward => ids.first().copied(),
            None => ids.last().copied(),
        }
    }

    /// The id of the clip on `track_idx` (any lane) whose span contains `beat`,
    /// if any — used to select the clip under a timeline beat cursor.
    pub fn clip_at_on_track(&self, track_idx: usize, beat: RationalTime) -> Option<u64> {
        let track = self.tracks.get(track_idx)?;
        track
            .lanes
            .iter()
            .flat_map(|l| l.clips.iter())
            .find(|c| c.contains(beat))
            .map(|c| c.id)
    }

    /// The clip on `track_idx` whose start is nearest to `beat` (any lane) — used
    /// to keep a clip cursor near the playhead/timeline position when switching
    /// tracks. `None` if the track has no clips.
    pub fn nearest_clip_on_track(&self, track_idx: usize, beat: RationalTime) -> Option<u64> {
        let track = self.tracks.get(track_idx)?;
        track
            .lanes
            .iter()
            .flat_map(|l| l.clips.iter())
            .min_by_key(|c| (c.start - beat).abs())
            .map(|c| c.id)
    }

    /// Total arrangement length in beats (end of the last clip), or zero.
    pub fn length_beats(&self) -> RationalTime {
        let mut max = RationalTime::ZERO;
        for t in &self.tracks {
            for l in &t.lanes {
                for c in &l.clips {
                    if c.end() > max {
                        max = c.end();
                    }
                }
            }
        }
        max
    }

    /// Every clip whose span contains the timeline position `beat`, with the
    /// **local source position** (beats into the clip's content) already resolved
    /// from `content_offset`. This is the query both the scheduler (what to play
    /// now) and the timeline UI (what's under the playhead) consume. Muted clips
    /// and muted tracks are skipped.
    pub fn clips_active_at(&self, beat: RationalTime) -> Vec<ClipHit> {
        let mut hits = Vec::new();
        for (ti, track) in self.tracks.iter().enumerate() {
            if track.mute {
                continue;
            }
            for (li, lane) in track.lanes.iter().enumerate() {
                for clip in &lane.clips {
                    if clip.muted || !clip.contains(beat) {
                        continue;
                    }
                    hits.push(ClipHit {
                        track_idx: ti,
                        lane_idx: li,
                        clip_id: clip.id,
                        pattern_key: clip.kind.pattern_key().map(str::to_string),
                        local_beat: clip.content_offset + (beat - clip.start),
                    });
                }
            }
        }
        hits
    }

    /// Resolve everything the scheduler needs to **play** the arrangement at the
    /// timeline position `beat`: for each active clip on a *routed*, non-muted
    /// track that references a pattern, the routing row, pattern key, and the
    /// local source beat. This is the playback counterpart to [`clips_active_at`]
    /// (which is routing-agnostic) and is the bridge the scheduler consumes.
    pub fn playback_hits(&self, beat: RationalTime) -> Vec<PlaybackHit> {
        let mut hits = Vec::new();
        for (ti, track) in self.tracks.iter().enumerate() {
            if track.mute {
                continue;
            }
            let Some(row) = track.source_row.clone() else { continue };
            for lane in &track.lanes {
                for clip in &lane.clips {
                    if clip.muted || !clip.contains(beat) {
                        continue;
                    }
                    let Some(pattern_key) = clip.kind.pattern_key() else { continue };
                    hits.push(PlaybackHit {
                        track_idx: ti,
                        source_row: row.clone(),
                        clip_id: clip.id,
                        pattern_key: pattern_key.to_string(),
                        local_beat: clip.content_offset + (beat - clip.start),
                    });
                }
            }
        }
        hits
    }

    /// Build a rational arrangement from the legacy bar-block `tracks`, converting
    /// each `(start_bar, length_bars, label)` to a `Pattern` clip. `beats_per_bar`
    /// comes from the project time signature (default 4 for 4/4). Lossless:
    /// integer bars map exactly to whole-beat rational positions.
    pub fn from_legacy_tracks(tracks: &[Track], beats_per_bar: i64) -> Self {
        let bpb = beats_per_bar.max(1);
        let mut arr = Arrangement::default();
        for t in tracks {
            let mut track = ArrangementTrack::new(&t.name, TrackKind::Midi);
            track.mute = t.mute;
            let lane = track.primary_lane_mut();
            for (start_bar, length_bars, label) in &t.blocks {
                let id = arr.next_clip_id;
                arr.next_clip_id += 1;
                let start = RationalTime::whole(*start_bar as i64 * bpb);
                let length = RationalTime::whole((*length_bars).max(1) as i64 * bpb);
                lane.clips.push(Clip::new(
                    id,
                    label.clone(),
                    ClipKind::Pattern { pattern_key: label.clone() },
                    start,
                    length,
                ));
            }
            arr.tracks.push(track);
        }
        arr
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(n: i64, d: i64) -> RationalTime {
        RationalTime::new(n, d)
    }

    fn pclip(id: u64, start: RationalTime, len: RationalTime) -> Clip {
        Clip::new(id, "C", ClipKind::Pattern { pattern_key: "P".into() }, start, len)
    }

    #[test]
    fn clip_span_and_contains() {
        let c = pclip(0, r(1, 2), r(2, 1)); // [0.5, 2.5)
        assert_eq!(c.end(), r(5, 2));
        assert!(c.contains(r(1, 1)));
        assert!(c.contains(r(1, 2))); // start inclusive
        assert!(!c.contains(r(5, 2))); // end exclusive
        assert!(!c.contains(RationalTime::ZERO));
    }

    #[test]
    fn clips_overlap() {
        let a = pclip(0, RationalTime::ZERO, r(2, 1)); // [0,2)
        let b = pclip(1, r(1, 1), r(2, 1)); // [1,3)
        let c = pclip(2, r(2, 1), r(1, 1)); // [2,3)
        assert!(a.overlaps(&b));
        assert!(!a.overlaps(&c)); // touch at 2, half-open → no overlap
    }

    #[test]
    fn split_at_divides_span_and_advances_offset() {
        let mut left = pclip(0, r(1, 1), r(4, 1)); // [1,5)
        left.content_offset = r(1, 2); // source started 0.5 in
        let right = left.split_at(r(3, 1), 7).unwrap();
        // Left becomes [1,3), right [3,5).
        assert_eq!(left.start, r(1, 1));
        assert_eq!(left.length, r(2, 1));
        assert_eq!(right.id, 7);
        assert_eq!(right.start, r(3, 1));
        assert_eq!(right.length, r(2, 1));
        // Right's content offset advanced by the left length (2 beats).
        assert_eq!(right.content_offset, r(5, 2));
        // Split outside the span fails.
        assert!(left.split_at(r(10, 1), 8).is_none());
        assert!(left.split_at(r(1, 1), 8).is_none()); // exactly at start
    }

    #[test]
    fn trim_end_changes_length_only() {
        let mut c = pclip(0, r(1, 1), r(4, 1)); // [1,5)
        c.trim_end(r(3, 1));
        assert_eq!(c.length, r(2, 1));
        assert_eq!(c.content_offset, RationalTime::ZERO);
        c.trim_end(r(1, 2)); // before start → no-op
        assert_eq!(c.length, r(2, 1));
    }

    #[test]
    fn trim_start_keeps_end_and_advances_offset() {
        let mut c = pclip(0, r(1, 1), r(4, 1)); // [1,5)
        c.trim_start(r(2, 1)); // → [2,5)
        assert_eq!(c.start, r(2, 1));
        assert_eq!(c.length, r(3, 1));
        assert_eq!(c.content_offset, r(1, 1)); // advanced by 1 beat
        assert_eq!(c.end(), r(5, 1)); // end preserved
    }

    #[test]
    fn lane_clip_at_and_sort() {
        let mut lane = Lane::default();
        lane.clips.push(pclip(0, r(4, 1), r(2, 1)));
        lane.clips.push(pclip(1, RationalTime::ZERO, r(2, 1)));
        lane.sort();
        assert_eq!(lane.clips[0].id, 1); // earlier start first
        assert_eq!(lane.clip_at(r(1, 1)).unwrap().id, 1);
        assert!(lane.clip_at(r(3, 1)).is_none()); // gap between clips
    }

    #[test]
    fn arrangement_id_alloc_is_monotonic() {
        let mut a = Arrangement::default();
        assert_eq!(a.alloc_id(), 0);
        assert_eq!(a.alloc_id(), 1);
        assert_eq!(a.next_clip_id, 2);
    }

    #[test]
    fn arrangement_length_is_last_clip_end() {
        let mut a = Arrangement::default();
        let mut t = ArrangementTrack::new("T", TrackKind::Midi);
        t.primary_lane_mut().clips.push(pclip(0, RationalTime::ZERO, r(4, 1)));
        t.primary_lane_mut().clips.push(pclip(1, r(4, 1), r(3, 1))); // ends at 7
        a.tracks.push(t);
        assert_eq!(a.length_beats(), r(7, 1));
        assert!(!a.is_empty());
    }

    #[test]
    fn migrate_legacy_blocks_to_rational_clips() {
        let mut leg = Track::new("Bass");
        leg.blocks = vec![(0, 2, "BASS1".into()), (4, 1, "BASS2".into())];
        leg.mute = true;
        let arr = Arrangement::from_legacy_tracks(&[leg], 4); // 4 beats/bar

        assert_eq!(arr.tracks.len(), 1);
        let track = &arr.tracks[0];
        assert_eq!(track.name, "Bass");
        assert!(track.mute);
        let clips = &track.lanes[0].clips;
        assert_eq!(clips.len(), 2);
        // bar 0, len 2 bars → start 0, length 8 beats.
        assert_eq!(clips[0].start, RationalTime::ZERO);
        assert_eq!(clips[0].length, r(8, 1));
        assert_eq!(clips[0].kind.pattern_key(), Some("BASS1"));
        // bar 4 → start 16 beats; len 1 bar → 4 beats.
        assert_eq!(clips[1].start, r(16, 1));
        assert_eq!(clips[1].length, r(4, 1));
        // ids are unique and the allocator points past them.
        assert_eq!(clips[0].id, 0);
        assert_eq!(clips[1].id, 1);
        assert_eq!(arr.next_clip_id, 2);
    }

    #[test]
    fn clips_active_at_resolves_local_position() {
        let mut a = Arrangement::default();
        let mut t = ArrangementTrack::new("T", TrackKind::Midi);
        let mut c = pclip(3, r(2, 1), r(4, 1)); // [2,6)
        c.content_offset = r(1, 1); // source starts 1 beat in
        t.primary_lane_mut().clips.push(c);
        a.tracks.push(t);

        // Before the clip: nothing.
        assert!(a.clips_active_at(RationalTime::ZERO).is_empty());
        // At beat 2 (clip start): local = offset 1 + 0 = 1.
        let h = a.clips_active_at(r(2, 1));
        assert_eq!(h.len(), 1);
        assert_eq!(h[0].clip_id, 3);
        assert_eq!(h[0].pattern_key.as_deref(), Some("P"));
        assert_eq!(h[0].local_beat, r(1, 1));
        // At beat 4 (2 beats in): local = 1 + 2 = 3.
        assert_eq!(a.clips_active_at(r(4, 1))[0].local_beat, r(3, 1));
        // At the exclusive end (beat 6): nothing.
        assert!(a.clips_active_at(r(6, 1)).is_empty());
    }

    #[test]
    fn clips_active_at_skips_muted() {
        let mut a = Arrangement::default();
        let mut t = ArrangementTrack::new("T", TrackKind::Midi);
        let mut c = pclip(0, RationalTime::ZERO, r(4, 1));
        c.muted = true;
        t.primary_lane_mut().clips.push(c);
        a.tracks.push(t);
        assert!(a.clips_active_at(r(1, 1)).is_empty(), "muted clip skipped");
        a.tracks[0].lanes[0].clips[0].muted = false;
        a.tracks[0].mute = true;
        assert!(a.clips_active_at(r(1, 1)).is_empty(), "muted track skipped");
    }

    #[test]
    fn clip_mut_finds_by_id() {
        let mut a = Arrangement::default();
        let mut t = ArrangementTrack::new("T", TrackKind::Midi);
        t.primary_lane_mut().clips.push(pclip(5, RationalTime::ZERO, r(2, 1)));
        a.tracks.push(t);
        a.clip_mut(5).unwrap().name = "renamed".into();
        assert_eq!(a.tracks[0].lanes[0].clips[0].name, "renamed");
        assert!(a.clip_mut(99).is_none());
    }

    #[test]
    fn add_delete_clip() {
        let mut a = Arrangement::default();
        a.tracks.push(ArrangementTrack::new("T", TrackKind::Midi));
        let id = a.add_clip(0, "C", ClipKind::Pattern { pattern_key: "P".into() },
            RationalTime::ZERO, r(4, 1)).unwrap();
        assert_eq!(a.tracks[0].lanes[0].clips.len(), 1);
        assert!(a.add_clip(9, "x", ClipKind::Pattern { pattern_key: "P".into() },
            RationalTime::ZERO, r(1, 1)).is_none(), "bad track index");
        assert!(a.delete_clip(id));
        assert!(a.tracks[0].lanes[0].clips.is_empty());
        assert!(!a.delete_clip(id), "already gone");
    }

    #[test]
    fn duplicate_clip_places_after() {
        let mut a = Arrangement::default();
        a.tracks.push(ArrangementTrack::new("T", TrackKind::Midi));
        let id = a.add_clip(0, "C", ClipKind::Pattern { pattern_key: "P".into() },
            r(2, 1), r(4, 1)).unwrap(); // [2,6)
        let dup = a.duplicate_clip(id).unwrap();
        let clips = &a.tracks[0].lanes[0].clips;
        assert_eq!(clips.len(), 2);
        let copy = clips.iter().find(|c| c.id == dup).unwrap();
        assert_eq!(copy.start, r(6, 1)); // right after the original's end
        assert_eq!(copy.length, r(4, 1));
    }

    #[test]
    fn split_clip_inserts_right_half() {
        let mut a = Arrangement::default();
        a.tracks.push(ArrangementTrack::new("T", TrackKind::Midi));
        let id = a.add_clip(0, "C", ClipKind::Pattern { pattern_key: "P".into() },
            r(1, 1), r(4, 1)).unwrap(); // [1,5)
        let right = a.split_clip(id, r(3, 1)).unwrap();
        let clips = &a.tracks[0].lanes[0].clips;
        assert_eq!(clips.len(), 2);
        assert_eq!(clips[0].id, id);
        assert_eq!(clips[0].length, r(2, 1)); // [1,3)
        assert_eq!(clips[1].id, right);
        assert_eq!(clips[1].start, r(3, 1)); // [3,5)
        // Split outside the span fails and adds nothing.
        assert!(a.split_clip(id, r(10, 1)).is_none());
        assert_eq!(a.tracks[0].lanes[0].clips.len(), 2);
    }

    #[test]
    fn track_clip_ids_ordered_by_start() {
        let mut a = Arrangement::default();
        let mut t = ArrangementTrack::new("T", TrackKind::Midi);
        t.primary_lane_mut().clips.push(pclip(2, r(4, 1), r(2, 1)));
        t.primary_lane_mut().clips.push(pclip(0, RationalTime::ZERO, r(2, 1)));
        t.primary_lane_mut().clips.push(pclip(1, r(2, 1), r(2, 1)));
        a.tracks.push(t);
        assert_eq!(a.track_clip_ids(0), vec![0, 1, 2]);
        assert_eq!(a.first_clip_on_track(0), Some(0));
        assert!(a.track_clip_ids(9).is_empty());
    }

    #[test]
    fn neighbor_clip_steps_and_stops_at_edges() {
        let mut a = Arrangement::default();
        let mut t = ArrangementTrack::new("T", TrackKind::Midi);
        for i in 0..3 {
            t.primary_lane_mut()
                .clips
                .push(pclip(i, RationalTime::whole(i as i64 * 2), r(2, 1)));
        }
        a.tracks.push(t);
        assert_eq!(a.neighbor_clip(0, 0, true), Some(1));
        assert_eq!(a.neighbor_clip(0, 1, true), Some(2));
        assert_eq!(a.neighbor_clip(0, 2, true), None); // past the end
        assert_eq!(a.neighbor_clip(0, 1, false), Some(0));
        assert_eq!(a.neighbor_clip(0, 0, false), None); // before the start
        // Unknown current id → entry point (first when forward, last when back).
        assert_eq!(a.neighbor_clip(0, 99, true), Some(0));
        assert_eq!(a.neighbor_clip(0, 99, false), Some(2));
    }

    #[test]
    fn nearest_clip_on_track_picks_closest_start() {
        let mut a = Arrangement::default();
        let mut t = ArrangementTrack::new("T", TrackKind::Midi);
        t.primary_lane_mut().clips.push(pclip(0, RationalTime::ZERO, r(2, 1)));
        t.primary_lane_mut().clips.push(pclip(1, r(8, 1), r(2, 1)));
        a.tracks.push(t);
        assert_eq!(a.nearest_clip_on_track(0, r(1, 1)), Some(0));
        assert_eq!(a.nearest_clip_on_track(0, r(7, 1)), Some(1));
        assert_eq!(a.locate_clip(1), Some((0, 0)));
        assert!(a.nearest_clip_on_track(9, RationalTime::ZERO).is_none());
        // clip_at_on_track: only when the beat falls inside a clip span.
        assert_eq!(a.clip_at_on_track(0, r(1, 1)), Some(0)); // inside [0,2)
        assert!(a.clip_at_on_track(0, r(4, 1)).is_none()); // gap between clips
        assert_eq!(a.clip_at_on_track(0, r(8, 1)), Some(1)); // start of [8,10)
    }

    #[test]
    fn playback_hits_only_for_routed_tracks() {
        let mut a = Arrangement::default();
        // Track 0: routed to row "A".
        let mut t0 = ArrangementTrack::new("Lead", TrackKind::Midi);
        t0.source_row = Some("A".into());
        let mut c = pclip(0, r(2, 1), r(4, 1)); // [2,6)
        c.content_offset = r(1, 1);
        t0.primary_lane_mut().clips.push(c);
        a.tracks.push(t0);
        // Track 1: same clip span but UNROUTED — must not produce hits.
        let mut t1 = ArrangementTrack::new("Pad", TrackKind::Midi);
        t1.primary_lane_mut().clips.push(pclip(1, r(2, 1), r(4, 1)));
        a.tracks.push(t1);

        // Outside any clip: nothing.
        assert!(a.playback_hits(RationalTime::ZERO).is_empty());
        // Inside the routed clip at beat 4: local = offset 1 + (4-2) = 3.
        let hits = a.playback_hits(r(4, 1));
        assert_eq!(hits.len(), 1, "only the routed track plays");
        assert_eq!(hits[0].source_row, "A");
        assert_eq!(hits[0].pattern_key, "P");
        assert_eq!(hits[0].local_beat, r(3, 1));
        // Muting the routed track silences it.
        a.tracks[0].mute = true;
        assert!(a.playback_hits(r(4, 1)).is_empty());
    }

    #[test]
    fn serde_roundtrip() {
        let mut a = Arrangement::default();
        let mut t = ArrangementTrack::new("T", TrackKind::Audio);
        t.primary_lane_mut().clips.push(Clip::new(
            0, "wav",
            ClipKind::Audio { path: "x.wav".into(), gain: 0.5 },
            r(1, 2), r(3, 1),
        ));
        a.tracks.push(t);
        a.next_clip_id = 1;
        let j = serde_json::to_string(&a).unwrap();
        let back: Arrangement = serde_json::from_str(&j).unwrap();
        assert_eq!(a, back);
    }
}
