//! Object-level three-way merge for SeqTerm project objects.
//!
//! Uses UUID identity to track objects across versions.  Given a common
//! ancestor (`base`) and two divergent versions (`ours`, `theirs`), produces
//! a merged result by applying non-conflicting changes from both sides and
//! flagging conflicts for manual resolution.
//!
//! ## Algorithm
//!
//! For each UUID-identified object:
//! 1. If only one side changed — take that side's version.
//! 2. If both sides changed identically — take either.
//! 3. If both sides changed differently — record a `MergeConflict`.
//! 4. If one side deleted and the other modified — record a conflict.

use std::collections::{HashMap, HashSet};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use serde_json::Value;

// ─── Object snapshot ──────────────────────────────────────────────────────────

/// A snapshot of a single named object, serialized to JSON for diffing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ObjectSnapshot {
    pub id:      Uuid,
    pub kind:    String,
    pub payload: Value,
}

impl ObjectSnapshot {
    pub fn new(id: Uuid, kind: impl Into<String>, payload: Value) -> Self {
        Self { id, kind: kind.into(), payload }
    }
}

// ─── Merge result ─────────────────────────────────────────────────────────────

/// Outcome of merging two divergent object sets.
#[derive(Debug, Clone)]
pub struct MergeResult {
    /// Objects that were successfully merged.
    pub merged:    Vec<ObjectSnapshot>,
    /// Objects that could not be automatically merged (require user action).
    pub conflicts: Vec<MergeConflict>,
}

/// A merge conflict for a single object.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MergeConflict {
    pub id:     Uuid,
    pub kind:   String,
    pub base:   Option<Value>,
    pub ours:   Option<Value>,
    pub theirs: Option<Value>,
    pub reason: ConflictReason,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ConflictReason {
    /// Both sides modified the object with different values.
    BothModified,
    /// One side deleted the object while the other modified it.
    DeleteModifyConflict,
}

// ─── Three-way merge ──────────────────────────────────────────────────────────

/// Perform a UUID-based three-way merge.
///
/// `base` is the common ancestor snapshot.  `ours` and `theirs` are the
/// two divergent heads.  Returns a `MergeResult` containing successfully
/// merged objects and any conflicts.
pub fn three_way_merge(
    base:   &[ObjectSnapshot],
    ours:   &[ObjectSnapshot],
    theirs: &[ObjectSnapshot],
) -> MergeResult {
    let base_map:   HashMap<Uuid, &ObjectSnapshot> = base.iter().map(|o| (o.id, o)).collect();
    let ours_map:   HashMap<Uuid, &ObjectSnapshot> = ours.iter().map(|o| (o.id, o)).collect();
    let theirs_map: HashMap<Uuid, &ObjectSnapshot> = theirs.iter().map(|o| (o.id, o)).collect();

    let all_ids: HashSet<Uuid> = base_map.keys()
        .chain(ours_map.keys())
        .chain(theirs_map.keys())
        .copied()
        .collect();

    let mut merged    = Vec::new();
    let mut conflicts = Vec::new();

    for id in all_ids {
        let b = base_map.get(&id).copied();
        let o = ours_map.get(&id).copied();
        let t = theirs_map.get(&id).copied();

        match (b, o, t) {
            // Both have the same object — no change.
            (_, Some(o), Some(t)) if o.payload == t.payload => {
                merged.push(o.clone());
            }
            // Only ours changed (or added) — take ours.
            (b, Some(o), Some(t)) if b.map(|b| b.payload == t.payload).unwrap_or(false) => {
                merged.push(o.clone());
            }
            // Only theirs changed (or added) — take theirs.
            (b, Some(o), Some(t)) if b.map(|b| b.payload == o.payload).unwrap_or(false) => {
                merged.push(t.clone());
            }
            // Object added by ours only.
            (None, Some(o), None) => merged.push(o.clone()),
            // Object added by theirs only.
            (None, None, Some(t)) => merged.push(t.clone()),
            // Object deleted by both — omit it.
            (Some(_), None, None) => {}
            // Object deleted by ours, unchanged in theirs — omit (ours wins delete).
            (Some(b), None, Some(t)) if b.payload == t.payload => {}
            // Object deleted by theirs, unchanged in ours — omit (theirs wins delete).
            (Some(b), Some(o), None) if b.payload == o.payload => {}
            // Conflict: both modified differently.
            (_, Some(o), Some(t)) => {
                conflicts.push(MergeConflict {
                    id,
                    kind:   o.kind.clone(),
                    base:   b.map(|b| b.payload.clone()),
                    ours:   Some(o.payload.clone()),
                    theirs: Some(t.payload.clone()),
                    reason: ConflictReason::BothModified,
                });
            }
            // Conflict: one deleted, one modified.
            (Some(b), None, Some(t)) => {
                conflicts.push(MergeConflict {
                    id,
                    kind:   b.kind.clone(),
                    base:   Some(b.payload.clone()),
                    ours:   None,
                    theirs: Some(t.payload.clone()),
                    reason: ConflictReason::DeleteModifyConflict,
                });
            }
            (Some(b), Some(o), None) => {
                conflicts.push(MergeConflict {
                    id,
                    kind:   b.kind.clone(),
                    base:   Some(b.payload.clone()),
                    ours:   Some(o.payload.clone()),
                    theirs: None,
                    reason: ConflictReason::DeleteModifyConflict,
                });
            }
            _ => {}
        }
    }

    MergeResult { merged, conflicts }
}

