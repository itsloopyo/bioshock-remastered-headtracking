//! D3D11 HUD interception.
//!
//! Hooks `IDXGISwapChain::Present` (to draw the overlay reticle on top
//! of the final frame) and the two `ID3D11DeviceContext` draw APIs that
//! BioShock Remastered's Scaleform HUD uses:
//!
//! - `DrawIndexed(idx=234)` is the compass / quest needle - used as a
//!   "HUD render is happening this frame" marker.
//! - `Draw(vtx=11)` is the health / EVE bar - same role; together with
//!   the compass it covers every gameplay frame.
//! - `Draw(vtx=9)` is the gun reticle, `Draw(vtx=21)` is the plasmid
//!   reticle - we drop these whenever head tracking is enabled, but
//!   only after the per-frame HUD-active flag has been raised so we
//!   don't clip world particles that happen to use the same vertex
//!   counts.

use std::ffi::c_void;
use std::mem;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use once_cell::sync::OnceCell;

use crate::hook_util::install_hook;
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDeviceAndSwapChain, ID3D11Device, ID3D11DeviceContext, D3D11_CREATE_DEVICE_FLAG,
    D3D11_SDK_VERSION,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_MODE_DESC, DXGI_MODE_SCALING_UNSPECIFIED,
    DXGI_MODE_SCANLINE_ORDER_UNSPECIFIED, DXGI_RATIONAL, DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Dxgi::{
    IDXGISwapChain, DXGI_SWAP_CHAIN_DESC, DXGI_SWAP_EFFECT_DISCARD, DXGI_USAGE_RENDER_TARGET_OUTPUT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, RegisterClassW, CS_HREDRAW, CS_VREDRAW,
    WINDOW_EX_STYLE, WNDCLASSW, WS_OVERLAPPEDWINDOW,
};

/// Per-frame "HUD render is happening RIGHT NOW" flag. Set whenever a
/// known HUD-only draw fires (compass `idx=234` or health `vtx=11`).
/// Cleared at the next `Present`. Gates reticle suppression so we only
/// drop `vtx=9 / 21` draws inside the HUD render window, never world
/// particles that happen to use the same vertex counts.
pub static HUD_ACTIVE_THIS_FRAME: AtomicBool = AtomicBool::new(false);

/// Vertex counts for the gun (`9`) and plasmid (`21`) reticles, dropped
/// during HUD render whenever head tracking is enabled.
const CROSSHAIR_VERTEX_COUNTS: &[u32] = &[9, 21];

/// Timestamps (ms-since-start via `engine_hook::now_ms`) of the most
/// recent compass / health draws. The overlay's `gameplay_is_live()`
/// gate reads these to decide whether to draw the reticle.
pub static LAST_HUD_COMPASS_MS: AtomicU64 = AtomicU64::new(0);
pub static LAST_HUD_HEALTH_MS: AtomicU64 = AtomicU64::new(0);

/// D3D11 vtable function pointer types.
type PresentFn =
    unsafe extern "system" fn(this: *mut c_void, sync_interval: u32, flags: u32) -> i32;
type DrawIndexedFn = unsafe extern "system" fn(
    this: *mut c_void,
    index_count: u32,
    start_index_location: u32,
    base_vertex_location: i32,
);
type DrawFn =
    unsafe extern "system" fn(this: *mut c_void, vertex_count: u32, start_vertex_location: u32);

static ORIGINAL_PRESENT: OnceCell<PresentFn> = OnceCell::new();
static ORIGINAL_DRAW_INDEXED: OnceCell<DrawIndexedFn> = OnceCell::new();
static ORIGINAL_DRAW: OnceCell<DrawFn> = OnceCell::new();
static HOOKED: AtomicBool = AtomicBool::new(false);

// =========================================================================
// Temp window + D3D11 init - throwaway device/swapchain just to read
// the vtable pointers, then everything is released.
// =========================================================================

unsafe extern "system" fn temp_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: windows::Win32::Foundation::WPARAM,
    lparam: windows::Win32::Foundation::LPARAM,
) -> windows::Win32::Foundation::LRESULT {
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

fn create_temp_window() -> Result<HWND, &'static str> {
    unsafe {
        let hmodule = GetModuleHandleW(None).map_err(|_| "GetModuleHandleW")?;
        let hinstance: windows::Win32::Foundation::HINSTANCE = mem::transmute(hmodule);
        let class_name = windows::core::w!("BsrHtTempD3D11Window");
        let wc = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(temp_wnd_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: hinstance,
            hIcon: Default::default(),
            hCursor: Default::default(),
            hbrBackground: Default::default(),
            lpszMenuName: windows::core::PCWSTR::null(),
            lpszClassName: class_name,
        };
        let _ = RegisterClassW(&wc);
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            class_name,
            windows::core::w!("BsrHt"),
            WS_OVERLAPPEDWINDOW,
            0,
            0,
            100,
            100,
            None,
            None,
            hinstance,
            None,
        )
        .map_err(|_| "CreateWindowExW")?;
        if hwnd.0.is_null() {
            return Err("CreateWindowExW returned null");
        }
        Ok(hwnd)
    }
}

struct VtableAddrs {
    present: *mut c_void,
    draw_indexed: *mut c_void,
    draw: *mut c_void,
}

