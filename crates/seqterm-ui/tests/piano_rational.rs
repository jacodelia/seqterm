//! Phase 6 piano-roll tests: Ctrl-zoom (pattern resolution down to 1/64) and
//! irregular/arbitrary tuplet figures (e.g. 7:9) stored exactly in the canonical
//! `events` layer and audible via `to_events`.

use crossterm::event::{KeyCode, KeyModifiers};
use seqterm_core::{Pattern, Project, RationalTime, Resolution, Tuplet};
use seqterm_ui::app::ViewKind;
use seqterm_ui::testkit::HeadlessApp;

fn piano_harness() -> HeadlessApp {
    let mut proj = Project::blank("test");
    proj.patterns.insert("P".to_string(), Pattern::new("P", 4));
    let mut h = HeadlessApp::with_project(proj);
    h.goto(ViewKind::Tracker);
    h.app_mut().tracker_state.pattern_key = Some("P".to_string());
    h.app_mut().tracker_section = 1; // piano roll
    h
}

#[test]
fn ctrl_zoom_subdivides_display_grid_without_changing_pattern_length() {
    let mut h = piano_harness();
    // Baseline: pattern is 4 steps (1 bar) at 1/16. Zoom must NOT touch this.
    let (len0, res0) = h.project(|p| (p.patterns["P"].length, p.patterns["P"].resolution));
    assert_eq!((len0, res0), (4, Resolution::Whole(16)));
    assert_eq!(h.app().edit_state.resolution, Resolution::Whole(16));

    // Ctrl+'=' steps the DISPLAY/edit grid finer: 1/16 → 1/32 → 1/64 (semifusa).
    h.key_mods(KeyCode::Char('='), KeyModifiers::CONTROL);
    assert_eq!(h.app().edit_state.resolution, Resolution::Whole(32));
    h.key_mods(KeyCode::Char('='), KeyModifiers::CONTROL);
    assert_eq!(h.app().edit_state.resolution, Resolution::Whole(64));

    // CRITICAL: the pattern itself is unchanged — same length and resolution.
    let (len1, res1) = h.project(|p| (p.patterns["P"].length, p.patterns["P"].resolution));
    assert_eq!((len1, res1), (4, Resolution::Whole(16)),
        "zoom must not re-grid or lengthen the pattern");

    // Ctrl+'-' steps back coarser (edit grid only).
    h.key_mods(KeyCode::Char('-'), KeyModifiers::CONTROL);
    assert_eq!(h.app().edit_state.resolution, Resolution::Whole(32));
    assert_eq!(h.project(|p| p.patterns["P"].length), 4);
}

#[test]
fn insert_arbitrary_tuplet_figure_7_over_9_is_exact_and_audible() {
    let mut h = piano_harness();
    // Arbitrary tuplet 7:9 (seven notes in the span of nine sixteenths).
    h.app_mut().edit_state.tuplet = Some(Tuplet::new(7, 9));
    // Cursor at step 0 (beat 0), some pitch row.
    h.app_mut().piano_cursor = (40, 0);

    // `g` drops the whole figure into the exact rational layer.
    h.ch('g');

    let events = h.project(|p| p.patterns["P"].events.clone());
    assert_eq!(events.len(), 7, "seven exact notes for a 7:9 figure");

    // Base step at 1/16 = 1/4 beat; tuplet cell = 1/4 * 9/7 = 9/28 beat.
    let cell = RationalTime::new(9, 28);
    assert_eq!(events[0].start, RationalTime::ZERO);
    assert_eq!(events[1].start, cell);
    assert_eq!(events[6].start, cell * 6);
    // Span = 7 * 9/28 = 9/4 beats = nine sixteenths → exactly 7:9.
    assert_eq!(cell * 7, RationalTime::new(9, 4));

    // They are audible: merged into the rational event stream the scheduler scans.
    assert_eq!(h.project(|p| p.patterns["P"].to_events().len()), 7);

    // One undo step removes the whole figure.
    h.key_mods(KeyCode::Char('z'), KeyModifiers::CONTROL);
    assert_eq!(h.project(|p| p.patterns["P"].events.len()), 0);
}

