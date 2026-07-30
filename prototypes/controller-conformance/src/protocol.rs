use std::fmt;

pub const PROTOCOL_MAJOR: u8 = 1;
pub const PROTOCOL_MINOR: u8 = 0;
pub const NODE_ID: u16 = 0x002a;
pub const CAPABILITY_GENERATION: u32 = 3;
pub const INITIAL_CONFIG_GENERATION: u32 = 7;
pub const RUNTIME_LEASE_MS: u64 = 1_000;
pub const COMMAND_LEASE_MS: u64 = 200;
pub const ACK_FRESHNESS_MS: u64 = 250;
pub const HEARTBEAT_PERIOD_MS: u64 = 100;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Lifecycle {
    Initializing,
    Ready,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ActuationMode {
    Fallback,
    Commanded,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FaultLatch {
    Clear,
    Latched,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FeatureAuthority {
    Fallback,
    Arming,
    Active,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AckResult {
    Accepted,
    RejectedNotReady,
    RejectedFaultLatched,
    RejectedBootSession,
    RejectedEpoch,
    RejectedSequence,
    RejectedConfigGeneration,
    RejectedUnderAuthority,
    RejectedOutOfRange,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RadiatorFraction(u16);

impl RadiatorFraction {
    pub const FALLBACK: Self = Self(5_000);

    pub fn from_basis_points(basis_points: u16) -> Option<Self> {
        (basis_points <= 10_000).then_some(Self(basis_points))
    }

    pub fn basis_points(self) -> u16 {
        self.0
    }
}

impl fmt::Display for RadiatorFraction {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{:.2} ({:.0}%)",
            f64::from(self.0) / 10_000.0,
            f64::from(self.0) / 100.0
        )
    }
}

#[derive(Clone, Debug)]
pub enum Message {
    DiscoveryProbe {
        protocol_major: u8,
    },
    NodeAnnounce {
        node_id: u16,
        boot_session: u32,
        protocol_major: u8,
        protocol_minor: u8,
        firmware_generation: u32,
        capability_generation: u32,
        config_generation: u32,
        lifecycle: Lifecycle,
    },
    CapabilityReport {
        node_id: u16,
        boot_session: u32,
        capability_generation: u32,
        actuator: &'static str,
        minimum: RadiatorFraction,
        maximum: RadiatorFraction,
        physical_feedback: bool,
    },
    Configuration {
        node_id: u16,
        boot_session: u32,
        config_generation: u32,
        fallback: RadiatorFraction,
        runtime_lease_ms: u64,
        command_lease_ms: u64,
    },
    ConfigurationAck {
        node_id: u16,
        boot_session: u32,
        config_generation: u32,
        result: AckResult,
    },
    RuntimeLease {
        node_id: u16,
        boot_session: u32,
        epoch: u64,
        renewal_sequence: u32,
        config_generation: u32,
        valid_for_ms: u64,
    },
    RuntimeLeaseAck {
        node_id: u16,
        boot_session: u32,
        epoch: u64,
        renewal_sequence: u32,
        config_generation: u32,
        result: AckResult,
    },
    Command {
        node_id: u16,
        boot_session: u32,
        epoch: u64,
        command_sequence: u32,
        config_generation: u32,
        radiator_fraction: RadiatorFraction,
    },
    CommandAck {
        node_id: u16,
        boot_session: u32,
        epoch: u64,
        command_sequence: u32,
        config_generation: u32,
        accepted_fraction: RadiatorFraction,
        mode: ActuationMode,
        fault_latch: FaultLatch,
        result: AckResult,
    },
    Heartbeat {
        node_id: u16,
        boot_session: u32,
        lifecycle: Lifecycle,
        mode: ActuationMode,
        fault_latch: FaultLatch,
        config_generation: u32,
        epoch: Option<u64>,
        last_command_sequence: Option<u32>,
        accepted_fraction: RadiatorFraction,
        runtime_lease_remaining_ms: u64,
        command_lease_remaining_ms: u64,
    },
    FaultReport {
        node_id: u16,
        boot_session: u32,
        code: &'static str,
        latched: bool,
    },
    FallbackRequest {
        node_id: u16,
        boot_session: u32,
        epoch: u64,
    },
    FallbackAck {
        node_id: u16,
        boot_session: u32,
        mode: ActuationMode,
    },
}

impl Message {
    fn name(&self) -> &'static str {
        match self {
            Self::DiscoveryProbe { .. } => "DiscoveryProbe",
            Self::NodeAnnounce { .. } => "NodeAnnounce",
            Self::CapabilityReport { .. } => "CapabilityReport",
            Self::Configuration { .. } => "Configuration",
            Self::ConfigurationAck { .. } => "ConfigurationAck",
            Self::RuntimeLease { .. } => "RuntimeLease",
            Self::RuntimeLeaseAck { .. } => "RuntimeLeaseAck",
            Self::Command { .. } => "Command",
            Self::CommandAck { .. } => "CommandAck",
            Self::Heartbeat { .. } => "Heartbeat",
            Self::FaultReport { .. } => "FaultReport",
            Self::FallbackRequest { .. } => "FallbackRequest",
            Self::FallbackAck { .. } => "FallbackAck",
        }
    }
}

impl fmt::Display for Message {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DiscoveryProbe { protocol_major } => {
                write!(formatter, "{} major={protocol_major}", self.name())
            }
            Self::NodeAnnounce {
                node_id,
                boot_session,
                protocol_major,
                protocol_minor,
                firmware_generation,
                capability_generation,
                config_generation,
                lifecycle,
            } => write!(
                formatter,
                "{} node={node_id:#06x} boot={boot_session} protocol={protocol_major}.{protocol_minor} firmware={firmware_generation} capability={capability_generation} config={config_generation} lifecycle={lifecycle:?}",
                self.name()
            ),
            Self::CapabilityReport {
                node_id,
                boot_session,
                capability_generation,
                actuator,
                minimum,
                maximum,
                physical_feedback,
            } => write!(
                formatter,
                "{} node={node_id:#06x} boot={boot_session} generation={capability_generation} actuator={actuator} range={minimum}..={maximum} physical_feedback={physical_feedback}",
                self.name()
            ),
            Self::Configuration {
                node_id,
                boot_session,
                config_generation,
                fallback,
                runtime_lease_ms,
                command_lease_ms,
            } => write!(
                formatter,
                "{} node={node_id:#06x} boot={boot_session} generation={config_generation} fallback={fallback} runtime_lease={runtime_lease_ms}ms command_lease={command_lease_ms}ms",
                self.name()
            ),
            Self::ConfigurationAck {
                node_id,
                boot_session,
                config_generation,
                result,
            } => write!(
                formatter,
                "{} node={node_id:#06x} boot={boot_session} generation={config_generation} result={result:?}",
                self.name()
            ),
            Self::RuntimeLease {
                node_id,
                boot_session,
                epoch,
                renewal_sequence,
                config_generation,
                valid_for_ms,
            } => write!(
                formatter,
                "{} node={node_id:#06x} boot={boot_session} epoch={epoch} renewal={renewal_sequence} config={config_generation} ttl={valid_for_ms}ms",
                self.name()
            ),
            Self::RuntimeLeaseAck {
                node_id,
                boot_session,
                epoch,
                renewal_sequence,
                config_generation,
                result,
            } => write!(
                formatter,
                "{} node={node_id:#06x} boot={boot_session} epoch={epoch} renewal={renewal_sequence} config={config_generation} result={result:?}",
                self.name()
            ),
            Self::Command {
                node_id,
                boot_session,
                epoch,
                command_sequence,
                config_generation,
                radiator_fraction,
            } => write!(
                formatter,
                "{} node={node_id:#06x} boot={boot_session} epoch={epoch} sequence={command_sequence} config={config_generation} radiator={radiator_fraction}",
                self.name()
            ),
            Self::CommandAck {
                node_id,
                boot_session,
                epoch,
                command_sequence,
                config_generation,
                accepted_fraction,
                mode,
                fault_latch,
                result,
            } => write!(
                formatter,
                "{} node={node_id:#06x} boot={boot_session} epoch={epoch} sequence={command_sequence} config={config_generation} accepted={accepted_fraction} mode={mode:?} fault={fault_latch:?} result={result:?}",
                self.name()
            ),
            Self::Heartbeat {
                node_id,
                boot_session,
                lifecycle,
                mode,
                fault_latch,
                config_generation,
                epoch,
                last_command_sequence,
                accepted_fraction,
                runtime_lease_remaining_ms,
                command_lease_remaining_ms,
            } => write!(
                formatter,
                "{} node={node_id:#06x} boot={boot_session} lifecycle={lifecycle:?} mode={mode:?} fault={fault_latch:?} config={config_generation} epoch={epoch:?} sequence={last_command_sequence:?} accepted={accepted_fraction} runtime_left={runtime_lease_remaining_ms}ms command_left={command_lease_remaining_ms}ms",
                self.name()
            ),
            Self::FaultReport {
                node_id,
                boot_session,
                code,
                latched,
            } => write!(
                formatter,
                "{} node={node_id:#06x} boot={boot_session} code={code} latched={latched}",
                self.name()
            ),
            Self::FallbackRequest {
                node_id,
                boot_session,
                epoch,
            } => write!(
                formatter,
                "{} node={node_id:#06x} boot={boot_session} epoch={epoch}",
                self.name()
            ),
            Self::FallbackAck {
                node_id,
                boot_session,
                mode,
            } => write!(
                formatter,
                "{} node={node_id:#06x} boot={boot_session} mode={mode:?}",
                self.name()
            ),
        }
    }
}

