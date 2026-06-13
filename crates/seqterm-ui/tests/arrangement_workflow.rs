//! End-to-end arrangement workflow tests driven through the real key/mouse
//! dispatchers via the headless harness (Milestone D). Assertions target the
//! rational `Arrangement` model / `NoteEvent` output, not grid cells.

use crossterm::event::KeyCode;
use seqterm_core::{Note, Pattern, Project};
use seqterm_ui::app::ViewKind;
use seqterm_ui::testkit::HeadlessApp;

/// A blank project carrying one pattern "P" with a note at step 0.
fn project_with_pattern() -> Project {
    let mut proj = Project::blank("test");
    let mut pat = Pattern::new("P", 4);
    pat.set_step(0, Note::from_midi(60, 100).unwrap());
    proj.patterns.insert("P".to_string(), pat);
    proj
}

#[test]
fn create_track_and_clip_via_keys() {
    let mut h = HeadlessApp::with_project(project_with_pattern());
    h.goto(ViewKind::Arranger).ch('g'); // enter the rational timeline

    // No tracks yet.
    assert_eq!(h.project(|p| p.arrangement.tracks.len()), 0);

    // `t` adds a track; `n` then Enter places the first pattern as a clip.
    h.ch('t');
    assert_eq!(h.project(|p| p.arrangement.tracks.len()), 1);

    h.ch('n').enter();
    assert_eq!(h.arrangement_clip_count(), 1, "clip created via pattern picker");

    // The clip references pattern "P".
    let key = h.project(|p| {
        p.arrangement.tracks[0].lanes[0].clips[0]
            .kind
            .pattern_key()
            .map(str::to_string)
    });
    assert_eq!(key.as_deref(), Some("P"));
}

#[test]
fn duplicate_and_delete_clip_via_keys() {
    let mut h = HeadlessApp::with_project(project_with_pattern());
    h.goto(ViewKind::Arranger).ch('g').ch('t').ch('n').enter();
    assert_eq!(h.arrangement_clip_count(), 1);

    h.ch('d'); // duplicate the cursor clip
    assert_eq!(h.arrangement_clip_count(), 2);

    h.ch('x'); // delete the cursor clip
    assert_eq!(h.arrangement_clip_count(), 1);
}

#[test]
fn route_and_toggle_playback_via_keys() {
    let mut h = HeadlessApp::with_project(project_with_pattern());
    h.goto(ViewKind::Arranger).ch('g').ch('t').ch('n').enter();

    // Unrouted by default.
    assert_eq!(h.project(|p| p.arrangement.tracks[0].source_row.clone()), None);

    // `R` cycles the route None → A.
    h.ch('R');
    assert_eq!(
        h.project(|p| p.arrangement.tracks[0].source_row.clone()),
        Some("A".to_string())
    );

    // `P` toggles the playback flag.
    assert!(!h.app().arranger_state.arr_playback);
    h.ch('P');
    assert!(h.app().arranger_state.arr_playback);

    // A routed pattern clip is now visible to the rational playback resolver.
    let hits = h.project(|p| {
        p.arrangement.playback_hits(seqterm_core::RationalTime::ZERO).len()
    });
    assert_eq!(hits, 1, "routed clip resolves for playback at beat 0");
}

#[test]
fn move_clip_with_keys_updates_rational_start() {
    let mut h = HeadlessApp::with_project(project_with_pattern());
    h.goto(ViewKind::Arranger).ch('g').ch('t').ch('n').enter();

    let start0 = h.project(|p| p.arrangement.tracks[0].lanes[0].clips[0].start);
    h.ch('.'); // nudge the clip +1 beat
    let start1 = h.project(|p| p.arrangement.tracks[0].lanes[0].clips[0].start);
    assert_eq!(start1 - start0, seqterm_core::RationalTime::whole(1));

    // Undo restores the exact rational position (Phase 1 command/undo path).
    h.key_mods(KeyCode::Char('z'), crossterm::event::KeyModifiers::CONTROL);
    let start2 = h.project(|p| p.arrangement.tracks[0].lanes[0].clips[0].start);
    assert_eq!(start2, start0, "Ctrl+Z restores the clip's rational start");
}

/// Drives the real mouse dispatcher (Milestone E): render to populate the panel
/// rect, then press on a clip and drag it to a new beat.
fn setup_one_clip() -> HeadlessApp {
    let mut h = HeadlessApp::with_project(project_with_pattern());
    h.goto(ViewKind::Arranger).ch('g').ch('t').ch('n').enter();
    // Fixed zoom so cell→beat maths are deterministic: bar_width 4 ⇒ 1 beat/col.
    h.app_mut().arranger_state.bar_width = 4;
    h.render();
    h
}

#[test]
fn mouse_drag_moves_clip() {
    let mut h = setup_one_clip();
    let rect = h.arranger_panel();
    assert!(rect.width > 0, "panel rect populated by render");
    let lane_x0 = rect.x + 18;
    let yrow = rect.y + 2; // track 0

    // Clip starts at beat 0; grab it there and drag 8 columns (= 8 beats) right.
    let start0 = h.project(|p| p.arrangement.tracks[0].lanes[0].clips[0].start);
    assert_eq!(start0, seqterm_core::RationalTime::ZERO);

    h.mouse_down(lane_x0, yrow)
        .mouse_drag(lane_x0 + 8, yrow)
        .mouse_up(lane_x0 + 8, yrow);

    let moved = h.project(|p| p.arrangement.tracks[0].lanes[0].clips[0].start);
    assert_eq!(moved, seqterm_core::RationalTime::whole(8), "clip dragged to beat 8");

    // The whole drag is one undo step.
    h.key_mods(KeyCode::Char('z'), crossterm::event::KeyModifiers::CONTROL);
    let undone = h.project(|p| p.arrangement.tracks[0].lanes[0].clips[0].start);
    assert_eq!(undone, start0, "Ctrl+Z restores the pre-drag position");
}

#[test]
fn alt_drag_duplicates_clip() {
    let mut h = setup_one_clip();
    let rect = h.arranger_panel();
    let lane_x0 = rect.x + 18;
    let yrow = rect.y + 2;

    assert_eq!(h.arrangement_clip_count(), 1);
    // Alt+press makes a copy and drags it; the original stays at beat 0.
    h.mouse_down_alt(lane_x0, yrow)
        .mouse_drag(lane_x0 + 4, yrow)
        .mouse_up(lane_x0 + 4, yrow);

    assert_eq!(h.arrangement_clip_count(), 2, "Alt+Drag duplicated the clip");
    let starts = h.project(|p| {
        let mut s: Vec<i64> = p.arrangement.tracks[0].lanes[0]
            .clips.iter().map(|c| c.start.num() / c.start.den().max(1)).collect();
        s.sort();
        s
    });
    assert_eq!(starts, vec![0, 4], "original at 0, copy dragged to beat 4");
}
