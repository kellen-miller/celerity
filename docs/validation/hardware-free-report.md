# Hardware-free validation report

- Date: 2026-09-01
- Base commit: `04abe23935987546c644d69bd2dd91fdca7e4773`
- Branch: `feat/telemetry-ui`
- Validated implementation commit: `5e2e568643a46139c0b95c592022d5cde75dffe3`
- GitHub Actions: PR #18, run `33565971839`, all three jobs passed

## Support contract

Linux is the sole vehicle runtime/deployment target. macOS portable results are
developer feedback for platform-independent logic; they do not claim macOS CAN,
systemd, firmware, or vehicle support. Local Linux containers below are explicit
software evidence, not CI, installed-system, electrical, display, road, or track
evidence.

## Executed on macOS

The checked-in gates passed:

```sh
./scripts/check-rust --portable
./scripts/check-python
./scripts/check-e2e
./scripts/check-ui
./scripts/check-artifacts
git diff --check
git diff --cached --check
```

The Rust gate passed rustfmt, workspace Clippy with warnings denied, 73 tests,
workspace build, and the STM32G431 release build. One production HTTP test is
intentionally ignored there and passed through `check-e2e`. The host ARM64 GNU
link step explicitly skipped because `aarch64-linux-gnu-gcc` is unavailable.

Python passed 16 tests. End-to-end validation passed one Rust production HTTP/
TCN/ONNX exercise, six model-slot tests, and 12 home application tests. Helm
lint/render/package and deterministic controller/model artifact comparisons
passed.

The UI gate used plain Node 26/npm and passed Prettier, ESLint, `svelte-check`
with zero errors/warnings, three Vitest cases, five `vehicle-ui` Rust tests, and
14 Chromium cases. Rust regenerated and byte-compared all five status goldens;
Vite rebuilt and byte-compared the single embedded HTML file and rejected
trailing blanks in both tracked and freshly generated output. Playwright
launched the production `celerity-ui` binary, rendered that embedded asset,
and observed its inline-only CSP plus `nosniff`. The HTTP smoke test also
returned initial unavailable JSON and 403 for a non-loopback Host.

The final staged-output audit selected exact-pinned Terser 5.51.2 through
Vite's supported minifier setting. A direct evaluation of the emitted Svelte
runtime string proved its cooked eight-character whitespace set unchanged;
only the physical source representation changed to remove trailing blanks.

## Executed in local Linux/ARM64 containers

The following local container evidence passed:

- Before closeout review, Node 26 rebuilt the single-file panel byte-identically
  to two macOS builds: SHA-256
  `c5eb8ecf72659533bfa1352190bc31df077f88feb2ad02d4ac2365dfb51edc07`.
- After the final staged-output correction, Node 26 on local Linux/ARM64
  rebuilt the panel byte-identically to the macOS tracked asset: SHA-256
  `6e4df8e21b49fcf9bf39cbe8852de36f60ac52121da00a072afc75e79e671a76`.
- Rust 1.97.1 compiled Linux tests for `vehicle-runtime`,
  `vehicle-diagnostics`, and `vehicle-ui`.
- A native Linux/ARM64 release build linked `vehicle-runtime`,
  `vehicle-diagnostics`, `vehicle-ui`, and `sync`.
- A privileged Linux/ARM64 container created distinct `vcan-powertrain` and
  `vcan-controller` interfaces. The live composition test passed through real
  kernel sockets, the production model command, full accepted command and
  Runtime Lease acknowledgements, typed diagnostics, producer-age staleness,
  shutdown evidence clearing, and Run sealing.
- Ubuntu 24.04 `systemd-analyze verify` passed all three production units after
  creating the real `celerity-diagnostics` group and `celerity-ui` identity.
- A separate local Linux/ARM64 container booted systemd as PID 1 and passed
  `./scripts/check-linux`. The lifecycle retained the supervisor watchdog test,
  ran the real celerityd socket/Run path over the explicit vCAN evidence mode,
  and proved both service start orders. State remained `0700 root:root`, the
  unit-owned runtime directory remained `0750 root:celerity-diagnostics`, and
  the socket was `0660 root:celerity-diagnostics`.
