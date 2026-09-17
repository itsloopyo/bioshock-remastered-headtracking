# BioShock Remastered Head Tracking

![BioShock Remastered running with this mod](https://raw.githubusercontent.com/itsloopyo/bioshock-remastered-headtracking/main/assets/readme-clip.gif)

<sub>Gameplay footage of BioShock Remastered, (c) 2K Games / Irrational Games /
Blind Squirrel Entertainment, shown to demonstrate what the mod does.</sub>

An unofficial head tracking mod for BioShock Remastered that moves the view with your head while your mouse or controller keeps aiming, driven by a webcam, phone, or any OpenTrack compatible tracker, with no VR headset required.

## Features

- **Decoupled look and aim** - head tracking moves the view; your
  mouse or controller keeps aiming.
- **6DOF tracking** - yaw, pitch and roll plus positional lean, peek and duck.
- **Works with any OpenTrack compatible tracker** - free options available for PC, iOS and Android
- **Native reticles** - BioShock's own reticles move with the aim point.

## Requirements

- A purchased copy of **BioShock Remastered** on Steam (AppID 409710).
- **Windows 10 / 11**.
- A tracking source that speaks OpenTrack UDP. Anything that can send
  48-byte OpenTrack packets to port 4242 works.

## Installation

### Lopari

Download [Lopari](https://lopari.app), choose **BioShock Remastered**, and click
**Play with head tracking**.

### Standalone Installer

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

## Setting Up OpenTrack

The mod listens for OpenTrack pose data on UDP port `4242`, on every network
interface. One datagram is six little-endian 64-bit floats in the order
`x, y, z, yaw, pitch, roll`: position in centimetres, rotation in degrees, 48
bytes in total. Anything that sends that to that port drives the view.
OpenTrack's **UDP over network** output sends exactly this, and the steps below
set it up.

1. Install [OpenTrack](https://github.com/opentrack/opentrack/releases).
2. Pick a tracker under **Input**, using the notes below.
3. Set **Output** to **UDP over network**, host `127.0.0.1`, port `4242`.
4. Press **Start**. Tracking and the game can start in either order.

### Webcam

OpenTrack ships a `neuralnet tracker` input that reads a plain webcam. Select it
under **Input**, pick your camera in its settings, and use the output settings
above. How well it tracks depends on your camera and your lighting, so try it
before buying anything.

### Phone

A phone app can reach the mod directly, with no OpenTrack on the PC, if it sends
the datagram described above. Point it at this PC's IP address (run `ipconfig`
to find it) on port `4242`. Not every phone tracker speaks this protocol, so
check yours for an OpenTrack or UDP output option first. [Headcam](https://headcam.app)
sends it, and I wrote it so decent tracking is free for anyone who already owns
a phone.

Sending direct works when the app filters its own signal on the device. The
mod's smoothing is sized to take the edge off a clean signal rather than to
rescue a noisy one, so a raw feed sent direct will jitter. If it does, point the
app at OpenTrack's **UDP over network** *input* on some other port, say 5252,
and let OpenTrack's filters and curves clean it up before its output forwards to
`127.0.0.1:4242`.

Anything arriving from outside `127.0.0.0/8` counts as a remote connection and
is smoothed with `RemoteSmoothing` rather than `LocalSmoothing`. That includes a
tracker on this very PC that sends to the machine's own LAN address, because the
mod reads the source address and not the machine.

### Headset or other hardware

If your device has an OpenTrack input driver, select it under **Input** and use
the same output settings. OpenTrack's own **Input** list is the authority on
what it can read; the mod only ever sees what OpenTrack sends.

### Centring

Centring belongs to your tracker. The mod subtracts no centre of its own: it
applies the pose it receives exactly as it arrives, so a stream of zeros holds
the view where the game itself puts it. Press the centre control in your tracker
(OpenTrack's **Center** bind, or the CENTER button in Headcam) and the tracker
zeroes its own output, which leaves the view centred with the mod doing nothing.

That is why there is no centre hotkey here and nothing to re-centre in game. Two
centres in series would drift apart, because each side re-centres at moments the
other cannot see, and you would end up pressing twice to centre once. If the
view sits off to one side, centre it in the tracker.

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

### Field of view

The mod reads the game's world and weapon projection matrices. Use the in-game
FOV setting; the old `[overlay] fov_h` override is no longer used.

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
- Look for `FPlayerSceneNode hook installed` and `render:` entries during
  gameplay. Include the log if either is missing.

**Game crashes on launch.**
- `xinput1_3.dll` must be in `Build/Final/`, not the game root.
- Steam -> right-click BioShock Remastered -> Properties -> Local Files
  -> Verify integrity, then reinstall the mod.

**Reticle drifts left/right as you yaw your head.**
- Send `HeadTracking.log` with your in-game FOV setting and a screenshot
  showing the misalignment. The mod uses the game's projection matrices;
  no manual FOV calibration is needed.

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