#[test]
fn fine_cursor_places_exact_note_off_the_step_grid() {
    let mut h = piano_harness();
    h.app_mut().piano_cursor = (40, 0); // pitch row 40 → MIDI 68
    // Default grid 1/16 = 1/4 beat. `]` thrice → fine beat 3/4.
    h.ch(']').ch(']').ch(']');
    assert_eq!(h.app().piano_fine_beat, RationalTime::new(3, 4));

    // `\` toggles an exact note at the fine beat with the cursor pitch.
    h.ch('\\');
    let evs = h.project(|p| p.patterns["P"].events.clone());
    assert_eq!(evs.len(), 1);
    assert_eq!(evs[0].start, RationalTime::new(3, 4));
    assert_eq!(evs[0].note.to_midi(), Some(68));

    // Toggling again removes it (undoable either way).
    h.ch('\\');
    assert_eq!(h.project(|p| p.patterns["P"].events.len()), 0);
}

#[test]
fn copy_paste_is_rhythm_aware_carrying_steps_and_events() {
    use crossterm::event::KeyModifiers;
    use seqterm_core::Note;
    let mut h = piano_harness();
    // Step note at step 0 and an exact 7-tuplet-ish event at 2/7 beat.
    h.app_mut().project.lock().patterns.get_mut("P").unwrap()
        .set_step(0, Note::from_midi(60, 100).unwrap());
    h.app_mut().project.lock().patterns.get_mut("P").unwrap()
        .add_event(RationalTime::new(2, 7), RationalTime::new(1, 7), Note::from_midi(67, 100).unwrap());

    // Select steps 0..1 in the piano roll, copy.
    h.app_mut().piano_selection.insert(0);
    h.app_mut().piano_selection.insert(1);
    h.key_mods(KeyCode::Char('c'), KeyModifiers::CONTROL);
    assert_eq!(h.app().pattern_clip.steps.len(), 1, "one step note copied");
    assert_eq!(h.app().pattern_clip.events.len(), 1, "the exact event copied too");

    // Move the cursor to step 2 (beat 0.5) and paste.
    h.app_mut().piano_cursor = (40, 2);
    h.key_mods(KeyCode::Char('v'), KeyModifiers::CONTROL);

    // Step note pasted at step 2; event pasted at beat 1/2 + 2/7.
    let (has_step2, ev_starts) = h.project(|p| {
        let pat = &p.patterns["P"];
        let s2 = !pat.steps[2].is_empty();
        let starts: Vec<RationalTime> = pat.events.iter().map(|e| e.start).collect();
        (s2, starts)
    });
    assert!(has_step2, "step note pasted at the cursor step");
    assert!(ev_starts.contains(&(RationalTime::new(1, 2) + RationalTime::new(2, 7))),
        "exact event pasted at cursor beat + offset (rhythm preserved): {ev_starts:?}");
}

#[test]
fn favorite_pattern_tab_shown_first_on_open() {
    let mut h = piano_harness();
    h.app_mut().settings.pattern_fav_tab = 2; // FX
    // Re-open the PATTERN view.
    h.goto(ViewKind::Matrix);
    h.goto(ViewKind::Tracker);
    assert_eq!(h.app().tracker_tab, 2, "favourite tab is shown first");
}

#[test]
fn tracker_step_view_inserts_tuplet_figure() {
    use seqterm_core::Tuplet;
    let mut h = piano_harness();
    h.app_mut().tracker_section = 0; // step table
    h.app_mut().tracker_state.cursor = (0, 0);
    h.app_mut().edit_state.tuplet = Some(Tuplet::new(5, 4)); // quintuplet
    h.ch('g');
    assert_eq!(h.project(|p| p.patterns["P"].events.len()), 5, "5-tuplet figure in the tracker");
}

