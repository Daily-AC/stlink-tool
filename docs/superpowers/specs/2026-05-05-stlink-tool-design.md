---
title: stlink-tool v0.1.0 design
date: 2026-05-05
status: locked
---

# stlink-tool — Design Spec

A Windows drag-and-drop flasher for STM32F10x with self-healing ST-Link WinUSB driver. Goal: collapse the embedded developer's "compile → find flasher → install/repair driver → click → wait" inner loop into a single drop.

## Context

ST's stack on Windows is famously punishing for new developers and clean machines: ST-Link USB driver is a separate ~30 MB download, the WinUSB binding required by OpenOCD/pyOCD must be installed via Zadig, the official ST tools and the open-source tools want different driver bindings, and corruption (yellow-bang in Device Manager) is common after Windows updates. STM32CubeProgrammer is 600 MB and license-restricted — too heavy and too closed to embed.

**Constraint**: ship fast. v0.1.0 targets only the **STM32F10x family** (specifically the F103C8T6 "Blue Pill") so we can hard-code the OpenOCD target config and avoid the multi-chip configuration matrix.

## Architecture

Single Rust binary (~12 MB) embedding two Windows tools:

```
stlink-tool.exe
├── embed: vendor/zadig-2.9.exe                   (5.3 MB, libwdi v1.5.1)
├── embed: vendor/xpack-openocd-0.12.0-7.zip      (3.2 MB, GPLv2)
├── embed: resources/zadig.ini                    (preset: WinUSB, exit_on_success)
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

## User pipeline

```
Idle ── drop .hex/.bin/.elf ──▶ Validating ──▶ Detecting
                                                  │
            ┌─────────────────────────────────────┤
            │                                     │
        no device                          NeedsDriverFix
            │                                     │
        Error                            AwaitingDriverFix
                                                  │
                                    [user clicks Fix Driver]
                                                  │
                                    Zadig opens (UAC prompt)
                                                  │
                                    [user clicks Replace Driver]
                                                  │
                                          Zadig auto-exits
                                                  │
                                            re-detect
                                                  │
                                              Ready
                                                  │
                                            Flashing ──▶ Done
```

**File handling** (in `flasher::build_args`):
- `.hex` and `.elf`: `program {file} verify reset exit` (addresses come from the file)
- `.bin`: `program {file} 0x08000000 verify reset exit` (F10x flash base)

OpenOCD command line:
```
openocd.exe -s <bundle>/scripts -f interface/stlink.cfg -f target/stm32f1x.cfg -c "<prog_cmd>"
```

**Auto-reflash**: a `notify` watcher on the dropped file's parent directory. When the canonical filename gets a Modify/Create event AND the app is in `Idle | Done | Error`, the pipeline re-fires automatically. Default-on; toggle visible in the UI.

## Driver-fix subsystem

**v0.1.0 strategy** — bundle Zadig 2.9 (the official libwdi-based GUI) plus a preset `zadig.ini`:

```ini
[general]  exit_on_success = true ; advanced_mode = false
[device]   list_all = true
[driver]   default_driver = 0     ; WinUSB
```

**Triggering**: `nusb::DeviceInfo::open()` requires WinUSB binding on Windows. If that fails, we know the driver is wrong → state transitions to `AwaitingDriverFix`. UI shows a "Fix Driver" button. Click → `ShellExecuteExW` with `runas` verb launches Zadig in the bundle dir (so it picks up our `zadig.ini`). UAC prompts; on confirm, Zadig opens with WinUSB pre-selected. User clicks "Replace Driver"; Zadig auto-exits via `exit_on_success=true`. The app re-runs detection; on success, flashing proceeds.

**Why not silent**: Zadig has no headless / `--vid` / `--pid` CLI flags (verified by reading `zadig.c` in libwdi master). The only path to fully silent install is building `wdi-simple.exe` from libwdi sources, which requires Visual Studio Build Tools + WDK — deferred to v0.2.

**Why not VCP auto-install**: ST's Virtual COM Port driver redistribution license is unclear. Win10 1709+ binds the V2-1 / V3 VCP interface to the built-in `usbser.sys` automatically in 95% of cases. v0.1 doesn't auto-install ST's VCP driver; if the user's machine is the rare 5% that needs it, the log surfaces a one-line hint.

**libwdi signing**: Zadig ships with Microsoft OEM category 0 signature, which Windows 10/11 honor for USB driver installation. No additional signing infrastructure needed on our side.

**Error model** (`FlashError`):
- `DriverFixCancelled` — UAC declined (ERROR_CANCELLED 1223 from ShellExecute)
- `DriverFixIneffective` — re-detection still fails after Zadig exits (suggests unplug/replug)
- `BundleError(String)` — extraction or Win32 surface error

## Build, test, ship

**Project layout**:
```
stlink-tool/
├── Cargo.toml
├── build.rs                   # embed Windows manifest (asInvoker)
├── vendor/                    # committed; ~9 MB
├── resources/zadig.ini
├── src/{main,app,bundle,device,driver_fix,error,flasher,watcher}.rs
└── docs/superpowers/specs/2026-05-05-stlink-tool-design.md
```

**Compile model**: source written on macOS, compiled on Windows (`cargo build --release`). No cross-compilation in v0.1 — `nusb` and `windows-sys` would require non-trivial mingw plumbing; not worth the time given the developer has Windows access.

**Test layers**:
1. **Unit (cross-platform)** — `cargo test` runs on macOS/Linux. Covers extension detection, file size validation, OpenOCD log parsing → error mapping. Inline `#[cfg(test)] mod tests` in `flasher.rs` and `error.rs`.
2. **Manual integration (Windows)** — three matrices on the developer's Windows machine:
   - **Driver state**: never-installed × wrong-driver-bound × correctly-bound
   - **File type**: `.hex` × `.bin` × `.elf`
   - **ST-Link variant**: V2 clone (PID 0x3748) × V2-1 Nucleo (PID 0x374B) × V3 if available

**Distribution**: GitHub Releases, single `.exe`. v0.1 unsigned (SmartScreen "unknown publisher" warning is acceptable for a developer tool). README documents the first-launch UAC prompt as expected.

**License**: stlink-tool dual MIT/Apache-2.0. Zadig (GPLv3) and OpenOCD (GPLv2) invoked as separate processes — no static linking, no copyleft contamination of our source.

## Roadmap

- **v0.2** — Self-built `wdi-simple.exe` (one-time Windows build with VS + WDK), replacing Zadig for fully silent driver fix
- **v0.2** — STM32F4 series support (target/stm32f4x.cfg)
- **v0.3** — F7, H7, U5, N6 series; chip auto-detection from IDCODE
- **v0.3** — VCP driver auto-install (after license clarification)
- **v0.4** — Code signing certificate; remove SmartScreen warning
