# stlink-tool

Drag-and-drop **STM32F10x** flasher for Windows with self-healing **ST-Link** WinUSB driver.

> v0.2.0 — fully silent driver fix via self-built `wdi-simple.exe`. Single chip family (F10x, e.g., F103C8T6 "Blue Pill"); other STM32 series in v0.3+.

## What it does

1. Drop a `.hex`, `.bin`, or `.elf` onto the window.
2. The tool detects your ST-Link (V2 / V2-1 / V3) and **silently installs the WinUSB driver** if missing or wrong-bound. The only interaction is a single OS-level UAC prompt — no GUI to click through.
3. It flashes the chip via bundled OpenOCD and reports success/failure.
4. **Auto-reflash on file change** (default on): rebuild from your IDE → the tool re-flashes on its own. No more "compile, find tool, click flash, repeat".

Built for the embedded inner loop. Drop once; iterate forever.

## Install

Download `stlink-tool-windows-x64.exe` from [the latest release](https://github.com/Daily-AC/stlink-tool/releases/latest) and double-click. Nothing else to install — Rust toolchain, OpenOCD, libwdi, and Visual C++ runtime are all bundled inside the single `.exe`.

PowerShell one-liner:
```powershell
curl.exe -L -o stlink-tool.exe https://github.com/Daily-AC/stlink-tool/releases/latest/download/stlink-tool-windows-x64.exe
.\stlink-tool.exe
```

## Build from source

```powershell
rustup default stable
cargo build --release
# binary → target\release\stlink-tool.exe
```

The repo is self-contained: `vendor/wdi-simple.exe` (built in CI from libwdi v1.5.1) and `vendor/xpack-openocd-0.12.0-7-win32-x64.zip` are checked in. No fetch script needed. Cross-platform `cargo test` runs the parser/validation tests on macOS or Linux.

## What's bundled

| Tool | Source | License |
|---|---|---|
| wdi-simple (libwdi v1.5.1, built in our CI) | [pbatard/libwdi](https://github.com/pbatard/libwdi/releases/tag/v1.5.1) | LGPLv3 |
| xpack-openocd 0.12.0-7 | [xpack-dev-tools/openocd-xpack](https://github.com/xpack-dev-tools/openocd-xpack/releases/tag/v0.12.0-7) | GPLv2 |

stlink-tool itself is dual-licensed MIT OR Apache-2.0. wdi-simple and OpenOCD are invoked as separate processes; no copyleft code is statically linked into our binary.

## Roadmap

- ~~v0.1.0 — F10x, drag-and-drop, semi-automated driver fix (Zadig click)~~
- **v0.2.0 (current)** — self-built `wdi-simple.exe` for fully silent driver fix
- **v0.3.0** — STM32F4 series support
- **v0.4.0** — F7 / H7 / U5 / N6 series; per-target chip auto-detection from IDCODE
- **v0.5.0** — Code-signing certificate (remove SmartScreen warning)

## License

MIT OR Apache-2.0
