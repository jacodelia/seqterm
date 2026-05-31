//! MIDI 2.0 — Universal MIDI Packet (UMP) and MIDI Capability Inquiry (CI).
//!
//! Spec references:
//!   M2-104-UM  §2   Universal MIDI Packet format
//!   M2-104-UM  §4   MIDI 2.0 Channel Voice messages
//!   M2-101-UM       MIDI Capability Inquiry
//!
//! This module provides:
//!   - [`UmpPacket`]      — typed wrapper over raw 32/64/128-bit UMP words
//!   - [`MidiCiMessage`]  — MIDI CI SysEx8 payload
//!   - [`ump_from_midi1`] — lossless upconversion: MIDI 1.0 → UMP (Type 2 + Type 4)
//!   - [`midi1_from_ump`] — downconversion: UMP → MIDI 1.0 (lossy for high-res fields)

use crate::MidiMessage;

// ─── Raw word types ───────────────────────────────────────────────────────────

pub type UmpWord = u32;

// ─── UMP Message Type nibble (bits 31-28 of word 0) ─────────────────────────

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UmpMessageType {
    Utility            = 0x0,
    SystemRealTime     = 0x1,
    Midi1ChannelVoice  = 0x2,
    Data64             = 0x3,
    Midi2ChannelVoice  = 0x4,
    Data128            = 0x5,
    Unknown(u8),
}

impl UmpMessageType {
    pub fn from_nibble(n: u8) -> Self {
        match n & 0x0F {
            0x0 => Self::Utility,
            0x1 => Self::SystemRealTime,
            0x2 => Self::Midi1ChannelVoice,
            0x3 => Self::Data64,
            0x4 => Self::Midi2ChannelVoice,
            0x5 => Self::Data128,
            v   => Self::Unknown(v),
        }
    }

    pub fn word_count(self) -> usize {
        match self {
            Self::Utility | Self::SystemRealTime | Self::Midi1ChannelVoice => 1,
            Self::Data64  | Self::Midi2ChannelVoice                        => 2,
            Self::Data128                                                   => 4,
            Self::Unknown(_)                                                => 1,
        }
    }
}

// ─── UmpPacket ────────────────────────────────────────────────────────────────

/// A Universal MIDI Packet — one to four 32-bit words.
/// Word 0 always carries the message type in bits 31-28.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UmpPacket {
    /// 32-bit packets: Utility, System Real-Time/Common, MIDI 1.0 Channel Voice.
    Single(UmpWord),
    /// 64-bit packets: MIDI 2.0 Channel Voice, SysEx7 data.
    Double([UmpWord; 2]),
    /// 128-bit packets: SysEx8, UMP stream messages.
    Quad([UmpWord; 4]),
}

impl UmpPacket {
    /// Construct from a flat slice of words (reads as many as the type requires).
    /// Returns `None` if the slice is too short.
    pub fn from_words(words: &[UmpWord]) -> Option<Self> {
        let mt = UmpMessageType::from_nibble((words.first()? >> 28) as u8);
        match mt.word_count() {
            1 => Some(Self::Single(words[0])),
            2 if words.len() >= 2 => Some(Self::Double([words[0], words[1]])),
            4 if words.len() >= 4 => Some(Self::Quad([words[0], words[1], words[2], words[3]])),
            _ => None,
        }
    }

    /// First word (always present).
    pub fn word0(&self) -> UmpWord {
        match self {
            Self::Single(w)    => *w,
            Self::Double(ws)   => ws[0],
            Self::Quad(ws)     => ws[0],
        }
    }

    pub fn message_type(&self) -> UmpMessageType {
        UmpMessageType::from_nibble((self.word0() >> 28) as u8)
    }

    /// UMP Group (0-15), bits 27-24 of word 0.
    pub fn group(&self) -> u8 {
        ((self.word0() >> 24) & 0x0F) as u8
    }

    /// Flat view of all words.
    pub fn words(&self) -> &[UmpWord] {
        match self {
            Self::Single(w)  => std::slice::from_ref(w),
            Self::Double(ws) => ws.as_slice(),
            Self::Quad(ws)   => ws.as_slice(),
        }
    }

    // ── MIDI 1.0 Channel Voice (Type 2) constructors ──────────────────────

