use seqterm_core::{Clip, Note, Pattern, PatternSource, Project};

pub mod serial;
pub use serial::{save_history, load_history};

const HISTORY_CAP: usize = 200;

// ─── Trait ────────────────────────────────────────────────────────────────────

pub trait EditCommand: std::fmt::Debug + Send {
    fn apply(&self, proj: &mut Project);
    fn revert(&self, proj: &mut Project);
    fn description(&self) -> &str;
    /// For downcasting during serialization.
    fn as_any(&self) -> &dyn std::any::Any;
}

// ─── Grouped transaction ──────────────────────────────────────────────────────

#[derive(Debug)]
pub struct GroupedCommands {
    pub commands: Vec<Box<dyn EditCommand>>,
    pub desc: String,
}

impl EditCommand for GroupedCommands {
    fn apply(&self, proj: &mut Project) {
        for cmd in &self.commands { cmd.apply(proj); }
    }
    fn revert(&self, proj: &mut Project) {
        for cmd in self.commands.iter().rev() { cmd.revert(proj); }
    }
    fn description(&self) -> &str { &self.desc }
    fn as_any(&self) -> &dyn std::any::Any { self }
}

// ─── History ──────────────────────────────────────────────────────────────────

#[derive(Default)]
pub struct History {
    past:   Vec<Box<dyn EditCommand>>,
    future: Vec<Box<dyn EditCommand>>,
    /// Pending commands for the current open transaction (None = no group open).
    pending_group: Option<Vec<Box<dyn EditCommand>>>,
}

impl History {
    pub fn push(&mut self, cmd: Box<dyn EditCommand>, proj: &mut Project) {
        cmd.apply(proj);
        if let Some(group) = &mut self.pending_group {
            group.push(cmd);
        } else {
            self.future.clear();
            self.past.push(cmd);
            if self.past.len() > HISTORY_CAP {
                self.past.remove(0);
            }
        }
    }

    /// Start collecting subsequent `push` calls into a single undo step.
    /// Nesting is not supported — calling begin_group while a group is open is a no-op.
    pub fn begin_group(&mut self) {
        if self.pending_group.is_none() {
            self.pending_group = Some(Vec::new());
        }
    }

    /// Close the current group and commit it as one atomic undo entry.
    /// If the group is empty it is discarded. Does nothing if no group is open.
    pub fn end_group(&mut self, desc: &str) {
        if let Some(commands) = self.pending_group.take() {
            if !commands.is_empty() {
                let group = Box::new(GroupedCommands { commands, desc: desc.to_string() });
                self.future.clear();
                self.past.push(group);
                if self.past.len() > HISTORY_CAP {
                    self.past.remove(0);
                }
            }
        }
    }

    pub fn undo(&mut self, proj: &mut Project) -> Option<&str> {
        let cmd = self.past.pop()?;
        cmd.revert(proj);
        self.future.push(cmd);
        Some(self.future.last().unwrap().description())
    }

    pub fn redo(&mut self, proj: &mut Project) -> Option<&str> {
        let cmd = self.future.pop()?;
        cmd.apply(proj);
        self.past.push(cmd);
        Some(self.past.last().unwrap().description())
    }

    pub fn can_undo(&self) -> bool { !self.past.is_empty() }
    pub fn can_redo(&self) -> bool { !self.future.is_empty() }
    pub fn depth(&self) -> usize  { self.past.len() }

    pub fn clear(&mut self) {
        self.past.clear();
        self.future.clear();
        self.pending_group = None;
    }

    /// Iterate over past commands (oldest first) as trait objects — used for serialization.
    pub fn past_iter(&self) -> impl Iterator<Item = &dyn EditCommand> {
        self.past.iter().map(|c| c.as_ref() as &dyn EditCommand)
    }

    /// Iterate over future commands (most recent pop first) as trait objects.
    pub fn future_iter(&self) -> impl Iterator<Item = &dyn EditCommand> {
        self.future.iter().map(|c| c.as_ref() as &dyn EditCommand)
    }

