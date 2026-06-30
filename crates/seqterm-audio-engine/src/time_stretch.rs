//! Offline WSOLA time-stretch (Phase F).
//!
//! Pitch-preserving time-stretch applied to decoded PCM *before* playback, so the
//! real-time path stays a plain sample player. `ratio > 1` makes the clip longer
//! (slower); `ratio < 1` shorter (faster). Pitch is preserved because WSOLA
//! resynthesises at the original sample rate using overlap-add of input windows,
//! choosing each window's offset to maximise waveform continuity (the "WS" in
//! WSOLA) — which suppresses the phase-discontinuity clicks plain OLA produces on
//! transients.
//!
//! Operates on interleaved f32 PCM. Alignment is computed once on the mono mix and
//! applied to every channel, keeping stereo phase-coherent.

use std::f32::consts::TAU;

/// Time-stretch interleaved `samples` (`channels`-interleaved) by `ratio`.
/// Returns new interleaved PCM ~`ratio×` the original length. A ratio of ~1.0
/// (or degenerate input) returns the samples unchanged.
pub fn time_stretch(samples: &[f32], channels: u16, ratio: f32) -> Vec<f32> {
    let ch = channels.max(1) as usize;
    let frames = samples.len() / ch;
    if !ratio.is_finite() || ratio <= 0.0 || (ratio - 1.0).abs() < 1e-3 || frames < 64 {
        return samples.to_vec();
    }

    let plane = |c: usize, f: usize| samples[f * ch + c];
    // Mono mix for the cross-correlation search.
    let mono: Vec<f32> = (0..frames)
        .map(|f| (0..ch).map(|c| plane(c, f)).sum::<f32>() / ch as f32)
        .collect();

    let win = 1024.min(frames / 2).max(64);
    let hs = (win / 4).max(1); // synthesis hop (fixed)
    let ha = (hs as f32 / ratio).max(1.0); // analysis hop (drives input advance)
    let search = (win / 2).min(256) as isize;
    let hann: Vec<f32> = (0..win)
        .map(|i| 0.5 - 0.5 * (TAU * i as f32 / (win as f32 - 1.0)).cos())
        .collect();

    let out_frames = (frames as f32 * ratio).ceil() as usize + win + 1;
    let mut out = vec![0.0f32; out_frames * ch];
    let mut wsum = vec![0.0f32; out_frames];

    let mut s: usize = 0;
    let mut a_ideal: f32 = 0.0;
    let mut prev_a: isize = 0;
    let mut first = true;

    while s + win < out_frames {
        let cand_center = a_ideal.round() as isize;
        let chosen_a = if first {
            first = false;
            cand_center.clamp(0, (frames - win) as isize)
        } else {
            // Template = the input that would naturally follow the previously
            // placed window by `hs` frames. Pick the offset whose window best
            // continues it (normalised cross-correlation).
            let mut best = f32::NEG_INFINITY;
            let mut best_off = 0isize;
            for off in -search..=search {
                let a = cand_center + off;
                if a < 0 || a as usize + win > frames {
                    continue;
                }
                let mut corr = 0.0f32;
                let mut energy = 1e-9f32;
                let mut k = 0;
                while k < win {
                    let t_idx = prev_a + hs as isize + k as isize;
                    let t = if t_idx >= 0 && (t_idx as usize) < frames {
                        mono[t_idx as usize]
                    } else {
                        0.0
                    };
                    let c = mono[a as usize + k];
                    corr += t * c;
                    energy += c * c;
                    k += 2; // stride: halves the search cost, ample for alignment
                }
                let score = corr / energy.sqrt();
                if score > best {
                    best = score;
                    best_off = off;
                }
            }
            (cand_center + best_off).clamp(0, (frames - win) as isize)
        };

        // Overlap-add this window into every channel.
        for k in 0..win {
            let src = chosen_a as usize + k;
            if src >= frames {
                break;
            }
            let w = hann[k];
            for c in 0..ch {
                out[(s + k) * ch + c] += plane(c, src) * w;
            }
            wsum[s + k] += w;
        }

        prev_a = chosen_a;
        s += hs;
        a_ideal += ha;
        if chosen_a as usize + win >= frames && a_ideal as usize >= frames {
            break;
        }
    }

    // Normalise the overlap window sum.
    for f in 0..out_frames {
        let w = wsum[f];
        if w > 1e-6 {
            for c in 0..ch {
                out[f * ch + c] /= w;
            }
        }
    }

    // Trim to the expected stretched length.
    let target = ((frames as f32 * ratio) as usize).saturating_mul(ch).min(out.len());
    out.truncate(target);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(freq: f32, sr: f32, frames: usize, ch: u16) -> Vec<f32> {
        let mut v = Vec::with_capacity(frames * ch as usize);
        for f in 0..frames {
            let s = (TAU * freq * f as f32 / sr).sin();
            for _ in 0..ch {
                v.push(s);
            }
        }
        v
    }

    #[test]
    fn identity_ratio_returns_input() {
        let x = sine(440.0, 48_000.0, 4096, 2);
        let y = time_stretch(&x, 2, 1.0);
        assert_eq!(x.len(), y.len());
    }

    #[test]
    fn stretch_lengthens_and_compress_shortens() {
        let frames = 8192;
        let x = sine(220.0, 48_000.0, frames, 1);

        let longer = time_stretch(&x, 1, 1.5);
        let lf = longer.len();
        assert!(
            (lf as f32 / frames as f32 - 1.5).abs() < 0.05,
            "1.5× ratio → ~1.5× frames, got {}",
            lf as f32 / frames as f32
        );
        assert!(longer.iter().all(|s| s.is_finite()));
        // Non-trivial energy survived the resynthesis.
        let rms = (longer.iter().map(|s| s * s).sum::<f32>() / lf as f32).sqrt();
        assert!(rms > 0.1, "stretched signal kept energy, rms={rms}");

        let shorter = time_stretch(&x, 1, 0.5);
        assert!(
            (shorter.len() as f32 / frames as f32 - 0.5).abs() < 0.05,
            "0.5× ratio → ~0.5× frames"
        );
    }

    #[test]
    fn stereo_stays_interleaved() {
        let x = sine(330.0, 48_000.0, 4096, 2);
        let y = time_stretch(&x, 2, 1.25);
        assert_eq!(y.len() % 2, 0, "stereo output stays frame-aligned");
    }
}
