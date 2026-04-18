//! UE2.5 engine-level camera hook.
//!
//! Hooks `APlayerController::eventPlayerCalcView(AActor** ViewActor,
//! FVector* CameraLocation, FRotator* CameraRotation)` - the event thunk
//! the engine calls per frame to compute the render view.
//!
//! Our detour lets the original run (UnrealScript fills `*CameraRotation`
//! with the game's intended view - typically `Controller.Rotation`), then
//! adds the current head-tracked offset on top.
//!
//! Aim stays decoupled because the gameplay code reads
//! `APlayerController.Rotation` directly (the mouse-driven control
//! rotation), which we never touch.
//!
//! ## Absolute offset, not a delta
//!
//! The original thunk rebuilds `*CameraRotation` from
//! `Controller.Rotation` on every call, so each frame the buffer arrives
//! "clean". We therefore add the full head-tracked rotation each frame,
//! not a delta. Toggle-off returns the view to the mouse instantly; no
//! accumulator state to keep in sync.
//!
//! ## Units
//!
//! UE2.5 `FRotator` stores each axis as an `i32` where one full turn is
//! `0x10000` (65536 units = 360°). Conversion is
//! `units = (degrees * 65536 / 360) as i32`.

use std::ffi::c_void;
use std::sync::atomic::{AtomicI32, AtomicU64, AtomicUsize, Ordering};
use std::time::Instant;

use once_cell::sync::OnceCell;

use crate::hook_util::install_hook;
use crate::tracking::{
    get_recentered_position_atomic, get_recentered_rotation_atomic, is_enabled_atomic,
    is_position_enabled_atomic,
};

/// UE2.5 FRotator layout.
#[repr(C)]
struct FRotator {
    pitch: i32,
    yaw: i32,
    roll: i32,
}

/// UE2.5 FVector - three single-precision floats in world units.
/// BSR uses 1 unit = 1 cm, matching OpenTrack's position units, so
/// head deltas apply 1:1.
#[repr(C)]
struct FVector {
    x: f32,
    y: f32,
    z: f32,
}

/// Asymmetric per-axis position limits, in centimetres. More forward
/// than back so the player can lean toward the screen without the
/// camera clipping through the player model behind them.
const POS_LIMIT_FORWARD_CM: f64 = 40.0;
const POS_LIMIT_BACK_CM: f64 = 10.0;
const POS_LIMIT_SIDE_CM: f64 = 30.0;
const POS_LIMIT_UP_CM: f64 = 20.0;
const POS_LIMIT_DOWN_CM: f64 = 5.0;

/// `eventPlayerCalcView` is MSVC __thiscall on x86: `this` in ECX, stack
/// args pushed right-to-left, callee cleans.
type EventPlayerCalcViewFn = unsafe extern "thiscall" fn(
    this: *mut c_void,
    view_actor: *mut *mut c_void,
    camera_location: *mut c_void,
    camera_rotation: *mut FRotator,
);

static ORIGINAL: OnceCell<EventPlayerCalcViewFn> = OnceCell::new();

/// Milliseconds-since-start of the most recent `eventPlayerCalcView`
/// call. The D3D11 overlay uses its recency as the "we're in gameplay"
/// signal.
pub static LAST_PCV_MS: AtomicU64 = AtomicU64::new(0);

/// Cached PlayerController `this` pointer, captured each
/// `eventPlayerCalcView` call. The overlay reads
/// `*(this + FOV_LIVE_OFFSET) as f32` to get the live in-engine FOV.
pub static PLAYER_CONTROLLER_PTR: AtomicUsize = AtomicUsize::new(0);

/// Cache of the most recently `is_memory_valid`-confirmed camera
/// `FVector` pointer. The PCV detour gets the same pointer every frame
/// once the world is loaded, so a per-frame `VirtualQuery` syscall is
/// wasted work. Re-validates on pointer change (level transition).
static VALIDATED_CAMERA_LOCATION_PTR: AtomicUsize = AtomicUsize::new(0);

/// Snapshot of the CLEAN (mouse-driven) camera rotation, captured each
/// `eventPlayerCalcView` call BEFORE the head-tracking delta is layered
/// on top. Stored in raw FRotator units (65536 = 360°). The overlay
/// uses these to project gun-aim direction through the head-rotated
/// view.
pub static CLEAN_PITCH_UNITS: AtomicI32 = AtomicI32::new(0);
pub static CLEAN_YAW_UNITS: AtomicI32 = AtomicI32::new(0);
pub static CLEAN_ROLL_UNITS: AtomicI32 = AtomicI32::new(0);

pub fn units_to_deg(units: i32) -> f64 {
    units as f64 * (360.0 / 65536.0)
}

/// Offset of `DefaultFOV` in BSR's PlayerController. The in-game FOV
/// slider writes elsewhere we couldn't locate, so users running a
/// non-default FOV declare it via `bioshock_headtrack.ini`.
pub const FOV_LIVE_OFFSET: usize = 0x00E0;

/// Read the live FOV (horizontal degrees, at the player's actual
/// rendering aspect). Returns `None` if the PlayerController hasn't
/// been captured yet or the value is outside a sane range.
pub fn read_game_fov_h_native() -> Option<f32> {
    let ptr = PLAYER_CONTROLLER_PTR.load(Ordering::Acquire);
    if ptr == 0 {
        return None;
    }
    if !crate::memory::is_memory_valid(ptr + FOV_LIVE_OFFSET, 4) {
        return None;
    }
    unsafe {
        let p = (ptr as *const u8).add(FOV_LIVE_OFFSET) as *const f32;
        let v = std::ptr::read_unaligned(p);
        if v.is_finite() && (30.0..=150.0).contains(&v) {
            Some(v)
        } else {
            None
        }
    }
}

