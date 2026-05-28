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
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // 8. Build App and run event loop.
    let settings            = load_settings();
    let recent_projects     = load_recent_projects();
    let recent_midi_imports = load_recent_midi_imports();

    let mut app = App::new(Arc::clone(&project), engine);
    app.settings            = settings;
    app.recent_projects     = recent_projects;
    app.recent_midi_imports = recent_midi_imports;

    // 9. Start audio engine using stored settings.
    {
        use seqterm_ports::AudioEngineConfig;
        let device = &app.settings.audio.device;
        let output_device = if device.is_empty() || device == "default" {
            None
        } else {
            Some(device.clone())
        };
        let audio_cfg = AudioEngineConfig {
            sample_rate:   app.settings.audio.sample_rate,
            buffer_size:   app.settings.audio.buffer_size,
            output_device,
            use_jack:      app.settings.audio.backend == "JACK",
            ..Default::default()
        };
        let mut audio_engine = AudioEngine::new(audio_cfg);
        match audio_engine.start() {
            Ok(()) => {
                app.engine.set_audio_latency(
                    app.settings.audio.buffer_size,
                    app.settings.audio.sample_rate,
                );
                info!(
                    "Audio engine started: {}Hz / {} frames",
                    app.settings.audio.sample_rate,
                    app.settings.audio.buffer_size
                );
            }
            Err(e) => {
                tracing::warn!("Audio engine failed to start: {e}");
            }
        }
        app.audio_engine = Some(audio_engine);
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