    /// Record a command that was **already applied** externally.
    /// Unlike [`push`], this does NOT call `apply` — it just adds the command to the undo stack.
    /// Use this when the mutation has already happened (e.g., direct in-place edits).
    pub fn record(&mut self, cmd: Box<dyn EditCommand>) {
        if let Some(group) = &mut self.pending_group {
            group.push(cmd);
        } else {
            self.future.clear();
            self.past.push(cmd);
            if self.past.len() > HISTORY_CAP {
                self.past.remove(0);
            }
        }
    }

    /// Reconstruct a `History` from deserialized stacks (no pending group).
    pub fn from_stacks(
        past:   Vec<Box<dyn EditCommand>>,
        future: Vec<Box<dyn EditCommand>>,
    ) -> Self {
        Self { past, future, pending_group: None }
    }
}

// ─── Concrete commands ────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct SetNote {
    pub pattern_key: String,
    pub step: usize,
    pub old: Note,
    pub new: Note,
}

impl EditCommand for SetNote {
    fn apply(&self, proj: &mut Project) {
        if let Some(pat) = proj.patterns.get_mut(&self.pattern_key) {
            if self.step < pat.steps.len() { pat.steps[self.step] = self.new.clone(); }
        }
    }
    fn revert(&self, proj: &mut Project) {
        if let Some(pat) = proj.patterns.get_mut(&self.pattern_key) {
            if self.step < pat.steps.len() { pat.steps[self.step] = self.old.clone(); }
        }
    }
    fn description(&self) -> &str { "Set note" }
    fn as_any(&self) -> &dyn std::any::Any { self }
}

#[derive(Debug)]
pub struct SetPatternLength {
    pub pattern_key: String,
    pub old: usize,
    pub new: usize,
}

impl EditCommand for SetPatternLength {
    fn apply(&self, proj: &mut Project) {
        if let Some(pat) = proj.patterns.get_mut(&self.pattern_key) {
            pat.length = self.new;
            pat.steps.resize(self.new, Note::default());
        }
    }
    fn revert(&self, proj: &mut Project) {
        if let Some(pat) = proj.patterns.get_mut(&self.pattern_key) {
            pat.length = self.old;
            pat.steps.resize(self.old, Note::default());
        }
    }
    fn description(&self) -> &str { "Set pattern length" }
    fn as_any(&self) -> &dyn std::any::Any { self }
}

#[derive(Debug)]
pub struct SetBpm { pub old: f64, pub new: f64 }
impl EditCommand for SetBpm {
    fn apply(&self, proj: &mut Project)  { proj.bpm = self.new; }
    fn revert(&self, proj: &mut Project) { proj.bpm = self.old; }
    fn description(&self) -> &str { "Set BPM" }
    fn as_any(&self) -> &dyn std::any::Any { self }
}

#[derive(Debug)]
pub struct SetClipMidiOut {
    pub row_key: String,
    pub col: usize,
    pub old: Option<String>,
    pub new: Option<String>,
}

impl EditCommand for SetClipMidiOut {
    fn apply(&self, proj: &mut Project) {
        if let Some(slots) = proj.matrix.get_mut(&self.row_key) {
            if let Some(Some(clip)) = slots.get_mut(self.col) {
                clip.midi_out = self.new.clone();
            }
        }
    }
    fn revert(&self, proj: &mut Project) {
        if let Some(slots) = proj.matrix.get_mut(&self.row_key) {
            if let Some(Some(clip)) = slots.get_mut(self.col) {
                clip.midi_out = self.old.clone();
            }
        }
    }
    fn description(&self) -> &str { "Set MIDI out" }
    fn as_any(&self) -> &dyn std::any::Any { self }
}

#[derive(Debug)]
pub struct ToggleClipEnabled {
    pub row_key: String,
    pub col: usize,
}

impl EditCommand for ToggleClipEnabled {
    fn apply(&self, proj: &mut Project) {
        if let Some(Some(clip)) = proj.matrix.get_mut(&self.row_key)
            .and_then(|s| s.get_mut(self.col)) {
            clip.enabled = !clip.enabled;
        }
    }
    fn revert(&self, proj: &mut Project) { self.apply(proj); }
    fn description(&self) -> &str { "Toggle clip" }
    fn as_any(&self) -> &dyn std::any::Any { self }
}