#[derive(Clone, Debug)]
pub struct RuntimeLeaseState {
    pub epoch: u64,
    pub renewal_sequence: u32,
    pub expires_at_ms: u64,
}

#[derive(Clone, Debug)]
pub struct ControllerState {
    pub boot_session: u32,
    pub lifecycle: Lifecycle,
    pub mode: ActuationMode,
    pub fault_latch: FaultLatch,
    pub config_generation: u32,
    pub accepted_fraction: RadiatorFraction,
    pub runtime_lease: Option<RuntimeLeaseState>,
    pub command_expires_at_ms: Option<u64>,
    pub last_command_sequence: Option<u32>,
    clear_fault_after_self_test: bool,
    next_heartbeat_at_ms: u64,
}

#[derive(Clone, Debug)]
pub struct ComputeState {
    pub observed_boot_session: Option<u32>,
    pub observed_lifecycle: Option<Lifecycle>,
    pub observed_mode: Option<ActuationMode>,
    pub observed_fault_latch: Option<FaultLatch>,
    pub observed_config_generation: Option<u32>,
    pub observed_capability_generation: Option<u32>,
    pub feature_authority: FeatureAuthority,
    pub epoch: u64,
    pub runtime_renewal_sequence: u32,
    pub next_command_sequence: u32,
    pub last_command_ack_at_ms: Option<u64>,
    pub last_heartbeat_at_ms: Option<u64>,
    pub last_ack_result: Option<AckResult>,
}

