# Third-Party Notices

`xinput1_3.dll` is a statically linked Rust binary. Everything listed here is
compiled into it, or ships alongside it in the release ZIPs, and its copyright
notice is reproduced below as those licenses require.

The mod contains no BioShock Remastered code, assets, or data. It loads the real
`xinput1_3.dll` from `%WINDIR%\System32` at runtime and redistributes no Microsoft
component.

---

## MinHook (C library, statically linked)

**License**: BSD 2-Clause
**Upstream**: https://github.com/TsudaKageyu/minhook

```
MinHook - The Minimalistic API Hooking Library for x64/x86
Copyright (C) 2009-2017 Tsuda Kageyu.
All rights reserved.

Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions
are met:

 1. Redistributions of source code must retain the above copyright
    notice, this list of conditions and the following disclaimer.
 2. Redistributions in binary form must reproduce the above copyright
    notice, this list of conditions and the following disclaimer in the
    documentation and/or other materials provided with the distribution.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS
"AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED
TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A
PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER
OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL,
EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO,
PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR
PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF
LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING
NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS
SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
```

### Hacker Disassembler Engine 32 / 64 (bundled inside MinHook)

MinHook's trampoline builder embeds HDE32 and HDE64. Both are separately
copyrighted and their notices must accompany binary redistribution.

**License**: BSD 2-Clause

```
Hacker Disassembler Engine 32 C
Copyright (c) 2008-2009, Vyacheslav Patkov.
All rights reserved.

Hacker Disassembler Engine 64 C
Copyright (c) 2008-2009, Vyacheslav Patkov.
All rights reserved.

Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions
are met:

 1. Redistributions of source code must retain the above copyright
    notice, this list of conditions and the following disclaimer.
 2. Redistributions in binary form must reproduce the above copyright
    notice, this list of conditions and the following disclaimer in the
    documentation and/or other materials provided with the distribution.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS
"AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED
TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A
PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE REGENTS OR
CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL,
EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO,
PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR
PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF
LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING
NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS
SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
```

---

## minhook (Rust crate, statically linked)

The Rust wrapper around the MinHook C library above.

**License**: MIT
**Upstream**: https://github.com/Jakobzs/minhook

