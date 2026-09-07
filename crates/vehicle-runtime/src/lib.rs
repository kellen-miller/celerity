//! Host adapters and read-only diagnostics for Celerity binaries.

#[cfg(target_os = "linux")]
mod live;

#[cfg(target_os = "linux")]
pub use live::LiveRuntime;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LiveNetdeviceError {
    SameInterface,
    Query(String),
    InvalidJson(String),
    InterfaceMismatch,
    LinkDown,
    NotClassicalCan,
    WrongBitrate,
    NotListenOnly,
    AutomaticRestartEnabled,
    NotCanFd,
    WrongDataBitrate,
    BusNotQualified,
}

/// Verifies the pre-provisioned actuator CAN FD bus without mutating it.
///
/// # Errors
///
/// Returns a typed error unless the interface is up and error-active with
/// ISO CAN FD configured for 500 kbit/s arbitration, 2 Mbit/s data, BRS, and
/// deliberate (zero automatic) bus-off restart.
pub fn verify_actuator_netdevice(
    expected_actuator: &str,
    powertrain_interface: &str,
    details_json: &str,
) -> Result<(), LiveNetdeviceError> {
    if expected_actuator == powertrain_interface {
        return Err(LiveNetdeviceError::SameInterface);
    }
    let value: serde_json::Value = serde_json::from_str(details_json)
        .map_err(|error| LiveNetdeviceError::InvalidJson(error.to_string()))?;
    let link = value
        .as_array()
        .and_then(|items| items.first())
        .ok_or_else(|| LiveNetdeviceError::InvalidJson("missing link object".to_owned()))?;
    if link.get("ifname").and_then(serde_json::Value::as_str) != Some(expected_actuator) {
        return Err(LiveNetdeviceError::InterfaceMismatch);
    }
    if link.get("operstate").and_then(serde_json::Value::as_str) != Some("UP") {
        return Err(LiveNetdeviceError::LinkDown);
    }
    let data = link
        .pointer("/linkinfo/info_data")
        .ok_or_else(|| LiveNetdeviceError::InvalidJson("missing CAN details".to_owned()))?;
    if data
        .pointer("/bittiming/bitrate")
        .or_else(|| data.get("bitrate"))
        .and_then(serde_json::Value::as_u64)
        != Some(500_000)
    {
        return Err(LiveNetdeviceError::WrongBitrate);
    }
    if data
        .pointer("/data_bittiming/bitrate")
        .or_else(|| data.get("dbitrate"))
        .and_then(serde_json::Value::as_u64)
        != Some(2_000_000)
    {
        return Err(LiveNetdeviceError::WrongDataBitrate);
    }
    let modes = data
        .get("ctrlmode")
        .and_then(serde_json::Value::as_array)
        .ok_or(LiveNetdeviceError::NotCanFd)?;
    if !modes.iter().any(|mode| mode.as_str() == Some("FD")) {
        return Err(LiveNetdeviceError::NotCanFd);
    }
    if data.get("restart_ms").and_then(serde_json::Value::as_u64) != Some(0) {
        return Err(LiveNetdeviceError::AutomaticRestartEnabled);
    }
    if data.get("state").and_then(serde_json::Value::as_str) != Some("ERROR-ACTIVE") {
        return Err(LiveNetdeviceError::BusNotQualified);
    }
    Ok(())
}

