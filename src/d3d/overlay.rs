//! Minimal D3D11 overlay for drawing our own reticle at the aim point.
//!
//! Attaches to the game's existing swap chain (via the Present hook in
//! sibling module `super::hud`) and draws a small `+` shape each frame at the screen
//! position where mouse-forward projects into the head-tracked view.
//!
//! Pipeline is intentionally tiny:
//!   - HLSL vertex + pixel shaders, compiled once at runtime with
//!     `D3DCompile`.
//!   - No vertex / index buffer - the VS emits four corners via
//!     `SV_VertexID`.
//!   - One constant buffer with `(rect_ndc, color)`.
//!   - Triangle strip (4 verts = 2 triangles = a quad).
//!   - Alpha-blend state, depth disabled, no culling.
//!   - Drawn from the Present hook *before* calling the game's
//!     original Present, so our quad lands on top of the final frame.
//!
//! State restore is skipped: Present is the last thing that runs per
//! frame, and the game rebinds everything at the start of the next
//! frame.

use std::ffi::c_void;
use std::mem;
use std::sync::atomic::{AtomicBool, Ordering};

use once_cell::sync::OnceCell;
use windows::core::{Interface, PCSTR};
use windows::Win32::Graphics::Direct3D::Fxc::D3DCompile;
use windows::Win32::Graphics::Direct3D::{ID3DBlob, D3D_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP};
use windows::Win32::Graphics::Direct3D11::{
    ID3D11BlendState, ID3D11Buffer, ID3D11DepthStencilState, ID3D11Device, ID3D11DeviceContext,
    ID3D11PixelShader, ID3D11RasterizerState, ID3D11VertexShader, D3D11_BIND_CONSTANT_BUFFER,
    D3D11_BLEND_DESC, D3D11_BLEND_INV_SRC_ALPHA, D3D11_BLEND_ONE, D3D11_BLEND_OP_ADD,
    D3D11_BLEND_SRC_ALPHA, D3D11_BUFFER_DESC, D3D11_COLOR_WRITE_ENABLE_ALL, D3D11_CPU_ACCESS_WRITE,
    D3D11_CULL_NONE, D3D11_DEPTH_STENCIL_DESC, D3D11_FILL_SOLID, D3D11_MAPPED_SUBRESOURCE,
    D3D11_MAP_WRITE_DISCARD, D3D11_RASTERIZER_DESC, D3D11_RENDER_TARGET_BLEND_DESC,
    D3D11_USAGE_DYNAMIC,
};
use windows::Win32::Graphics::Dxgi::IDXGISwapChain;

/// Runtime state of the overlay. Lives in a `OnceCell` - populated on
/// the first `Present` call we see, used on every subsequent one.
struct Overlay {
    context: ID3D11DeviceContext,
    vs: ID3D11VertexShader,
    ps: ID3D11PixelShader,
    cb: ID3D11Buffer,
    blend: ID3D11BlendState,
    depth: ID3D11DepthStencilState,
    raster: ID3D11RasterizerState,
}

// COM pointers aren't Send/Sync by default; we only touch them from
// the render thread (inside the Present hook), so we assert it.
unsafe impl Send for Overlay {}
unsafe impl Sync for Overlay {}

static OVERLAY: OnceCell<Overlay> = OnceCell::new();
static INIT_FAILED: AtomicBool = AtomicBool::new(false);

/// Hardcoded fallback horizontal FOV used only before the engine FOV
/// read is online (typically the first one or two frames of gameplay)
/// and when the user has set no INI override. Matches the BioShock
/// Remastered stock slider value.
const FALLBACK_FOV_H_DEG: f64 = 100.0;

/// Width of the overlay reticle quad in NDC. Roughly 32 px on a
/// 1920x1080 buffer; the height tracks via the active aspect ratio so
/// the dot stays circular at any resolution.
const RETICLE_QUAD_NDC_W: f32 = 0.035;

