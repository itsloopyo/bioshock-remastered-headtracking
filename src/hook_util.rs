//! Shared MinHook install helper.
//!
//! Every detour we install follows the same dance:
//!
//! 1. Idempotency guard - if the `OnceCell` already holds the original,
//!    do nothing (subsequent install calls are no-ops by design).
//! 2. `MinHook::create_hook(target, detour)` returns a trampoline
//!    pointer.
//! 3. Transmute the trampoline into the per-hook function-pointer
//!    type and stash it in the `OnceCell` so the detour can chain
//!    through to the game's original implementation.
//! 4. `MinHook::enable_all_hooks` flips the JMP. This is global and
//!    idempotent; calling it after each install costs nothing once the
//!    set is fully enabled.
//! 5. Log a one-line install banner.
//!
//! Keeping this in one place removes ~10 lines of error-mapping
//! boilerplate per hook site (8 sites across `engine_hook` and
//! `d3d::hud`).

use std::ffi::c_void;

use minhook::MinHook;
use once_cell::sync::OnceCell;

/// Install a single MinHook detour, store the trampoline as the
/// per-hook "original" function pointer, and enable.
///
/// `F` is the function-pointer type the detour calls through to (e.g.
/// `unsafe extern "thiscall" fn(*mut Foo) -> u32`). MinHook always
/// returns the trampoline as `*mut c_void`; we `transmute_copy` it
/// into `F` because `transmute` would refuse the size-unknown generic.
/// Soundness: on every supported architecture (currently i686-only),
/// function pointers and `*mut c_void` are the same size.
///
/// `label` is used for log messages and error context - it should
/// uniquely identify the hook so the install banner reads linearly
/// (e.g. `"eventPlayerCalcView"`, `"IDXGISwapChain::Present"`).
///
/// # Safety
///
/// `target` must point to an executable function whose calling
/// convention and signature match `detour` and `F`. `detour` must
/// remain valid for the process lifetime.
pub(crate) unsafe fn install_hook<F: Copy>(
    target: *mut c_void,
    detour: *mut c_void,
    slot: &OnceCell<F>,
    label: &str,
) -> Result<(), String> {
    if slot.get().is_some() {
        return Ok(());
    }
    let trampoline = MinHook::create_hook(target, detour)
        .map_err(|e| format!("MinHook::create_hook ({}): {:?}", label, e))?;
    let original_fn: F = std::mem::transmute_copy(&trampoline);
    slot.set(original_fn)
        .map_err(|_| format!("{}: original already set", label))?;
    MinHook::enable_all_hooks()
        .map_err(|e| format!("MinHook::enable_all_hooks ({}): {:?}", label, e))?;
    log::info!("{} hook installed at 0x{:p}", label, target);
    Ok(())
}