```
MIT License

Copyright (c) 2025 Jakobzs

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

---

## Rust crates compiled into the DLL

The list below is the `i686-pc-windows-msvc` dependency graph of the shipped
binary. MIT requires its copyright and permission notice to travel with binary
copies, and Apache-2.0 requires a copy of the license, so each crate is named
here with its license and upstream, where the full text and the copyright
holders are published. Where a crate is dual-licensed, this distribution takes
it under MIT.

| Crate | License (SPDX) | Upstream |
|-------|----------------|----------|
| cfg-if | MIT OR Apache-2.0 | https://github.com/rust-lang/cfg-if |
| deranged | MIT OR Apache-2.0 | https://github.com/jhpratt/deranged |
| itoa | MIT OR Apache-2.0 | https://github.com/dtolnay/itoa |
| lock_api | MIT OR Apache-2.0 | https://github.com/Amanieu/parking_lot |
| log | MIT OR Apache-2.0 | https://github.com/rust-lang/log |
| num-conv | MIT OR Apache-2.0 | https://github.com/jhpratt/num-conv |
| once_cell | MIT OR Apache-2.0 | https://github.com/matklad/once_cell |
| parking_lot | MIT OR Apache-2.0 | https://github.com/Amanieu/parking_lot |
| parking_lot_core | MIT OR Apache-2.0 | https://github.com/Amanieu/parking_lot |
| pin-project-lite | Apache-2.0 OR MIT | https://github.com/taiki-e/pin-project-lite |
| powerfmt | MIT OR Apache-2.0 | https://github.com/jhpratt/powerfmt |
| scopeguard | MIT OR Apache-2.0 | https://github.com/bluss/scopeguard |
| simplelog | MIT OR Apache-2.0 | https://github.com/Drakulix/simplelog.rs |
| smallvec | MIT OR Apache-2.0 | https://github.com/servo/rust-smallvec |
| termcolor | Unlicense OR MIT | https://github.com/BurntSushi/termcolor |
| time | MIT OR Apache-2.0 | https://github.com/time-rs/time |
| time-core | MIT OR Apache-2.0 | https://github.com/time-rs/time |
| tracing | MIT | https://github.com/tokio-rs/tracing |
| tracing-core | MIT | https://github.com/tokio-rs/tracing |
| winapi-util | Unlicense OR MIT | https://github.com/BurntSushi/winapi-util |
| windows | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| windows-core | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| windows-link | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| windows-result | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| windows-strings | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| windows-sys | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| windows-targets | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| windows_i686_msvc | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |

Build-time only (procedural macros and their support crates; they run in the
compiler and are not linked into the DLL): `proc-macro2`, `quote`, `syn`,
`time-macros`, `tracing-attributes`, `unicode-ident`, `windows-implement`,
`windows-interface` - all MIT OR Apache-2.0, except `unicode-ident`, which is
`(MIT OR Apache-2.0) AND Unicode-3.0`.

`Cargo.lock` additionally pins `libc`, `bitflags`, `serde`, `redox_syscall`,
`num_threads`. None of them are reachable
from the `i686-pc-windows-msvc` graph, so none is present in the shipped DLL.

`cc`, `find-msvc-tools` and `shlex` compile the C++ lean-clamp wrapper at build
time and are not linked into the DLL.

---

## cameraunlock-core

Git submodule at `cameraunlock-core/`. The installer ZIP redistributes part of
it: `Copy-SharedBundle` stages the game-detection data and the shared installer
scripts into `shared/` (`games.json`, `GamePathDetection.psm1`, `find-game.ps1`,
`check-loader-arch.ps1`, `cecil-marker-check.ps1`, and the install / uninstall
script bodies). MIT requires its notice to travel with those copies, so the full
text is reproduced below. The DLL also compiles the shared C++ `LeanClamp`
policy, so both ZIP variants redistribute that code.

This is our own code, under a different copyright holder to the mod's own
`LICENSE`, which is why it needs a notice of its own.

- Pinned commit: `883ff59a4cbcd072f7ae18e35af26e5d38c70c5f`
- **Upstream**: https://github.com/itsloopyo/cameraunlock-core

```
MIT License

Copyright (c) 2026 itsloopyo

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

---

## OpenTrack

The mod implements OpenTrack's UDP wire format (a 48-byte packet of six
little-endian doubles). No OpenTrack code is copied, linked, or redistributed;
the format is implemented from its public documentation.

**Upstream**: https://github.com/opentrack/opentrack

---

## BioShock Remastered footage and screenshots

- **Files:** `assets/readme-clip.gif`
- **Rights holder:** the developers and publishers of BioShock Remastered, together with the
  rights holders of any third-party marks visible in frame.
- **Usage:** recorded from the game running with this mod, captured on a
  legitimately purchased copy, shown so a reader can see what the mod does
  before installing it.
- **Bundled:** no. It is kept in this repository only. The packaging scripts
  ship no part of `assets/`, so it is in neither release ZIP nor anything the
  launcher deploys.
- **Licence:** none is granted or implied by this repository. This material is
  not covered by the MIT licence in `LICENSE`, and nothing here permits reuse
  of it. Rights holders who would rather it were not published: open an issue
  or reach us on Discord and it comes down.

---

## BioShock Remastered

BioShock Remastered and all related names, logos, characters and marks are
trademarks of their respective owners. They are used here only to identify the
game this mod applies to, which is nominative use and not a claim of any right
in them. This project is an unofficial, fan-made modification. It is not
affiliated with, endorsed by, or sponsored by the game's developers, its
publishers, its engine vendor, or any other rights holder. It redistributes no
game code, no game assets and no proprietary DLLs, and it requires a
legitimately purchased copy of the game. Any engine structure offsets,
function addresses or byte patterns referenced in the source were derived by
the authors through independent analysis of a legitimately owned copy. They
are factual measurements recorded as numbers; no decompiled or disassembled
game code is stored in this repository.
