use std::{
    io,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use control_core::{ControllerEmulator, EmulatorProvisioning};
use control_protocol::{Frame, decode, encode};
use socketcan::{CanFdFrame, CanFdSocket, CanFrame, CanSocket, EmbeddedFrame, Frame as SocketCanFrame, Socket};
use vehicle_diagnostics::DiagnosticsStore;

use super::LiveTestResult;

pub(super) struct LiveWorkers {
    stopping: Arc<AtomicBool>,
    controller: Option<JoinHandle<LiveTestResult<()>>>,
    diagnostics: Option<JoinHandle<Result<(), String>>>,
}

impl LiveWorkers {
    pub(super) fn start(
        diagnostics_socket: &std::path::Path,
        diagnostics_store: DiagnosticsStore,
        observed_commands: Arc<Mutex<Vec<u16>>>,
    ) -> LiveTestResult<Self> {
        let stopping = Arc::new(AtomicBool::new(false));
        let diagnostics =
            vehicle_diagnostics::serve_diagnostics(diagnostics_socket, Arc::clone(&stopping), diagnostics_store)?;
        let mut workers = Self {
            stopping: Arc::clone(&stopping),
            controller: None,
            diagnostics: Some(diagnostics),
        };
        workers.controller = Some(spawn_controller(stopping, observed_commands)?);

        Ok(workers)
    }

    pub(super) fn stop(&mut self) -> LiveTestResult<()> {
        self.stopping.store(true, Ordering::Relaxed);
        let wakeup_result = if self.controller.is_some() {
            send_controller_wakeup()
        } else {
            Ok(())
        };
        let controller_result = self
            .controller
            .take()
            .map(JoinHandle::join)
            .transpose()
            .map_err(|_| io::Error::other("controller thread panicked"));
        let diagnostics_result = self
            .diagnostics
            .take()
            .map(JoinHandle::join)
            .transpose()
            .map_err(|_| io::Error::other("diagnostics thread panicked"));
        wakeup_result?;
        if let Some(result) = controller_result? {
            result?;
        }
        if let Some(result) = diagnostics_result? {
            result.map_err(io::Error::other)?;
        }

        Ok(())
    }
}

impl Drop for LiveWorkers {
    fn drop(&mut self) {
        if let Err(error) = self.stop() {
            eprintln!("live composition worker cleanup failed: {error}");
        }
    }
}

fn spawn_controller(
    stopping: Arc<AtomicBool>,
    observed_commands: Arc<Mutex<Vec<u16>>>,
) -> io::Result<JoinHandle<LiveTestResult<()>>> {
    thread::Builder::new()
        .name("celerity-test-controller".to_owned())
        .spawn(move || run_controller(stopping, observed_commands))
}

fn run_controller(stopping: Arc<AtomicBool>, observed_commands: Arc<Mutex<Vec<u16>>>) -> LiveTestResult<()> {
    let socket = CanFdSocket::open("vcan-controller")?;
    socket.set_read_timeout(Duration::from_millis(100))?;
    let mut emulator = ControllerEmulator::new(EmulatorProvisioning {
        node: 1,
        identity: 1,
        boot_session: 7,
        firmware_generation: 1,
        capability_generation: 1,
        fallback_basis_points: 9_000,
    });
    let started = std::time::Instant::now();
    while !stopping.load(Ordering::Relaxed) {
        let frame = match socket.read_frame() {
            Ok(frame) => frame,
            Err(error) if matches!(error.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut) => {
                continue;
            }
            Err(error) => return Err(error.into()),
        };
        let can_id = u16::try_from(frame.raw_id())?;
        let decoded = decode(can_id, frame.data()).map_err(|error| io::Error::other(format!("{error:?}")))?;
        if let Frame::Command { message, .. } = &decoded {
            observed_commands
                .lock()
                .map_err(|error| io::Error::other(error.to_string()))?
                .push(message.radiator_split_basis_points);
        }

        let now_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        let mut response = emulator
            .ingest(now_ms, can_id, frame.data())
            .map_err(|error| io::Error::other(format!("{error:?}")))?;
        while let Some(current) = response {
            write_fd(&socket, current.can_id, current.payload())?;
            response = current.follow_up().cloned();
        }
        if let Some(heartbeat) = emulator
            .heartbeat(now_ms)
            .map_err(|error| io::Error::other(format!("{error:?}")))?
        {
            write_fd(&socket, heartbeat.can_id, heartbeat.payload())?;
        }
    }

    Ok(())
}

fn write_fd(socket: &CanFdSocket, can_id: u16, payload: &[u8]) -> LiveTestResult<()> {
    let id = socketcan::StandardId::new(can_id).ok_or_else(|| io::Error::other("invalid standard CAN id"))?;
    let frame = CanFdFrame::new(id, payload).ok_or_else(|| io::Error::other("invalid CAN FD frame"))?;
    socket.write_frame(&frame)?;

    Ok(())
}

pub(super) fn send_powertrain_temperature(coolant_c: i16, iat_c: i16, nexus: bool) -> LiveTestResult<()> {
    let (id, coolant, iat) = if nexus {
        (0x600, (coolant_c * 10).to_be_bytes(), (iat_c * 10).to_be_bytes())
    } else {
        let coolant = u16::try_from(i32::from(coolant_c) * 10 + 2_731)?.to_be_bytes();
        let iat = u16::try_from(i32::from(iat_c) * 10 + 2_731)?.to_be_bytes();
        (0x3e0, coolant, iat)
    };
    send_powertrain_frame(id, &[coolant[0], coolant[1], iat[0], iat[1], 0, 0, 0, 0])
}

pub(super) fn send_powertrain_frame(can_id: u16, payload: &[u8]) -> LiveTestResult<()> {
    let socket = CanSocket::open("vcan-powertrain")?;
    let id = socketcan::StandardId::new(can_id).ok_or_else(|| io::Error::other("invalid standard CAN id"))?;
    let frame = CanFrame::new(id, payload).ok_or_else(|| io::Error::other("invalid Classical CAN frame"))?;
    socket.write_frame(&frame)?;

    Ok(())
}

fn send_controller_wakeup() -> LiveTestResult<()> {
    let socket = CanFdSocket::open("vcan-controller")?;
    let mut payload = [0_u8; 64];
    let encoded = encode(
        &Frame::DiscoveryProbe(control_protocol::DiscoveryProbe {
            protocol_major: 1,
            protocol_minor: 0,
            flags: 0,
            probe_sequence: 0,
        }),
        &mut payload,
    )
    .map_err(|error| io::Error::other(format!("{error:?}")))?;
    write_fd(&socket, encoded.can_id, &payload[..encoded.len])?;
    thread::sleep(Duration::from_millis(10));

    Ok(())
}