#[test]
fn rhythm_toolbar_zoom_buttons_and_figure_opens_modal() {
    let mut h = piano_harness();
    h.render(); // populate the RHYTHM toolbar hit-test rects

    let rects = h.app().tracker_rhythm_btn_rects.get();
    assert!(rects[0].width > 0 && rects[1].width > 0 && rects[2].width > 0,
        "three RHYTHM buttons rendered");
    assert_eq!(rects[3].width, 0, "TUPLET/TRIPLET buttons removed");
    let center = |r: ratatui::layout::Rect| (r.x + r.width / 2, r.y + r.height / 2);

    // ZOOM+ (index 1) → edit grid 1/16 → 1/32; ZOOM− (index 0) → back.
    assert_eq!(h.app().edit_state.resolution, Resolution::Whole(16));
    let (zx, zy) = center(rects[1]);
    h.click(zx, zy);
    assert_eq!(h.app().edit_state.resolution, Resolution::Whole(32));
    let (zx, zy) = center(rects[0]);
    h.click(zx, zy);
    assert_eq!(h.app().edit_state.resolution, Resolution::Whole(16));

    // FIGURE (index 2) with a selection opens the irregular-rhythm modal.
    h.app_mut().piano_selection.insert(0);
    let (fx, fy) = center(rects[2]);
    h.click(fx, fy);
    assert!(h.app().rhythm_modal.is_some(), "FIGURE opens the modal for the selection");
}

#[test]
fn zoomed_subcell_middle_click_places_exact_event_within_a_beat() {
    let mut h = piano_harness();
    // Zoom the display to 1/32 → each 1/16 step splits into 2 sub-cells (pdiv=2).
    h.app_mut().edit_state.resolution = Resolution::Whole(32);
    h.app_mut().piano_note_scroll = 0; // first grid row = MIDI 108
    h.render();

    let area = h.app().piano_roll_area.get();
    assert!(area.width > 0, "piano roll rendered");
    let step_start_x = area.x + 1 + 5; // inner_x(+1) + key_w(5)
    let header_row = area.y + 1;

    // MIDDLE-click step 0's SECOND sub-cell (col offset 2 → cell_in_view 1 →
    // step 0,sub 1) on the first note row. That sub-beat is 1/8 (off the 1/16 grid).
    let col = step_start_x + 2;
    let r = header_row + 1;
    h.middle_click(col, r);

    let events = h.project(|p| p.patterns["P"].events.clone());
    assert_eq!(events.len(), 1, "one exact event placed via the sub-cell middle-click");
    assert_eq!(events[0].start, RationalTime::new(1, 8),
        "event placed at the sub-step beat (1/8), not snapped to a whole step");
    // The pattern length/resolution are untouched by the fine placement.
    assert_eq!(h.project(|p| (p.patterns["P"].length, p.patterns["P"].resolution)),
        (4, Resolution::Whole(16)));
}

#[test]
fn left_drag_rubber_band_selects_notes_and_middle_click_inserts_step_note() {
    use seqterm_core::Note;
    let mut h = piano_harness();
    h.app_mut().piano_note_scroll = 0; // first grid row = MIDI 108
    // Two step notes (MIDI 108) at steps 0 and 1 so a rectangle can grab them.
    {
        let mut proj = h.app_mut().project.lock();
        let pat = proj.patterns.get_mut("P").unwrap();
        pat.set_step(0, Note::from_midi(108, 100).unwrap());
        pat.set_step(1, Note::from_midi(108, 100).unwrap());
    }
    h.render();
    let area = h.app().piano_roll_area.get();
    let step_start_x = area.x + 1 + 5;
    let header_row = area.y + 1;
    let r = header_row + 1; // row 0 → MIDI 108

    // LEFT button now rubber-band selects: press at step 0, drag to step 1, release.
    h.mouse_down(step_start_x, r)
        .mouse_drag(step_start_x + 2, r)
        .mouse_up(step_start_x + 2, r);
    assert_eq!(h.app().piano_selection.len(), 2, "left drag selected both notes");
    // Left-drag must NOT have inserted anything.
    assert_eq!(h.project(|p| p.patterns["P"].steps.iter().filter(|n| !n.is_empty()).count()), 2);

    // MIDDLE-click step 3 (empty) inserts a step note there.
    assert!(h.project(|p| p.patterns["P"].steps[3].is_empty()));
    h.middle_click(step_start_x + 6, r); // cell_in_view 3 → step 3 (pdiv=1)
    assert!(!h.project(|p| p.patterns["P"].steps[3].is_empty()),
        "middle-click inserted a step note");
}

