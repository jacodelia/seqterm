use std::{
    io,
    path::Path,
    sync::Arc,
    time::Duration,
};

use anyhow::Result;
use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use parking_lot::Mutex;
use ratatui::{Terminal, backend::CrosstermBackend};
use seqterm_audio_engine::AudioEngine;
use seqterm_engine::PlaybackEngine;
use seqterm_persistence::{Autosave, load_or_default, load_recent_projects, load_recent_midi_imports, load_settings, save_project};
use seqterm_ui::{app::App, run_app};
use tracing::info;
use tracing_subscriber::{EnvFilter, fmt};

fn main() -> Result<()> {
    // 0. Suppress ALSA C-library stderr spam (e.g. "open /dev/snd/seq failed")
    //    before any MIDI or audio initialisation touches ALSA.
    seqterm_midi::suppress_alsa_stderr();

    // 1. Initialize tracing.
    init_tracing();
    info!("SeqTerm-rs starting up");

    // Install panic hook: log the panic to seqterm.log, restore the terminal
    // so the output is visible, then resume the default panic behaviour.
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        // Restore terminal before printing so panic text is readable. The TUI
        // draws to /dev/tty (fd 1/2 are redirected to the log), so restore there.
        let _ = crossterm::terminal::disable_raw_mode();
        if let Ok(mut tty) = std::fs::OpenOptions::new().write(true).open("/dev/tty") {
            let _ = crossterm::execute!(
                tty,
                crossterm::terminal::LeaveAlternateScreen,
                crossterm::event::DisableMouseCapture,
            );
        }
        // Log to seqterm.log.
        let loc = info.location().map(|l| format!("{}:{}", l.file(), l.line())).unwrap_or_default();
        let msg = if let Some(s) = info.payload().downcast_ref::<&str>() {
            s.to_string()
        } else if let Some(s) = info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            "unknown panic payload".to_string()
        };
        tracing::error!(target: "panic", "PANIC at {loc}: {msg}");
        // Also write directly to the log file in case the tracing subscriber is gone.
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open("seqterm.log") {
            use std::io::Write;
            let _ = writeln!(f, "[PANIC] {loc}: {msg}");
        }
        default_hook(info);
    }));

    // 2. Load or create default project.
    let project_path = Path::new("projects/demo.json");
    let project = load_or_default(project_path);
    let project = Arc::new(Mutex::new(project));

    // 3. Open direct ALSA output connections for each routed clip destination.
    //    Uses a single "SeqTerm" ALSA client — no aconnect, no client exhaustion.
    let destinations: Vec<String> = {
        use std::collections::HashSet;
        let proj = project.lock();
        let mut seen = HashSet::new();
        proj.matrix.values()
            .flat_map(|slots| slots.iter().flatten())
            .filter_map(|clip| clip.midi_out.clone())
            .filter(|d| seen.insert(d.clone()))
            .collect()
    };
    let midi_ports = seqterm_midi::open_output_connections(&destinations);
    info!("Opened direct MIDI output to {} destination(s)", midi_ports.len());

    // 4. Start playback engine with destination-keyed MIDI ports.
    let engine = PlaybackEngine::start_with_midi(Arc::clone(&project), midi_ports);

    // 5. Start autosave (every 60 seconds).
    let _autosave = Autosave::start(
        Arc::clone(&project),
        project_path.to_path_buf(),
        Duration::from_secs(60),
    );

    // 6. Log available MIDI ports for diagnostics.
    match seqterm_midi::list_output_ports() {
        Ok(ports) => info!("Available MIDI outputs: {:?}", ports),
        Err(e) => tracing::warn!("MIDI port enumeration failed: {e}"),
    }

    // 7. Set up crossterm terminal with mouse support.
    //
    // Plugin scanning dlopens third-party libraries (VST2/LADSPA) that print to
    // stdout/stderr (e.g. "convolution: samplerate mismatch"). Since the scan now
    // runs on a background thread, that output would land on the live TUI. Route
    // the process's fd 1/2 to the log and draw the TUI to the controlling
    // terminal (/dev/tty) directly, so plugin noise can never corrupt the screen.
    let tui_writer: Box<dyn io::Write + Send> = open_tui_writer();
    enable_raw_mode()?;
    let mut writer = tui_writer;
    execute!(writer, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(writer);
    let mut terminal = Terminal::new(backend)?;

    // 8. Build App and run event loop.
    let settings            = load_settings();
    let recent_projects     = load_recent_projects();
    let recent_midi_imports = load_recent_midi_imports();

    let mut app = App::new(Arc::clone(&project), engine);
    app.settings            = settings;
    app.recent_projects     = recent_projects;
    app.recent_midi_imports = recent_midi_imports;

    // Select the SF2 sample engine before any SoundFont is loaded. FluidSynth
    // only takes effect in a build with the `fluidsynth` feature + libfluidsynth
    // present; otherwise SoundFontSynth transparently falls back to oxisynth.
    seqterm_audio_engine::set_sf2_prefer_fluidsynth(
        app.settings.audio.sf2_backend.eq_ignore_ascii_case("fluidsynth"),
    );

    // 9. Start audio engine using stored settings.
    {
        use seqterm_ports::AudioEngineConfig;
        let device = &app.settings.audio.device;
        let output_device = if device.is_empty() || device == "default" {
            None
        } else {
            Some(device.clone())
        };
        let backend = app.settings.audio.backend.to_uppercase();
        // "AUTO": prefer PipeWire-JACK when available, then JACK, then ALSA.
        let pw_running = seqterm_audio_engine::pipewire_is_running();
        let use_jack = matches!(backend.as_str(), "JACK" | "PIPEWIRE")
            || (backend == "AUTO" && pw_running)
            || (backend == "AUTO"
                && std::process::Command::new("jack_lsp")
                    .output()
                    .map(|o| o.status.success())
                    .unwrap_or(false));
        let pipewire_quantum = app.settings.audio.pipewire_quantum;

        let audio_cfg = AudioEngineConfig {
            sample_rate:   app.settings.audio.sample_rate,
            buffer_size:   app.settings.audio.buffer_size,
            output_device,
            use_jack,
            pipewire_quantum,
            ..Default::default()
        };
        let mut audio_engine = AudioEngine::new(audio_cfg.clone());
        match audio_engine.start() {
            Ok(()) => {
                // For JACK, StreamStarted event will call set_audio_latency with the
                // real buffer size. For CPAL/ALSA, use the configured values now.
                if !use_jack {
                    app.engine.set_audio_latency(
                        audio_cfg.buffer_size,
                        audio_cfg.sample_rate,
                    );
                }
                info!(
                    "Audio engine started ({}) {}Hz / {} frames",
                    backend, audio_cfg.sample_rate, audio_cfg.buffer_size
                );
            }
            Err(ref e) if use_jack => {
                tracing::warn!("JACK failed ({e}), retrying with ALSA");
                let fallback = AudioEngineConfig { use_jack: false, ..audio_cfg };
                audio_engine = AudioEngine::new(fallback.clone());
                match audio_engine.start() {
                    Ok(()) => {
                        app.engine.set_audio_latency(fallback.buffer_size, fallback.sample_rate);
                        info!("Audio engine started via ALSA fallback");
                    }
                    Err(e2) => tracing::warn!("ALSA fallback also failed: {e2}"),
                }
            }
            Err(e) => tracing::warn!("Audio engine failed to start: {e}"),
        }
        app.audio_engine = Some(audio_engine);

        // Load SF2 / audio clips from the startup project into the audio engine.
        seqterm_ui::rebuild_audio_slots(&mut app);
    }

    let result = run_app(&mut terminal, &mut app);

    // 9. On exit: restore terminal and disable mouse.
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    // 10. Save project on clean exit.
    {
        let proj = project.lock();
        if let Err(e) = save_project(&proj, project_path) {
            eprintln!("Warning: failed to save project on exit: {e}");
        } else {
            println!("Project saved to {}", project_path.display());
        }
    }

    if let Err(e) = result {
        eprintln!("Application error: {e}");
        std::process::exit(1);
    }

    info!("SeqTerm-rs exited cleanly");
    Ok(())
}

