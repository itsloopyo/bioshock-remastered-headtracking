//! BioShock Remastered Head Tracking Mod
//!
//! 6DOF head tracking for BioShock Remastered via OpenTrack UDP on the
//! project-standard port 4242.
//!
//! Ships as a DLL hijack via `xinput1_3.dll` drop-in. The game loads this
//! proxy, which forwards XInput calls to the real library while we initialize
//! the tracking pipeline (OpenTrack receiver, hotkey poller, the UE 2.5
//! `eventPlayerCalcView` hook, and the D3D11 reticle overlay / HUD-suppress
//! hooks).

// This module is the XInput proxy boundary. All `pub unsafe extern "system"`
// XInput exports have the same trivial safety contract (caller must satisfy
// the XInput ABI), and all `transmute` calls here cast `FARPROC` to a typed
// XInput function pointer - same shape every time. Adding identical
// `# Safety` paragraphs and `transmute::<From, To>` turbofishes 7x adds
// noise without information.
#![allow(clippy::missing_safety_doc, clippy::missing_transmute_annotations)]

mod config;
mod d3d;
mod engine_hook;
mod hook_util;
mod hotkeys;
mod memory;
mod opentrack;
mod tracking;

use std::ffi::c_void;

use once_cell::sync::OnceCell;
use windows::core::{s, w};
use windows::Win32::Foundation::{BOOL, HINSTANCE, HMODULE, TRUE};
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows::Win32::System::SystemServices::{DLL_PROCESS_ATTACH, DLL_PROCESS_DETACH};

use crate::tracking::GLOBAL_STATE;

