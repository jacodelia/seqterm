//! MidirMidiAdapter — implements seqterm_ports::MidiBackendPort via midir.

use std::collections::{HashMap, HashSet};
use anyhow::{Context, Result};
use parking_lot::Mutex;
use seqterm_ports::{MidiBackendPort, MidiDeviceInfo, MidiMessage, MidiInputCallback};

pub struct MidirMidiAdapter {
    /// Named output channels (key = port name, value = byte sender to the output thread).
    outputs: Mutex<HashMap<String, flume::Sender<Vec<u8>>>>,
    /// Names of open input connections.
    open_inputs: Mutex<HashSet<String>>,
    /// Names of virtual outputs (to distinguish them in is_port_open).
    virtual_outputs: Mutex<HashSet<String>>,
}

impl MidirMidiAdapter {
    pub fn new() -> Self {
        Self {
            outputs: Mutex::new(HashMap::new()),
            open_inputs: Mutex::new(HashSet::new()),
            virtual_outputs: Mutex::new(HashSet::new()),
        }
    }
}

impl Default for MidirMidiAdapter {
    fn default() -> Self { Self::new() }
}

impl MidiBackendPort for MidirMidiAdapter {
    fn list_devices(&self) -> Vec<MidiDeviceInfo> {
        let mut devices = Vec::new();
        if let Ok(out) = midir::MidiOutput::new("seqterm-list-out") {
            for p in out.ports() {
                if let Ok(name) = out.port_name(&p) {
                    devices.push(MidiDeviceInfo {
                        name,
                        is_input: false,
                        is_output: true,
                        is_virtual: false,
                    });
                }
            }
        }
        if let Ok(inp) = midir::MidiInput::new("seqterm-list-in") {
            for p in inp.ports() {
                if let Ok(name) = inp.port_name(&p) {
                    devices.push(MidiDeviceInfo {
                        name,
                        is_input: true,
                        is_output: false,
                        is_virtual: false,
                    });
                }
            }
        }
        devices
    }

    fn open_output(&mut self, port_name: &str) -> Result<()> {
        let mut outputs = self.outputs.lock();
        if outputs.contains_key(port_name) {
            return Ok(());
        }
        let midi_out = midir::MidiOutput::new("seqterm-out")
            .context("failed to create MidiOutput")?;
        let port = midi_out
            .ports()
            .into_iter()
            .find(|p| midi_out.port_name(p).as_deref() == Ok(port_name))
            .with_context(|| format!("MIDI output port not found: {port_name}"))?;
        let conn = midi_out
            .connect(&port, "seqterm-conn")
            .map_err(|e| anyhow::anyhow!("connect to '{port_name}': {e}"))?;

        let (tx, rx) = flume::unbounded::<Vec<u8>>();
        let label = port_name.chars().take(24).collect::<String>();
        std::thread::Builder::new()
            .name(format!("midi-out-{label}"))
            .spawn(move || {
                let mut conn = conn;
                for bytes in rx.iter() {
                    let _ = conn.send(&bytes);
                }
            })?;
        outputs.insert(port_name.to_owned(), tx);
        tracing::info!("MIDI output opened: {port_name}");
        Ok(())
    }

    fn send(&self, port_name: &str, msg: MidiMessage) -> Result<()> {
        let outputs = self.outputs.lock();
        let tx = outputs
            .get(port_name)
            .with_context(|| format!("MIDI output not open: {port_name}"))?;
        tx.send(msg.bytes().to_vec())
            .map_err(|_| anyhow::anyhow!("MIDI output thread dead: {port_name}"))
    }

    fn open_input(&mut self, port_name: &str, callback: MidiInputCallback) -> Result<()> {
        let mut open_inputs = self.open_inputs.lock();
        if open_inputs.contains(port_name) {
            return Ok(());
        }
        let midi_in = midir::MidiInput::new("seqterm-in")
            .context("failed to create MidiInput")?;
        let port = midi_in
            .ports()
            .into_iter()
            .find(|p| midi_in.port_name(p).as_deref() == Ok(port_name))
            .with_context(|| format!("MIDI input port not found: {port_name}"))?;
        // The connection must stay alive; park it in a thread that never exits
        // until the sender is dropped.
        let (alive_tx, alive_rx) = flume::bounded::<()>(0);
        let _conn = midi_in
            .connect(
                &port,
                "seqterm-in-conn",
                move |ts, data, _| {
                    if let Some(msg) = raw_to_ports_msg(data) {
                        callback(ts, msg);
                    }
                },
                (),
            )
            .map_err(|e| anyhow::anyhow!("connect input '{port_name}': {e}"))?;
        let label = port_name.chars().take(24).collect::<String>();
        std::thread::Builder::new()
            .name(format!("midi-in-{label}"))
            .spawn(move || {
                let _conn = _conn;
                // Block until the sender is dropped (port closed).
                let _ = alive_rx.recv();
            })?;
        // alive_tx is intentionally NOT stored — the thread parks until
        // the label's entry is removed in close_port which drops the connection.
        // Store it in the open_inputs set just to prevent double-open.
        drop(alive_tx);
        open_inputs.insert(port_name.to_owned());
        tracing::info!("MIDI input opened: {port_name}");
        Ok(())
    }

    fn close_port(&mut self, port_name: &str) {
        self.outputs.lock().remove(port_name);
        self.open_inputs.lock().remove(port_name);
        self.virtual_outputs.lock().remove(port_name);
        tracing::info!("MIDI port closed: {port_name}");
    }

    fn create_virtual_output(&mut self, name: &str) -> Result<()> {
        let mut outputs = self.outputs.lock();
        if outputs.contains_key(name) {
            return Ok(());
        }
        let tx = crate::_create_midir_virtual_port(name)?;
        outputs.insert(name.to_owned(), tx);
        self.virtual_outputs.lock().insert(name.to_owned());
        tracing::info!("MIDI virtual output created: {name}");
        Ok(())
    }

    fn is_port_open(&self, port_name: &str) -> bool {
        self.outputs.lock().contains_key(port_name)
            || self.open_inputs.lock().contains(port_name)
    }
}

fn raw_to_ports_msg(data: &[u8]) -> Option<MidiMessage> {
    if data.is_empty() {
        return None;
    }
    let len = data.len().min(3) as u8;
    let mut arr = [0u8; 3];
    arr[..len as usize].copy_from_slice(&data[..len as usize]);
    Some(MidiMessage { data: arr, len })
}