#[derive(Debug)]
pub struct CreatePattern {
    pub key: String,
    pub pattern: Pattern,
    pub row_key: String,
    pub col: usize,
}

impl EditCommand for CreatePattern {
    fn apply(&self, proj: &mut Project) {
        proj.patterns.insert(self.key.clone(), self.pattern.clone());
        if let Some(slots) = proj.matrix.get_mut(&self.row_key) {
            if self.col < slots.len() {
                let clip = seqterm_core::Clip::new(self.key.clone(), 0, self.col)
                    .with_pattern(&self.key);
                slots[self.col] = Some(clip);
            }
        }
    }
    fn revert(&self, proj: &mut Project) {
        proj.patterns.remove(&self.key);
        if let Some(slots) = proj.matrix.get_mut(&self.row_key) {
            if self.col < slots.len() { slots[self.col] = None; }
        }
    }
    fn description(&self) -> &str { "Create pattern" }
    fn as_any(&self) -> &dyn std::any::Any { self }
}

#[derive(Debug)]
pub struct DeleteClip {
    pub row_key: String,
    pub col: usize,
    pub old_clip: Option<seqterm_core::Clip>,
}

impl EditCommand for DeleteClip {
    fn apply(&self, proj: &mut Project) {
        if let Some(slots) = proj.matrix.get_mut(&self.row_key) {
            if self.col < slots.len() { slots[self.col] = None; }
        }
    }
    fn revert(&self, proj: &mut Project) {
        if let Some(slots) = proj.matrix.get_mut(&self.row_key) {
            if self.col < slots.len() { slots[self.col] = self.old_clip.clone(); }
        }
    }
    fn description(&self) -> &str { "Delete clip" }
    fn as_any(&self) -> &dyn std::any::Any { self }
}

#[derive(Debug)]
pub struct RenamePattern {
    pub old_key: String,
    pub new_key: String,
}

impl EditCommand for RenamePattern {
    fn apply(&self, proj: &mut Project) { rename(proj, &self.old_key, &self.new_key); }
    fn revert(&self, proj: &mut Project) { rename(proj, &self.new_key, &self.old_key); }
    fn description(&self) -> &str { "Rename pattern" }
    fn as_any(&self) -> &dyn std::any::Any { self }
}

fn rename(proj: &mut Project, from: &str, to: &str) {
    if let Some(mut pat) = proj.patterns.remove(from) {
        pat.name = to.to_string();
        proj.patterns.insert(to.to_string(), pat);
    }
    for slots in proj.matrix.values_mut() {
        for slot in slots.iter_mut().flatten() {
            if slot.name == from { slot.name = to.to_string(); }
            if slot.pattern_key.as_deref() == Some(from) {
                slot.pattern_key = Some(to.to_string());
            }
        }
    }
}

#[derive(Debug)]
pub struct SetNoteField {
    pub pattern_key: String,
    pub step: usize,
    pub field: NoteField,
    pub old: i32,
    pub new: i32,
}

#[derive(Debug, Clone, Copy)]
pub enum NoteField { Velocity, Gate, Micro, Cc01, Cc74, Prob }

impl EditCommand for SetNoteField {
    fn apply(&self, proj: &mut Project) {
        apply_field(proj, &self.pattern_key, self.step, self.field, self.new);
    }
    fn revert(&self, proj: &mut Project) {
        apply_field(proj, &self.pattern_key, self.step, self.field, self.old);
    }
    fn description(&self) -> &str { "Edit note field" }
    fn as_any(&self) -> &dyn std::any::Any { self }
}

#[cfg(test)]
mod tests {
    use super::*;
    use seqterm_core::{Note, Pattern, Project};

    fn project_with_pattern() -> Project {
        let mut proj = Project::default();
        let mut pat = Pattern::new("PAT1", 8);
        pat.set_step(0, Note::from_midi(60, 100).unwrap());
        proj.patterns.insert("PAT1".into(), pat);
        proj
    }

