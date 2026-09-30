use windows::core::PCSTR;
use windows::Win32::Security::Cryptography::{BCryptHash, BCRYPT_SHA256_ALG_HANDLE};
use windows::Win32::System::LibraryLoader::GetModuleHandleA;

pub struct RenderHooks {
    pub camera_constructor: usize,
    pub matrix_updater: usize,
    pub point_region: usize,
    pub hud_draw: usize,
    pub mesh_draw: usize,
    pub lit_mesh_draw: usize,
    pub compass_vtable: usize,
}

const STEAM_SHA256: &str = "aec21a0072cfdb15e4b525e2320c87256f14f16894f714272069270ad099a05b";

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
    let base = unsafe { GetModuleHandleA(PCSTR::null()) }
        .map_err(|error| error.to_string())?
        .0 as usize;
    select_render_hooks(&bytes, &fingerprint, base)
}

fn select_render_hooks(
    bytes: &[u8],
    fingerprint: &str,
    base: usize,
) -> Result<RenderHooks, String> {
    if fingerprint != STEAM_SHA256 {
        return Err(format!("Unsupported game executable SHA256: {fingerprint}"));
    }
    let compass_vtable = crate::rtti::compass_vtable(bytes)?;
    if compass_vtable != 0xd8992c {
        return Err(format!(
            "Compass RTTI disagrees with exact-build profile: RVA {compass_vtable:#x}, expected 0xd8992c"
        ));
    }
    log::info!("Compass RTTI: AGPSArrow primary vtable RVA {compass_vtable:#x}; exact-build cross-check passed");
    Ok(RenderHooks {
        camera_constructor: base + 0x56dc30,
        matrix_updater: base + 0x56dd90,
        point_region: base + 0x55b7c0,
        hud_draw: base + 0x765ac0,
        mesh_draw: base + 0x455110,
        lit_mesh_draw: base + 0x455180,
        compass_vtable: base + compass_vtable,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unlisted_builds_cannot_use_the_remaining_profile_pins() {
        assert_eq!(
            select_render_hooks(&[], "unlisted", 0x400000)
                .err()
                .unwrap(),
            "Unsupported game executable SHA256: unlisted"
        );
        assert!(select_render_hooks(&[], STEAM_SHA256, 0x400000).is_err());
    }

    #[test]
    #[ignore = "requires the installed Steam executable through BIOSHOCK_DISCOVERY_EXE"]
    fn installed_image_selection_then_unlisted_rejection() {
        let path = std::env::var_os("BIOSHOCK_DISCOVERY_EXE").unwrap();
        let bytes = std::fs::read(path).unwrap();
        let hooks = select_render_hooks(&bytes, STEAM_SHA256, 0x70000000).unwrap();
        assert_eq!(hooks.compass_vtable, 0x70d8992c);
        assert_eq!(hooks.camera_constructor, 0x7056dc30);
        assert!(select_render_hooks(&bytes, "unlisted", 0x70000000).is_err());
        assert!(select_render_hooks(&[], STEAM_SHA256, 0x70000000).is_err());
    }
}
