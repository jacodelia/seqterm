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

#[test]
fn automation_points_via_keys_are_undoable() {
    use seqterm_core::RationalTime;
    let mut h = HeadlessApp::with_project(project_with_pattern());
    h.goto(ViewKind::Arranger).ch('g').ch('t').ch('n').enter();

    // `V` reveals the automation sub-lane (default dest "volume", value 0.5).
    h.ch('V');
    assert!(h.app().arranger_state.arr_auto_edit);
    assert_eq!(h.app().arranger_state.arr_auto_dest, "volume");

    // Write a point at the beat-0 cursor at 0.5, then move one bar right and
    // raise the value before writing a second point — a ramp.
    h.ch('p'); // point at beat 0, value 0.5
    h.ch('l'); // beat cursor +1 bar (4 beats)
    h.ch('+').ch('+'); // 0.5 → 0.6
    h.ch('p'); // point at beat 4, value 0.6

    let pts = h.project(|p| p.arrangement.tracks[0].automation.len());
    assert_eq!(pts, 1, "one 'volume' lane created");
    let n_points = h.project(|p| p.arrangement.tracks[0].automation[0].points.len());
    assert_eq!(n_points, 2, "two breakpoints written");

    // The lane interpolates between the two points.
    let mid = h.project(|p| p.arrangement.automation_value(0, "volume", RationalTime::whole(2)));
    assert!(mid.is_some_and(|v| (0.5..=0.6).contains(&v)), "ramps between points");

    // `c` removes the nearest point (we're at beat 4) → back to one point.
    h.ch('c');
    let n_after = h.project(|p| p.arrangement.tracks[0].automation[0].points.len());
    assert_eq!(n_after, 1, "nearest point removed");

    // Each automation edit is its own undo step; undo the removal.
    h.key_mods(KeyCode::Char('z'), crossterm::event::KeyModifiers::CONTROL);
    let n_undo = h.project(|p| p.arrangement.tracks[0].automation[0].points.len());
    assert_eq!(n_undo, 2, "Ctrl+Z restores the removed breakpoint");
}

#[test]
fn automation_destination_picker_cycles_and_writes_distinct_lanes() {
    let mut h = HeadlessApp::with_project(project_with_pattern());
    h.goto(ViewKind::Arranger).ch('g').ch('t').ch('n').enter();

    // Open the automation sub-lane (default dest "volume") and write a point.
    h.ch('V').ch('p');
    // `b` picks the next destination ("pan") — a separate lane.
    h.ch('b');
    assert_eq!(h.app().arranger_state.arr_auto_dest, "pan");
    h.ch('p');

    let dests = h.project(|p| {
        let mut d: Vec<String> = p.arrangement.tracks[0]
            .automation.iter().map(|l| l.destination.clone()).collect();
        d.sort();
        d
    });
    assert_eq!(dests, vec!["pan".to_string(), "volume".to_string()],
        "each picked destination writes its own lane");

    // `B` cycles backward through the fixed list, wrapping from "pan" to "volume".
    h.ch('B');
    assert_eq!(h.app().arranger_state.arr_auto_dest, "volume");
}

#[test]
fn markers_add_jump_and_remove_via_keys() {
    use seqterm_core::RationalTime;
    let mut h = HeadlessApp::with_project(project_with_pattern());
    h.goto(ViewKind::Arranger).ch('g').ch('t');

    // `m` at beat 0 → "Intro"; move +1 bar (4 beats) and add "Verse".
    h.ch('m');
    h.ch('l');
    h.ch('m');
    let markers = h.project(|p| {
        p.arrangement.markers.iter().map(|m| (m.beat, m.name.clone())).collect::<Vec<_>>()
    });
    assert_eq!(markers, vec![
        (RationalTime::ZERO, "Intro".to_string()),
        (RationalTime::whole(4), "Verse".to_string()),
    ]);

    // Jump back to the previous marker (Intro at beat 0).
    h.ch('<');
    assert_eq!(h.app().arranger_state.arr_cursor_beat, RationalTime::ZERO);
    // Jump forward to the next marker (Verse at beat 4).
    h.ch('>');
    assert_eq!(h.app().arranger_state.arr_cursor_beat, RationalTime::whole(4));

    // `M` removes the nearest marker (Verse); undo restores it.
    h.ch('M');
    assert_eq!(h.project(|p| p.arrangement.markers.len()), 1);
    h.key_mods(KeyCode::Char('z'), crossterm::event::KeyModifiers::CONTROL);
    assert_eq!(h.project(|p| p.arrangement.markers.len()), 2, "Ctrl+Z restores the marker");
}

