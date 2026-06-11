//! Runtime validation harness for the CLAP host (requires the `clap` feature
//! and at least one real `.clap` instrument installed).
//!
//! Run against the system CLAP dir (default `/usr/lib/clap`) or a custom path:
//!
//! ```text
//! cargo run -p seqterm-plugin-clap --features clap --example clap_validate
//! cargo run -p seqterm-plugin-clap --features clap --example clap_validate -- "/path/to/Plugin.clap"
//! ```
//!
//! It scans, picks an instrument, builds a live audio instance, drives a note
//! plus a CC and a pitch-bend, and reports the output peak (non-zero = audible).

#[cfg(not(feature = "clap"))]
fn main() {
    eprintln!("build with --features clap to run this validation harness");
}

#[cfg(feature = "clap")]
fn main() {
    use seqterm_plugin_clap::{scan_directory, ClapHost};
    use seqterm_ports::plugin::PluginHostPort;

    let arg = std::env::args().nth(1);
    let dir = std::path::PathBuf::from(
        arg.clone().unwrap_or_else(|| "/usr/lib/clap".to_string()),
    );
    // Allow passing either a directory or a single .clap file.
    let scan_dir = if dir.is_file() { dir.parent().unwrap().to_path_buf() } else { dir.clone() };

    // 1) Scan / descriptors.
    let infos = scan_directory(&scan_dir);
    println!("scanned {} descriptor(s) in {}", infos.len(), scan_dir.display());
    let instruments: Vec<_> = infos.iter().filter(|i| i.is_instrument).collect();
    println!("  instruments: {}, effects: {}",
        instruments.len(), infos.iter().filter(|i| i.is_effect).count());
    for i in infos.iter().take(8) {
        println!("  - {:24} [{}] inst={} fx={}", i.name, i.id, i.is_instrument, i.is_effect);
    }

    // 2) Pick the instrument: a specific file if given, else the first one.
    let chosen = if dir.is_file() {
        infos.iter().find(|i| i.path == dir && i.is_instrument)
            .or_else(|| infos.iter().find(|i| i.path == dir))
    } else {
        instruments.first().copied()
    };
    let Some(info) = chosen else {
        println!("RESULT: no instrument found to validate");
        return;
    };
    println!("building: {} ({})", info.name, info.id);

    // 3) Build a live audio instance and drive notes + CC + pitch-bend.
    let (sr, block) = (48_000u32, 512u32);
    let mut host = ClapHost::new();
    host.scan(&scan_dir).expect("scan");
    let Some(mut src) = host.create_audio_source(&info.id, sr, block) else {
        println!("RESULT: create_audio_source returned None");
        return;
    };
    {
        let synth = src.as_synth().expect("instrument exposes AudioSynthPort");
        // Polyphonic expression: pitch-bend → per-note tuning, CC74 → brightness.
        synth.set_mpe(true, 48.0);
        synth.note_on(0, 60, 100);
        synth.control_change(0, 74, 90); // → Brightness note expression
        synth.pitch_bend(0, 4096);       // → Tuning note expression (~+12 st)
        synth.channel_pressure(0, 80);   // → Pressure note expression
    }

    let mut buf = vec![0.0f32; (block as usize) * 2];
    let mut peak = 0.0f32;
    for _ in 0..40 {
        // ~0.4s of audio
        let frames = src.render(&mut buf, sr);
        for s in &buf[..frames * 2] {
            peak = peak.max(s.abs());
        }
    }
    if let Some(synth) = src.as_synth() {
        synth.all_notes_off();
    }

    println!("after note+CC+bend: output peak = {peak:.6} (active={})", src.is_active());
    println!("RESULT: {}", if peak > 1e-4 { "PASS — audible output" } else { "SILENT (peak ~ 0)" });
}