/// Compute the (fov_h, fov_v) the overlay should project with this
/// frame, in degrees. Source of truth, in priority order:
///   1. INI override `[overlay] fov_h`.
///   2. PlayerController `DefaultFOV` (read live).
///   3. Hardcoded 100°/67.7° fallback (only used pre-PCV-capture).
///
/// `fov_v` is derived from `fov_h` via 16:9 aspect.
fn current_fov_deg() -> (f32, f32) {
    const NATIVE_ASPECT: f64 = 16.0 / 9.0;

    let fov_h = crate::config::fov_h_override()
        .or_else(crate::engine_hook::read_game_fov_h_native)
        .unwrap_or(FALLBACK_FOV_H_DEG as f32);

    let h_rad = (fov_h as f64).to_radians();
    let v_rad = 2.0 * ((h_rad * 0.5).tan() / NATIVE_ASPECT).atan();
    (fov_h, v_rad.to_degrees() as f32)
}

/// "Gameplay is live" gate. Draws the overlay if either the compass
/// (`DrawIndexed(idx=234)`) or the health bar (`Draw(vtx=11)`) fired
/// recently. The compass alone is the cleanest gameplay-only signal
/// but it disappears whenever there's no active objective; health
/// covers that gap. Together they fire every gameplay frame and
/// (essentially) never on the main menu / loading screens.
const GAMEPLAY_STALE_MS: u64 = 250;

fn gameplay_is_live() -> bool {
    let now = crate::engine_hook::now_ms();
    let fresh = |last: u64| last != 0 && now.saturating_sub(last) < GAMEPLAY_STALE_MS;
    fresh(super::hud::LAST_HUD_COMPASS_MS.load(Ordering::Relaxed))
        || fresh(super::hud::LAST_HUD_HEALTH_MS.load(Ordering::Relaxed))
}

/// HLSL for both shaders. Compiled at runtime with `D3DCompile`.
///
/// The constant buffer has two `float4`s:
///   `rect.xy` = top-left corner of the quad in NDC (-1..+1 range).
///   `rect.zw` = width/height of the quad in NDC.
///   `color`   = RGBA multiplied into the reticle colour.
///
/// The pixel shader draws a `+` shape by testing whether the UV is
/// inside either the horizontal or vertical arm of a cross, and
/// `discard`s otherwise.
const VS_SRC: &str = r#"
cbuffer Params : register(b0) {
    float4 rect;
    float4 color;
};
struct VSOut {
    float4 pos : SV_POSITION;
    float2 uv  : TEXCOORD0;
};
VSOut VSMain(uint vid : SV_VertexID) {
    float2 c;
    if      (vid == 0) c = float2(0.0, 0.0);
    else if (vid == 1) c = float2(1.0, 0.0);
    else if (vid == 2) c = float2(0.0, 1.0);
    else               c = float2(1.0, 1.0);
    float2 p = rect.xy + c * rect.zw;
    VSOut o;
    o.pos = float4(p.x, p.y, 0.0, 1.0);
    o.uv  = c;
    return o;
}
"#;

const PS_SRC: &str = r#"
cbuffer Params : register(b0) {
    float4 rect;
    float4 color;
};
struct PSIn {
    float4 pos : SV_POSITION;
    float2 uv  : TEXCOORD0;
};
float4 PSMain(PSIn input) : SV_TARGET {
    float2 c = input.uv - 0.5;
    float r = length(c);
    if (r > 0.10) discard;
    return color;
}
"#;

/// Constant-buffer layout must match the HLSL above. `#[repr(C)]` +
/// 16-byte alignment via explicit padding to keep Rust and HLSL in
/// sync.
#[repr(C)]
#[derive(Clone, Copy)]
struct OverlayCb {
    rect: [f32; 4],
    color: [f32; 4],
}

fn compile_shader(src: &str, entry: &str, target: &str) -> Option<ID3DBlob> {
    let mut blob: Option<ID3DBlob> = None;
    let mut err_blob: Option<ID3DBlob> = None;
    let entry_c = std::ffi::CString::new(entry).unwrap();
    let target_c = std::ffi::CString::new(target).unwrap();
    let result = unsafe {
        D3DCompile(
            src.as_ptr() as *const c_void,
            src.len(),
            PCSTR::null(),
            None,
            None,
            PCSTR(entry_c.as_ptr() as *const u8),
            PCSTR(target_c.as_ptr() as *const u8),
            0,
            0,
            &mut blob,
            Some(&mut err_blob),
        )
    };
    if result.is_err() {
        if let Some(b) = err_blob {
            unsafe {
                let ptr = b.GetBufferPointer() as *const u8;
                let len = b.GetBufferSize();
                let msg = std::slice::from_raw_parts(ptr, len);
                log::error!(
                    "D3DCompile ({} {}) failed: {}",
                    entry,
                    target,
                    String::from_utf8_lossy(msg).trim_end()
                );
            }
        } else {
            log::error!("D3DCompile ({} {}) failed: {:?}", entry, target, result);
        }
        return None;
    }
    blob
}

