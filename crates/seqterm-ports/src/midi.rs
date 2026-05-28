//! MIDI backend port.

use anyhow::Result;

/// A raw MIDI message (up to 3 bytes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MidiMessage {
    pub data: [u8; 3],
    pub len: u8,
}

impl MidiMessage {
    pub fn note_on(channel: u8, note: u8, vel: u8) -> Self {
        Self { data: [0x90 | (channel & 0x0F), note & 0x7F, vel & 0x7F], len: 3 }
    }
    pub fn note_off(channel: u8, note: u8) -> Self {
        Self { data: [0x80 | (channel & 0x0F), note & 0x7F, 0], len: 3 }
    }
    pub fn control_change(channel: u8, cc: u8, value: u8) -> Self {
        Self { data: [0xB0 | (channel & 0x0F), cc & 0x7F, value & 0x7F], len: 3 }
    }
    pub fn bytes(&self) -> &[u8] {
        &self.data[..self.len as usize]
    }
}

/// Describes a MIDI device.
#[derive(Debug, Clone)]
pub struct MidiDeviceInfo {
    pub name: String,
    pub is_input: bool,
    pub is_output: bool,
    pub is_virtual: bool,
}

/// Callback type for incoming MIDI messages.
pub type MidiInputCallback = Box<dyn Fn(u64, MidiMessage) + Send + 'static>;

/// Port: MIDI I/O backend.
/// Implemented by MidirMidiAdapter, AlsaMidiAdapter, CoreMidiAdapter, etc.
pub trait MidiBackendPort: Send + Sync {
    /// List all available MIDI devices.
    fn list_devices(&self) -> Vec<MidiDeviceInfo>;

    /// Open an output connection to a named port.
    fn open_output(&mut self, port_name: &str) -> Result<()>;

    /// Send a raw MIDI message to a named output port.
    fn send(&self, port_name: &str, msg: MidiMessage) -> Result<()>;

    /// Open an input port and register a callback for incoming messages.
    fn open_input(&mut self, port_name: &str, callback: MidiInputCallback) -> Result<()>;

    /// Close a port by name.
    fn close_port(&mut self, port_name: &str);

    /// Create a virtual MIDI port (loopback / IAC).
    fn create_virtual_output(&mut self, name: &str) -> Result<()>;

    /// Whether a named port is currently open.
    fn is_port_open(&self, port_name: &str) -> bool;
}