    /// Build a 32-bit MIDI 1.0 Channel Voice packet (Type 2).
    /// `status` = upper nibble of MIDI status (e.g. 0x90 for NoteOn),
    /// `channel` 0-15, `data0`/`data1` = payload bytes (7-bit each).
    pub fn midi1_channel_voice(group: u8, status: u8, channel: u8, data0: u8, data1: u8) -> Self {
        let w = (0x2u32 << 28)
            | ((group   as u32 & 0x0F) << 24)
            | ((status  as u32 & 0xF0) << 16)
            | ((channel as u32 & 0x0F) << 16)
            | ((data0   as u32 & 0x7F) << 8)
            |  (data1   as u32 & 0x7F);
        Self::Single(w)
    }

    // ── MIDI 2.0 Channel Voice (Type 4) constructors ──────────────────────

    fn midi2_cv(group: u8, status: u8, channel: u8, index0: u8, index1: u8, data32: u32) -> Self {
        let w0 = (0x4u32 << 28)
            | ((group   as u32 & 0x0F) << 24)
            | ((status  as u32 & 0xF0) << 16)
            | ((channel as u32 & 0x0F) << 16)
            | ((index0  as u32)        << 8)
            |  (index1  as u32);
        Self::Double([w0, data32])
    }

    /// MIDI 2.0 Note On (Type 4, status 0x90).
    /// `velocity` is 16-bit (0x0000–0xFFFF).
    pub fn note_on_midi2(group: u8, channel: u8, note: u8, velocity: u16, attr_type: u8, attr_data: u16) -> Self {
        // attr_data is 16-bit; store MSB in byte, LSB in separate field
        let w0 = (0x4u32 << 28)
            | ((group   as u32 & 0x0F) << 24)
            | (0x90u32 << 16)
            | ((channel as u32 & 0x0F) << 16)
            | ((note    as u32 & 0x7F) << 8)
            |  (attr_type as u32 & 0xFF);
        let w1 = ((velocity as u32) << 16) | (attr_data as u32 & 0xFFFF);
        Self::Double([w0, w1])
    }

    /// MIDI 2.0 Note Off (Type 4, status 0x80).
    pub fn note_off_midi2(group: u8, channel: u8, note: u8, velocity: u16, attr_type: u8, attr_data: u16) -> Self {
        let w0 = (0x4u32 << 28)
            | ((group   as u32 & 0x0F) << 24)
            | (0x80u32 << 16)
            | ((channel as u32 & 0x0F) << 16)
            | ((note    as u32 & 0x7F) << 8)
            |  (attr_type as u32 & 0xFF);
        let w1 = ((velocity as u32) << 16) | (attr_data as u32 & 0xFFFF);
        Self::Double([w0, w1])
    }

    /// MIDI 2.0 Pitch Bend (Type 4, status 0x60).
    /// `value` is 32-bit unsigned, centre = 0x8000_0000.
    pub fn pitch_bend_midi2(group: u8, channel: u8, value: u32) -> Self {
        Self::midi2_cv(group, 0x60, channel, 0, 0, value)
    }

    /// MIDI 2.0 Channel Pressure (Type 4, status 0xD0 → opcode 0x60 in CV).
    pub fn channel_pressure_midi2(group: u8, channel: u8, value: u32) -> Self {
        // Spec §4.3.9: status nibble 0xD, opcode field 0x60 in Type 4
        Self::midi2_cv(group, 0xD0, channel, 0, 0, value)
    }

    /// MIDI 2.0 Registered Controller (NRPN/RPN) — status 0x20.
    pub fn registered_ctrl_midi2(group: u8, channel: u8, bank: u8, index: u8, value: u32) -> Self {
        Self::midi2_cv(group, 0x20, channel, bank, index, value)
    }

    /// MIDI 2.0 Assignable Controller — status 0x30.
    pub fn assignable_ctrl_midi2(group: u8, channel: u8, bank: u8, index: u8, value: u32) -> Self {
        Self::midi2_cv(group, 0x30, channel, bank, index, value)
    }
}

// ─── Scaling helpers ──────────────────────────────────────────────────────────

/// Scale a 7-bit MIDI 1.0 velocity to a 16-bit MIDI 2.0 velocity.
/// Uses the M2-104-UM recommended bit-shift scaling.
#[inline]
pub fn velocity_midi1_to_midi2(v7: u8) -> u16 {
    let v7 = v7 as u32 & 0x7F;
    // Expand 7 → 16 bits: replicate bits into the lower positions
    let v16 = (v7 << 9) | (v7 << 2) | (v7 >> 5);
    v16 as u16
}

