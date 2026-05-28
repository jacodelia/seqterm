use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use seqterm_core::{Note, Pattern, Clip};

use crate::{
    History, NoteField,
    SetNote, SetPatternLength, SetBpm, SetClipMidiOut, ToggleClipEnabled,
    CreatePattern, DeleteClip, RenamePattern, SetNoteField,
};

/// Serializable mirror of every concrete `EditCommand` variant.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "cmd")]
pub enum SerializedCommand {
    SetNote         { pattern_key: String, step: usize, old: Note, new: Note },
    SetPatternLength{ pattern_key: String, old: usize,  new: usize },
    SetBpm          { old: f64, new: f64 },
    SetClipMidiOut  { row_key: String, col: usize, old: Option<String>, new: Option<String> },
    ToggleClipEnabled{ row_key: String, col: usize },
    CreatePattern   { key: String, pattern: Pattern, row_key: String, col: usize },
    DeleteClip      { row_key: String, col: usize, old_clip: Option<Clip> },
    RenamePattern   { old_key: String, new_key: String },
    SetNoteField    { pattern_key: String, step: usize, field: NoteFieldSer, old: i32, new: i32 },
}

/// Serializable copy of `NoteField` (not re-using the original to keep serde derive clean).
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum NoteFieldSer { Velocity, Gate, Micro, Cc01, Cc74, Prob }

impl From<NoteField> for NoteFieldSer {
    fn from(f: NoteField) -> Self {
        match f {
            NoteField::Velocity => Self::Velocity,
            NoteField::Gate     => Self::Gate,
            NoteField::Micro    => Self::Micro,
            NoteField::Cc01     => Self::Cc01,
            NoteField::Cc74     => Self::Cc74,
            NoteField::Prob     => Self::Prob,
        }
    }
}

impl From<NoteFieldSer> for NoteField {
    fn from(f: NoteFieldSer) -> Self {
        match f {
            NoteFieldSer::Velocity => Self::Velocity,
            NoteFieldSer::Gate     => Self::Gate,
            NoteFieldSer::Micro    => Self::Micro,
            NoteFieldSer::Cc01     => Self::Cc01,
            NoteFieldSer::Cc74     => Self::Cc74,
            NoteFieldSer::Prob     => Self::Prob,
        }
    }
}

/// A snapshot of the undo/redo stacks that can be persisted alongside a project file.
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct SerializedHistory {
    pub past:   Vec<SerializedCommand>,
    pub future: Vec<SerializedCommand>,
}

/// Serialize a `History` into a `SerializedHistory`.
/// Commands that don't have a concrete serializable type (e.g. `GroupedCommands`
/// produced by `begin_group/end_group`) are silently skipped.
pub fn serialize_history(history: &History) -> SerializedHistory {
    SerializedHistory {
        past:   history.past_iter().flat_map(to_serialized).collect(),
        future: history.future_iter().flat_map(to_serialized).collect(),
    }
}

/// Restore a `History` from a `SerializedHistory` (no project mutation needed —
/// the commands are ready to be applied/reverted later).
pub fn deserialize_history(data: SerializedHistory) -> History {
    let past:   Vec<Box<dyn crate::EditCommand>> = data.past
        .into_iter().map(from_serialized).collect();
    let future: Vec<Box<dyn crate::EditCommand>> = data.future
        .into_iter().map(from_serialized).collect();
    History::from_stacks(past, future)
}

/// Write a `SerializedHistory` to `<project_path>.history.json`.
pub fn save_history(history: &History, project_path: &Path) -> Result<()> {
    let data = serialize_history(history);
    let path = history_path(project_path);
    let json = serde_json::to_string_pretty(&data)
        .context("failed to serialize undo history")?;
    std::fs::write(&path, json)
        .with_context(|| format!("failed to write history file: {}", path.display()))?;
    Ok(())
}

/// Load a `SerializedHistory` from `<project_path>.history.json`, returning an
/// empty `History` if the file doesn't exist or can't be parsed.
pub fn load_history(project_path: &Path) -> History {
    let path = history_path(project_path);
    if !path.exists() { return History::default(); }
    std::fs::read_to_string(&path).ok()
        .and_then(|s| serde_json::from_str::<SerializedHistory>(&s).ok())
        .map(deserialize_history)
        .unwrap_or_default()
}

