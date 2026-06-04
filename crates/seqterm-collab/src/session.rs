//! WebSocket collaboration session (requires `websocket` feature + tokio runtime).
//!
//! ## Architecture
//!
//! ```text
//! ┌──────────────┐   DeltaOp (JSON)   ┌──────────────┐
//! │  Peer A      │ ─────────────────▶ │  Peer B      │
//! │  ColabClient │ ◀───────────────── │  ColabClient │
//! └──────────────┘   ws://host:port   └──────────────┘
//! ```
//!
//! Each peer runs a `CollabClient` that:
//! 1. Connects to a central `CollabServer` (or peer-to-peer via TURN).
//! 2. Sends local `DeltaOp` operations as JSON WebSocket text frames.
//! 3. Receives remote `DeltaOp` operations and applies them to the project.
//! 4. On reconnect, requests ops since its last known timestamp (catch-up).
//!
//! ## Usage
//!
//! ```rust,ignore
//! use seqterm_collab::session::{CollabServer, CollabClient};
//!
//! // Server side:
//! tokio::spawn(async { CollabServer::run("0.0.0.0:7777").await.unwrap(); });
//!
//! // Client side:
//! let (client, rx) = CollabClient::connect("ws://localhost:7777").await?;
//! client.send_op(my_delta_op).await?;
//! while let Some(op) = rx.recv().await {
//!     apply_op_to_project(&op);
//! }
//! ```

use anyhow::Result;
use crate::crdt::{DeltaOp, OpLog};
use std::sync::Arc;
use tokio::sync::{broadcast, Mutex};
use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::{
    accept_async, connect_async,
    tungstenite::Message,
};

// ─── Protocol message ─────────────────────────────────────────────────────────

/// Wire protocol messages between peers.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ColabMsg {
    /// A single CRDT operation.
    Op(DeltaOp),
    /// Request all ops since `(ts, site_id)` — sent on reconnect.
    CatchUp { since_ts: u64, since_site: uuid::Uuid },
    /// Response to CatchUp: a batch of ops.
    Batch { ops: Vec<DeltaOp> },
    /// Ping / keep-alive.
    Ping,
    /// Pong.
    Pong,
}

// ─── Collaboration Server ─────────────────────────────────────────────────────

/// A simple central relay server that broadcasts ops to all connected peers.
///
/// The server does not apply ops itself — it only stores them in its `OpLog`
/// for catch-up and relays them to all other connected peers.
pub struct CollabServer {
    log: Arc<Mutex<OpLog>>,
    tx:  broadcast::Sender<DeltaOp>,
}

impl CollabServer {
    /// Start listening on `addr` (e.g. `"0.0.0.0:7777"`).
    pub async fn run(addr: &str) -> Result<()> {
        let log: Arc<Mutex<OpLog>> = Arc::new(Mutex::new(OpLog::new()));
        let (tx, _) = broadcast::channel::<DeltaOp>(1024);

        let listener = tokio::net::TcpListener::bind(addr).await?;
        tracing::info!("Collaboration server listening on {addr}");

        loop {
            let (stream, peer_addr) = listener.accept().await?;
            tracing::info!("Peer connected: {peer_addr}");
            let log2 = Arc::clone(&log);
            let tx2  = tx.clone();
            let rx2  = tx.subscribe();
            tokio::spawn(async move {
                if let Err(e) = handle_peer(stream, log2, tx2, rx2).await {
                    tracing::warn!("Peer {peer_addr} error: {e}");
                }
            });
        }
    }
}

async fn handle_peer(
    stream:  tokio::net::TcpStream,
    log:     Arc<Mutex<OpLog>>,
    tx:      broadcast::Sender<DeltaOp>,
    mut rx:  broadcast::Receiver<DeltaOp>,
) -> Result<()> {
    let ws = accept_async(stream).await?;
    let (mut ws_tx, mut ws_rx) = ws.split();

    loop {
        tokio::select! {
            // Incoming from peer.
            msg = ws_rx.next() => {
                let msg = match msg {
                    Some(Ok(m)) => m,
                    _ => break,
                };
                if let Message::Text(text) = msg {
                    if let Ok(colab_msg) = serde_json::from_str::<ColabMsg>(&text) {
                        match colab_msg {
                            ColabMsg::Op(op) => {
                                log.lock().await.push(op.clone());
                                let _ = tx.send(op);
                            }
                            ColabMsg::CatchUp { since_ts, since_site } => {
                                let log = log.lock().await;
                                let batch = log.since(since_ts, &since_site)
                                    .into_iter().cloned().collect();
                                let reply = ColabMsg::Batch { ops: batch };
                                let text = serde_json::to_string(&reply)?;
                                ws_tx.send(Message::Text(text)).await?;
                            }
                            ColabMsg::Ping => {
                                ws_tx.send(Message::Text(
                                    serde_json::to_string(&ColabMsg::Pong)?
                                )).await?;
                            }
                            _ => {}
                        }
                    }
                }
            }
            // Broadcast from another peer.
            op = rx.recv() => {
                if let Ok(op) = op {
                    let text = serde_json::to_string(&ColabMsg::Op(op))?;
                    ws_tx.send(Message::Text(text)).await?;
                }
            }
        }
    }
    Ok(())
}

// ─── Collaboration Client ─────────────────────────────────────────────────────

/// Client handle for sending and receiving CRDT operations over WebSocket.
pub struct CollabClient {
    tx: tokio::sync::mpsc::Sender<DeltaOp>,
}

impl CollabClient {
    /// Connect to a collaboration server.
    ///
    /// Returns a `(CollabClient, Receiver<DeltaOp>)` pair.
    /// Incoming ops are sent to the receiver; outgoing ops are sent via the client.
    pub async fn connect(
        url: &str,
    ) -> Result<(Self, tokio::sync::mpsc::Receiver<DeltaOp>)> {
        let (ws, _) = connect_async(url).await?;
        let (mut ws_tx, mut ws_rx) = ws.split();

        let (out_tx, mut out_rx) = tokio::sync::mpsc::channel::<DeltaOp>(256);
        let (in_tx, in_rx)       = tokio::sync::mpsc::channel::<DeltaOp>(256);

        tokio::spawn(async move {
            loop {
                tokio::select! {
                    // Send outgoing ops.
                    op = out_rx.recv() => {
                        if let Some(op) = op {
                            let text = match serde_json::to_string(&ColabMsg::Op(op)) {
                                Ok(t) => t,
                                Err(_) => continue,
                            };
                            if ws_tx.send(Message::Text(text)).await.is_err() { break; }
                        } else {
                            break;
                        }
                    }
                    // Receive incoming ops.
                    msg = ws_rx.next() => {
                        match msg {
                            Some(Ok(Message::Text(text))) => {
                                if let Ok(ColabMsg::Op(op)) = serde_json::from_str(&text) {
                                    let _ = in_tx.send(op).await;
                                }
                            }
                            None | Some(Err(_)) => break,
                            _ => {}
                        }
                    }
                }
            }
        });

        Ok((Self { tx: out_tx }, in_rx))
    }

    /// Send a CRDT operation to the server (and thus to all peers).
    pub async fn send_op(&self, op: DeltaOp) -> Result<()> {
        self.tx.send(op).await.map_err(|e| anyhow::anyhow!("{e}"))
    }
}
