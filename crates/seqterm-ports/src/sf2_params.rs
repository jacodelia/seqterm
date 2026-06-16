//! Universal [`ParameterProvider`] view of an editable SF2 zone.
//!
//! Exposes a [`seqterm_core::Sf2Zone`]'s generators (envelope, filter, LFO, zone
//! mapping, loop, gain) as typed universal parameters, so the same
//! auto-generated inspector that edits plugins also edits SoundFont zones —
//! with the correct widget per generator (Integer spin for key ranges, Enum
//! selector for filter type / LFO waveform / loop mode, Float slider for the
//! continuous controls).

use seqterm_core::sf2_instrument::{Sf2FilterType, Sf2LfoWaveform, Sf2LoopMode, Sf2Zone};

use crate::instrument::{Parameter, ParameterProvider, ParameterType};

/// Borrowing editor wrapping a mutable [`Sf2Zone`] as a [`ParameterProvider`].
pub struct Sf2ZoneParams<'a> {
    zone: &'a mut Sf2Zone,
}

impl<'a> Sf2ZoneParams<'a> {
    pub fn new(zone: &'a mut Sf2Zone) -> Self { Self { zone } }
}

/// Number of editable parameters exposed for a zone.
const PARAM_COUNT: usize = 25;

fn int(id: &str, name: &str, v: f64, min: f64, max: f64, unit: &str) -> Parameter {
    Parameter {
        id: id.into(), name: name.into(), kind: ParameterType::Integer,
        value: v, minimum: min, maximum: max, default: v, unit: unit.into(),
        automatable: false, modulatable: false, read_only: false, enum_values: Vec::new(),
    }
}

fn float(id: &str, name: &str, v: f64, min: f64, max: f64, unit: &str, modulatable: bool) -> Parameter {
    let mut p = Parameter::float(id, name, v, min, max).with_unit(unit);
    if !modulatable { p = p.non_modulatable(); }
    p
}

fn enumer(id: &str, name: &str, idx: usize, choices: &[&str]) -> Parameter {
    Parameter::enumerated(id, name, idx, choices.iter().map(|s| s.to_string()).collect())
}

impl ParameterProvider for Sf2ZoneParams<'_> {
    fn parameter_count(&self) -> usize { PARAM_COUNT }

    fn parameter(&self, index: usize) -> Option<Parameter> {
        let z = &*self.zone;
        let p = match index {
            0  => int("key_low", "Key Low", z.key_low as f64, 0.0, 127.0, ""),
            1  => int("key_high", "Key High", z.key_high as f64, 0.0, 127.0, ""),
            2  => int("vel_low", "Vel Low", z.vel_low as f64, 0.0, 127.0, ""),
            3  => int("vel_high", "Vel High", z.vel_high as f64, 0.0, 127.0, ""),
            4  => int("root_key", "Root Key", z.root_key as f64, 0.0, 127.0, ""),
            5  => int("fine_tune", "Fine Tune", z.fine_tune as f64, -100.0, 100.0, "ct"),
            6  => int("coarse_tune", "Coarse Tune", z.coarse_tune as f64, -64.0, 64.0, "st"),
            7  => float("attack", "Attack", z.attack as f64, 0.0, 10.0, "s", true),
            8  => float("hold", "Hold", z.hold as f64, 0.0, 10.0, "s", true),
            9  => float("decay", "Decay", z.decay as f64, 0.0, 10.0, "s", true),
            10 => float("sustain", "Sustain", z.sustain as f64, 0.0, 1.0, "", true),
            11 => float("release", "Release", z.release as f64, 0.0, 10.0, "s", true),
            12 => enumer("filter_type", "Filter Type",
                         Sf2FilterType::ALL.iter().position(|f| *f == z.filter_type).unwrap_or(0),
                         &["LPF", "HPF", "BPF"]),
            13 => float("cutoff", "Cutoff", z.cutoff as f64, 20.0, 20_000.0, "Hz", true),
            14 => float("resonance", "Resonance", z.resonance as f64, 0.0, 1.0, "", true),
            15 => float("key_tracking", "Key Track", z.key_tracking as f64, 0.0, 1.0, "", true),
            16 => enumer("lfo_waveform", "LFO Wave",
                         Sf2LfoWaveform::ALL.iter().position(|w| *w == z.lfo_waveform).unwrap_or(0),
                         &["Sine", "Triangle", "Square", "Saw"]),
            17 => float("lfo_freq", "LFO Freq", z.lfo_freq as f64, 0.0, 20.0, "Hz", true),
            18 => float("lfo_delay", "LFO Delay", z.lfo_delay as f64, 0.0, 5.0, "s", true),
            19 => float("lfo_depth", "LFO Depth", z.lfo_depth as f64, 0.0, 1.0, "", true),
            20 => enumer("loop_mode", "Loop Mode",
                         Sf2LoopMode::ALL.iter().position(|m| *m == z.loop_mode).unwrap_or(0),
                         &["None", "Forward", "PingPong"]),
            21 => int("loop_start", "Loop Start", z.loop_start as f64, 0.0, 16_000_000.0, "smp"),
            22 => int("loop_end", "Loop End", z.loop_end as f64, 0.0, 16_000_000.0, "smp"),
            23 => float("loop_crossfade", "Loop XFade", z.loop_crossfade as f64, 0.0, 1000.0, "ms", false),
            24 => float("gain", "Gain", z.gain_db as f64, -60.0, 24.0, "dB", true),
            _ => return None,
        };
        Some(p)
    }