fn history_path(project_path: &Path) -> std::path::PathBuf {
    let mut p = project_path.to_path_buf();
    let new_name = format!(
        "{}.history.json",
        p.file_name().map(|n| n.to_string_lossy()).unwrap_or_default()
    );
    p.set_file_name(new_name);
    p
}

fn to_serialized(cmd: &dyn crate::EditCommand) -> Option<SerializedCommand> {
    let any = cmd.as_any();
    if let Some(c) = any.downcast_ref::<SetNote>() {
        return Some(SerializedCommand::SetNote {
            pattern_key: c.pattern_key.clone(),
            step: c.step,
            old: c.old.clone(),
            new: c.new.clone(),
        });
    }
    if let Some(c) = any.downcast_ref::<SetPatternLength>() {
        return Some(SerializedCommand::SetPatternLength {
            pattern_key: c.pattern_key.clone(),
            old: c.old,
            new: c.new,
        });
    }
    if let Some(c) = any.downcast_ref::<SetBpm>() {
        return Some(SerializedCommand::SetBpm { old: c.old, new: c.new });
    }
    if let Some(c) = any.downcast_ref::<SetClipMidiOut>() {
        return Some(SerializedCommand::SetClipMidiOut {
            row_key: c.row_key.clone(), col: c.col,
            old: c.old.clone(), new: c.new.clone(),
        });
    }
    if let Some(c) = any.downcast_ref::<ToggleClipEnabled>() {
        return Some(SerializedCommand::ToggleClipEnabled {
            row_key: c.row_key.clone(), col: c.col,
        });
    }
    if let Some(c) = any.downcast_ref::<CreatePattern>() {
        return Some(SerializedCommand::CreatePattern {
            key: c.key.clone(), pattern: c.pattern.clone(),
            row_key: c.row_key.clone(), col: c.col,
        });
    }
    if let Some(c) = any.downcast_ref::<DeleteClip>() {
        return Some(SerializedCommand::DeleteClip {
            row_key: c.row_key.clone(), col: c.col,
            old_clip: c.old_clip.clone(),
        });
    }
    if let Some(c) = any.downcast_ref::<RenamePattern>() {
        return Some(SerializedCommand::RenamePattern {
            old_key: c.old_key.clone(), new_key: c.new_key.clone(),
        });
    }
    if let Some(c) = any.downcast_ref::<SetNoteField>() {
        return Some(SerializedCommand::SetNoteField {
            pattern_key: c.pattern_key.clone(),
            step: c.step,
            field: c.field.into(),
            old: c.old,
            new: c.new,
        });
    }
    None // GroupedCommands and unknown types are skipped
}

fn from_serialized(cmd: SerializedCommand) -> Box<dyn crate::EditCommand> {
    match cmd {
        SerializedCommand::SetNote { pattern_key, step, old, new } =>
            Box::new(SetNote { pattern_key, step, old, new }),
        SerializedCommand::SetPatternLength { pattern_key, old, new } =>
            Box::new(SetPatternLength { pattern_key, old, new }),
        SerializedCommand::SetBpm { old, new } =>
            Box::new(SetBpm { old, new }),
        SerializedCommand::SetClipMidiOut { row_key, col, old, new } =>
            Box::new(SetClipMidiOut { row_key, col, old, new }),
        SerializedCommand::ToggleClipEnabled { row_key, col } =>
            Box::new(ToggleClipEnabled { row_key, col }),
        SerializedCommand::CreatePattern { key, pattern, row_key, col } =>
            Box::new(CreatePattern { key, pattern, row_key, col }),
        SerializedCommand::DeleteClip { row_key, col, old_clip } =>
            Box::new(DeleteClip { row_key, col, old_clip }),
        SerializedCommand::RenamePattern { old_key, new_key } =>
            Box::new(RenamePattern { old_key, new_key }),
        SerializedCommand::SetNoteField { pattern_key, step, field, old, new } =>
            Box::new(SetNoteField { pattern_key, step, field: field.into(), old, new }),
    }
}