#[test]
fn ctrl_scroll_zooms_the_edit_grid_without_changing_length() {
    let mut h = piano_harness();
    h.render();
    let area = h.app().piano_roll_area.get();
    let (cx, cy) = (area.x + area.width / 2, area.y + area.height / 2);

    assert_eq!(h.app().edit_state.resolution, Resolution::Whole(16));
    h.ctrl_scroll(true, cx, cy); // wheel up = zoom in
    assert_eq!(h.app().edit_state.resolution, Resolution::Whole(32));
    h.ctrl_scroll(true, cx, cy);
    assert_eq!(h.app().edit_state.resolution, Resolution::Whole(64));
    h.ctrl_scroll(false, cx, cy); // wheel down = zoom out
    assert_eq!(h.app().edit_state.resolution, Resolution::Whole(32));
    // The pattern is never re-gridded by zoom.
    assert_eq!(h.project(|p| (p.patterns["P"].length, p.patterns["P"].resolution)),
        (4, Resolution::Whole(16)));
}

#[test]
fn delete_key_and_right_click_erase_notes() {
    use seqterm_core::Note;
    let mut h = piano_harness();

    // An exact event at the fine cursor → Delete removes it (rational layer).
    h.app_mut().piano_cursor = (40, 0); // row 40 → MIDI 68
    h.app_mut().project.lock().patterns.get_mut("P").unwrap()
        .add_event(RationalTime::new(1, 4), RationalTime::new(1, 8), Note::from_midi(68, 100).unwrap());
    h.app_mut().piano_fine_beat = RationalTime::new(1, 4);
    h.key(KeyCode::Delete);
    assert_eq!(h.project(|p| p.patterns["P"].events.len()), 0, "Delete erased the exact event");

    // A step note → right-click on its cell erases it (step layer).
    h.app_mut().project.lock().patterns.get_mut("P").unwrap()
        .set_step(0, Note::from_midi(108, 100).unwrap()); // row 0 = MIDI 108
    h.app_mut().piano_note_scroll = 0;
    h.render();
    let area = h.app().piano_roll_area.get();
    let step_start_x = area.x + 1 + 5;
    let header_row = area.y + 1;
    h.right_click(step_start_x, header_row + 1); // step 0, sub 0, row 0
    assert!(h.project(|p| p.patterns["P"].steps[0].is_empty()), "right-click erased the step note");
}

#[test]
fn tracker_note_column_shows_top_of_chord_and_events() {
    use seqterm_core::Note;
    let mut h = piano_harness();
    h.app_mut().tracker_section = 0; // step table
    let (g4, c5) = {
        let mut proj = h.app_mut().project.lock();
        let pat = proj.patterns.get_mut("P").unwrap();
        // Chord at step 0: C4(60) + G4(67) → the tracker must show the TOP (G4).
        let mut n = Note::from_midi(60, 100).unwrap();
        n.chord_notes.push(Note::from_midi(67, 100).unwrap().note);
        pat.set_step(0, n);
        // An exact event at step 1's span (beat 1/4 @ 1/16 grid) → C5(72).
        pat.add_event(RationalTime::new(1, 4), RationalTime::new(1, 8), Note::from_midi(72, 100).unwrap());
        (Note::from_midi(67, 100).unwrap().note, Note::from_midi(72, 100).unwrap().note)
    };
    let text = h.render_text(120, 40);
    assert!(text.contains(&g4), "top-of-chord note {g4} shown in the tracker NOTE column");
    assert!(text.contains(&c5), "exact piano-roll event note {c5} reflected in the tracker");
}

