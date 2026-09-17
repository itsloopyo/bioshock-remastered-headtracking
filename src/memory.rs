use windows::core::PCSTR;
use windows::Win32::Security::Cryptography::{BCryptHash, BCRYPT_SHA256_ALG_HANDLE};
use windows::Win32::System::LibraryLoader::GetModuleHandleA;

pub struct RenderHooks {
    pub camera_constructor: usize,
    pub matrix_updater: usize,
    pub hud_draw: usize,
    pub mesh_draw: usize,
    pub lit_mesh_draw: usize,
    pub compass_vtable: usize,
}

pub fn find_render_hooks() -> Result<RenderHooks, String> {
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let bytes = std::fs::read(executable).map_err(|error| error.to_string())?;
    let mut digest = [0_u8; 32];
    unsafe { BCryptHash(BCRYPT_SHA256_ALG_HANDLE, None, &bytes, &mut digest) }
        .ok()
        .map_err(|error| error.to_string())?;
    let fingerprint = digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    if fingerprint != "aec21a0072cfdb15e4b525e2320c87256f14f16894f714272069270ad099a05b" {
        return Err(format!("Unsupported game executable SHA256: {fingerprint}"));
    }
    let base = unsafe { GetModuleHandleA(PCSTR::null()) }
        .map_err(|error| error.to_string())?
        .0 as usize;
    Ok(RenderHooks {
        camera_constructor: base + 0x56dc30,
        matrix_updater: base + 0x56dd90,
        hud_draw: base + 0x765ac0,
        mesh_draw: base + 0x455110,
        lit_mesh_draw: base + 0x455180,
        compass_vtable: base + 0xd8992c,
    })
}
