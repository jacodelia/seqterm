pub mod router;
pub mod midir_adapter;
pub mod input_bus;
pub mod midi2;

pub use router::{shared_router, MidiRouter, SharedMidiRouter};
pub use midir_adapter::MidirMidiAdapter;
pub use input_bus::MidiInputBus;
pub use midi2::{
    MidiCiMessage, MidiCiSubId, Muid,
    UmpMessageType, UmpPacket, UmpWord,
    ump_from_midi1, midi1_from_ump,
    velocity_midi1_to_midi2, velocity_midi2_to_midi1,
    pitch_bend_midi1_to_midi2, pitch_bend_midi2_to_midi1,
    cc_midi1_to_midi2, cc_midi2_to_midi1,
    parse_ump_stream, encode_ump_stream,
};

use std::collections::HashMap;

#[cfg(unix)]
use midir::os::unix::VirtualOutput;

// ─── ALSA stderr suppression ──────────────────────────────────────────────────

/// Install a no-op ALSA error handler so that ALSA's C library stops writing
/// diagnostic messages (e.g. "open /dev/snd/seq failed") directly to stderr,
/// which would corrupt the ratatui TUI.
///
/// Safe to call multiple times; idempotent after the first call.
/// Only compiled on Linux; on other platforms this is an empty function.
#[cfg(target_os = "linux")]
pub fn suppress_alsa_stderr() {
    use std::ffi::{c_char, c_int};

    // No-op replacement for ALSA's default stderr printer.
    // The real signature has variadic `...` which we cannot express in stable
    // Rust, but since this handler ignores every argument and C calling
    // convention (cdecl) has the CALLER clean up the stack, passing a
    // non-variadic fn pointer is safe here.
    unsafe extern "C" fn noop(
        _file: *const c_char,
        _line: c_int,
        _func: *const c_char,
        _err:  c_int,
        _fmt:  *const c_char,
    ) {}

    unsafe extern "C" {
        // libasound is already linked transitively via midir → alsa-sys.
        fn snd_lib_error_set_handler(
            handler: Option<unsafe extern "C" fn(
                *const c_char,
                c_int,
                *const c_char,
                c_int,
                *const c_char,
            )>,
        );
    }

    unsafe { snd_lib_error_set_handler(Some(noop)); }
}

#[cfg(not(target_os = "linux"))]
pub fn suppress_alsa_stderr() {}

/// Canonical MIDI message types.
#[derive(Debug, Clone, PartialEq)]
pub enum MidiMessage {
    NoteOn { channel: u8, note: u8, velocity: u8 },
    NoteOff { channel: u8, note: u8 },
    CC { channel: u8, control: u8, value: u8 },
    ProgramChange { channel: u8, program: u8 },
    PitchBend { channel: u8, value: i16 },
    Clock,
    Start,
    Stop,
    Continue,
    ActiveSensing,
    SysEx(Vec<u8>),
}