    #[test]
    fn push_applies_command() {
        let mut proj = project_with_pattern();
        let mut hist = History::default();
        let old = proj.bpm;
        hist.push(Box::new(SetBpm { old, new: 140.0 }), &mut proj);
        assert!((proj.bpm - 140.0).abs() < 1e-9);
    }

    #[test]
    fn undo_reverts_command() {
        let mut proj = project_with_pattern();
        let mut hist = History::default();
        let old = proj.bpm;
        hist.push(Box::new(SetBpm { old, new: 140.0 }), &mut proj);
        hist.undo(&mut proj);
        assert!((proj.bpm - old).abs() < 1e-9);
    }

    #[test]
    fn redo_reapplies_command() {
        let mut proj = project_with_pattern();
        let mut hist = History::default();
        let old = proj.bpm;
        hist.push(Box::new(SetBpm { old, new: 150.0 }), &mut proj);
        hist.undo(&mut proj);
        hist.redo(&mut proj);
        assert!((proj.bpm - 150.0).abs() < 1e-9);
    }

    #[test]
    fn undo_empty_is_noop() {
        let mut proj = project_with_pattern();
        let mut hist = History::default();
        let result = hist.undo(&mut proj);
        assert!(result.is_none());
    }

    #[test]
    fn redo_empty_is_noop() {
        let mut proj = project_with_pattern();
        let mut hist = History::default();
        let result = hist.redo(&mut proj);
        assert!(result.is_none());
    }

    #[test]
    fn push_clears_redo_stack() {
        let mut proj = project_with_pattern();
        let mut hist = History::default();
        hist.push(Box::new(SetBpm { old: proj.bpm, new: 120.0 }), &mut proj);
        hist.undo(&mut proj);
        assert!(hist.can_redo());
        hist.push(Box::new(SetBpm { old: proj.bpm, new: 180.0 }), &mut proj);
        // Redo stack should be cleared by the new push.
        assert!(!hist.can_redo());
    }

    #[test]
    fn set_note_apply_revert() {
        let mut proj = project_with_pattern();
        let mut hist = History::default();
        let old_note = Note::default();
        let new_note = Note::from_midi(72, 80).unwrap();
        hist.push(Box::new(SetNote {
            pattern_key: "PAT1".into(),
            step: 1,
            old: old_note.clone(),
            new: new_note.clone(),
        }), &mut proj);
        assert_eq!(proj.patterns["PAT1"].steps[1].to_midi(), Some(72));
        hist.undo(&mut proj);
        assert!(proj.patterns["PAT1"].steps[1].is_empty());
    }

    #[test]
    fn group_transaction_undone_atomically() {
        let mut proj = project_with_pattern();
        let mut hist = History::default();
        let bpm_before = proj.bpm;
        hist.begin_group();
        hist.push(Box::new(SetBpm { old: proj.bpm, new: 90.0 }), &mut proj);
        hist.push(Box::new(SetBpm { old: proj.bpm, new: 95.0 }), &mut proj);
        hist.end_group("double bpm change");
        // Both changes applied.
        assert!((proj.bpm - 95.0).abs() < 1e-9);
        // Single undo reverts both.
        hist.undo(&mut proj);
        assert!((proj.bpm - bpm_before).abs() < 1e-9);
    }

    #[test]
    fn depth_tracks_history_size() {
        let mut proj = project_with_pattern();
        let mut hist = History::default();
        assert_eq!(hist.depth(), 0);
        hist.push(Box::new(SetBpm { old: proj.bpm, new: 100.0 }), &mut proj);
        assert_eq!(hist.depth(), 1);
        hist.push(Box::new(SetBpm { old: proj.bpm, new: 110.0 }), &mut proj);
        assert_eq!(hist.depth(), 2);
        hist.undo(&mut proj);
        assert_eq!(hist.depth(), 1);
    }

    #[test]
    fn record_without_apply_then_undo() {
        let mut proj = project_with_pattern();
        let mut hist = History::default();
        let old_bpm = proj.bpm;
        // Externally apply the change.
        proj.bpm = 180.0;
        // Record it WITHOUT applying again.
        hist.record(Box::new(SetBpm { old: old_bpm, new: 180.0 }));
        assert!((proj.bpm - 180.0).abs() < 1e-9, "record should not double-apply");
        // Undo should revert.
        hist.undo(&mut proj);
        assert!((proj.bpm - old_bpm).abs() < 1e-9);
    }

