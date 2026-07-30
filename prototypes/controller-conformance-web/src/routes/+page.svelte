<script>
  import {
    initialState,
    protocol,
    runAction,
    walkthrough,
  } from "$lib/simulation.js";

  let simulation = $state(initialState());
  let walkthroughIndex = $state(0);

  let currentStep = $derived(walkthrough[walkthroughIndex]);
  let staleObservation = $derived(
    simulation.compute.mode === "Commanded" &&
      simulation.controller.mode === "Fallback",
  );
  let runtimeLeaseRemaining = $derived(
    remaining(simulation.controller.runtimeLease?.expiresAt ?? null),
  );
  let commandLeaseRemaining = $derived(
    remaining(simulation.controller.commandExpiresAt),
  );

  function run(action) {
    runAction(simulation, action);
  }

  function runGuidedStep() {
    if (!currentStep) {
      reset();
      return;
    }

    run(currentStep.action);
    walkthroughIndex += 1;
  }

  function reset() {
    simulation = initialState();
    walkthroughIndex = 0;
  }

  function remaining(expiresAt) {
    return expiresAt === null ? 0 : Math.max(0, expiresAt - simulation.now);
  }

  function percentage(remainingMs, totalMs) {
    return Math.min(100, (remainingMs / totalMs) * 100);
  }

  function display(value, fallback = "Unknown") {
    return value === null || value === undefined ? fallback : String(value);
  }
</script>

<svelte:head>
  <title>Celerity · Controller Contract Lab</title>
  <meta
    name="description"
    content="Interactive Celerity controller conformance protocol prototype."
  />
</svelte:head>

