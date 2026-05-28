//! Command bus and event bus — the backbone of the application layer.

use std::sync::Arc;
use parking_lot::Mutex;

use crate::{AppCmd, DomainEvent};

// ─── Command Bus ──────────────────────────────────────────────────────────────

/// Routes commands to registered handlers.
///
/// Usage:
/// ```rust,ignore
/// let bus = CommandBus::new();
/// bus.register(|cmd| { /* handle */ });
/// bus.dispatch(AppCmd::Play);
/// ```
pub struct CommandBus {
    handlers: Vec<Box<dyn Fn(AppCmd) + Send + Sync>>,
}

impl CommandBus {
    pub fn new() -> Self {
        Self { handlers: Vec::new() }
    }

    pub fn register(&mut self, handler: impl Fn(AppCmd) + Send + Sync + 'static) {
        self.handlers.push(Box::new(handler));
    }

    pub fn dispatch(&self, cmd: AppCmd) {
        for h in &self.handlers {
            h(cmd.clone());
        }
    }
}

impl Default for CommandBus {
    fn default() -> Self { Self::new() }
}

// ─── Event Bus ────────────────────────────────────────────────────────────────

/// Broadcasts domain events to all registered listeners.
///
/// Thread-safe (Arc<Mutex> internally).
/// Frontends subscribe via `subscribe()` and receive events on their channel.
#[derive(Clone)]
pub struct EventBus {
    subscribers: Arc<Mutex<Vec<flume::Sender<DomainEvent>>>>,
}

impl EventBus {
    pub fn new() -> Self {
        Self { subscribers: Arc::new(Mutex::new(Vec::new())) }
    }

    /// Register a new subscriber. Returns the receiver end.
    pub fn subscribe(&self) -> flume::Receiver<DomainEvent> {
        let (tx, rx) = flume::unbounded();
        self.subscribers.lock().push(tx);
        rx
    }

    /// Publish an event to all subscribers (dead subscribers are pruned).
    pub fn publish(&self, event: DomainEvent) {
        let mut subs = self.subscribers.lock();
        subs.retain(|tx| tx.send(event.clone()).is_ok());
    }
}

impl Default for EventBus {
    fn default() -> Self { Self::new() }
}