/// Verifies the real powertrain link attributes returned by
/// `ip -details -json link show dev <interface>` before any socket is opened.
///
/// # Errors
///
/// Returns a typed error unless the named interface is up, Classical CAN at
/// 1 Mbit/s, listen-only, restart-ms zero, and distinct from the actuator bus.
pub fn verify_powertrain_netdevice(
    expected_powertrain: &str,
    actuator_interface: &str,
    details_json: &str,
) -> Result<(), LiveNetdeviceError> {
    if expected_powertrain == actuator_interface {
        return Err(LiveNetdeviceError::SameInterface);
    }
    let value: serde_json::Value = serde_json::from_str(details_json)
        .map_err(|error| LiveNetdeviceError::InvalidJson(error.to_string()))?;
    let link = value
        .as_array()
        .and_then(|items| items.first())
        .ok_or_else(|| LiveNetdeviceError::InvalidJson("missing link object".to_owned()))?;
    if link.get("ifname").and_then(serde_json::Value::as_str) != Some(expected_powertrain) {
        return Err(LiveNetdeviceError::InterfaceMismatch);
    }
    if link.get("operstate").and_then(serde_json::Value::as_str) != Some("UP") {
        return Err(LiveNetdeviceError::LinkDown);
    }
    if link
        .pointer("/linkinfo/info_kind")
        .and_then(serde_json::Value::as_str)
        != Some("can")
    {
        return Err(LiveNetdeviceError::NotClassicalCan);
    }
    let data = link
        .pointer("/linkinfo/info_data")
        .ok_or_else(|| LiveNetdeviceError::InvalidJson("missing CAN details".to_owned()))?;
    let bitrate = data
        .pointer("/bittiming/bitrate")
        .or_else(|| data.get("bitrate"))
        .and_then(serde_json::Value::as_u64);
    if bitrate != Some(1_000_000) {
        return Err(LiveNetdeviceError::WrongBitrate);
    }
    let listen_only = data
        .get("ctrlmode")
        .and_then(serde_json::Value::as_array)
        .is_some_and(|modes| {
            modes
                .iter()
                .any(|mode| mode.as_str() == Some("LISTEN-ONLY"))
        });
    if !listen_only {
        return Err(LiveNetdeviceError::NotListenOnly);
    }
    if data.get("restart_ms").and_then(serde_json::Value::as_u64) != Some(0) {
        return Err(LiveNetdeviceError::AutomaticRestartEnabled);
    }
    Ok(())
}

#[cfg(target_os = "linux")]
/// Queries and verifies the production powertrain netdevice before binding.
///
/// # Errors
///
/// Returns a typed query or qualification failure.
pub fn query_and_verify_powertrain_netdevice(
    powertrain_interface: &str,
    actuator_interface: &str,
) -> Result<(), LiveNetdeviceError> {
    let output = std::process::Command::new("ip")
        .args([
            "-details",
            "-json",
            "link",
            "show",
            "dev",
            powertrain_interface,
        ])
        .output()
        .map_err(|error| LiveNetdeviceError::Query(error.to_string()))?;
    if !output.status.success() {
        return Err(LiveNetdeviceError::Query(
            String::from_utf8_lossy(&output.stderr).into_owned(),
        ));
    }
    verify_powertrain_netdevice(
        powertrain_interface,
        actuator_interface,
        &String::from_utf8_lossy(&output.stdout),
    )
}

#[cfg(target_os = "linux")]
/// Queries and verifies the production actuator netdevice before binding.
///
/// # Errors
///
/// Returns a typed query or qualification failure.
pub fn query_and_verify_actuator_netdevice(
    actuator_interface: &str,
    powertrain_interface: &str,
) -> Result<(), LiveNetdeviceError> {
    let output = std::process::Command::new("ip")
        .args([
            "-details",
            "-statistics",
            "-json",
            "link",
            "show",
            "dev",
            actuator_interface,
        ])
        .output()
        .map_err(|error| LiveNetdeviceError::Query(error.to_string()))?;
    if !output.status.success() {
        return Err(LiveNetdeviceError::Query(
            String::from_utf8_lossy(&output.stderr).into_owned(),
        ));
    }
    verify_actuator_netdevice(
        actuator_interface,
        powertrain_interface,
        &String::from_utf8_lossy(&output.stdout),
    )
}

#[cfg(target_os = "linux")]
pub struct PowertrainCanReceiver {
    socket: socketcan::CanSocket,
}

#[cfg(target_os = "linux")]
impl PowertrainCanReceiver {
    /// Opens a receive-only application surface with error-frame and arrival
    /// timestamp evidence enabled. The type intentionally exposes no send API.
    ///
    /// # Errors
    ///
    /// Returns an error when netdevice verification, socket binding, error
    /// filtering, or timestamp configuration fails.
    pub fn open(powertrain_interface: &str, actuator_interface: &str) -> Result<Self, String> {
        query_and_verify_powertrain_netdevice(powertrain_interface, actuator_interface)
            .map_err(|error| format!("{error:?}"))?;
        Self::bind_prequalified(powertrain_interface)
    }