/// Scale a 16-bit MIDI 2.0 velocity down to 7-bit MIDI 1.0.
#[inline]
pub fn velocity_midi2_to_midi1(v16: u16) -> u8 {
    (v16 >> 9) as u8
}

/// Scale a 14-bit MIDI 1.0 pitch bend (0–16383, centre 8192) to 32-bit MIDI 2.0
/// (0–0xFFFF_FFFF, centre 0x8000_0000).
#[inline]
pub fn pitch_bend_midi1_to_midi2(pb14: i16) -> u32 {
    // pb14 ∈ [-8192, 8191]; unsigned_14 ∈ [0, 16383]
    let u14 = (pb14 + 8192).clamp(0, 16383) as u64;
    // Scale: u14 * 0xFFFF_FFFF / 16383
    ((u14 * 0xFFFF_FFFF) / 16383) as u32
}

/// Scale a 32-bit MIDI 2.0 pitch bend back to 14-bit MIDI 1.0 (signed, centre 0).
#[inline]
pub fn pitch_bend_midi2_to_midi1(pb32: u32) -> i16 {
    let u14 = (pb32 as u64 * 16383 / 0xFFFF_FFFF) as i16;
    u14 - 8192
}

/// Scale a 7-bit MIDI 1.0 control value to 32-bit MIDI 2.0.
/// Uses bit-replication to guarantee exact roundtrip.
#[inline]
pub fn cc_midi1_to_midi2(v7: u8) -> u32 {
    let v = v7 as u32 & 0x7F;
    (v << 25) | (v << 18) | (v << 11) | (v << 4) | (v >> 3)
}

/// Scale a 32-bit MIDI 2.0 controller value to 7-bit MIDI 1.0.
#[inline]
pub fn cc_midi2_to_midi1(v32: u32) -> u8 {
    (v32 >> 25) as u8
}

// ─── MIDI 1.0 ↔ UMP conversion ───────────────────────────────────────────────

/// Convert a MIDI 1.0 message to a UMP packet.
///
/// Channel voice messages produce **Type 4** (MIDI 2.0 Channel Voice) with
/// properly upscaled values.  System messages (Clock, Start, Stop, …) produce
/// **Type 1** (System Real-Time).  SysEx produces a **Type 3** (SysEx7, 64-bit)
/// or **Type 5** (SysEx8) — here we emit a Type 3 start fragment (single
/// packet; callers must segment for payloads > 6 bytes).
pub fn ump_from_midi1(msg: &MidiMessage, group: u8) -> UmpPacket {
    let g = group & 0x0F;
    match msg {
        MidiMessage::NoteOn { channel, note, velocity } => {
            UmpPacket::note_on_midi2(g, *channel, *note,
                velocity_midi1_to_midi2(*velocity), 0, 0)
        }
        MidiMessage::NoteOff { channel, note } => {
            UmpPacket::note_off_midi2(g, *channel, *note, 0, 0, 0)
        }
        MidiMessage::CC { channel, control, value } => {
            // Map CC to MIDI 2.0 Assignable Controller (bank=0, index=cc).
            UmpPacket::assignable_ctrl_midi2(g, *channel, 0, *control,
                cc_midi1_to_midi2(*value))
        }
        MidiMessage::PitchBend { channel, value } => {
            UmpPacket::pitch_bend_midi2(g, *channel, pitch_bend_midi1_to_midi2(*value))
        }
        MidiMessage::ProgramChange { channel, program } => {
            // MIDI 2.0 Program Change (Type 4, status 0xC0) — 64-bit.
            // Word 0: status|channel, option flags, bank MSB, bank LSB
            // Word 1: program, 0, 0, 0
            let w0 = (0x4u32 << 28)
                | ((g as u32) << 24)
                | (0xC0u32 << 16)
                | ((*channel as u32 & 0x0F) << 16);
            let w1 = (*program as u32) << 24;
            UmpPacket::Double([w0, w1])
        }
        MidiMessage::Clock    => sys_rt(g, 0xF8),
        MidiMessage::Start    => sys_rt(g, 0xFA),
        MidiMessage::Stop     => sys_rt(g, 0xFC),
        MidiMessage::Continue => sys_rt(g, 0xFB),
        MidiMessage::ActiveSensing => sys_rt(g, 0xFE),
        MidiMessage::SysEx(data) => {
            // For short SysEx (≤ 6 bytes) emit a single Complete packet.
            // Longer SysEx is segmented: use ump_sysex7_packets and return the first packet.
            // Callers that need all packets should call ump_sysex7_packets() directly.
            if data.len() <= 6 {
                sysex7_packet(g, SysEx7Status::Complete, data)
            } else {
                // Return only the Start packet here; full segmentation available via
                // ump_sysex7_packets().  This keeps the existing single-return signature.
                sysex7_packet(g, SysEx7Status::Start, &data[..6])
            }
        }
    }
}

