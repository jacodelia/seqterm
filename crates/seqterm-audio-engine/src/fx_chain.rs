//! Build realtime FX processor chains from serializable specs.
//!
//! This is the single source of truth for mapping a normalised (0–1) parameter
//! set + a stable kind id onto a concrete [`FxProcessor`]. Both the live UI
//! (`seqterm-ui`'s `build_fx_chain`) and the offline export renderer build their
//! chains through here, so a chain sounds identical live and on export.

use seqterm_core::FxSpec;

use crate::fx::{
    Bitcrusher, Cassette, Chorus, Compressor, Expander, FilterBankFx, Flanger, Gain,
    Gate, GranularDelay, Isolator, Looper, Protocosmos, MonoMaker, Pan as PanFx, ParametricEq,
    Phaser, PhaseInvert, Reverb, ReverseDelay, SidechainDuck, SoftClipper, SpaceEcho,
    StereoWidener, Svf, SvfMode, TubeSaturation, VinylSim,
};
use crate::fx::delay::DelayLine;
use crate::FxProcessor;

/// Build a single FX processor from a stable `kind` id and normalised params.
///
/// Returns `None` for an unknown kind id. The returned processor's wet/dry mix
/// is left at its default — callers apply `set_mix(wet)` themselves.
pub fn build_processor(
    kind: &str,
    params: &[f32],
    sample_rate: u32,
) -> Option<Box<dyn FxProcessor>> {
    let p = |i: usize| params.get(i).copied().unwrap_or(0.0);
    let sr = sample_rate.max(1);

    let proc: Box<dyn FxProcessor> = match kind {
        "delay" => {
            let delay_ms = 10.0 + p(0) * 990.0;
            let feedback = p(1);
            let damping  = p(2);
            let mut d    = DelayLine::new(delay_ms, feedback, damping);
            d.set_ping_pong(p(3) > 0.5);
            d.set_crossfeed(p(5)); // L/R crossfeed (p(4) is Wet)
            Box::new(d)
        }
        "reverb" => {
            let mut r = Reverb::new(sr);
            r.set_room_size(p(0));
            r.set_damp(p(1));
            r.set_width(p(2)); // Width: 0 = mono, 1 = normal stereo
            Box::new(r)
        }
        // GranularDelay::new(delay_ms, feedback, scatter_st, density).
        // UI knobs are [Size, Density, Pitch, Feedback] → map p(1)=Density,
        // p(3)=Feedback to the matching args (previously swapped).
        "grandelay" => Box::new(GranularDelay::new(
            20.0 + p(0) * 980.0,
            p(3),
            (p(2) - 0.5) * 24.0,
            1.0 + p(1) * 31.0,
        )),
        "compressor" => {
            let mut c = Compressor::new();
            c.threshold_db = -(1.0 - p(0)) * 60.0;
            c.ratio        = 1.0 + p(1) * 19.0;
            c.attack_ms    = 0.1 + p(2) * 99.9;
            c.release_ms   = 10.0 + p(3) * 990.0;
            c.makeup_db    = p(4) * 24.0;
            c.knee_db      = p(5) * 12.0;
            Box::new(c)
        }
        "limiter" => {
            let mut lim = Compressor::limiter();
            lim.threshold_db = -(1.0 - p(0)) * 12.0;
            lim.release_ms   = 1.0 + p(1) * 199.0;
            Box::new(lim)
        }
        "gate" => {
            let mut g = Gate::new();
            g.threshold_db = -(1.0 - p(0)) * 80.0;
            g.attack_ms    = 0.1 + p(1) * 49.9;
            g.hold_ms      = 1.0 + p(2) * 499.0;
            g.release_ms   = 10.0 + p(3) * 990.0;
            g.floor_db     = -(1.0 - p(4)) * 80.0;
            Box::new(g)
        }
        "parameq" => {
            use crate::fx::parametric_eq::EqBandKind;
            let mut eq = ParametricEq::new();
            eq.bands[1].gain_db = (p(0) - 0.5) * 36.0;
            eq.bands[2].gain_db = (p(1) - 0.5) * 36.0;
            eq.bands[3].gain_db = (p(2) - 0.5) * 36.0;
            eq.bands[3].kind    = EqBandKind::HighShelf;
            eq.bands[3].gain_db = (p(3) - 0.5) * 36.0;
            eq.bands[1].freq    = 20.0 * (800.0f32 / 20.0).powf(p(4));
            eq.bands[3].freq    = 1000.0 * 20.0f32.powf(p(5));
            eq.bands[2].q       = 0.1 + p(6) * 9.9;
            Box::new(eq)
        }
        "filter" => {
            let freq = 20.0 + p(0) * 19980.0;
            let res  = p(1) * 4.0 + 0.5;
            Box::new(Svf::new(SvfMode::Lowpass, freq, res))
        }
        "filterbank" => {
            // UI exposes 3 macro knobs (Low/Mid/High); map each to a third of the
            // 48 bands as ±24 dB (0.5 = flat).
            let mut fb = FilterBankFx::new(sr);
            let gdb = |x: f32| (x - 0.5) * 48.0;
            let mut gains = [0.0f32; 48];
            for (b, g) in gains.iter_mut().enumerate() {
                *g = if b < 16 { gdb(p(0)) } else if b < 32 { gdb(p(1)) } else { gdb(p(2)) };
            }
            fb.set_all_gains(&gains);
            Box::new(fb)
        }
        "chorus" => {
            let mut c = Chorus::new();
            c.rate     = 0.05 + p(0) * 4.95;
            c.depth    = 0.5  + p(1) * 9.5;
            c.delay_ms = 5.0  + p(2) * 25.0;
            c.feedback = (p(3) - 0.5) * 1.8;
            Box::new(c)
        }
        "flanger" => {
            let mut f = Flanger::new();
            f.rate     = 0.05 + p(0) * 4.95;
            f.depth    = p(1) * 7.0;
            f.delay_ms = 0.5  + p(2) * 9.5;
            f.feedback = (p(3) - 0.5) * 1.9;
            Box::new(f)
        }
        "phaser" => {
            let mut ph = Phaser::new();
            ph.rate     = 0.05 + p(0) * 4.95;
            ph.depth    = p(1);
            ph.center   = 200.0 + p(2) * 1800.0;
            ph.feedback = (p(3) - 0.5) * 1.8;
            Box::new(ph)
        }
        "bitcrusher" => {
            let mut b = Bitcrusher::new();
            b.set_bits((1.0 + p(0) * 15.0) as u8);
            b.set_hold((1.0 + p(1) * 15.0) as u32);
            Box::new(b)
        }
        "vinyl" => {
            let mut v = VinylSim::new();
            v.set_wow(p(0) * 0.1);
            v.set_flutter(p(1) * 0.05);
            v.set_crackle(p(2));
            Box::new(v)
        }
        "cassette" => {
            let mut c = Cassette::new();
            c.set_drive(0.5 + p(0) * 7.5); // Drive knob → 0.5..8.0
            Box::new(c)
        }
        "softclip" => {
            let mut s = SoftClipper::new();
            s.drive = 1.0 + p(0) * 9.0;
            Box::new(s)
        }
        "tubesat" => {
            let mut t = TubeSaturation::new();
            t.drive = 1.0 + p(0) * 19.0;
            t.tone  = p(1);
            Box::new(t)
        }
        "widener" => {
            let mut w = StereoWidener::new();
            w.width = p(0) * 2.0;
            Box::new(w)
        }
        "isolator" => {
            // UI knobs Low/Mid/High are linear gains; 0.5 = unity (×1), 1.0 = ×2.
            let mut iso = Isolator::new();
            iso.set_gains(p(0) * 2.0, p(1) * 2.0, p(2) * 2.0);
            Box::new(iso)
        }
        "gain" => {
            let mut g = Gain::new();
            g.gain_db = (p(0) - 0.5) * 48.0;
            Box::new(g)
        }
        "phaseinvert" => Box::new(PhaseInvert { invert_l: p(0) > 0.5, invert_r: p(1) > 0.5 }),
        "monomaker" => Box::new(MonoMaker::new()),
        "looper" => Box::new(Looper::new(sr)),
        "sidechain" => Box::new(SidechainDuck::new()),
        "expander" => {
            let mut exp = Expander::new();
            exp.threshold_db = -(1.0 - p(0)) * 80.0;
            exp.ratio        = 1.0 + p(1) * 9.0;
            exp.attack_ms    = 0.1 + p(2) * 49.9;
            exp.release_ms   = 10.0 + p(3) * 990.0;
            exp.range_db     = p(4) * 80.0;
            Box::new(exp)
        }
        "pan" => {
            let mut pan = PanFx::new();
            pan.pan            = (p(0) - 0.5) * 2.0;
            pan.constant_power = p(1) > 0.5;
            Box::new(pan)
        }
        // Creative time/texture. Params: [Time,Feedback,Wow,Flutter,Age,Spring,Tone,Wet].
        "spaceecho" => Box::new(SpaceEcho::new(sr, p(0), p(1), p(2), p(3), p(4), p(5), p(6))),
        // Params: [Size,Density,Pitch,Spray,Reverse,Freeze,Diffuse,Wet].
        "protocosmos" => Box::new(Protocosmos::new(sr, p(0), p(1), p(2), p(3), p(4), p(5), p(6))),
        // Params: [Time,Feedback,Wet].
        "reverse" => Box::new(ReverseDelay::new(sr, p(0), p(1))),
        _ => return None,
    };
    Some(proc)
}