/// Monotonic time source shared by every "recent activity?" probe.
pub fn now_ms() -> u64 {
    static START: OnceCell<Instant> = OnceCell::new();
    START.get_or_init(Instant::now).elapsed().as_millis() as u64
}

const UNITS_PER_DEGREE: f64 = 65536.0 / 360.0;

#[inline]
fn deg_to_units(deg: f64) -> i32 {
    (deg * UNITS_PER_DEGREE) as i32
}

/// The detour. Calls original first, then mutates the out-FRotator
/// (and the camera FVector when 6DOF position is enabled).
unsafe extern "thiscall" fn event_player_calc_view_detour(
    this: *mut c_void,
    view_actor: *mut *mut c_void,
    camera_location: *mut c_void,
    camera_rotation: *mut FRotator,
) {
    let Some(&original) = ORIGINAL.get() else {
        return;
    };

    // Let UnrealScript compute the canonical view first. After this
    // returns, `*camera_rotation` is the rotation the engine would render
    // with.
    original(this, view_actor, camera_location, camera_rotation);

    // Stamp the "gameplay is live" timestamp.
    LAST_PCV_MS.store(now_ms(), Ordering::Relaxed);

    // Cache the PlayerController pointer for live FOV reads.
    if !this.is_null() {
        PLAYER_CONTROLLER_PTR.store(this as usize, Ordering::Release);
    }

    if camera_rotation.is_null() {
        return;
    }

    // Snapshot the CLEAN rotation BEFORE we add the head-tracking
    // delta. The overlay needs this for parallax-correct projection.
    let clean = &*camera_rotation;
    CLEAN_PITCH_UNITS.store(clean.pitch, Ordering::Relaxed);
    CLEAN_YAW_UNITS.store(clean.yaw, Ordering::Relaxed);
    CLEAN_ROLL_UNITS.store(clean.roll, Ordering::Relaxed);

    if !is_enabled_atomic() {
        return;
    }

    let (yaw_deg, pitch_deg, roll_deg) = get_recentered_rotation_atomic();

    // Roll is inverted: BioShock's FRotator.Roll increases clockwise
    // around the view axis, OpenTrack reports counter-clockwise positive.
    let rot = &mut *camera_rotation;
    rot.pitch = rot.pitch.wrapping_add(deg_to_units(pitch_deg));
    rot.yaw = rot.yaw.wrapping_add(deg_to_units(yaw_deg));
    rot.roll = rot.roll.wrapping_add(deg_to_units(-roll_deg));

    // 6DOF position. Apply the head's translational delta to the
    // engine's CameraLocation FVector. Lateral / forward components
    // are rotated by the camera's CLEAN yaw so leaning forward goes
    // "into the screen" relative to the player's in-world heading,
    // not where their head is currently turned.
    if is_position_enabled_atomic() && !camera_location.is_null() {
        let (right_cm, up_cm, forward_cm) = get_recentered_position_atomic();

        let right = right_cm.clamp(-POS_LIMIT_SIDE_CM, POS_LIMIT_SIDE_CM);
        let up = up_cm.clamp(-POS_LIMIT_DOWN_CM, POS_LIMIT_UP_CM);
        let forward = forward_cm.clamp(-POS_LIMIT_BACK_CM, POS_LIMIT_FORWARD_CM);

        // Rotate (forward, right) into world XY using clean yaw.
        // UE convention: forward = +X world, right = +Y world.
        let yaw_rad = units_to_deg(clean.yaw).to_radians();
        let cos_y = yaw_rad.cos();
        let sin_y = yaw_rad.sin();
        let world_dx = forward * cos_y - right * sin_y;
        let world_dy = forward * sin_y + right * cos_y;
        let world_dz = up;

        let loc = camera_location as *mut FVector;
        let loc_addr = loc as usize;
        // Pointer-cache the VirtualQuery: skip the syscall when this
        // is the same FVector slot we've already validated.
        let cached = VALIDATED_CAMERA_LOCATION_PTR.load(Ordering::Relaxed);
        let valid = if cached == loc_addr {
            true
        } else if crate::memory::is_memory_valid(loc_addr, std::mem::size_of::<FVector>()) {
            VALIDATED_CAMERA_LOCATION_PTR.store(loc_addr, Ordering::Relaxed);
            true
        } else {
            false
        };
        if valid {
            (*loc).x += world_dx as f32;
            (*loc).y += world_dy as f32;
            (*loc).z += world_dz as f32;
        }

        // Publish the body-frame head offset so the overlay can do
        // parallax-correct reticle projection.
        crate::tracking::store_applied_head_offset(right, up, forward);
    } else {
        // Position tracking off / no camera_location - overlay must
        // not apply any parallax compensation this frame.
        crate::tracking::store_applied_head_offset(0.0, 0.0, 0.0);
    }
}

/// Install the detour. Idempotent.
pub fn install(target_addr: usize) -> Result<(), String> {
    unsafe {
        install_hook(
            target_addr as *mut c_void,
            event_player_calc_view_detour as *mut c_void,
            &ORIGINAL,
            "eventPlayerCalcView",
        )
    }
}
