//! The mod's settings, in `CameraUnlock.ini` beside the game's executable.
//!
//! cameraunlock-core's config owner (C++, through src/config_owner.cpp) is the one reader
//! and writer of that file. While it is absent the owner imports `bioshock_headtrack.ini`,
//! the file every earlier build read, through the frozen reader in src/legacy_config, and
//! it never writes that file. Rows set to `default` take their values from the player's
//! Defaults.ini, which the owner reads and never changes.

use std::ffi::{c_char, c_void};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

use once_cell::sync::OnceCell;

/// What the frozen reader gave, for the C++ import.
#[repr(C)]
struct LegacyConfig {
    outcome: i32,
    world_space_yaw: u8,
    yaw_mode_key: i32,
    local_smoothing: f64,
    remote_smoothing: f64,
}

const LEGACY_READ: i32 = 0;
const LEGACY_NO_FILE: i32 = 1;
const LEGACY_UNREAD: i32 = 2;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct RawSettings {
    udp_port: u16,
    enable_on_startup: u8,
    world_space_yaw: u8,
    rotation_enabled: u8,
    position_enabled: u8,
    local_smoothing: f64,
    remote_smoothing: f64,
    collision_enabled: u8,
    collision_release_smoothing: f32,
}

/// The settings the session starts on. The hotkey lists stay with the owner, which
/// registers them (see [`Owner::start_hotkeys`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    pub udp_port: u16,
    pub enable_on_startup: bool,
    pub world_space_yaw: bool,
    pub rotation_enabled: bool,
    pub position_enabled: bool,
    pub local_smoothing: f64,
    pub remote_smoothing: f64,
    pub collision_enabled: bool,
    pub collision_release_smoothing: f32,
}

impl From<RawSettings> for Settings {
    fn from(raw: RawSettings) -> Self {
        Self {
            udp_port: raw.udp_port,
            enable_on_startup: raw.enable_on_startup != 0,
            world_space_yaw: raw.world_space_yaw != 0,
            rotation_enabled: raw.rotation_enabled != 0,
            position_enabled: raw.position_enabled != 0,
            local_smoothing: raw.local_smoothing,
            remote_smoothing: raw.remote_smoothing,
            collision_enabled: raw.collision_enabled != 0,
            collision_release_smoothing: raw.collision_release_smoothing,
        }
    }
}

#[repr(C)]
pub struct RawOwner {
    _private: [u8; 0],
}

type TextSink = unsafe extern "C" fn(*mut c_void, i32, *const c_char, usize);
type LegacyReader = unsafe extern "C" fn(*const u16, usize, *mut LegacyConfig);
type Action = extern "C" fn();

extern "C" {
    fn bsr_config_owner_new(
        folder: *const u16,
        len: usize,
        reader: LegacyReader,
        text: TextSink,
        context: *mut c_void,
    ) -> *mut RawOwner;
    fn bsr_config_owner_free(owner: *mut RawOwner);
    fn bsr_config_load(
        owner: *mut RawOwner,
        out: *mut RawSettings,
        text: TextSink,
        context: *mut c_void,
    ) -> i32;
    fn bsr_config_save_tracking_mode(
        owner: *mut RawOwner,
        rotation_enabled: u8,
        position_enabled: u8,
        text: TextSink,
        context: *mut c_void,
    ) -> i32;
    fn bsr_config_save_world_space_yaw(
        owner: *mut RawOwner,
        world_space_yaw: u8,
        text: TextSink,
        context: *mut c_void,
    ) -> i32;
    fn bsr_hotkeys_start(
        owner: *mut RawOwner,
        toggle: Action,
        cycle_mode: Action,
        yaw_mode: Action,
        text: TextSink,
        context: *mut c_void,
    ) -> i32;
}

#[cfg(feature = "test-support")]
extern "C" {
    fn bsr_test_config_owner_new_at(
        folder: *const u16,
        folder_len: usize,
        defaults: *const u16,
        defaults_len: usize,
        reader: LegacyReader,
        text: TextSink,
        context: *mut c_void,
    ) -> *mut RawOwner;
}

/// A line from the owner for the mod's log.
#[derive(Clone, Debug, PartialEq)]
pub enum Line {
    Info(String),
    Warning(String),
}

#[derive(Default)]
struct Lines {
    lines: Vec<Line>,
    exception: Option<String>,
}

unsafe extern "C" fn collect(context: *mut c_void, level: i32, text: *const c_char, len: usize) {
    let lines = &mut *context.cast::<Lines>();
    let text = String::from_utf8_lossy(std::slice::from_raw_parts(text.cast(), len)).into_owned();
    match level {
        0 => lines.lines.push(Line::Info(text)),
        1 => lines.lines.push(Line::Warning(text)),
        _ => lines.exception = Some(text),
    }
}

