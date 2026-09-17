use std::{cell::Cell, ffi::c_void};

use once_cell::sync::OnceCell;

use windows::core::Interface;
use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Direct3D11::{ID3D11DeviceContext, D3D11_VIEWPORT};

pub type DrawFn = unsafe extern "system" fn(*mut c_void, u32, u32);

type DrawPrimitiveFn =
    unsafe extern "thiscall" fn(*mut c_void, u32, u32, *const Vertex, u32) -> i32;

#[repr(C)]
struct Vertex {
    position: [f32; 2],
    uv: [f32; 2],
    color: [f32; 4],
    color_add: [f32; 4],
}

static ORIGINAL_PRIMITIVE: OnceCell<DrawPrimitiveFn> = OnceCell::new();
thread_local! {
    static RETICLE_DRAW: Cell<bool> = const { Cell::new(false) };
}

fn is_centered_shape(vertices: &[Vertex]) -> bool {
    let mut min = [f32::INFINITY; 2];
    let mut max = [f32::NEG_INFINITY; 2];
    for vertex in vertices {
        for axis in 0..2 {
            let coordinate = vertex.position[axis];
            if !coordinate.is_finite() || coordinate.abs() > 0.25 {
                return false;
            }
            min[axis] = min[axis].min(coordinate);
            max[axis] = max[axis].max(coordinate);
        }
    }
    min[0] < 0.0 && max[0] > 0.0 && min[1] < 0.0 && max[1] > 0.0
}

unsafe extern "thiscall" fn draw_primitive(
    this: *mut c_void,
    topology: u32,
    primitive_count: u32,
    vertices: *const Vertex,
    stride: u32,
) -> i32 {
    // The HUD shader passes these clip-space positions through unchanged.
    // Vertex counts alone also match the ammo ornament and other HUD shapes.
    let reticle = topology == 5
        && stride as usize == std::mem::size_of::<Vertex>()
        && is_centered_shape(std::slice::from_raw_parts(
            vertices,
            (primitive_count + 2) as usize,
        ));
    let previous = RETICLE_DRAW.replace(reticle);
    let result =
        ORIGINAL_PRIMITIVE.get().unwrap()(this, topology, primitive_count, vertices, stride);
    RETICLE_DRAW.set(previous);
    result
}

pub fn is_current_draw() -> bool {
    RETICLE_DRAW.get()
}

pub fn install(target: usize) -> Result<(), String> {
    unsafe {
        crate::hook_util::install_hook(
            target as *mut c_void,
            draw_primitive as *mut c_void,
            &ORIGINAL_PRIMITIVE,
            "HUD primitive submission",
        )
    }
}

pub unsafe fn draw(
    this: *mut c_void,
    vertex_count: u32,
    start_vertex: u32,
    original: DrawFn,
    offset: [f32; 2],
) {
    let context = ID3D11DeviceContext::from_raw_borrowed(&this).unwrap();
    let mut viewports = [D3D11_VIEWPORT::default(); 16];
    let mut viewport_count = viewports.len() as u32;
    context.RSGetViewports(&mut viewport_count, Some(viewports.as_mut_ptr()));
    let mut scissors = [RECT::default(); 16];
    let mut scissor_count = scissors.len() as u32;
    context.RSGetScissorRects(&mut scissor_count, Some(scissors.as_mut_ptr()));
    let mut shifted_viewports = viewports;
    let mut shifted_scissors = scissors;
    for (index, viewport) in shifted_viewports[..viewport_count as usize]
        .iter_mut()
        .enumerate()
    {
        let dx = offset[0] * viewport.Width * 0.5;
        let dy = -offset[1] * viewport.Height * 0.5;
        viewport.TopLeftX += dx;
        viewport.TopLeftY += dy;
        if index < scissor_count as usize {
            let rect = &mut shifted_scissors[index];
            rect.left += dx.round() as i32;
            rect.right += dx.round() as i32;
            rect.top += dy.round() as i32;
            rect.bottom += dy.round() as i32;
        }
    }
    context.RSSetViewports(Some(&shifted_viewports[..viewport_count as usize]));
    context.RSSetScissorRects(Some(&shifted_scissors[..scissor_count as usize]));
    original(this, vertex_count, start_vertex);
    context.RSSetViewports(Some(&viewports[..viewport_count as usize]));
    context.RSSetScissorRects(Some(&scissors[..scissor_count as usize]));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vertices(positions: &[[f32; 2]]) -> Vec<Vertex> {
        positions
            .iter()
            .map(|&position| Vertex {
                position,
                uv: [0.0; 2],
                color: [1.0; 4],
                color_add: [0.0; 4],
            })
            .collect()
    }

    #[test]
    fn centered_crosshair_is_selected_but_ammo_ring_is_not() {
        assert!(is_centered_shape(&vertices(&[
            [-0.04, -0.06],
            [0.04, -0.06],
            [0.04, 0.04],
            [-0.04, 0.04],
        ])));
        assert!(!is_centered_shape(&vertices(&[
            [-0.88, -0.77],
            [-0.8, -0.77],
            [-0.8, -0.65],
            [-0.88, -0.65],
        ])));
    }

    #[test]
    fn large_centered_panels_and_offscreen_shapes_are_not_reticles() {
        assert!(!is_centered_shape(&vertices(&[[-1.0, -1.0], [1.0, 1.0]])));
        assert!(!is_centered_shape(&vertices(&[[1.7, -0.75], [1.8, -0.9]])));
        assert!(!is_centered_shape(&vertices(&[
            [f32::NAN, 0.0],
            [0.01, 0.01]
        ])));
    }

    #[test]
    fn centered_shapes_do_not_depend_on_weapon_vertex_count() {
        let corners = [[-0.02, -0.04], [0.02, -0.04], [0.02, 0.02], [-0.02, 0.02]];
        for count in [5, 9, 21, 37] {
            let positions: Vec<_> = corners.into_iter().cycle().take(count).collect();
            assert!(is_centered_shape(&vertices(&positions)));
        }
    }
}
