//! CURVES (harmonograph) sidebar tab smoke test: the 5th tab renders without
//! panicking and the label appears in the strip.

use seqterm_core::{Note, Pattern, Project};
use seqterm_ui::app::ViewKind;
use seqterm_ui::testkit::HeadlessApp;

#[test]
fn curves_tab_renders() {
    let mut proj = Project::blank("t");
    let mut pat = Pattern::new("P", 4);
    pat.set_step(0, Note::from_midi(60, 100).unwrap());
    proj.patterns.insert("P".to_string(), pat);

    let mut h = HeadlessApp::with_project(proj);
    h.goto(ViewKind::Matrix);
    h.app_mut().sidebar_tab = 4;       // CURVES
    h.app_mut().matrix_section = 2;    // visualizer focused

    let text = h.render_text(160, 44);
    assert!(text.contains("CURVES"), "CURVES tab present in the strip");
    // The 5-tab order is a valid permutation of 0..5.
    let mut order = h.app().sidebar_tab_order.to_vec();
    order.sort();
    assert_eq!(order, vec![0, 1, 2, 3, 4]);
}
