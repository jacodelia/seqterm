/// The playback / transport state owned by the scheduler.
#[derive(Debug, Clone)]
pub struct TransportState {
    pub playing: bool,
    /// True while paused: scheduler is stopped but position is preserved.
    pub paused: bool,
    pub recording: bool,
    pub bpm: f64,
    pub ppqn: u32,
    pub current_bar: usize,
    pub current_step: usize,
    pub current_tick: u32,
    pub active_pattern: Option<String>,
    /// Total elapsed ticks since last play.
    pub elapsed_ticks: u64,
}

impl Default for TransportState {
    fn default() -> Self {
        Self {
            playing: false,
            paused: false,
            recording: false,
            bpm: 128.0,
            // 480 PPQN matches standard MIDI files and gives ~1ms tick resolution
            // at 120 BPM — same granularity as FluidSynth.
            ppqn: 480,
            current_bar: 0,
            current_step: 0,
            current_tick: 0,
            active_pattern: None,
            elapsed_ticks: 0,
        }
    }
}

impl TransportState {
    /// Duration of one PPQN tick in microseconds.
    pub fn tick_duration_us(&self) -> u64 {
        // One quarter note = 60_000_000 / bpm µs
        // One tick = one quarter note / ppqn
        (60_000_000.0 / (self.bpm * self.ppqn as f64)) as u64
    }

    /// Duration of one PPQN tick in nanoseconds. Used by the scheduler clock so
    /// per-tick truncation error (≤1 ns) is negligible — at µs resolution the
    /// fractional microsecond dropped each tick accumulates into audible tempo
    /// drift (~270 µs/beat at 128 BPM / 480 PPQN).
    pub fn tick_duration_ns(&self) -> u64 {
        (60_000_000_000.0 / (self.bpm * self.ppqn as f64)) as u64
    }

    /// Steps per bar (assumes 4/4 at 16th-note grid).
    pub fn steps_per_bar(&self) -> usize {
        16
    }

    /// Advance one step globally. Each pattern is responsible for its own phase
    /// (`global_step % pat.length`). Bar advances every `steps_per_bar` (16) steps.
    pub fn advance_step(&mut self) -> bool {
        self.current_step += 1;
        let bar_advanced = self.current_step % self.steps_per_bar() == 0;
        if bar_advanced {
            self.current_bar += 1;
        }
        bar_advanced
    }

    pub fn reset(&mut self) {
        self.current_step = 0;
        self.current_tick = 0;
        self.current_bar = 0;
        self.elapsed_ticks = 0;
        self.paused = false;
    }
}
