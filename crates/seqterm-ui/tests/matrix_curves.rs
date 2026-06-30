//! CURVES (score-lines) sidebar tab smoke test: the 5th tab renders the per-
//! pattern melodic contour without panicking, and the label appears in the strip.

use seqterm_core::{Note, Pattern, Project};
use seqterm_ui::app::ViewKind;
use seqterm_ui::testkit::HeadlessApp;

#[test]
fn curves_tab_renders() {
    let mut proj = Project::blank("t");
    let mut pat = Pattern::new("P", 8);
    // A little melody so the contour line + peak rings have something to draw.
    for (s, m) in [(0usize, 60u8), (2, 67), (4, 64), (6, 72)] {
        pat.set_step(s, Note::from_midi(m, 100).unwrap());
    }
    proj.patterns.insert("P".to_string(), pat);
    // Assign + enable on matrix row A so it counts as an active pattern.
    let clip = seqterm_core::Clip::new("c", 0, 0).with_pattern("P");
    proj.matrix.insert("A".to_string(), vec![Some(clip)]);

    let mut h = HeadlessApp::with_project(proj);
    h.goto(ViewKind::Matrix);
    h.app_mut().sidebar_tab = 4; // CURVES
    h.app_mut().matrix_section = 2; // visualizer focused

    let text = h.render_text(160, 44);
    assert!(text.contains("CURVES"), "CURVES tab present in the strip");
    assert!(!text.contains("no active patterns"), "active pattern's score line should render");

    // The 5-tab order is a valid permutation of 0..5.
    let mut order = h.app().sidebar_tab_order.to_vec();
    order.sort();
    assert_eq!(order, vec![0, 1, 2, 3, 4]);
}
