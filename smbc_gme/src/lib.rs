//! Minimal safe wrapper over libgme for SMBC.
use std::ffi::{CStr, c_char, c_int, c_long, c_void};

#[repr(C)]
pub struct MusicEmu {
    _private: [u8; 0],
}
type GmeType = *const c_void;

unsafe extern "C" {
    static gme_gbs_type: GmeType;
    static gme_kss_type: GmeType;
    static gme_nsf_type: GmeType;
    static gme_nsfe_type: GmeType;
    static gme_sap_type: GmeType;
    static gme_spc_type: GmeType;
    fn gme_new_emu(t: GmeType, sample_rate: c_int) -> *mut MusicEmu;
    fn gme_delete(e: *mut MusicEmu);
    fn gme_load_data(e: *mut MusicEmu, data: *const c_void, size: c_long) -> *const c_char;
    fn gme_start_track(e: *mut MusicEmu, index: c_int) -> *const c_char;
    fn gme_play(e: *mut MusicEmu, count: c_int, out: *mut i16) -> *const c_char;
    fn gme_track_ended(e: *const MusicEmu) -> c_int;
    fn gme_tell(e: *const MusicEmu) -> c_int;
    fn gme_seek(e: *mut MusicEmu, msec: c_int) -> *const c_char;
    fn gme_set_fade(e: *mut MusicEmu, start_msec: c_int);
    fn gme_set_tempo(e: *mut MusicEmu, tempo: f64);
    fn gme_ignore_silence(e: *mut MusicEmu, ignore: c_int);
    fn gme_set_stereo_depth(e: *mut MusicEmu, depth: f64);
    fn gme_track_count(e: *const MusicEmu) -> c_int;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EmuType {
    Gbs,
    Kss,
    Nsf,
    Nsfe,
    Sap,
    Spc,
}

fn err(p: *const c_char) -> Result<(), String> {
    if p.is_null() {
        Ok(())
    } else {
        Err(unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned())
    }
}

pub struct Emu(*mut MusicEmu);

// libgme emulators are plain heap objects with no thread affinity.
unsafe impl Send for Emu {}

impl Emu {
    pub fn new(t: EmuType, sample_rate: i32) -> Option<Self> {
        let ty = unsafe {
            match t {
                EmuType::Gbs => gme_gbs_type,
                EmuType::Kss => gme_kss_type,
                EmuType::Nsf => gme_nsf_type,
                EmuType::Nsfe => gme_nsfe_type,
                EmuType::Sap => gme_sap_type,
                EmuType::Spc => gme_spc_type,
            }
        };
        let p = unsafe { gme_new_emu(ty, sample_rate) };
        (!p.is_null()).then_some(Self(p))
    }
    pub fn load_data(&mut self, data: &[u8]) -> Result<(), String> {
        err(unsafe { gme_load_data(self.0, data.as_ptr().cast(), data.len() as c_long) })
    }
    pub fn start_track(&mut self, index: i32) -> Result<(), String> {
        err(unsafe { gme_start_track(self.0, index) })
    }
    /// `out.len()` must be even (stereo samples).
    pub fn play(&mut self, out: &mut [i16]) -> Result<(), String> {
        err(unsafe { gme_play(self.0, out.len() as c_int, out.as_mut_ptr()) })
    }
    pub fn track_ended(&self) -> bool {
        unsafe { gme_track_ended(self.0) != 0 }
    }
    pub fn tell(&self) -> i32 {
        unsafe { gme_tell(self.0) }
    }
    pub fn seek(&mut self, msec: i32) -> Result<(), String> {
        err(unsafe { gme_seek(self.0, msec) })
    }
    pub fn set_fade(&mut self, msec: i32) {
        unsafe { gme_set_fade(self.0, msec) }
    }
    pub fn ignore_silence(&mut self, ignore: bool) {
        unsafe { gme_ignore_silence(self.0, ignore as c_int) }
    }
    pub fn set_tempo(&mut self, t: f64) {
        unsafe { gme_set_tempo(self.0, t) }
    }
    pub fn set_stereo_depth(&mut self, d: f64) {
        unsafe { gme_set_stereo_depth(self.0, d) }
    }
    pub fn track_count(&self) -> i32 {
        unsafe { gme_track_count(self.0) }
    }
}

impl Drop for Emu {
    fn drop(&mut self) {
        unsafe { gme_delete(self.0) }
    }
}
