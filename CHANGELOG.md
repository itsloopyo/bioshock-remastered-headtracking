# Changelog

## [Unreleased]

### Added

- A `First tracker packet from ...` line in the log. Nothing previously recorded
  that tracker data had arrived, so a log from a misconfigured tracker looked
  identical to a healthy one.

### Fixed

- The mod no longer aborts the game when it cannot create its log file. Under
  `panic = "abort"` the old `.expect` took the whole process down over a
  diagnostic file; it now continues without logging.
- Three receive-loop warnings (non-finite packet, unexpected packet size, UDP
  receive error) were logged per datagram with no latch. A tracker emitting NaN
  produced ~250 lines a second, and a sticky socket error spun the loop with no
  sleep and logged at CPU speed. Each is now reported once, with the socket
  error deduplicated by error kind.

### Changed

- Removed recentring from the mod. The `Home` / `Ctrl+Shift+T` hotkey is
  gone, and the mod no longer acts on the CENTER signal a tracker app
  sends in its packets. Centre the view in your tracker app instead. A
  centre in the mod sat in series with the tracker's own and the two
  drifted apart, so switching trackers meant recentring twice.
- Smoothing is now two INI keys instead of one: `[Smoothing]
  LocalSmoothing` (default `0.0`) for a tracker running on this machine,
  and `[Smoothing] RemoteSmoothing` (default `0.15`) for a remote device
  sending over the network. Both cover rotation and position, so the
  separate position smoothing value is gone.
- Removed the hidden `0.15` baseline floor. It silently overrode the
  configured value, so local users now get zero-latency tracking by
  default instead of a forced 0.15.
- The OpenTrack receiver now reads each packet's source address and
  classifies loopback senders as local and everything else as remote.
  The smoothing value is re-selected per frame, so switching between a
  local OpenTrack instance and a phone on WiFi takes effect without a
  game restart.

## [0.3.6] - 2026-08-03

### Added

- recenter from Headcam trailer packets, drop non-finite tracking data

### Fixed

- show full control set in pixi install via shared -Controls

## [0.3.5] - 2026-06-07

### Fixed

- harden release.ps1 - changelog gate before version bump, add -Force

## [0.3.3] - 2026-06-07

### Added

- add HeadTrackingSession and expand C++ core with RE Engine, Unreal, and tracking-session modules
- aim projection, reframework/unreal hooks, input/logging hardening, games
- add Mass Effect Legendary Edition to games catalog
- expand games catalog, fix unicode games.json read, stage launcher manifest
- add Pacific Drive to games catalog
- add Homeworld: Remastered Collection to games catalog
- add manifest-mode installer validator and ASI loader subdir support
- authenticate GitHub API requests via env token when present
- add world/camera-local yaw mode toggle, migrate to manifest delivery and pixi-driven CI
- add R.E.P.O. detection data

### Fixed

- fail fast in ASI dev-deploy when the game is running
- restore il2cpp camera position by undoing applied local delta
- set SO_REUSEADDR so the receiver reclaims its port on relaunch

### Other

- Add Ubisoft Connect detection and VendorZip BepInEx install
- Add PluginSubfolder param to Invoke-DevDeployBepInEx
- Add Xbox install path for Easy Delivery Co
- Add GOG IDs for Cyberpunk 2077
- Add PLUGIN_SUBFOLDER support to BepInEx install/uninstall bodies
- scripts: drop the two-phase loader-init prompt from install bodies
- data: add Black & White (Lionhead) to games registry
- scripts: detect BepInEx 6 IL2CPP via BepInEx.Core.dll marker
- powershell: skip cameraunlock-core remote refresh in CI
- scripts: add UE4SS install template, fix delayed expansion in ASI body, expand games registry
- protocol: reject finite-but-out-of-float-range packet values
- data: add Subnautica 2 to games registry
- detection: add installer-registry game path lookup (Black & White GameDir)
- protocol: reorder tracking data member in udp_receiver
- data: fix Subnautica 2 Steam app id (3367150 -> 1962700)
- data: add Ni no Kuni Remastered and Yakuza 0; switch find-game output to UTF-8
- detection: add Xbox/GDK build support for Subnautica 2 (and any future GDK title)
- find-game: escape `&` in GAME_DISPLAY_NAME so echo doesn't split
- templates: add uninstall.ps1; data: add Deus Ex Mankind Divided
- powershell: add NightlyRelease module for Patreon-gated nightly builds
- Add release nightly dispatch and publisher shim
- protocol: disable SIO_UDP_CONNRESET and add one-shot receiver diagnostics; powershell: write nightly manifest.json without UTF-8 BOM; data: add Mixtape
- powershell: stop redirecting git stderr in Update-CameraUnlockCoreToRemoteTip
- powershell: publish dev builds as GitHub pre-releases
- protocol: disable SIO_UDP_CONNRESET and add one-shot receiver diagnostics
- data: add Mixtape
- powershell: stop redirecting git stderr in Update-CameraUnlockCoreToRemoteTip
- powershell: run gh under Continue so its stderr doesn't abort the dev-release publish
- reframework: strip VR runtime DLLs on install for flatscreen mode
- reframework: cache GetValue method and avoid per-call heap in ArrayGetValue; data: add BioShock Infinite
- uninstall: remove reframework_revision.txt marker dropped at game root
- install: render MOD_CONTROLS multi-line via percent expansion
- Add YAPYAP to games.json
- powershell: write state file BOM-less so Lopari JSON parser accepts it

## [0.3.2] - 2026-05-03

### Added

- center game window on first frame to fix ultrawide top-left launch

### Other

- Verify existing BepInEx loader arch and replace on mismatch
- Fall back to dev-tree vendor path in BepInEx install body

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
