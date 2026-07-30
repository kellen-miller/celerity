export const protocol = Object.freeze({
  runtimeLeaseMs: 1_000,
  commandLeaseMs: 200,
  ackFreshnessMs: 250,
  heartbeatPeriodMs: 100,
  nodeId: "0x002A",
  capabilityGeneration: 3,
  initialConfigGeneration: 7,
  fallbackSetpoint: 50,
});

export const walkthrough = Object.freeze([
  {
    action: "discover",
    label: "Discover controller",
    copy: "Discovery reveals identity and configuration, but grants no authority.",
  },
  {
    action: "selfTest",
    label: "Complete self-test",
    copy: "The controller becomes Ready while remaining in controller-local Fallback.",
  },
  {
    action: "discover",
    label: "Reconcile ready state",
    copy: "Refresh Celerity's observed lifecycle before asking for authority.",
  },
  {
    action: "grant",
    label: "Grant Runtime Lease",
    copy: "Global permission arms the feature; it does not move the actuator.",
  },
  {
    action: "command30",
    label: "Command 30%",
    copy: "A valid command starts the shorter Command Lease and activates the feature.",
  },
  {
    action: "toggleLink",
    label: "Drop the CAN FD link",
    copy: "The controller continues only for the remaining bounded Command Lease.",
  },
  {
    action: "tick250",
    label: "Advance 250 ms",
    copy: "The Command Lease expires locally even though Celerity cannot observe the transition.",
  },
  {
    action: "toggleLink",
    label: "Restore the CAN FD link",
    copy: "Transport recovery alone does not restore command authority.",
  },
  {
    action: "discover",
    label: "Reconcile controller state",
    copy: "Celerity observes that the controller selected Fallback independently.",
  },
  {
    action: "grant",
    label: "Renew Runtime Lease",
    copy: "A fresh, ordered lease renewal returns the feature to Arming.",
  },
  {
    action: "command70",
    label: "Command 70%",
    copy: "A fresh command completes recovery and returns the feature to Active.",
  },
]);

export function initialState() {
  return {
    now: 0,
    linkUp: true,
    lastResult: "Power-on selected controller-local fallback.",
    nextEventId: 2,
    compute: {
      observedBoot: null,
      lifecycle: null,
      mode: null,
      fault: null,
      capabilityGeneration: null,
      configGeneration: null,
      authority: "Fallback",
      epoch: 40,
      renewalSequence: 0,
      nextCommandSequence: 1,
      lastCommandAckAt: null,
      lastHeartbeatAt: null,
      lastAckResult: null,
    },
    controller: {
      bootSession: 1,
      lifecycle: "Initializing",
      mode: "Fallback",
      fault: "Clear",
      configGeneration: protocol.initialConfigGeneration,
      acceptedSetpoint: protocol.fallbackSetpoint,
      runtimeLease: null,
      commandExpiresAt: null,
      lastCommandSequence: null,
      clearFaultAfterSelfTest: false,
      nextHeartbeatAt: protocol.heartbeatPeriodMs,
    },
    events: [
      {
        id: 1,
        at: 0,
        kind: "node",
        title: "Power-on",
        detail: "Initializing · Fallback · no authority",
      },
    ],
  };
}

export function runAction(state, action) {
  switch (action) {
    case "selfTest":
      selfTest(state);
      break;
    case "discover":
      discover(state);
      break;
    case "grant":
      grant(state);
      break;
    case "command30":
      command(state, 30);
      break;
    case "command70":
      command(state, 70);
      break;
    case "tick50":
      advanceTime(state, 50);
      break;
    case "tick250":
      advanceTime(state, 250);
      break;
    case "toggleLink":
      toggleLink(state);
      break;
    case "staleSequence":
      staleSequence(state);
      break;
    case "wrongEpoch":
      wrongEpoch(state);
      break;
    case "configure":
      configure(state);
      break;
    case "hardFault":
      hardFault(state);
      break;
    case "powerCycle":
      powerCycle(state);
      break;
    case "fallback":
      controlledFallback(state);
      break;
    default:
      throw new Error(`Unknown simulation action: ${action}`);
  }

  reconcile(state);
}

function record(state, kind, title, detail) {
  state.events.unshift({
    id: state.nextEventId,
    at: state.now,
    kind,
    title,
    detail,
  });
  state.nextEventId += 1;
  state.events = state.events.slice(0, 36);
  state.lastResult = `${title}: ${detail}`;
}

