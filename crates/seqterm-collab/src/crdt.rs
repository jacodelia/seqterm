//! CRDT primitives with Lamport timestamps.
//!
//! Implements two fundamental CRDTs suitable for collaborative editing:
//!
//! - [`LamportClock`] — logical clock for causal ordering of operations
//! - [`GrowOnlySet<T>`] — add-only set (2P-Set requires separate remove tracking)
//! - [`LwwRegister<T>`] — Last-Write-Wins register (most recent timestamp wins)
//! - [`DeltaOp`] — a single CRDT operation that can be sent over the wire

use std::collections::{BTreeMap, HashSet};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ─── Lamport Clock ────────────────────────────────────────────────────────────

/// Logical clock for causal ordering of CRDT operations.
///
/// Each site (peer) maintains its own counter. On every local operation the
/// clock is incremented. On receive, the clock is set to `max(local, remote) + 1`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LamportClock {
    /// Site identifier (unique per peer).
    pub site_id: Uuid,
    /// Current logical time.
    pub time: u64,
}

impl LamportClock {
    pub fn new(site_id: Uuid) -> Self { Self { site_id, time: 0 } }

    /// Increment the clock (before sending an operation).
    pub fn tick(&mut self) -> u64 {
        self.time += 1;
        self.time
    }

    /// Advance the clock on receive (ensures causal ordering).
    pub fn receive(&mut self, remote_time: u64) {
        self.time = self.time.max(remote_time) + 1;
    }
}

// ─── Lamport Timestamp ────────────────────────────────────────────────────────

/// A (time, site_id) pair that defines total order across all peers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LamportTs {
    pub time:    u64,
    pub site_id: Uuid,
}

impl PartialOrd for LamportTs {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for LamportTs {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.time.cmp(&other.time)
            .then_with(|| self.site_id.cmp(&other.site_id))
    }
}

// ─── Grow-Only Set ────────────────────────────────────────────────────────────

/// A grow-only CRDT set.  Elements can be added but never removed.
/// Merge is the set union.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GrowOnlySet<T: Eq + std::hash::Hash + Clone> {
    elements: HashSet<T>,
}

impl<T: Eq + std::hash::Hash + Clone> GrowOnlySet<T> {
    pub fn new() -> Self { Self { elements: HashSet::new() } }

    /// Add an element.
    pub fn add(&mut self, v: T) { self.elements.insert(v); }

    /// Check membership.
    pub fn contains(&self, v: &T) -> bool { self.elements.contains(v) }

    /// Merge with another set (union).
    pub fn merge(&mut self, other: &Self) {
        for e in &other.elements { self.elements.insert(e.clone()); }
    }

    pub fn iter(&self) -> impl Iterator<Item = &T> { self.elements.iter() }
    pub fn len(&self) -> usize { self.elements.len() }
    pub fn is_empty(&self) -> bool { self.elements.is_empty() }
}

impl<T: Eq + std::hash::Hash + Clone> Default for GrowOnlySet<T> {
    fn default() -> Self { Self::new() }
}

// ─── LWW Register ────────────────────────────────────────────────────────────

/// Last-Write-Wins register: holds one value with a Lamport timestamp.
/// On concurrent writes, the higher timestamp wins; ties broken by site_id.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LwwRegister<T: Clone> {
    value: Option<T>,
    ts:    Option<LamportTs>,
}

impl<T: Clone> LwwRegister<T> {
    pub fn new() -> Self { Self { value: None, ts: None } }

    /// Set the value with a new timestamp.
    pub fn set(&mut self, value: T, ts: LamportTs) {
        if self.ts.as_ref().map(|t| &ts > t).unwrap_or(true) {
            self.value = Some(value);
            self.ts    = Some(ts);
        }
    }

    pub fn get(&self) -> Option<&T> { self.value.as_ref() }

    /// Merge with another register (higher timestamp wins).
    pub fn merge(&mut self, other: &Self) {
        if let (Some(v), Some(t)) = (&other.value, &other.ts) {
            self.set(v.clone(), t.clone());
        }
    }
}

impl<T: Clone> Default for LwwRegister<T> { fn default() -> Self { Self::new() } }

// ─── Delta Operations ─────────────────────────────────────────────────────────

