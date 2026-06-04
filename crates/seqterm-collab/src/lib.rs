//! # SeqTerm Collaboration Engine
//!
//! Provides:
//! - **Object-level merge** — UUID-based three-way diff for project objects
//! - **CRDT delta operations** — Lamport-timestamped grow-only sets and LWW registers
//! - **WebSocket session** — real-time collaboration server/client (behind `websocket` feature)

pub mod crdt;
pub mod merge;

#[cfg(feature = "websocket")]
pub mod session;