/// Runs one call into the owner with a fresh line sink. A negative return is an exception
/// out of core, which is a broken contract, so it stops the mod with core's own words.
fn call<T>(operation: &str, f: impl FnOnce(TextSink, *mut c_void) -> T) -> (T, Vec<Line>) {
    let mut lines = Lines::default();
    let value = f(collect, (&mut lines as *mut Lines).cast());
    if let Some(exception) = lines.exception {
        panic!("{operation}: {exception}");
    }
    (value, lines.lines)
}

unsafe extern "C" fn read_legacy(path: *const u16, len: usize, out: *mut LegacyConfig) {
    let path = PathBuf::from(std::ffi::OsString::from_wide(std::slice::from_raw_parts(
        path, len,
    )));
    let (outcome, config) = match crate::legacy_config::read(&path) {
        crate::legacy_config::Outcome::Read(config) => (LEGACY_READ, config),
        crate::legacy_config::Outcome::Unread(e) if e.kind() == std::io::ErrorKind::NotFound => {
            (LEGACY_NO_FILE, crate::legacy_config::Config::default())
        }
        crate::legacy_config::Outcome::Unread(_) => {
            (LEGACY_UNREAD, crate::legacy_config::Config::default())
        }
    };
    *out = LegacyConfig {
        outcome,
        world_space_yaw: u8::from(config.world_space_yaw),
        yaw_mode_key: config.yaw_mode_key,
        local_smoothing: config.local_smoothing,
        remote_smoothing: config.remote_smoothing,
    };
}

/// The folder as the owner takes it: UTF-16, with its trailing separator.
fn wide_folder(folder: &Path) -> Vec<u16> {
    let mut wide: Vec<u16> = folder.as_os_str().encode_wide().collect();
    if wide.last() != Some(&u16::from(b'\\')) {
        wide.push(u16::from(b'\\'));
    }
    wide
}

/// What `Owner::save_*` reported, as core's ConfigSaveStatus.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SaveStatus {
    Saved,
    NotSaved,
    Uncertain,
}

fn save_status(status: i32) -> SaveStatus {
    match status {
        0 => SaveStatus::Saved,
        1 => SaveStatus::NotSaved,
        2 => SaveStatus::Uncertain,
        other => unreachable!("ConfigSaveStatus {other}"),
    }
}

/// One `CameraUnlock.ini` and its owner.
pub struct Owner(*mut RawOwner);

// The C++ owner serialises Load, Save and Reload behind its own mutex.
unsafe impl Send for Owner {}
unsafe impl Sync for Owner {}

impl Owner {
    /// The mod's owner for the files in `folder`, over the player's own Defaults.ini.
    pub fn per_user(folder: &Path) -> Self {
        let wide = wide_folder(folder);
        let (raw, _) = call("creating the config owner", |text, context| unsafe {
            bsr_config_owner_new(wide.as_ptr(), wide.len(), read_legacy, text, context)
        });
        Self(raw)
    }

    /// An owner for a test, over the Defaults.ini at `defaults`.
    #[cfg(feature = "test-support")]
    pub fn at(folder: &Path, defaults: &Path) -> Self {
        let wide = wide_folder(folder);
        let defaults: Vec<u16> = defaults.as_os_str().encode_wide().collect();
        let (raw, _) = call("creating the config owner", |text, context| unsafe {
            bsr_test_config_owner_new_at(
                wide.as_ptr(),
                wide.len(),
                defaults.as_ptr(),
                defaults.len(),
                read_legacy,
                text,
                context,
            )
        });
        Self(raw)
    }

    #[cfg(feature = "test-support")]
    pub fn raw(&self) -> *mut RawOwner {
        self.0
    }

    /// Reads Defaults.ini and `CameraUnlock.ini`, importing or creating the latter where it
    /// is absent. Returns core's ConfigLoadStatus number, the settings and the log lines.
    pub fn load(&self) -> (i32, Settings, Vec<Line>) {
        let mut raw = RawSettings::default();
        let (status, lines) = call("loading the config", |text, context| unsafe {
            bsr_config_load(self.0, &mut raw, text, context)
        });
        (status, raw.into(), lines)
    }

    pub fn save_tracking_mode(
        &self,
        rotation_enabled: bool,
        position_enabled: bool,
    ) -> (SaveStatus, Vec<Line>) {
        let (status, lines) = call("saving the tracking mode", |text, context| unsafe {
            bsr_config_save_tracking_mode(
                self.0,
                u8::from(rotation_enabled),
                u8::from(position_enabled),
                text,
                context,
            )
        });
        (save_status(status), lines)
    }

    pub fn save_world_space_yaw(&self, world_space_yaw: bool) -> (SaveStatus, Vec<Line>) {
        let (status, lines) = call("saving the yaw mode", |text, context| unsafe {
            bsr_config_save_world_space_yaw(self.0, u8::from(world_space_yaw), text, context)
        });
        (save_status(status), lines)
    }

