//! MidiInputBus — multiplexes multiple open MIDI input ports into one receiver channel.

use std::collections::HashMap;
use crate::MidiMessage;

/// Owns a set of open MIDI input ports and delivers every incoming message
/// through a single `(port_name, MidiMessage)` receiver.
///
/// Ports are opened on demand and closed explicitly (or on drop of this struct).
pub struct MidiInputBus {
    /// Maps port_name → close_tx.  Dropping the sender signals the keeper
    /// thread to exit, which drops the `MidiInputConnection` and closes the port.
    open_ports: HashMap<String, flume::Sender<()>>,
    rx: flume::Receiver<(String, MidiMessage)>,
    tx: flume::Sender<(String, MidiMessage)>,
}

impl Default for MidiInputBus {
    fn default() -> Self { Self::new() }
}

impl MidiInputBus {
    pub fn new() -> Self {
        let (tx, rx) = flume::unbounded();
        Self { open_ports: HashMap::new(), rx, tx }
    }

    /// Open a MIDI input port and start forwarding its messages to the bus.
    /// No-op if the port is already open.
    pub fn open_port(&mut self, name: &str) -> anyhow::Result<()> {
        if self.open_ports.contains_key(name) {
            return Ok(());
        }
        let midi_in = midir::MidiInput::new("seqterm-in-bus")
            .map_err(|e| anyhow::anyhow!("MidiInput::new: {e}"))?;
        let port = midi_in
            .ports()
            .into_iter()
            .find(|p| midi_in.port_name(p).as_deref() == Ok(name))
            .ok_or_else(|| anyhow::anyhow!("MIDI input not found: {name}"))?;

        let bus_tx   = self.tx.clone();
        let port_name = name.to_owned();
        let (close_tx, close_rx) = flume::bounded::<()>(0);

        let conn = midi_in
            .connect(
                &port,
                "seqterm-in-bus",
                move |_ts, data, _| {
                    if let Some(msg) = MidiMessage::from_bytes(data) {
                        let _ = bus_tx.send((port_name.clone(), msg));
                    }
                },
                (),
            )
            .map_err(|e| anyhow::anyhow!("connect '{name}': {e}"))?;

        let label = name.chars().take(24).collect::<String>();
        std::thread::Builder::new()
            .name(format!("midi-in-bus-{label}"))
            .spawn(move || {
                let _conn = conn; // keep connection alive
                let _ = close_rx.recv(); // block until close_tx is dropped
            })?;

        self.open_ports.insert(name.to_owned(), close_tx);
        tracing::info!("MIDI input bus: opened '{name}'");
        Ok(())
    }

    /// Close a previously opened port.  No-op if not open.
    pub fn close_port(&mut self, name: &str) {
        if self.open_ports.remove(name).is_some() {
            tracing::info!("MIDI input bus: closed '{name}'");
        }
    }

    /// Returns `true` if the named port is currently open.
    pub fn is_open(&self, name: &str) -> bool {
        self.open_ports.contains_key(name)
    }

    /// Non-blocking drain — returns the next queued `(port_name, message)`, if any.
    pub fn try_recv(&self) -> Option<(String, MidiMessage)> {
        self.rx.try_recv().ok()
    }
}
