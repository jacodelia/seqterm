//! Headless application harness (Milestone D).
//!
//! Drives a **real [`App`]** — the same `handle_key` / `handle_mouse` dispatchers
//! the live event loop calls — without a terminal or audio hardware, so workflow
//! tests can send input and inspect the resulting state. Per the canonical-note
//! decision (`docs/rational-storage.md`), assertions should target rational
//! output (`NoteEvent` / the `Arrangement` model) rather than grid cells.
//!
//! ```ignore
//! let mut h = HeadlessApp::new();
//! h.goto(ViewKind::Arranger).ch('g').ch('t');
//! assert_eq!(h.project(|p| p.arrangement.tracks.len()), 1);
//! ```

use std::sync::Arc;

use crossterm::event::{
    KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use parking_lot::Mutex;
use seqterm_core::Project;
use seqterm_engine::PlaybackEngine;

use crate::app::{App, ViewKind};

/// A real [`App`] wired to a flume-backed scheduler (no audio device, no TTY).
pub struct HeadlessApp {
    pub app: App,
}

impl Default for HeadlessApp {
    fn default() -> Self {
        Self::new()
    }
}

impl HeadlessApp {
    /// Build a harness around a blank project.
    pub fn new() -> Self {
        Self::with_project(Project::blank("test"))
    }

    /// Build a harness around a specific project.
    pub fn with_project(project: Project) -> Self {
        let project = Arc::new(Mutex::new(project));
        let engine = PlaybackEngine::start(Arc::clone(&project));
        let mut app = App::new(project, engine);
        // Skip the startup splash so the first render lays out the real UI.
        app.splash_state.showing = false;
        Self { app }
    }

    // ── Input ────────────────────────────────────────────────────────────────

    /// Send a key with explicit modifiers through the real dispatcher.
    pub fn key_mods(&mut self, code: KeyCode, mods: KeyModifiers) -> &mut Self {
        crate::handle_key(&mut self.app, KeyEvent::new(code, mods));
        self
    }

    /// Send a bare key (no modifiers).
    pub fn key(&mut self, code: KeyCode) -> &mut Self {
        self.key_mods(code, KeyModifiers::NONE)
    }

    /// Send a single character key.
    pub fn ch(&mut self, c: char) -> &mut Self {
        self.key(KeyCode::Char(c))
    }

    /// Send each character of `s` as its own key press.
    pub fn chars(&mut self, s: &str) -> &mut Self {
        for c in s.chars() {
            self.ch(c);
        }
        self
    }

    pub fn enter(&mut self) -> &mut Self {
        self.key(KeyCode::Enter)
    }

    pub fn esc(&mut self) -> &mut Self {
        self.key(KeyCode::Esc)
    }

    /// Send a left-mouse event of `kind` at `(col, row)` with modifiers.
    fn mouse(&mut self, kind: MouseEventKind, col: u16, row: u16, mods: KeyModifiers) -> &mut Self {
        crate::handle_mouse(
            &mut self.app,
            MouseEvent { kind, column: col, row, modifiers: mods },
        );
        self
    }

    /// Press the left button at `(col, row)`.
    pub fn mouse_down(&mut self, col: u16, row: u16) -> &mut Self {
        self.mouse(MouseEventKind::Down(MouseButton::Left), col, row, KeyModifiers::NONE)
    }

    /// Press the left button with Alt held (Alt+Drag duplicate).
    pub fn mouse_down_alt(&mut self, col: u16, row: u16) -> &mut Self {
        self.mouse(MouseEventKind::Down(MouseButton::Left), col, row, KeyModifiers::ALT)
    }

    /// Press the left button with Shift held (rectangular selection).
    pub fn mouse_down_shift(&mut self, col: u16, row: u16) -> &mut Self {
        self.mouse(MouseEventKind::Down(MouseButton::Left), col, row, KeyModifiers::SHIFT)
    }

    /// Drag (button held) to `(col, row)`.
    pub fn mouse_drag(&mut self, col: u16, row: u16) -> &mut Self {
        self.mouse(MouseEventKind::Drag(MouseButton::Left), col, row, KeyModifiers::NONE)
    }

    /// Release the left button at `(col, row)`.
    pub fn mouse_up(&mut self, col: u16, row: u16) -> &mut Self {
        self.mouse(MouseEventKind::Up(MouseButton::Left), col, row, KeyModifiers::NONE)
    }

    /// One full left-click (down + up) at `(col, row)`.
    pub fn click(&mut self, col: u16, row: u16) -> &mut Self {
        self.mouse_down(col, row).mouse_up(col, row)
    }

    /// Render once into an off-screen `TestBackend` of the given size so layout
    /// caches (panel rects used for mouse hit-testing) are populated.
    pub fn render_sized(&mut self, w: u16, h: u16) -> &mut Self {
        use ratatui::{backend::TestBackend, Terminal};
        let mut term = Terminal::new(TestBackend::new(w, h)).expect("test terminal");
        let app = &mut self.app;
        term.draw(|f| crate::ui(f, app)).expect("headless draw");
        self
    }

    /// Render at a default 120×40 size.
    pub fn render(&mut self) -> &mut Self {
        self.render_sized(120, 40)
    }

    /// The cached arrangement tracks-panel rect (valid after [`render`]).
    pub fn arranger_panel(&self) -> ratatui::layout::Rect {
        self.app.arranger_panel_rects.get()[0]
    }

    /// Switch the active view (convenience; mirrors `App::switch_view`).
    pub fn goto(&mut self, view: ViewKind) -> &mut Self {
        self.app.switch_view(view);
        self
    }

    // ── Inspection ─────────────────────────────────────────────────────────────

    /// Read the project under lock.
    pub fn project<R>(&self, f: impl FnOnce(&Project) -> R) -> R {
        f(&self.app.project.lock())
    }

    /// Total clips across all arrangement tracks/lanes.
    pub fn arrangement_clip_count(&self) -> usize {
        self.project(|p| {
            p.arrangement
                .tracks
                .iter()
                .flat_map(|t| t.lanes.iter())
                .map(|l| l.clips.len())
                .sum()
        })
    }

    pub fn app(&self) -> &App {
        &self.app
    }

    pub fn app_mut(&mut self) -> &mut App {
        &mut self.app
    }
}