    #[test]
    fn set_clip_source_apply_revert() {
        use seqterm_core::{Clip, PatternSource};
        let mut proj = project_with_pattern();
        let clip = Clip::new("PAT1", 0, 0).with_pattern("PAT1");
        proj.matrix.entry("A".into()).or_default()[0] = Some(clip);

        let mut hist = History::default();
        let sf2_source = PatternSource::Sf2 {
            path: std::path::PathBuf::from("/tmp/test.sf2"),
            bank: 0, preset: 0,
            preset_name: "Piano".into(),
        };
        hist.push(Box::new(SetClipSource {
            row_key: "A".into(),
            col: 0,
            old: PatternSource::Midi,
            new: sf2_source.clone(),
            clip_existed: true,
        }), &mut proj);
        assert!(matches!(
            proj.matrix["A"][0].as_ref().unwrap().source,
            PatternSource::Sf2 { .. }
        ));
        hist.undo(&mut proj);
        assert!(matches!(
            proj.matrix["A"][0].as_ref().unwrap().source,
            PatternSource::Midi
        ));
    }

    #[test]
    fn set_clip_source_creates_clip_on_empty_cell() {
        use seqterm_core::PatternSource;
        let mut proj = project_with_pattern();
        // Ensure A[0] is empty.
        proj.matrix.entry("A".into()).or_default()[0] = None;

        let mut hist = History::default();
        let sf2 = PatternSource::Sf2 {
            path: std::path::PathBuf::from("/tmp/x.sf2"),
            bank: 0, preset: 0, preset_name: "P".into(),
        };
        hist.push(Box::new(SetClipSource {
            row_key: "A".into(), col: 0,
            old: PatternSource::Midi, new: sf2,
            clip_existed: false,
        }), &mut proj);
        // A clip was materialised carrying the SF2 source.
        assert!(matches!(
            proj.matrix["A"][0].as_ref().unwrap().source,
            PatternSource::Sf2 { .. }
        ));
        // Undo removes the clip we created.
        hist.undo(&mut proj);
        assert!(proj.matrix["A"][0].is_none());
    }

    #[test]
    fn swap_clips_apply_revert() {
        use seqterm_core::Clip;
        let mut proj = project_with_pattern();
        let clip_a = Some(Clip::new("PAT1", 0, 0).with_pattern("PAT1"));
        // Explicitly set A[0] to our test clip and B[0] to empty.
        proj.matrix.entry("A".into()).or_default()[0] = clip_a;
        proj.matrix.entry("B".into()).or_default()[0] = None;

        let mut hist = History::default();
        hist.push(Box::new(SwapClips {
            from_key: "A".into(), from_col: 0,
            to_key: "B".into(),   to_col: 0,
        }), &mut proj);
        assert!(proj.matrix["A"][0].is_none());
        assert!(proj.matrix["B"][0].is_some());
        hist.undo(&mut proj);
        assert!(proj.matrix["A"][0].is_some());
        assert!(proj.matrix["B"][0].is_none());
    }

    #[test]
    fn set_channel_param_volume_apply_revert() {
        use seqterm_core::Channel;
        let mut proj = project_with_pattern();
        let mut ch = Channel::new("SYNTH");
        ch.midi_port = Some("SYNTH".to_string());
        ch.volume = 0.0;
        proj.channels.push(ch);

        let mut hist = History::default();
        hist.push(Box::new(SetChannelParam {
            channel_port: "SYNTH".into(),
            param: ChannelParam::Volume,
            old: 0,
            new: 30,  // 3.0 dB
        }), &mut proj);
        let vol = proj.channels.iter().find(|c| c.midi_port.as_deref() == Some("SYNTH")).unwrap().volume;
        assert!((vol - 3.0).abs() < 1e-4);
        hist.undo(&mut proj);
        let vol = proj.channels.iter().find(|c| c.midi_port.as_deref() == Some("SYNTH")).unwrap().volume;
        assert!(vol.abs() < 1e-4);
    }
}

