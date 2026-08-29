use control_protocol::{
    CapabilityReport, CommandAck, ConfigurationAck, Frame, Heartbeat, NodeAnnounce,
    RuntimeLeaseAck, WireError, decode, encode,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ControllerMode {
    LocalFallback,
    RemoteAuthority,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EmulatorProvisioning {
    pub node: u8,
    pub identity: u64,
    pub boot_session: u32,
    pub firmware_generation: u32,
    pub capability_generation: u32,
    pub fallback_basis_points: u16,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EmulatorResponse {
    pub can_id: u16,
    bytes: [u8; 64],
    len: usize,
    follow_up: Option<Box<Self>>,
}

impl EmulatorResponse {
    #[must_use]
    pub fn payload(&self) -> &[u8] {
        &self.bytes[..self.len]
    }

    #[must_use]
    pub fn follow_up(&self) -> Option<&Self> {
        self.follow_up.as_deref()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ActiveConfiguration {
    generation: u32,
    digest_prefix: u32,
    command_lease_ms: u16,
    heartbeat_period_ms: u16,
    fallback_basis_points: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ActiveRuntimeLease {
    epoch: u64,
    renewal_sequence: u32,
    deadline_ms: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ControllerEmulator {
    provisioning: EmulatorProvisioning,
    configuration: Option<ActiveConfiguration>,
    runtime_lease: Option<ActiveRuntimeLease>,
    command_deadline_ms: Option<u64>,
    last_command_sequence: Option<u32>,
    acknowledgement_sequence: u32,
    heartbeat_sequence: u32,
    next_heartbeat_ms: Option<u64>,
    mode: ControllerMode,
    accepted_basis_points: u16,
}

impl ControllerEmulator {
    #[must_use]
    pub fn new(provisioning: EmulatorProvisioning) -> Self {
        Self {
            accepted_basis_points: provisioning.fallback_basis_points,
            provisioning,
            configuration: None,
            runtime_lease: None,
            command_deadline_ms: None,
            last_command_sequence: None,
            acknowledgement_sequence: 0,
            heartbeat_sequence: 0,
            next_heartbeat_ms: None,
            mode: ControllerMode::LocalFallback,
        }
    }

    /// Ingests an encoded production frame and returns at most one immediate
    /// encoded response. Timer-driven fallback remains visible through
    /// [`Self::advance_to`].
    ///
    /// # Errors
    ///
    /// Returns [`WireError`] when the incoming frame or generated response is
    /// outside the fixed protocol contract.
    pub fn ingest(
        &mut self,
        now_ms: u64,
        can_id: u16,
        payload: &[u8],
    ) -> Result<Option<EmulatorResponse>, WireError> {
        self.advance_to(now_ms);
        let frame = decode(can_id, payload)?;
        let discovery = matches!(&frame, Frame::DiscoveryProbe(_));
        let response = match frame {
            Frame::DiscoveryProbe(message) if message.protocol_major == 1 => {
                Some(Frame::NodeAnnounce {
                    node: self.provisioning.node,
                    message: NodeAnnounce {
                        protocol_major: 1,
                        protocol_minor: 0,
                        lifecycle: if self.configuration.is_some() { 2 } else { 1 },
                        state_flags: match self.mode {
                            ControllerMode::LocalFallback => 1,
                            ControllerMode::RemoteAuthority => 2,
                        },
                        boot_session: self.provisioning.boot_session,
                        provisioned_identity: self.provisioning.identity,
                        firmware_generation: self.provisioning.firmware_generation,
                        capability_generation: self.provisioning.capability_generation,
                        configuration_generation: self
                            .configuration
                            .map_or(0, |configuration| configuration.generation),
                        announce_sequence: message.probe_sequence,
                    },
                })
            }
            Frame::Configuration { node, message } if node == self.provisioning.node => {
                let accepted = message.boot_session == self.provisioning.boot_session
                    && self.mode == ControllerMode::LocalFallback
                    && self.runtime_lease.is_none();
                if accepted {
                    self.configuration = Some(ActiveConfiguration {
                        generation: message.configuration_generation,
                        digest_prefix: message.digest_prefix,
                        command_lease_ms: message.command_lease_ms,
                        heartbeat_period_ms: message.heartbeat_period_ms,
                        fallback_basis_points: message.fallback_basis_points,
                    });
                    self.next_heartbeat_ms =
                        Some(now_ms.saturating_add(u64::from(message.heartbeat_period_ms)));
                    self.accepted_basis_points = message.fallback_basis_points;
                }

                Some(Frame::ConfigurationAck {
                    node,
                    message: ConfigurationAck {
                        boot_session: self.provisioning.boot_session,
                        configuration_generation: message.configuration_generation,
                        digest_prefix: message.digest_prefix,
                        result: if accepted { 1 } else { 2 },
                    },
                })
            }
            Frame::RuntimeLease { node, message } if node == self.provisioning.node => {
                let accepted = self.configuration.is_some_and(|configuration| {
                    message.boot_session == self.provisioning.boot_session
                        && message.configuration_generation == configuration.generation
                        && self.runtime_lease.is_none_or(|lease| {
                            message.epoch == lease.epoch
                                && message.renewal_sequence > lease.renewal_sequence
                        })
                });
                if accepted {
                    self.runtime_lease = Some(ActiveRuntimeLease {
                        epoch: message.epoch,
                        renewal_sequence: message.renewal_sequence,
                        deadline_ms: now_ms.saturating_add(u64::from(message.validity_ms)),
                    });
                }

                Some(Frame::RuntimeLeaseAck {
                    node,
                    message: RuntimeLeaseAck {
                        boot_session: self.provisioning.boot_session,
                        configuration_generation: message.configuration_generation,
                        epoch: message.epoch,
                        renewal_sequence: message.renewal_sequence,
                        result: if accepted { 1 } else { 2 },
                    },
                })
            }
            Frame::Command { node, message } if node == self.provisioning.node => {
                let accepted = self.configuration.is_some_and(|configuration| {
                    self.runtime_lease_valid(now_ms)
                        && message.boot_session == self.provisioning.boot_session
                        && message.configuration_generation == configuration.generation
                        && self
                            .runtime_lease
                            .is_some_and(|lease| message.epoch == lease.epoch)
                        && self
                            .last_command_sequence
                            .is_none_or(|sequence| message.command_sequence > sequence)
                });
                if accepted && let Some(configuration) = self.configuration {
                    self.command_deadline_ms =
                        Some(now_ms.saturating_add(u64::from(configuration.command_lease_ms)));
                    self.last_command_sequence = Some(message.command_sequence);
                    self.accepted_basis_points = message.radiator_split_basis_points;
                    self.mode = ControllerMode::RemoteAuthority;
                }
                self.acknowledgement_sequence = self.acknowledgement_sequence.wrapping_add(1);

                Some(Frame::CommandAck {
                    node,
                    message: CommandAck {
                        boot_session: self.provisioning.boot_session,
                        configuration_generation: message.configuration_generation,
                        epoch: message.epoch,
                        command_sequence: message.command_sequence,
                        accepted_basis_points: self.accepted_basis_points,
                        mode: match self.mode {
                            ControllerMode::LocalFallback => 1,
                            ControllerMode::RemoteAuthority => 2,
                        },
                        fault_latch: 1,
                        result: if accepted { 1 } else { 2 },
                        output_state: 1,
                        command_lease_remaining_ms: self.command_lease_remaining_ms(now_ms),
                        acknowledgement_sequence: self.acknowledgement_sequence,
                    },
                })
            }
            Frame::FallbackRequest { node, message } if node == self.provisioning.node => {
                self.select_fallback();
                Some(Frame::FallbackAck {
                    node,
                    message: control_protocol::FallbackAck {
                        boot_session: self.provisioning.boot_session,
                        request_sequence: message.request_sequence,
                        accepted_basis_points: self.accepted_basis_points,
                        mode: 1,
                        fault_latch: 1,
                        result: 1,
                    },
                })
            }
            _ => None,
        };

        let mut response = response.as_ref().map(encode_response).transpose()?;
        if discovery && let Some(announce) = &mut response {
            announce.follow_up = Some(Box::new(encode_response(&Frame::CapabilityReport {
                node: self.provisioning.node,
                message: CapabilityReport {
                    boot_session: self.provisioning.boot_session,
                    capability_generation: self.provisioning.capability_generation,
                    resource_id: 1,
                    minimum_basis_points: 0,
                    maximum_basis_points: 10_000,
                    capability_flags: 0,
                    maximum_command_rate_hz: 50,
                },
            })?));
        }
        Ok(response)
    }

    /// Emits the configured periodic controller truth when its local deadline
    /// has elapsed. Callers drive this from their own monotonic timer.
    ///
    /// # Errors
    ///
    /// Returns [`WireError`] if the fixed heartbeat cannot be encoded.
    pub fn heartbeat(&mut self, now_ms: u64) -> Result<Option<EmulatorResponse>, WireError> {
        self.advance_to(now_ms);
        let Some(configuration) = self.configuration else {
            return Ok(None);
        };
        if self
            .next_heartbeat_ms
            .is_none_or(|deadline| now_ms < deadline)
        {
            return Ok(None);
        }

        self.heartbeat_sequence = self.heartbeat_sequence.wrapping_add(1);
        self.next_heartbeat_ms =
            Some(now_ms.saturating_add(u64::from(configuration.heartbeat_period_ms)));
        encode_response(&Frame::Heartbeat {
            node: self.provisioning.node,
            message: Heartbeat {
                boot_session: self.provisioning.boot_session,
                configuration_generation: configuration.generation,
                capability_generation: self.provisioning.capability_generation,
                heartbeat_sequence: self.heartbeat_sequence,
                current_epoch: self.runtime_lease.map_or(0, |lease| lease.epoch),
                last_command_sequence: self.last_command_sequence.unwrap_or(0),
                accepted_basis_points: self.accepted_basis_points,
                state_flags: match self.mode {
                    ControllerMode::LocalFallback => 1,
                    ControllerMode::RemoteAuthority => 2,
                },
            },
        })
        .map(Some)
    }

    pub fn advance_to(&mut self, now_ms: u64) {
        if self.runtime_lease.is_some() && !self.runtime_lease_valid(now_ms) {
            self.runtime_lease = None;
            self.select_fallback();
        } else if self.mode == ControllerMode::RemoteAuthority
            && self
                .command_deadline_ms
                .is_none_or(|deadline| now_ms >= deadline)
        {
            self.select_fallback();
        }
    }

    pub fn reboot(&mut self, boot_session: u32) {
        self.provisioning.boot_session = boot_session;
        self.configuration = None;
        self.runtime_lease = None;
        self.command_deadline_ms = None;
        self.last_command_sequence = None;
        self.next_heartbeat_ms = None;
        self.select_fallback();
    }

    #[must_use]
    pub fn runtime_lease_valid(&self, now_ms: u64) -> bool {
        self.runtime_lease
            .is_some_and(|lease| now_ms < lease.deadline_ms)
    }

    #[must_use]
    pub const fn mode(&self) -> ControllerMode {
        self.mode
    }

    #[must_use]
    pub const fn accepted_basis_points(&self) -> u16 {
        self.accepted_basis_points
    }

    #[must_use]
    pub const fn boot_session(&self) -> u32 {
        self.provisioning.boot_session
    }

    fn command_lease_remaining_ms(&self, now_ms: u64) -> u16 {
        self.command_deadline_ms.map_or(0, |deadline| {
            u16::try_from(deadline.saturating_sub(now_ms)).unwrap_or(u16::MAX)
        })
    }

    fn select_fallback(&mut self) {
        self.mode = ControllerMode::LocalFallback;
        self.command_deadline_ms = None;
        self.accepted_basis_points = self
            .configuration
            .map_or(self.provisioning.fallback_basis_points, |configuration| {
                configuration.fallback_basis_points
            });
    }
}

fn encode_response(frame: &Frame) -> Result<EmulatorResponse, WireError> {
    let mut bytes = [0_u8; 64];
    let encoded = encode(frame, &mut bytes)?;
    Ok(EmulatorResponse {
        can_id: encoded.can_id,
        bytes,
        len: encoded.len,
        follow_up: None,
    })
}