    pub(crate) fn bind_prequalified(powertrain_interface: &str) -> Result<Self, String> {
        use nix::sys::socket::{setsockopt, sockopt};
        use socketcan::{Socket, SocketOptions};

        let socket =
            socketcan::CanSocket::open(powertrain_interface).map_err(|error| error.to_string())?;
        socket
            .set_error_filter_accept_all()
            .map_err(|error| error.to_string())?;
        socket
            .set_recv_timestamp(true)
            .map_err(|error| error.to_string())?;
        socket
            .set_timestamping(
                socketcan::SOF_TIMESTAMPING_RX_HARDWARE
                    | socketcan::SOF_TIMESTAMPING_RAW_HARDWARE
                    | socketcan::SOF_TIMESTAMPING_RX_SOFTWARE
                    | socketcan::SOF_TIMESTAMPING_SOFTWARE
                    | socketcan::SOF_TIMESTAMPING_OPT_CMSG,
            )
            .map_err(|error| error.to_string())?;
        setsockopt(&socket, sockopt::RxqOvfl, &1).map_err(|error| error.to_string())?;
        Ok(Self { socket })
    }

    /// Receives one frame with its kernel arrival timestamp.
    ///
    /// # Errors
    ///
    /// Returns an error when the kernel receive operation or timestamp
    /// ancillary-data decoding fails.
    pub fn receive(&self) -> Result<ReceivedCanFrame, String> {
        use socketcan::{EmbeddedFrame, Frame, Socket};

        let (frame, timestamps) = self
            .socket
            .read_frame_with_timestamps()
            .map_err(|error| error.to_string())?;
        let (timestamp_ns, timestamp_source) = if let Some(timestamp) = timestamps.hw {
            (timestamp.as_nanos(), TimestampSource::RawHardware)
        } else {
            let timestamp = timestamps
                .sw
                .or(timestamps.socket)
                .ok_or_else(|| "kernel supplied no receive timestamp".to_owned())?;
            (
                timestamp
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_err(|error| error.to_string())?
                    .as_nanos(),
                TimestampSource::SoftwareFallback,
            )
        };
        Ok(ReceivedCanFrame {
            id: frame.raw_id(),
            data: frame.data().to_vec(),
            timestamp_ns: u64::try_from(timestamp_ns).unwrap_or(u64::MAX),
            timestamp_source,
        })
    }

    pub(crate) fn borrowed_fd(&self) -> std::os::fd::BorrowedFd<'_> {
        use std::os::fd::AsFd;

        self.socket.as_fd()
    }
}

#[cfg(target_os = "linux")]
pub struct ActuatorCanTransport {
    socket: socketcan::CanFdSocket,
}

#[cfg(target_os = "linux")]
pub(crate) struct ReceivedControllerFrame {
    pub id: u32,
    pub data: Vec<u8>,
    pub fd: bool,
    pub bit_rate_switch: bool,
    pub decoded: Option<control_protocol::Frame>,
    pub ignored_reason: Option<String>,
}

#[cfg(target_os = "linux")]
impl ActuatorCanTransport {
    /// Opens the separately qualified controller CAN FD interface.
    ///
    /// # Errors
    ///
    /// Returns an error for qualification, binding, or CAN error subscription.
    pub fn open(actuator_interface: &str, powertrain_interface: &str) -> Result<Self, String> {
        use socketcan::{Socket, SocketOptions};

        query_and_verify_actuator_netdevice(actuator_interface, powertrain_interface)
            .map_err(|error| format!("{error:?}"))?;
        let socket =
            socketcan::CanFdSocket::open(actuator_interface).map_err(|error| error.to_string())?;
        socket
            .set_error_filter_accept_all()
            .map_err(|error| error.to_string())?;
        Ok(Self { socket })
    }

    pub(crate) fn bind_prequalified(actuator_interface: &str) -> Result<Self, String> {
        use socketcan::{Socket, SocketOptions};

        let socket =
            socketcan::CanFdSocket::open(actuator_interface).map_err(|error| error.to_string())?;
        socket
            .set_error_filter_accept_all()
            .map_err(|error| error.to_string())?;
        Ok(Self { socket })
    }

