<script lang="ts">
  import { onMount } from 'svelte';

  onMount(() => {
    const frame = window.requestAnimationFrame(() => {
      if (window.location.hash.length <= 1) return;
      try {
        document
          .getElementById(decodeURIComponent(window.location.hash.slice(1)))
          ?.scrollIntoView();
      } catch {
        // An invalid percent-encoded fragment has no matching durable section.
      }
    });
    return () => window.cancelAnimationFrame(frame);
  });

  const steps = [
    [
      '01',
      'Observe',
      'Powertrain CAN is receive-only. Raw frames become ordered, typed observations.'
    ],
    [
      '02',
      'Gate',
      'Freshness, controller truth, storage, and shared health form one visible authority gate.'
    ],
    [
      '03',
      'Choose',
      'Deterministic protection is always available; eligible ONNX prediction may optimize within it.'
    ],
    [
      '04',
      'Shape',
      'One command shaper clamps, slews, and hands off from the last acknowledged position.'
    ],
    [
      '05',
      'Lease',
      'The reconciler alone issues runtime and command leases to the dedicated controller.'
    ],
    [
      '06',
      'Record',
      'Every observation, decision, transition, frame, and fault enters an immutable Run.'
    ]
  ];

  const evidence = [
    [
      'Hardware-free',
      'Protocol golden vectors',
      'Exact IDs, lengths, endian layout, reserved bytes, and range rejection.'
    ],
    [
      'Hardware-free',
      'Logical-time runtime',
      'Repeatable authority, stale-input, reboot, rejection, and lease-expiry scenarios.'
    ],
    [
      'Hardware-free',
      'Real software artifacts',
      'Embedded release link, ONNX/tract execution, SQLite recovery, systemd CI, and static build.'
    ],
    [
      'Physical only',
      'Electrical boundary',
      'Fail-silent tap, transceivers, termination, harness, power, EMC, and on-wire silence.'
    ],
    [
      'Physical only',
      'Mechanism and timing',
      'PWM behavior under target load, passive return, calibration, settling, and backlash.'
    ],
    [
      'Physical only',
      'Vehicle acceptance',
      'Live signal validation, model envelope, thermal trials, installation, road, and track evidence.'
    ]
  ];
</script>

<svelte:head>
  <title>Celerity — bounded vehicle control</title>
  <link
    rel="icon"
    href="data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 32 32'%3E%3Crect width='32' height='32' fill='%23090b0c'/%3E%3Cpath d='M23 9a10 10 0 1 0 0 14' fill='none' stroke='%23e7a947' stroke-width='3'/%3E%3C/svg%3E"
  />
  <meta
    name="description"
    content="Architecture and evidence boundaries for the Celerity vehicle control runtime"
  />
</svelte:head>

<header>
  <a class="skip-link" href="#content">Skip to content</a>
  <a class="brand" href="#top" aria-label="Celerity architecture home">
    <span class="mark">C</span><span>CELERITY</span>
  </a>
  <nav aria-label="Walkthrough sections">
    <a href="#authority">Authority</a>
    <a href="#lifecycle">Lifecycle</a>
    <a href="#learning">Learning</a>
    <a href="#evidence">Evidence</a>
  </nav>
  <span class="document-label">PROJECT DOCUMENTATION · V1</span>
</header>

