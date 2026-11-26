use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};

use libloading::Library;

const BASS_STREAM_PRESCAN: u32 = 0x20000;
const BASS_STREAM_DECODE: u32 = 0x200000;
#[cfg(windows)]
const BASS_UNICODE: u32 = 0x8000_0000;
const BASS_POS_BYTE: u32 = 0;
const BASS_ACTIVE_PLAYING: u32 = 1;
const BASS_FX_FREESOURCE: u32 = 0x10000;
const BASS_ATTRIB_TEMPO: u32 = 0x10000;

#[cfg(windows)]
const BASS_FX_BPM_FLAGS: u32 = 1; // BASS_FX_BPM_BKGRND (Windows only per bass_fx.h)
#[cfg(not(windows))]
const BASS_FX_BPM_FLAGS: u32 = 0;

type BassInitFn = unsafe extern "system" fn(i32, u32, u32, *mut c_void, *const c_void) -> i32;
type BassFreeFn = unsafe extern "system" fn() -> i32;
type BassGetVersionFn = unsafe extern "system" fn() -> u32;
type BassErrorGetCodeFn = unsafe extern "system" fn() -> i32;
type BassStreamCreateFileFn =
    unsafe extern "system" fn(u32, *const c_void, u64, u64, u32) -> u32;
type BassStreamFreeFn = unsafe extern "system" fn(u32) -> i32;
type BassChannelPlayFn = unsafe extern "system" fn(u32, i32) -> i32;
type BassChannelPauseFn = unsafe extern "system" fn(u32) -> i32;
type BassChannelStopFn = unsafe extern "system" fn(u32) -> i32;
type BassChannelIsActiveFn = unsafe extern "system" fn(u32) -> u32;
type BassChannelGetLengthFn = unsafe extern "system" fn(u32, u32) -> u64;
type BassChannelGetPositionFn = unsafe extern "system" fn(u32, u32) -> u64;
type BassChannelSetPositionFn = unsafe extern "system" fn(u32, u64, u32) -> i32;
type BassChannelBytes2SecondsFn = unsafe extern "system" fn(u32, u64) -> f64;
type BassChannelSeconds2BytesFn = unsafe extern "system" fn(u32, f64) -> u64;
type BassChannelSetAttributeFn = unsafe extern "system" fn(u32, u32, f32) -> i32;
type BassFxGetVersionFn = unsafe extern "system" fn() -> u32;
type BassFxTempoCreateFn = unsafe extern "system" fn(u32, u32) -> u32;
type BassFxBpmDecodeGetFn =
    unsafe extern "system" fn(u32, f64, f64, u32, u32, *mut c_void, *mut c_void) -> f32;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AudioInfo {
    pub duration_s: f64,
    pub bpm: f64,
}

pub struct BassLibs {
    _bass: Library,
    _bass_fx: Library,
    bass_init: BassInitFn,
    bass_free: BassFreeFn,
    bass_get_version: BassGetVersionFn,
    bass_error_get_code: BassErrorGetCodeFn,
    bass_stream_create_file: BassStreamCreateFileFn,
    bass_stream_free: BassStreamFreeFn,
    bass_channel_play: BassChannelPlayFn,
    bass_channel_pause: BassChannelPauseFn,
    bass_channel_stop: BassChannelStopFn,
    bass_channel_is_active: BassChannelIsActiveFn,
    bass_channel_get_length: BassChannelGetLengthFn,
    bass_channel_get_position: BassChannelGetPositionFn,
    bass_channel_set_position: BassChannelSetPositionFn,
    bass_channel_bytes2seconds: BassChannelBytes2SecondsFn,
    bass_channel_seconds2bytes: BassChannelSeconds2BytesFn,
    bass_channel_set_attribute: BassChannelSetAttributeFn,
    bass_fx_get_version: BassFxGetVersionFn,
    bass_fx_tempo_create: BassFxTempoCreateFn,
    bass_fx_bpm_decode_get: BassFxBpmDecodeGetFn,
}