/// Build the writer the TUI draws to. On Unix, open the controlling terminal
/// (`/dev/tty`) for the TUI and redirect the process's stdout/stderr (fd 1/2) to
/// `seqterm.log`, so any C-library/plugin output during scanning goes to the log
/// instead of corrupting the screen. Falls back to plain stdout if `/dev/tty`
/// is unavailable (e.g. output piped), in which case fds are left untouched.
fn open_tui_writer() -> Box<dyn io::Write + Send> {
    #[cfg(unix)]
    {
        use std::os::unix::io::AsRawFd;
        if let Ok(tty) = std::fs::OpenOptions::new().write(true).open("/dev/tty") {
            if let Ok(log) = std::fs::OpenOptions::new().create(true).append(true).open("seqterm.log") {
                // Point fd 1 and fd 2 at the log; the dup'd targets stay valid
                // after `log` is dropped (it closes only its own descriptor).
                unsafe {
                    libc::dup2(log.as_raw_fd(), libc::STDOUT_FILENO);
                    libc::dup2(log.as_raw_fd(), libc::STDERR_FILENO);
                }
            }
            return Box::new(tty);
        }
    }
    Box::new(io::stdout())
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("seqterm=info,warn"));

    fmt()
        .with_env_filter(filter)
        .with_target(false)
        .with_thread_names(true)
        .with_writer(|| {
            // Write logs to a file to avoid polluting the terminal.
            use std::fs::OpenOptions;
            OpenOptions::new()
                .create(true)
                .append(true)
                .open("seqterm.log")
                .unwrap_or_else(|_| {
                    // Fall back to /dev/null if we can't open the log file.
                    unsafe { std::mem::transmute(std::fs::File::open("/dev/null").unwrap()) }
                })
        })
        .init();
}
