use std::{fs, path::PathBuf};

use super::{SyncError, sync_io};

pub trait NetworkPresence {
    /// Reports whether the configured link is up with the configured default route.
    ///
    /// # Errors
    ///
    /// Returns an error when platform network state cannot be read.
    fn is_home(&self, interface: &str, expected_gateway: &str) -> Result<bool, SyncError>;
}

/// Reads Linux link and default-route state. HTTP availability is deliberately
/// not part of this decision.
pub struct LinuxNetworkPresence {
    sys_class_net: PathBuf,
    proc_net_route: PathBuf,
}

impl LinuxNetworkPresence {
    #[must_use]
    pub fn host() -> Self {
        Self {
            sys_class_net: PathBuf::from("/sys/class/net"),
            proc_net_route: PathBuf::from("/proc/net/route"),
        }
    }

    #[must_use]
    pub const fn from_paths(sys_class_net: PathBuf, proc_net_route: PathBuf) -> Self {
        Self {
            sys_class_net,
            proc_net_route,
        }
    }
}

impl NetworkPresence for LinuxNetworkPresence {
    fn is_home(&self, interface: &str, expected_gateway: &str) -> Result<bool, SyncError> {
        let operstate = fs::read_to_string(self.sys_class_net.join(interface).join("operstate"))
            .map_err(sync_io)?;
        if operstate.trim() != "up" {
            return Ok(false);
        }
        let route = fs::read_to_string(&self.proc_net_route).map_err(sync_io)?;
        for line in route.lines().skip(1) {
            let columns: Vec<_> = line.split_ascii_whitespace().collect();
            if columns.len() < 4 || columns[0] != interface || columns[1] != "00000000" {
                continue;
            }
            let flags = u16::from_str_radix(columns[3], 16).unwrap_or_default();
            if flags & 0x3 != 0x3 {
                continue;
            }
            let encoded = u32::from_str_radix(columns[2], 16).unwrap_or_default();
            let gateway = encoded
                .to_le_bytes()
                .map(|octet| octet.to_string())
                .join(".");
            return Ok(gateway == expected_gateway);
        }
        Ok(false)
    }
}
