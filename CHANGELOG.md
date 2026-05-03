# Changelog

## [0.3.1] - 2026-05-03

### Other

- Add DX11 overlay header for crosshair rendering
- Update PositionInterpolator tests for bounded extrapolation
- Skip vendor refresh when SHA-256 matches existing copy
- Fix degenerate-input bugs in scanners, projection, and color parser
- Add yaw-mode key and WorldSpaceYaw config options
- Quote /y flag detection and add shared install/uninstall bodies
- Add DevDeploy module with Cecil dev-install orchestrator
- Auto-refresh cameraunlock-core submodule in Copy-SharedBundle
- Add install bodies and dev-deploy orchestrators for non-Cecil frameworks
- Resolve exe relpath from games.json in ASI/shim dev-deploy
- Add automatic port retry to C++ UdpReceiver
- Take BuildOutputPath in dev-deploy and add loader/config auto-install

## [0.3.0] - 2026-04-30

### Fixed

- skip rotation compensation in reticle projection when rotation tracking is off

### Other

- Expand submodule pointer commits in generated changelogs
- Fix /y flag detection and bundle vendored BepInEx in installers
- Use WriteAllBytes for .cmd output to avoid Defender race

## [0.2.2] - 2026-04-29

### Added

- cycle rotation/position tracking on Page Up

### Other

- build: bundle shared installer scripts in release ZIP
- chore: bump cameraunlock-core to 2c5511e

## [0.2.0] - 2026-04-29

### Added

- per-axis smoothing pipeline and resilient UDP bind

## [0.1.1] - 2026-04-19

### Fixed

- correct hotkey banners and paren-safe installer error path

## [0.1.0] - 2026-04-18

First release.
