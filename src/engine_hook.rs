use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::time::Instant;

use once_cell::sync::OnceCell;
use parking_lot::Mutex;

use crate::hook_util::install_hook;
use crate::projection::{self, Matrix};
use crate::tracking::{
    is_enabled_atomic, is_position_enabled_atomic, is_rotation_enabled_atomic,
    is_world_space_yaw_atomic,
};

#[repr(C)]
#[derive(Clone, Copy)]
struct FRotator {
    pitch: i32,
    yaw: i32,
    roll: i32,
}

#[repr(C)]
#[derive(Clone, Copy)]
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

type CameraSceneNodeFn = unsafe extern "thiscall" fn(
    *mut u8,
    *mut c_void,
    *mut c_void,
    *mut u8,
    f32,
    f32,
    f32,
    i32,
    i32,
    i32,
    f32,
    f32,
) -> *mut u8;
type UpdateMatricesFn = unsafe extern "thiscall" fn(*mut u8);
type LineCheckFn = unsafe extern "thiscall" fn(
    *mut c_void,
    *mut Hit,
    *mut u8,
    *const FVector,
    *const FVector,
    u32,
    *const FVector,
    u32,
) -> i32;

#[repr(C)]
struct Hit {
    next: u32,
    actor: u32,
    location: FVector,
    normal: FVector,
    primitive: u32,
    time: f32,
    item: i32,
    rest: [u32; 5],
}

extern "C" {
    fn apply_lean_clamp(
        offset: *mut f32,
        delta_time: f32,
        skin: f32,
        release_smoothing: f32,
        blocked: i32,
        distance: f32,
    );
    fn reset_lean_clamp();
}

static ORIGINAL: OnceCell<CameraSceneNodeFn> = OnceCell::new();
static UPDATE_MATRICES: OnceCell<UpdateMatricesFn> = OnceCell::new();
#[derive(Clone, Copy)]
pub enum ReticleState {
    Inactive,
    Hidden,
    Position([f32; 2]),
}

static RETICLE: Mutex<ReticleState> = Mutex::new(ReticleState::Inactive);
static CLEAN_SCENE: Mutex<Option<(usize, Matrix, Matrix)>> = Mutex::new(None);

pub fn clean_scene(scene: usize) -> Option<(Matrix, Matrix)> {
    // Auxiliary views must keep their own matrices.
    CLEAN_SCENE
        .lock()
        .filter(|entry| entry.0 == scene)
        .map(|entry| (entry.1, entry.2))
}
static LAST_VIEW_MS: AtomicU64 = AtomicU64::new(0);
static LAST_LOG_MS: AtomicU64 = AtomicU64::new(0);
static LAST_ACTOR: AtomicUsize = AtomicUsize::new(0);
static LAST_PAUSED: AtomicBool = AtomicBool::new(false);
static VIEW_CALLS: AtomicU64 = AtomicU64::new(0);

pub fn now_ms() -> u64 {
    static START: OnceCell<Instant> = OnceCell::new();
    START.get_or_init(Instant::now).elapsed().as_millis() as u64
}

pub fn reticle_state() -> ReticleState {
    if !is_enabled_atomic() || now_ms().saturating_sub(LAST_VIEW_MS.load(Ordering::Relaxed)) > 250 {
        return ReticleState::Inactive;
    }
    *RETICLE.lock()
}