/// A single CRDT delta operation that can be serialized and sent over the wire.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeltaOp {
    /// Unique operation ID.
    pub op_id:   Uuid,
    /// Site that originated this operation.
    pub site_id: Uuid,
    /// Lamport timestamp at the time of the operation.
    pub ts:      u64,
    /// The operation payload.
    pub payload: DeltaPayload,
}

/// Payload variants for CRDT operations on the project.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DeltaPayload {
    /// Add a pattern key to the active set.
    PatternAdded { key: String },
    /// Remove a pattern key from the active set.
    PatternRemoved { key: String },
    /// Set a note field in a pattern (LWW).
    NoteSet { pattern_key: String, step: usize, field: String, value: serde_json::Value },
    /// Set the project BPM (LWW).
    BpmSet { bpm: f64 },
    /// Set a channel field (volume, pan, etc.) (LWW).
    ChannelFieldSet { channel: usize, field: String, value: serde_json::Value },
    /// Raw JSON blob for future-proofing.
    Custom { tag: String, data: serde_json::Value },
}

// ─── OpLog ────────────────────────────────────────────────────────────────────

/// Ordered log of CRDT operations.  Used to replay history on reconnect.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpLog {
    ops: BTreeMap<(u64, [u8; 16]), DeltaOp>,
}

impl OpLog {
    pub fn new() -> Self { Self { ops: BTreeMap::new() } }

    /// Append an operation.
    pub fn push(&mut self, op: DeltaOp) {
        let key = (op.ts, *op.site_id.as_bytes());
        self.ops.insert(key, op);
    }

    /// Iterate operations in causal order.
    pub fn iter(&self) -> impl Iterator<Item = &DeltaOp> {
        self.ops.values()
    }

    /// Ops after a given (ts, site_id) — used for catch-up on reconnect.
    pub fn since(&self, ts: u64, site_id: &Uuid) -> Vec<&DeltaOp> {
        let start = (ts, *site_id.as_bytes());
        self.ops.range(start..).map(|(_, op)| op).collect()
    }

    pub fn len(&self) -> usize { self.ops.len() }
    pub fn is_empty(&self) -> bool { self.ops.is_empty() }
}

impl Default for OpLog { fn default() -> Self { Self::new() } }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lamport_clock_ordering() {
        let site = Uuid::new_v4();
        let mut c = LamportClock::new(site);
        assert_eq!(c.tick(), 1);
        assert_eq!(c.tick(), 2);
        c.receive(10);
        assert_eq!(c.time, 11);
        assert_eq!(c.tick(), 12);
    }

    #[test]
    fn grow_only_set_merge() {
        let mut a: GrowOnlySet<u32> = GrowOnlySet::new();
        let mut b: GrowOnlySet<u32> = GrowOnlySet::new();
        a.add(1); a.add(2);
        b.add(2); b.add(3);
        a.merge(&b);
        assert!(a.contains(&1));
        assert!(a.contains(&2));
        assert!(a.contains(&3));
        assert_eq!(a.len(), 3);
    }

    #[test]
    fn lww_register_last_wins() {
        let s1 = Uuid::new_v4();
        let s2 = Uuid::new_v4();
        let mut r: LwwRegister<u32> = LwwRegister::new();
        r.set(1, LamportTs { time: 1, site_id: s1 });
        r.set(2, LamportTs { time: 2, site_id: s2 });
        r.set(3, LamportTs { time: 1, site_id: s1 }); // older, should be ignored
        assert_eq!(*r.get().unwrap(), 2);
    }

    #[test]
    fn oplog_preserves_order() {
        let s = Uuid::new_v4();
        let mut log = OpLog::new();
        for t in [3u64, 1, 2] {
            log.push(DeltaOp {
                op_id: Uuid::new_v4(), site_id: s, ts: t,
                payload: DeltaPayload::BpmSet { bpm: t as f64 },
            });
        }
        let bpms: Vec<f64> = log.iter()
            .filter_map(|op| if let DeltaPayload::BpmSet { bpm } = op.payload { Some(bpm) } else { None })
            .collect();
        assert_eq!(bpms, vec![1.0, 2.0, 3.0]);
    }
}
