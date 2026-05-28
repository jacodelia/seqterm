//! MIDI Polyphonic Expression (MPE) support.
//!
//! MPE uses a dedicated MIDI channel per active note so that pitch bend,
//! pressure, and timbre (CC 74) can be applied per-note instead of per-channel.
//!
//! Reference: MIDI Association MPE specification (2018).

use serde::{Deserialize, Serialize};

/// Which zone the MPE configuration applies to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MpeZoneKind {
    /// Lower zone: master channel 1 (index 0), member channels 2–N.
    Lower,
    /// Upper zone: master channel 16 (index 15), member channels 15–N (counting down).
    Upper,
}

impl Default for MpeZoneKind {
    fn default() -> Self { Self::Lower }
}

/// MPE zone configuration attached to a clip.
///
/// Enables per-note pitch bend, pressure, and timbre routing via a dedicated
/// member channel per active note.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MpeZone {
    /// Which zone this is (Lower or Upper).
    pub kind: MpeZoneKind,
    /// Number of member channels (1-15).  Master channel is separate.
    /// Lower zone: master=1, members=2..=num_channels+1.
    /// Upper zone: master=16, members=15..=16-num_channels.
    pub num_channels: u8,
    /// Per-note pitch-bend semitone range (MPE default 48).
    pub pitch_bend_range: u8,
}

impl Default for MpeZone {
    fn default() -> Self {
        Self { kind: MpeZoneKind::Lower, num_channels: 15, pitch_bend_range: 48 }
    }
}

impl MpeZone {
    /// Create an MPE lower zone with the given number of member channels.
    pub fn lower(num_channels: u8) -> Self {
        Self { kind: MpeZoneKind::Lower, num_channels: num_channels.clamp(1, 15), pitch_bend_range: 48 }
    }

    /// Create an MPE upper zone with the given number of member channels.
    pub fn upper(num_channels: u8) -> Self {
        Self { kind: MpeZoneKind::Upper, num_channels: num_channels.clamp(1, 15), pitch_bend_range: 48 }
    }

    /// Return the 0-based MIDI channel index of the zone master channel.
    pub fn master_channel(&self) -> u8 {
        match self.kind {
            MpeZoneKind::Lower => 0,   // MIDI channel 1
            MpeZoneKind::Upper => 15,  // MIDI channel 16
        }
    }

    /// Return the 0-based MIDI channel indices of all member channels (in allocation order).
    pub fn member_channels(&self) -> Vec<u8> {
        let n = self.num_channels as u8;
        match self.kind {
            MpeZoneKind::Lower => (1..=n).collect(),
            MpeZoneKind::Upper => ((15 - n)..15).rev().collect(),
        }
    }
}

/// Allocates MIDI member channels round-robin for active MPE notes.
///
/// Each active note occupies one member channel.  When all channels are busy
/// the oldest active note's channel is stolen (LRU).
pub struct MpeChannelMap {
    zone: MpeZone,
    /// Active allocation: member_channel_idx → MIDI note currently on that channel.
    /// None = free.
    slots: Vec<Option<u8>>,
    /// Round-robin allocation pointer.
    next_slot: usize,
}

impl MpeChannelMap {
    pub fn new(zone: MpeZone) -> Self {
        let n = zone.num_channels as usize;
        Self { zone, slots: vec![None; n], next_slot: 0 }
    }

    /// Allocate a member channel for the given MIDI note.
    ///
    /// Returns the 0-based MIDI channel index to use for NoteOn + expressions.
    /// If all channels are busy the oldest is stolen (round-robin).
    pub fn allocate(&mut self, note: u8) -> u8 {
        let n = self.slots.len();
        // First pass: find a free slot.
        for i in 0..n {
            let idx = (self.next_slot + i) % n;
            if self.slots[idx].is_none() {
                self.slots[idx] = Some(note);
                self.next_slot = (idx + 1) % n;
                return self.channel_for_slot(idx);
            }
        }
        // All busy — steal next_slot (oldest in round-robin order).
        let idx = self.next_slot % n;
        self.slots[idx] = Some(note);
        self.next_slot = (idx + 1) % n;
        self.channel_for_slot(idx)
    }

    /// Release the channel assigned to the given MIDI note (if any).
    /// Returns the 0-based MIDI channel that was freed, or None if not found.
    pub fn release(&mut self, note: u8) -> Option<u8> {
        for (idx, slot) in self.slots.iter_mut().enumerate() {
            if *slot == Some(note) {
                *slot = None;
                return Some(self.channel_for_slot(idx));
            }
        }
        None
    }

    /// Return the channel currently assigned to `note`, if any.
    pub fn channel_of(&self, note: u8) -> Option<u8> {
        self.slots.iter().enumerate()
            .find(|(_, s)| **s == Some(note))
            .map(|(idx, _)| self.channel_for_slot(idx))
    }

    /// Return the zone master channel (0-based index).
    pub fn master_channel(&self) -> u8 { self.zone.master_channel() }

    /// Return the zone's pitch bend range in semitones.
    pub fn pitch_bend_range(&self) -> u8 { self.zone.pitch_bend_range }

    fn channel_for_slot(&self, slot_idx: usize) -> u8 {
        self.zone.member_channels()[slot_idx]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lower_zone_master_and_members() {
        let z = MpeZone::lower(3);
        assert_eq!(z.master_channel(), 0);
        assert_eq!(z.member_channels(), vec![1, 2, 3]);
    }

    #[test]
    fn upper_zone_master_and_members() {
        let z = MpeZone::upper(3);
        assert_eq!(z.master_channel(), 15);
        assert_eq!(z.member_channels(), vec![14, 13, 12]);
    }

    #[test]
    fn allocate_sequential() {
        let mut map = MpeChannelMap::new(MpeZone::lower(3));
        assert_eq!(map.allocate(60), 1);
        assert_eq!(map.allocate(64), 2);
        assert_eq!(map.allocate(67), 3);
    }

    #[test]
    fn release_frees_channel() {
        let mut map = MpeChannelMap::new(MpeZone::lower(3));
        map.allocate(60); // ch 1
        map.allocate(64); // ch 2
        map.release(60);  // ch 1 now free
        let ch = map.allocate(72); // should reuse ch 1 (round-robin wraps)
        assert_eq!(ch, 3); // next free after ch2 is ch3
    }

    #[test]
    fn steal_when_all_busy() {
        let mut map = MpeChannelMap::new(MpeZone::lower(2));
        map.allocate(60); // ch 1
        map.allocate(64); // ch 2 — all busy
        // Steal: next_slot wraps back to 0, steals ch 1.
        let ch = map.allocate(67);
        assert_eq!(ch, 1);
    }

    #[test]
    fn channel_of_returns_correct() {
        let mut map = MpeChannelMap::new(MpeZone::lower(4));
        map.allocate(60); // ch 1
        map.allocate(64); // ch 2
        assert_eq!(map.channel_of(64), Some(2));
        assert_eq!(map.channel_of(99), None);
    }
}
