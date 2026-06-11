//! On-demand runtime validation of the CLAP host against a real `.clap`.
//!
//! `#[ignore]`d so normal `cargo test` (and CI without plugins) skips it — heavy
//! plugin loads shouldn't run unattended. Run explicitly when a CLAP instrument
//! is installed:
//!
//! ```text
//! cargo test -p seqterm-plugin-clap --features clap -- --ignored --nocapture
//! SEQTERM_CLAP_DIR=/path/to/clap cargo test -p seqterm-plugin-clap --features clap -- --ignored
//! ```
//!
//! Validates: factory scan with real metadata, building a live instrument, and
//! driving note + CC + pitch-bend to audible output (proving the CC/pitch-bend
//! path added on top of note events does not break rendering).

#![cfg(feature = "clap")]

use seqterm_plugin_clap::{scan_directory, ClapHost};
use seqterm_ports::plugin::PluginHostPort;

#[test]
#[ignore = "requires a real .clap instrument installed (run with --ignored)"]
fn instrument_renders_audio_with_cc_and_pitch_bend() {
    let dir = std::path::PathBuf::from(
        std::env::var("SEQTERM_CLAP_DIR").unwrap_or_else(|_| "/usr/lib/clap".to_string()),
    );

    let infos = scan_directory(&dir);
    if infos.is_empty() {
        eprintln!("no CLAP plugins in {} — skipping", dir.display());
        return;
    }
    // Scanning must yield real, non-empty ids.
    assert!(infos.iter().all(|i| !i.id.is_empty()), "every descriptor has a real id");

    let Some(inst) = infos.iter().find(|i| i.is_instrument) else {
        eprintln!("no CLAP *instrument* in {} — skipping", dir.display());
        return;
    };

    let (sr, block) = (48_000u32, 512u32);
    let mut host = ClapHost::new();
    host.scan(&dir).expect("scan");
    let mut src = host
        .create_audio_source(&inst.id, sr, block)
        .expect("create_audio_source for an instrument");

    {
        let synth = src.as_synth().expect("instrument exposes AudioSynthPort");
        // Enable polyphonic (MPE) expression: pitch-bend → per-note tuning,
        // CC74 → per-note brightness, ±48 semitone range.
        synth.set_mpe(true, 48.0);
        synth.note_on(0, 60, 100);
        synth.control_change(0, 74, 90); // → Brightness note expression
        synth.pitch_bend(0, 4096);       // → Tuning note expression (~+12 st)
        synth.channel_pressure(0, 80);   // → Pressure note expression
    }

    let mut buf = vec![0.0f32; (block as usize) * 2];
    let mut peak = 0.0f32;
    for _ in 0..40 {
        let frames = src.render(&mut buf, sr);
        for s in &buf[..frames * 2] {
            peak = peak.max(s.abs());
        }
    }

    eprintln!("{} ({}): peak={peak:.6}", inst.name, inst.id);
    assert!(peak > 1e-4, "instrument {} produced silence (peak={peak})", inst.name);
}

#[test]
#[ignore = "requires a real .clap instrument installed (run with --ignored)"]
fn instrument_state_saves_and_restores() {
    let dir = std::path::PathBuf::from(
        std::env::var("SEQTERM_CLAP_DIR").unwrap_or_else(|_| "/usr/lib/clap".to_string()),
    );
    let infos = scan_directory(&dir);
    let Some(inst) = infos.iter().find(|i| i.is_instrument) else {
        eprintln!("no CLAP instrument in {} — skipping", dir.display());
        return;
    };

    let mut host = ClapHost::new();
    host.scan(&dir).expect("scan");
    let mut src = host
        .create_audio_source(&inst.id, 48_000, 512)
        .expect("create_audio_source for an instrument");
    let synth = src.as_synth().expect("instrument exposes AudioSynthPort");

    let Some(saved) = synth.save_state() else {
        eprintln!("{} has no CLAP state extension — skipping", inst.name);
        return;
    };
    assert!(!saved.is_empty(), "saved state should be non-empty");
    // Restoring the saved bytes succeeds and re-serializes identically.
    assert!(synth.load_state(&saved), "load_state should accept its own blob");
    let resaved = synth.save_state().expect("save after load");
    eprintln!("{}: state {} bytes (round-trip stable={})",
        inst.name, saved.len(), resaved == saved);
    assert_eq!(resaved, saved, "plugin state should round-trip deterministically");
}