#[derive(Clone, Debug)]
pub struct Simulation {
    pub now_ms: u64,
    pub link_up: bool,
    pub compute: ComputeState,
    pub controller: ControllerState,
    pub last_action: &'static str,
    pub exchange: Vec<String>,
}

#[derive(Clone, Copy, Debug)]
pub enum Action {
    CompleteSelfTest,
    Discover,
    GrantOrRenewRuntimeLease,
    Command(RadiatorFraction),
    AdvanceTime(u64),
    ToggleLink,
    SendStaleSequence,
    SendWrongEpoch,
    ConfigureNextGeneration,
    InjectHardFault,
    PowerCycle,
    ControlledShutdown,
}

impl Simulation {
    pub fn new() -> Self {
        Self {
            now_ms: 0,
            link_up: true,
            compute: ComputeState {
                observed_boot_session: None,
                observed_lifecycle: None,
                observed_mode: None,
                observed_fault_latch: None,
                observed_config_generation: None,
                observed_capability_generation: None,
                feature_authority: FeatureAuthority::Fallback,
                epoch: 40,
                runtime_renewal_sequence: 0,
                next_command_sequence: 1,
                last_command_ack_at_ms: None,
                last_heartbeat_at_ms: None,
                last_ack_result: None,
            },
            controller: ControllerState {
                boot_session: 1,
                lifecycle: Lifecycle::Initializing,
                mode: ActuationMode::Fallback,
                fault_latch: FaultLatch::Clear,
                config_generation: INITIAL_CONFIG_GENERATION,
                accepted_fraction: RadiatorFraction::FALLBACK,
                runtime_lease: None,
                command_expires_at_ms: None,
                last_command_sequence: None,
                clear_fault_after_self_test: false,
                next_heartbeat_at_ms: HEARTBEAT_PERIOD_MS,
            },
            last_action: "Power-on",
            exchange: vec!["NODE  Power-on -> Initializing + Fallback; no authority".to_owned()],
        }
    }
}