/// Called from the Present hook with the live swap chain. Lazily
/// initialises all D3D11 objects we need. Returns `true` if the
/// overlay is ready to draw, `false` if init failed (in which case
/// `INIT_FAILED` is latched and we stop retrying).
unsafe fn ensure_init(swap_chain_ptr: *mut c_void) -> bool {
    if OVERLAY.get().is_some() {
        return true;
    }
    if INIT_FAILED.load(Ordering::Relaxed) {
        return false;
    }

    // Treat the swap-chain pointer as an IDXGISwapChain; use it to
    // fish out the ID3D11Device. `Interface::from_raw_borrowed` lets
    // us do the COM AddRef/Release dance without taking ownership.
    let sc = match IDXGISwapChain::from_raw_borrowed(&swap_chain_ptr) {
        Some(sc) => sc,
        None => {
            log::error!("overlay: from_raw_borrowed(IDXGISwapChain) returned None");
            INIT_FAILED.store(true, Ordering::Relaxed);
            return false;
        }
    };

    let device: ID3D11Device = match sc.GetDevice() {
        Ok(d) => d,
        Err(e) => {
            log::error!("overlay: IDXGISwapChain::GetDevice failed: {:?}", e);
            INIT_FAILED.store(true, Ordering::Relaxed);
            return false;
        }
    };

    let context: ID3D11DeviceContext = match device.GetImmediateContext() {
        Ok(c) => c,
        Err(e) => {
            log::error!("overlay: GetImmediateContext: {:?}", e);
            INIT_FAILED.store(true, Ordering::Relaxed);
            return false;
        }
    };

    let vs_blob = match compile_shader(VS_SRC, "VSMain", "vs_5_0") {
        Some(b) => b,
        None => {
            INIT_FAILED.store(true, Ordering::Relaxed);
            return false;
        }
    };
    let ps_blob = match compile_shader(PS_SRC, "PSMain", "ps_5_0") {
        Some(b) => b,
        None => {
            INIT_FAILED.store(true, Ordering::Relaxed);
            return false;
        }
    };

    let vs_bytes = std::slice::from_raw_parts(
        vs_blob.GetBufferPointer() as *const u8,
        vs_blob.GetBufferSize(),
    );
    let ps_bytes = std::slice::from_raw_parts(
        ps_blob.GetBufferPointer() as *const u8,
        ps_blob.GetBufferSize(),
    );

    let mut vs: Option<ID3D11VertexShader> = None;
    if let Err(e) = device.CreateVertexShader(vs_bytes, None, Some(&mut vs)) {
        log::error!("overlay: CreateVertexShader: {:?}", e);
        INIT_FAILED.store(true, Ordering::Relaxed);
        return false;
    }
    let vs = vs.unwrap();

    let mut ps: Option<ID3D11PixelShader> = None;
    if let Err(e) = device.CreatePixelShader(ps_bytes, None, Some(&mut ps)) {
        log::error!("overlay: CreatePixelShader: {:?}", e);
        INIT_FAILED.store(true, Ordering::Relaxed);
        return false;
    }
    let ps = ps.unwrap();

    // Constant buffer. Dynamic usage so we can Map/Unmap each frame.
    let cb_desc = D3D11_BUFFER_DESC {
        ByteWidth: mem::size_of::<OverlayCb>() as u32,
        Usage: D3D11_USAGE_DYNAMIC,
        BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
        CPUAccessFlags: D3D11_CPU_ACCESS_WRITE.0 as u32,
        MiscFlags: 0,
        StructureByteStride: 0,
    };
    let mut cb: Option<ID3D11Buffer> = None;
    if let Err(e) = device.CreateBuffer(&cb_desc, None, Some(&mut cb)) {
        log::error!("overlay: CreateBuffer(CB): {:?}", e);
        INIT_FAILED.store(true, Ordering::Relaxed);
        return false;
    }
    let cb = cb.unwrap();

    // Standard alpha-blend state.
    let mut blend_desc = D3D11_BLEND_DESC::default();
    blend_desc.RenderTarget[0] = D3D11_RENDER_TARGET_BLEND_DESC {
        BlendEnable: true.into(),
        SrcBlend: D3D11_BLEND_SRC_ALPHA,
        DestBlend: D3D11_BLEND_INV_SRC_ALPHA,
        BlendOp: D3D11_BLEND_OP_ADD,
        SrcBlendAlpha: D3D11_BLEND_ONE,
        DestBlendAlpha: D3D11_BLEND_INV_SRC_ALPHA,
        BlendOpAlpha: D3D11_BLEND_OP_ADD,
        RenderTargetWriteMask: D3D11_COLOR_WRITE_ENABLE_ALL.0 as u8,
    };
    let mut blend: Option<ID3D11BlendState> = None;
    if let Err(e) = device.CreateBlendState(&blend_desc, Some(&mut blend)) {
        log::error!("overlay: CreateBlendState: {:?}", e);
        INIT_FAILED.store(true, Ordering::Relaxed);
        return false;
    }
    let blend = blend.unwrap();

    // Depth / stencil disabled entirely.
    let depth_desc = D3D11_DEPTH_STENCIL_DESC {
        DepthEnable: false.into(),
        StencilEnable: false.into(),
        ..Default::default()
    };
    let mut depth: Option<ID3D11DepthStencilState> = None;
    if let Err(e) = device.CreateDepthStencilState(&depth_desc, Some(&mut depth)) {
        log::error!("overlay: CreateDepthStencilState: {:?}", e);
        INIT_FAILED.store(true, Ordering::Relaxed);
        return false;
    }
    let depth = depth.unwrap();

    // No culling, solid fill.
    let raster_desc = D3D11_RASTERIZER_DESC {
        FillMode: D3D11_FILL_SOLID,
        CullMode: D3D11_CULL_NONE,
        FrontCounterClockwise: false.into(),
        DepthBias: 0,
        DepthBiasClamp: 0.0,
        SlopeScaledDepthBias: 0.0,
        DepthClipEnable: false.into(),
        ScissorEnable: false.into(),
        MultisampleEnable: false.into(),
        AntialiasedLineEnable: false.into(),
    };
    let mut raster: Option<ID3D11RasterizerState> = None;
    if let Err(e) = device.CreateRasterizerState(&raster_desc, Some(&mut raster)) {
        log::error!("overlay: CreateRasterizerState: {:?}", e);
        INIT_FAILED.store(true, Ordering::Relaxed);
        return false;
    }
    let raster = raster.unwrap();

    let overlay = Overlay {
        context,
        vs,
        ps,
        cb,
        blend,
        depth,
        raster,
    };
    if OVERLAY.set(overlay).is_err() {
        log::error!("overlay: OVERLAY.set - already initialised?");
        return false;
    }
    log::info!("overlay: D3D11 overlay initialised");
    true
}