impl MidiMessage {
    /// Encode the message as raw MIDI bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        match self {
            MidiMessage::NoteOn { channel, note, velocity } => {
                vec![0x90 | (channel & 0x0F), *note & 0x7F, *velocity & 0x7F]
            }
            MidiMessage::NoteOff { channel, note } => {
                vec![0x80 | (channel & 0x0F), *note & 0x7F, 0]
            }
            MidiMessage::CC { channel, control, value } => {
                vec![0xB0 | (channel & 0x0F), *control & 0x7F, *value & 0x7F]
            }
            MidiMessage::ProgramChange { channel, program } => {
                vec![0xC0 | (channel & 0x0F), *program & 0x7F]
            }
            MidiMessage::PitchBend { channel, value } => {
                let v = (*value + 8192).clamp(0, 16383) as u16;
                vec![0xE0 | (channel & 0x0F), (v & 0x7F) as u8, ((v >> 7) & 0x7F) as u8]
            }
            MidiMessage::Clock => vec![0xF8],
            MidiMessage::Start => vec![0xFA],
            MidiMessage::Stop => vec![0xFC],
            MidiMessage::Continue => vec![0xFB],
            MidiMessage::ActiveSensing => vec![0xFE],
            MidiMessage::SysEx(data) => {
                let mut out = vec![0xF0];
                out.extend_from_slice(data);
                out.push(0xF7);
                out
            }
        }
    }

    /// Parse raw MIDI bytes into a MidiMessage.
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.is_empty() {
            return None;
        }
        let status = bytes[0];
        match status & 0xF0 {
            0x90 if bytes.len() >= 3 => Some(MidiMessage::NoteOn {
                channel: status & 0x0F,
                note: bytes[1],
                velocity: bytes[2],
            }),
            0x80 if bytes.len() >= 3 => Some(MidiMessage::NoteOff {
                channel: status & 0x0F,
                note: bytes[1],
            }),
            0xB0 if bytes.len() >= 3 => Some(MidiMessage::CC {
                channel: status & 0x0F,
                control: bytes[1],
                value: bytes[2],
            }),
            0xC0 if bytes.len() >= 2 => Some(MidiMessage::ProgramChange {
                channel: status & 0x0F,
                program: bytes[1],
            }),
            0xE0 if bytes.len() >= 3 => {
                let lsb = bytes[1] as i16;
                let msb = bytes[2] as i16;
                Some(MidiMessage::PitchBend {
                    channel: status & 0x0F,
                    value: (msb << 7 | lsb) - 8192,
                })
            }
            _ => match status {
                0xF8 => Some(MidiMessage::Clock),
                0xFA => Some(MidiMessage::Start),
                0xFC => Some(MidiMessage::Stop),
                0xFB => Some(MidiMessage::Continue),
                0xFE => Some(MidiMessage::ActiveSensing),
                0xF0 => {
                    let data = bytes[1..].to_vec();
                    Some(MidiMessage::SysEx(data))
                }
                _ => None,
            },
        }
    }
}

// ─── Per-pattern virtual ports ────────────────────────────────────────────────

/// Create virtual MIDI output ports for all given pattern keys.
///
/// On Linux the implementation opens a **single** ALSA sequencer client
/// ("SeqTerm") with one port per pattern, leaving ALSA client slots free for
/// other programs (QSynth, Hydrogen, Carla, …).
///
/// On other Unix platforms each pattern gets its own midir virtual port.
/// On Windows / non-Unix the map is empty.
///
/// Returns a map from pattern key → raw-byte sender.
/// All ports are closed when the last sender for a key is dropped.
pub fn create_pattern_ports(pattern_keys: &[impl AsRef<str>]) -> HashMap<String, flume::Sender<Vec<u8>>> {
    if pattern_keys.is_empty() { return HashMap::new(); }

    #[cfg(target_os = "linux")]
    {
        let keys: Vec<&str> = pattern_keys.iter().map(|k| k.as_ref()).collect();
        match _create_alsa_multi_port(&keys) {
            Ok(map) => {
                tracing::info!(
                    "SeqTerm MIDI client opened with {} port(s)",
                    map.len()
                );
                return map;
            }
            Err(e) => {
                tracing::warn!("ALSA multi-port client failed ({e}), falling back to midir");
            }
        }
    }

    #[cfg(unix)]
    {
        let mut map = HashMap::new();
        for key in pattern_keys {
            let key = key.as_ref();
            match _create_midir_port(key) {
                Ok(tx) => { map.insert(key.to_owned(), tx); }
                Err(e) => tracing::warn!("MIDI port skipped for '{key}': {e}"),
            }
        }
        return map;
    }

    #[allow(unreachable_code)]
    HashMap::new()
}