pub fn reduce(mut state: Simulation, action: Action) -> Simulation {
    state.exchange.clear();

    match action {
        Action::CompleteSelfTest => {
            state.last_action = "Complete startup self-test";
            state.controller.lifecycle = Lifecycle::Ready;

            if state.controller.clear_fault_after_self_test {
                state.controller.fault_latch = FaultLatch::Clear;
                state.controller.clear_fault_after_self_test = false;
                state.exchange.push(
                    "NODE  Successful post-power-cycle self-test cleared hard-fault latch"
                        .to_owned(),
                );
            } else {
                state
                    .exchange
                    .push("NODE  Startup self-test passed; node is Ready in Fallback".to_owned());
            }

            emit_heartbeat(&mut state);
        }
        Action::Discover => {
            state.last_action = "Discover and reconcile node";
            send_to_controller(
                &mut state,
                Message::DiscoveryProbe {
                    protocol_major: PROTOCOL_MAJOR,
                },
            );
        }
        Action::GrantOrRenewRuntimeLease => {
            state.last_action = "Grant or renew Runtime Lease";

            let Some(boot_session) = state.compute.observed_boot_session else {
                state.exchange.push(
                    "HOST  Denied locally: node boot session has not been discovered".to_owned(),
                );
                return reconcile(state);
            };
            let Some(config_generation) = state.compute.observed_config_generation else {
                state
                    .exchange
                    .push("HOST  Denied locally: node configuration is unknown".to_owned());
                return reconcile(state);
            };

            let continuing_epoch = state
                .controller
                .runtime_lease
                .as_ref()
                .is_some_and(|lease| lease.epoch == state.compute.epoch);

            if !continuing_epoch {
                state.compute.epoch += 1;
                state.compute.runtime_renewal_sequence = 0;
                state.compute.next_command_sequence = 1;
            }

            state.compute.runtime_renewal_sequence += 1;
            let epoch = state.compute.epoch;
            let renewal_sequence = state.compute.runtime_renewal_sequence;
            send_to_controller(
                &mut state,
                Message::RuntimeLease {
                    node_id: NODE_ID,
                    boot_session,
                    epoch,
                    renewal_sequence,
                    config_generation,
                    valid_for_ms: RUNTIME_LEASE_MS,
                },
            );
        }
        Action::Command(radiator_fraction) => {
            state.last_action = "Send valid actuator command";
            let epoch = state.compute.epoch;
            let command_sequence = state.compute.next_command_sequence;
            send_next_command(&mut state, epoch, command_sequence, radiator_fraction);
            state.compute.next_command_sequence += 1;
        }
        Action::AdvanceTime(delta_ms) => {
            state.last_action = "Advance monotonic time";
            state.now_ms += delta_ms;
            apply_deadlines(&mut state);

            if state.controller.next_heartbeat_at_ms <= state.now_ms {
                while state.controller.next_heartbeat_at_ms <= state.now_ms {
                    state.controller.next_heartbeat_at_ms += HEARTBEAT_PERIOD_MS;
                }
                emit_heartbeat(&mut state);
            }

            if state.exchange.is_empty() {
                state
                    .exchange
                    .push(format!("CLOCK Advanced by {delta_ms}ms; no transition"));
            }
        }
        Action::ToggleLink => {
            state.last_action = "Toggle CAN FD link";
            state.link_up = !state.link_up;
            state.exchange.push(format!(
                "LINK  CAN FD transport is now {}",
                if state.link_up { "UP" } else { "DOWN" }
            ));
        }
        Action::SendStaleSequence => {
            state.last_action = "Inject stale command sequence";
            let epoch = state.compute.epoch;
            let stale_sequence = state.controller.last_command_sequence.unwrap_or(0);
            send_next_command(
                &mut state,
                epoch,
                stale_sequence,
                RadiatorFraction::from_basis_points(6_000).expect("constant is valid"),
            );
        }
        Action::SendWrongEpoch => {
            state.last_action = "Inject wrong-epoch command";
            let wrong_epoch = state.compute.epoch.saturating_sub(1);
            let command_sequence = state.compute.next_command_sequence;
            send_next_command(
                &mut state,
                wrong_epoch,
                command_sequence,
                RadiatorFraction::from_basis_points(6_000).expect("constant is valid"),
            );
        }
        Action::ConfigureNextGeneration => {
            state.last_action = "Activate next configuration generation";
            let Some(boot_session) = state.compute.observed_boot_session else {
                state.exchange.push(
                    "HOST  Denied locally: discover the node before configuring it".to_owned(),
                );
                return reconcile(state);
            };
            let next_generation = state.controller.config_generation + 1;

            send_to_controller(
                &mut state,
                Message::Configuration {
                    node_id: NODE_ID,
                    boot_session,
                    config_generation: next_generation,
                    fallback: RadiatorFraction::FALLBACK,
                    runtime_lease_ms: RUNTIME_LEASE_MS,
                    command_lease_ms: COMMAND_LEASE_MS,
                },
            );
        }
        Action::InjectHardFault => {
            state.last_action = "Inject latched actuator fault";
            state.controller.fault_latch = FaultLatch::Latched;
            state.controller.runtime_lease = None;
            select_fallback(&mut state, "hard actuator fault latched");
            let boot_session = state.controller.boot_session;
            send_to_compute(
                &mut state,
                Message::FaultReport {
                    node_id: NODE_ID,
                    boot_session,
                    code: "ACTUATOR_OUTPUT_FAULT",
                    latched: true,
                },
            );
        }
        Action::PowerCycle => {
            state.last_action = "Power-cycle controller";
            let fault_was_latched = state.controller.fault_latch == FaultLatch::Latched;
            state.controller.boot_session += 1;
            state.controller.lifecycle = Lifecycle::Initializing;
            state.controller.mode = ActuationMode::Fallback;
            state.controller.accepted_fraction = RadiatorFraction::FALLBACK;
            state.controller.runtime_lease = None;
            state.controller.command_expires_at_ms = None;
            state.controller.last_command_sequence = None;
            state.controller.clear_fault_after_self_test = fault_was_latched;
            state.controller.next_heartbeat_at_ms = state.now_ms + HEARTBEAT_PERIOD_MS;
            state.exchange.push(format!(
                "NODE  Rebooted as boot session {}; authority and command history were discarded",
                state.controller.boot_session
            ));
            if fault_was_latched {
                state.exchange.push(
                    "NODE  Hard fault remains latched until startup self-test succeeds".to_owned(),
                );
            }
            emit_heartbeat(&mut state);
        }
        Action::ControlledShutdown => {
            state.last_action = "Request controlled fallback";
            let Some(boot_session) = state.compute.observed_boot_session else {
                state
                    .exchange
                    .push("HOST  No discovered node; stop leases and continue shutdown".to_owned());
                return reconcile(state);
            };

            let epoch = state.compute.epoch;
            send_to_controller(
                &mut state,
                Message::FallbackRequest {
                    node_id: NODE_ID,
                    boot_session,
                    epoch,
                },
            );
        }
    }

    reconcile(state)
}