// ─── ProjectSnapshot (universal undo) ─────────────────────────────────────────

/// A whole-`Project` before/after snapshot. This is the universal undo command:
/// any edit gesture — however many fields it touches — can be made undoable by
/// capturing the project state before and after and recording one of these.
/// Used by `App::record_edit` to cover edits that don't have a bespoke typed
/// command. Derived/live state (audio slots, FX mirrors, engine) is rebuilt from
/// the project after undo/redo by the UI's resync step.
#[derive(Debug)]
pub struct ProjectSnapshot {
    pub desc: String,
    pub before: Project,
    pub after: Project,
}

impl EditCommand for ProjectSnapshot {
    fn apply(&self, proj: &mut Project) { *proj = self.after.clone(); }
    fn revert(&self, proj: &mut Project) { *proj = self.before.clone(); }
    fn description(&self) -> &str { &self.desc }
    fn as_any(&self) -> &dyn std::any::Any { self }
}

// ─── SetSf2Instrument ─────────────────────────────────────────────────────────

/// Replace the edited SF2 instrument stored under `key` (`"{path}|{bank}|{preset}"`).
/// Captures the previous value so EDITOR zone edits are undoable. `None` means
/// "no edit stored yet" (revert removes the entry).
#[derive(Debug)]
pub struct SetSf2Instrument {
    pub key: String,
    pub old: Option<seqterm_core::Sf2Instrument>,
    pub new: seqterm_core::Sf2Instrument,
}

impl EditCommand for SetSf2Instrument {
    fn apply(&self, proj: &mut Project) {
        proj.sf2_edits.insert(self.key.clone(), self.new.clone());
    }
    fn revert(&self, proj: &mut Project) {
        match &self.old {
            Some(prev) => { proj.sf2_edits.insert(self.key.clone(), prev.clone()); }
            None => { proj.sf2_edits.remove(&self.key); }
        }
    }
    fn description(&self) -> &str { "Edit SF2 instrument" }
    fn as_any(&self) -> &dyn std::any::Any { self }
}

// ─── SetClipSource ────────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct SetClipSource {
    pub row_key: String,
    pub col: usize,
    pub old: PatternSource,
    pub new: PatternSource,
    /// Whether a clip already existed in the cell when this command was built.
    /// When it did not, `apply` creates an empty clip to carry the new source and
    /// `revert` removes it again, so assigning a source to an empty matrix cell
    /// (e.g. CHANGE SOURCE → SF2) works and undoes cleanly.
    #[doc(hidden)]
    pub clip_existed: bool,
}

impl SetClipSource {
    /// Row index (0-based) derived from the `A`–`P` row key.
    fn row_index(&self) -> usize {
        self.row_key
            .chars()
            .next()
            .map(|c| (c as u8).wrapping_sub(b'A') as usize)
            .unwrap_or(0)
    }
}

impl EditCommand for SetClipSource {
    fn apply(&self, proj: &mut Project) {
        let (row, col) = (self.row_index(), self.col);
        if let Some(slots) = proj.matrix.get_mut(&self.row_key) {
            match slots.get_mut(col) {
                Some(Some(clip)) => clip.source = self.new.clone(),
                // Empty cell: materialise a clip so the source actually sticks.
                Some(slot @ None) => {
                    let mut clip = Clip::new("", row, col);
                    clip.source = self.new.clone();
                    *slot = Some(clip);
                }
                None => {}
            }
        }
    }
    fn revert(&self, proj: &mut Project) {
        if let Some(slots) = proj.matrix.get_mut(&self.row_key) {
            if let Some(slot) = slots.get_mut(self.col) {
                if self.clip_existed {
                    if let Some(clip) = slot {
                        clip.source = self.old.clone();
                    }
                } else {
                    // We created the clip in `apply`; remove it on undo.
                    *slot = None;
                }
            }
        }
    }
    fn description(&self) -> &str { "Set clip source" }
    fn as_any(&self) -> &dyn std::any::Any { self }
}

// ─── SwapClips ────────────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct SwapClips {
    pub from_key: String,
    pub from_col: usize,
    pub to_key: String,
    pub to_col: usize,
}

