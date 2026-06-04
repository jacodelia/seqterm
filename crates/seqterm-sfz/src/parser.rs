//! Minimal SFZ text parser.
//!
//! Handles the subset of SFZ used by most freely-available SFZ instruments:
//! `<group>`, `<region>`, `sample`, `lokey`/`hikey`, `pitch_keycenter`,
//! `lovel`/`hivel`, `volume`.

use std::path::{Path, PathBuf};
use anyhow::{bail, Context};
use crate::{SfzInstrument, SfzRegion};

/// Parse an SFZ file and return an `SfzInstrument`.
pub fn parse(path: &Path) -> anyhow::Result<SfzInstrument> {
    let base_dir = path.parent()
        .unwrap_or(Path::new("."))
        .to_path_buf();

    let text = std::fs::read_to_string(path)
        .with_context(|| format!("Cannot read SFZ file: {}", path.display()))?;

    let regions = parse_text(&text, &base_dir)?;
    if regions.is_empty() {
        bail!("SFZ file has no regions: {}", path.display());
    }

    Ok(SfzInstrument { regions, base_dir })
}

fn parse_text(text: &str, base_dir: &Path) -> anyhow::Result<Vec<SfzRegion>> {
    let mut regions: Vec<SfzRegion> = Vec::new();

    // Current group/region defaults.
    let mut in_region = false;
    let mut sample: Option<PathBuf> = None;
    let mut lo_key: u8  = 0;
    let mut hi_key: u8  = 127;
    let mut pkc:    u8  = 60;
    let mut lo_vel: u8  = 0;
    let mut hi_vel: u8  = 127;
    let mut gain:   f32 = 1.0;

    // Group-level defaults (inherited by regions).
    let mut group_lo_key: u8  = 0;
    let mut group_hi_key: u8  = 127;
    let mut group_pkc:    u8  = 60;
    let mut group_lo_vel: u8  = 0;
    let mut group_hi_vel: u8  = 127;
    let mut group_gain:   f32 = 1.0;

    let push_region = |regions: &mut Vec<SfzRegion>,
                       sample: &Option<PathBuf>,
                       lo_key: u8, hi_key: u8, pkc: u8,
                       lo_vel: u8, hi_vel: u8, gain: f32| {
        if let Some(s) = sample {
            regions.push(SfzRegion {
                sample: s.clone(),
                lo_key, hi_key, pitch_key_center: pkc,
                lo_vel, hi_vel, gain,
            });
        }
    };

    for raw_line in text.lines() {
        // Strip // comments.
        let line = if let Some(pos) = raw_line.find("//") { &raw_line[..pos] } else { raw_line };
        let line = line.trim();
        if line.is_empty() { continue; }

        // Handle section headers.
        let mut remaining = line;
        while !remaining.is_empty() {
            if let Some(rest) = remaining.strip_prefix("<group>") {
                // Save previous region if any.
                if in_region {
                    push_region(&mut regions, &sample, lo_key, hi_key, pkc, lo_vel, hi_vel, gain);
                }
                in_region = false;
                sample = None;
                // Reset group defaults.
                group_lo_key = 0; group_hi_key = 127; group_pkc = 60;
                group_lo_vel = 0; group_hi_vel = 127; group_gain = 1.0;
                remaining = rest.trim();
            } else if let Some(rest) = remaining.strip_prefix("<region>") {
                if in_region {
                    push_region(&mut regions, &sample, lo_key, hi_key, pkc, lo_vel, hi_vel, gain);
                }
                in_region = true;
                sample = None;
                // Inherit group defaults.
                lo_key = group_lo_key; hi_key = group_hi_key; pkc = group_pkc;
                lo_vel = group_lo_vel; hi_vel = group_hi_vel; gain = group_gain;
                remaining = rest.trim();
            } else {
                break;
            }
        }

        // Parse opcodes on this line.
        for token in remaining.split_whitespace() {
            if let Some((key, val)) = token.split_once('=') {
                let target_lo_key  = if in_region { &mut lo_key }  else { &mut group_lo_key };
                let target_hi_key  = if in_region { &mut hi_key }  else { &mut group_hi_key };
                let target_pkc     = if in_region { &mut pkc }     else { &mut group_pkc };
                let target_lo_vel  = if in_region { &mut lo_vel }  else { &mut group_lo_vel };
                let target_hi_vel  = if in_region { &mut hi_vel }  else { &mut group_hi_vel };
                let target_gain    = if in_region { &mut gain }    else { &mut group_gain };

                match key.to_lowercase().as_str() {
                    "sample" => {
                        let p = PathBuf::from(val.replace('\\', "/"));
                        sample = Some(if p.is_absolute() { p } else { base_dir.join(p) });
                    }
                    "lokey" | "lo_key" => {
                        *target_lo_key = parse_note_or_midi(val).unwrap_or(0);
                    }
                    "hikey" | "hi_key" => {
                        *target_hi_key = parse_note_or_midi(val).unwrap_or(127);
                    }
                    "key" => {
                        let n = parse_note_or_midi(val).unwrap_or(60);
                        *target_lo_key = n; *target_hi_key = n;
                    }
                    "pitch_keycenter" => {
                        *target_pkc = parse_note_or_midi(val).unwrap_or(60);
                    }
                    "lovel" => { *target_lo_vel = val.parse().unwrap_or(0); }
                    "hivel" => { *target_hi_vel = val.parse().unwrap_or(127); }
                    "volume" => {
                        let db: f32 = val.parse().unwrap_or(0.0);
                        *target_gain = 10.0_f32.powf(db / 20.0);
                    }
                    _ => {}
                }
            }
        }
    }

    // Save last region.
    if in_region {
        push_region(&mut regions, &sample, lo_key, hi_key, pkc, lo_vel, hi_vel, gain);
    }

    Ok(regions)
}