function selfTest(state) {
  state.controller.lifecycle = "Ready";

  if (state.controller.clearFaultAfterSelfTest) {
    state.controller.fault = "Clear";
    state.controller.clearFaultAfterSelfTest = false;
    record(
      state,
      "node",
      "Self-test passed",
      "Hard-fault latch cleared after power cycle",
    );
  } else {
    record(state, "node", "Self-test passed", "Ready · Fallback");
  }

  heartbeat(state);
}

function discover(state) {
  transmit(state, "DiscoveryProbe", "protocol=1");
  if (!state.linkUp) {
    return;
  }

  if (state.compute.observedBoot !== state.controller.bootSession) {
    state.compute.authority = "Fallback";
    state.compute.lastCommandAckAt = null;
    state.compute.epoch += 1;
    state.compute.renewalSequence = 0;
    state.compute.nextCommandSequence = 1;
  }

  state.compute.observedBoot = state.controller.bootSession;
  state.compute.lifecycle = state.controller.lifecycle;
  state.compute.mode = state.controller.mode;
  state.compute.fault = state.controller.fault;
  state.compute.configGeneration = state.controller.configGeneration;
  state.compute.capabilityGeneration = protocol.capabilityGeneration;
  state.compute.lastHeartbeatAt = state.now;

  receive(
    state,
    "NodeAnnounce",
    `node=${protocol.nodeId} · boot=${state.controller.bootSession} · lifecycle=${state.controller.lifecycle}`,
  );
  receive(
    state,
    "CapabilityReport",
    "duct.radiator_air_fraction · range=10–90% · physical_feedback=false",
  );
  receive(state, "Heartbeat", heartbeatDetail(state));
}

function grant(state) {
  if (
    state.compute.observedBoot === null ||
    state.compute.configGeneration === null
  ) {
    record(
      state,
      "host",
      "Lease denied locally",
      "Discover the controller before granting authority",
    );
    return;
  }

  const continuingEpoch =
    state.controller.runtimeLease?.epoch === state.compute.epoch;

  if (!continuingEpoch) {
    state.compute.epoch += 1;
    state.compute.renewalSequence = 0;
    state.compute.nextCommandSequence = 1;
  }

  state.compute.renewalSequence += 1;
  transmit(
    state,
    "RuntimeLease",
    `boot=${state.compute.observedBoot} · epoch=${state.compute.epoch} · renewal=${state.compute.renewalSequence}`,
  );
  if (!state.linkUp) {
    return;
  }

  const rejection = validateLease(state);
  if (rejection) {
    state.compute.lastAckResult = rejection;
    state.compute.authority = "Fallback";
    receive(state, "RuntimeLeaseAck", rejection);
    return;
  }

  state.controller.runtimeLease = {
    epoch: state.compute.epoch,
    renewalSequence: state.compute.renewalSequence,
    expiresAt: state.now + protocol.runtimeLeaseMs,
  };
  state.compute.lastAckResult = "Accepted";
  state.compute.lastCommandAckAt = null;
  state.compute.authority = "Arming";
  receive(state, "RuntimeLeaseAck", "Accepted · feature is Arming");
}

function validateLease(state) {
  if (state.compute.observedBoot !== state.controller.bootSession) {
    return "RejectedBootSession";
  }

  if (state.controller.lifecycle !== "Ready") {
    return "RejectedNotReady";
  }

  if (state.controller.fault === "Latched") {
    return "RejectedFaultLatched";
  }

  if (state.compute.configGeneration !== state.controller.configGeneration) {
    return "RejectedConfigGeneration";
  }

  const current = state.controller.runtimeLease;
  if (
    current &&
    (state.compute.epoch < current.epoch ||
      (state.compute.epoch === current.epoch &&
        state.compute.renewalSequence <= current.renewalSequence))
  ) {
    return "RejectedEpoch";
  }

  return null;
}