/// Build a full FX chain from a list of [`FxSpec`]s. Disabled entries are
/// skipped; unknown kinds are dropped. Each processor's wet/dry mix is set
/// from the spec.
pub fn build_chain_from_specs(specs: &[FxSpec], sample_rate: u32) -> Vec<Box<dyn FxProcessor>> {
    specs.iter()
        .filter(|s| s.enabled)
        .filter_map(|s| {
            let mut proc = build_processor(&s.kind, &s.params, sample_rate)?;
            proc.set_mix(s.wet);
            Some(proc)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(kind: &str, enabled: bool) -> FxSpec {
        FxSpec { kind: kind.to_string(), enabled, wet: 0.5, params: vec![0.5; 8] }
    }

    #[test]
    fn builds_known_kinds_and_skips_disabled_and_unknown() {
        let specs = vec![
            spec("delay", true),     // built
            spec("reverb", false),   // disabled → skipped
            spec("bogus-fx", true),  // unknown → dropped
            spec("compressor", true),// built
        ];
        let chain = build_chain_from_specs(&specs, 48_000);
        assert_eq!(chain.len(), 2, "only enabled, known kinds are built");
    }

    #[test]
    fn every_kind_id_builds() {
        // Guards against the UI's AudioFxKind::id() drifting from the builder.
        for kind in [
            "delay","reverb","grandelay","compressor","limiter","gate","parameq",
            "filter","filterbank","chorus","flanger","phaser","bitcrusher","vinyl",
            "cassette","softclip","tubesat","widener","isolator","gain","phaseinvert",
            "monomaker","looper","sidechain","expander","pan","spaceecho","protocosmos",
            "reverse",
        ] {
            assert!(
                build_processor(kind, &[0.5; 8], 48_000).is_some(),
                "kind '{kind}' should build a processor",
            );
        }
    }
}