    /// Registers ToggleKey, CycleTrackingModeKey and YawModeKey as the last load read them
    /// on core's hotkey poller and starts it. Once per process.
    pub fn start_hotkeys(&self, toggle: Action, cycle_mode: Action, yaw_mode: Action) -> Vec<Line> {
        let (_, lines) = call("starting the hotkeys", |text, context| unsafe {
            bsr_hotkeys_start(self.0, toggle, cycle_mode, yaw_mode, text, context)
        });
        lines
    }
}

impl Drop for Owner {
    fn drop(&mut self) {
        unsafe { bsr_config_owner_free(self.0) }
    }
}

fn write_log(lines: &[Line]) {
    for line in lines {
        match line {
            Line::Info(text) => log::info!("{text}"),
            Line::Warning(text) => log::warn!("{text}"),
        }
    }
}

static OWNER: OnceCell<Owner> = OnceCell::new();

/// User-configured smoothing, stored as f64 bits. Both cover rotation
/// and position; the receiver's connection locality picks which one the
/// pipeline uses per frame.
static LOCAL_SMOOTHING_BITS: AtomicU64 =
    AtomicU64::new(crate::smoothing::DEFAULT_LOCAL_SMOOTHING.to_bits());
static REMOTE_SMOOTHING_BITS: AtomicU64 =
    AtomicU64::new(crate::smoothing::DEFAULT_REMOTE_SMOOTHING.to_bits());

/// Whether a lean stops at walls, and how gently it reopens once they clear, as loaded.
static COLLISION_ENABLED: AtomicBool = AtomicBool::new(true);
static COLLISION_RELEASE_SMOOTHING_BITS: AtomicU32 = AtomicU32::new(0.9_f32.to_bits());

pub fn collision_enabled() -> bool {
    COLLISION_ENABLED.load(Ordering::Acquire)
}

pub fn collision_release_smoothing() -> f32 {
    f32::from_bits(COLLISION_RELEASE_SMOOTHING_BITS.load(Ordering::Acquire))
}

/// Smoothing applied when the tracker runs on this machine (loopback).
pub fn local_smoothing() -> f64 {
    f64::from_bits(LOCAL_SMOOTHING_BITS.load(Ordering::Acquire))
}

/// Smoothing applied when the tracker is a remote device on the network.
pub fn remote_smoothing() -> f64 {
    f64::from_bits(REMOTE_SMOOTHING_BITS.load(Ordering::Acquire))
}

/// Loads `CameraUnlock.ini` from `folder`, the folder of the game's executable, which is
/// where the game loads this DLL from. Once, on the init thread.
pub fn load(folder: &Path) -> Settings {
    let owner = OWNER.get_or_init(|| Owner::per_user(folder));
    let (_, settings, lines) = owner.load();
    write_log(&lines);
    LOCAL_SMOOTHING_BITS.store(settings.local_smoothing.to_bits(), Ordering::Release);
    REMOTE_SMOOTHING_BITS.store(settings.remote_smoothing.to_bits(), Ordering::Release);
    COLLISION_ENABLED.store(settings.collision_enabled, Ordering::Release);
    COLLISION_RELEASE_SMOOTHING_BITS.store(
        settings.collision_release_smoothing.to_bits(),
        Ordering::Release,
    );
    log::info!(
        "lean collision: {}, release smoothing {}",
        if settings.collision_enabled {
            "on"
        } else {
            "off"
        },
        settings.collision_release_smoothing
    );
    settings
}

fn owner() -> &'static Owner {
    OWNER
        .get()
        .expect("the config is loaded before anything saves it")
}

pub fn start_hotkeys(toggle: Action, cycle_mode: Action, yaw_mode: Action) {
    write_log(&owner().start_hotkeys(toggle, cycle_mode, yaw_mode));
}

fn log_save(what: &str, (status, lines): (SaveStatus, Vec<Line>)) {
    write_log(&lines);
    if status != SaveStatus::Saved {
        log::warn!("config: {what} was not saved ({status:?})");
    }
}

/// Saves the tracking mode the cycle hotkey just applied.
pub fn save_tracking_mode(rotation_enabled: bool, position_enabled: bool) {
    log_save(
        "the tracking mode",
        owner().save_tracking_mode(rotation_enabled, position_enabled),
    );
}

/// Saves the yaw mode the yaw hotkey just applied.
pub fn save_world_space_yaw(world_space_yaw: bool) {
    log_save(
        "the yaw mode",
        owner().save_world_space_yaw(world_space_yaw),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smoothing_defaults_match_the_core() {
        assert_eq!(local_smoothing(), crate::smoothing::DEFAULT_LOCAL_SMOOTHING);
        assert_eq!(
            remote_smoothing(),
            crate::smoothing::DEFAULT_REMOTE_SMOOTHING
        );
    }

    #[test]
    fn a_folder_gets_one_trailing_separator() {
        let with = wide_folder(Path::new("C:\\Games\\Build\\Final\\"));
        let without = wide_folder(Path::new("C:\\Games\\Build\\Final"));
        assert_eq!(with, without);
        assert_eq!(
            String::from_utf16(&with).unwrap(),
            "C:\\Games\\Build\\Final\\"
        );
    }
}
