# Changelog

## [Unreleased]

### Changed

- Settings move to `Build\Final\CameraUnlock.ini`. Earlier versions of the mod kept these settings in `bioshock_headtrack.ini`, in the same folder. The first time this version starts and finds no `CameraUnlock.ini`, it reads your settings from `bioshock_headtrack.ini` and writes them into `CameraUnlock.ini`. It never changes `bioshock_headtrack.ini`, and does not read it again while `CameraUnlock.ini` exists.
- A setting that the defaults the README shows set to `default` is written as `default` when the value imported for it equals its default at that start, which is the value `Defaults.ini` gives it, or the built-in value where `Defaults.ini` gives none. It then follows `Defaults.ini`. Every other setting is written with the value imported for it.
- `RotationEnabled` and `PositionEnabled` are one setting here, the tracking mode, so both are written as `default` or neither is.
- Comments, and keys the mod never read, are not carried over. Nor are these, where your old file had them:
  - A sensitivity, scale, deadzone, response curve or axis inversion you changed from its default. Set these in your tracker instead.
  - Reticle settings, and a key that toggled the reticle.
  - The setting for a feature that earlier versions shipped switched off while it was untested. It now follows the mod's default.
- An older version of the mod reads `bioshock_headtrack.ini` and never reads `CameraUnlock.ini`, so a setting you change after updating is not in `bioshock_headtrack.ini`.
- Deleting only `CameraUnlock.ini` makes the next start read `bioshock_headtrack.ini` again. To go back to the defaults, replace everything in `CameraUnlock.ini` with the defaults the README shows. Every setting they set to `default` then follows `Defaults.ini`.
- Hotkeys are written as key names, and each hotkey lists every key that triggers it, the Ctrl+Shift chord included: `ToggleKey=End, Ctrl+Shift+Y`. A `YawModeKey` code from `bioshock_headtrack.ini` is carried as its key name with `Ctrl+Shift+H` beside it, since that chord always fired it too: `YawModeKey=0x70` becomes `YawModeKey=F1, Ctrl+Shift+H`.
- A hotkey bound to a plain key no longer fires while Ctrl and Shift are both held, so Ctrl+Shift with that key reaches only a binding that names the chord.
- Holding `End` or `Page Up` no longer repeats its action every 0.3 seconds: each press acts once.

### Added

- A setting set to `default` in `CameraUnlock.ini` takes its value from `Defaults.ini`, which every head tracking mod that keeps its settings in `CameraUnlock.ini` reads. Head tracking mods that keep their settings in another file do not read it, and neither do earlier versions of this mod. Writing a value in place of `default` changes that setting for this game only. When the mod saves a setting that a hotkey changed in game, it writes the new value in place of `default`, so that setting no longer follows `Defaults.ini` in this game until you set it to `default` again.
- `Defaults.ini` is `%AppData%\CameraUnlock\Defaults.ini` on Windows; `$XDG_CONFIG_HOME/CameraUnlock/Defaults.ini` on Linux, or `~/.config/CameraUnlock/Defaults.ini` where `XDG_CONFIG_HOME` is not set, under Wine and Proton too; and `~/Library/Application Support/CameraUnlock/Defaults.ini` on macOS. The mod's log, where it writes one, names the file it read.
- When the mod starts and finds no `Defaults.ini`, it creates one holding the built-in values, unless Windows runs the game as a packaged app. The mod never changes `Defaults.ini` after that.
- The tracking mode (`Page Up` / `Ctrl+Shift+G`) and the yaw mode (`Page Down` / `Ctrl+Shift+H`) are saved to `CameraUnlock.ini` when you change them, as `RotationEnabled` and `PositionEnabled`, and `WorldSpaceYaw`, and the next start begins in them. Earlier versions started in rotation and position every time and never saved the yaw mode.
- New settings: `UdpPort` (the port the mod listens on, 4242 by default), `EnableOnStartup` (whether tracking is on when the game starts, true by default), and `ToggleKey` and `CycleTrackingModeKey`, which were fixed keys before. `YawModeKey` now holds its chord too.

## [0.5.0] - 2026-09-17

### Added

- standardise the log file, name every discovery failure, gate license notices
- hook the render scene node and move the game's own reticle

### Fixed

- Head tracking stepped at the tracker's rate instead of interpolating between
  samples whenever the sender repeated a value. OpenTrack relays at ~250Hz
  regardless of the source rate and phone trackers resend the last pose rather
  than going quiet, and the pipeline treated every datagram as a fresh sample: the
  sample-interval estimate collapsed onto the packet interval, so each segment
  finished within a frame or two and the in-between frames the interpolator exists
  to generate went flat. It also reset the stall clock on every repeat, so the
  extrapolation never expired for a tracker streaming a stale pose - one of the two
  cases that expiry was written for. Rotation and position now each compare the
  values, not just the packet counter.
- A datagram whose fields were finite doubles but outside the range of a 32-bit
  float, such as `1e300`, passed validation and became an infinity when the engine
  hook narrowed it to write the game's `FVector`. The socket binds `0.0.0.0`, so
  any host on the network could send one. Validation now gates on the narrowed
  value.
- re-sync THIRD-PARTY-NOTICES before cutting a release
- mirror the vertical limit and restore the MIT grant
- retry the tracker port every 500ms instead of every 5s
- preserve originals across shim upgrades and uninstall failures
- settle and confirm the window centering instead of moving once
- suspend head tracking in the pause menu

## [0.4.0] - 2026-08-20

### Added

- A `First tracker packet from ...` line in the log. Nothing previously recorded
  that tracker data had arrived, so a log from a misconfigured tracker looked
  identical to a healthy one.

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

### Fixed

- The mod no longer aborts the game when it cannot create its log file. Under
  `panic = "abort"` the old `.expect` took the whole process down over a
  diagnostic file; it now continues without logging.
- Three receive-loop warnings (non-finite packet, unexpected packet size, UDP
  receive error) were logged per datagram with no latch. A tracker emitting NaN
  produced ~250 lines a second, and a sticky socket error spun the loop with no
  sleep and logged at CPU speed. Each is now reported once, with the socket
  error deduplicated by error kind.
- honour the 5s HCAM re-arm window and publish a press with its pose
- expire the extrapolation instead of holding the overshoot
- clamp position into the limits before smoothing, not only after
- keep the heading when the view pitch reaches vertical
- assume a 30Hz tracker, not 60Hz, until the real rate is measured
- take the shortest arc for yaw and roll across the 180 seam
- pass required ShimMarker to Invoke-DevDeployShim

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