#[test]
fn regions_and_cycle_via_keys() {
    use seqterm_core::RationalTime;
    let mut h = HeadlessApp::with_project(project_with_pattern());
    h.goto(ViewKind::Arranger).ch('g').ch('t');

    // `i` at beat 0 sets the region start; move +2 bars (8 beats); `e` closes it.
    h.ch('i').ch('l').ch('l').ch('e');
    let regions = h.project(|p| {
        p.arrangement.regions.iter().map(|r| (r.start, r.end)).collect::<Vec<_>>()
    });
    assert_eq!(regions, vec![(RationalTime::ZERO, RationalTime::whole(8))]);
    assert!(h.app().arranger_state.arr_region_anchor.is_none(), "anchor cleared after e");

    // Move the cursor inside the region; `L` toggles cycle over it.
    h.ch('h'); // beat 4 (inside [0,8))
    h.ch('L');
    assert_eq!(
        h.project(|p| p.arrangement.cycle),
        Some((RationalTime::ZERO, RationalTime::whole(8))),
        "cycle set to the region under the cursor"
    );
    // `L` again on the same span clears it.
    h.ch('L');
    assert_eq!(h.project(|p| p.arrangement.cycle), None, "cycle toggled off");

    // `E` removes the region under the cursor; undo restores it (one step).
    h.ch('E');
    assert_eq!(h.project(|p| p.arrangement.regions.len()), 0);
    h.key_mods(KeyCode::Char('z'), crossterm::event::KeyModifiers::CONTROL);
    assert_eq!(h.project(|p| p.arrangement.regions.len()), 1, "Ctrl+Z restores the region");
}

#[test]
fn track_reorder_kind_and_delete_via_keys() {
    use seqterm_core::TrackKind;
    let mut h = HeadlessApp::with_project(project_with_pattern());
    h.goto(ViewKind::Arranger).ch('g').ch('t').ch('t').ch('t'); // 3 tracks
    // Name them so reorder is observable.
    for (i, name) in ["A", "B", "C"].iter().enumerate() {
        h.app_mut().project.lock().arrangement.tracks[i].name = name.to_string();
    }
    let names = |h: &HeadlessApp| {
        h.project(|p| p.arrangement.tracks.iter().map(|t| t.name.clone()).collect::<Vec<_>>())
    };

    // Focus track 0, move it down (`J`) → A swaps below B.
    h.app_mut().arranger_state.selected_track = 0;
    h.ch('J');
    assert_eq!(h.app().arranger_state.selected_track, 1, "selection follows the moved track");
    assert_eq!(names(&h), vec!["B", "A", "C"]);

    // `T` cycles the focused track's kind (default MIDI → Audio).
    assert_eq!(h.project(|p| p.arrangement.tracks[1].kind), TrackKind::Midi);
    h.ch('T');
    assert_eq!(h.project(|p| p.arrangement.tracks[1].kind), TrackKind::Audio);

    // `X` deletes the focused track; undo restores it.
    h.ch('X');
    assert_eq!(h.project(|p| p.arrangement.tracks.len()), 2);
    assert_eq!(names(&h), vec!["B", "C"]);
    h.key_mods(KeyCode::Char('z'), crossterm::event::KeyModifiers::CONTROL);
    assert_eq!(h.project(|p| p.arrangement.tracks.len()), 3, "Ctrl+Z restores the deleted track");
}