fn sys_rt(group: u8, status: u8) -> UmpPacket {
    let w = (0x1u32 << 28) | ((group as u32 & 0x0F) << 24) | ((status as u32) << 16);
    UmpPacket::Single(w)
}

/// SysEx7 packet status nibble (UMP spec §4.4).
#[derive(Clone, Copy)]
enum SysEx7Status {
    Complete  = 0x0,
    Start     = 0x1,
    Continue  = 0x2,
    End       = 0x3,
}

/// Build one 64-bit Type-3 SysEx7 packet from up to 6 payload bytes.
fn sysex7_packet(group: u8, status: SysEx7Status, payload: &[u8]) -> UmpPacket {
    let g = group as u32 & 0x0F;
    let len = payload.len().min(6) as u32;
    let mut w0 = (0x3u32 << 28) | (g << 24) | ((status as u32) << 20) | (len << 16);
    if payload.len() > 0 { w0 |= (payload[0] as u32) << 8; }
    if payload.len() > 1 { w0 |= payload[1] as u32; }
    let mut w1 = 0u32;
    for (i, &b) in payload.iter().enumerate().skip(2).take(4) {
        w1 |= (b as u32) << (24 - (i - 2) * 8);
    }
    UmpPacket::Double([w0, w1])
}

/// Segment a SysEx payload into one or more 64-bit Type-3 UMP packets.
///
/// Each packet carries up to 6 payload bytes.  The first packet uses Start (or
/// Complete if ≤ 6 bytes), middle packets use Continue, and the last uses End.
pub fn ump_sysex7_packets(payload: &[u8], group: u8) -> Vec<UmpPacket> {
    if payload.is_empty() {
        return vec![sysex7_packet(group, SysEx7Status::Complete, &[])];
    }
    let chunks: Vec<&[u8]> = payload.chunks(6).collect();
    let n = chunks.len();
    chunks.iter().enumerate().map(|(i, chunk)| {
        let status = match (i, n) {
            (0, 1) => SysEx7Status::Complete,
            (0, _) => SysEx7Status::Start,
            (k, _) if k == n - 1 => SysEx7Status::End,
            _ => SysEx7Status::Continue,
        };
        sysex7_packet(group, status, chunk)
    }).collect()
}

