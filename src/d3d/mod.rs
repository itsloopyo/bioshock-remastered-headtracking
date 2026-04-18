//! D3D11-side hooks: the HUD-suppress / draw-call introspection layer
//! (`hud`) and the parallax-corrected reticle overlay (`overlay`).
//!
//! Grouped because they share the same swap chain, share the
//! `HUD_ACTIVE_THIS_FRAME` gate that lets the overlay draw only on
//! frames the game itself drew its HUD on, and are always installed
//! together from `lib::install_d3d_hooks`.

pub mod hud;
pub mod overlay;
