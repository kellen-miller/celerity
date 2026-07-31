#![cfg_attr(target_arch = "arm", no_std)]
#![cfg_attr(target_arch = "arm", no_main)]

#[cfg(target_arch = "arm")]
use panic_halt as _;

#[cfg(target_arch = "arm")]
embassy_stm32::bind_interrupts!(struct Irqs {
    FDCAN1_IT0 => embassy_stm32::can::IT0InterruptHandler<embassy_stm32::peripherals::FDCAN1>;
    FDCAN1_IT1 => embassy_stm32::can::IT1InterruptHandler<embassy_stm32::peripherals::FDCAN1>;
});

#[cfg(target_arch = "arm")]
#[embassy_executor::main(
    executor = "embassy_stm32::executor::Executor",
    entry = "cortex_m_rt::entry"
)]
async fn main(_spawner: embassy_executor::Spawner) {
    use celerity_protocol::{
        CommandAck, ConfigurationAck, FallbackAck, FaultReport, Frame, Heartbeat, RuntimeLeaseAck,
        decode, encode,
    };
    use duct_controller::{ControllerState, FirmwareConfiguration, FirmwareMode};
    use embassy_stm32::can::filter::{Action, Filter, FilterType, StandardFilterSlot};
    use embassy_stm32::can::{CanConfigurator, OperatingMode};
    use embassy_stm32::gpio::{Level, Output, OutputType, Speed};
    use embassy_stm32::time::Hertz;
    use embassy_stm32::timer::low_level::CountingMode;
    use embassy_stm32::timer::simple_pwm::{PwmPin, SimplePwm};
    use embassy_stm32::wdg::IndependentWatchdog;
    use embassy_time::{Duration, Instant, with_timeout};
    use embedded_can::{Id, StandardId};

    const NODE_ADDRESS: u8 = 1;
    const BOOT_SESSION: u32 = 1;
    const PWM_PERIOD_US: u32 = 20_000;

    let peripherals = embassy_stm32::init(Default::default());

    // PB0 is the installed-carrier contract for the active-high TCAN1044A-Q1
    // standby input. Its external pull-up keeps the bus transceiver recessive
    // through reset and until the local fallback configuration is active.
    let mut transceiver_standby = Output::new(peripherals.PB0, Level::High, Speed::Low);
    let pwm_pin = PwmPin::new(peripherals.PA0, OutputType::PushPull);
    let mut pwm = SimplePwm::new(
        peripherals.TIM2,
        Some(pwm_pin),
        None,
        None,
        None,
        Hertz::hz(50),
        CountingMode::EdgeAlignedUp,
    );
    let mut pwm_channel = pwm.ch1();
    pwm_channel.set_duty_cycle_fully_off();

    // The compiled generation is the recovery record used when neither flash
    // slot contains a complete commissioned record. A valid CAN Configuration
    // replaces it only while the controller remains in local fallback.
    let mut state = ControllerState::new(BOOT_SESSION, 7_500);
    let _configured = state.configure(FirmwareConfiguration {
        generation: 1,
        fallback_basis_points: 7_500,
        pwm_endpoint_a_us: 1_000,
        pwm_endpoint_b_us: 2_000,
        direction: 1,
        runtime_lease_ms: 500,
        command_lease_ms: 150,
    });
    pwm_channel.set_duty_cycle_fraction(u32::from(state.pwm_microseconds()), PWM_PERIOD_US);
    pwm_channel.enable();

    let mut can_configurator =
        CanConfigurator::new(peripherals.FDCAN1, peripherals.PA11, peripherals.PA12, Irqs);
    can_configurator.set_bitrate(500_000);
    can_configurator.set_fd_data_bitrate(2_000_000, true);
    let can_configuration = can_configurator
        .config()
        .set_global_filter(embassy_stm32::can::config::GlobalFilter::reject_all());
    can_configurator.set_config(can_configuration);
    let mut can = can_configurator.start(OperatingMode::NormalOperationMode);
    for (slot, can_id) in [0x080, 0x101, 0x181, 0x241, 0x2c1].into_iter().enumerate() {
        let id = StandardId::new(can_id).expect("fixed controller filter ID");
        can.properties().set_standard_filter(
            StandardFilterSlot::from(u8::try_from(slot).expect("five filter slots")),
            Filter {
                filter: FilterType::DedicatedSingle(id),
                action: Action::StoreInFifo0,
            },
        );
    }

    let mut watchdog = IndependentWatchdog::new(peripherals.IWDG, 250_000);
    watchdog.unleash();
    transceiver_standby.set_low();

    let mut acknowledgement_sequence = 0_u32;
    let mut heartbeat_sequence = 0_u32;
    let mut fault_sequence = 0_u32;
    let mut next_heartbeat_ms = 100_u64;
    loop {
        let now_ms = Instant::now().as_millis();
        state.advance_to(now_ms);
        pwm_channel.set_duty_cycle_fraction(u32::from(state.pwm_microseconds()), PWM_PERIOD_US);
        watchdog.pet();

        if now_ms >= next_heartbeat_ms {
            heartbeat_sequence = heartbeat_sequence.wrapping_add(1);
            next_heartbeat_ms = now_ms.saturating_add(100);
            let heartbeat = Frame::Heartbeat {
                node: NODE_ADDRESS,
                message: Heartbeat {
                    boot_session: BOOT_SESSION,
                    configuration_generation: state.configuration_generation().unwrap_or(0),
                    capability_generation: 1,
                    heartbeat_sequence,
                    current_epoch: state.current_epoch().unwrap_or(0),
                    last_command_sequence: state.last_command_sequence().unwrap_or(0),
                    accepted_basis_points: state.accepted_basis_points(),
                    state_flags: if state.mode() == FirmwareMode::RemoteAuthority {
                        2
                    } else {
                        1
                    },
                },
            };
            let mut payload = [0_u8; 64];
            if let Ok(encoded) = encode(&heartbeat, &mut payload)
                && let Some(id) = StandardId::new(encoded.can_id)
                && let Some(frame) =
                    <embassy_stm32::can::frame::FdFrame as embedded_can::Frame>::new(
                        id,
                        &payload[..encoded.len],
                    )
            {
                let _superseded = can.write_fd(&frame).await;
            }
        }

        let envelope = match with_timeout(Duration::from_millis(10), can.read_fd()).await {
            Ok(Ok(envelope)) => envelope,
            Err(_) => continue,
            Ok(Err(_bus_error)) => {
                fault_sequence = fault_sequence.wrapping_add(1);
                let fault = Frame::FaultReport {
                    node: NODE_ADDRESS,
                    message: FaultReport {
                        boot_session: BOOT_SESSION,
                        fault_sequence,
                        fault_code: 1,
                        severity: 2,
                        flags: 0,
                        related_epoch: state.current_epoch().unwrap_or(0),
                        related_command_sequence: state.last_command_sequence().unwrap_or(0),
                    },
                };
                let mut payload = [0_u8; 64];
                if let Ok(encoded) = encode(&fault, &mut payload)
                    && let Some(id) = StandardId::new(encoded.can_id)
                    && let Some(frame) =
                        <embassy_stm32::can::frame::FdFrame as embedded_can::Frame>::new(
                            id,
                            &payload[..encoded.len],
                        )
                {
                    let _superseded = can.write_fd(&frame).await;
                }
                continue;
            }
        };
        let received = envelope.frame;
        let can_id = match received.id() {
            Id::Standard(id) => id.as_raw(),
            Id::Extended(_) => continue,
        };
        let Ok(frame) = decode(can_id, received.data()) else {
            continue;
        };
        let now_ms = Instant::now().as_millis();

        let response = match frame {
            Frame::Configuration { node, message } if node == NODE_ADDRESS => {
                let accepted = message.boot_session == BOOT_SESSION
                    && state.configure(FirmwareConfiguration {
                        generation: message.configuration_generation,
                        fallback_basis_points: message.fallback_basis_points,
                        pwm_endpoint_a_us: message.pwm_endpoint_a_us,
                        pwm_endpoint_b_us: message.pwm_endpoint_b_us,
                        direction: message.direction,
                        runtime_lease_ms: message.runtime_lease_ms,
                        command_lease_ms: message.command_lease_ms,
                    });
                Some(Frame::ConfigurationAck {
                    node,
                    message: ConfigurationAck {
                        boot_session: BOOT_SESSION,
                        configuration_generation: message.configuration_generation,
                        digest_prefix: message.digest_prefix,
                        result: if accepted { 1 } else { 2 },
                    },
                })
            }
            Frame::RuntimeLease { node, message } if node == NODE_ADDRESS => {
                let accepted = state.accept_runtime_lease(
                    now_ms,
                    message.boot_session,
                    message.configuration_generation,
                    message.epoch,
                    message.renewal_sequence,
                    message.validity_ms,
                );
                Some(Frame::RuntimeLeaseAck {
                    node,
                    message: RuntimeLeaseAck {
                        boot_session: BOOT_SESSION,
                        configuration_generation: message.configuration_generation,
                        epoch: message.epoch,
                        renewal_sequence: message.renewal_sequence,
                        result: if accepted { 1 } else { 2 },
                    },
                })
            }
            Frame::Command { node, message } if node == NODE_ADDRESS => {
                let accepted = state.accept_command(
                    now_ms,
                    message.boot_session,
                    message.configuration_generation,
                    message.epoch,
                    message.command_sequence,
                    message.radiator_split_basis_points,
                );
                acknowledgement_sequence = acknowledgement_sequence.wrapping_add(1);
                Some(Frame::CommandAck {
                    node,
                    message: CommandAck {
                        boot_session: BOOT_SESSION,
                        configuration_generation: message.configuration_generation,
                        epoch: message.epoch,
                        command_sequence: message.command_sequence,
                        accepted_basis_points: state.accepted_basis_points(),
                        mode: if state.mode() == FirmwareMode::RemoteAuthority {
                            2
                        } else {
                            1
                        },
                        fault_latch: 1,
                        result: if accepted { 1 } else { 2 },
                        output_state: 1,
                        command_lease_remaining_ms: state.command_lease_remaining_ms(now_ms),
                        acknowledgement_sequence,
                    },
                })
            }
            Frame::FallbackRequest { node, message } if node == NODE_ADDRESS => {
                state.revoke_authority();
                Some(Frame::FallbackAck {
                    node,
                    message: FallbackAck {
                        boot_session: BOOT_SESSION,
                        request_sequence: message.request_sequence,
                        accepted_basis_points: state.accepted_basis_points(),
                        mode: 1,
                        fault_latch: 1,
                        result: 1,
                    },
                })
            }
            _ => None,
        };

        if let Some(response) = response {
            let mut payload = [0_u8; 64];
            if let Ok(encoded) = encode(&response, &mut payload)
                && let Some(id) = StandardId::new(encoded.can_id)
                && let Some(frame) =
                    <embassy_stm32::can::frame::FdFrame as embedded_can::Frame>::new(
                        id,
                        &payload[..encoded.len],
                    )
            {
                let _superseded = can.write_fd(&frame).await;
            }
        }
    }
}

#[cfg(not(target_arch = "arm"))]
fn main() {}
