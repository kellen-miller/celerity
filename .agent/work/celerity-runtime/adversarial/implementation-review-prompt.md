# Celerity implementation adversarial review

You are the sole implementation-boundary adversarial reviewer for Celerity. The author is
OpenAI `gpt-5.6-sol`; you are Anthropic `claude-opus-4-8` at medium effort. This is a
cross-provider, read-only review. Do not modify files, run commands, push branches, edit the
pull request, or change any external state.

## Repository and review boundary

- Repository/worktree: `/Users/kellen/development/github/kellen-miller/celerity/.worktrees/celerity-runtime`
- Branch: `feat/celerity-runtime`
- Pull request: `https://github.com/kellen-miller/celerity/pull/17` (draft)
- Review base: `origin/main`
- Current reviewed source head: `e55df1c8b01a71852c1c855d5ba7f26a2173dfea`
- Historical planning base recorded by the work item: `a993404daef379ef8d8943d054d2b3ebddc23870`

Review the complete implementation in `origin/main...HEAD`, plus the current uncommitted
work-item updates in `.agent/work/celerity-runtime/execplan.md` and `meta.json`. Read the code,
contracts, tests, scripts, Helm chart, systemd units, firmware, site, and validation report
needed to verify claims. Do not rely on filenames or summaries alone.

Start with:

- `CONTEXT.md`
- `.agent/work/celerity-runtime/decision.md`
- `.agent/work/celerity-runtime/execplan.md`
- `.agent/work/celerity-runtime/meta.json`
- `.agent/work/celerity-runtime/adversarial/plan-review.md`
- `docs/validation/hardware-free-report.md`
- `.github/workflows/ci.yml`
- `scripts/check-*`

The planning adversarial review already happened and its six findings were dispositioned. Do
not repeat planning review. A normal implementation closeout review also already found and led
to fixes for two serious defects: production `celerityd` did not execute the live control loop,
and model optimization could not reach actuator commands. Look for remaining implementation
defects, including defects introduced by those corrections.

## Fixed intent and constraints

Celerity is a greenfield vehicle control runtime for one owner's street/track RX-7. The first
feature controls a duct split, with future feature growth kept possible without adding a plugin
platform now. Production vehicle compute is generic Linux, not Raspberry-Pi-specific. Haltech
Elite 2500 and CANTCU traffic is receive-only Classical CAN. Celerity's actuator network is ISO
CAN FD from day one. The STM32G431 duct controller is trusted open-loop actuator truth; there is
currently no position feedback. Normal operation is always active. Experiment mode requires an
explicit owner-authored Experiment Plan but has no stationary gate.

Rust owns the vehicle runtime, CAN contracts, synchronization, model inference, optimizer, and
firmware protocol. Python/PyTorch owns the home trainer and runs as one application replica in
Kubernetes. SvelteKit owns the public architecture walkthrough. Home upload/train/model download
is automatic on home-network availability, but model activation occurs only at process startup
using two slots; there is no shadow mode or mid-drive swap. The home deployment uses Helm,
SQLite/filesystem on a PVC, External Secrets Operator, and a pre-existing 1Password
`ClusterSecretStore`. Do not demand generic fleet, enterprise, dynamic-plugin, multi-user, or
Pi-specific machinery. Prefer direct cohesive lifecycle code and minimal complexity.

No physical hardware is installed. Software must be mockable, and evidence must not claim
electrical, target-HIL timing, servo/mechanism calibration, thermal/model accuracy, or road/track
proof.

## Current evidence

- Fresh GitHub Actions run `30673118789` is green at reviewed head `e55df1c`: Linux
  `check-linux` (including production vCAN composition, systemd lifecycle, ARM64 link, firmware),
  Ubuntu portable, and macOS portable all passed.
- Local host: `./scripts/check-rust --portable`, `./scripts/check-python`,
  `./scripts/check-e2e`, `./scripts/check-artifacts`, `./scripts/check-site`, and
  `git diff --check` passed during implementation.
