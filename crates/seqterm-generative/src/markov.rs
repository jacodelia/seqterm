use std::collections::HashMap;

/// A first-order Markov chain for note sequences.
///
/// States are MIDI note numbers (u8).
pub struct MarkovChain {
    /// Transition counts: from_note -> (to_note -> count).
    transitions: HashMap<u8, HashMap<u8, u32>>,
    /// Total transitions from each state (for normalisation).
    totals: HashMap<u8, u32>,
}

impl MarkovChain {
    pub fn new() -> Self {
        Self {
            transitions: HashMap::new(),
            totals: HashMap::new(),
        }
    }

    /// Train the chain from a sequence of MIDI notes.
    pub fn train(&mut self, sequence: &[u8]) {
        for window in sequence.windows(2) {
            let from = window[0];
            let to = window[1];
            *self
                .transitions
                .entry(from)
                .or_default()
                .entry(to)
                .or_insert(0) += 1;
            *self.totals.entry(from).or_insert(0) += 1;
        }
    }

    /// Generate `count` notes starting from `seed` using a simple LCG RNG.
    pub fn generate(&self, seed: u8, count: usize, rng_seed: u64) -> Vec<u8> {
        let mut out = Vec::with_capacity(count);
        let mut current = seed;
        let mut rng = LcgRng::new(rng_seed);

        out.push(current);
        for _ in 1..count {
            if let Some(targets) = self.transitions.get(&current) {
                let total = *self.totals.get(&current).unwrap_or(&1) as f64;
                let pick = (rng.next_f64() * total) as u32;
                let mut acc = 0u32;
                let mut chosen = current;
                for (&note, &count) in targets {
                    acc += count;
                    if acc > pick {
                        chosen = note;
                        break;
                    }
                }
                current = chosen;
            }
            // If no transitions, stay on current note.
            out.push(current);
        }
        out
    }

    /// Reset all learned transitions.
    pub fn reset(&mut self) {
        self.transitions.clear();
        self.totals.clear();
    }
}

impl Default for MarkovChain {
    fn default() -> Self {
        Self::new()
    }
}

/// A mutation engine that randomly alters pattern steps.
pub struct MutationEngine {
    /// Probability of mutating each step (0.0-1.0).
    pub rate: f32,
    pub rng: LcgRng,
}

impl MutationEngine {
    pub fn new(rate: f32) -> Self {
        Self {
            rate: rate.clamp(0.0, 1.0),
            rng: LcgRng::new(0xDEAD_BEEF),
        }
    }

    /// Mutate a sequence of MIDI notes in-place.
    /// Steps may be transposed by ±semitones, silenced, or doubled.
    pub fn mutate(&mut self, notes: &mut Vec<Option<u8>>, semitone_range: i8) {
        for slot in notes.iter_mut() {
            if self.rng.next_f64() < self.rate as f64 {
                match slot {
                    Some(note) => {
                        let delta = (self.rng.next_f64() * (semitone_range as f64 * 2.0 + 1.0))
                            as i16
                            - semitone_range as i16;
                        let new_note = (*note as i16 + delta).clamp(0, 127) as u8;
                        *note = new_note;
                    }
                    None => {
                        // Randomly insert a note.
                        if self.rng.next_f64() < 0.3 {
                            *slot = Some(60); // middle C as placeholder
                        }
                    }
                }
            }
        }
    }
}

/// Applies probability gating to a sequence.
pub struct StochasticGate {
    pub rng: LcgRng,
}

impl StochasticGate {
    pub fn new(seed: u64) -> Self {
        Self {
            rng: LcgRng::new(seed),
        }
    }

    /// Returns whether a step with the given probability (0-100) should trigger.
    pub fn should_trigger(&mut self, prob: u8) -> bool {
        if prob >= 100 {
            return true;
        }
        if prob == 0 {
            return false;
        }
        (self.rng.next_f64() * 100.0) < prob as f64
    }
}

/// Minimal linear congruential generator for no-std-compatible randomness.
pub struct LcgRng {
    state: u64,
}

impl LcgRng {
    pub fn new(seed: u64) -> Self {
        Self {
            state: seed ^ 0x6c62_272e_07bb_0142,
        }
    }

    pub fn next_u64(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.state
    }

    pub fn next_f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }
}