#[test]
fn zoomed_rubber_band_selects_step_notes_and_events() {
    use seqterm_core::Note;
    let mut h = piano_harness();
    h.app_mut().piano_note_scroll = 0; // first grid row = MIDI 108
    // Zoom to 1/32 → pdiv 2, sub-cell = 1/8 beat.
    h.app_mut().edit_state.resolution = Resolution::Whole(32);
    {
        let mut pr = h.app_mut().project.lock();
        let pat = pr.patterns.get_mut("P").unwrap();
        pat.set_step(1, Note::from_midi(108, 100).unwrap());          // beat 1/4 (cell 2)
        pat.add_event(RationalTime::new(1, 8), RationalTime::new(1, 8),
            Note::from_midi(108, 100).unwrap());                       // beat 1/8 (cell 1)
    }
    h.render();
    let area = h.app().piano_roll_area.get();
    let step_start_x = area.x + 1 + 5;
    let r = area.y + 2; // row 0 = MIDI 108

    // Rubber-band global cells 0..2 (cols +0..+4) across row 0.
    h.mouse_down(step_start_x, r)
        .mouse_drag(step_start_x + 4, r)
        .mouse_up(step_start_x + 4, r);

    assert!(h.app().piano_selection.contains(&1),
        "step note inside the rectangle is selected at zoom");
    assert!(h.app().piano_event_selection.contains(&0),
        "exact sub-cell event inside the rectangle is selected at zoom");
}

#[test]
fn left_drag_marquee_tracks_corner_and_clears_on_release() {
    let mut h = piano_harness();
    h.app_mut().piano_note_scroll = 0;
    h.render();
    let area = h.app().piano_roll_area.get();
    let step_start_x = area.x + 1 + 5;
    let r = area.y + 2; // first note row

    h.mouse_down(step_start_x, r);
    assert!(h.app().piano_select_cur.is_some(), "marquee corner set on press");
    h.mouse_drag(step_start_x + 4, r + 1);
    assert!(h.app().piano_select_cur.is_some(), "marquee corner tracks the drag");
    h.mouse_up(step_start_x + 4, r + 1);
    assert!(h.app().piano_select_cur.is_none(), "marquee cleared on release");
}

#[test]
fn figure_modal_applies_polyrhythm_within_its_span_only() {
    use crossterm::event::KeyModifiers;
    use seqterm_core::Note;
    let mut proj = Project::blank("test");
    proj.patterns.insert("P".to_string(), Pattern::new("P", 16));
    let mut h = HeadlessApp::with_project(proj);
    h.goto(ViewKind::Tracker);
    h.app_mut().tracker_state.pattern_key = Some("P".to_string());
    h.app_mut().tracker_section = 1;
    // Three selected events at beats 0,1,2 plus an UNSELECTED one at beat 3.
    {
        let mut pr = h.app_mut().project.lock();
        let pat = pr.patterns.get_mut("P").unwrap();
        pat.add_event(RationalTime::ZERO, RationalTime::new(1, 4), Note::from_midi(60, 100).unwrap());
        pat.add_event(RationalTime::whole(1), RationalTime::new(1, 4), Note::from_midi(62, 100).unwrap());
        pat.add_event(RationalTime::whole(2), RationalTime::new(1, 4), Note::from_midi(64, 100).unwrap());
        pat.add_event(RationalTime::whole(3), RationalTime::new(1, 4), Note::from_midi(67, 100).unwrap());
    }
    for i in 0..3 { h.app_mut().piano_event_selection.insert(i); } // not index 3

    // `g` opens the modal; pick grouping 3 (a 3:2 polyrhythm) and apply.
    h.ch('g');
    assert!(h.app().rhythm_modal.is_some());
    h.ch('3').enter();
    assert!(h.app().rhythm_modal.is_none());

    // Span = first start (0) → last selected end (2 + 1/4) = 9/4. n=3, m=2.
    // True polyrhythm: the N=3 tuplet layer at multiples of span/3 = 3/4, plus the
    // M=2 straight layer at multiples of span/2 = 9/8. Both layers sound.
    let region_end = RationalTime::new(9, 4);
    let starts = h.project(|p| p.patterns["P"].events.iter().map(|e| e.start).collect::<Vec<_>>());
    // The unselected note at beat 3 survives untouched (bounded to the selection).
    assert!(starts.contains(&RationalTime::whole(3)), "note outside the selection is untouched");
    let inside: Vec<RationalTime> = starts.iter().copied().filter(|&s| s < region_end).collect();
    // 2 (straight) + 3 (tuplet) = 5 events inside the span (0 is shared but added twice).
    assert_eq!(inside.len(), 5, "m + n polyrhythm notes laid across the span");
    let tuplet_cell = RationalTime::new(3, 4); // span/3
    for i in 0..3 {
        assert!(inside.contains(&(tuplet_cell * i as i64)), "tuplet note {i} present");
    }
    let straight_cell = RationalTime::new(9, 8); // span/2
    for j in 0..2 {
        assert!(inside.contains(&(straight_cell * j as i64)), "straight note {j} present");
    }
    for s in &inside { assert!(*s < region_end, "polyrhythm never extends past the selection"); }
    assert_eq!(h.app().edit_state.tuplet, Some(Tuplet::new(3, 2)));

    // A score-style bracket annotation now spans exactly the selection.
    let marks = h.project(|p| p.patterns["P"].tuplet_marks.clone());
    assert_eq!(marks.len(), 1, "one figure bracket recorded");
    assert_eq!(marks[0].start, RationalTime::ZERO);
    assert_eq!(marks[0].duration, RationalTime::new(9, 4), "bracket covers the span");
    assert_eq!(marks[0].count, 3);

    // Undo restores the originals (incl. the unselected note unchanged) and drops
    // the bracket.
    h.key_mods(KeyCode::Char('z'), KeyModifiers::CONTROL);
    let restored = h.project(|p| p.patterns["P"].events.iter().map(|e| e.start).collect::<Vec<_>>());
    assert!(restored.contains(&RationalTime::whole(1)));
    assert!(restored.contains(&RationalTime::whole(3)));
    assert!(h.project(|p| p.patterns["P"].tuplet_marks.is_empty()), "undo removes the bracket");
}