#[test]
fn shift_click_multi_select_then_delete() {
    use seqterm_core::RationalTime;
    let mut h = HeadlessApp::with_project(project_with_pattern());
    h.goto(ViewKind::Arranger).ch('g').ch('t');
    // Two clips at beats 0 and 4 on track 0.
    h.app_mut().arranger_state.arr_cursor_beat = RationalTime::ZERO;
    h.ch('n').enter();
    h.app_mut().arranger_state.arr_cursor_beat = RationalTime::whole(4);
    h.ch('n').enter();
    assert_eq!(h.arrangement_clip_count(), 2);
    h.app_mut().arranger_state.bar_width = 4; // 1 beat/col
    h.render();

    let rect = h.arranger_panel();
    let lane_x0 = rect.x + 18;
    let yrow = rect.y + 2;

    // Shift+click both clips (beat 0 at col 0, beat 4 at col 4) → both selected.
    h.mouse_down_shift(lane_x0, yrow).mouse_up(lane_x0, yrow);
    h.mouse_down_shift(lane_x0 + 4, yrow).mouse_up(lane_x0 + 4, yrow);
    assert_eq!(h.app().arr_selection.len(), 2, "both clips multi-selected");

    // `x` deletes the whole selection as one undo step.
    h.ch('x');
    assert_eq!(h.arrangement_clip_count(), 0);
    assert!(h.app().arr_selection.is_empty());
    h.key_mods(KeyCode::Char('z'), crossterm::event::KeyModifiers::CONTROL);
    assert_eq!(h.arrangement_clip_count(), 2, "Ctrl+Z restores both clips");
}

#[test]
fn rename_track_inline_via_keys() {
    let mut h = HeadlessApp::with_project(project_with_pattern());
    h.goto(ViewKind::Arranger).ch('g').ch('t');

    // `r` starts the inline name editor seeded with the current name.
    h.ch('r');
    assert!(h.app().arranger_track_name_editing);
    // Clear the seeded name, type a new one, confirm.
    for _ in 0..12 {
        h.key(KeyCode::Backspace);
    }
    h.chars("LEAD").enter();

    assert!(!h.app().arranger_track_name_editing, "editor closed on Enter");
    assert_eq!(h.project(|p| p.arrangement.tracks[0].name.clone()), "LEAD");

    // Rename is undoable.
    h.key_mods(KeyCode::Char('z'), crossterm::event::KeyModifiers::CONTROL);
    assert_ne!(h.project(|p| p.arrangement.tracks[0].name.clone()), "LEAD");
}

#[test]
fn overview_minimap_click_moves_cursor() {
    use seqterm_core::RationalTime;
    let mut h = HeadlessApp::with_project(project_with_pattern());
    h.goto(ViewKind::Arranger).ch('g').ch('t').ch('n').enter();
    // Extend the arrangement so the minimap spans a meaningful range: move the
    // clip out to beat 16 → length_beats ~ 20.
    h.app_mut().project.lock().arrangement.tracks[0].lanes[0].clips[0].start = RationalTime::whole(16);
    h.render();

    let ov = h.app().arr_overview_rect.get();
    assert!(ov.width > 0, "overview minimap rect recorded after render");

    // Click the right-middle of the strip → cursor jumps near that beat.
    let total = h.project(|p| p.arrangement.length_beats().to_f64());
    let click_x = ov.x + ov.width / 2;
    h.mouse_down(click_x, ov.y).mouse_up(click_x, ov.y);

    let cursor = h.app().arranger_state.arr_cursor_beat.to_f64();
    let expected = (click_x - ov.x) as f64 / ov.width as f64 * total;
    assert!((cursor - expected).abs() <= 1.0, "cursor {cursor} ≈ clicked beat {expected}");
}

