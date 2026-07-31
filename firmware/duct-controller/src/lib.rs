#![no_std]

/// Safe powered fallback before controller configuration and authority.
pub const STARTS_IN_CONTROLLER_LOCAL_FALLBACK: bool = true;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FirmwareMode {
    LocalFallback,
    RemoteAuthority,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FirmwareConfiguration {
    pub generation: u32,
    pub fallback_basis_points: u16,
    pub pwm_endpoint_a_us: u16,
    pub pwm_endpoint_b_us: u16,
    pub direction: u8,
    pub runtime_lease_ms: u16,
    pub command_lease_ms: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct RuntimeLeaseState {
    epoch: u64,
    renewal_sequence: u32,
    deadline_ms: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ControllerState {
    boot_session: u32,
    immutable_fallback_basis_points: u16,
    active_configuration: Option<FirmwareConfiguration>,
    staging_configuration: Option<FirmwareConfiguration>,
    runtime_lease: Option<RuntimeLeaseState>,
    command_deadline_ms: Option<u64>,
    last_command_sequence: Option<u32>,
    accepted_basis_points: u16,
    mode: FirmwareMode,
}

impl ControllerState {
    #[must_use]
    pub const fn new(boot_session: u32, immutable_fallback_basis_points: u16) -> Self {
        Self {
            boot_session,
            immutable_fallback_basis_points,
            active_configuration: None,
            staging_configuration: None,
            runtime_lease: None,
            command_deadline_ms: None,
            last_command_sequence: None,
            accepted_basis_points: immutable_fallback_basis_points,
            mode: FirmwareMode::LocalFallback,
        }
    }

    pub fn configure(&mut self, configuration: FirmwareConfiguration) -> bool {
        if self.mode != FirmwareMode::LocalFallback
            || self.runtime_lease.is_some()
            || !valid_configuration(configuration)
        {
            return false;
        }

        self.staging_configuration = Some(configuration);
        self.active_configuration = self.staging_configuration.take();
        self.accepted_basis_points = configuration.fallback_basis_points;
        true
    }

    pub fn begin_configuration_write(&mut self, configuration: FirmwareConfiguration) {
        if valid_configuration(configuration) {
            self.staging_configuration = Some(configuration);
        }
    }

    pub fn recover_after_interrupted_write(&mut self) {
        self.staging_configuration = None;
        self.select_fallback();
    }

    pub fn revoke_authority(&mut self) {
        self.runtime_lease = None;
        self.select_fallback();
    }

    pub fn accept_runtime_lease(
        &mut self,
        now_ms: u64,
        boot_session: u32,
        configuration_generation: u32,
        epoch: u64,
        renewal_sequence: u32,
        validity_ms: u16,
    ) -> bool {
        let valid = boot_session == self.boot_session
            && self
                .active_configuration
                .is_some_and(|configuration| configuration.generation == configuration_generation)
            && self.runtime_lease.is_none_or(|lease| {
                lease.epoch == epoch && renewal_sequence > lease.renewal_sequence
            });
        if valid {
            self.runtime_lease = Some(RuntimeLeaseState {
                epoch,
                renewal_sequence,
                deadline_ms: now_ms.saturating_add(u64::from(validity_ms)),
            });
        }

        valid
    }

    pub fn accept_command(
        &mut self,
        now_ms: u64,
        boot_session: u32,
        configuration_generation: u32,
        epoch: u64,
        command_sequence: u32,
        radiator_split_basis_points: u16,
    ) -> bool {
        self.advance_to(now_ms);
        let valid = radiator_split_basis_points <= 10_000
            && boot_session == self.boot_session
            && self.runtime_lease_valid(now_ms)
            && self
                .active_configuration
                .is_some_and(|configuration| configuration.generation == configuration_generation)
            && self.runtime_lease.is_some_and(|lease| lease.epoch == epoch)
            && self
                .last_command_sequence
                .is_none_or(|sequence| command_sequence > sequence);
        if valid && let Some(configuration) = self.active_configuration {
            self.command_deadline_ms =
                Some(now_ms.saturating_add(u64::from(configuration.command_lease_ms)));
            self.last_command_sequence = Some(command_sequence);
            self.accepted_basis_points = radiator_split_basis_points;
            self.mode = FirmwareMode::RemoteAuthority;
        }

        valid
    }

    pub fn advance_to(&mut self, now_ms: u64) {
        if self.runtime_lease.is_some() && !self.runtime_lease_valid(now_ms) {
            self.runtime_lease = None;
            self.select_fallback();
        } else if self.mode == FirmwareMode::RemoteAuthority
            && self
                .command_deadline_ms
                .is_none_or(|deadline| now_ms >= deadline)
        {
            self.select_fallback();
        }
    }

    #[must_use]
    pub fn runtime_lease_valid(&self, now_ms: u64) -> bool {
        self.runtime_lease
            .is_some_and(|lease| now_ms < lease.deadline_ms)
    }

    #[must_use]
    pub const fn mode(&self) -> FirmwareMode {
        self.mode
    }

    #[must_use]
    pub const fn accepted_basis_points(&self) -> u16 {
        self.accepted_basis_points
    }

    #[must_use]
    pub fn configuration_generation(&self) -> Option<u32> {
        self.active_configuration
            .map(|configuration| configuration.generation)
    }

    #[must_use]
    pub fn current_epoch(&self) -> Option<u64> {
        self.runtime_lease.map(|lease| lease.epoch)
    }

    #[must_use]
    pub const fn last_command_sequence(&self) -> Option<u32> {
        self.last_command_sequence
    }

    #[must_use]
    pub fn command_lease_remaining_ms(&self, now_ms: u64) -> u16 {
        self.command_deadline_ms.map_or(0, |deadline| {
            u16::try_from(deadline.saturating_sub(now_ms)).unwrap_or(u16::MAX)
        })
    }

    #[must_use]
    pub fn pwm_microseconds(&self) -> u16 {
        let Some(configuration) = self.active_configuration else {
            return 0;
        };
        let span = configuration
            .pwm_endpoint_b_us
            .abs_diff(configuration.pwm_endpoint_a_us);
        let offset = u32::from(span) * u32::from(self.accepted_basis_points) / 10_000;
        let offset = u16::try_from(offset).unwrap_or(u16::MAX);
        match (
            configuration.direction,
            configuration.pwm_endpoint_a_us <= configuration.pwm_endpoint_b_us,
        ) {
            (1, true) | (2, false) => configuration.pwm_endpoint_a_us.saturating_add(offset),
            (1, false) | (2, true) => configuration.pwm_endpoint_a_us.saturating_sub(offset),
            _ => 0,
        }
    }

    fn select_fallback(&mut self) {
        self.mode = FirmwareMode::LocalFallback;
        self.command_deadline_ms = None;
        self.accepted_basis_points = self
            .active_configuration
            .map_or(self.immutable_fallback_basis_points, |configuration| {
                configuration.fallback_basis_points
            });
    }
}

const fn valid_configuration(configuration: FirmwareConfiguration) -> bool {
    configuration.fallback_basis_points <= 10_000
        && configuration.pwm_endpoint_a_us != configuration.pwm_endpoint_b_us
        && (configuration.direction == 1 || configuration.direction == 2)
        && configuration.runtime_lease_ms > 0
        && configuration.command_lease_ms > 0
}
