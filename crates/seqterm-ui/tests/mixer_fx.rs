//! MIXER/FX add + move buttons (channel FX `[FxSlot; 3]`).

use seqterm_core::{Clip, FxKind, Pattern, Project};
use seqterm_ui::app::ViewKind;
use seqterm_ui::testkit::HeadlessApp;

/// A project with one MIDI matrix clip → one mixer channel entry ("A0").
fn mixer_project() -> Project {
    let mut proj = Project::blank("test");
    proj.patterns.insert("P".to_string(), Pattern::new("P", 4));
    let clip = Clip::new("c", 0, 0).with_pattern("P"); // PatternSource::Midi by default
    proj.matrix.insert("A".to_string(), vec![Some(clip)]);
    proj
}

fn fx_kinds(h: &HeadlessApp) -> Vec<FxKind> {
    h.project(|p| {
        p.channels
            .iter()
            .find(|c| c.midi_port.as_deref() == Some("A0"))
            .map(|c| c.fx.iter().map(|s| s.kind.clone()).collect())
            .unwrap_or_else(|| vec![FxKind::None; 3])
    })
}

#[test]
fn mixer_fx_add_then_move_reorders_slots() {
    let mut h = HeadlessApp::with_project(mixer_project());
    h.goto(ViewKind::Mixer);
    h.app_mut().mixer_state.selected_channel = 0;

    // Add an effect to slot 0.
    h.app_mut().mixer_state.fx_slot_idx = 0;
    h.app_mut().mixer_fx_add();
    assert_ne!(fx_kinds(&h)[0], FxKind::None, "slot 0 now has an effect");

    // Add a (different) effect to slot 1.
    h.app_mut().mixer_state.fx_slot_idx = 1;
    h.app_mut().mixer_fx_add();
    assert_ne!(fx_kinds(&h)[1], FxKind::None, "slot 1 now has an effect");

    // Move slot 1 up → swaps with slot 0; selection follows to slot 0.
    let before = fx_kinds(&h);
    h.app_mut().mixer_fx_move(-1);
    assert_eq!(h.app().mixer_state.fx_slot_idx, 0, "selection follows the moved slot");
    let after = fx_kinds(&h);
    assert_eq!(after[0], before[1], "slot 1's effect moved up to slot 0");
    assert_eq!(after[1], before[0], "slot 0's effect moved down to slot 1");

    // Can't move past the top edge.
    h.app_mut().mixer_fx_move(-1);
    assert_eq!(h.app().mixer_state.fx_slot_idx, 0);
}

#[test]
fn mixer_fx_buttons_clickable_after_render() {
    let mut h = HeadlessApp::with_project(mixer_project());
    h.goto(ViewKind::Mixer);
    h.app_mut().focus = seqterm_ui::app::FocusId::MixerFxSidebar;
    h.app_mut().mixer_state.selected_channel = 0;
    h.render();

    let add = h.app().mixer_fx_add_rect.get();
    assert!(add.width > 0, "Add button rect recorded after render");
    // Click the Add button → an effect appears in the selected slot.
    h.click(add.x + 1, add.y);
    assert_ne!(fx_kinds(&h)[0], FxKind::None, "clicking [+ Add] adds an effect");
}