    /// Writes one exact typed controller frame as ISO CAN FD with BRS enabled.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid encoding, IDs, or kernel transmission.
    pub fn send(&self, frame: &control_protocol::Frame) -> Result<(), String> {
        use socketcan::{EmbeddedFrame, Socket};

        let mut payload = [0_u8; 64];
        let encoded =
            control_protocol::encode(frame, &mut payload).map_err(|error| format!("{error:?}"))?;
        let id = socketcan::StandardId::new(encoded.can_id)
            .ok_or_else(|| "controller frame has invalid standard ID".to_owned())?;
        let mut frame = socketcan::CanFdFrame::new(id, &payload[..encoded.len])
            .ok_or_else(|| "controller frame has invalid CAN FD length".to_owned())?;
        frame.set_brs(true);
        self.socket
            .write_frame(&frame)
            .map_err(|error| error.to_string())
    }

    /// Reads one controller frame. CAN error frames and unknown protocol IDs
    /// are returned as evidence-only observations rather than link failures.
    ///
    /// # Errors
    ///
    /// Returns an error for kernel receive or invalid protocol payload.
    pub(crate) fn receive(&self) -> Result<ReceivedControllerFrame, String> {
        use socketcan::{EmbeddedFrame, Frame, Socket};

        let frame = self
            .socket
            .read_frame()
            .map_err(|error| error.to_string())?;
        let id = frame.raw_id();
        let data = frame.data().to_vec();
        let (decoded, ignored_reason) = if let socketcan::CanAnyFrame::Error(error) = frame {
            (None, Some(format!("CAN error {error:?}")))
        } else {
            match u16::try_from(frame.raw_id()) {
                Ok(id) => match control_protocol::decode(id, frame.data()) {
                    Ok(decoded) => (Some(decoded), None),
                    Err(error) => (None, Some(format!("unknown controller frame: {error:?}"))),
                },
                Err(error) => (
                    None,
                    Some(format!("controller frame ID is out of range: {error}")),
                ),
            }
        };
        Ok(ReceivedControllerFrame {
            id,
            data,
            fd: matches!(frame, socketcan::CanAnyFrame::Fd(_)),
            bit_rate_switch: matches!(frame, socketcan::CanAnyFrame::Fd(ref fd) if fd.is_brs()),
            decoded,
            ignored_reason,
        })
    }

    pub(crate) fn borrowed_fd(&self) -> std::os::fd::BorrowedFd<'_> {
        use std::os::fd::AsFd;

        self.socket.as_fd()
    }
}

#[cfg(target_os = "linux")]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReceivedCanFrame {
    pub id: u32,
    pub data: Vec<u8>,
    pub timestamp_ns: u64,
    pub timestamp_source: TimestampSource,
}

#[cfg(target_os = "linux")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TimestampSource {
    RawHardware,
    SoftwareFallback,
}

#[cfg(all(test, target_os = "linux"))]
mod linux_application_tests {
    use std::time::Duration;

    use socketcan::{CanFrame, CanSocket, EmbeddedFrame, Socket, StandardId};

    use super::{PowertrainCanReceiver, TimestampSource};

    #[test]
    fn production_receive_surface_never_transmits_to_either_vcan() {
        let receiver = PowertrainCanReceiver::bind_prequalified("vcan-powertrain")
            .expect("bind production receive-only surface");
        let powertrain_sender = CanSocket::open("vcan-powertrain").expect("powertrain sender");
        let controller_observer = CanSocket::open("vcan-controller").expect("controller observer");
        controller_observer
            .set_read_timeout(Duration::from_millis(50))
            .expect("bounded observation");
        let frame = CanFrame::new(StandardId::new(0x3e0).expect("standard ID"), &[0; 8])
            .expect("Classical CAN frame");
        powertrain_sender
            .write_frame(&frame)
            .expect("inject powertrain evidence");

        let evidence = receiver.receive().expect("production receive");
        assert_eq!(evidence.id, 0x3e0);
        assert_eq!(evidence.timestamp_source, TimestampSource::SoftwareFallback);
        let error = controller_observer
            .read_frame()
            .expect_err("receive-only application must not transmit on controller vcan");
        assert!(matches!(
            error.kind(),
            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
        ));
    }
}