- The production viewer identity and authorized CLI read the socket; a real
  nonmember received `EACCES`; the viewer identity received permission denial
  reading a root-selected Run. The viewer listened only on 127.0.0.1:8080,
  exposed the expected hardening properties, remained active then became
  unavailable when celerityd stopped, and could stop/fail/restart without
  changing celerityd PID or restart count. Repeated bind failure stayed within
  its start limit. Trap cleanup removed every created identity and test path.
- After implementation adversarial review, the Linux/ARM64 PID1 lifecycle
  passed again and then removed the viewer identity, diagnostics group, and
  tmpfiles fragment. celerityd still became active and served schema 2 with
  `0750 root:root` on its unit-owned runtime directory and `0660 root:root` on
  the socket, then removed the runtime directory on stop.

## Viewer behavior proved

- `celerityd` publishes the typed schema through a bounded read-only Unix
  socket. Silent, malformed, disconnected, and nonresponsive peers cannot
  wedge shutdown or terminate later status requests.
- Producer freshness is monotonic and separate from observer failures. The
  observer retains two misses, removes values on the third, preserves last-read
  age, and recovers on success.
- The viewer dependency closure excludes `control-core`, `control-protocol`,
  `socketcan`, Run machinery, and tract/ONNX. HTTP reads only in-memory state;
  diagnostics I/O remains outside its lock and request path.
- Loopback Host validation, GET-only routes, 403/404/405 behavior, no CORS,
  cache/security headers, and localhost binding are covered by Rust tests.
- Browser fixtures cover current, producer-stale, observer-stale, unavailable,
  unknown, and observer-unreachable transitions. Ages include signal,
  producer, observer, and browser elapsed time; the command is an accepted
  ratio with two decimals, never a requested position, airflow, or percentage.
- Closeout review tightened the browser boundary so current/stale envelopes
  with null last-success age are invalid. Both states have focused coverage;
  the asset regenerated at that review boundary was SHA-256
  `5dd1becb31d4fdefbfbcfc4d1511dbaf1e26051139729c38674db2d89e849281`.

## Executed in GitHub Actions

PR #18 implementation commit `5e2e568643a46139c0b95c592022d5cde75dffe3`
passed GitHub Actions run `33565971839`:

- Linux passed `./scripts/check-linux` in 2m36s.
- macOS 15 portable passed the Rust, Python, end-to-end, UI/browser, and
  artifact gates in 4m15s.
- Ubuntu 24.04 portable passed the same gates, including vCAN-enabled portable
  coverage, in 4m21s.

This is CI evidence for the committed software and generated asset. It is not
physical vehicle, display, electrical, road, or track evidence.

## Browser evidence

Playwright produced wide 1440x900 and narrow 700x900 images for current,
producer-stale, observer-stale, unavailable, unknown, and observer-unreachable
states under `.agent/work/local-status-viewer/evidence/ui/`. All twelve were
manually inspected: text, qualifiers, and state icons are readable; values are
visible; wide/narrow hierarchy is coherent; and no clipping, overlap,
horizontal scroll, or vertical overflow is present. Automated contrast,
stable accessible-name, keyboard skip-link, layout, and axe checks passed. Axe
automation is not screen-reader evidence. The final macOS embedded asset is
SHA-256 `6e4df8e21b49fcf9bf39cbe8852de36f60ac52121da00a072afc75e79e671a76`.

## Explicitly pending or unclaimed

The PID1 lifecycle above is local-container evidence, not a production host or
installed vehicle. No real CAN hardware, electrical fail-silence, target
timing, physical display,
seated-distance sunlight/night legibility, glare, installed-resolution fit,
vehicle-trained model quality, thermal-envelope, road, or track claim is made.
Those remain unchecked in `docs/validation/first-deployment-checklist.md`.