fn send_next_command(
    state: &mut Simulation,
    epoch: u64,
    command_sequence: u32,
    radiator_fraction: RadiatorFraction,
) {
    let Some(boot_session) = state.compute.observed_boot_session else {
        state
            .exchange
            .push("HOST  Denied locally: node boot session has not been discovered".to_owned());
        return;
    };
    let Some(config_generation) = state.compute.observed_config_generation else {
        state
            .exchange
            .push("HOST  Denied locally: node configuration is unknown".to_owned());
        return;
    };

    send_to_controller(
        state,
        Message::Command {
            node_id: NODE_ID,
            boot_session,
            epoch,
            command_sequence,
            config_generation,
            radiator_fraction,
        },
    );
}

fn send_to_controller(state: &mut Simulation, message: Message) {
    state.exchange.push(format!("TX →  {message}"));
    if !state.link_up {
        state
            .exchange
            .push("DROP  Link down; node did not receive the frame".to_owned());
        return;
    }

    let responses = controller_receive(state, message);
    for response in responses {
        send_to_compute(state, response);
    }
}

fn send_to_compute(state: &mut Simulation, message: Message) {
    if !state.link_up {
        state
            .exchange
            .push(format!("DROP  Link down; host missed {}", message.name()));
        return;
    }

    state.exchange.push(format!("RX ←  {message}"));
    compute_receive(state, &message);
}

