use crate::projection::Matrix;
use once_cell::sync::OnceCell;
use std::ffi::c_void;
use std::sync::atomic::{AtomicU64, Ordering};

type MeshFn = unsafe extern "thiscall" fn(*mut u8, *mut u8, *mut u8);
type LitMeshFn = unsafe extern "thiscall" fn(*mut u8, *mut u8, *mut u8, *mut u8, u32);
type SetTransformFn = unsafe extern "thiscall" fn(*mut u8, u32, *const Matrix);
static ORIGINAL: OnceCell<MeshFn> = OnceCell::new();
static ORIGINAL_LIT: OnceCell<LitMeshFn> = OnceCell::new();
static ARROW_VTABLE: OnceCell<usize> = OnceCell::new();

unsafe extern "thiscall" fn draw(this: *mut u8, render: *mut u8, pass: *mut u8) {
    with_camera(this, render, pass, || {
        ORIGINAL.get().unwrap()(this, render, pass)
    });
}

unsafe extern "thiscall" fn draw_lit(
    this: *mut u8,
    render: *mut u8,
    pass: *mut u8,
    light_pass: *mut u8,
    flags: u32,
) {
    with_camera(this, render, pass, || {
        ORIGINAL_LIT.get().unwrap()(this, render, pass, light_pass, flags)
    });
}

unsafe fn with_camera(this: *mut u8, render: *mut u8, pass: *mut u8, draw: impl FnOnce()) {
    let primitive = std::ptr::read_unaligned(this.add(0x54).cast::<*mut u8>());
    let actor = std::ptr::read_unaligned(primitive.cast::<*mut usize>());
    if !actor.is_null() && *actor == *ARROW_VTABLE.get().unwrap() {
        let scene = std::ptr::read_unaligned(pass.add(4).cast::<usize>());
        let clean = crate::engine_hook::clean_scene(scene);
        static LAST: AtomicU64 = AtomicU64::new(0);
        let now = crate::engine_hook::now_ms();
        if now.saturating_sub(LAST.load(Ordering::Relaxed)) > 1000 {
            LAST.store(now, Ordering::Relaxed);
            log::info!("compass: scene={scene:#x} stabilized={}", clean.is_some());
        }
        if let Some((view, projection)) = clean {
            let saved_view = std::ptr::read_unaligned(render.add(0x2c0).cast::<Matrix>());
            let saved_projection = std::ptr::read_unaligned(render.add(0x300).cast::<Matrix>());
            let vtable = *render.cast::<*const usize>();
            let set_transform: SetTransformFn = std::mem::transmute(*vtable.add(0xa4 / 4));
            // Use the renderer's setters so its matrix and camera-position caches
            // stay consistent during this draw and after restoring tracking.
            set_transform(render, 1, &view);
            set_transform(render, 2, &projection);
            draw();
            set_transform(render, 1, &saved_view);
            set_transform(render, 2, &saved_projection);
            return;
        }
    }
    draw();
}

pub fn install(target: usize, lit_target: usize, vtable: usize) -> Result<(), String> {
    ARROW_VTABLE
        .set(vtable)
        .map_err(|_| "Compass hook already installed".to_string())?;
    unsafe {
        crate::hook_util::install_hook(
            target as *mut c_void,
            draw as *mut c_void,
            &ORIGINAL,
            "compass mesh rendering",
        )?;
        crate::hook_util::install_hook(
            lit_target as *mut c_void,
            draw_lit as *mut c_void,
            &ORIGINAL_LIT,
            "compass lighting rendering",
        )
    }
}
