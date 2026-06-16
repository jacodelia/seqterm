//! SF2 → editable instrument loader.
//!
//! Parses a SoundFont2 file's hydra structure into SeqTerm's **editable**
//! [`Sf2Instrument`] model (zones + envelope/filter/LFO/loop generators) *and*
//! extracts each referenced sample's PCM into an [`Sf2SampleData`] pool, so the
//! instrument can be played by SeqTerm's own sampler ([`crate::sf2_sampler`])
//! instead of fluidsynth. This is what lets the EDITOR edit an SF2 zone and hear
//! the change.
//!
//! The generator → model conversion is a pragmatic subset of the SF2 spec aimed
//! at editing fidelity (not bit-exact playback): instrument-zone generators are
//! read as absolute values, preset-zone key/velocity ranges intersect, and the
//! remaining preset-level offsets are ignored. SF2 unit conversions (timecents,
//! absolute cents, centibels) are applied so the model carries seconds/Hz/dB.

use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};
use soundfont::data::GeneratorType;
use soundfont::SoundFont2;

use seqterm_core::{Sf2Instrument, Sf2LoopMode, Sf2Zone};

/// Decoded PCM + loop window for one SF2 sample (mono f32).
#[derive(Debug, Clone)]
pub struct Sf2SampleData {
    pub name: String,
    /// Mono PCM, normalised to roughly [-1, 1].
    pub pcm: Arc<[f32]>,
    /// The sample's own recorded rate (resampled by the player to the engine rate).
    pub sample_rate: u32,
    /// Loop window, in frames relative to `pcm[0]`.
    pub loop_start: u32,
    pub loop_end: u32,
    /// MIDI note the sample was recorded at.
    pub root_key: u8,
    /// Pitch correction, cents.
    pub pitch_correction: i8,
}

/// A fully loaded, playable SF2 instrument: the editable model plus the PCM pool
/// its zones reference by `sample_name`.
#[derive(Debug, Clone)]
pub struct LoadedSf2 {
    pub instrument: Sf2Instrument,
    pub samples: Vec<Sf2SampleData>,
}

impl LoadedSf2 {
    /// Look up a zone's sample PCM by name.
    pub fn sample(&self, name: &str) -> Option<&Sf2SampleData> {
        self.samples.iter().find(|s| s.name == name)
    }
}

// ── SF2 unit conversions ────────────────────────────────────────────────────

/// Timecents → seconds (`2^(tc/1200)`). SF2's "instant" default is -12000 tc.
fn timecents_to_secs(tc: i16) -> f32 {
    if tc <= -12000 { return 0.0; }
    2f32.powf(tc as f32 / 1200.0)
}

/// Absolute cents → Hz (`8.176 * 2^(c/1200)`), used for filter cutoff & LFO freq.
fn abs_cents_to_hz(c: i16) -> f32 {
    8.176 * 2f32.powf(c as f32 / 1200.0)
}

/// Initial-attenuation centibels (positive = quieter) → gain dB.
fn attenuation_cb_to_db(cb: i16) -> f32 {
    -(cb as f32) / 10.0
}

/// Filter Q centibels → a normalised 0..1 resonance (≈40 dB maps to full).
fn filter_q_cb_to_res(cb: i16) -> f32 {
    ((cb as f32 / 10.0) / 40.0).clamp(0.0, 1.0)
}

// ── Generator lookup helpers ────────────────────────────────────────────────

fn gen_i16(gens: &[soundfont::data::Generator], ty: GeneratorType) -> Option<i16> {
    gens.iter().rev().find(|g| g.ty == ty).and_then(|g| g.amount.as_i16().copied())
}

fn gen_u16(gens: &[soundfont::data::Generator], ty: GeneratorType) -> Option<u16> {
    gens.iter().rev().find(|g| g.ty == ty).and_then(|g| g.amount.as_u16().copied())
}

fn gen_range(gens: &[soundfont::data::Generator], ty: GeneratorType) -> Option<(u8, u8)> {
    gens.iter().rev().find(|g| g.ty == ty)
        .and_then(|g| g.amount.as_range().map(|r| (r.low, r.high)))
}