fn controller_receive(state: &mut Simulation, message: Message) -> Vec<Message> {
    match message {
        Message::DiscoveryProbe { protocol_major } => {
            if protocol_major != PROTOCOL_MAJOR {
                return Vec::new();
            }

            vec![
                announce(&state.controller),
                Message::CapabilityReport {
                    node_id: NODE_ID,
                    boot_session: state.controller.boot_session,
                    capability_generation: CAPABILITY_GENERATION,
                    actuator: "duct.radiator_air_fraction",
                    minimum: RadiatorFraction::from_basis_points(1_000).expect("constant is valid"),
                    maximum: RadiatorFraction::from_basis_points(9_000).expect("constant is valid"),
                    physical_feedback: false,
                },
                heartbeat(state),
            ]
        }
        Message::Configuration {
            node_id,
            boot_session,
            config_generation,
            fallback,
            runtime_lease_ms,
            command_lease_ms,
        } => {
            let result = if node_id != NODE_ID || boot_session != state.controller.boot_session {
                AckResult::RejectedBootSession
            } else if state.controller.runtime_lease.is_some()
                || state.controller.mode == ActuationMode::Commanded
            {
                AckResult::RejectedUnderAuthority
            } else if runtime_lease_ms != RUNTIME_LEASE_MS || command_lease_ms != COMMAND_LEASE_MS {
                AckResult::RejectedConfigGeneration
            } else {
                state.controller.config_generation = config_generation;
                state.controller.accepted_fraction = fallback;
                AckResult::Accepted
            };

            vec![Message::ConfigurationAck {
                node_id: NODE_ID,
                boot_session: state.controller.boot_session,
                config_generation,
                result,
            }]
        }
        Message::RuntimeLease {
            node_id,
            boot_session,
            epoch,
            renewal_sequence,
            config_generation,
            valid_for_ms,
        } => {
            let result = validate_runtime_lease(
                state,
                node_id,
                boot_session,
                epoch,
                renewal_sequence,
                config_generation,
                valid_for_ms,
            );

            if result == AckResult::Accepted {
                state.controller.runtime_lease = Some(RuntimeLeaseState {
                    epoch,
                    renewal_sequence,
                    expires_at_ms: state.now_ms + valid_for_ms,
                });
            }

            vec![Message::RuntimeLeaseAck {
                node_id: NODE_ID,
                boot_session: state.controller.boot_session,
                epoch,
                renewal_sequence,
                config_generation: state.controller.config_generation,
                result,
            }]
        }
        Message::Command {
            node_id,
            boot_session,
            epoch,
            command_sequence,
            config_generation,
            radiator_fraction,
        } => {
            let result = validate_command(
                state,
                node_id,
                boot_session,
                epoch,
                command_sequence,
                config_generation,
                radiator_fraction,
            );

            if result == AckResult::Accepted {
                state.controller.mode = ActuationMode::Commanded;
                state.controller.accepted_fraction = radiator_fraction;
                state.controller.last_command_sequence = Some(command_sequence);
                state.controller.command_expires_at_ms = Some(state.now_ms + COMMAND_LEASE_MS);
            }

            vec![Message::CommandAck {
                node_id: NODE_ID,
                boot_session: state.controller.boot_session,
                epoch,
                command_sequence,
                config_generation: state.controller.config_generation,
                accepted_fraction: state.controller.accepted_fraction,
                mode: state.controller.mode,
                fault_latch: state.controller.fault_latch,
                result,
            }]
        }
        Message::FallbackRequest {
            node_id,
            boot_session,
            epoch,
        } => {
            if node_id == NODE_ID
                && boot_session == state.controller.boot_session
                && state
                    .controller
                    .runtime_lease
                    .as_ref()
                    .is_some_and(|lease| lease.epoch == epoch)
            {
                state.controller.runtime_lease = None;
                select_fallback(state, "controlled fallback requested");
            }

            vec![Message::FallbackAck {
                node_id: NODE_ID,
                boot_session: state.controller.boot_session,
                mode: state.controller.mode,
            }]
        }
        _ => Vec::new(),
    }
}