<main class="shell">
  <header class="masthead">
    <div>
      <p class="eyebrow">Celerity / Protocol study 01</p>
      <h1>Controller contract lab</h1>
      <p class="lede">
        Drive the Vehicle Compute Node and duct controller through startup,
        authority, message loss, rejection, and recovery.
      </p>
    </div>
    <div
      class="prototype-stamp"
      aria-label="Throwaway prototype, not vehicle code"
    >
      <span>Throwaway</span>
      <strong>Not vehicle code</strong>
    </div>
  </header>

  <section class="status-rail" aria-label="Simulation status">
    <div class="rail-item">
      <span>Monotonic time</span>
      <strong>{simulation.now.toLocaleString()} ms</strong>
    </div>
    <div class="rail-item">
      <span>CAN FD link</span>
      <strong
        class="status-pill"
        data-tone={simulation.linkUp ? "healthy" : "fault"}
      >
        {simulation.linkUp ? "Up" : "Down"}
      </strong>
    </div>
    <div class="rail-item rail-authority">
      <span>Feature authority</span>
      <strong
        class="status-pill"
        data-tone={simulation.compute.authority === "Active"
          ? "active"
          : simulation.compute.authority === "Arming"
            ? "healthy"
            : "fallback"}
      >
        {simulation.compute.authority}
      </strong>
    </div>
    <button class="text-button" type="button" onclick={reset}
      >Reset scenario</button
    >
  </section>

  <section class="guide" aria-labelledby="guide-title">
    <div>
      <p class="section-kicker">Guided failure walkthrough</p>
      <h2 id="guide-title">Make the hidden transitions visible</h2>
      <p>
        {currentStep?.copy ??
          "The controller recovered through discovery, ordered leases, and a fresh command."}
      </p>
    </div>
    <div class="guide-action">
      <span>
        {currentStep
          ? `Step ${walkthroughIndex + 1} of ${walkthrough.length}`
          : "Walkthrough complete"}
      </span>
      <button class="primary-action" type="button" onclick={runGuidedStep}>
        {currentStep?.label ?? "Run it again"}
      </button>
    </div>
  </section>

  <section
    class="truth-grid"
    aria-label="Desired and observed controller state"
  >
    <article class="truth-card compute-card">
      <div class="card-heading">
        <div>
          <p class="section-kicker">Celerity last observed</p>
          <h2>Vehicle Compute Node</h2>
        </div>
        <span class="node-mark">VCN</span>
      </div>

      <dl class="state-list">
        <div>
          <dt>Boot session</dt>
          <dd>{display(simulation.compute.observedBoot)}</dd>
        </div>
        <div>
          <dt>Lifecycle</dt>
          <dd>{display(simulation.compute.lifecycle)}</dd>
        </div>
        <div>
          <dt>Actuation mode</dt>
          <dd>{display(simulation.compute.mode)}</dd>
        </div>
        <div>
          <dt>Fault latch</dt>
          <dd>{display(simulation.compute.fault)}</dd>
        </div>
        <div>
          <dt>Capability gen.</dt>
          <dd>{display(simulation.compute.capabilityGeneration)}</dd>
        </div>
        <div>
          <dt>Configuration</dt>
          <dd>{display(simulation.compute.configGeneration)}</dd>
        </div>
        <div>
          <dt>Runtime epoch</dt>
          <dd>{simulation.compute.epoch}</dd>
        </div>
        <div>
          <dt>Next command</dt>
          <dd>{simulation.compute.nextCommandSequence}</dd>
        </div>
        <div>
          <dt>Last command ACK</dt>
          <dd>
            {simulation.compute.lastCommandAckAt === null
              ? "Never"
              : `${simulation.compute.lastCommandAckAt} ms · ${simulation.compute.lastAckResult}`}
          </dd>
        </div>
        <div>
          <dt>Last heartbeat</dt>
          <dd>
            {simulation.compute.lastHeartbeatAt === null
              ? "Never"
              : `${simulation.compute.lastHeartbeatAt} ms`}
          </dd>
        </div>
      </dl>
    </article>

    <div class="link-column" aria-hidden="true">
      <span>CAN FD</span>
      <div class="link-line" data-down={String(!simulation.linkUp)}></div>
      <small>500 kbit/s · 2 Mbit/s</small>
    </div>

    <article class="truth-card controller-card">
      <div class="card-heading">
        <div>
          <p class="section-kicker">Physical authority</p>
          <h2>Controller truth</h2>
        </div>
        <span class="node-mark">{protocol.nodeId}</span>
      </div>

      <dl class="state-list">
        <div>
          <dt>Boot session</dt>
          <dd>{simulation.controller.bootSession}</dd>
        </div>
        <div>
          <dt>Lifecycle</dt>
          <dd>{simulation.controller.lifecycle}</dd>
        </div>
        <div>
          <dt>Actuation mode</dt>
          <dd>{simulation.controller.mode}</dd>
        </div>
        <div>
          <dt>Fault latch</dt>
          <dd>{simulation.controller.fault}</dd>
        </div>
        <div>
          <dt>Configuration</dt>
          <dd>{simulation.controller.configGeneration}</dd>
        </div>
        <div>
          <dt>Accepted setpoint</dt>
          <dd>{simulation.controller.acceptedSetpoint}% radiator</dd>
        </div>
        <div>
          <dt>Last sequence</dt>
          <dd>{display(simulation.controller.lastCommandSequence, "None")}</dd>
        </div>
      </dl>

      <div class="lease-block">
        <div class="lease-label">
          <span>Runtime Lease</span>
          <strong>
            {simulation.controller.runtimeLease
              ? `Epoch ${simulation.controller.runtimeLease.epoch} · ${runtimeLeaseRemaining} ms`
              : "None"}
          </strong>
        </div>
        <div class="lease-track">
          <span
            style:width={`${percentage(runtimeLeaseRemaining, protocol.runtimeLeaseMs)}%`}
          ></span>
        </div>
      </div>

      <div class="lease-block">
        <div class="lease-label">
          <span>Command Lease</span>
          <strong>
            {simulation.controller.commandExpiresAt === null
              ? "None"
              : `Fresh command · ${commandLeaseRemaining} ms`}
          </strong>
        </div>
        <div class="lease-track command-track">
          <span
            style:width={`${percentage(commandLeaseRemaining, protocol.commandLeaseMs)}%`}
          ></span>
        </div>
      </div>
    </article>
  </section>

  {#if staleObservation}
    <p class="stale-warning">
      Celerity's observation is stale: its last observed actuation mode is
      Commanded while the controller has independently selected Fallback.
    </p>
  {/if}

  <section class="control-deck" aria-labelledby="control-title">
    <div class="section-heading">
      <div>
        <p class="section-kicker">Manual controls</p>
        <h2 id="control-title">Push the contract</h2>
      </div>
      <p>Every action is local to this browser. Reloading clears all state.</p>
    </div>

    <div class="control-groups">
      <fieldset>
        <legend>Startup & authority</legend>
        <div class="button-grid">
          <button type="button" onclick={() => run("selfTest")}
            >Complete self-test</button
          >
          <button type="button" onclick={() => run("discover")}
            >Discover controller</button
          >
          <button type="button" onclick={() => run("grant")}
            >Grant / renew Runtime Lease</button
          >
          <button type="button" onclick={() => run("command30")}
            >Command 30%</button
          >
          <button type="button" onclick={() => run("command70")}
            >Command 70%</button
          >
          <button type="button" onclick={() => run("fallback")}
            >Controlled fallback</button
          >
        </div>
      </fieldset>

      <fieldset>
        <legend>Time & transport</legend>
        <div class="button-grid">
          <button type="button" onclick={() => run("tick50")}
            >Advance 50 ms</button
          >
          <button type="button" onclick={() => run("tick250")}
            >Advance 250 ms</button
          >
          <button type="button" onclick={() => run("toggleLink")}
            >Toggle CAN FD link</button
          >
          <button type="button" onclick={() => run("configure")}
            >Next configuration</button
          >
        </div>
      </fieldset>

      <fieldset class="fault-controls">
        <legend>Failure injection</legend>
        <div class="button-grid">
          <button type="button" onclick={() => run("staleSequence")}
            >Stale sequence</button
          >
          <button type="button" onclick={() => run("wrongEpoch")}
            >Wrong epoch</button
          >
          <button type="button" onclick={() => run("hardFault")}
            >Latch hard fault</button
          >
          <button type="button" onclick={() => run("powerCycle")}
            >Power-cycle controller</button
          >
        </div>
      </fieldset>
    </div>
  </section>

  <section class="timeline-panel" aria-labelledby="timeline-title">
    <div class="section-heading">
      <div>
        <p class="section-kicker">Canonical event view</p>
        <h2 id="timeline-title">Protocol exchange</h2>
      </div>
      <p>{simulation.lastResult}</p>
    </div>
    <ol class="timeline" aria-live="polite">
      {#each simulation.events as entry (entry.id)}
        <li>
          <time>{entry.at} ms</time>
          <span class="event-direction" data-kind={entry.kind}
            >{entry.kind}</span
          >
          <div class="event-copy">
            <strong>{entry.title}</strong>
            <span>{entry.detail}</span>
          </div>
        </li>
      {/each}
    </ol>
  </section>

  <footer>
    <p>
      ACK means the controller accepted and electrically applied a setpoint. It
      never claims measured physical duct position.
    </p>
    <p>
      Demo timing: Runtime Lease {protocol.runtimeLeaseMs.toLocaleString()} ms · Command
      Lease
      {protocol.commandLeaseMs} ms · ACK freshness {protocol.ackFreshnessMs} ms
    </p>
  </footer>
</main>