#[test]
fn figure_bracket_indicator_renders_over_the_grouped_notes() {
    use seqterm_core::Note;
    let mut h = piano_harness();
    h.app_mut().tracker_section = 1;
    h.app_mut().piano_note_scroll = 0; // row 0 = MIDI 108, so high notes are visible
    {
        let mut pr = h.app_mut().project.lock();
        let pat = pr.patterns.get_mut("P").unwrap();
        // Two grouped notes (high pitch → near the top of the grid) within [0, 1/2),
        // so the bracket has notes to anchor onto.
        pat.add_event(RationalTime::ZERO, RationalTime::new(1, 4), Note::from_midi(105, 100).unwrap());
        pat.add_event(RationalTime::new(1, 4), RationalTime::new(1, 4), Note::from_midi(105, 100).unwrap());
        pat.tuplet_marks.push(seqterm_core::TupletMark {
            start: RationalTime::ZERO,
            duration: RationalTime::new(1, 2),
            count: 5,
        });
    }
    let text = h.render_text(120, 40);
    // The bracket + grouping digit are now drawn on the note grid itself.
    assert!(text.contains('5'), "bracket grouping digit shown over the notes");
    assert!(text.contains('⌐') && text.contains('¬'), "bracket end caps drawn");
}

#[test]
fn deleting_a_grouped_note_drops_the_figure_mark() {
    use seqterm_core::Note;
    let mut h = piano_harness();
    h.app_mut().tracker_section = 1;
    {
        let mut pr = h.app_mut().project.lock();
        let pat = pr.patterns.get_mut("P").unwrap();
        pat.add_event(RationalTime::ZERO, RationalTime::new(1, 4), Note::from_midi(105, 100).unwrap());
        pat.tuplet_marks.push(seqterm_core::TupletMark {
            start: RationalTime::ZERO,
            duration: RationalTime::new(1, 2),
            count: 5,
        });
    }
    // Cursor on the grouped note (MIDI 105 = row 3), fine cursor at its beat.
    h.app_mut().piano_cursor = (3, 0);
    h.app_mut().piano_fine_beat = RationalTime::ZERO;
    h.key(KeyCode::Delete);

    // Deleting in the piano roll must NOT keep the figure: the mark is gone and
    // does not re-appear when a new note lands in that span.
    assert!(h.project(|p| p.patterns["P"].tuplet_marks.is_empty()),
        "deleting the note drops the figure mark");
    {
        let mut pr = h.app_mut().project.lock();
        pr.patterns.get_mut("P").unwrap()
            .add_event(RationalTime::ZERO, RationalTime::new(1, 4), Note::from_midi(105, 100).unwrap());
    }
    assert!(h.project(|p| p.patterns["P"].tuplet_marks.is_empty()),
        "the figure stays gone after re-adding a note");
}