    fn set_parameter(&mut self, index: usize, value: f64) {
        let Some(desc) = self.parameter(index) else { return };
        let v = desc.sanitize(value);
        let z = &mut *self.zone;
        match index {
            0  => z.key_low = v as u8,
            1  => z.key_high = v as u8,
            2  => z.vel_low = v as u8,
            3  => z.vel_high = v as u8,
            4  => z.root_key = v as u8,
            5  => z.fine_tune = v as i32,
            6  => z.coarse_tune = v as i32,
            7  => z.attack = v as f32,
            8  => z.hold = v as f32,
            9  => z.decay = v as f32,
            10 => z.sustain = v as f32,
            11 => z.release = v as f32,
            12 => z.filter_type = Sf2FilterType::ALL.get(v as usize).copied().unwrap_or_default(),
            13 => z.cutoff = v as f32,
            14 => z.resonance = v as f32,
            15 => z.key_tracking = v as f32,
            16 => z.lfo_waveform = Sf2LfoWaveform::ALL.get(v as usize).copied().unwrap_or_default(),
            17 => z.lfo_freq = v as f32,
            18 => z.lfo_delay = v as f32,
            19 => z.lfo_depth = v as f32,
            20 => z.loop_mode = Sf2LoopMode::ALL.get(v as usize).copied().unwrap_or_default(),
            21 => z.loop_start = v as u32,
            22 => z.loop_end = v as u32,
            23 => z.loop_crossfade = v as f32,
            24 => z.gain_db = v as f32,
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exposes_expected_typed_params() {
        let mut z = Sf2Zone::new("piano");
        let p = Sf2ZoneParams::new(&mut z);
        assert_eq!(p.parameter_count(), PARAM_COUNT);
        // Integer zone-mapping param.
        let kl = p.parameter_by_id("key_low").unwrap().1;
        assert_eq!(kl.kind, ParameterType::Integer);
        // Enum filter type with 3 choices.
        let ft = p.parameter_by_id("filter_type").unwrap().1;
        assert_eq!(ft.kind, ParameterType::Enum);
        assert_eq!(ft.enum_values.len(), 3);
        // Float cutoff in Hz.
        let cut = p.parameter_by_id("cutoff").unwrap().1;
        assert_eq!(cut.kind, ParameterType::Float);
        assert_eq!(cut.unit, "Hz");
    }

    #[test]
    fn set_params_write_back_with_sanitize() {
        let mut z = Sf2Zone::new("p");
        {
            let mut p = Sf2ZoneParams::new(&mut z);
            // Integer rounds; out-of-range clamps.
            p.set_parameter_by_id("root_key", 200.0);   // clamps to 127
            p.set_parameter_by_id("coarse_tune", 3.6);  // rounds to 4
            // Enum picks the index.
            p.set_parameter_by_id("filter_type", 1.0);  // HPF
            p.set_parameter_by_id("cutoff", 1000.0);
        }
        assert_eq!(z.root_key, 127);
        assert_eq!(z.coarse_tune, 4);
        assert_eq!(z.filter_type, Sf2FilterType::HighPass);
        assert!((z.cutoff - 1000.0).abs() < 1e-3);
    }

    #[test]
    fn normalized_set_maps_into_native_range() {
        let mut z = Sf2Zone::new("p");
        {
            let mut p = Sf2ZoneParams::new(&mut z);
            // 0.0 normalised over [-60, 24] dB = -60.
            p.set_parameter_normalized(24, 0.0);
        }
        assert!((z.gain_db - (-60.0)).abs() < 1e-3);
    }
}