/// Convert a UMP packet back to a MIDI 1.0 message where possible.
///
/// Returns `None` for packet types that have no MIDI 1.0 equivalent (e.g.
/// per-note controllers) or for malformed packets.
pub fn midi1_from_ump(pkt: &UmpPacket) -> Option<MidiMessage> {
    match pkt {
        UmpPacket::Single(w) => {
            let mt = (w >> 28) as u8;
            let status = ((w >> 16) & 0xFF) as u8;
            let d0 = ((w >> 8) & 0x7F) as u8;
            let d1 = (w & 0x7F) as u8;
            let ch = status & 0x0F;
            match mt {
                0x2 => midi1_cv_from(status & 0xF0, ch, d0, d1),
                0x1 => sys_rt_from(status),
                _ => None,
            }
        }
        UmpPacket::Double(ws) => {
            let mt = (ws[0] >> 28) as u8;
            let status = ((ws[0] >> 16) & 0xFF) as u8;
            let ch = status & 0x0F;
            let index0 = ((ws[0] >> 8) & 0xFF) as u8; // note / bank
            let index1 = (ws[0] & 0xFF) as u8;         // attr_type / ctrl_index
            let data32 = ws[1];

            match mt {
                0x4 => match status & 0xF0 {
                    0x90 => {
                        let v16 = (data32 >> 16) as u16;
                        let v7  = velocity_midi2_to_midi1(v16);
                        Some(MidiMessage::NoteOn { channel: ch, note: index0, velocity: v7 })
                    }
                    0x80 => Some(MidiMessage::NoteOff { channel: ch, note: index0 }),
                    0x60 => {
                        Some(MidiMessage::PitchBend {
                            channel: ch,
                            value: pitch_bend_midi2_to_midi1(data32),
                        })
                    }
                    0xD0 => {
                        // Channel pressure → CC 0x74 (aftertouch channel) — no direct MIDI1 mapping;
                        // approximate as NoteOff-like, or skip. We return None to signal no equivalent.
                        None
                    }
                    0x30 => {
                        // Assignable Controller → CC
                        let v7 = cc_midi2_to_midi1(data32);
                        Some(MidiMessage::CC { channel: ch, control: index1, value: v7 })
                    }
                    0xC0 => {
                        let prog = (data32 >> 24) as u8;
                        Some(MidiMessage::ProgramChange { channel: ch, program: prog })
                    }
                    _ => None,
                },
                0x3 => {
                    // SysEx7 — reassemble payload bytes from both words.
                    let len = ((ws[0] >> 16) & 0x0F) as usize;
                    let mut data = Vec::with_capacity(len);
                    if len > 0 { data.push(((ws[0] >> 8) & 0x7F) as u8); }
                    if len > 1 { data.push((ws[0] & 0x7F) as u8); }
                    for i in 0..len.saturating_sub(2).min(4) {
                        data.push(((ws[1] >> (24 - i * 8)) & 0x7F) as u8);
                    }
                    Some(MidiMessage::SysEx(data))
                }
                _ => None,
            }
        }
        UmpPacket::Quad(_) => None, // SysEx8 / UMP Stream — no MIDI 1.0 equivalent
    }
}

fn midi1_cv_from(status_nibble: u8, ch: u8, d0: u8, d1: u8) -> Option<MidiMessage> {
    match status_nibble {
        0x90 if d1 > 0 => Some(MidiMessage::NoteOn  { channel: ch, note: d0, velocity: d1 }),
        0x90           => Some(MidiMessage::NoteOff  { channel: ch, note: d0 }),
        0x80           => Some(MidiMessage::NoteOff  { channel: ch, note: d0 }),
        0xB0           => Some(MidiMessage::CC { channel: ch, control: d0, value: d1 }),
        0xC0           => Some(MidiMessage::ProgramChange { channel: ch, program: d0 }),
        0xE0           => {
            let v = (d0 as i16) | ((d1 as i16) << 7);
            Some(MidiMessage::PitchBend { channel: ch, value: v - 8192 })
        }
        _ => None,
    }
}

fn sys_rt_from(status: u8) -> Option<MidiMessage> {
    match status {
        0xF8 => Some(MidiMessage::Clock),
        0xFA => Some(MidiMessage::Start),
        0xFC => Some(MidiMessage::Stop),
        0xFB => Some(MidiMessage::Continue),
        0xFE => Some(MidiMessage::ActiveSensing),
        _    => None,
    }
}

// ─── MIDI Capability Inquiry (CI) ────────────────────────────────────────────

/// MUID — a 28-bit MIDI CI unique identifier (4 × 7-bit bytes, LSB-first).
/// `0x0FFF_FFFF` is the broadcast MUID.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Muid(pub u32);

impl Muid {
    pub const BROADCAST: Self = Self(0x0FFF_FFFF);

    /// Encode as four 7-bit bytes (LSB first) into `buf[0..4]`.
    pub fn encode(&self, buf: &mut [u8; 4]) {
        let v = self.0;
        buf[0] = (v & 0x7F) as u8;
        buf[1] = ((v >> 7) & 0x7F) as u8;
        buf[2] = ((v >> 14) & 0x7F) as u8;
        buf[3] = ((v >> 21) & 0x7F) as u8;
    }

    /// Decode from four 7-bit bytes (LSB first).
    pub fn decode(buf: &[u8; 4]) -> Self {
        let v = (buf[0] as u32)
            | ((buf[1] as u32) << 7)
            | ((buf[2] as u32) << 14)
            | ((buf[3] as u32) << 21);
        Self(v & 0x0FFF_FFFF)
    }
}