fn validate_runtime_lease(
    state: &Simulation,
    node_id: u16,
    boot_session: u32,
    epoch: u64,
    renewal_sequence: u32,
    config_generation: u32,
    valid_for_ms: u64,
) -> AckResult {
    if node_id != NODE_ID || boot_session != state.controller.boot_session {
        return AckResult::RejectedBootSession;
    }
    if state.controller.lifecycle != Lifecycle::Ready {
        return AckResult::RejectedNotReady;
    }
    if state.controller.fault_latch == FaultLatch::Latched {
        return AckResult::RejectedFaultLatched;
    }
    if config_generation != state.controller.config_generation || valid_for_ms != RUNTIME_LEASE_MS {
        return AckResult::RejectedConfigGeneration;
    }

    if let Some(current) = &state.controller.runtime_lease {
        if epoch < current.epoch
            || (epoch == current.epoch && renewal_sequence <= current.renewal_sequence)
        {
            return AckResult::RejectedEpoch;
        }
    }

    AckResult::Accepted
}

fn validate_command(
    state: &Simulation,
    node_id: u16,
    boot_session: u32,
    epoch: u64,
    command_sequence: u32,
    config_generation: u32,
    radiator_fraction: RadiatorFraction,
) -> AckResult {
    if node_id != NODE_ID || boot_session != state.controller.boot_session {
        return AckResult::RejectedBootSession;
    }
    if state.controller.lifecycle != Lifecycle::Ready {
        return AckResult::RejectedNotReady;
    }
    if state.controller.fault_latch == FaultLatch::Latched {
        return AckResult::RejectedFaultLatched;
    }
    if config_generation != state.controller.config_generation {
        return AckResult::RejectedConfigGeneration;
    }
    if !(1_000..=9_000).contains(&radiator_fraction.basis_points()) {
        return AckResult::RejectedOutOfRange;
    }

    let Some(runtime_lease) = &state.controller.runtime_lease else {
        return AckResult::RejectedEpoch;
    };

    if runtime_lease.epoch != epoch || runtime_lease.expires_at_ms <= state.now_ms {
        return AckResult::RejectedEpoch;
    }
    if state
        .controller
        .last_command_sequence
        .is_some_and(|last_sequence| command_sequence <= last_sequence)
    {
        return AckResult::RejectedSequence;
    }

    AckResult::Accepted
}

fn compute_receive(state: &mut Simulation, message: &Message) {
    match message {
        Message::NodeAnnounce {
            boot_session,
            config_generation,
            lifecycle,
            ..
        } => {
            if state.compute.observed_boot_session != Some(*boot_session) {
                state.compute.feature_authority = FeatureAuthority::Fallback;
                state.compute.last_command_ack_at_ms = None;
                state.compute.epoch += 1;
                state.compute.runtime_renewal_sequence = 0;
                state.compute.next_command_sequence = 1;
            }

            state.compute.observed_boot_session = Some(*boot_session);
            state.compute.observed_config_generation = Some(*config_generation);
            state.compute.observed_lifecycle = Some(*lifecycle);
        }
        Message::CapabilityReport {
            capability_generation,
            ..
        } => {
            state.compute.observed_capability_generation = Some(*capability_generation);
        }
        Message::ConfigurationAck {
            config_generation,
            result,
            ..
        } => {
            state.compute.last_ack_result = Some(*result);
            if *result == AckResult::Accepted {
                state.compute.observed_config_generation = Some(*config_generation);
            }
        }
        Message::RuntimeLeaseAck {
            result,
            config_generation,
            ..
        } => {
            state.compute.last_ack_result = Some(*result);
            state.compute.observed_config_generation = Some(*config_generation);
            state.compute.feature_authority = if *result == AckResult::Accepted {
                state.compute.last_command_ack_at_ms = None;
                FeatureAuthority::Arming
            } else {
                FeatureAuthority::Fallback
            };
        }
        Message::CommandAck {
            boot_session,
            mode,
            fault_latch,
            config_generation,
            result,
            ..
        } => {
            state.compute.observed_boot_session = Some(*boot_session);
            state.compute.observed_mode = Some(*mode);
            state.compute.observed_fault_latch = Some(*fault_latch);
            state.compute.observed_config_generation = Some(*config_generation);
            state.compute.last_ack_result = Some(*result);

            if *result == AckResult::Accepted {
                state.compute.last_command_ack_at_ms = Some(state.now_ms);
                state.compute.feature_authority = FeatureAuthority::Active;
            }
        }
        Message::Heartbeat {
            boot_session,
            lifecycle,
            mode,
            fault_latch,
            config_generation,
            ..
        } => {
            if state.compute.observed_boot_session != Some(*boot_session) {
                state.compute.feature_authority = FeatureAuthority::Fallback;
                state.compute.last_command_ack_at_ms = None;
            }
            state.compute.observed_boot_session = Some(*boot_session);
            state.compute.observed_lifecycle = Some(*lifecycle);
            state.compute.observed_mode = Some(*mode);
            state.compute.observed_fault_latch = Some(*fault_latch);
            state.compute.observed_config_generation = Some(*config_generation);
            state.compute.last_heartbeat_at_ms = Some(state.now_ms);
        }
        Message::FaultReport { latched, .. } => {
            state.compute.observed_fault_latch = Some(if *latched {
                FaultLatch::Latched
            } else {
                FaultLatch::Clear
            });
            state.compute.feature_authority = FeatureAuthority::Fallback;
        }
        Message::FallbackAck { mode, .. } => {
            state.compute.observed_mode = Some(*mode);
            state.compute.feature_authority = FeatureAuthority::Fallback;
        }
        _ => {}
    }
}

