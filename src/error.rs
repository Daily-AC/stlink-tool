//! Error types surfaced to the UI layer.
//!
//! Each variant carries enough information to render a single human-readable
//! line in the app's status area. The `Display` impl IS the user-facing copy.

use thiserror::Error;

#[derive(Error, Debug)]
pub enum FlashError {
    #[error("Unsupported file extension. Drop a .hex, .bin, or .elf file.")]
    BadExtension,

    #[error("File too large for STM32F10x flash ({size} bytes, max 128 KB).")]
    FileTooLarge { size: u64 },

    #[error("File is empty.")]
    FileEmpty,

    #[error("File no longer exists: {path}")]
    FileMissing { path: String },

    #[error("No ST-Link found. Plug in the debugger and try again.")]
    NoStlinkDevice,

    #[error("ST-Link driver missing or wrong — Zadig will open. Pick the ST-Link entry and click 'Replace Driver'.")]
    DriverFixNeeded,

    #[error("Driver fix was cancelled (UAC declined).")]
    DriverFixCancelled,

    #[error("Driver fix did not take effect. Try unplug/replug, then drop the file again.")]
    DriverFixIneffective,

    #[error("OpenOCD failed: {0}")]
    OpenocdFailed(String),

    #[error("Target IDCODE mismatch — this is not an STM32F10x chip. v0.1.0 only supports F10x.")]
    WrongChip,

    #[error("Bundled resource extraction failed: {0}")]
    BundleError(String),

    #[error("USB enumeration failed: {0}")]
    UsbError(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

impl FlashError {
    /// Best-effort categoriser of an OpenOCD non-zero exit. Looks at recent
    /// log lines to pick a more specific error than the generic OpenocdFailed.
    pub fn from_openocd_log(log: &[String]) -> Self {
        let tail: String = log.iter().rev().take(40).cloned().collect::<Vec<_>>().join("\n");
        if tail.contains("IDCODE") && (tail.contains("expected") || tail.contains("mismatch")) {
            return FlashError::WrongChip;
        }
        if tail.contains("Error:") {
            // Surface the first meaningful Error: line (skip the openocd banner)
            for line in log.iter().rev() {
                if let Some(rest) = line.split_once("Error:") {
                    return FlashError::OpenocdFailed(rest.1.trim().to_string());
                }
            }
        }
        FlashError::OpenocdFailed("see log for details".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(xs: &[&str]) -> Vec<String> {
        xs.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn idcode_mismatch_maps_to_wrong_chip() {
        let log = lines(&[
            "Open On-Chip Debugger 0.12.0",
            "Error: target IDCODE 0xdeadbeef expected one of stm32f1x ...",
        ]);
        assert!(matches!(FlashError::from_openocd_log(&log), FlashError::WrongChip));
    }

    #[test]
    fn surfaces_error_line() {
        let log = lines(&[
            "Info : Hardware thread awareness created",
            "Error: stm32x_erase_options failed",
        ]);
        let s = FlashError::from_openocd_log(&log).to_string();
        assert!(s.contains("stm32x_erase_options failed"), "got: {s}");
    }

    #[test]
    fn no_error_lines_falls_back() {
        let log = lines(&["Info : Listening on port 6666"]);
        let err = FlashError::from_openocd_log(&log);
        assert!(matches!(err, FlashError::OpenocdFailed(_)));
    }
}