<main id="content">
  <section id="top" class="hero" aria-labelledby="hero-title">
    <div class="eyebrow">
      <span></span> ACTIVE VEHICLE SYSTEMS / EXPLICIT AUTHORITY
    </div>
    <h1 id="hero-title">Control that fails<br /><em>toward safety.</em></h1>
    <p class="lede">
      Celerity is a host-native control runtime and dedicated actuator
      controller for an FD RX-7. Every path from observation to physical command
      is bounded, leased, recorded, and inspectable.
    </p>
    <div class="boundary-callout">
      <strong>Architecture walkthrough</strong>
      <span
        >This static site explains the system. It is not a driver interface and
        exposes no controls.</span
      >
    </div>
    <a class="continue" href="#authority"
      >Follow the control path <span aria-hidden="true">↓</span></a
    >
  </section>

  <section id="authority" class="section" aria-labelledby="authority-title">
    <p class="section-number">01 / OWNERSHIP</p>
    <h2 id="authority-title">One writer. Narrow boundaries.</h2>
    <p class="intro">
      The vehicle runtime is the only process that can construct authority.
      Network sync, home training, diagnostics, and this site remain outside the
      controller transport boundary.
    </p>
    <div class="ownership" role="list">
      <article class="observe" role="listitem">
        <small>RX-ONLY</small>
        <h3>Powertrain observation</h3>
        <p>
          Timestamped Linux SocketCAN receive path. No production transmit API
          exists.
        </p>
      </article>
      <span class="flow" aria-hidden="true">→</span>
      <article class="control" role="listitem">
        <small>SOLE EXECUTOR</small>
        <h3>celerityd</h3>
        <p>
          Freezes state, evaluates gates, selects policy, validates intent, and
          records evidence.
        </p>
      </article>
      <span class="flow" aria-hidden="true">→</span>
      <article class="physical" role="listitem">
        <small>LEASED OUTPUT</small>
        <h3>Duct controller</h3>
        <p>
          Applies accepted PWM commands and returns locally to radiator-biased
          fallback on expiry.
        </p>
      </article>
    </div>
    <aside class="rule">
      <span>BOUNDARY RULE</span>
      Discovery identifies a statically commissioned node. It grants no authority.
      Configuration reconciliation and fresh leases are required after every reboot.
    </aside>
  </section>

  <section
    id="lifecycle"
    class="section lifecycle"
    aria-labelledby="lifecycle-title"
  >
    <p class="section-number">02 / CONTROL CYCLE</p>
    <h2 id="lifecycle-title">The lifecycle reads top to bottom.</h2>
    <div class="steps">
      {#each steps as step (step[0])}
        <article>
          <span>{step[0]}</span>
          <div>
            <h3>{step[1]}</h3>
            <p>{step[2]}</p>
          </div>
        </article>
      {/each}
    </div>
    <div class="states" aria-label="Authority state progression">
      <span>FALLBACK</span><b>→</b><span>ARMING</span><b>→</b><span
        class="active">ACTIVE</span
      >
      <i>Any failed invariant stops renewal</i><b>↘</b><span class="fault"
        >LOCAL FALLBACK</span
      >
    </div>
  </section>

  <section id="learning" class="section" aria-labelledby="learning-title">
    <p class="section-number">03 / DECISION HIERARCHY</p>
    <h2 id="learning-title">Learning is optional. Protection is not.</h2>
    <div class="hierarchy">
      <article>
        <small>BASELINE</small>
        <h3>Deterministic policy</h3>
        <p>
          A validated monotonic 3×3 map converts coolant and post-intercooler
          temperature into a safe split.
        </p>
      </article>
      <article>
        <small>ELIGIBLE ENHANCEMENT</small>
        <h3>Prediction + finite choices</h3>
        <p>
          A synchronous ONNX model predicts thermal trajectories. The optimizer
          rejects unsafe candidates, then minimizes intake temperature and
          movement lexicographically.
        </p>
      </article>
      <article>
        <small>HARD EDGE</small>
        <h3>Budget or validity failure</h3>
        <p>
          Absent, corrupt, incompatible, out-of-distribution, nonfinite, or late
          inference selects deterministic protection. No remote service sits in
          the control loop.
        </p>
      </article>
    </div>
    <div class="run-flow">
      <strong>VEHICLE</strong><span>sealed Run</span><b>→</b><strong
        >HOME</strong
      ><span>derive · split · train · evaluate</span><b>→</b><strong
        >INACTIVE SLOT</strong
      ><span>verify · next-start activation</span>
    </div>
  </section>

  <section id="evidence" class="section" aria-labelledby="evidence-title">
    <p class="section-number">04 / EVIDENCE BOUNDARY</p>
    <h2 id="evidence-title">Prove only what the test can prove.</h2>
    <p class="intro">
      Hardware-free checks establish deterministic software behavior and
      artifact integrity. They do not substitute for electrical, target-timing,
      mechanism, thermal, or installed-vehicle evidence.
    </p>
    <div class="evidence-grid">
      {#each evidence as item (item[1])}
        <article class:physical-card={item[0] === 'Physical only'}>
          <small>{item[0]}</small>
          <h3>{item[1]}</h3>
          <p>{item[2]}</p>
        </article>
      {/each}
    </div>
    <aside class="warning">
      <strong>NOT YET CLAIMED</strong>
      No installed hardware, electrical proof, target ARM deadline, trained vehicle-model
      quality, thermal envelope, road acceptance, or track acceptance is represented
      here.
    </aside>
  </section>
</main>

<footer>
  <span>CELERITY / ARCHITECTURE V1</span><a href="#top">Back to top ↑</a>
</footer>

<style>
  :global(*) {
    box-sizing: border-box;
  }
  :global(html) {
    scroll-behavior: smooth;
    background: #090b0c;
  }
  :global(body) {
    margin: 0;
    color: #e9e6dc;
    background-color: #090b0c;
    background-image:
      linear-gradient(rgba(83, 121, 128, 0.07) 1px, transparent 1px),
      linear-gradient(90deg, rgba(83, 121, 128, 0.07) 1px, transparent 1px);
    background-size: 42px 42px;
    font-family: Inter, ui-sans-serif, system-ui, sans-serif;
  }
  :global(a) {
    color: inherit;
  }
  :global(a:focus-visible) {
    outline: 2px solid #63d6e4;
    outline-offset: 4px;
  }
  .skip-link {
    position: fixed;
    top: 12px;
    left: 12px;
    z-index: 20;
    padding: 0.7rem 0.9rem;
    color: #090b0c;
    background: #63d6e4;
    font:
      0.7rem ui-monospace,
      monospace;
    text-transform: uppercase;
    transform: translateY(-150%);
  }
  .skip-link:focus {
    transform: translateY(0);
  }
  header {
    position: sticky;
    top: 0;
    z-index: 10;
    min-height: 62px;
    display: flex;
    align-items: center;
    gap: 2rem;
    padding: 0 4vw;
    border-bottom: 1px solid #283033;
    background: rgba(9, 11, 12, 0.94);
    backdrop-filter: blur(14px);
  }
  .brand {
    display: flex;
    align-items: center;
    gap: 0.7rem;
    text-decoration: none;
    font:
      700 0.75rem/1 ui-monospace,
      monospace;
    letter-spacing: 0.2em;
  }
  .mark {
    display: grid;
    place-items: center;
    width: 28px;
    height: 28px;
    border: 1px solid #e7a947;
    color: #e7a947;
  }
  nav {
    display: flex;
    gap: 1.5rem;
    margin-left: auto;
  }
  nav a,
  footer a {
    color: #9ba6a7;
    font:
      0.7rem ui-monospace,
      monospace;
    text-decoration: none;
    text-transform: uppercase;
    letter-spacing: 0.12em;
  }
  nav a:hover,
  nav a:focus-visible,
  footer a:hover {
    color: #63d6e4;
  }
  .document-label {
    color: #e7a947;
    font:
      0.65rem ui-monospace,
      monospace;
    letter-spacing: 0.12em;
  }
  main {
    overflow: hidden;
  }
  .hero,
  .section {
    max-width: 1180px;
    margin: 0 auto;
    padding: 8rem 5vw;
    scroll-margin-top: 62px;
  }
  .hero {
    min-height: calc(100vh - 62px);
    display: flex;
    flex-direction: column;
    justify-content: center;
  }
  .eyebrow,
  .section-number {
    color: #63d6e4;
    font:
      0.7rem ui-monospace,
      monospace;
    letter-spacing: 0.15em;
  }
  .eyebrow span {
    display: inline-block;
    width: 38px;
    height: 1px;
    margin: 0 0.75rem 0.2rem 0;
    background: #63d6e4;
  }
  h1 {
    max-width: 850px;
    margin: 1.4rem 0;
    font:
      300 clamp(3.6rem, 9vw, 7.8rem)/0.9 Georgia,
      serif;
    letter-spacing: -0.055em;
  }
  h1 em {
    color: #e7a947;
    font-weight: 300;
  }
  .lede,
  .intro {
    max-width: 730px;
    color: #aeb6b5;
    font-size: clamp(1.05rem, 2vw, 1.35rem);
    line-height: 1.65;
  }
  .boundary-callout {
    max-width: 730px;
    display: grid;
    grid-template-columns: 180px 1fr;
    gap: 1.2rem;
    margin-top: 2rem;
    padding: 1.2rem;
    border-left: 2px solid #c44e42;
    background: #121617;
  }
  .boundary-callout strong,
  .warning strong {
    color: #d96154;
    font:
      0.7rem ui-monospace,
      monospace;
    letter-spacing: 0.12em;
  }
  .boundary-callout span {
    color: #bcc4c3;
  }
  .continue {
    margin-top: 3rem;
    color: #e7a947;
    font:
      0.72rem ui-monospace,
      monospace;
    text-decoration: none;
    letter-spacing: 0.1em;
    text-transform: uppercase;
  }
  .section {
    border-top: 1px solid #283033;
  }
  h2 {
    max-width: 780px;
    margin: 1rem 0 1.4rem;
    font:
      400 clamp(2.5rem, 5vw, 4.5rem)/1 Georgia,
      serif;
    letter-spacing: -0.035em;
  }
  h3 {
    margin: 0.45rem 0 0.6rem;
    font:
      500 1.15rem Georgia,
      serif;
  }
  article p,
  aside {
    color: #9ba6a7;
    line-height: 1.6;
  }
  article small {
    color: #63d6e4;
    font:
      0.62rem ui-monospace,
      monospace;
    letter-spacing: 0.15em;
  }
  .ownership {
    display: grid;
    grid-template-columns: 1fr auto 1fr auto 1fr;
    align-items: stretch;
    gap: 1rem;
    margin-top: 3rem;
  }
  .ownership article,
  .hierarchy article,
  .evidence-grid article {
    padding: 1.5rem;
    border: 1px solid #30383a;
    background: rgba(14, 18, 19, 0.9);
  }
  .ownership .control {
    border-color: #76592e;
  }
  .ownership .physical {
    border-color: #713b37;
  }
  .flow {
    align-self: center;
    color: #566164;
  }
  .rule,
  .warning {
    margin-top: 1.5rem;
    padding: 1.2rem 1.5rem;
    border: 1px solid #713b37;
    background: rgba(75, 30, 26, 0.12);
  }
  .rule span {
    margin-right: 1rem;
    color: #d96154;
    font:
      0.65rem ui-monospace,
      monospace;
  }
  .steps {
    margin-top: 3rem;
    border-top: 1px solid #30383a;
  }
  .steps article {
    display: grid;
    grid-template-columns: 80px 1fr;
    padding: 1.2rem 0;
    border-bottom: 1px solid #30383a;
  }
  .steps article > span {
    color: #e7a947;
    font:
      0.75rem ui-monospace,
      monospace;
  }
  .steps h3,
  .steps p {
    margin: 0;
  }
  .states {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 0.8rem;
    margin-top: 2rem;
    font:
      0.7rem ui-monospace,
      monospace;
  }
  .states span {
    padding: 0.65rem 0.9rem;
    border: 1px solid #5c6364;
  }
  .states .active {
    color: #e7a947;
    border-color: #e7a947;
  }
  .states .fault {
    color: #d96154;
    border-color: #d96154;
  }
  .states i {
    margin-left: auto;
    color: #8d9697;
  }
  .hierarchy {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 1rem;
    margin-top: 3rem;
  }
  .hierarchy article:nth-child(1) {
    border-top: 2px solid #e7a947;
  }
  .hierarchy article:nth-child(2) {
    border-top: 2px solid #63d6e4;
  }
  .hierarchy article:nth-child(3) {
    border-top: 2px solid #d96154;
  }
  .run-flow {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.8rem;
    margin-top: 1.5rem;
    padding: 1.2rem;
    background: #111516;
    font:
      0.67rem ui-monospace,
      monospace;
  }
  .run-flow strong {
    color: #e7a947;
  }
  .run-flow span {
    color: #8d9697;
  }
  .evidence-grid {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 1rem;
    margin-top: 3rem;
  }
  .evidence-grid .physical-card {
    border-style: dashed;
  }
  .evidence-grid .physical-card small {
    color: #d96154;
  }
  .warning {
    display: grid;
    grid-template-columns: 180px 1fr;
    gap: 1rem;
  }
  footer {
    display: flex;
    justify-content: space-between;
    padding: 2rem 4vw;
    border-top: 1px solid #283033;
    color: #687274;
    font:
      0.65rem ui-monospace,
      monospace;
    letter-spacing: 0.12em;
  }
  @media (max-width: 760px) {
    header {
      gap: 0.8rem;
      padding: 0 1rem;
    }
    nav {
      display: none;
    }
    .document-label {
      margin-left: auto;
      font-size: 0.55rem;
    }
    .hero,
    .section {
      padding: 5rem 1.3rem;
    }
    .ownership,
    .hierarchy,
    .evidence-grid {
      grid-template-columns: 1fr;
    }
    .ownership .flow {
      transform: rotate(90deg);
      justify-self: center;
    }
    .boundary-callout,
    .warning {
      grid-template-columns: 1fr;
    }
    .states i {
      flex-basis: 100%;
      margin-left: 0;
    }
  }
  @media (prefers-reduced-motion: reduce) {
    :global(html) {
      scroll-behavior: auto;
    }
  }
</style>