fn get_vtable_addrs() -> Result<VtableAddrs, &'static str> {
    unsafe {
        let hwnd = create_temp_window()?;
        let desc = DXGI_SWAP_CHAIN_DESC {
            BufferDesc: DXGI_MODE_DESC {
                Width: 100,
                Height: 100,
                RefreshRate: DXGI_RATIONAL {
                    Numerator: 60,
                    Denominator: 1,
                },
                Format: DXGI_FORMAT_R8G8B8A8_UNORM,
                ScanlineOrdering: DXGI_MODE_SCANLINE_ORDER_UNSPECIFIED,
                Scaling: DXGI_MODE_SCALING_UNSPECIFIED,
            },
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
            BufferCount: 1,
            OutputWindow: hwnd,
            Windowed: true.into(),
            SwapEffect: DXGI_SWAP_EFFECT_DISCARD,
            Flags: 0,
        };
        let mut swap_chain: Option<IDXGISwapChain> = None;
        let mut device: Option<ID3D11Device> = None;
        let mut context: Option<ID3D11DeviceContext> = None;
        let result = D3D11CreateDeviceAndSwapChain(
            None,
            D3D_DRIVER_TYPE_HARDWARE,
            None,
            D3D11_CREATE_DEVICE_FLAG(0),
            None,
            D3D11_SDK_VERSION,
            Some(&desc),
            Some(&mut swap_chain),
            Some(&mut device),
            None,
            Some(&mut context),
        );
        let _ = DestroyWindow(hwnd);
        if result.is_err() {
            return Err("D3D11CreateDeviceAndSwapChain failed");
        }
        let swap_chain = swap_chain.ok_or("SwapChain is None")?;
        let context = context.ok_or("DeviceContext is None")?;

        // Vtable index 8 on IDXGISwapChain = Present.
        let sc_ptr = windows::core::Interface::as_raw(&swap_chain);
        let sc_vtable = *(sc_ptr as *const *const *const c_void);
        let present = *sc_vtable.add(8) as *mut c_void;

        // Vtable indices on ID3D11DeviceContext: 12 = DrawIndexed, 13 = Draw.
        let ctx_ptr = windows::core::Interface::as_raw(&context);
        let ctx_vtable = *(ctx_ptr as *const *const *const c_void);
        let draw_indexed = *ctx_vtable.add(12) as *mut c_void;
        let draw = *ctx_vtable.add(13) as *mut c_void;

        Ok(VtableAddrs {
            present,
            draw_indexed,
            draw,
        })
    }
}

// =========================================================================
// Hook detours
// =========================================================================

unsafe extern "system" fn hooked_present(this: *mut c_void, sync_interval: u32, flags: u32) -> i32 {
    // Draw our reticle on top of the game's final frame, just before
    // the swap. Only does anything when head tracking is enabled.
    if crate::tracking::is_enabled_atomic() {
        let (yaw_deg, pitch_deg, roll_deg) = crate::tracking::get_recentered_rotation_atomic();
        super::overlay::draw(this, yaw_deg, pitch_deg, roll_deg);
    }

    // Clear the per-frame HUD-active flag at frame boundary. Compass /
    // health draws will raise it again next frame if HUD is still up.
    HUD_ACTIVE_THIS_FRAME.store(false, Ordering::Release);

    if let Some(&orig) = ORIGINAL_PRESENT.get() {
        orig(this, sync_interval, flags)
    } else {
        0
    }
}

unsafe extern "system" fn hooked_draw_indexed(
    this: *mut c_void,
    index_count: u32,
    start_index_location: u32,
    base_vertex_location: i32,
) {
    // Compass / quest needle - HUD render marker.
    if index_count == 234 {
        LAST_HUD_COMPASS_MS.store(crate::engine_hook::now_ms(), Ordering::Relaxed);
        HUD_ACTIVE_THIS_FRAME.store(true, Ordering::Release);
    }
    if let Some(&orig) = ORIGINAL_DRAW_INDEXED.get() {
        orig(
            this,
            index_count,
            start_index_location,
            base_vertex_location,
        );
    }
}

unsafe extern "system" fn hooked_draw(
    this: *mut c_void,
    vertex_count: u32,
    start_vertex_location: u32,
) {
    // Health / EVE bar - HUD render marker. Multiple draws per
    // gameplay frame.
    if vertex_count == 11 {
        LAST_HUD_HEALTH_MS.store(crate::engine_hook::now_ms(), Ordering::Relaxed);
        HUD_ACTIVE_THIS_FRAME.store(true, Ordering::Release);
    }

    // Reticle suppression - drop the gun and plasmid reticles
    // whenever head tracking is enabled, but only inside the HUD
    // render window so we don't clip world particles with the same
    // vertex counts. With tracking disabled the user gets vanilla
    // behaviour: game reticle visible, our overlay not drawn.
    let suppress = crate::tracking::is_enabled_atomic()
        && HUD_ACTIVE_THIS_FRAME.load(Ordering::Acquire)
        && CROSSHAIR_VERTEX_COUNTS.contains(&vertex_count);
    if suppress {
        return;
    }

    if let Some(&orig) = ORIGINAL_DRAW.get() {
        orig(this, vertex_count, start_vertex_location);
    }
}

// =========================================================================
// Public install
// =========================================================================

/// Install the D3D11 hooks. Idempotent; safe to call once at startup.
pub fn install() -> Result<(), String> {
    if HOOKED.load(Ordering::Relaxed) {
        return Ok(());
    }
    let addrs = get_vtable_addrs().map_err(|e| e.to_string())?;
    unsafe {
        install_hook(
            addrs.present,
            hooked_present as *mut c_void,
            &ORIGINAL_PRESENT,
            "IDXGISwapChain::Present",
        )?;
        install_hook(
            addrs.draw_indexed,
            hooked_draw_indexed as *mut c_void,
            &ORIGINAL_DRAW_INDEXED,
            "ID3D11DeviceContext::DrawIndexed",
        )?;
        install_hook(
            addrs.draw,
            hooked_draw as *mut c_void,
            &ORIGINAL_DRAW,
            "ID3D11DeviceContext::Draw",
        )?;
    }
    HOOKED.store(true, Ordering::Release);
    Ok(())
}
