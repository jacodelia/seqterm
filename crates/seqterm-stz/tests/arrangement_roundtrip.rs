//! Phase A guard: a full arrangement (clips / markers / regions / sections /
//! automation) must survive a `.stz` save→load round-trip byte-for-byte.
//!
//! Persistence is lossless today because `bridge::from_core` stores the whole
//! `Project` as `core_project_json` and `load_core` deserializes it back. This
//! test locks that in so a future refactor of the `.stz` writer can't silently
//! drop arrangement data.

use seqterm_core::{
    ArrangementTrack, AutomationCurve, ClipKind, Project, RationalTime, TrackKind,
};
use seqterm_stz::{bridge, load, save};

fn r(n: i64, d: i64) -> RationalTime {
    RationalTime::new(n, d)
}

fn build_project() -> Project {
    let mut p = Project::blank("roundtrip");
    let arr = &mut p.arrangement;

    // Two tracks of different kinds.
    arr.tracks.push(ArrangementTrack::new("Lead", TrackKind::Midi));
    arr.tracks.push(ArrangementTrack::new("Drums", TrackKind::Audio));

    // Clips of each kind across both tracks.
    arr.add_clip(
        0,
        "verse",
        ClipKind::Pattern { pattern_key: "A".into() },
        r(0, 1),
        r(4, 1),
    );
    arr.add_clip(
        0,
        "chorus",
        ClipKind::Midi { pattern_key: Some("B".into()) },
        r(4, 1),
        r(4, 1),
    );
    arr.add_clip(
        1,
        "loop.wav",
        ClipKind::Audio { path: "samples/loop.wav".into(), gain: 0.8 },
        r(2, 1),
        r(8, 1),
    );

    // Markers, regions, sections, cycle.
    arr.add_marker(r(0, 1), "start");
    arr.add_marker(r(8, 3), "drop"); // odd denominator on purpose
    arr.add_region(r(0, 1), r(8, 1), "intro");
    arr.add_section(r(0, 1), r(16, 1), "A");
    arr.cycle = Some((r(4, 1), r(12, 1)));

    // Automation on track 0.
    arr.set_automation_point(0, "cutoff", r(0, 1), 0.0, AutomationCurve::Linear);
    arr.set_automation_point(0, "cutoff", r(4, 1), 1.0, AutomationCurve::Bezier);
    arr.set_automation_point(0, "cutoff", r(8, 3), 0.5, AutomationCurve::Exponential);

    p
}

#[test]
fn arrangement_survives_stz_roundtrip() {
    let original = build_project();

    // Sanity: the fixture is non-trivial.
    assert!(!original.arrangement.is_empty());
    assert_eq!(original.arrangement.tracks.len(), 2);
    assert_eq!(original.arrangement.markers.len(), 2);

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("roundtrip.stz");

    let container = bridge::from_core(&original);
    save(&container, &path).unwrap();
    let loaded_container = load(&path).unwrap();
    let loaded = bridge::load_core(&loaded_container);

    assert_eq!(
        loaded.arrangement, original.arrangement,
        "arrangement changed across .stz round-trip"
    );
}