pub fn units_to_deg(units: i32) -> f64 {
    units as f64 * (360.0 / 65536.0)
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

/// |cos(pitch)| below which yaw and roll are the same rotation and have to
/// be extracted together. Reached whenever the game's own view pitch plus
/// the head's pitch lands on vertical, which the 65536-unit FRotator
/// quantisation makes an exactly-representable angle rather than a
/// measure-zero one.
const VERTICAL_COS_PITCH: f64 = 1e-6;

fn basis_to_rotator(basis: Basis) -> FRotator {
    let pitch = basis.forward.z.clamp(-1.0, 1.0).asin();
    let cp = pitch.cos();

    if cp.abs() <= VERTICAL_COS_PITCH {
        // Forward is vertical, so `atan2(forward.y, forward.x)` has nothing
        // left to read and the yaw-from-forward, roll-from-right pair both
        // collapse to zero - discarding up to 90 degrees of heading and
        // snapping the view to due north. The right vector still carries it:
        // right is (-sin yaw, cos yaw, 0) whatever the pitch, and at the
        // singularity a roll about the vertical view axis IS a yaw, so
        // folding the whole heading into yaw reproduces the orientation
        // exactly and stays continuous with the general branch.
        let yaw = (-basis.right.x).atan2(basis.right.y);
        return FRotator {
            pitch: deg_to_units(pitch.to_degrees()),
            yaw: deg_to_units(yaw.to_degrees()),
            roll: 0,
        };
    }

    let yaw = basis.forward.y.atan2(basis.forward.x);
    let right0 = Vec3 {
        x: -yaw.sin(),
        y: yaw.cos(),
        z: 0.0,
    };
    let up0 = Vec3 {
        x: -pitch.sin() * yaw.cos(),
        y: -pitch.sin() * yaw.sin(),
        z: cp,
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

unsafe fn read_matrix(scene: *mut u8, offset: usize) -> Matrix {
    std::ptr::read_unaligned(scene.add(offset).cast())
}

unsafe fn write_matrix(scene: *mut u8, offset: usize, matrix: Matrix) {
    std::ptr::write_unaligned(scene.add(offset).cast(), matrix);
}

unsafe fn trace(actor: *mut u8, start: FVector, end: FVector, radius: f32) -> Hit {
    let level = *actor.add(0xfc).cast::<*mut c_void>();
    let vtable = *level.cast::<*const usize>();
    let line_check: LineCheckFn = std::mem::transmute(*vtable.add(0x168 / 4));
    let mut hit = Hit {
        next: 0,
        actor: 0,
        location: FVector {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        },
        normal: FVector {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        },
        primitive: 0,
        time: 1.0,
        item: -1,
        rest: [0; 5],
    };
    let extent = FVector {
        x: radius,
        y: radius,
        z: radius,
    };
    line_check(level, &mut hit, actor, &end, &start, 0x101097, &extent, 0);
    hit
}

#[allow(clippy::too_many_arguments)]
unsafe extern "thiscall" fn camera_scene_node_detour(
    this: *mut u8,
    viewport: *mut c_void,
    render_target: *mut c_void,
    actor: *mut u8,
    x: f32,
    y: f32,
    z: f32,
    pitch: i32,
    yaw: i32,
    roll: i32,
    fov: f32,
    weapon_fov: f32,
) -> *mut u8 {
    let scene = ORIGINAL.get().unwrap()(
        this,
        viewport,
        render_target,
        actor,
        x,
        y,
        z,
        pitch,
        yaw,
        roll,
        fov,
        weapon_fov,
    );
    let now = now_ms();
    let frame = VIEW_CALLS.fetch_add(1, Ordering::Relaxed) + 1;
    let previous = LAST_VIEW_MS.swap(now, Ordering::Relaxed);
    let changed_actor = LAST_ACTOR.swap(actor as usize, Ordering::Relaxed) != actor as usize;
    let pawn = *actor.add(0x450).cast::<*mut u8>();
    // ALevelInfo::Pauser is shared by the pause interface and its submenus.
    let level_info = *actor.add(0xf8).cast::<*mut u8>();
    let paused = !level_info.is_null() && *level_info.add(0x668).cast::<usize>() != 0;
    let changed_pause = LAST_PAUSED.swap(paused, Ordering::Relaxed) != paused;
    if changed_pause {
        crate::smoothing::reset();
        log::info!("pause state: paused={paused}");
    }
    let log_frame = changed_actor
        || changed_pause
        || now.saturating_sub(LAST_LOG_MS.load(Ordering::Relaxed)) >= 1000;
    if log_frame {
        LAST_LOG_MS.store(now, Ordering::Relaxed);
        log::info!(
            "render view: frame={frame} controller={actor:p} pawn={pawn:p} enabled={} paused={paused} FOV={fov}/{weapon_fov}",
            is_enabled_atomic()
        );
    }
    if changed_actor || !is_enabled_atomic() || pawn.is_null() || paused {
        reset_lean_clamp();
    }
    if !is_enabled_atomic() || pawn.is_null() || paused {
        *CLEAN_SCENE.lock() = None;
        *RETICLE.lock() = ReticleState::Inactive;
        return scene;
    }

    let clean_view = read_matrix(scene, 0x150);
    let clean_inverse = read_matrix(scene, 0x190);
    let world_projection = read_matrix(scene, 0x1d0);
    let inverse_projection = read_matrix(scene, 0x210);
    let weapon_projection = read_matrix(scene, 0x380);
    *CLEAN_SCENE.lock() = Some((scene as usize, clean_view, weapon_projection));
    let clean = FRotator { pitch, yaw, roll };
    let pose = crate::smoothing::tick_frame();
    if log_frame {
        log::info!(
            "render pose: rotation={:?} position={:?}",
            pose.rotation,
            pose.position
        );
    }
    let tracked = if !is_rotation_enabled_atomic() {
        clean
    } else if is_world_space_yaw_atomic() {
        apply_world_space_yaw(&clean, pose.rotation.0, pose.rotation.1, pose.rotation.2)
    } else {
        apply_camera_local_yaw(&clean, pose.rotation.0, pose.rotation.1, pose.rotation.2)
    };
    let mut offset = [0.0_f32; 3];
    if is_position_enabled_atomic() {
        let (right, up, forward) = pose.position;
        let angle = units_to_deg(yaw).to_radians();
        offset = [
            (forward * angle.cos() - right * angle.sin()) as f32,
            (forward * angle.sin() + right * angle.cos()) as f32,
            up as f32,
        ];
        let desired = offset.iter().map(|value| value * value).sum::<f32>().sqrt();
        let skin = (world_projection.0[3][2] / world_projection.0[2][2]).abs() + 1.0;
        if desired > 0.0001 && crate::config::collision_enabled() {
            let distance = desired + skin;
            let hit = trace(
                actor,
                FVector { x, y, z },
                FVector {
                    x: x + offset[0] * distance / desired,
                    y: y + offset[1] * distance / desired,
                    z: z + offset[2] * distance / desired,
                },
                skin,
            );
            apply_lean_clamp(
                offset.as_mut_ptr(),
                now.saturating_sub(previous) as f32 / 1000.0,
                skin,
                crate::config::collision_release_smoothing(),
                i32::from(hit.actor != 0),
                hit.time * distance,
            );
        } else {
            reset_lean_clamp();
        }
    } else {
        reset_lean_clamp();
    }

    let direction = clean_inverse.transform([0.0, 0.0, 1.0, 0.0]);
    let end = FVector {
        x: x + direction[0] * 100_000.0,
        y: y + direction[1] * 100_000.0,
        z: z + direction[2] * 100_000.0,
    };
    let hit = trace(actor, FVector { x, y, z }, end, 0.0);
    if log_frame {
        log::info!("render trace: actor={:#x} time={}", hit.actor, hit.time);
    }
    let aim = if hit.actor == 0 {
        direction
    } else {
        [
            x + (end.x - x) * hit.time,
            y + (end.y - y) * hit.time,
            z + (end.z - z) * hit.time,
            1.0,
        ]
    };

    std::ptr::write_unaligned(
        scene.add(0x310).cast(),
        FVector {
            x: x + offset[0],
            y: y + offset[1],
            z: z + offset[2],
        },
    );
    std::ptr::write_unaligned(scene.add(0x3e4).cast(), tracked);
    UPDATE_MATRICES.get().unwrap()(scene);

    let tracked_view = read_matrix(scene, 0x150);
    let corrected_weapon = projection::weapon_projection(
        clean_view,
        clean_inverse,
        tracked_view,
        read_matrix(scene, 0x190),
        world_projection,
        inverse_projection,
        weapon_projection,
    );
    write_matrix(scene, 0x380, corrected_weapon);
    write_matrix(scene, 0x290, tracked_view.multiply(corrected_weapon));
    let reticle = projection::project(aim, tracked_view.multiply(world_projection));
    *RETICLE.lock() = reticle.map_or(ReticleState::Hidden, ReticleState::Position);

    if log_frame {
        log::info!(
            "render: clean=({},{},{}) tracked=({},{},{}) offset={:?} world_scale=({:.4},{:.4}) weapon_scale=({:.4},{:.4}) aim_fraction={:.5} reticle={:?}",
            pitch, yaw, roll, tracked.pitch, tracked.yaw, tracked.roll, offset,
            world_projection.0[0][0], world_projection.0[1][1],
            weapon_projection.0[0][0], weapon_projection.0[1][1], hit.time, reticle,
        );
    }
    scene
}

pub fn install(constructor: usize, update_matrices: usize) -> Result<(), String> {
    unsafe {
        UPDATE_MATRICES
            .set(std::mem::transmute::<usize, UpdateMatricesFn>(
                update_matrices,
            ))
            .map_err(|_| "Camera scene matrix updater already installed".to_string())?;
        install_hook(
            constructor as *mut c_void,
            camera_scene_node_detour as *mut c_void,
            &ORIGINAL,
            "FCameraSceneNode",
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Largest per-axis discrepancy between two orientations. The FRotator
    /// round trip quantises to 65536ths of a turn, so ~2e-4 is the floor.
    fn basis_error(a: Basis, b: Basis) -> f64 {
        let axis = |p: Vec3, q: Vec3| {
            ((p.x - q.x).powi(2) + (p.y - q.y).powi(2) + (p.z - q.z).powi(2)).sqrt()
        };
        axis(a.forward, b.forward)
            .max(axis(a.right, b.right))
            .max(axis(a.up, b.up))
    }

    fn rotator(pitch_deg: f64, yaw_deg: f64, roll_deg: f64) -> FRotator {
        FRotator {
            pitch: deg_to_units(pitch_deg),
            yaw: deg_to_units(yaw_deg),
            roll: deg_to_units(roll_deg),
        }
    }

    #[test]
    fn vertical_pitch_keeps_the_heading() {
        // The game's view pitch plus the head's pitch summing to vertical is
        // ordinary play (look steeply down, tilt the head down). At exactly
        // vertical the yaw/roll pair is degenerate, and a decomposition that
        // zeroes both throws the heading away: the view snaps to due north,
        // then snaps back as soon as the sum leaves vertical.
        for &(clean_pitch, head_pitch) in &[(0.0, 90.0), (0.0, -90.0), (-46.0, -44.0)] {
            for &head_roll in &[0.0, 30.0, -75.0] {
                let clean = rotator(clean_pitch, 45.0, 0.0);
                let composed = mul_basis(
                    rotator_to_basis(&clean),
                    rotator_to_basis(&rotator(head_pitch, -30.0, -head_roll)),
                );

                let error = basis_error(composed, rotator_to_basis(&basis_to_rotator(composed)));
                assert!(
                    error < 1e-2,
                    "orientation lost at vertical pitch (clean {}, head {}, roll {}): error {}",
                    clean_pitch,
                    head_pitch,
                    head_roll,
                    error
                );
            }
        }
    }

    #[test]
    fn well_conditioned_pitch_round_trips() {
        // Control for the branch above: away from vertical the decomposition
        // is unchanged and must still reproduce its input.
        for &pitch in &[-80.0, -35.0, 0.0, 35.0, 80.0] {
            let composed = rotator_to_basis(&rotator(pitch, 137.0, 20.0));
            let error = basis_error(composed, rotator_to_basis(&basis_to_rotator(composed)));
            assert!(
                error < 1e-2,
                "round trip failed at pitch {}: {}",
                pitch,
                error
            );
        }
    }

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
