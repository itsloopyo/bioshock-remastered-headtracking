use std::ffi::c_void;
use std::mem;
use std::sync::atomic::{AtomicBool, Ordering};

use once_cell::sync::OnceCell;

use crate::hook_util::install_hook;
use windows::core::Interface;
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

/// D3D11 vtable function pointer types.
type PresentFn =
    unsafe extern "system" fn(this: *mut c_void, sync_interval: u32, flags: u32) -> i32;
type DrawFn =
    unsafe extern "system" fn(this: *mut c_void, vertex_count: u32, start_vertex_location: u32);

static ORIGINAL_PRESENT: OnceCell<PresentFn> = OnceCell::new();
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

        // Vtable index 13 on ID3D11DeviceContext = Draw.
        let ctx_ptr = windows::core::Interface::as_raw(&context);
        let ctx_vtable = *(ctx_ptr as *const *const *const c_void);
        let draw = *ctx_vtable.add(13) as *mut c_void;

        Ok(VtableAddrs { present, draw })
    }
}

// =========================================================================
// Hook detours
// =========================================================================

unsafe extern "system" fn hooked_present(this: *mut c_void, sync_interval: u32, flags: u32) -> i32 {
    // First-frame chore: center the game window on its monitor if it
    // launched in the top-left of an ultrawide. No-op after the first
    // call, so users can still drag the window afterwards.
    if let Some(sc) = IDXGISwapChain::from_raw_borrowed(&this) {
        if let Ok(desc) = sc.GetDesc() {
            crate::window::center_once(desc.OutputWindow);
        }
    }

    if let Some(&orig) = ORIGINAL_PRESENT.get() {
        orig(this, sync_interval, flags)
    } else {
        0
    }
}

unsafe extern "system" fn hooked_draw(
    this: *mut c_void,
    vertex_count: u32,
    start_vertex_location: u32,
) {
    let original = *ORIGINAL_DRAW.get().unwrap();
    if super::reticle::is_current_draw() {
        match crate::engine_hook::reticle_state() {
            crate::engine_hook::ReticleState::Position(offset) => {
                super::reticle::draw(this, vertex_count, start_vertex_location, original, offset);
                return;
            }
            crate::engine_hook::ReticleState::Hidden => return,
            crate::engine_hook::ReticleState::Inactive => {}
        }
    }
    original(this, vertex_count, start_vertex_location);
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
            addrs.draw,
            hooked_draw as *mut c_void,
            &ORIGINAL_DRAW,
            "ID3D11DeviceContext::Draw",
        )?;
    }
    HOOKED.store(true, Ordering::Release);
    Ok(())
}
