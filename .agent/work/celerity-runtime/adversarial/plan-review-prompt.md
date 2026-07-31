# Celerity planning-boundary adversarial review

Perform the sole planning-boundary adversarial review for the Elevated-risk Celerity `grill-plan-build` workflow.

You are a fresh cross-provider reviewer. The plan author was OpenAI
`gpt-5.6-sol` at high reasoning. You are Anthropic Claude Opus at medium
effort. Do not spawn additional reviewers.

## Read-only contract

This review is strictly read-only. Do not mutate files, repositories, branches,
worktrees, trackers, external systems, or review sessions. Do not commit, push,
create pull requests, invoke Lavish, or run state-changing commands. Your tools
already prohibit Bash, Edit, Write, and NotebookEdit. Return the complete review
as Markdown in your final response; the orchestrator owns artifact storage and
finding disposition.

## Repository packet

- Repository: `/Users/kellen/development/github/kellen-miller/celerity`
- Worktree: `/Users/kellen/development/github/kellen-miller/celerity/.worktrees/celerity-runtime`
- Branch: `feat/celerity-runtime`
- Base: `planning/duct-mechanism-contract` at
  `a993404daef379ef8d8943d054d2b3ebddc23870`
- Upstream: none
- Work item: `.agent/work/celerity-runtime`
- Approved revision: 1
- Approved fingerprint:
  `da11869e9f441af334579edcf9e5cbdbcdb29ed2a5c7fee9f2237db8ed11d855`
- Lavish approval was explicitly submitted with no requested changes.

Read these files in full:

- `.agent/work/celerity-runtime/decision.md`
- `.agent/work/celerity-runtime/execplan.md`
- `.agent/work/celerity-runtime/meta.json`
- `.agent/PLANS.md`
- `CONTEXT.md`
- `README.md`
- `.gitignore`

Inspect the closed Wayfinder map at
<https://github.com/kellen-miller/celerity/issues/1>, linked issues #2 through
#16, and the named evidence branches only when necessary to verify a concrete
claim. The evidence branches are `feat/controller-conformance-prototype`,
`research/actuator-controller-hardware`, `research/can-fd-hardware`,
`research/haltech-cantcu-telemetry`, and `research/thermal-model-optimizer`.

## Goal and hard constraints

- This is one continuous implementation effort through every buildable and
  testable hardware-free subsystem. Dependency milestones express ordering
  only; they are not releases, deferments, gates, or acceptable stopping points.
- Immediate scope is the complete Rust runtime; fixed production v1 CAN FD,
  configuration, Run, model, and home contracts; emulator, simulation, replay,
  deterministic control, STM32G431 firmware, PyTorch causal multi-output TCN to
  ONNX to Rust `tract`, Rust optimizer, Run storage, systemd integration,
  read-only CLI, `celerity-sync`, Python home application and Kubernetes
  deployment, webhook notifications, startup-only two-slot model lifecycle,
  public SvelteKit architecture site, CI, tests, docs, and configuration.
- Only physical installation/calibration, target HIL, electrical, thermal,
  mechanical, and road proof remain external.
- Quality gates precede feature code. Rust uses rustfmt, Clippy, rust-analyzer,
  host tests/builds, ARM64 cross-build, and embedded build. Python uses `uv`
  exclusively with Ruff and tests. Svelte/TypeScript uses format, lint,
  `svelte-check`, tests, and production build. CI uses the same checked-in
  developer commands.
- Celerity is receive-only on the Haltech/CANTCU Classical CAN network and
  separately controls its CAN FD actuator network. Authority, fault, lease,
  startup, fallback, and open-loop truth must remain explicit.
- This is a greenfield final shape: no compatibility shims or translators, no
  provisional protocol, no dynamic plugins, no fleet/enterprise machinery, and
  no interface created only for testing.
- The owner strongly prefers minimal complexity. Flag over-factoring, shallow
  modules, leaked sequencing/policy, unnecessary machinery, and contradictory
  requirements as well as safety, feasibility, and testability failures.

Known risks and physical limits are recorded in `decision.md`; no user judgment
is open. Planning validation already completed: exactly one useful improvement
pass (8/10), the fingerprint was recomputed, `git diff --check` passed, the
review HTML is ignored, no product code exists, and no Goal was activated.

## Review questions

Report only findings that could materially change the plan, implementation,
validation, or acceptance decision. For every finding include severity, exact
artifact/section evidence, impact, and a concrete fix or verification check. Do
not report nits and do not request backward compatibility.

Explicitly assess:

1. Whether the exact v1 CAN FD identifiers and fixed payload layouts are
   internally consistent, collision-free, and implementable on the selected
   controller.
2. Whether any plan language accidentally authorizes powertrain-bus
   transmission or leaks a transmit-capable handle.
3. Safety and liveness of session/generation/sequence/lease semantics,
   controller-local and passive fallback, shutdown, reboot, recovery, and
   open-loop acknowledgement truth.
4. Whether the data/model lifecycle can be tested without weakening production
   behavior, leaking authority, training on invalid evidence, or permitting
   mid-drive model activation.
5. Whether any confirmed immediate subsystem is omitted or subtly deferred.
6. Whether bootstrapping and CI commands are executable from this greenfield
   repository on the named platforms.
7. Whether interfaces and seams are deep and real rather than pass-through or
   test-only.
8. Whether validation claims exceed hardware-free evidence.
9. Whether complexity can be removed without deleting confirmed behavior.

Treat every issue as an evidence-backed claim. Finish with this exact block:

```text
---ADVERSARIAL_REVIEW_STATUS---
AUTHOR_PROVIDER: OpenAI
REVIEWER_PROVIDER: Anthropic
REVIEWER_MODEL: Claude Opus
INDEPENDENCE: cross_provider
ISSUES_FOUND: <number>
CRITICAL_COUNT: <number>
HIGH_COUNT: <number>
MEDIUM_COUNT: <number>
LOW_COUNT: <number>
CONFIDENCE: HIGH | MEDIUM | LOW
BLOCKING: true | false
SUMMARY: <one line>
---END_ADVERSARIAL_REVIEW_STATUS---
```
