---
title: stlink-tool design
date: 2026-05-05
last_updated: 2026-05-05 (v0.2.0)
status: locked
---

# stlink-tool — Design Spec

A Windows drag-and-drop flasher for STM32F10x with self-healing ST-Link WinUSB driver. Goal: collapse the embedded developer's "compile → find flasher → install/repair driver → click → wait" inner loop into a single drop.

## Context

ST's stack on Windows is famously punishing for new developers and clean machines: ST-Link USB driver is a separate ~30 MB download, the WinUSB binding required by OpenOCD/pyOCD must be installed via Zadig, the official ST tools and the open-source tools want different driver bindings, and corruption (yellow-bang in Device Manager) is common after Windows updates. STM32CubeProgrammer is 600 MB and license-restricted — too heavy and too closed to embed.

**Constraint**: ship fast. v0.1.0 targets only the **STM32F10x family** (specifically the F103C8T6 "Blue Pill") so we can hard-code the OpenOCD target config and avoid the multi-chip configuration matrix.

## Architecture

Single Rust binary (~14 MB) embedding two Windows tools:

```
stlink-tool.exe
├── embed: vendor/wdi-simple.exe                  (4.6 MB; libwdi v1.5.1, built in our CI)
├── embed: vendor/xpack-openocd-0.12.0-7.zip      (3.2 MB, GPLv2)
└── runtime: extract bundle to %LOCALAPPDATA%\stlink-tool\bundle-<sha8>\
            (cache key = SHA-256 of embedded bytes; old bundles GC'd on upgrade)
```

**Crates**:
- `eframe` 0.34 + `egui` 0.34 — GUI (drag-and-drop via `ctx.input(|i| i.raw.dropped_files)`)
- `nusb` 0.2 — USB enumeration; `open()` doubles as our "is the driver in the right state?" probe
- `tokio` (rt-multi-thread, process, io-util, sync) — streaming OpenOCD subprocess output
- `notify` 8 — file watcher for auto-reflash
- `windows` 0.62 — `ShellExecuteExW` with `runas` verb to elevate just the driver-fix step
- `zip` 8, `sha2`, `directories`, `thiserror`, `tracing`

**Process model**: GUI runs on the main thread; a 2-worker tokio runtime handles validation, USB detection (via `spawn_blocking`), driver-fix subprocess, and OpenOCD flashing. Cross-thread comms via `tokio::sync::mpsc` (workers → GUI) and `std::sync::mpsc` (file watcher → GUI), drained in the egui `update()` loop.

## User pipeline (v0.2.0)

```
Idle ── drop .hex/.bin/.elf ──▶ Validating ──▶ Detecting
                                                  │
            ┌─────────────────────────────────────┤
            │                                     │
        no device                          NeedsDriverFix
            │                                     │
        Error                              FixingDriver
                                          (UAC prompts once;
                                           wdi-simple --silent runs)
                                                  │
                                            re-detect
                                                  │
                                              Ready ──▶ Flashing ──▶ Done
```

Single linear pipeline. No "AwaitingDriverFix" wait-for-click state — once the user drops a file, the only remaining intervention is the OS-level UAC prompt.

**File handling** (in `flasher::build_args`):
- `.hex` and `.elf`: `program {file} verify reset exit` (addresses come from the file)
- `.bin`: `program {file} 0x08000000 verify reset exit` (F10x flash base)

OpenOCD command line:
```
openocd.exe -s <bundle>/scripts -f interface/stlink.cfg -f target/stm32f1x.cfg -c "<prog_cmd>"
```

**Auto-reflash**: a `notify` watcher on the dropped file's parent directory. When the canonical filename gets a Modify/Create event AND the app is in `Idle | Done | Error`, the pipeline re-fires automatically. Default-on; toggle visible in the UI.

## Driver-fix subsystem (v0.2.0)

**Triggering**: `nusb::DeviceInfo::open()` requires WinUSB binding on Windows. If that fails, we know the driver is wrong → state transitions to `Working(FixingDriver)`. The pipeline immediately invokes `driver_fix::install_winusb_blocking(bundle, vid, pid)` via `spawn_blocking`.

