//! Integration tests against real LV2 plugins installed under /usr/lib/lv2.
//! Each test is guarded: it no-ops (passes) when the plugin isn't present, so
//! CI without LV2 plugins still goes green.

use std::path::Path;

use seqterm_plugin_lv2::Lv2PluginHost;
use seqterm_ports::plugin::PluginHostPort;
use seqterm_ports::realtime::{AudioSource, AudioSynthPort};

const AMP_URI: &str = "http://plugin.org.uk/swh-plugins/amp";
const PIANO_URI: &str = "http://drobilla.net/plugins/mda/Piano";

fn scan_system(host: &mut Lv2PluginHost) {
    for dir in seqterm_plugin_lv2::default_search_paths() {
        let _ = host.scan(&dir);
    }
}

#[test]
fn scans_system_lv2() {
    if !Path::new("/usr/lib/lv2").exists() {
        eprintln!("no /usr/lib/lv2, skipping");
        return;
    }
    let mut host = Lv2PluginHost::new();
    scan_system(&mut host);
    let n = host.list_plugins().len();
    eprintln!("discovered {n} LV2 plugins");
    assert!(n > 0, "expected to discover at least one LV2 plugin");
}

#[test]
fn amp_processes_and_responds_to_gain() {
    if !Path::new("/usr/lib/lv2/amp-swh.lv2").exists() {
        eprintln!("amp-swh.lv2 not installed, skipping");
        return;
    }
    let mut host = Lv2PluginHost::new();
    scan_system(&mut host);
    assert!(
        host.list_plugins().iter().any(|p| p.id == AMP_URI),
        "amp plugin not discovered"
    );

    let id = host.instantiate(AMP_URI, 48_000, 256).expect("instantiate amp");

    // amp has one control input: "gain (dB)", min -70, max +70, default 0 (unity).
    assert!(host.param_count(id) >= 1, "amp should expose a gain param");

    let frames = 256usize;
    let input = vec![0.5f32; frames * 2]; // interleaved stereo, constant 0.5
    let mut output = vec![0.0f32; frames * 2];

    // Unity gain (default 0 dB): output ≈ input.
    host.process(id, &input, &mut output).unwrap();
    let peak_unity = output.iter().fold(0.0f32, |a, &b| a.max(b.abs()));
    eprintln!("peak @ default gain = {peak_unity}");
    assert!(peak_unity > 0.4, "audio did not flow through at unity gain");

    // Minimum gain (-70 dB): output ≈ silence.
    host.set_param(id, 0, 0.0);
    let mut quiet = vec![0.0f32; frames * 2];
    host.process(id, &input, &mut quiet).unwrap();
    let peak_quiet = quiet.iter().fold(0.0f32, |a, &b| a.max(b.abs()));
    eprintln!("peak @ min gain = {peak_quiet}");
    assert!(peak_quiet < peak_unity * 0.5, "gain param had no effect");

    host.destroy(id);
}

#[test]
fn instrument_source_sounds_on_note_on() {
    if !Path::new("/usr/lib/lv2/mda.lv2").exists() {
        eprintln!("mda.lv2 not installed, skipping");
        return;
    }
    let mut host = Lv2PluginHost::new();
    scan_system(&mut host);
    if !host.list_plugins().iter().any(|p| p.id == PIANO_URI) {
        eprintln!("mda Piano not discovered, skipping");
        return;
    }

    let mut src = host
        .create_instrument_source(PIANO_URI, 48_000, 256)
        .expect("create mda Piano instrument source");

    let frames = 256usize;
    let mut out = vec![0.0f32; frames * 2];

    // Before any note: silence.
    src.render(&mut out, 48_000);
    let peak_idle = out.iter().fold(0.0f32, |a, &b| a.max(b.abs()));
    eprintln!("peak idle = {peak_idle}");
    assert!(peak_idle < 1e-4, "expected silence before note_on");

    // Note on, then render several blocks — a piano has a sharp attack.
    src.note_on(0, 60, 100);
    let mut peak_note = 0.0f32;
    for _ in 0..16 {
        out.iter_mut().for_each(|s| *s = 0.0);
        src.render(&mut out, 48_000);
        peak_note = peak_note.max(out.iter().fold(0.0f32, |a, &b| a.max(b.abs())));
    }
    eprintln!("peak after note_on = {peak_note}");
    assert!(peak_note > 0.01, "instrument produced no sound on note_on");

    src.note_off(0, 60);
}