- Native Linux container: workspace Clippy with `-D warnings`, workspace tests including the
  production vCAN composition, and the full Python gate passed.
- A Linux-only live lifecycle failure was fixed by treating a validated `CommandAck` as fresh
  controller truth and making emulator/firmware heartbeat cadence use accepted configuration.
- An Ubuntu-only PyTorch/ONNX absolute drift of `1.52587890625e-05` degrees was fixed with one
  shared `1e-4`-degree parity boundary, still far below sensor resolution, plus boundary tests.
- The normal closeout review is already consumed and must not be rerun. This is the exactly-one
  implementation adversarial event.

## Adversarial focus

Prioritize only material, evidence-backed defects. In particular, reconstruct and challenge:

1. The real production composition from Linux CAN frames through controller discovery,
   configuration, leases, acknowledgements, authority selection, optimizer choice, command
   transmission, immutable Run evidence, and fallback/recovery. Check time domains, freshness,
   reboot/generation handling, duplicate/stale frames, command truth, lease behavior, and failure
   ordering.
2. Whether a promoted model is genuinely fitted from admitted canonical Run observations, is
   evaluated without leakage, produces an immutable compatible bundle, reaches sync staging, is
   selected only at startup, passes `tract` inference/eligibility gates, and can produce an
   accepted model-sourced actuator command. Challenge digest and promotion truth, two-slot
   rollback, persistent invalidity, baseline comparisons, parity thresholds, and crash windows.
3. Home job durability and automation: complete-Run admission, SQLite/process ownership,
   restart recovery, concurrent starts, terminal states, webhook independence, API token use,
   network-triggered sync, exact acknowledgement, and retention ownership.
4. The RX-only Classical CAN boundary and separate CAN FD control bus. Look for accidental TX,
   wrong interface assumptions, unvalidated controller mode claims, and behavior that differs
   between vCAN and real SocketCAN.
5. Controller emulator/STM32 firmware conformance to the same codec and lifecycle, including
   configuration, heartbeat timing, watchdog/fault/fallback behavior, message RAM, and any
   emulator-only success path.
6. systemd startup/shutdown/watchdog behavior, ARM64 portability, bounded storage behavior, and
   whether CI actually exercises the production entrypoints it claims.
7. Helm split templates, one-replica/PVC/SQLite assumptions, probes, security context, config,
   ExternalSecret/1Password references, and compatibility with use as a dependency/proxy chart.
8. Public site and documentation claims versus actual implementation and evidence boundaries.
9. Unnecessary complexity, optional wiring that silently disables required behavior, test-only
   seams, hidden side effects, duplicated normalization/validation paths, or over-factored control
   flow. Do not invent speculative abstractions as fixes.

Do not report missing physical/HIL/road evidence when it is already explicitly unclaimed. Do
report software that falsely claims or bypasses such evidence. Treat green tests as evidence to
inspect, not proof that no defect exists.

## Output contract

Write a concise Markdown review. Begin with the verification you actually performed. For every
finding include:

- sequential finding number and severity (`CRITICAL`, `HIGH`, `MEDIUM`, or `LOW`);
- precise file path and line/evidence references;
- the concrete failure mode and user/vehicle impact;
- the smallest direct fix or check that resolves it.

Do not include stylistic preferences, generic hardening, speculative future scope, or duplicate
variants of the same root cause. Explicitly list important areas checked clean. End with exactly
this machine-readable block and no text after it:

```text
---ADVERSARIAL_REVIEW_STATUS---
AUTHOR_PROVIDER: OpenAI
REVIEWER_PROVIDER: Anthropic
REVIEWER_MODEL: Claude Opus
INDEPENDENCE: cross_provider
ISSUES_FOUND: <integer>
CRITICAL_COUNT: <integer>
HIGH_COUNT: <integer>
MEDIUM_COUNT: <integer>
LOW_COUNT: <integer>
CONFIDENCE: <LOW|MEDIUM|HIGH>
BLOCKING: <true|false>
SUMMARY: <one line>
---END_ADVERSARIAL_REVIEW_STATUS---
```