/// Wrapper for HMODULE to make it Send+Sync
/// Safety: Module handles are valid for the lifetime of the process
#[derive(Clone, Copy)]
struct SendSyncModule(#[allow(dead_code)] HMODULE);
unsafe impl Send for SendSyncModule {}
unsafe impl Sync for SendSyncModule {}

/// Version of the mod
const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Handle to the real xinput1_3.dll loaded from System32
static REAL_XINPUT: OnceCell<SendSyncModule> = OnceCell::new();

// XInput function type definitions
type XInputGetStateFn = unsafe extern "system" fn(u32, *mut XInputState) -> u32;
type XInputSetStateFn = unsafe extern "system" fn(u32, *mut XInputVibration) -> u32;
type XInputGetCapabilitiesFn = unsafe extern "system" fn(u32, u32, *mut XInputCapabilities) -> u32;
type XInputEnableFn = unsafe extern "system" fn(BOOL);
type XInputGetDSoundAudioDeviceGuidsFn =
    unsafe extern "system" fn(u32, *mut windows::core::GUID, *mut windows::core::GUID) -> u32;
type XInputGetBatteryInformationFn =
    unsafe extern "system" fn(u32, u8, *mut XInputBatteryInformation) -> u32;
type XInputGetKeystrokeFn = unsafe extern "system" fn(u32, u32, *mut XInputKeystroke) -> u32;

// Static function pointers for XInput proxy
static XINPUT_GET_STATE: OnceCell<XInputGetStateFn> = OnceCell::new();
static XINPUT_SET_STATE: OnceCell<XInputSetStateFn> = OnceCell::new();
static XINPUT_GET_CAPABILITIES: OnceCell<XInputGetCapabilitiesFn> = OnceCell::new();
static XINPUT_ENABLE: OnceCell<XInputEnableFn> = OnceCell::new();
static XINPUT_GET_DSOUND_AUDIO_DEVICE_GUIDS: OnceCell<XInputGetDSoundAudioDeviceGuidsFn> =
    OnceCell::new();
static XINPUT_GET_BATTERY_INFORMATION: OnceCell<XInputGetBatteryInformationFn> = OnceCell::new();
static XINPUT_GET_KEYSTROKE: OnceCell<XInputGetKeystrokeFn> = OnceCell::new();

// XInput structures (simplified for proxy purposes)
#[repr(C)]
pub struct XInputState {
    pub packet_number: u32,
    pub gamepad: XInputGamepad,
}

#[repr(C)]
pub struct XInputGamepad {
    pub buttons: u16,
    pub left_trigger: u8,
    pub right_trigger: u8,
    pub thumb_lx: i16,
    pub thumb_ly: i16,
    pub thumb_rx: i16,
    pub thumb_ry: i16,
}

#[repr(C)]
pub struct XInputVibration {
    pub left_motor_speed: u16,
    pub right_motor_speed: u16,
}

#[repr(C)]
pub struct XInputCapabilities {
    pub type_: u8,
    pub sub_type: u8,
    pub flags: u16,
    pub gamepad: XInputGamepad,
    pub vibration: XInputVibration,
}

#[repr(C)]
pub struct XInputBatteryInformation {
    pub battery_type: u8,
    pub battery_level: u8,
}

#[repr(C)]
pub struct XInputKeystroke {
    pub virtual_key: u16,
    pub unicode: u16,
    pub flags: u16,
    pub user_index: u8,
    pub hid_code: u8,
}

/// Error code for XInput: device not connected
const ERROR_DEVICE_NOT_CONNECTED: u32 = 1167;

/// Load the real xinput1_3.dll from System32 and get all function pointers
fn load_real_xinput() -> Result<(), &'static str> {
    unsafe {
        let path = w!("C:\\Windows\\System32\\xinput1_3.dll");
        let handle = LoadLibraryW(path).map_err(|_| "Failed to load real xinput1_3.dll")?;

        REAL_XINPUT
            .set(SendSyncModule(handle))
            .map_err(|_| "Failed to store xinput handle")?;

        // Get all XInput function pointers
        let get_state = GetProcAddress(handle, s!("XInputGetState"));
        if let Some(ptr) = get_state {
            let _ = XINPUT_GET_STATE.set(std::mem::transmute(ptr));
        }

        let set_state = GetProcAddress(handle, s!("XInputSetState"));
        if let Some(ptr) = set_state {
            let _ = XINPUT_SET_STATE.set(std::mem::transmute(ptr));
        }

        let get_capabilities = GetProcAddress(handle, s!("XInputGetCapabilities"));
        if let Some(ptr) = get_capabilities {
            let _ = XINPUT_GET_CAPABILITIES.set(std::mem::transmute(ptr));
        }

        let enable = GetProcAddress(handle, s!("XInputEnable"));
        if let Some(ptr) = enable {
            let _ = XINPUT_ENABLE.set(std::mem::transmute(ptr));
        }

        let get_dsound = GetProcAddress(handle, s!("XInputGetDSoundAudioDeviceGuids"));
        if let Some(ptr) = get_dsound {
            let _ = XINPUT_GET_DSOUND_AUDIO_DEVICE_GUIDS.set(std::mem::transmute(ptr));
        }

        let get_battery = GetProcAddress(handle, s!("XInputGetBatteryInformation"));
        if let Some(ptr) = get_battery {
            let _ = XINPUT_GET_BATTERY_INFORMATION.set(std::mem::transmute(ptr));
        }

        let get_keystroke = GetProcAddress(handle, s!("XInputGetKeystroke"));
        if let Some(ptr) = get_keystroke {
            let _ = XINPUT_GET_KEYSTROKE.set(std::mem::transmute(ptr));
        }
    }
    Ok(())
}

/// Initialize the mod from the spawn thread: logging → config → runtime
/// state + threads → engine hooks → D3D11 hooks. Runs once.
fn initialize_mod() {
    init_logging();
    log::info!("BioShock Head Tracking v{} loaded", VERSION);
    config::load();
    init_runtime_state();
    install_engine_hook();
    install_d3d_hooks();
}

/// File logger setup. Falls back to `%TEMP%` if the working-directory
/// log file can't be created (e.g. read-only Steam install).
fn init_logging() {
    if let Err(e) = simplelog::WriteLogger::init(
        simplelog::LevelFilter::Info,
        simplelog::Config::default(),
        std::fs::File::create("bioshock_headtrack.log").unwrap_or_else(|_| {
            std::fs::File::create(std::env::temp_dir().join("bioshock_headtrack.log"))
                .expect("Failed to create log file")
        }),
    ) {
        eprintln!("Failed to initialize logging: {}", e);
    }
}

/// Bring up the runtime state shared across the OpenTrack receiver,
/// the hotkey thread, and the engine / D3D hooks.
fn init_runtime_state() {
    // Touch the lazy global so it constructs before threads race for it.
    let _ = &*GLOBAL_STATE;

    if let Err(e) = opentrack::start_receiver() {
        log::error!("Failed to start OpenTrack receiver: {}", e);
    }

    hotkeys::start_hotkey_thread();
}