/// Connect a SeqTerm MIDI port to an external destination via `aconnect`.
///
/// On single-client Linux builds `src_client` should be "SeqTerm" and
/// `dst_port` the destination recognised by aconnect.
pub fn auto_connect(src_client: &str, dst_port: &str) -> anyhow::Result<()> {
    let out = std::process::Command::new("aconnect")
        .arg(src_client)
        .arg(dst_port)
        .output()
        .map_err(|e| anyhow::anyhow!("aconnect not found (install alsa-utils): {e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        anyhow::bail!("aconnect '{src_client}' → '{dst_port}': {}", err.trim());
    }
    Ok(())
}

// ─── Linux: direct-delivery output connections ────────────────────────────────

/// Open direct ALSA output connections to specific named destinations.
///
/// Returns a map of dest_port_name → raw-byte sender.  Each sender delivers
/// MIDI directly to the resolved ALSA address — no `aconnect` required.
///
/// Port names are expected in the midir format:
///   `"ClientName:PortName ClientID:PortID"` (e.g. `"Hydrogen:Hydrogen Midi-In 130:0"`)
/// If the trailing numeric `N:M` is absent (aconnect-style names), the function
/// does a one-shot ALSA client enumeration to resolve the address.
pub fn open_output_connections(
    dest_names: &[impl AsRef<str>],
) -> HashMap<String, flume::Sender<Vec<u8>>> {
    if dest_names.is_empty() {
        return HashMap::new();
    }
    #[cfg(target_os = "linux")]
    {
        match _open_alsa_direct(dest_names) {
            Ok(map) => {
                tracing::info!(
                    "SeqTerm: direct ALSA output to {} destination(s)",
                    map.len()
                );
                return map;
            }
            Err(e) => {
                tracing::warn!("Direct ALSA output failed ({e})");
            }
        }
    }
    HashMap::new()
}

/// Parse the trailing `"N:M"` ALSA client:port IDs that midir appends to port names.
/// midir format: `"ClientName:PortName ClientID:PortID"`
#[cfg(target_os = "linux")]
fn parse_alsa_addr_from_name(port_name: &str) -> Option<alsa::seq::Addr> {
    let last = port_name.rsplit_once(' ')?.1;
    let (c, p) = last.split_once(':')?;
    Some(alsa::seq::Addr {
        client: c.parse().ok()?,
        port: p.parse().ok()?,
    })
}

/// Enumerate ALSA clients/ports to find an address matching `port_name`.
/// The name is matched against `"ClientName:PortName"` (the part before any trailing IDs).
#[cfg(target_os = "linux")]
fn find_alsa_addr_by_name(seq: &alsa::seq::Seq, port_name: &str) -> Option<alsa::seq::Addr> {
    use alsa::seq::{ClientIter, PortIter};
    // Strip trailing numeric IDs if present ("ClientName:PortName 128:0" → "ClientName:PortName").
    let name_part = if let Some(space) = port_name.rfind(' ') {
        let candidate = &port_name[space + 1..];
        if candidate.contains(':') && candidate.chars().next().map_or(false, |c| c.is_ascii_digit()) {
            &port_name[..space]
        } else {
            port_name
        }
    } else {
        port_name
    };

    let (client_name, port_part) = name_part.split_once(':')?;
    let client_name = client_name.trim();
    let port_part   = port_part.trim();

    for cinfo in ClientIter::new(seq) {
        if cinfo.get_name().map(|n| n == client_name).unwrap_or(false) {
            let cid = cinfo.get_client();
            for pinfo in PortIter::new(seq, cid) {
                if pinfo.get_name().map(|n| n == port_part).unwrap_or(false) {
                    return Some(alsa::seq::Addr { client: cid, port: pinfo.get_port() });
                }
            }
            // Fallback: first readable port of this client.
            if let Some(pinfo) = PortIter::new(seq, cid).next() {
                return Some(alsa::seq::Addr { client: cid, port: pinfo.get_port() });
            }
        }
    }
    None
}

/// Core Linux implementation: one "SeqTerm" ALSA client, direct delivery per destination.
#[cfg(target_os = "linux")]
fn _open_alsa_direct(
    dest_names: &[impl AsRef<str>],
) -> anyhow::Result<HashMap<String, flume::Sender<Vec<u8>>>> {
    use alsa::seq::{MidiEvent, Seq};
    use alsa::Direction;
    use std::ffi::CString;

    let seq = Seq::open(None, Some(Direction::Playback), false)
        .map_err(|e| anyhow::anyhow!("snd_seq_open: {e}"))?;
    seq.set_client_name(CString::new("SeqTerm").unwrap().as_c_str())
        .map_err(|e| anyhow::anyhow!("set_client_name: {e}"))?;

    // Resolve each destination to an ALSA Addr.
    let mut addr_map: HashMap<String, alsa::seq::Addr> = HashMap::new();
    for name_ref in dest_names {
        let name = name_ref.as_ref();
        let addr = parse_alsa_addr_from_name(name)
            .or_else(|| find_alsa_addr_by_name(&seq, name));
        match addr {
            Some(a) => { addr_map.insert(name.to_owned(), a); }
            None    => { tracing::warn!("Cannot resolve ALSA addr for '{name}', skipping"); }
        }
    }
    if addr_map.is_empty() {
        anyhow::bail!("no destinations could be resolved");
    }

    // Dispatch channel: (alsa_addr, bytes).
    let (dispatch_tx, dispatch_rx) = flume::unbounded::<(alsa::seq::Addr, Vec<u8>)>();

    // Per-destination relay threads forward bytes to the dispatch channel.
    let mut result: HashMap<String, flume::Sender<Vec<u8>>> = HashMap::new();
    for name_ref in dest_names {
        let name = name_ref.as_ref();
        if let Some(&addr) = addr_map.get(name) {
            let (key_tx, key_rx) = flume::unbounded::<Vec<u8>>();
            let dtx = dispatch_tx.clone();
            let label = name.chars().take(16).collect::<String>();
            std::thread::Builder::new()
                .name(format!("midi-relay-{label}"))
                .spawn(move || {
                    for bytes in key_rx.iter() {
                        let _ = dtx.send((addr, bytes));
                    }
                })?;
            result.insert(name.to_owned(), key_tx);
        }
    }
    drop(dispatch_tx); // dispatch thread exits when all relay threads do.

    // Single ALSA output thread — owns the Seq, sends events to resolved addresses.
    std::thread::Builder::new()
        .name("seqterm-alsa-out".to_string())
        .spawn(move || {
            let mut encoder = match MidiEvent::new(256) {
                Ok(e) => e,
                Err(e) => { tracing::error!("MidiEvent::new: {e}"); return; }
            };
            encoder.enable_running_status(false);

            for (addr, bytes) in dispatch_rx.iter() {
                let mut buf = bytes.as_slice();
                while !buf.is_empty() {
                    match encoder.encode(buf) {
                        Ok((consumed, Some(mut ev))) => {
                            ev.set_dest(addr);
                            ev.set_direct();
                            let _ = seq.event_output_direct(&mut ev);
                            buf = &buf[consumed..];
                        }
                        Ok((consumed, None)) => { buf = &buf[consumed..]; }
                        Err(e) => { tracing::warn!("MIDI encode: {e}"); break; }
                    }
                }
            }
            drop(seq);
        })?;

    Ok(result)
}

// ─── Linux: single ALSA client, one port per pattern ─────────────────────────

/// Open one "SeqTerm" ALSA sequencer client with one output port per pattern.
/// All encoding/sending is done in a single background thread that owns the Seq.
/// Returns a HashMap of pattern-key → sender.
#[cfg(target_os = "linux")]
fn _create_alsa_multi_port(
    keys: &[&str],
) -> anyhow::Result<HashMap<String, flume::Sender<Vec<u8>>>> {
    use alsa::seq::{MidiEvent, PortCap, PortType, Seq};
    use alsa::Direction;
    use std::ffi::CString;

    let seq = Seq::open(None, Some(Direction::Playback), false)
        .map_err(|e| anyhow::anyhow!("snd_seq_open: {e}"))?;
    seq.set_client_name(
        CString::new("SeqTerm")
            .unwrap()
            .as_c_str(),
    )
    .map_err(|e| anyhow::anyhow!("set_client_name: {e}"))?;

    // (port_id, raw MIDI bytes) channel.
    let (dispatch_tx, dispatch_rx) = flume::unbounded::<(i32, Vec<u8>)>();

    let mut result: HashMap<String, flume::Sender<Vec<u8>>> = HashMap::new();

    for &key in keys {
        let port_id = seq
            .create_simple_port(
                CString::new(key)
                    .unwrap_or_else(|_| CString::new("port").unwrap())
                    .as_c_str(),
                PortCap::READ | PortCap::SUBS_READ,
                PortType::MIDI_GENERIC | PortType::APPLICATION,
            )
            .map_err(|e| anyhow::anyhow!("create_simple_port '{key}': {e}"))?;

        let (key_tx, key_rx) = flume::unbounded::<Vec<u8>>();
        let dtx = dispatch_tx.clone();
        std::thread::Builder::new()
            .name(format!("midi-relay-{key}"))
            .spawn(move || {
                for bytes in key_rx.iter() {
                    let _ = dtx.send((port_id, bytes));
                }
            })?;

        result.insert(key.to_owned(), key_tx);
    }

    // Drop our copy of dispatch_tx so the MIDI thread exits when all senders are dropped.
    drop(dispatch_tx);

    // Single output thread: owns the Seq, converts raw bytes → ALSA seq events.
    std::thread::Builder::new()
        .name("seqterm-alsa-out".to_string())
        .spawn(move || {
            let mut encoder = match MidiEvent::new(256) {
                Ok(e) => e,
                Err(e) => {
                    tracing::error!("MidiEvent::new: {e}");
                    return;
                }
            };
            encoder.enable_running_status(false);

            for (port_id, bytes) in dispatch_rx.iter() {
                let mut buf = bytes.as_slice();
                while !buf.is_empty() {
                    match encoder.encode(buf) {
                        Ok((consumed, Some(mut ev))) => {
                            ev.set_source(port_id);
                            ev.set_subs();
                            ev.set_direct();
                            let _ = seq.event_output_direct(&mut ev);
                            buf = &buf[consumed..];
                        }
                        Ok((consumed, None)) => {
                            // Incomplete MIDI; consumed bytes were buffered in encoder.
                            buf = &buf[consumed..];
                        }
                        Err(e) => {
                            tracing::warn!("MIDI encode error: {e}");
                            break;
                        }
                    }
                }
            }
            drop(seq);
        })?;

    Ok(result)
}

// ─── Non-Linux Unix: one midir virtual port per pattern ──────────────────────

#[cfg(unix)]
fn _create_midir_port(pattern_key: &str) -> anyhow::Result<flume::Sender<Vec<u8>>> {
    let midi_out = midir::MidiOutput::new(pattern_key)?;
    let conn = midi_out
        .create_virtual("MIDI Out")
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    let (tx, rx) = flume::unbounded::<Vec<u8>>();
    let key = pattern_key.to_owned();
    std::thread::Builder::new()
        .name(format!("midi-{key}"))
        .spawn(move || {
            let mut conn = conn;
            for msg in rx.iter() {
                let _ = conn.send(&msg);
            }
        })?;
    Ok(tx)
}

// ─── Port watcher ─────────────────────────────────────────────────────────────

/// Spawn a background thread that polls output ports every `interval`.
/// Sends a new `Vec<String>` only when the list has changed.
/// The receiver stays valid as long as at least one clone exists.
pub fn spawn_port_watcher(interval: std::time::Duration) -> flume::Receiver<Vec<String>> {
    let (tx, rx) = flume::unbounded();
    std::thread::Builder::new()
        .name("midi-port-watcher".to_string())
        .spawn(move || {
            let mut last: Vec<String> = Vec::new();
            loop {
                std::thread::sleep(interval);
                if tx.is_disconnected() { break; }
                let ports = list_output_ports().unwrap_or_default();
                if ports != last {
                    last = ports.clone();
                    if tx.send(ports).is_err() { break; }
                }
            }
        })
        .expect("midi-port-watcher thread");
    rx
}

// ─── Port enumeration ─────────────────────────────────────────────────────────

/// List available MIDI output port names.
/// Tries midir first; if that returns nothing (e.g. ALSA client limit hit),
/// falls back to parsing `aconnect -l` on Linux.
pub fn list_output_ports() -> anyhow::Result<Vec<String>> {
    if let Ok(midi_out) = midir::MidiOutput::new("seqterm-list") {
        let ports: Vec<String> = midi_out
            .ports()
            .iter()
            .filter_map(|p| midi_out.port_name(p).ok())
            .collect();
        if !ports.is_empty() {
            return Ok(ports);
        }
    }
    // Fallback: enumerate via aconnect (doesn't create an ALSA client).
    #[cfg(target_os = "linux")]
    return list_ports_via_aconnect();
    #[cfg(not(target_os = "linux"))]
    Ok(vec![])
}

/// List available MIDI input port names.
/// Tries midir first; falls back to `aconnect -l` on Linux.
pub fn list_input_ports() -> anyhow::Result<Vec<String>> {
    if let Ok(midi_in) = midir::MidiInput::new("seqterm-list") {
        let ports: Vec<String> = midi_in
            .ports()
            .iter()
            .filter_map(|p| midi_in.port_name(p).ok())
            .collect();
        if !ports.is_empty() {
            return Ok(ports);
        }
    }
    // Fallback: same aconnect output contains both in and out.
    #[cfg(target_os = "linux")]
    return list_ports_via_aconnect();
    #[cfg(not(target_os = "linux"))]
    Ok(vec![])
}

/// Parse `aconnect -l` to enumerate all user-space ALSA MIDI ports.
/// This works even when creating a new midir client would exceed the ALSA limit.
/// Returns port strings in the same "ClientName:PortName" format as midir.
#[cfg(target_os = "linux")]
fn list_ports_via_aconnect() -> anyhow::Result<Vec<String>> {
    let output = std::process::Command::new("aconnect")
        .arg("-l")
        .output()
        .map_err(|e| anyhow::anyhow!("aconnect not available: {e}"))?;

    let text = String::from_utf8_lossy(&output.stdout);
    let mut ports = Vec::new();
    let mut client_name = String::new();
    let mut is_user_client = false;

    for line in text.lines() {
        if line.starts_with("client ") {
            // "client N: 'Name' [type=kernel|user,...]"
            is_user_client = line.contains("type=user");
            client_name = extract_quoted(line).unwrap_or_default();
        } else if is_user_client && line.starts_with("    ") {
            // "    N 'Port Name' [...]" — only count user-space clients
            if let Some(port_name) = extract_quoted(line) {
                if !client_name.is_empty() && !client_name.starts_with("seqterm") {
                    ports.push(format!("{}:{}", client_name, port_name));
                }
            }
        }
    }
    Ok(ports)
}

/// Create a virtual MIDI output port, returning a byte-sender for it.
/// On non-Unix platforms this returns an error (not supported).
pub fn _create_midir_virtual_port(name: &str) -> anyhow::Result<flume::Sender<Vec<u8>>> {
    #[cfg(unix)]
    return _create_midir_port(name);
    #[cfg(not(unix))]
    {
        let _ = name;
        anyhow::bail!("virtual MIDI ports are not supported on this platform")
    }
}

/// Extract the first single-quoted string from a line.
fn extract_quoted(line: &str) -> Option<String> {
    let start = line.find('\'')?;
    let rest = &line[start + 1..];
    let end = rest.find('\'')?;
    Some(rest[..end].trim().to_string())
}
