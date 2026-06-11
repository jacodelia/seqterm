//! FX parameter addressing for the realtime automation / modulation driver.
//!
//! Pattern-FX (per mixer slot) and mixer-FX (master bus) parameters are given
//! stable string ids so the universal modulation matrix and automation engine
//! (which key everything by destination id) can target them. The driver
//! resolves these ids back to `(chain, entry, param)` to build the effective FX
//! chain each control block.
//!
//! Id scheme:
//! - pattern / slot FX: `pfx:<slot_id>:<entry>:<param>`
//! - mixer / master FX: `mfx:<entry>:<param>`

use crate::app::{AudioFxEntry, AudioFxKind, build_fx_chain};

/// Whether a kind's realtime `FxProcessor::set_param` exists AND maps a
/// normalised value the same way `build_processor` does. For these kinds the
/// modulation driver can update parameters in place (preserving DSP tails);
/// for all others it must rebuild the chain (which resets state, but those
/// kinds have no audible tail to lose).
///
/// Keep this in lockstep with the `set_param` impls in `seqterm-audio-engine`'s
/// fx modules.
pub fn kind_supports_live_param(kind: AudioFxKind) -> bool {
    matches!(
        kind,
        AudioFxKind::Delay
            | AudioFxKind::Reverb
            | AudioFxKind::Compressor
            | AudioFxKind::Gate
            | AudioFxKind::Chorus
            | AudioFxKind::Flanger
            | AudioFxKind::Phaser
    )
}

/// A resolved FX parameter destination.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FxDest {
    /// A parameter on a per-pattern mixer slot's insert chain.
    Slot { slot_id: u32, entry: usize, param: usize },
    /// A parameter on the master-bus FX chain.
    Master { entry: usize, param: usize },
}

impl FxDest {
    /// The stable destination id string.
    pub fn id(&self) -> String {
        match self {
            FxDest::Slot { slot_id, entry, param } => format!("pfx:{slot_id}:{entry}:{param}"),
            FxDest::Master { entry, param } => format!("mfx:{entry}:{param}"),
        }
    }

    /// Parse a destination id back into an `FxDest`. Returns `None` for ids that
    /// are not FX destinations (e.g. instrument parameters).
    pub fn parse(id: &str) -> Option<Self> {
        let mut it = id.split(':');
        match it.next()? {
            "pfx" => {
                let slot_id = it.next()?.parse().ok()?;
                let entry = it.next()?.parse().ok()?;
                let param = it.next()?.parse().ok()?;
                Some(FxDest::Slot { slot_id, entry, param })
            }
            "mfx" => {
                let entry = it.next()?.parse().ok()?;
                let param = it.next()?.parse().ok()?;
                Some(FxDest::Master { entry, param })
            }
            _ => None,
        }
    }

    /// A human-readable label for the editor (e.g. "P12 REVERB·Wet").
    pub fn label(&self, kind_label: &str, param_label: &str) -> String {
        match self {
            FxDest::Slot { slot_id, .. } => format!("S{slot_id} {kind_label}·{param_label}"),
            FxDest::Master { .. } => format!("M {kind_label}·{param_label}"),
        }
    }
}

/// Build an FX chain from `base` entries with per-`(entry, param)` value
/// overrides applied on top (used to inject automation + modulation without
/// mutating the user's stored base values).
pub fn build_effective_chain(
    base: &[AudioFxEntry],
    overrides: &[(usize, usize, f32)],
) -> Vec<Box<dyn seqterm_audio_engine::FxProcessor>> {
    if overrides.is_empty() {
        return build_fx_chain(base);
    }
    let mut entries: Vec<AudioFxEntry> = base.to_vec();
    for &(entry, param, value) in overrides {
        if let Some(e) = entries.get_mut(entry) {
            if let Some(p) = e.params.get_mut(param) {
                *p = value.clamp(0.0, 1.0);
            }
            e.sync_wet();
        }
    }
    build_fx_chain(&entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn id_roundtrips() {
        let s = FxDest::Slot { slot_id: 7, entry: 2, param: 3 };
        assert_eq!(FxDest::parse(&s.id()), Some(s));
        let m = FxDest::Master { entry: 1, param: 4 };
        assert_eq!(FxDest::parse(&m.id()), Some(m));
        assert_eq!(FxDest::parse("inst:0:cutoff"), None);
        assert_eq!(FxDest::parse("pfx:bad"), None);
    }
}