/// Parse a MIDI note from a string like "C4", "c#4", "60", "A-1".
fn parse_note_or_midi(s: &str) -> Option<u8> {
    // Try numeric first.
    if let Ok(n) = s.parse::<u8>() { return Some(n); }

    const NOTES: &[(&str, u8)] = &[
        ("c", 0), ("d", 2), ("e", 4), ("f", 5), ("g", 7), ("a", 9), ("b", 11),
    ];

    let s = s.to_lowercase();
    let mut chars = s.chars().peekable();
    let note_char = chars.next()?;
    let semitone = NOTES.iter().find(|(n, _)| *n == note_char.to_string().as_str())?.1;

    let mut sharp_flat: i8 = 0;
    if chars.peek() == Some(&'#') { sharp_flat = 1;  chars.next(); }
    if chars.peek() == Some(&'b') { sharp_flat = -1; chars.next(); }

    let octave_str: String = chars.collect();
    let octave: i8 = octave_str.parse().ok()?;

    let midi = (octave + 1) * 12 + semitone as i8 + sharp_flat;
    if (0..=127).contains(&midi) { Some(midi as u8) } else { None }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_note_names() {
        assert_eq!(parse_note_or_midi("C4"),  Some(60));
        assert_eq!(parse_note_or_midi("c4"),  Some(60));
        assert_eq!(parse_note_or_midi("A#3"), Some(58));
        assert_eq!(parse_note_or_midi("60"),  Some(60));
        assert_eq!(parse_note_or_midi("0"),   Some(0));
        assert_eq!(parse_note_or_midi("127"), Some(127));
    }

    #[test]
    fn parse_simple_sfz() {
        let sfz = "<region> sample=kick.wav lokey=36 hikey=36 pitch_keycenter=36\n\
                   <region> sample=snare.wav key=38";
        let regions = parse_text(sfz, Path::new(".")).unwrap();
        assert_eq!(regions.len(), 2);
        assert_eq!(regions[0].lo_key, 36);
        assert_eq!(regions[1].lo_key, 38);
        assert_eq!(regions[1].hi_key, 38);
    }

    #[test]
    fn group_defaults_inherited() {
        let sfz = "<group> lovel=64 hivel=127\n\
                   <region> sample=hard.wav lokey=36 hikey=36";
        let regions = parse_text(sfz, Path::new(".")).unwrap();
        assert_eq!(regions[0].lo_vel, 64);
        assert_eq!(regions[0].hi_vel, 127);
    }
}