impl BassLibs {
    pub fn load() -> Result<Self, String> {
        let bass_name = lib_filename("bass");
        let fx_name = lib_filename("bass_fx");

        let mut dirs: Vec<PathBuf> = Vec::new();
        let platform_dir = if cfg!(target_os = "windows") {
            "windows"
        } else if cfg!(target_os = "macos") {
            "macos"
        } else {
            "linux"
        };
        if let Ok(exe) = std::env::current_exe()
            && let Some(exe_dir) = exe.parent()
        {
            dirs.push(exe_dir.to_path_buf());
            dirs.push(exe_dir.join("lib"));
            dirs.push(exe_dir.join("../../../lib").join(platform_dir));
        }
        dirs.push(PathBuf::from("."));
        dirs.push(PathBuf::from("lib").join(platform_dir));
        dirs.push(PathBuf::from("../lib").join(platform_dir));

        for dir in &dirs {
            let bass_path = dir.join(&bass_name);
            let fx_path = dir.join(&fx_name);
            if bass_path.exists() && fx_path.exists() {
                return Self::load_pair(&bass_path, &fx_path);
            }
        }

        Self::load_pair(Path::new(&bass_name), Path::new(&fx_name))
    }

    pub fn load_from_dir(dir: &Path) -> Result<Self, String> {
        let bass_path = dir.join(lib_filename("bass"));
        let fx_path = dir.join(lib_filename("bass_fx"));
        if !bass_path.exists() || !fx_path.exists() {
            return Err(format!("BASS libraries not found in {}", dir.display()));
        }
        Self::load_pair(&bass_path, &fx_path)
    }

    fn load_pair(bass_path: &Path, fx_path: &Path) -> Result<Self, String> {
        unsafe {
            let bass = load_bass_library(bass_path)
                .map_err(|e| format!("failed to load {}: {e}", bass_path.display()))?;
            let bass_fx = Library::new(fx_path)
                .map_err(|e| format!("failed to load {}: {e}", fx_path.display()))?;

            Ok(Self {
                bass_init: *bass.get(b"BASS_Init\0").map_err(sym_err)?,
                bass_free: *bass.get(b"BASS_Free\0").map_err(sym_err)?,
                bass_get_version: *bass.get(b"BASS_GetVersion\0").map_err(sym_err)?,
                bass_error_get_code: *bass.get(b"BASS_ErrorGetCode\0").map_err(sym_err)?,
                bass_stream_create_file: *bass.get(b"BASS_StreamCreateFile\0").map_err(sym_err)?,
                bass_stream_free: *bass.get(b"BASS_StreamFree\0").map_err(sym_err)?,
                bass_channel_play: *bass.get(b"BASS_ChannelPlay\0").map_err(sym_err)?,
                bass_channel_pause: *bass.get(b"BASS_ChannelPause\0").map_err(sym_err)?,
                bass_channel_stop: *bass.get(b"BASS_ChannelStop\0").map_err(sym_err)?,
                bass_channel_is_active: *bass.get(b"BASS_ChannelIsActive\0").map_err(sym_err)?,
                bass_channel_get_length: *bass.get(b"BASS_ChannelGetLength\0").map_err(sym_err)?,
                bass_channel_get_position: *bass.get(b"BASS_ChannelGetPosition\0").map_err(sym_err)?,
                bass_channel_set_position: *bass.get(b"BASS_ChannelSetPosition\0").map_err(sym_err)?,
                bass_channel_bytes2seconds: *bass.get(b"BASS_ChannelBytes2Seconds\0").map_err(sym_err)?,
                bass_channel_seconds2bytes: *bass.get(b"BASS_ChannelSeconds2Bytes\0").map_err(sym_err)?,
                bass_channel_set_attribute: *bass.get(b"BASS_ChannelSetAttribute\0").map_err(sym_err)?,
                bass_fx_get_version: *bass_fx.get(b"BASS_FX_GetVersion\0").map_err(sym_err)?,
                bass_fx_tempo_create: *bass_fx.get(b"BASS_FX_TempoCreate\0").map_err(sym_err)?,
                bass_fx_bpm_decode_get: *bass_fx.get(b"BASS_FX_BPM_DecodeGet\0").map_err(sym_err)?,
                _bass: bass,
                _bass_fx: bass_fx,
            })
        }
    }

    pub fn versions(&self) -> (u32, u32) {
        unsafe { ((self.bass_get_version)(), (self.bass_fx_get_version)()) }
    }
}

pub struct AudioEngine {
    libs: BassLibs,
    stream: u32,
}

impl AudioEngine {
    pub fn new() -> Result<Self, String> {
        Self::from_libs(BassLibs::load()?)
    }

    pub fn from_libs(libs: BassLibs) -> Result<Self, String> {
        let ok = unsafe { (libs.bass_init)(-1, 44100, 0, null_mut(), null()) };
        if ok == 0 {
            let code = unsafe { (libs.bass_error_get_code)() };
            return Err(format!("BASS_Init failed (error {code})"));
        }
        Ok(Self { libs, stream: 0 })
    }