#[test]
fn pattern_status_bar_shows_midi_in_out_monitor() {
    let mut h = piano_harness();
    // Simulate a recent incoming + outgoing note on channel 0 (shown 1-based).
    let now = std::time::Instant::now();
    h.app_mut().midi_monitor_in = Some((0, 60, 100, now));   // C4 in
    h.app_mut().midi_monitor_out = Some((0, 67, 96, now));   // G4 out
    let text = h.render_text(120, 40);
    assert!(text.contains('◀') && text.contains('▶'), "MIDI in/out arrows shown in PATTERN");
    assert!(text.contains("1·C-5:100"), "incoming note with channel shown");
    assert!(text.contains("1·G-5:96"), "outgoing note with channel shown");
}

#[test]
fn zooming_subdivides_the_tracker_into_decimal_sub_lines() {
    let mut h = piano_harness();
    h.app_mut().tracker_section = 0; // step table

    // At the default 1/16 grid the tracker is one row per step (no decimals).
    let text0 = h.render_text(120, 40);
    assert!(!text0.contains(".50"), "no sub-lines at the base grid");

    // Zoom the edit grid finer (1/16 → 1/32): each step now splits into 2 rows,
    // surfacing a decimal sub-line (`.50`) between steps.
    h.key_mods(KeyCode::Char('='), KeyModifiers::CONTROL);
    assert_eq!(h.app().edit_state.resolution, Resolution::Whole(32));
    let text1 = h.render_text(120, 40);
    assert!(text1.contains(".50"), "zoom adds a decimal sub-line in the tracker");
}

#[test]
fn selected_notes_are_recoloured_in_the_piano_roll() {
    use ratatui::style::Color;
    use seqterm_core::Note;
    let mut h = piano_harness();
    h.app_mut().piano_note_scroll = 0; // row 0 = MIDI 108
    h.app_mut().project.lock().patterns.get_mut("P").unwrap()
        .set_step(0, Note::from_midi(108, 100).unwrap());

    let area = { h.render(); h.app().piano_roll_area.get() };
    let step_start_x = area.x + 1 + 5;
    let r = area.y + 2; // row 0

    // Unselected: the note cell does NOT have the selection background.
    let sel_bg = Color::Rgb(90, 20, 100);
    assert_ne!(h.render_bg_at(120, 40, step_start_x, r), sel_bg, "not selected yet");

    // Select it via a left-drag rectangle over its cell.
    h.mouse_down(step_start_x, r).mouse_drag(step_start_x, r).mouse_up(step_start_x, r);
    assert!(h.app().piano_selection.contains(&0), "note selected");

    // Now the note cell shows the selection background colour.
    assert_eq!(h.render_bg_at(120, 40, step_start_x, r), sel_bg,
        "selected note is recoloured so the marked set is visible");
}

#[test]
fn shift_t_prompt_sets_arbitrary_tuplet_ratio() {
    let mut h = piano_harness();
    // Shift+T opens the ratio prompt; type "7:9", Enter.
    h.key(KeyCode::Char('T'));
    assert!(h.app().tuplet_input.is_some(), "tuplet prompt active");
    h.chars("7:9").enter();
    assert_eq!(h.app().edit_state.tuplet, Some(Tuplet::new(7, 9)));
    assert!(h.app().tuplet_input.is_none(), "prompt closed");
}
