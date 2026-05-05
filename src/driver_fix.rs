//! Driver-fix subsystem.
//!
//! v0.2.0: bundle our own self-built `wdi-simple.exe` (compiled in CI from
//! libwdi v1.5.1 sources). Launch via `ShellExecute("runas")` with
//! `--silent` so it installs WinUSB on the matching VID/PID/IID without
//! showing any GUI. The only user interaction is the OS-level UAC prompt.

use crate::bundle::Bundle;
use crate::error::FlashError;

/// Returned by `wdi-simple --silent` on success.
const WDI_SUCCESS: u32 = 0;

#[cfg(windows)]
pub fn install_winusb_blocking(bundle: &Bundle, vid: u16, pid: u16) -> Result<(), FlashError> {
    use std::os::windows::ffi::OsStrExt;

    use windows::core::PCWSTR;
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{
        GetExitCodeProcess, WaitForSingleObject, INFINITE,
    };
    use windows::Win32::UI::Shell::{
        ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW,
    };
    use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;

    fn to_wide(s: impl AsRef<std::ffi::OsStr>) -> Vec<u16> {
        s.as_ref()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    // wdi-simple flags (verified against libwdi/examples/wdi-simple.c at v1.5.1):
    //   --vid <id>   --pid <id>   --iid <interface_index>
    //   --type 0     (WinUSB)
    //   --silent     (no prompts, suppresses console output)
    let args_str = format!(
        "--vid 0x{:04X} --pid 0x{:04X} --iid 0 --type 0 --silent --name \"ST-Link (stlink-tool)\"",
        vid, pid
    );

    let verb = to_wide("runas");
    let exe = to_wide(bundle.wdi_simple_exe.as_os_str());
    let args = to_wide(&args_str);

    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS,
        lpVerb: PCWSTR(verb.as_ptr()),
        lpFile: PCWSTR(exe.as_ptr()),
        lpParameters: PCWSTR(args.as_ptr()),
        nShow: SW_HIDE.0,
        ..Default::default()
    };

    unsafe {
        ShellExecuteExW(&mut info).map_err(|e| {
            // ERROR_CANCELLED (1223) means the user dismissed the UAC prompt.
            if e.code().0 as u32 == 0x800704C7 {
                FlashError::DriverFixCancelled
            } else {
                FlashError::BundleError(format!("ShellExecuteExW: {e}"))
            }
        })?;
    }

    if info.hProcess.is_invalid() {
        return Err(FlashError::DriverFixCancelled);
    }

    let exit_code = unsafe {
        WaitForSingleObject(info.hProcess, INFINITE);
        let mut code: u32 = 0;
        let _ = GetExitCodeProcess(info.hProcess, &mut code);
        let _ = CloseHandle(info.hProcess);
        code
    };

    match exit_code {
        WDI_SUCCESS => Ok(()),
        n => Err(FlashError::DriverFixFailed { exit_code: n }),
    }
}

#[cfg(not(windows))]
pub fn install_winusb_blocking(
    _bundle: &Bundle,
    _vid: u16,
    _pid: u16,
) -> Result<(), FlashError> {
    Err(FlashError::BundleError(
        "driver fix is Windows-only (this is a dev build on a non-Windows host)".into(),
    ))
}