    pub fn analyze(&self, path: &Path) -> Result<AudioInfo, String> {
        let handle = self.open_decode_stream(path)?;
        let len_bytes = unsafe { (self.libs.bass_channel_get_length)(handle, BASS_POS_BYTE) };
        let duration_s = unsafe { (self.libs.bass_channel_bytes2seconds)(handle, len_bytes) }.max(0.0);

        let mut raw_bpm = unsafe {
            (self.libs.bass_fx_bpm_decode_get)(
                handle,
                0.0,
                duration_s,
                0,
                BASS_FX_BPM_FLAGS,
                null_mut(),
                null_mut(),
            )
        };
        if raw_bpm <= 0.0 && duration_s > 30.0 {
            raw_bpm = unsafe {
                (self.libs.bass_fx_bpm_decode_get)(
                    handle,
                    30.0,
                    60.0_f64.min(duration_s),
                    0,
                    BASS_FX_BPM_FLAGS,
                    null_mut(),
                    null_mut(),
                )
            };
        }
        unsafe {
            (self.libs.bass_stream_free)(handle);
        }

        Ok(AudioInfo {
            duration_s,
            bpm: normalize_bpm(raw_bpm),
        })
    }

    pub fn open(&mut self, path: &Path) -> Result<f64, String> {
        self.stop();
        let raw = self.open_decode_stream(path)?;
        let tempo = unsafe { (self.libs.bass_fx_tempo_create)(raw, BASS_FX_FREESOURCE) };
        if tempo == 0 {
            let code = unsafe { (self.libs.bass_error_get_code)() };
            unsafe {
                (self.libs.bass_stream_free)(raw);
            }
            return Err(format!("BASS_FX_TempoCreate failed (error {code})"));
        }
        self.stream = tempo;
        let len_bytes = unsafe { (self.libs.bass_channel_get_length)(tempo, BASS_POS_BYTE) };
        Ok(unsafe { (self.libs.bass_channel_bytes2seconds)(tempo, len_bytes) }.max(0.0))
    }

    pub fn play(&self) -> Result<(), String> {
        self.play_impl(1)
    }

    pub fn resume(&self) -> Result<(), String> {
        self.play_impl(0)
    }

    fn play_impl(&self, restart: i32) -> Result<(), String> {
        if self.stream == 0 {
            return Err("no audio stream open".into());
        }
        let ok = unsafe { (self.libs.bass_channel_play)(self.stream, restart) };
        if ok == 0 {
            let code = unsafe { (self.libs.bass_error_get_code)() };
            return Err(format!("BASS_ChannelPlay failed (error {code})"));
        }
        Ok(())
    }

    pub fn pause(&self) {
        if self.stream != 0 {
            unsafe {
                (self.libs.bass_channel_pause)(self.stream);
            }
        }
    }

    pub fn stop(&mut self) {
        if self.stream != 0 {
            unsafe {
                (self.libs.bass_channel_stop)(self.stream);
                (self.libs.bass_stream_free)(self.stream);
            }
            self.stream = 0;
        }
    }

    pub fn set_tempo(&self, percent_offset: f32) -> Result<(), String> {
        if self.stream == 0 {
            return Err("no audio stream open".into());
        }
        let clamped = percent_offset.clamp(-50.0, 50.0);
        let ok = unsafe { (self.libs.bass_channel_set_attribute)(self.stream, BASS_ATTRIB_TEMPO, clamped) };
        if ok == 0 {
            let code = unsafe { (self.libs.bass_error_get_code)() };
            return Err(format!("BASS_ChannelSetAttribute(TEMPO) failed (error {code})"));
        }
        Ok(())
    }

    pub fn position_ms(&self) -> f64 {
        if self.stream == 0 {
            return 0.0;
        }
        unsafe {
            let bytes = (self.libs.bass_channel_get_position)(self.stream, BASS_POS_BYTE);
            ((self.libs.bass_channel_bytes2seconds)(self.stream, bytes) * 1000.0).max(0.0)
        }
    }

    pub fn seek_ms(&self, ms: f64) {
        if self.stream == 0 {
            return;
        }
        let seconds = (ms / 1000.0).max(0.0);
        unsafe {
            let bytes = (self.libs.bass_channel_seconds2bytes)(self.stream, seconds);
            (self.libs.bass_channel_set_position)(self.stream, bytes, BASS_POS_BYTE);
        }
    }

    pub fn is_playing(&self) -> bool {
        if self.stream == 0 {
            return false;
        }
        unsafe { (self.libs.bass_channel_is_active)(self.stream) == BASS_ACTIVE_PLAYING }
    }

    pub fn versions(&self) -> (u32, u32) {
        self.libs.versions()
    }

