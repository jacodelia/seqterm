//! Minimal OSC (Open Sound Control) server for SeqTerm.
//!
//! Listens on UDP and converts incoming OSC messages to `OscMsg` events.
//! Supports the route table defined in `Project::osc_routes`.

use std::net::UdpSocket;

use rosc::{OscMessage, OscPacket, OscType, decoder};

/// Decoded OSC event forwarded to the App event loop.
#[derive(Debug, Clone)]
pub enum OscMsg {
    Play,
    Stop,
    SetBpm(f64),
    /// channel 0-indexed, linear gain 0.0–2.0
    SetChannelVolume { channel: usize, gain: f32 },
    /// Raw address + args for extension / custom routes.
    Custom { address: String, args: Vec<OscArg> },
}

/// Simplified OSC argument type (subset of the full spec).
#[derive(Debug, Clone)]
pub enum OscArg {
    Int(i32),
    Float(f32),
    String(String),
    Bool(bool),
}

impl From<OscType> for OscArg {
    fn from(t: OscType) -> Self {
        match t {
            OscType::Int(i)   => OscArg::Int(i),
            OscType::Float(f) => OscArg::Float(f),
            OscType::String(s) => OscArg::String(s),
            OscType::Bool(b)  => OscArg::Bool(b),
            OscType::Double(d) => OscArg::Float(d as f32),
            OscType::Long(l)  => OscArg::Int(l as i32),
            _ => OscArg::Int(0),
        }
    }
}

/// Background UDP/OSC listener.
///
/// Call [`OscServer::start`] to spawn the listener thread.
/// Received messages are forwarded via the returned `flume::Receiver<OscMsg>`.
pub struct OscServer;

impl OscServer {
    /// Spawn a background thread listening on `udp_port`.
    /// Returns a receiver that yields decoded [`OscMsg`] events.
    /// The thread exits automatically when the receiver is dropped.
    pub fn start(udp_port: u16) -> anyhow::Result<flume::Receiver<OscMsg>> {
        let addr = format!("0.0.0.0:{udp_port}");
        let socket = UdpSocket::bind(&addr)
            .map_err(|e| anyhow::anyhow!("OSC bind {addr}: {e}"))?;
        // Non-blocking with 100ms timeout so the thread can notice channel close.
        socket.set_read_timeout(Some(std::time::Duration::from_millis(100)))?;

        let (tx, rx) = flume::unbounded::<OscMsg>();
        std::thread::Builder::new()
            .name("seqterm-osc".into())
            .spawn(move || {
                tracing::info!("OSC server listening on UDP :{udp_port}");
                let mut buf = [0u8; 4096];
                loop {
                    if tx.is_disconnected() { break; }
                    match socket.recv_from(&mut buf) {
                        Ok((n, _src)) => {
                            if let Ok((_, packet)) = decoder::decode_udp(&buf[..n]) {
                                dispatch_packet(&tx, packet);
                            }
                        }
                        Err(ref e)
                            if e.kind() == std::io::ErrorKind::WouldBlock
                                || e.kind() == std::io::ErrorKind::TimedOut => {}
                        Err(e) => {
                            tracing::warn!("OSC recv error: {e}");
                        }
                    }
                }
                tracing::info!("OSC server stopped");
            })
            .map_err(|e| anyhow::anyhow!("spawn osc thread: {e}"))?;

        Ok(rx)
    }
}

fn dispatch_packet(tx: &flume::Sender<OscMsg>, packet: OscPacket) {
    match packet {
        OscPacket::Message(msg) => {
            if let Some(ev) = decode_msg(msg) {
                let _ = tx.send(ev);
            }
        }
        OscPacket::Bundle(bundle) => {
            for p in bundle.content {
                dispatch_packet(tx, p);
            }
        }
    }
}

fn decode_msg(msg: OscMessage) -> Option<OscMsg> {
    let addr = msg.addr.as_str();
    let args = msg.args;

    match addr {
        "/seq/play"  => return Some(OscMsg::Play),
        "/seq/stop"  => return Some(OscMsg::Stop),
        "/seq/bpm" => {
            let bpm = first_number(&args)?;
            return Some(OscMsg::SetBpm(bpm as f64));
        }
        _ => {}
    }

    // /mixer/vol/<n>  <gain 0.0–2.0>
    if let Some(rest) = addr.strip_prefix("/mixer/vol/") {
        if let Ok(ch) = rest.parse::<usize>() {
            let gain = first_number(&args).unwrap_or(1.0);
            return Some(OscMsg::SetChannelVolume { channel: ch, gain });
        }
    }

    // Fall through: emit a Custom event so integrators can handle it.
    let osc_args: Vec<OscArg> = args.into_iter().map(OscArg::from).collect();
    Some(OscMsg::Custom { address: addr.to_string(), args: osc_args })
}

fn first_number(args: &[OscType]) -> Option<f32> {
    args.first().and_then(|a| match a {
        OscType::Float(f)  => Some(*f),
        OscType::Double(d) => Some(*d as f32),
        OscType::Int(i)    => Some(*i as f32),
        OscType::Long(l)   => Some(*l as f32),
        _ => None,
    })
}