function command(state, setpoint, overrides = {}) {
  if (
    state.compute.observedBoot === null ||
    state.compute.configGeneration === null
  ) {
    record(
      state,
      "host",
      "Command denied locally",
      "Discover the controller first",
    );
    return;
  }

  const epoch = overrides.epoch ?? state.compute.epoch;
  const sequence = overrides.sequence ?? state.compute.nextCommandSequence;
  if (!overrides.keepSequence) {
    state.compute.nextCommandSequence += 1;
  }

  transmit(
    state,
    "Command",
    `boot=${state.compute.observedBoot} · epoch=${epoch} · sequence=${sequence} · radiator=${setpoint}%`,
  );
  if (!state.linkUp) {
    return;
  }

  const rejection = validateCommand(state, epoch, sequence, setpoint);
  if (rejection) {
    state.compute.lastAckResult = rejection;
    receive(
      state,
      "CommandAck",
      `${rejection} · holding ${state.controller.acceptedSetpoint}% only within existing lease`,
    );
    return;
  }

  state.controller.mode = "Commanded";
  state.controller.acceptedSetpoint = setpoint;
  state.controller.lastCommandSequence = sequence;
  state.controller.commandExpiresAt = state.now + protocol.commandLeaseMs;

  state.compute.mode = "Commanded";
  state.compute.fault = state.controller.fault;
  state.compute.lastAckResult = "Accepted";
  state.compute.lastCommandAckAt = state.now;
  state.compute.authority = "Active";
  receive(
    state,
    "CommandAck",
    `Accepted · sequence=${sequence} · radiator=${setpoint}%`,
  );
}

function validateCommand(state, epoch, sequence, setpoint) {
  if (state.compute.observedBoot !== state.controller.bootSession) {
    return "RejectedBootSession";
  }

  if (state.controller.lifecycle !== "Ready") {
    return "RejectedNotReady";
  }

  if (state.controller.fault === "Latched") {
    return "RejectedFaultLatched";
  }

  if (state.compute.configGeneration !== state.controller.configGeneration) {
    return "RejectedConfigGeneration";
  }

  if (setpoint < 10 || setpoint > 90) {
    return "RejectedOutOfRange";
  }

  const lease = state.controller.runtimeLease;
  if (!lease || lease.epoch !== epoch || lease.expiresAt <= state.now) {
    return "RejectedEpoch";
  }

  if (
    state.controller.lastCommandSequence !== null &&
    sequence <= state.controller.lastCommandSequence
  ) {
    return "RejectedSequence";
  }

  return null;
}

function advanceTime(state, delta) {
  state.now += delta;

  if (
    state.controller.runtimeLease &&
    state.controller.runtimeLease.expiresAt <= state.now
  ) {
    state.controller.runtimeLease = null;
    selectFallback(state, "Runtime Lease expired");
  } else if (
    state.controller.commandExpiresAt !== null &&
    state.controller.commandExpiresAt <= state.now
  ) {
    selectFallback(state, "Command Lease expired");
  }

  if (state.controller.nextHeartbeatAt <= state.now) {
    while (state.controller.nextHeartbeatAt <= state.now) {
      state.controller.nextHeartbeatAt += protocol.heartbeatPeriodMs;
    }

    heartbeat(state);
  }

  if (
    state.compute.lastCommandAckAt !== null &&
    state.now - state.compute.lastCommandAckAt >= protocol.ackFreshnessMs
  ) {
    state.compute.authority = "Fallback";
  }

  record(
    state,
    "clock",
    `Advanced ${delta} ms`,
    `monotonic time=${state.now} ms`,
  );
}

function heartbeat(state) {
  if (!state.linkUp) {
    record(state, "drop", "Heartbeat dropped", "CAN FD link is down");
    return;
  }

  if (state.compute.observedBoot !== state.controller.bootSession) {
    state.compute.authority = "Fallback";
    state.compute.lastCommandAckAt = null;
  }

  state.compute.observedBoot = state.controller.bootSession;
  state.compute.lifecycle = state.controller.lifecycle;
  state.compute.mode = state.controller.mode;
  state.compute.fault = state.controller.fault;
  state.compute.configGeneration = state.controller.configGeneration;
  state.compute.lastHeartbeatAt = state.now;
  receive(state, "Heartbeat", heartbeatDetail(state));
}

function heartbeatDetail(state) {
  return `boot=${state.controller.bootSession} · ${state.controller.lifecycle} · ${state.controller.mode} · fault=${state.controller.fault}`;
}

function toggleLink(state) {
  state.linkUp = !state.linkUp;
  record(
    state,
    state.linkUp ? "link" : "drop",
    state.linkUp ? "CAN FD link restored" : "CAN FD link lost",
    state.linkUp
      ? "Transport is available; authority still requires reconciliation"
      : "Controller deadlines remain active locally",
  );
}