/// MIDI CI sub-ID₂ values (M2-101-UM §5).
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MidiCiSubId {
    /// Management: Discovery.
    Discovery             = 0x70,
    DiscoveryReply        = 0x71,
    /// Management: Invalidate MUID.
    InvalidateMuid        = 0x7E,
    /// Management: NAK.
    Nak                   = 0x7F,
    /// Profile Configuration: Inquiry.
    ProfileInquiry        = 0x20,
    ProfileInquiryReply   = 0x21,
    ProfileAdded          = 0x26,
    ProfileRemoved        = 0x27,
    ProfileDetails        = 0x28,
    ProfileDetailsReply   = 0x29,
    /// Property Exchange: capabilities.
    PropertyCapabilities       = 0x30,
    PropertyCapabilitiesReply  = 0x31,
    GetProperty           = 0x34,
    GetPropertyReply      = 0x35,
    SetProperty           = 0x38,
    SetPropertyReply      = 0x39,
}

impl MidiCiSubId {
    pub fn from_byte(b: u8) -> Option<Self> {
        Some(match b {
            0x70 => Self::Discovery,
            0x71 => Self::DiscoveryReply,
            0x7E => Self::InvalidateMuid,
            0x7F => Self::Nak,
            0x20 => Self::ProfileInquiry,
            0x21 => Self::ProfileInquiryReply,
            0x26 => Self::ProfileAdded,
            0x27 => Self::ProfileRemoved,
            0x28 => Self::ProfileDetails,
            0x29 => Self::ProfileDetailsReply,
            0x30 => Self::PropertyCapabilities,
            0x31 => Self::PropertyCapabilitiesReply,
            0x34 => Self::GetProperty,
            0x35 => Self::GetPropertyReply,
            0x38 => Self::SetProperty,
            0x39 => Self::SetPropertyReply,
            _ => return None,
        })
    }
}

/// A parsed MIDI CI message (SysEx sub-ID 0x0D header + payload).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MidiCiMessage {
    pub version:         u8,
    pub source_muid:     Muid,
    pub destination_muid: Muid,
    pub sub_id:          MidiCiSubId,
    /// Remaining payload bytes after the 13-byte CI header.
    pub payload:         Vec<u8>,
}

impl MidiCiMessage {
    pub const CI_SUB_ID1: u8 = 0x0D;

    /// Build a Discovery message (§5.5).
    /// `manufacturer` is 3 bytes (or two 7-bit bytes with `0x00` prefix for odd ones).
    pub fn discovery(
        source: Muid,
        manufacturer: [u8; 3],
        family: [u8; 2],
        family_member: [u8; 2],
        version: [u8; 4],
        ci_categories: u8,
        receivable_max_sysex_size: u32,
    ) -> Self {
        let mut payload = Vec::with_capacity(14);
        payload.extend_from_slice(&manufacturer);
        payload.extend_from_slice(&family);
        payload.extend_from_slice(&family_member);
        payload.extend_from_slice(&version);
        payload.push(ci_categories);
        // max SysEx size as 4 × 7-bit bytes, LSB first
        for i in 0..4 {
            payload.push(((receivable_max_sysex_size >> (7 * i)) & 0x7F) as u8);
        }
        Self {
            version: 2,
            source_muid: source,
            destination_muid: Muid::BROADCAST,
            sub_id: MidiCiSubId::Discovery,
            payload,
        }
    }

    /// Encode to a SysEx7 byte sequence (without 0xF0/0xF7 framing).
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(14 + self.payload.len());
        // Universal System Exclusive, Device ID 0x7F (whole port)
        out.push(0x7E);
        out.push(0x7F); // device ID
        out.push(Self::CI_SUB_ID1);
        out.push(self.sub_id as u8);
        out.push(self.version);
        let mut src = [0u8; 4]; self.source_muid.encode(&mut src);
        let mut dst = [0u8; 4]; self.destination_muid.encode(&mut dst);
        out.extend_from_slice(&src);
        out.extend_from_slice(&dst);
        out.extend_from_slice(&self.payload);
        out
    }

    /// Parse from raw SysEx payload bytes (after the 0xF0, starting with 0x7E).
    /// Returns `None` if the slice is too short or has wrong sub-IDs.
    pub fn parse(bytes: &[u8]) -> Option<Self> {
        // Minimum: 0x7E, device, 0x0D, sub_id2, version, src(4), dst(4) = 13 bytes
        if bytes.len() < 13 { return None; }
        if bytes[0] != 0x7E { return None; }
        if bytes[2] != Self::CI_SUB_ID1 { return None; }
        let sub_id = MidiCiSubId::from_byte(bytes[3])?;
        let version = bytes[4];
        let src_bytes = [bytes[5], bytes[6], bytes[7], bytes[8]];
        let dst_bytes = [bytes[9], bytes[10], bytes[11], bytes[12]];
        Some(Self {
            version,
            source_muid:      Muid::decode(&src_bytes),
            destination_muid: Muid::decode(&dst_bytes),
            sub_id,
            payload: bytes[13..].to_vec(),
        })
    }

    /// Wrap encoded bytes in a [`MidiMessage::SysEx`].
    pub fn to_sysex(&self) -> MidiMessage {
        MidiMessage::SysEx(self.encode())
    }
}

