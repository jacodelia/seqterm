//! Piano-roll mouse-editing tests (Milestone E): Shift+drag rectangular
//! selection and batch delete, driven through the real dispatchers. Assertions
//! target `NoteEvent` output (per the canonical-note decision).

use crossterm::event::KeyCode;
use seqterm_core::{Note, Pattern, Project};
use seqterm_ui::app::ViewKind;
use seqterm_ui::testkit::HeadlessApp;

/// Pattern "P" (8 steps) with notes at steps 0, 1, 4 — pitch MIDI 105 (row 3,
/// near the top of the visible grid).
fn project_with_notes() -> Project {
    let mut proj = Project::blank("test");
    let mut pat = Pattern::new("P", 8);
    for step in [0usize, 1, 4] {
        pat.set_step(step, Note::from_midi(105, 100).unwrap());
    }
    proj.patterns.insert("P".to_string(), pat);
    proj
}

/// Put the app in the piano roll showing pattern "P", rendered so the grid rect
/// is populated.
fn piano_harness() -> HeadlessApp {
    let mut h = HeadlessApp::with_project(project_with_notes());
    h.goto(ViewKind::Tracker);
    h.app_mut().tracker_state.pattern_key = Some("P".to_string());
    h.app_mut().tracker_section = 1; // piano roll
    // Scroll so the high-pitch notes (row 3) sit in the visible window.
    h.app_mut().piano_note_scroll = 0;
    h.render();
    h
}

#[test]
fn shift_drag_selects_then_delete_removes_notes() {
    let mut h = piano_harness();
    let area = h.app().piano_roll_area.get();
    assert!(area.width > 0 && area.height > 0, "piano roll rendered");

    let step_start_x = area.x + 1 + 5; // border + 5-col key label
    let ytop = area.y + 2; // header_row(area.y+1) + 1 = first note row
    let ybot = area.y + area.height - 3; // last grid row above the scrollbar

    // Shift+drag a rectangle covering step columns 0..1 across the visible rows.
    h.mouse_down_shift(step_start_x, ytop)
        .mouse_drag(step_start_x + 2, ybot)
        .mouse_up(step_start_x + 2, ybot);

    let mut sel: Vec<usize> = h.app().piano_selection.iter().copied().collect();
    sel.sort();
    assert_eq!(sel, vec![0, 1], "Shift+drag selects steps 0 and 1, not step 4");

    // Three notes before, one survivor (step 4) after deleting the selection.
    assert_eq!(h.project(|p| p.patterns["P"].to_events().len()), 3);
    h.key(KeyCode::Delete);
    assert!(h.app().piano_selection.is_empty(), "selection cleared after delete");
    assert_eq!(
        h.project(|p| p.patterns["P"].to_events().len()),
        1,
        "only the unselected note at step 4 remains"
    );

    // The batch delete is one undo step.
    h.key_mods(KeyCode::Char('z'), crossterm::event::KeyModifiers::CONTROL);
    assert_eq!(
        h.project(|p| p.patterns["P"].to_events().len()),
        3,
        "Ctrl+Z restores all three notes"
    );
}

#[test]
fn esc_clears_piano_selection() {
    let mut h = piano_harness();
    let area = h.app().piano_roll_area.get();
    let step_start_x = area.x + 1 + 5;
    let ytop = area.y + 2;
    let ybot = area.y + area.height - 3;

    h.mouse_down_shift(step_start_x, ytop)
        .mouse_drag(step_start_x + 2, ybot)
        .mouse_up(step_start_x + 2, ybot);
    assert!(!h.app().piano_selection.is_empty());

    h.key(KeyCode::Esc);
    assert!(h.app().piano_selection.is_empty(), "Esc clears the selection");
    // Notes are untouched by Esc.
    assert_eq!(h.project(|p| p.patterns["P"].to_events().len()), 3);
}
