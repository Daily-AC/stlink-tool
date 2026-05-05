//! Driver-fix subsystem.
//!
//! v0.1.0: bundle Zadig + a preset zadig.ini, launch via `ShellExecute("runas")`
//! so Zadig comes up elevated with WinUSB pre-selected. User clicks
//! "Replace Driver" once, Zadig exits on success (per `exit_on_success=true`
//! in zadig.ini), we re-detect.
//!
//! v0.2.0 path: replace this with a self-built wdi-simple.exe for fully silent
//! installation — no INI, no GUI click.

use crate::bundle::Bundle;
use crate::error::FlashError;

#[cfg(windows)]
pub fn launch_zadig_blocking(bundle: &Bundle) -> Result<(), FlashError> {
    use std::os::windows::ffi::OsStrExt;

    use windows::core::PCWSTR;
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{WaitForSingleObject, INFINITE};
    use windows::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW};
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    fn to_wide(s: &std::ffi::OsStr) -> Vec<u16> {
        s.encode_wide().chain(std::iter::once(0)).collect()
    }

    let verb = to_wide(std::ffi::OsStr::new("runas"));
    let exe = to_wide(bundle.zadig_exe.as_os_str());
    let cwd = to_wide(bundle.zadig_cwd.as_os_str());

    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS,
        lpVerb: PCWSTR(verb.as_ptr()),
        lpFile: PCWSTR(exe.as_ptr()),
        lpDirectory: PCWSTR(cwd.as_ptr()),
        nShow: SW_SHOWNORMAL.0,
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

    unsafe {
        WaitForSingleObject(info.hProcess, INFINITE);
        let _ = CloseHandle(info.hProcess);
    }
    Ok(())
}

#[cfg(not(windows))]
pub fn launch_zadig_blocking(_bundle: &Bundle) -> Result<(), FlashError> {
    Err(FlashError::BundleError(
        "driver fix is Windows-only (this is a dev build on a non-Windows host)".into(),
    ))
}