// ─── UMP stream helpers ───────────────────────────────────────────────────────

/// Parse a raw byte stream into a sequence of [`UmpPacket`]s.
/// Bytes are consumed four at a time (each word is big-endian).
pub fn parse_ump_stream(bytes: &[u8]) -> Vec<UmpPacket> {
    let mut packets = Vec::new();
    let mut i = 0;
    while i + 3 < bytes.len() {
        let w = u32::from_be_bytes([bytes[i], bytes[i+1], bytes[i+2], bytes[i+3]]);
        let mt = UmpMessageType::from_nibble((w >> 28) as u8);
        let needed = mt.word_count() * 4;
        if i + needed > bytes.len() { break; }
        let mut words = Vec::with_capacity(mt.word_count());
        for j in 0..mt.word_count() {
            words.push(u32::from_be_bytes([
                bytes[i + j*4],
                bytes[i + j*4 + 1],
                bytes[i + j*4 + 2],
                bytes[i + j*4 + 3],
            ]));
        }
        if let Some(pkt) = UmpPacket::from_words(&words) {
            packets.push(pkt);
        }
        i += needed;
    }
    packets
}

/// Encode a slice of [`UmpPacket`]s into a big-endian byte stream.
pub fn encode_ump_stream(packets: &[UmpPacket]) -> Vec<u8> {
    let mut out = Vec::new();
    for pkt in packets {
        for &w in pkt.words() {
            out.extend_from_slice(&w.to_be_bytes());
        }
    }
    out
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn velocity_scaling_roundtrip() {
        for v7 in 0u8..=127 {
            let v16 = velocity_midi1_to_midi2(v7);
            let back = velocity_midi2_to_midi1(v16);
            assert_eq!(back, v7, "velocity roundtrip failed for {v7}");
        }
    }

    #[test]
    fn pitch_bend_scaling_roundtrip() {
        for pb in [-8192i16, -4096, 0, 4095, 8191] {
            let v32 = pitch_bend_midi1_to_midi2(pb);
            let back = pitch_bend_midi2_to_midi1(v32);
            // Allow ±1 rounding error
            assert!((back - pb).abs() <= 1, "pb roundtrip failed for {pb}: got {back}");
        }
    }

    #[test]
    fn cc_scaling_roundtrip() {
        for v in 0u8..=127 {
            let v32 = cc_midi1_to_midi2(v);
            let back = cc_midi2_to_midi1(v32);
            assert_eq!(back, v, "cc roundtrip failed for {v}");
        }
    }

    #[test]
    fn note_on_midi1_to_ump_and_back() {
        let msg = MidiMessage::NoteOn { channel: 1, note: 60, velocity: 100 };
        let pkt = ump_from_midi1(&msg, 0);
        assert_eq!(pkt.message_type(), UmpMessageType::Midi2ChannelVoice);
        let back = midi1_from_ump(&pkt).unwrap();
        match back {
            MidiMessage::NoteOn { channel, note, velocity } => {
                assert_eq!(channel, 1);
                assert_eq!(note, 60);
                assert_eq!(velocity, 100);
            }
            other => panic!("expected NoteOn, got {other:?}"),
        }
    }

    #[test]
    fn note_off_midi1_to_ump_and_back() {
        let msg = MidiMessage::NoteOff { channel: 2, note: 48 };
        let pkt = ump_from_midi1(&msg, 0);
        let back = midi1_from_ump(&pkt).unwrap();
        match back {
            MidiMessage::NoteOff { channel, note } => {
                assert_eq!(channel, 2);
                assert_eq!(note, 48);
            }
            other => panic!("expected NoteOff, got {other:?}"),
        }
    }

    #[test]
    fn pitch_bend_midi1_to_ump_and_back() {
        let msg = MidiMessage::PitchBend { channel: 0, value: 4096 };
        let pkt = ump_from_midi1(&msg, 0);
        let back = midi1_from_ump(&pkt).unwrap();
        match back {
            MidiMessage::PitchBend { value, .. } => {
                assert!((value - 4096).abs() <= 1);
            }
            other => panic!("expected PitchBend, got {other:?}"),
        }
    }

    #[test]
    fn sys_rt_clock_roundtrip() {
        let pkt = ump_from_midi1(&MidiMessage::Clock, 0);
        assert_eq!(pkt.message_type(), UmpMessageType::SystemRealTime);
        assert_eq!(midi1_from_ump(&pkt), Some(MidiMessage::Clock));
    }

    #[test]
    fn cc_midi1_to_ump_and_back() {
        let msg = MidiMessage::CC { channel: 3, control: 74, value: 64 };
        let pkt = ump_from_midi1(&msg, 0);
        let back = midi1_from_ump(&pkt).unwrap();
        match back {
            MidiMessage::CC { channel, control, value } => {
                assert_eq!(channel, 3);
                assert_eq!(control, 74);
                assert_eq!(value, 64);
            }
            other => panic!("expected CC, got {other:?}"),
        }
    }

    #[test]
    fn sysex_midi1_to_ump_and_back() {
        let payload = vec![0x41, 0x10, 0x42, 0x12];
        let msg = MidiMessage::SysEx(payload.clone());
        let pkt = ump_from_midi1(&msg, 0);
        assert_eq!(pkt.message_type(), UmpMessageType::Data64);
        let back = midi1_from_ump(&pkt).unwrap();
        match back {
            MidiMessage::SysEx(data) => assert_eq!(&data, &payload),
            other => panic!("expected SysEx, got {other:?}"),
        }
    }

    #[test]
    fn ump_stream_encode_decode_roundtrip() {
        let packets = vec![
            ump_from_midi1(&MidiMessage::NoteOn { channel: 0, note: 60, velocity: 80 }, 0),
            ump_from_midi1(&MidiMessage::Clock, 0),
        ];
        let bytes = encode_ump_stream(&packets);
        let decoded = parse_ump_stream(&bytes);
        assert_eq!(decoded, packets);
    }

    #[test]
    fn muid_encode_decode_roundtrip() {
        let muid = Muid(0x0123_4567);
        let mut buf = [0u8; 4];
        muid.encode(&mut buf);
        let back = Muid::decode(&buf);
        assert_eq!(back, muid);
    }

    #[test]
    fn muid_broadcast_value() {
        assert_eq!(Muid::BROADCAST.0, 0x0FFF_FFFF);
    }

    #[test]
    fn midi_ci_discovery_parse_roundtrip() {
        let src = Muid(0x1234);
        let ci = MidiCiMessage::discovery(
            src,
            [0x41, 0x00, 0x00],
            [0x01, 0x00],
            [0x01, 0x00],
            [0x01, 0x00, 0x00, 0x00],
            0x7F,
            512,
        );
        let encoded = ci.encode();
        let parsed  = MidiCiMessage::parse(&encoded).unwrap();
        assert_eq!(parsed.sub_id, MidiCiSubId::Discovery);
        assert_eq!(parsed.source_muid, src);
        assert_eq!(parsed.destination_muid, Muid::BROADCAST);
        assert_eq!(parsed.payload, ci.payload);
    }

    #[test]
    fn midi2_note_on_packet_structure() {
        let pkt = UmpPacket::note_on_midi2(0, 1, 60, 0xC800, 0, 0);
        let UmpPacket::Double(ws) = pkt else { panic!("expected Double") };
        // bits 31-28 = 0x4 (MIDI2 CV), bits 27-24 = group 0, bits 23-16 = 0x91 (NoteOn ch1)
        assert_eq!((ws[0] >> 28) & 0xF, 0x4);
        assert_eq!((ws[0] >> 16) & 0xFF, 0x91);
        // note field bits 15-8
        assert_eq!((ws[0] >> 8) & 0x7F, 60);
        // velocity bits 31-16 of word 1
        assert_eq!((ws[1] >> 16) as u16, 0xC800);
    }
}
