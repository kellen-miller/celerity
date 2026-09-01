<script lang="ts">
  import { untrack } from "svelte";
  import StatusCard from "./StatusCard.svelte";
  import "./app.css";
  import {
    commandRatio,
    commandSourceLabel,
    diagnosticStatusLabel,
    featureAuthorityLabel,
    globalAuthorityLabel,
    initialView,
    loadStatus,
    runStorageLabel,
    temperatureAgeNow,
    type DiagnosticStatus,
    type ObservedTemperature,
    type ViewerStatus,
  } from "./status";

  type Tone = "current" | "warning" | "danger" | "neutral";

  let view = $state(initialView(performance.now()));
  let now = $state(performance.now());
  let snapshot = $derived(view.envelope.snapshot);
  let banner = $derived(bannerFor(view));
  let unavailable = $derived(view.envelope.state === "unavailable");

  $effect(() => {
    let active = true;
    const refresh = async () => {
      const next = await loadStatus(untrack(() => view));
      if (active) view = next;
    };
    void refresh();
    const poll = window.setInterval(() => void refresh(), 1_000);
    const clock = window.setInterval(() => (now = performance.now()), 250);
    return () => {
      active = false;
      window.clearInterval(poll);
      window.clearInterval(clock);
    };
  });

  function bannerFor(status: ViewerStatus): {
    title: string;
    detail: string;
    icon: string;
    tone: Tone;
  } {
    const envelope = status.envelope;
    if (envelope.state === "current") {
      return {
        title: "CONNECTED / RUNTIME UPDATING",
        detail: `LAST READ ${formatAge(envelope.last_success_age_ms)} AGO`,
        icon: "◆",
        tone: "current",
      };
    }
    if (envelope.state === "stale" && envelope.consecutive_failures === 0) {
      return {
        title: "CONNECTED / RUNTIME UPDATE DELAYED",
        detail: `PRODUCER UPDATE ${formatAge(envelope.snapshot?.runtime_update_age_ms ?? null)} OLD`,
        icon: "△",
        tone: "warning",
      };
    }
    if (envelope.state === "stale") {
      return {
        title: `DIAGNOSTICS STALE / LAST READ ${formatAge(envelope.last_success_age_ms)} AGO`,
        detail: `${envelope.consecutive_failures} CONSECUTIVE READ MISSES`,
        icon: "△",
        tone: "warning",
      };
    }
    return {
      title: "STATUS UNAVAILABLE",
      detail:
        envelope.last_success_age_ms === null
          ? (status.reason ?? "NOT YET OBSERVED")
          : `${status.reason ?? "DIAGNOSTICS UNAVAILABLE"} / LAST READ ${formatAge(envelope.last_success_age_ms)} AGO`,
      icon: "!",
      tone: "danger",
    };
  }

  function temperatureValue(
    temperature: ObservedTemperature | null | undefined,
  ): string {
    if (unavailable) return "UNAVAILABLE";
    return temperature
      ? `${temperature.degrees_celsius.toFixed(1)} °C`
      : "UNKNOWN";
  }

  function temperatureDetail(
    temperature: ObservedTemperature | null | undefined,
  ): string {
    if (unavailable) return "NO TRUSTWORTHY STATUS SNAPSHOT";
    return temperature
      ? `AGE NOW ${formatAge(temperatureAgeNow(temperature, view, now))}`
      : "NO TEMPERATURE EVIDENCE";
  }

  function statusTone(status: DiagnosticStatus | undefined): Tone {
    if (unavailable) return "danger";
    if (view.envelope.state === "stale") return "warning";
    if (status?.state === "healthy") return "current";
    if (status?.state === "unhealthy") return "danger";
    return "neutral";
  }

  function authorityTone(value: string | undefined): Tone {
    if (unavailable) return "danger";
    if (view.envelope.state === "stale") return "warning";
    if (value === "hard_fault") return "danger";
    if (value === "fallback" || value === "arming") return "warning";
    return "current";
  }

  function fieldValue(value: string | null | undefined): string {
    return unavailable ? "UNAVAILABLE" : (value ?? "UNKNOWN");
  }

  function formatAge(milliseconds: number | null): string {
    if (milliseconds === null) return "UNKNOWN";
    if (milliseconds < 1_000) return `${milliseconds} MS`;
    return `${(milliseconds / 1_000).toFixed(1)} S`;
  }
</script>