**Mechanism**: `ShellExecuteExW("runas", wdi-simple.exe, "--vid 0x0483 --pid <pid> --iid 0 --type 0 --silent --name \"...\"", SW_HIDE)`. UAC prompts; on confirm, wdi-simple installs the WinUSB driver in the background with no visible window. The pipeline `WaitForSingleObject` until completion, reads the exit code via `GetExitCodeProcess`, then re-runs detection.

**Why we self-build wdi-simple**: libwdi's official releases ship only `zadig.exe` (GUI). Their wiki explicitly notes prebuilt `wdi-simple.exe` isn't distributed. v0.1.0 worked around this by bundling Zadig + a preset `zadig.ini` and asking the user to click "Replace Driver" once. v0.2.0 builds wdi-simple in our own CI (`.github/workflows/build-wdi-simple.yml`) using libwdi sources at v1.5.1, the WDK 8.0 redistributable, and the libusb0 / libusbK driver binaries — exactly the recipe libwdi's own CI uses. The resulting binary lives in `vendor/wdi-simple.exe`.

**Why not VCP auto-install**: ST's Virtual COM Port driver redistribution license is unclear. Win10 1709+ binds the V2-1 / V3 VCP interface to the built-in `usbser.sys` automatically in 95% of cases. v0.2 doesn't auto-install ST's VCP driver; if the user's machine is the rare 5% that needs it, the log surfaces a one-line hint.

**libwdi signing**: wdi-simple ships with Microsoft OEM category 0 signature, which Windows 10/11 honor for USB driver installation. No additional signing infrastructure needed on our side.

**Error model** (`FlashError`):
- `DriverFixCancelled` — UAC declined (ERROR_CANCELLED 1223 from ShellExecute)
- `DriverFixFailed { exit_code }` — wdi-simple returned non-zero
- `DriverFixIneffective` — re-detection still fails after wdi-simple succeeds
- `BundleError(String)` — extraction or Win32 surface error

## Build, test, ship

**Project layout**:
```
stlink-tool/
├── Cargo.toml
├── build.rs                                # embed Windows manifest (asInvoker)
├── vendor/                                 # committed; ~8 MB total
│   ├── wdi-simple.exe                      # 4.6 MB, our CI build
│   └── xpack-openocd-0.12.0-7-win32-x64.zip  # 3.2 MB
├── src/{main,app,bundle,device,driver_fix,error,flasher,watcher}.rs
├── .github/workflows/{ci,release,build-wdi-simple}.yml
└── docs/superpowers/specs/2026-05-05-stlink-tool-design.md
```

**Compile model**: source written on macOS, compiled in CI on `windows-latest` (release.yml). Tag a `v*` and the GitHub Release auto-populates with `stlink-tool-windows-x64.exe`. No local Windows build environment required to ship.

**Test layers**:
1. **Unit (cross-platform)** — `cargo test` runs on macOS/Linux/Windows. Covers extension detection, file size validation, OpenOCD log parsing → error mapping. Inline `#[cfg(test)] mod tests` in `flasher.rs` and `error.rs`. Wired to GitHub Actions via ci.yml.
2. **Manual integration (Windows + real ST-Link)** — three matrices:
   - **Driver state**: never-installed × wrong-driver-bound × correctly-bound
   - **File type**: `.hex` × `.bin` × `.elf`
   - **ST-Link variant**: V2 clone (PID 0x3748) × V2-1 Nucleo (PID 0x374B) × V3 if available

**Distribution**: GitHub Releases, single `.exe`. Unsigned in v0.2 (SmartScreen "unknown publisher" warning is acceptable for a developer tool); v0.5 will add code signing.

**License**: stlink-tool dual MIT/Apache-2.0. wdi-simple (LGPLv3) and OpenOCD (GPLv2) invoked as separate processes — no static linking, no copyleft contamination of our source.

## Roadmap

- ~~**v0.1.0**~~ (released) — F10x, drag-and-drop, semi-automated driver fix (Zadig click)
- **v0.2.0 (current)** — Self-built `wdi-simple.exe`; fully silent driver fix
- **v0.3.0** — STM32F4 series support (target/stm32f4x.cfg)
- **v0.4.0** — F7, H7, U5, N6 series; chip auto-detection from IDCODE
- **v0.4.0** — VCP driver auto-install (after license clarification)
- **v0.5.0** — Code signing certificate; remove SmartScreen warning