#[test]
fn sections_create_shift_duplicate_via_keys() {
    use seqterm_core::RationalTime;
    let mut h = HeadlessApp::with_project(project_with_pattern());
    h.goto(ViewKind::Arranger).ch('g').ch('t').ch('n').enter(); // 1 track + clip at beat 0

    // Define a section [0, 8): `i` at beat 0, move +2 bars, `S` to close.
    h.ch('i').ch('l').ch('l').ch('S');
    let secs = h.project(|p| p.arrangement.sections.iter().map(|s| (s.start, s.end)).collect::<Vec<_>>());
    assert_eq!(secs, vec![(RationalTime::ZERO, RationalTime::whole(8))]);

    // Move cursor inside (beat 4) and shift the section +1 bar with `)`.
    h.ch('h'); // beat 4
    h.ch(')');
    assert_eq!(
        h.project(|p| (p.arrangement.sections[0].start, p.arrangement.sections[0].end)),
        (RationalTime::whole(4), RationalTime::whole(12)),
        "section span shifted +4 beats"
    );
    // The clip that was at beat 0 is NOT in the [0,8) span after the shift? It was
    // inside when shifted: clip started at 0 ∈ [0,8) at shift time → moved to 4.
    let clip_starts = h.project(|p| {
        p.arrangement.tracks[0].lanes[0].clips.iter().map(|c| c.start).collect::<Vec<_>>()
    });
    assert!(clip_starts.contains(&RationalTime::whole(4)), "contained clip moved with the section");

    // Cursor now at beat 4 (still inside the moved section [4,12)); duplicate it.
    h.ch('D');
    assert_eq!(h.project(|p| p.arrangement.sections.len()), 2, "section duplicated");

    // Undo the duplicate (one step).
    h.key_mods(KeyCode::Char('z'), crossterm::event::KeyModifiers::CONTROL);
    assert_eq!(h.project(|p| p.arrangement.sections.len()), 1, "Ctrl+Z undoes the duplicate");
}

/// Section shift must move every contained clip across ALL tracks together
/// (clips inside the span follow; clips outside stay put), and be one undo step.
#[test]
fn section_shift_moves_contained_clips_across_all_tracks() {
    use seqterm_core::RationalTime;
    let mut h = HeadlessApp::with_project(project_with_pattern());
    // Two tracks, each with a clip at beat 0 (inside) and one at beat 12 (outside).
    h.goto(ViewKind::Arranger).ch('g').ch('t').ch('t');
    for ti in 0..2 {
        h.app_mut().arranger_state.selected_track = ti;
        h.app_mut().arranger_state.arr_cursor_beat = RationalTime::ZERO;
        h.ch('n').enter();
        h.app_mut().arranger_state.arr_cursor_beat = RationalTime::whole(12);
        h.ch('n').enter();
    }
    assert_eq!(h.arrangement_clip_count(), 4);

    // Section [0, 8): cursor at 0, `i`, +2 bars, `S`.
    h.app_mut().arranger_state.arr_cursor_beat = RationalTime::ZERO;
    h.ch('i').ch('l').ch('l').ch('S');

    // Shift +1 bar with the cursor inside the section.
    h.ch('h'); // beat 4, inside [0,8)
    h.ch(')');

    // On every track: the beat-0 clip moved to beat 4; the beat-12 clip stayed.
    let starts = |h: &HeadlessApp, ti: usize| {
        h.project(|p| {
            let mut s: Vec<i64> = p.arrangement.tracks[ti].lanes[0]
                .clips.iter().map(|c| c.start.num() / c.start.den().max(1)).collect();
            s.sort();
            s
        })
    };
    assert_eq!(starts(&h, 0), vec![4, 12], "track 0: contained clip shifted, outer stayed");
    assert_eq!(starts(&h, 1), vec![4, 12], "track 1: contained clip shifted, outer stayed");

    // The whole multi-track shift is one undo step.
    h.key_mods(KeyCode::Char('z'), crossterm::event::KeyModifiers::CONTROL);
    assert_eq!(starts(&h, 0), vec![0, 12], "Ctrl+Z restores track 0");
    assert_eq!(starts(&h, 1), vec![0, 12], "Ctrl+Z restores track 1");
}
