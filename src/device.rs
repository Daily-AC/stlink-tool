//! ST-Link USB detection.
//!
//! v0.1.0 strategy: enumerate VID 0x0483 with one of the known ST-Link PIDs,
//! then attempt to *open* the device via nusb. On Windows nusb requires the
//! debug interface to be bound to WinUSB; if open fails, that's our signal
//! the driver needs fixing. This avoids hand-rolling SetupAPI driver-state
//! queries — open()-or-fail is the same check OpenOCD will do moments later.

use nusb::MaybeFuture;

use crate::error::FlashError;

const STLINK_VID: u16 = 0x0483;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StLinkVariant {
    V1,
    V2,
    V21,
    V21NoMSD,
    V3,
    V3NoMSD,
    V3DFU,
}

impl StLinkVariant {
    fn from_pid(pid: u16) -> Option<Self> {
        match pid {
            0x3744 => Some(Self::V1),
            0x3748 => Some(Self::V2),
            0x374B => Some(Self::V21),
            0x3752 => Some(Self::V21NoMSD),
            0x374D => Some(Self::V3DFU),
            0x374E | 0x374F | 0x3754 => Some(Self::V3),
            0x3753 => Some(Self::V3NoMSD),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::V1 => "ST-Link V1",
            Self::V2 => "ST-Link V2",
            Self::V21 => "ST-Link V2-1",
            Self::V21NoMSD => "ST-Link V2-1 (no MSD)",
            Self::V3 => "ST-Link V3",
            Self::V3NoMSD => "ST-Link V3 (no MSD)",
            Self::V3DFU => "ST-Link V3 (DFU mode)",
        }
    }
}

#[derive(Debug, Clone)]
pub struct StLinkInfo {
    pub vid: u16,
    pub pid: u16,
    pub variant: StLinkVariant,
    pub serial: Option<String>,
}

#[derive(Debug)]
pub enum DetectResult {
    None,
    NeedsDriverFix(StLinkInfo),
    Ready(StLinkInfo),
}

pub fn detect() -> Result<DetectResult, FlashError> {
    let mut found: Option<nusb::DeviceInfo> = None;

    let iter = nusb::list_devices()
        .wait()
        .map_err(|e| FlashError::UsbError(e.to_string()))?;
    for d in iter {
        if d.vendor_id() != STLINK_VID {
            continue;
        }
        if StLinkVariant::from_pid(d.product_id()).is_some() {
            found = Some(d);
            break;
        }
    }

    let Some(info) = found else {
        return Ok(DetectResult::None);
    };

    let summary = StLinkInfo {
        vid: info.vendor_id(),
        pid: info.product_id(),
        variant: StLinkVariant::from_pid(info.product_id()).unwrap(),
        serial: info.serial_number().map(|s| s.to_string()),
    };

    // Try to open. On Windows this only succeeds when the debug interface is
    // bound to WinUSB — a perfect proxy for "driver is in the right state".
    match info.open().wait() {
        Ok(_) => Ok(DetectResult::Ready(summary)),
        Err(_) => Ok(DetectResult::NeedsDriverFix(summary)),
    }
}