/// Locate `APlayerController::eventPlayerCalcView` in the live module
/// and install the head-tracking detour.
fn install_engine_hook() {
    let Some(scanner) = memory::MemoryScanner::new() else {
        log::error!("Failed to create memory scanner");
        return;
    };
    let Some(addr) = memory::find_player_calc_view_target(&scanner) else {
        log::error!(
            "Could not locate eventPlayerCalcView - head tracking will not move the camera"
        );
        return;
    };
    if let Err(e) = engine_hook::install(addr) {
        log::error!("Engine hook install failed: {}", e);
    }
}

/// Hook the swap chain `Present` (for the overlay reticle) and the
/// two `ID3D11DeviceContext` draw APIs Scaleform uses for the HUD
/// (for HUD-active gating + reticle suppression).
fn install_d3d_hooks() {
    if let Err(e) = d3d::hud::install() {
        log::error!("D3D11 hook install failed: {}", e);
    }
}

/// Shutdown the mod: stop threads, cleanup.
fn shutdown_mod() {
    GLOBAL_STATE.write().shutdown_requested = true;
}

/// DLL entry point - called by Windows when the DLL is loaded/unloaded
#[no_mangle]
pub extern "system" fn DllMain(_hmodule: HINSTANCE, reason: u32, _reserved: *mut c_void) -> BOOL {
    match reason {
        DLL_PROCESS_ATTACH => {
            // Load real xinput1_3.dll SYNCHRONOUSLY - must be ready before any exports are called
            if load_real_xinput().is_err() {
                return BOOL(0);
            }
            // Spawn initialization thread for everything else
            std::thread::spawn(|| {
                initialize_mod();
            });
        }
        DLL_PROCESS_DETACH => {
            shutdown_mod();
        }
        _ => {}
    }
    TRUE
}

// ============================================================================
// XInput Proxy Exports
// These functions forward calls to the real xinput1_3.dll
// ============================================================================

#[no_mangle]
pub unsafe extern "system" fn XInputGetState(user_index: u32, state: *mut XInputState) -> u32 {
    if let Some(func) = XINPUT_GET_STATE.get() {
        func(user_index, state)
    } else {
        ERROR_DEVICE_NOT_CONNECTED
    }
}

#[no_mangle]
pub unsafe extern "system" fn XInputSetState(
    user_index: u32,
    vibration: *mut XInputVibration,
) -> u32 {
    if let Some(func) = XINPUT_SET_STATE.get() {
        func(user_index, vibration)
    } else {
        ERROR_DEVICE_NOT_CONNECTED
    }
}

#[no_mangle]
pub unsafe extern "system" fn XInputGetCapabilities(
    user_index: u32,
    flags: u32,
    capabilities: *mut XInputCapabilities,
) -> u32 {
    if let Some(func) = XINPUT_GET_CAPABILITIES.get() {
        func(user_index, flags, capabilities)
    } else {
        ERROR_DEVICE_NOT_CONNECTED
    }
}

#[no_mangle]
pub unsafe extern "system" fn XInputEnable(enable: BOOL) {
    if let Some(func) = XINPUT_ENABLE.get() {
        func(enable);
    }
}

#[no_mangle]
pub unsafe extern "system" fn XInputGetDSoundAudioDeviceGuids(
    user_index: u32,
    render_guid: *mut windows::core::GUID,
    capture_guid: *mut windows::core::GUID,
) -> u32 {
    if let Some(func) = XINPUT_GET_DSOUND_AUDIO_DEVICE_GUIDS.get() {
        func(user_index, render_guid, capture_guid)
    } else {
        ERROR_DEVICE_NOT_CONNECTED
    }
}

#[no_mangle]
pub unsafe extern "system" fn XInputGetBatteryInformation(
    user_index: u32,
    dev_type: u8,
    battery_information: *mut XInputBatteryInformation,
) -> u32 {
    if let Some(func) = XINPUT_GET_BATTERY_INFORMATION.get() {
        func(user_index, dev_type, battery_information)
    } else {
        ERROR_DEVICE_NOT_CONNECTED
    }
}

#[no_mangle]
pub unsafe extern "system" fn XInputGetKeystroke(
    user_index: u32,
    reserved: u32,
    keystroke: *mut XInputKeystroke,
) -> u32 {
    if let Some(func) = XINPUT_GET_KEYSTROKE.get() {
        func(user_index, reserved, keystroke)
    } else {
        ERROR_DEVICE_NOT_CONNECTED
    }
}
