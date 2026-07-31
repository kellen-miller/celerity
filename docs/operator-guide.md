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