/// Write a new `OverlayCb` into the constant buffer via `Map`.
unsafe fn update_cb(ctx: &ID3D11DeviceContext, cb: &ID3D11Buffer, data: OverlayCb) {
    let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
    if ctx
        .Map(cb, 0, D3D11_MAP_WRITE_DISCARD, 0, Some(&mut mapped))
        .is_err()
    {
        return;
    }
    std::ptr::copy_nonoverlapping(
        &data as *const OverlayCb as *const u8,
        mapped.pData as *mut u8,
        mem::size_of::<OverlayCb>(),
    );
    ctx.Unmap(cb, 0);
}

/// Draw the overlay reticle. Called by the Present hook *before* the
/// game's original Present, passing the swap chain the game is using.
/// First call lazily initialises; later calls update the constant
/// buffer and draw.
///
/// `yaw_deg` / `pitch_deg` are the current recentered head rotation
/// from OpenTrack. The reticle is placed at the screen position where
/// mouse-forward (unchanged by head motion) projects into the
/// head-rotated view.
pub fn draw(swap_chain_ptr: *mut c_void, yaw_deg: f64, pitch_deg: f64, roll_deg: f64) {
    if swap_chain_ptr.is_null() {
        return;
    }
    // Only draw during active gameplay - skip menus / loading / pause.
    if !gameplay_is_live() {
        return;
    }
    unsafe {
        if !ensure_init(swap_chain_ptr) {
            return;
        }
        let ov = match OVERLAY.get() {
            Some(o) => o,
            None => return,
        };

        // Per-axis tangent projection. The earlier spherical-decomp
        // formula (ported from subnautica) caused a "U-on-yaw" where
        // the reticle dipped vertically as the user yawed left/right
        // - the cross-coupling factors (`cos(yaw)` on `ay`/`az`)
        // assume an intrinsic-YPR rotation chain, but BioShock's
        // FRotator → view matrix doesn't compose the same way, so
        // the math overcorrects and demands a wildly inflated `fov_h`
        // (~117° vs the engine's actual ~96°) to cancel the resulting
        // horizontal drift.
        //
        // The per-axis form has no yaw↔pitch coupling at all:
        //   ndc_x depends only on yaw and fov_h
        //   ndc_y depends only on pitch and fov_v
        // It loses pure roll-around-view-axis correction, so we add a
        // separate screen-space roll rotation afterwards. The slight
        // FOV/aspect distortion this introduces (per the global
        // CLAUDE.md note) is dwarfed by the U-on-yaw error we'd see
        // otherwise.
        let (fov_h_deg, fov_v_deg) = current_fov_deg();
        let fov_h = (fov_h_deg as f64).to_radians();
        let fov_v = (fov_v_deg as f64).to_radians();
        let aspect = 16.0_f64 / 9.0;

        let yaw_rad = yaw_deg.to_radians();
        let pitch_rad = pitch_deg.to_radians();

        // Full-spherical projection. We need the gun-aim direction
        // (= clean camera forward in world) expressed in the
        // head-rotated camera's local frame, then perspective-
        // divided. Treating the clean rotation as zero (as the older
        // per-axis form did) makes the reticle slide flat horizontally
        // when you yaw - but the *real* aim point traces a U/arc on
        // screen because of spherical geometry whenever the mouse
        // is pitched off-level. That's the "MASSIVE semicircle" bug.
        //
        // Setup:
        //   P_c, Y_c = clean (mouse-driven) pitch/yaw, in radians.
        //   P_h, Y_h = head-tracking deltas, in radians.
        //   P_t = P_c + P_h, Y_t = Y_c + Y_h (component sums match
        //         how engine_hook stitches them into the FRotator).
        //
        // Building rendered & clean orthonormal bases in UE local
        // axes (X = forward, Y = right, Z = up) and projecting
        // clean's forward into rendered's basis gives:
        //   aim.fwd   =  cos(P_t)cos(P_c)cos(Y_h) + sin(P_t)sin(P_c)
        //   aim.right = -cos(P_c) sin(Y_h)
        //   aim.up    = -sin(P_t)cos(P_c)cos(Y_h) + cos(P_t)sin(P_c)
        // (Y_h = Y_t - Y_c falls out by sum-of-angles.)
        //
        // Yaw sign is `+Y_h` (matches how engine_hook adds the head
        // delta into the FRotator); flipping to `-Y_h` inverted the
        // reticle's horizontal direction in the spherical-projection
        // form, even though the per-axis form needed the negative
        // sign. The two formulas differ in how they parametrise the
        // rotation, so the empirical sign is opposite.
        let p_c = (crate::engine_hook::units_to_deg(
            crate::engine_hook::CLEAN_PITCH_UNITS.load(Ordering::Relaxed),
        ))
        .to_radians();
        let p_h = pitch_rad;
        let p_t = p_c + p_h;
        let y_h = yaw_rad;

        let cos_pt = p_t.cos();
        let sin_pt = p_t.sin();
        let cos_pc = p_c.cos();
        let sin_pc = p_c.sin();
        let cos_yh = y_h.cos();
        let sin_yh = y_h.sin();

        let aim_fwd_dir = cos_pt * cos_pc * cos_yh + sin_pt * sin_pc;
        let aim_right_dir = -cos_pc * sin_yh;
        let aim_up_dir = -sin_pt * cos_pc * cos_yh + cos_pt * sin_pc;

        // Parallax compensation for 6DOF position tracking.
        //
        // Without this, the reticle marks the gun's aim DIRECTION,
        // not the bullet HIT POINT. With positional tracking on, the
        // rendered camera is translated by the head offset, so wall
        // features shift on screen due to parallax - but the
        // direction-based reticle stays put. Bullets land where the
        // reticle WOULD be if 3DOF only; the visible reticle drifts.
        //
        // Fix (lifted from RE:Requiem
        // resident-evil-requiem/src/camera/camera_hook.cpp:380):
        // build the world aim point at a fixed distance from the
        // un-translated (clean) camera, then form the vector from
        // the head-translated camera to that point. Project that
        // vector through the head-rotated basis. Position offset
        // automatically reduces the reticle's apparent angular
        // position, glueing it to the bullet hit point.
        //
        // KAimDist = 500cm (5m). RE:Requiem uses ~50 (their units)
        // because the engine renders much larger spaces; BSR's
        // gameplay distances are mostly 3–10m indoor, so 5m gives
        // perceptible parallax shift on a typical 5–20cm lean
        // (~10–40 px on a 1080p screen at fov_h=100°). Without an
        // actual raycast for the live aim distance, this is a
        // best-fit constant.
        const K_AIM_DIST_CM: f64 = 500.0;
        let (head_right, head_up, head_fwd) = crate::tracking::applied_head_offset();
        let aim_fwd = K_AIM_DIST_CM * aim_fwd_dir - head_fwd;
        let aim_right = K_AIM_DIST_CM * aim_right_dir - head_right;
        let aim_up = K_AIM_DIST_CM * aim_up_dir - head_up;

        let mut ndc_x = (aim_right / aim_fwd / (fov_h * 0.5).tan()) as f32;
        let mut ndc_y = (aim_up / aim_fwd / (fov_v * 0.5).tan()) as f32;

        // Screen-space roll, applied AFTER the spherical projection.
        // Re-derived from the UE2.5 basis-with-roll formulation:
        // for a rendered roll R_t around the camera-forward axis,
        // the rolled NDC position is the no-roll NDC rotated by
        // **-R_t** in aspect-corrected screen space (NOT +R_t - the
        // `right_with_roll` basis vector is `cos R · right − sin R · up`
        // in UE convention, which flips the cross-term sign).
        //
        // engine_hook writes `-OpenTrack_roll` to the FRotator, so
        // R_t = -roll_deg (with clean roll ≈ 0). Therefore the
        // rotation angle to apply here = -R_t = +roll_deg. But the
        // cross-term sign flip from the basis derivation means we
        // negate that, ending up at -roll_deg. ROLL_SIGN absorbs
        // that net negation; flip to +1.0 if the roll direction
        // still ends up wrong in-game.
        const ROLL_SIGN: f32 = -1.0;
        let r = ROLL_SIGN * (roll_deg as f32) * std::f32::consts::PI / 180.0;
        let cr = r.cos();
        let sr = r.sin();
        let aspect_f = aspect as f32;
        let sx = ndc_x * aspect_f;
        let sy = ndc_y;
        let sx_r = sx * cr - sy * sr;
        let sy_r = sx * sr + sy * cr;
        ndc_x = sx_r / aspect_f;
        ndc_y = sy_r;

        let quad_w_ndc = RETICLE_QUAD_NDC_W;
        let quad_h_ndc = quad_w_ndc * aspect as f32;

        let rect = [
            ndc_x - quad_w_ndc * 0.5,
            ndc_y - quad_h_ndc * 0.5,
            quad_w_ndc,
            quad_h_ndc,
        ];
        // Light cream - soft against BSR's mostly-warm palette without
        // washing out into pure white.
        let color = [0.98_f32, 0.96, 0.88, 0.95];
        update_cb(&ov.context, &ov.cb, OverlayCb { rect, color });

        // Bind pipeline.
        ov.context.IASetInputLayout(None);
        ov.context
            .IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP);
        ov.context.VSSetShader(&ov.vs, None);
        ov.context.PSSetShader(&ov.ps, None);
        ov.context
            .VSSetConstantBuffers(0, Some(&[Some(ov.cb.clone())]));
        ov.context
            .PSSetConstantBuffers(0, Some(&[Some(ov.cb.clone())]));
        ov.context
            .OMSetBlendState(&ov.blend, Some(&[1.0, 1.0, 1.0, 1.0]), 0xFFFFFFFF);
        ov.context.OMSetDepthStencilState(&ov.depth, 0);
        ov.context.RSSetState(&ov.raster);

        ov.context.Draw(4, 0);
    }
}
