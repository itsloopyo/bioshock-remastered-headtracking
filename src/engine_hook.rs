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
    is_enabled_atomic, is_position_enabled_atomic, is_rotation_enabled_atomic,
    is_world_space_yaw_atomic,
};

/// UE2.5 FRotator layout.
#[repr(C)]
#[derive(Clone, Copy)]
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

#[derive(Clone, Copy)]
struct Vec3 {
    x: f64,
    y: f64,
    z: f64,
}

#[derive(Clone, Copy)]
struct Basis {
    forward: Vec3,
    right: Vec3,
    up: Vec3,
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

fn rotator_to_basis(rot: &FRotator) -> Basis {
    let pitch = units_to_deg(rot.pitch).to_radians();
    let yaw = units_to_deg(rot.yaw).to_radians();
    let roll = units_to_deg(rot.roll).to_radians();

    let cp = pitch.cos();
    let sp = pitch.sin();
    let cy = yaw.cos();
    let sy = yaw.sin();
    let cr = roll.cos();
    let sr = roll.sin();

    let forward = Vec3 {
        x: cp * cy,
        y: cp * sy,
        z: sp,
    };
    let right0 = Vec3 {
        x: -sy,
        y: cy,
        z: 0.0,
    };
    let up0 = Vec3 {
        x: -sp * cy,
        y: -sp * sy,
        z: cp,
    };
    Basis {
        forward,
        right: add(scale(right0, cr), scale(up0, -sr)),
        up: add(scale(right0, sr), scale(up0, cr)),
    }
}

fn basis_to_rotator(basis: Basis) -> FRotator {
    let pitch = basis.forward.z.clamp(-1.0, 1.0).asin();
    let yaw = basis.forward.y.atan2(basis.forward.x);
    let cp = pitch.cos();
    let (right0, up0) = if cp.abs() > 1e-6 {
        (
            Vec3 {
                x: -yaw.sin(),
                y: yaw.cos(),
                z: 0.0,
            },
            Vec3 {
                x: -pitch.sin() * yaw.cos(),
                y: -pitch.sin() * yaw.sin(),
                z: cp,
            },
        )
    } else {
        (
            Vec3 {
                x: basis.right.x,
                y: basis.right.y,
                z: 0.0,
            },
            Vec3 {
                x: 0.0,
                y: 0.0,
                z: cp.signum(),
            },
        )
    };
    let roll = (-dot(basis.right, up0)).atan2(dot(basis.right, right0));
    FRotator {
        pitch: deg_to_units(pitch.to_degrees()),
        yaw: deg_to_units(yaw.to_degrees()),
        roll: deg_to_units(roll.to_degrees()),
    }
}

fn apply_world_space_yaw(
    clean: &FRotator,
    yaw_deg: f64,
    pitch_deg: f64,
    roll_deg: f64,
) -> FRotator {
    let clean_basis = rotator_to_basis(clean);
    let yawed = rotate_world_z(clean_basis, yaw_deg.to_radians());
    let pitch_roll = rotator_to_basis(&FRotator {
        pitch: deg_to_units(pitch_deg),
        yaw: 0,
        roll: deg_to_units(-roll_deg),
    });
    basis_to_rotator(mul_basis(yawed, pitch_roll))
}

fn apply_camera_local_yaw(
    clean: &FRotator,
    yaw_deg: f64,
    pitch_deg: f64,
    roll_deg: f64,
) -> FRotator {
    let clean_basis = rotator_to_basis(clean);
    let head = rotator_to_basis(&FRotator {
        pitch: deg_to_units(pitch_deg),
        yaw: deg_to_units(yaw_deg),
        roll: deg_to_units(-roll_deg),
    });
    basis_to_rotator(mul_basis(clean_basis, head))
}

fn rotate_world_z(basis: Basis, angle: f64) -> Basis {
    Basis {
        forward: rotate_vec_world_z(basis.forward, angle),
        right: rotate_vec_world_z(basis.right, angle),
        up: rotate_vec_world_z(basis.up, angle),
    }
}

fn rotate_vec_world_z(v: Vec3, angle: f64) -> Vec3 {
    let c = angle.cos();
    let s = angle.sin();
    Vec3 {
        x: v.x * c - v.y * s,
        y: v.x * s + v.y * c,
        z: v.z,
    }
}

fn mul_basis(a: Basis, b: Basis) -> Basis {
    Basis {
        forward: transform_vec(a, b.forward),
        right: transform_vec(a, b.right),
        up: transform_vec(a, b.up),
    }
}

fn transform_vec(basis: Basis, v: Vec3) -> Vec3 {
    add(
        add(scale(basis.forward, v.x), scale(basis.right, v.y)),
        scale(basis.up, v.z),
    )
}

fn scale(v: Vec3, s: f64) -> Vec3 {
    Vec3 {
        x: v.x * s,
        y: v.y * s,
        z: v.z * s,
    }
}

fn add(a: Vec3, b: Vec3) -> Vec3 {
    Vec3 {
        x: a.x + b.x,
        y: a.y + b.y,
        z: a.z + b.z,
    }
}

fn dot(a: Vec3, b: Vec3) -> f64 {
    a.x * b.x + a.y * b.y + a.z * b.z
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
    let clean = *camera_rotation;
    CLEAN_PITCH_UNITS.store(clean.pitch, Ordering::Relaxed);
    CLEAN_YAW_UNITS.store(clean.yaw, Ordering::Relaxed);
    CLEAN_ROLL_UNITS.store(clean.roll, Ordering::Relaxed);

    if !is_enabled_atomic() {
        return;
    }

    // Drive the per-axis interpolator + smoother once per frame. The
    // interpolator bridges low-rate trackers (60Hz phone) to the
    // display refresh rate so the camera advances every frame instead
    // of every other frame. Both rotation and position are returned;
    // the same values are also published to ATOMIC_SMOOTHED_* so the
    // D3D overlay's reticle projection stays glued to the rendered
    // view.
    let pose = crate::smoothing::tick_frame();
    let (yaw_deg, pitch_deg, roll_deg) = pose.rotation;

    // Roll is inverted: BioShock's FRotator.Roll increases clockwise
    // around the view axis, OpenTrack reports counter-clockwise positive.
    if is_rotation_enabled_atomic() {
        let rot = &mut *camera_rotation;
        if is_world_space_yaw_atomic() {
            *rot = apply_world_space_yaw(&clean, yaw_deg, pitch_deg, roll_deg);
        } else {
            *rot = apply_camera_local_yaw(&clean, yaw_deg, pitch_deg, roll_deg);
        }
    }

    // 6DOF position. Apply the head's translational delta to the
    // engine's CameraLocation FVector. Lateral / forward components
    // are rotated by the camera's CLEAN yaw so leaning forward goes
    // "into the screen" relative to the player's in-world heading,
    // not where their head is currently turned.
    if is_position_enabled_atomic() && !camera_location.is_null() {
        let (right_cm, up_cm, forward_cm) = pose.position;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yaw_modes_diverge_at_steep_pitch() {
        let clean = FRotator {
            pitch: deg_to_units(-80.0),
            yaw: 0,
            roll: 0,
        };

        let world = apply_world_space_yaw(&clean, 30.0, 0.0, 0.0);
        let local = apply_camera_local_yaw(&clean, 30.0, 0.0, 0.0);

        let pitch_delta = (world.pitch - local.pitch).abs();
        let roll_delta = (world.roll - local.roll).abs();
        assert!(
            pitch_delta > deg_to_units(5.0) || roll_delta > deg_to_units(5.0),
            "expected yaw modes to diverge at steep pitch, world=({}, {}, {}), local=({}, {}, {})",
            units_to_deg(world.pitch),
            units_to_deg(world.yaw),
            units_to_deg(world.roll),
            units_to_deg(local.pitch),
            units_to_deg(local.yaw),
            units_to_deg(local.roll)
        );
    }
}