/// Apply a resolved conflict by choosing one side.
pub fn resolve_conflict(conflict: &MergeConflict, take_ours: bool) -> Option<ObjectSnapshot> {
    let payload = if take_ours { conflict.ours.clone() } else { conflict.theirs.clone() };
    payload.map(|p| ObjectSnapshot {
        id: conflict.id,
        kind: conflict.kind.clone(),
        payload: p,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn obj(id: Uuid, kind: &str, val: Value) -> ObjectSnapshot {
        ObjectSnapshot::new(id, kind, val)
    }

    #[test]
    fn no_change_returns_same() {
        let id = Uuid::new_v4();
        let snap = obj(id, "pattern", json!({"length": 16}));
        let r = three_way_merge(&[snap.clone()], &[snap.clone()], &[snap.clone()]);
        assert_eq!(r.merged.len(), 1);
        assert!(r.conflicts.is_empty());
    }

    #[test]
    fn one_side_changed() {
        let id = Uuid::new_v4();
        let base  = obj(id, "bpm", json!(120));
        let ours  = obj(id, "bpm", json!(140)); // we changed it
        let their = obj(id, "bpm", json!(120)); // they didn't
        let r = three_way_merge(&[base], &[ours.clone()], &[their]);
        assert_eq!(r.merged[0].payload, json!(140));
        assert!(r.conflicts.is_empty());
    }

    #[test]
    fn both_changed_differently_is_conflict() {
        let id = Uuid::new_v4();
        let base  = obj(id, "bpm", json!(120));
        let ours  = obj(id, "bpm", json!(140));
        let their = obj(id, "bpm", json!(160));
        let r = three_way_merge(&[base], &[ours], &[their]);
        assert!(r.merged.is_empty());
        assert_eq!(r.conflicts.len(), 1);
        assert!(matches!(r.conflicts[0].reason, ConflictReason::BothModified));
    }

    #[test]
    fn added_by_one_side() {
        let id = Uuid::new_v4();
        let new_obj = obj(id, "pattern", json!({"length": 8}));
        let r = three_way_merge(&[], &[new_obj.clone()], &[]);
        assert_eq!(r.merged.len(), 1);
        assert!(r.conflicts.is_empty());
    }

    #[test]
    fn deleted_by_both() {
        let id = Uuid::new_v4();
        let base = obj(id, "pattern", json!({"length": 8}));
        let r = three_way_merge(&[base], &[], &[]);
        assert!(r.merged.is_empty());
        assert!(r.conflicts.is_empty());
    }

    #[test]
    fn delete_modify_conflict() {
        let id = Uuid::new_v4();
        let base  = obj(id, "pattern", json!({"length": 8}));
        let ours  = obj(id, "pattern", json!({"length": 16})); // we modified
        let r = three_way_merge(&[base], &[ours], &[]); // they deleted
        assert!(r.merged.is_empty());
        assert_eq!(r.conflicts.len(), 1);
        assert!(matches!(r.conflicts[0].reason, ConflictReason::DeleteModifyConflict));
    }
}
