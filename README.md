# BioShock Remastered Head Tracking

![BioShock Remastered running with this mod](https://raw.githubusercontent.com/itsloopyo/bioshock-remastered-headtracking/main/assets/readme-clip.gif)

<sub>Gameplay footage of BioShock Remastered, (c) 2K Games / Irrational Games /
Blind Squirrel Entertainment, shown to demonstrate what the mod does.</sub>

An unofficial head tracking mod for BioShock Remastered that moves the view with your head while your mouse or controller keeps aiming, driven by a webcam, phone, or any OpenTrack compatible tracker, with no VR headset required.

## Features

- **6DOF head tracking** via OpenTrack UDP - yaw, pitch, roll, and
  positional lean.
- **True look / aim decoupling** - the engine sees your unmodified
  mouse aim while the rendered view follows your head, so bullets
  always land where the reticle is drawn.
- **Parallax-correct reticle** drawn directly into the swap chain,
  replacing BioShock's gun and plasmid reticles.

## Requirements

- A purchased copy of **BioShock Remastered** on Steam (AppID 409710).
- **Windows 10 / 11**.
- A tracking source that speaks OpenTrack UDP. Anything that can send
  48-byte OpenTrack packets to port 4242 works.

## Installation

1. Download the latest `BioshockRemasteredHeadTracking-v<version>-installer.zip`
   from [Releases](https://github.com/itsloopyo/bioshock-remastered-headtracking/releases).
2. Extract anywhere.
3. Run `install.cmd` - it auto-detects your Steam install and copies
   `xinput1_3.dll` into `BioShock Remastered/Build/Final/`.

For NexusMods users: grab the `-nexus.zip` variant and extract straight
into your game's root folder.

### Manual install

Copy `xinput1_3.dll` into:

```
<Steam library>/steamapps/common/BioShock Remastered/Build/Final/
```

## OpenTrack setup

| Setting  | Value              |
|----------|--------------------|
| Output   | UDP over network   |
| Address  | 127.0.0.1          |
| Port     | **4242**           |
| Protocol | UDP Position + Rotation (raw doubles) |

Filter / smoothing / deadzone choice is up to you.

## Phone app setup

The receiver binds to `0.0.0.0:4242`, so a phone tracking app on the
same WiFi can send directly to your PC at port 4242 - provided the
app smooths the signal before sending. Without smoothing, raw phone
gyro data is too jittery for use.

## Controls

Two equivalent binding sets - use whichever your keyboard has:

| Action              | Nav-cluster | Chord           |
|---------------------|-------------|-----------------|
| Toggle tracking     | `End`       | `Ctrl+Shift+Y`  |
| Cycle tracking mode | `Page Up`   | `Ctrl+Shift+G`  |
| Toggle yaw mode     | `Page Down` | `Ctrl+Shift+H`  |

`Page Up` / `Ctrl+Shift+G` cycles tracking mode:

1. Normal head-tracked gameplay
2. Positional tracking disabled, rotational tracking enabled
3. Rotational tracking disabled, positional tracking enabled
4. Back to normal

There is no recenter key. Centre the view in your tracker app
(OpenTrack's Center bind, or the CENTER button in a phone tracker) and
the mod follows it.

The chord letters Y/G/H sit in a vertical strip in the centre of
the keyboard. `Ctrl+Shift+<letter>` is universally avoided by games,
so the chord set works whether or not your keyboard has a nav
cluster.

## Configuration

The mod writes a self-documenting `bioshock_headtrack.ini` to
`BioShock Remastered/Build/Final/` on first launch.

```ini
[General]
; Yaw mode: true = horizon-locked yaw (default), false = camera-local
WorldSpaceYaw=true

[Hotkeys]
; Page Down - toggle world/local yaw
YawModeKey=0x22

[Smoothing]
; Smoothing applied when the tracker runs on this machine (loopback).
; 0 = no smoothing, 1 = heavy. Covers rotation and position.
LocalSmoothing=0.0
; Smoothing applied when the tracker is a remote device on the network.
; 0 = no smoothing, 1 = heavy. Covers rotation and position.
RemoteSmoothing=0.15
```

### Smoothing

| Key | Default | Range | Applies to |
|-----|---------|-------|------------|
| `LocalSmoothing` | 0.0 | 0.0 - 1.0 | Smoothing applied when the tracker runs on this machine (loopback). 0 = no smoothing, 1 = heavy. |
| `RemoteSmoothing` | 0.15 | 0.0 - 1.0 | Smoothing applied when the tracker is a remote device on the network. 0 = no smoothing, 1 = heavy. |

Both cover rotation and position; there is no separate position
smoothing setting. The mod reads the source address of each tracking
packet: loopback (`127.0.0.1`, `::1`) means local, anything else means
remote. Switching from a local OpenTrack instance to a phone on WiFi
swaps the value immediately, with no game restart. Local defaults to
zero because a same-machine tracker is already stable and any smoothing
there is pure added latency.

### Non-default FOV

If you've changed the FOV slider in BSR's options away from 100° (the
stock value), the head-tracked reticle may drift away from the actual
aim point - the mod can't auto-detect the slider value. Tell it your
FOV manually:

1. Open `bioshock_headtrack.ini`.
2. Uncomment and set the overlay FOV:

   ```ini
   [overlay]
   fov_h = 90
   ```

   Replace `90` with whatever horizontal FOV you've set in-game. Valid
   range: 40-150°. Vertical FOV is derived from horizontal at 16:9.
3. Restart the game.

## Troubleshooting

**No tracking in-game.**
- Confirm your tracker is sending UDP to `127.0.0.1:4242` (or your
  PC's LAN IP if tracking from a phone).
- Check the mod log at
  `BioShock Remastered/Build/Final/HeadTracking.log`. It is rewritten on
  every launch, so it only ever holds the most recent session - send it
  when reporting a problem. The session before it is kept alongside as
  `HeadTracking.prev.log`, which is the one to send if the game crashed.
- Look for `First tracker packet from ...` in that log. If it is absent,
  no tracker packet ever reached the mod and the problem is upstream of
  the game.
- Look for `eventPlayerCalcView detour is receiving calls`. If the hook
  installed but that line never appears, the mod loaded without ever
  getting the camera - include the log so we can see which build you are
  on.

**Game crashes on launch.**
- `xinput1_3.dll` must be in `Build/Final/`, not the game root.
- Steam -> right-click BioShock Remastered -> Properties -> Local Files
  -> Verify integrity, then reinstall the mod.

**Reticle drifts left/right as you yaw your head.**
- You're likely running a non-stock FOV. Set `[overlay] fov_h` in
  `bioshock_headtrack.ini` (see Configuration).

**Yaw feels wrong when looking up or down at extreme angles.**
- Try toggling between world-locked and camera-local yaw with
  `Page Down`. World-locked (default) is horizon-stable; camera-local
  follows the camera's current up-axis.

**Wrong rotation / jitter.**
- Increase smoothing in your tracking source.
- Set `BIOSHOCK_PATH` env var to override game detection if
  `install.cmd` can't find your install.

## Updating / uninstalling

To update, run the new installer over the old DLL - it overwrites in
place.

Run `uninstall.cmd` from the extracted installer, or run
`pixi run uninstall` from a source checkout. Both restore the
original `xinput1_3.dll` from the `.backup` file the installer left
behind.

## Building from source

```powershell
pixi run build-release   # 32-bit, i686-pc-windows-msvc
pixi run install-release # deploy locally to your Steam install
```

`pixi run release <version>` bumps `Cargo.toml`, regenerates the
changelog from commits, tags, and pushes - CI builds and uploads the
release ZIPs.

## Community & Support

- Discord: [Loop's Head Tracking Hangout](https://discord.com/invite/dxyZdyFNT9) - setup help, bug reports, and new-release announcements
- [Lopari](https://lopari.app) - free Windows launcher with one-click install and launch for the released head-tracking mods
- [Headcam](https://headcam.app) - free app that turns your iPhone or Android phone into the head tracker

## License

MIT - see [LICENSE](LICENSE). Third-party components compiled into the
DLL are listed with their notices in
[THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md).

## Legal

This is an unofficial, non-commercial fan modification. It is not
affiliated with, endorsed by, or sponsored by 2K Games, Take-Two
Interactive, Irrational Games, or Blind Squirrel Entertainment.
"BioShock" and all related names, logos, and marks are trademarks of
their respective owners and are used here only to identify the game the
mod applies to.

The mod contains no game code, assets, or data. It ships a single DLL of
original work that hooks the running game in memory, and it requires a
legitimately purchased copy of BioShock Remastered. It defeats no copy
protection and includes no part of the game or of Microsoft's XInput
runtime, which it loads from your own Windows installation.

The clip at the top of this page is a short piece of BioShock Remastered
gameplay footage, copyright its respective owners, included solely to
demonstrate what the mod does. No claim of ownership is made over it. If
a rights holder would prefer it removed, open an issue and it comes
down.

## Credits

- BioShock Remastered (c) 2K / Irrational Games / Blind Squirrel
  Entertainment.
- [OpenTrack](https://github.com/opentrack/opentrack) for the UDP
  protocol.
- [MinHook](https://github.com/TsudaKageyu/minhook) for function
  hooking.