    fn open_decode_stream(&self, path: &Path) -> Result<u32, String> {
        let handle = open_stream_platform(self.libs.bass_stream_create_file, path)?;
        if handle == 0 {
            let code = unsafe { (self.libs.bass_error_get_code)() };
            return Err(format!(
                "BASS_StreamCreateFile failed for {} (error {code})",
                path.display()
            ));
        }
        Ok(handle)
    }
}

impl Drop for AudioEngine {
    fn drop(&mut self) {
        self.stop();
        unsafe {
            (self.libs.bass_free)();
        }
    }
}

pub fn normalize_bpm(raw: f32) -> f64 {
    if !raw.is_finite() || raw <= 0.0 {
        return 400.0;
    }
    let mut bpm = (f64::from(raw) * 2.0).round() / 2.0;
    if bpm <= 0.0 {
        return 400.0;
    }
    while bpm <= 200.0 {
        bpm *= 2.0;
    }
    bpm
}

fn sym_err(e: libloading::Error) -> String {
    format!("missing BASS symbol: {e}")
}

fn lib_filename(base: &str) -> String {
    #[cfg(windows)]
    {
        format!("{base}.dll")
    }
    #[cfg(target_os = "macos")]
    {
        format!("lib{base}.dylib")
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        format!("lib{base}.so")
    }
}

#[cfg(unix)]
unsafe fn load_bass_library(path: &Path) -> Result<Library, libloading::Error> {
    use libloading::os::unix::{Library as UnixLibrary, RTLD_GLOBAL, RTLD_NOW};
    let unix_lib = unsafe { UnixLibrary::open(Some(path), RTLD_NOW | RTLD_GLOBAL)? };
    Ok(unix_lib.into())
}

#[cfg(not(unix))]
unsafe fn load_bass_library(path: &Path) -> Result<Library, libloading::Error> {
    unsafe { Library::new(path) }
}

#[cfg(windows)]
fn open_stream_platform(create_file: BassStreamCreateFileFn, path: &Path) -> Result<u32, String> {
    use std::os::windows::ffi::OsStrExt;
    let s = path.to_string_lossy();
    let stripped = s.strip_prefix(r"\\?\").unwrap_or(&s);
    let wide: Vec<u16> = std::ffi::OsStr::new(stripped)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    Ok(unsafe {
        create_file(
            0,
            wide.as_ptr() as *const c_void,
            0,
            0,
            BASS_STREAM_DECODE | BASS_STREAM_PRESCAN | BASS_UNICODE,
        )
    })
}

#[cfg(not(windows))]
fn open_stream_platform(create_file: BassStreamCreateFileFn, path: &Path) -> Result<u32, String> {
    use std::ffi::CString;
    let c = CString::new(path.as_os_str().as_encoded_bytes())
        .map_err(|_| format!("audio path contains interior NUL: {}", path.display()))?;
    Ok(unsafe {
        create_file(
            0,
            c.as_ptr() as *const c_void,
            0,
            0,
            BASS_STREAM_DECODE | BASS_STREAM_PRESCAN,
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_bpm_handles_edge_cases_and_doubling() {
        assert_eq!(normalize_bpm(0.0), 400.0);
        assert_eq!(normalize_bpm(-10.0), 400.0);
        assert_eq!(normalize_bpm(f32::NAN), 400.0);
        assert_eq!(normalize_bpm(f32::INFINITY), 400.0);
        assert_eq!(normalize_bpm(100.24), 400.0);
        assert_eq!(normalize_bpm(100.26), 201.0);
        assert_eq!(normalize_bpm(125.3), 251.0);
        assert_eq!(normalize_bpm(250.4), 250.5);
        assert_eq!(normalize_bpm(300.0), 300.0);
    }

    #[test]
    fn load_missing_libraries_returns_err() {
        assert!(BassLibs::load_from_dir(Path::new("nonexistent_dir_xyz")).is_err());
    }

    #[test]
    #[ignore]
    fn local_bass_smoke_test() {
        let libs = BassLibs::load_from_dir(Path::new("../lib/windows")).expect("load local BASS");
        let (bass_ver, fx_ver) = libs.versions();
        assert_ne!(bass_ver, 0);
        assert_ne!(fx_ver, 0);

        let engine = AudioEngine::from_libs(libs).expect("BASS_Init");
        let sample = Path::new("../.tmp/taglib/tests/data/lame_vbr.mp3");
        if sample.exists() {
            let info = engine.analyze(sample).expect("analyze sample");
            assert!(info.bpm > 200.0);
        }
    }
}