/// Read the whole `smpl` chunk into a shared i16 PCM pool.
fn read_sample_pool(path: &Path, sf: &SoundFont2) -> Result<Arc<[i16]>> {
    let Some(smpl) = sf.sample_data.smpl.as_ref() else {
        return Ok(Arc::from(Vec::<i16>::new()));
    };
    let file = std::fs::File::open(path).with_context(|| format!("reopen sf2 {path:?}"))?;
    let mut reader = std::io::BufReader::new(file);
    let bytes = smpl.read_contents(&mut reader).context("read smpl chunk")?;
    let pool: Vec<i16> = bytes
        .chunks_exact(2)
        .map(|b| i16::from_le_bytes([b[0], b[1]]))
        .collect();
    Ok(Arc::from(pool))
}

/// Load `(bank, preset)` from an SF2 file into an editable + playable instrument.
///
/// Call from a non-realtime background thread (does file I/O and allocation).
pub fn load_sf2_instrument(path: &Path, bank: u8, preset: u8) -> Result<LoadedSf2> {
    let file = std::fs::File::open(path).with_context(|| format!("open sf2 {path:?}"))?;
    let sf = SoundFont2::load(&mut std::io::BufReader::new(file))
        .map_err(|e| anyhow::anyhow!("parse sf2 {path:?}: {e:?}"))?;
    let pool = read_sample_pool(path, &sf)?;

    let preset_obj = sf.presets.iter()
        .find(|p| p.header.bank as u8 == bank && p.header.preset as u8 == preset)
        .or_else(|| sf.presets.iter().find(|p| p.header.name != "EOP"))
        .context("no matching preset in sf2")?;

    let mut zones: Vec<Sf2Zone> = Vec::new();
    let mut samples: Vec<Sf2SampleData> = Vec::new();

    for pz in &preset_obj.zones {
        // Preset zone → which instrument, plus an optional key/vel range to intersect.
        let Some(inst_id) = gen_u16(&pz.gen_list, GeneratorType::Instrument) else { continue };
        let pre_key = gen_range(&pz.gen_list, GeneratorType::KeyRange);
        let pre_vel = gen_range(&pz.gen_list, GeneratorType::VelRange);
        let Some(inst) = sf.instruments.get(inst_id as usize) else { continue };

        for iz in &inst.zones {
            let Some(sample_id) = gen_u16(&iz.gen_list, GeneratorType::SampleID) else { continue };
            let Some(hdr) = sf.sample_headers.get(sample_id as usize) else { continue };

            // ── Build the editable zone from instrument-zone generators ──────
            let mut z = Sf2Zone::new(hdr.name.clone());

            if let Some((lo, hi)) = gen_range(&iz.gen_list, GeneratorType::KeyRange) {
                z.key_low = lo; z.key_high = hi;
            }
            if let Some((lo, hi)) = gen_range(&iz.gen_list, GeneratorType::VelRange) {
                z.vel_low = lo; z.vel_high = hi;
            }
            // Intersect with the preset zone's ranges.
            if let Some((lo, hi)) = pre_key { z.key_low = z.key_low.max(lo); z.key_high = z.key_high.min(hi); }
            if let Some((lo, hi)) = pre_vel { z.vel_low = z.vel_low.max(lo); z.vel_high = z.vel_high.min(hi); }

            z.root_key = gen_i16(&iz.gen_list, GeneratorType::OverridingRootKey)
                .filter(|&k| (0..=127).contains(&k))
                .map(|k| k as u8)
                .unwrap_or(hdr.origpitch);
            z.coarse_tune = gen_i16(&iz.gen_list, GeneratorType::CoarseTune).unwrap_or(0) as i32;
            z.fine_tune   = gen_i16(&iz.gen_list, GeneratorType::FineTune).unwrap_or(0) as i32;

            z.attack  = timecents_to_secs(gen_i16(&iz.gen_list, GeneratorType::AttackVolEnv).unwrap_or(-12000));
            z.hold    = timecents_to_secs(gen_i16(&iz.gen_list, GeneratorType::HoldVolEnv).unwrap_or(-12000));
            z.decay   = timecents_to_secs(gen_i16(&iz.gen_list, GeneratorType::DecayVolEnv).unwrap_or(-12000));
            z.release = timecents_to_secs(gen_i16(&iz.gen_list, GeneratorType::ReleaseVolEnv).unwrap_or(-12000));
            // Sustain generator is attenuation in centibels (0 = full); convert to 0..1.
            let sus_cb = gen_i16(&iz.gen_list, GeneratorType::SustainVolEnv).unwrap_or(0).max(0);
            z.sustain = (1.0 - (sus_cb as f32 / 1000.0)).clamp(0.0, 1.0);

            z.cutoff = gen_i16(&iz.gen_list, GeneratorType::InitialFilterFc)
                .map(abs_cents_to_hz).unwrap_or(20_000.0).clamp(20.0, 20_000.0);
            z.resonance = gen_i16(&iz.gen_list, GeneratorType::InitialFilterQ)
                .map(filter_q_cb_to_res).unwrap_or(0.0);

            if let Some(freq) = gen_i16(&iz.gen_list, GeneratorType::FreqModLFO) {
                z.lfo_freq = abs_cents_to_hz(freq);
            }
            z.lfo_delay = timecents_to_secs(gen_i16(&iz.gen_list, GeneratorType::DelayModLFO).unwrap_or(-12000));

            z.gain_db = gen_i16(&iz.gen_list, GeneratorType::InitialAttenuation)
                .map(attenuation_cb_to_db).unwrap_or(0.0);

            z.loop_mode = match gen_i16(&iz.gen_list, GeneratorType::SampleModes).unwrap_or(0) {
                1 | 3 => Sf2LoopMode::Forward,
                _ => Sf2LoopMode::None,
            };
            z.loop_start = hdr.loop_start.saturating_sub(hdr.start);
            z.loop_end   = hdr.loop_end.saturating_sub(hdr.start);

            zones.push(z);

            // ── Extract the sample PCM once per unique sample name ───────────
            if !samples.iter().any(|s| s.name == hdr.name) {
                let start = hdr.start as usize;
                let end = (hdr.end as usize).min(pool.len()).max(start);
                let pcm: Vec<f32> = pool[start..end].iter().map(|&s| s as f32 / 32768.0).collect();
                samples.push(Sf2SampleData {
                    name: hdr.name.clone(),
                    pcm: Arc::from(pcm),
                    sample_rate: hdr.sample_rate.max(1),
                    loop_start: hdr.loop_start.saturating_sub(hdr.start),
                    loop_end: hdr.loop_end.saturating_sub(hdr.start),
                    root_key: hdr.origpitch,
                    pitch_correction: hdr.pitchadj,
                });
            }
        }
    }

    if zones.is_empty() {
        anyhow::bail!("sf2 preset {bank}:{preset} produced no playable zones");
    }

    let instrument = Sf2Instrument {
        name: preset_obj.header.name.clone(),
        zones,
        selected: 0,
    };
    Ok(LoadedSf2 { instrument, samples })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_conversions_have_expected_anchors() {
        // 0 timecents = 1 second; the "instant" sentinel collapses to 0.
        assert!((timecents_to_secs(0) - 1.0).abs() < 1e-4);
        assert_eq!(timecents_to_secs(-12000), 0.0);
        // 6900 abs cents ≈ 440 Hz (A4) via the 8.176 Hz anchor.
        assert!((abs_cents_to_hz(6900) - 440.0).abs() < 5.0);
        // 100 cB attenuation = -10 dB.
        assert!((attenuation_cb_to_db(100) + 10.0).abs() < 1e-4);
    }

    #[test]
    fn missing_file_errors_cleanly() {
        let r = load_sf2_instrument(Path::new("/nonexistent/x.sf2"), 0, 0);
        assert!(r.is_err());
    }
}