<a class="skip-link" href="#status-grid">Skip to status values</a>
<main>
  <header class="masthead">
    <div>
      <p class="eyebrow">CELERITY // VEHICLE LOCAL</p>
      <h1>Runtime status</h1>
    </div>
    <p class="read-only">
      <span aria-hidden="true">◇</span> READ-ONLY OBSERVER
    </p>
  </header>

  <section class="banner banner--{banner.tone}">
    <span class="banner__icon" aria-hidden="true">{banner.icon}</span>
    <div>
      <div
        role="status"
        aria-live="polite"
        aria-atomic="true"
        aria-label={banner.title}
      >
        <h2>{banner.title}</h2>
      </div>
      <p aria-live="off">{banner.detail}</p>
    </div>
  </section>

  <section class="status-section" aria-labelledby="primary-heading">
    <h2 id="primary-heading">Primary evidence</h2>
    <div class="primary-grid">
      <StatusCard
        label="Coolant temperature"
        value={temperatureValue(snapshot?.coolant_temperature)}
        detail={temperatureDetail(snapshot?.coolant_temperature)}
        icon="TEMP"
        tone={unavailable
          ? "danger"
          : view.envelope.state === "stale"
            ? "warning"
            : snapshot?.coolant_temperature
              ? "current"
              : "neutral"}
        primary
      />
      <StatusCard
        label="Radiator Split Command"
        value={unavailable
          ? "UNAVAILABLE"
          : snapshot?.accepted_radiator_split_command
            ? commandRatio(
                snapshot.accepted_radiator_split_command.basis_points,
              )
            : "NO ACCEPTED COMMAND"}
        detail={unavailable
          ? "NO TRUSTWORTHY STATUS SNAPSHOT"
          : snapshot?.accepted_radiator_split_command
            ? `ACCEPTED / ${commandSourceLabel(snapshot.accepted_radiator_split_command.source)}`
            : "NO MATCHING CONTROLLER ACKNOWLEDGEMENT"}
        icon="CMD"
        tone={unavailable
          ? "danger"
          : view.envelope.state === "stale"
            ? "warning"
            : snapshot?.accepted_radiator_split_command
              ? "current"
              : "neutral"}
        primary
      />
    </div>
  </section>

  <section class="status-section secondary" aria-labelledby="secondary-heading">
    <h2 id="secondary-heading">Runtime evidence</h2>
    <div class="status-grid" id="status-grid">
      <StatusCard
        label="Intake-air temperature"
        value={temperatureValue(snapshot?.intake_air_temperature)}
        detail={temperatureDetail(snapshot?.intake_air_temperature)}
        icon="IAT"
        tone={unavailable
          ? "danger"
          : view.envelope.state === "stale"
            ? "warning"
            : "neutral"}
      />
      <StatusCard
        label="Global authority"
        value={fieldValue(
          snapshot && globalAuthorityLabel(snapshot.global_authority),
        )}
        detail="CELERITY AUTHORITY SUMMARY"
        icon="AUTH"
        tone={authorityTone(snapshot?.global_authority)}
      />
      <StatusCard
        label="Feature authority"
        value={fieldValue(
          snapshot && featureAuthorityLabel(snapshot.feature_authority),
        )}
        detail="RADIATOR SPLIT FEATURE"
        icon="FEAT"
        tone={authorityTone(snapshot?.feature_authority)}
      />
      <StatusCard
        label="Command source"
        value={fieldValue(
          snapshot && commandSourceLabel(snapshot.command_source),
        )}
        detail="SOURCE OF CURRENT RUNTIME REQUEST"
        icon="SRC"
        tone={authorityTone(snapshot?.feature_authority)}
      />
      <StatusCard
        label="Controller runtime lease"
        value={fieldValue(
          snapshot &&
            diagnosticStatusLabel(snapshot.controller_runtime_lease_health),
        )}
        detail="EXPECTATION-AWARE LEASE EVIDENCE"
        icon="LEASE"
        tone={statusTone(snapshot?.controller_runtime_lease_health)}
      />
      <StatusCard
        label="Controller command ACK"
        value={fieldValue(
          snapshot &&
            diagnosticStatusLabel(snapshot.controller_command_ack_health),
        )}
        detail="TWO-SECOND ACK SUMMARY"
        icon="ACK"
        tone={statusTone(snapshot?.controller_command_ack_health)}
      />
      <StatusCard
        label="Run storage"
        value={fieldValue(
          snapshot && runStorageLabel(snapshot.run_storage_health),
        )}
        detail="WRITE-PATH EVIDENCE; VIEWER DOES NOT READ RUNS"
        icon="RUN"
        tone={unavailable
          ? "danger"
          : view.envelope.state === "stale"
            ? "warning"
            : snapshot?.run_storage_health === "healthy"
              ? "current"
              : snapshot?.run_storage_health === "degraded"
                ? "danger"
                : "neutral"}
      />
    </div>
  </section>
</main>