impl EditCommand for SwapClips {
    fn apply(&self, proj: &mut Project) { swap_clips(proj, &self.from_key, self.from_col, &self.to_key, self.to_col); }
    fn revert(&self, proj: &mut Project) { swap_clips(proj, &self.to_key, self.to_col, &self.from_key, self.from_col); }
    fn description(&self) -> &str { "Move clip" }
    fn as_any(&self) -> &dyn std::any::Any { self }
}

fn swap_clips(proj: &mut Project, from_key: &str, from_col: usize, to_key: &str, to_col: usize) {
    let from_clip = proj.matrix
        .get_mut(from_key)
        .and_then(|s| s.get_mut(from_col))
        .map(|slot| slot.take())
        .flatten();
    if let Some(clip) = from_clip {
        let dest_slot = proj.matrix
            .entry(to_key.to_string())
            .or_default()
            .get_mut(to_col);
        if let Some(dest) = dest_slot {
            let old_dest = dest.take();
            *dest = Some(clip);
            if let Some(src) = proj.matrix.get_mut(from_key).and_then(|s| s.get_mut(from_col)) {
                *src = old_dest;
            }
        } else if let Some(src) = proj.matrix.get_mut(from_key).and_then(|s| s.get_mut(from_col)) {
            *src = Some(clip);
        }
    }
}

// ─── SetChannelParam ─────────────────────────────────────────────────────────

/// Which mixer channel parameter is being changed.
#[derive(Debug, Clone, Copy)]
pub enum ChannelParam { Volume, Pan, EqLow, EqLowMid, EqHighMid, EqHigh, FxAmount }

#[derive(Debug)]
pub struct SetChannelParam {
    /// MIDI port name used as channel identifier.
    pub channel_port: String,
    pub param: ChannelParam,
    /// Stored as i32 (millidB * 10 for volume, centipan for pan, etc.).
    pub old: i32,
    pub new: i32,
}

impl EditCommand for SetChannelParam {
    fn apply(&self, proj: &mut Project) {
        apply_channel_param(proj, &self.channel_port, self.param, self.new);
    }
    fn revert(&self, proj: &mut Project) {
        apply_channel_param(proj, &self.channel_port, self.param, self.old);
    }
    fn description(&self) -> &str { "Adjust channel" }
    fn as_any(&self) -> &dyn std::any::Any { self }
}

fn apply_channel_param(proj: &mut Project, port: &str, param: ChannelParam, val: i32) {
    if let Some(ch) = proj.channels.iter_mut().find(|c| c.midi_port.as_deref() == Some(port)) {
        match param {
            ChannelParam::Volume    => ch.volume      = (val as f32) / 10.0,
            ChannelParam::Pan       => ch.pan = seqterm_core::channel::Pan::from_val(val.clamp(-50, 50) as i8),
            ChannelParam::EqLow     => ch.eq_low      = val.clamp(-12, 12) as i8,
            ChannelParam::EqLowMid  => ch.eq_low_mid  = val.clamp(-12, 12) as i8,
            ChannelParam::EqHighMid => ch.eq_high_mid = val.clamp(-12, 12) as i8,
            ChannelParam::EqHigh    => ch.eq_high     = val.clamp(-12, 12) as i8,
            ChannelParam::FxAmount  => ch.fx_amount   = val.clamp(0, 127) as u8,
        }
    }
}

fn apply_field(proj: &mut Project, key: &str, step: usize, field: NoteField, val: i32) {
    if let Some(pat) = proj.patterns.get_mut(key) {
        if let Some(n) = pat.steps.get_mut(step) {
            match field {
                NoteField::Velocity => n.velocity = val.clamp(0, 127) as u8,
                NoteField::Gate     => n.gate = val.clamp(10, 400) as u16,
                NoteField::Micro    => n.micro = val.clamp(-99, 99) as i8,
                NoteField::Cc01     => n.cc01 = val.clamp(0, 127) as u8,
                NoteField::Cc74     => n.cc74 = val.clamp(0, 127) as u8,
                NoteField::Prob     => n.prob = val.clamp(0, 100) as u8,
            }
        }
    }
}
