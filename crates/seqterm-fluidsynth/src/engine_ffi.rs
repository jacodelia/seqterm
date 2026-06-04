//! System libfluidsynth engine — full FluidSynth 2.x via dynamic linking.
//!
//! Selected by the `fluidsynth` feature when `fluidlite` is *not* enabled. Needs
//! a linkable libfluidsynth (≥ 2.0); see `build.rs` for cross-platform linking.

use std::ffi::CString;
use std::path::PathBuf;

use seqterm_ports::realtime::PresetInfo;

use crate::ffi;

/// This engine produces real sound.
pub const REAL: bool = true;

pub struct Engine {
    settings: *mut ffi::fluid_settings_t,
    synth: *mut ffi::fluid_synth_t,
    sfont_id: i32,
    #[allow(dead_code)]
    sf2_path: PathBuf,
}

// libfluidsynth's synth is internally synchronized; the handle is safe to move
// to the audio thread, where it is only touched from one thread at a time.
unsafe impl Send for Engine {}

impl Engine {
    pub fn new(
        sf2_path: PathBuf,
        channels: &[(u8, u8, u8)],
        sample_rate: u32,
    ) -> anyhow::Result<Self> {
        unsafe {
            let settings = ffi::new_fluid_settings();
            if settings.is_null() {
                return Err(anyhow::anyhow!("new_fluid_settings failed"));
            }
            set_str(settings, "synth.reverb.active", "yes");
            set_str(settings, "synth.chorus.active", "yes");
            set_num(settings, "synth.sample-rate", sample_rate as f64);
            set_int(settings, "synth.polyphony", 512);
            set_int(settings, "synth.audio-channels", 1);

            let synth = ffi::new_fluid_synth(settings);
            if synth.is_null() {
                ffi::delete_fluid_settings(settings);
                return Err(anyhow::anyhow!("new_fluid_synth failed"));
            }
            ffi::fluid_synth_set_gain(synth, 1.0);

            let c_path = CString::new(sf2_path.to_string_lossy().as_bytes())
                .map_err(|_| anyhow::anyhow!("SF2 path contains NUL byte"))?;
            let sfont_id = ffi::fluid_synth_sfload(synth, c_path.as_ptr(), 1);
            if sfont_id == ffi::FLUID_FAILED {
                ffi::delete_fluid_synth(synth);
                ffi::delete_fluid_settings(settings);
                return Err(anyhow::anyhow!("fluid_synth_sfload failed: {}", sf2_path.display()));
            }

            for &(ch, bank, preset) in channels {
                let rc = ffi::fluid_synth_program_select(synth, ch as i32, sfont_id, bank as i32, preset as i32);
                if rc == ffi::FLUID_FAILED {
                    tracing::warn!(
                        "FluidSynth program_select ch={ch} bank={bank} preset={preset} failed; falling back to 0/0"
                    );
                    let _ = ffi::fluid_synth_program_select(synth, ch as i32, sfont_id, 0, 0);
                }
            }

            for ch in 0..16i32 {
                ffi::fluid_synth_cc(synth, ch, 91, 40);
                ffi::fluid_synth_cc(synth, ch, 93, 0);
                ffi::fluid_synth_cc(synth, ch, 7, 100);
                ffi::fluid_synth_cc(synth, ch, 10, 64);
            }

            Ok(Self { settings, synth, sfont_id, sf2_path })
        }
    }

    pub fn render_into(&mut self, l: &mut [f32], r: &mut [f32]) {
        let frames = l.len().min(r.len());
        if frames == 0 { return; }
        unsafe {
            ffi::fluid_synth_write_float(
                self.synth,
                frames as i32,
                l.as_mut_ptr() as *mut std::os::raw::c_void, 0, 1,
                r.as_mut_ptr() as *mut std::os::raw::c_void, 0, 1,
            );
        }
    }

    pub fn note_on(&mut self, channel: u8, note: u8, velocity: u8) {
        unsafe { ffi::fluid_synth_noteon(self.synth, channel as i32, note as i32, velocity as i32); }
    }

    pub fn note_off(&mut self, channel: u8, note: u8) {
        unsafe { ffi::fluid_synth_noteoff(self.synth, channel as i32, note as i32); }
    }

    pub fn control_change(&mut self, channel: u8, cc: u8, value: u8) {
        unsafe { ffi::fluid_synth_cc(self.synth, channel as i32, cc as i32, value as i32); }
    }

    pub fn pitch_bend(&mut self, channel: u8, value: i16) {
        let v = (value as i32 + 8192).clamp(0, 16383);
        unsafe { ffi::fluid_synth_pitch_bend(self.synth, channel as i32, v); }
    }

    pub fn all_notes_off(&mut self) {
        for ch in 0..16i32 {
            unsafe { ffi::fluid_synth_all_notes_off(self.synth, ch); }
        }
    }

    pub fn select_preset(&mut self, bank: u16, program: u8) {
        unsafe {
            ffi::fluid_synth_bank_select(self.synth, 0, bank as u32);
            ffi::fluid_synth_program_change(self.synth, 0, program as i32);
        }
    }

    pub fn list_presets(&self) -> Vec<PresetInfo> {
        unsafe {
            let sfont = ffi::fluid_synth_get_sfont_by_id(self.synth, self.sfont_id);
            if sfont.is_null() { return Vec::new(); }
            let mut out = Vec::new();
            ffi::fluid_sfont_iteration_start(sfont);
            loop {
                let preset = ffi::fluid_sfont_iteration_next(sfont);
                if preset.is_null() { break; }
                let name_ptr = ffi::fluid_preset_get_name(preset);
                let name = if name_ptr.is_null() {
                    String::new()
                } else {
                    std::ffi::CStr::from_ptr(name_ptr).to_string_lossy().into_owned()
                };
                out.push(PresetInfo {
                    bank: ffi::fluid_preset_get_banknum(preset) as u16,
                    program: ffi::fluid_preset_get_num(preset) as u8,
                    name,
                });
            }
            out
        }
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        unsafe {
            if !self.synth.is_null() { ffi::delete_fluid_synth(self.synth); }
            if !self.settings.is_null() { ffi::delete_fluid_settings(self.settings); }
        }
    }
}

unsafe fn set_str(s: *mut ffi::fluid_settings_t, name: &str, val: &str) {
    if let (Ok(n), Ok(v)) = (CString::new(name), CString::new(val)) {
        ffi::fluid_settings_setstr(s, n.as_ptr(), v.as_ptr());
    }
}

unsafe fn set_num(s: *mut ffi::fluid_settings_t, name: &str, val: f64) {
    if let Ok(n) = CString::new(name) {
        ffi::fluid_settings_setnum(s, n.as_ptr(), val);
    }
}

unsafe fn set_int(s: *mut ffi::fluid_settings_t, name: &str, val: i32) {
    if let Ok(n) = CString::new(name) {
        ffi::fluid_settings_setint(s, n.as_ptr(), val);
    }
}
