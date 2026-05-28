use std::sync::Arc;

use anyhow::Result;
use parking_lot::Mutex;
use tracing::{debug, warn};

use crate::MidiMessage;

/// Abstraction over a MIDI output port.
pub trait MidiOutput: Send {
    fn send(&mut self, msg: &MidiMessage) -> Result<()>;
    fn name(&self) -> &str;
}

/// Abstraction over a MIDI input port.
pub trait MidiInput: Send {
    fn poll(&mut self) -> Option<MidiMessage>;
    fn name(&self) -> &str;
}

/// Routes MIDI messages between inputs, outputs, and internal buses.
pub struct MidiRouter {
    outputs: Vec<Box<dyn MidiOutput>>,
    /// Pending outgoing messages.
    queue: Vec<MidiMessage>,
    /// Channel filter (None = pass all).
    channel_filter: Option<u8>,
}

impl MidiRouter {
    pub fn new() -> Self {
        Self {
            outputs: Vec::new(),
            queue: Vec::new(),
            channel_filter: None,
        }
    }

    /// Register a MIDI output port.
    pub fn add_output(&mut self, output: Box<dyn MidiOutput>) {
        debug!("MIDI output added: {}", output.name());
        self.outputs.push(output);
    }

    /// Enqueue a message for sending.
    pub fn send(&mut self, msg: MidiMessage) {
        self.queue.push(msg);
    }

    /// Flush all queued messages to all registered outputs.
    pub fn flush(&mut self) {
        for msg in self.queue.drain(..) {
            for output in &mut self.outputs {
                if let Err(e) = output.send(&msg) {
                    warn!("MIDI send error on {}: {e}", output.name());
                }
            }
        }
    }

    pub fn set_channel_filter(&mut self, channel: Option<u8>) {
        self.channel_filter = channel;
    }
}

impl Default for MidiRouter {
    fn default() -> Self {
        Self::new()
    }
}

/// A thread-safe, shared MIDI router.
pub type SharedMidiRouter = Arc<Mutex<MidiRouter>>;

/// Create a shared MIDI router.
pub fn shared_router() -> SharedMidiRouter {
    Arc::new(Mutex::new(MidiRouter::new()))
}
