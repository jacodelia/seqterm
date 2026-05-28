//! # SeqTerm Application Layer
//!
//! This crate contains the **application layer** of SeqTerm's hexagonal architecture.
//!
//! ## Responsibilities
//!
//! - **Use cases**: high-level operations (PlayUseCase, LoadProjectUseCase, etc.)
//! - **Command bus**: routes AppCommand values to their use-case handlers
//! - **Event bus**: broadcasts domain events to all registered listeners
//! - **State coordination**: coordinates between engine, persistence, and UI ports
//!
//! ## What this crate MUST NOT contain
//!
//! - Terminal/UI code (ratatui, crossterm)
//! - Audio I/O (cpal, jack)
//! - Filesystem access (except via ProjectRepository port)
//! - MIDI I/O (except via MidiBackendPort)

pub mod commands;
pub mod events;
pub mod bus;
pub mod use_cases;
pub mod plugin_registry;

pub use commands::AppCmd;
pub use events::DomainEvent;
pub use bus::{CommandBus, EventBus};
pub use plugin_registry::{PluginRegistry, PluginInstance, InstanceState};