fn apply_deadlines(state: &mut Simulation) {
    if state
        .controller
        .runtime_lease
        .as_ref()
        .is_some_and(|lease| lease.expires_at_ms <= state.now_ms)
    {
        state.controller.runtime_lease = None;
        select_fallback(state, "Runtime Lease expired");
    } else if state
        .controller
        .command_expires_at_ms
        .is_some_and(|expires_at| expires_at <= state.now_ms)
    {
        select_fallback(state, "Command Lease expired");
    }
}

fn select_fallback(state: &mut Simulation, reason: &str) {
    let transitioned = state.controller.mode != ActuationMode::Fallback
        || state.controller.accepted_fraction != RadiatorFraction::FALLBACK;

    state.controller.mode = ActuationMode::Fallback;
    state.controller.accepted_fraction = RadiatorFraction::FALLBACK;
    state.controller.command_expires_at_ms = None;

    if transitioned {
        state
            .exchange
            .push(format!("NODE  Selected configured fallback: {reason}"));
    }
}

fn announce(controller: &ControllerState) -> Message {
    Message::NodeAnnounce {
        node_id: NODE_ID,
        boot_session: controller.boot_session,
        protocol_major: PROTOCOL_MAJOR,
        protocol_minor: PROTOCOL_MINOR,
        firmware_generation: 12,
        capability_generation: CAPABILITY_GENERATION,
        config_generation: controller.config_generation,
        lifecycle: controller.lifecycle,
    }
}

fn heartbeat(state: &Simulation) -> Message {
    Message::Heartbeat {
        node_id: NODE_ID,
        boot_session: state.controller.boot_session,
        lifecycle: state.controller.lifecycle,
        mode: state.controller.mode,
        fault_latch: state.controller.fault_latch,
        config_generation: state.controller.config_generation,
        epoch: state
            .controller
            .runtime_lease
            .as_ref()
            .map(|lease| lease.epoch),
        last_command_sequence: state.controller.last_command_sequence,
        accepted_fraction: state.controller.accepted_fraction,
        runtime_lease_remaining_ms: state
            .controller
            .runtime_lease
            .as_ref()
            .map_or(0, |lease| lease.expires_at_ms.saturating_sub(state.now_ms)),
        command_lease_remaining_ms: state
            .controller
            .command_expires_at_ms
            .map_or(0, |expires_at| expires_at.saturating_sub(state.now_ms)),
    }
}

fn emit_heartbeat(state: &mut Simulation) {
    let message = heartbeat(state);
    send_to_compute(state, message);
}

fn reconcile(mut state: Simulation) -> Simulation {
    if let Some(last_ack_at_ms) = state.compute.last_command_ack_at_ms {
        if state.now_ms.saturating_sub(last_ack_at_ms) >= ACK_FRESHNESS_MS {
            state.compute.feature_authority = FeatureAuthority::Fallback;
        }
    }

    if state.compute.observed_fault_latch == Some(FaultLatch::Latched)
        || state.compute.observed_lifecycle == Some(Lifecycle::Initializing)
    {
        state.compute.feature_authority = FeatureAuthority::Fallback;
    }

    state
}