function staleSequence(state) {
  command(state, 60, {
    sequence: state.controller.lastCommandSequence ?? 0,
    keepSequence: true,
  });
}

function wrongEpoch(state) {
  command(state, 60, {
    epoch: Math.max(0, state.compute.epoch - 1),
    keepSequence: true,
  });
}

function configure(state) {
  if (state.compute.observedBoot === null) {
    record(
      state,
      "host",
      "Configuration denied locally",
      "Discover the controller first",
    );
    return;
  }

  const nextGeneration = state.controller.configGeneration + 1;
  transmit(state, "Configuration", `generation=${nextGeneration}`);
  if (!state.linkUp) {
    return;
  }

  if (state.controller.runtimeLease || state.controller.mode === "Commanded") {
    state.compute.lastAckResult = "RejectedUnderAuthority";
    receive(state, "ConfigurationAck", "RejectedUnderAuthority");
    return;
  }

  state.controller.configGeneration = nextGeneration;
  state.compute.configGeneration = nextGeneration;
  state.compute.lastAckResult = "Accepted";
  receive(state, "ConfigurationAck", `Accepted · generation=${nextGeneration}`);
}

function hardFault(state) {
  state.controller.fault = "Latched";
  state.controller.runtimeLease = null;
  selectFallback(state, "hard actuator fault latched");

  if (state.linkUp) {
    state.compute.fault = "Latched";
    state.compute.mode = "Fallback";
    state.compute.authority = "Fallback";
    receive(state, "FaultReport", "ACTUATOR_OUTPUT_FAULT · latched=true");
  } else {
    record(
      state,
      "drop",
      "FaultReport dropped",
      "Controller remains safely latched in Fallback",
    );
  }
}

function powerCycle(state) {
  const faultWasLatched = state.controller.fault === "Latched";
  state.controller.bootSession += 1;
  state.controller.lifecycle = "Initializing";
  state.controller.mode = "Fallback";
  state.controller.acceptedSetpoint = protocol.fallbackSetpoint;
  state.controller.runtimeLease = null;
  state.controller.commandExpiresAt = null;
  state.controller.lastCommandSequence = null;
  state.controller.clearFaultAfterSelfTest = faultWasLatched;
  state.controller.nextHeartbeatAt = state.now + protocol.heartbeatPeriodMs;
  record(
    state,
    "node",
    "Controller power-cycled",
    `boot=${state.controller.bootSession} · authority history discarded${
      faultWasLatched ? " · fault remains latched pending self-test" : ""
    }`,
  );
  heartbeat(state);
}

function controlledFallback(state) {
  if (state.compute.observedBoot === null) {
    record(
      state,
      "host",
      "Shutdown continues",
      "No discovered node; leases stop by omission",
    );
    return;
  }

  transmit(
    state,
    "FallbackRequest",
    `boot=${state.compute.observedBoot} · epoch=${state.compute.epoch}`,
  );
  if (!state.linkUp) {
    return;
  }

  if (
    state.compute.observedBoot === state.controller.bootSession &&
    state.controller.runtimeLease?.epoch === state.compute.epoch
  ) {
    state.controller.runtimeLease = null;
    selectFallback(state, "controlled fallback requested");
  }

  state.compute.mode = state.controller.mode;
  state.compute.authority = "Fallback";
  receive(state, "FallbackAck", `mode=${state.controller.mode}`);
}

function selectFallback(state, reason) {
  const transitioned =
    state.controller.mode !== "Fallback" ||
    state.controller.acceptedSetpoint !== protocol.fallbackSetpoint;
  state.controller.mode = "Fallback";
  state.controller.acceptedSetpoint = protocol.fallbackSetpoint;
  state.controller.commandExpiresAt = null;

  if (transitioned) {
    record(state, "node", "Controller selected Fallback", reason);
  }
}

function transmit(state, title, detail) {
  record(state, "tx", title, detail);
  if (!state.linkUp) {
    record(state, "drop", `${title} dropped`, "CAN FD link is down");
  }
}

function receive(state, title, detail) {
  record(state, "rx", title, detail);
}

function reconcile(state) {
  if (
    state.compute.lastCommandAckAt !== null &&
    state.now - state.compute.lastCommandAckAt >= protocol.ackFreshnessMs
  ) {
    state.compute.authority = "Fallback";
  }

  if (
    state.compute.fault === "Latched" ||
    state.compute.lifecycle === "Initializing"
  ) {
    state.compute.authority = "Fallback";
  }
}
