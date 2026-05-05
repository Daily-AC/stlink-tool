# stlink-tool

Drag-and-drop **STM32F10x** flasher for Windows with self-healing **ST-Link** WinUSB driver.

> v0.1.0 — single-chip-family build (F10x, e.g., F103C8T6 "Blue Pill"). Other STM32 series will land in v0.2+.

## What it does

1. Drop a `.hex`, `.bin`, or `.elf` onto the window.
2. The tool detects your ST-Link (V2 / V2-1 / V3) and **automatically launches Zadig with WinUSB pre-selected** if the driver is missing or wrong. You click "Replace Driver" once; the rest is automatic.
3. It flashes the chip via bundled OpenOCD and reports success/failure.
4. **Auto-reflash on file change** (default on): rebuild from your IDE → the tool re-flashes on its own. No more "compile, find tool, click flash, repeat".

Built for the embedded inner loop. Drop once; iterate forever.

## Build

```powershell
# On Windows:
rustup default stable
cargo build --release
# binary → target\release\stlink-tool.exe
```

The repo is self-contained — `vendor/zadig-2.9.exe` and `vendor/xpack-openocd-0.12.0-7-win32-x64.zip` are checked in. No fetch script needed.

Cross-platform `cargo test` runs the parser/validation tests on macOS or Linux.

## What's bundled

| Tool | Source | License |
|---|---|---|
| Zadig 2.9 | [pbatard/libwdi v1.5.1](https://github.com/pbatard/libwdi/releases/tag/v1.5.1) | GPLv3 (separate executable) |
| xpack-openocd 0.12.0-7 | [xpack-dev-tools/openocd-xpack v0.12.0-7](https://github.com/xpack-dev-tools/openocd-xpack/releases/tag/v0.12.0-7) | GPLv2 (OpenOCD) |

stlink-tool itself is dual-licensed MIT OR Apache-2.0. It invokes Zadig and OpenOCD as separate processes; no GPL code is statically linked.

## Roadmap

- **v0.1.0 (this release)** — F10x, drag-and-drop, semi-automated driver fix (one click in Zadig)
- **v0.2.0** — Self-built `wdi-simple.exe` for fully silent driver fix; expand to F4 series
- **v0.3.0** — F7 / H7 / U5 / N6; per-target chip auto-detection

## License

MIT OR Apache-2.0
