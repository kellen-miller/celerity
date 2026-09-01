# Operator guide

Normal startup is fallback, controller discovery and configuration
reconciliation, fresh runtime/command leases, arming dwell, then active
authority. A restored link grants nothing. Controller reboot, stale observation
or ACK, essential queue failure, invalid time, or a shared invariant failure
stops lease renewal and requires reconciliation.

`celerityctl` is read-only. Runs under the configured spool are append-only;
incomplete Runs stay incomplete and are never training inputs. Do not manually
delete Runs before the home service returns the exact digest acknowledgement.

Model bundles are written only to an inactive temporary path, checked by digest,
ABI, and real `tract` load, then published atomically. They are selected only at
a later startup. Corruption selects the known-good slot or deterministic policy,
never a partially staged artifact.

## Local status viewer

On the vehicle host, open `http://127.0.0.1:8080/`. The panel is read-only and
does not grant or summarize control authority beyond the exact fields shown.

- `CONNECTED / RUNTIME UPDATING` means the observer read succeeded and the
  producer update is within its freshness threshold.
- `CONNECTED / RUNTIME UPDATE DELAYED` means the socket read succeeded but the
  producer snapshot is old.
- `DIAGNOSTICS STALE` retains the last values for the first two read failures.
- `UNAVAILABLE` removes values after three failures, or before any successful
  observation. It is not a controller or celerityd supervisor state.
- `UNKNOWN` is field-level truth: evidence was never observed, is not expected
  in fallback, is not renewing, has no outstanding command, or is still being
  awaited. It is not silently promoted to healthy or unhealthy.

Temperature ages are the displayed values' age now, including signal,
producer, observer, and browser elapsed age. Radiator Split Command is the last
controller-accepted ratio, not requested position, airflow, or a percentage.
Stopping the viewer has no effect on celerityd; if celerityd stops, the viewer
remains reachable and transitions to unavailable.
