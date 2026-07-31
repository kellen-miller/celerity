# Vehicle service installation

Build release binaries for the target, install `celerityd` and `celerity-sync`
under `/usr/local/bin`, the validated bundle under `/etc/celerity/celerity.toml`,
and the exact units from `deploy/systemd` under `/etc/systemd/system`.

```sh
sudo systemctl daemon-reload
sudo systemctl enable --now celerityd.service celerity-sync.service
systemctl status celerityd.service celerity-sync.service
```

`celerityd` uses `Type=notify`, a systemd watchdog, bounded stopping, restart
limits, and local-filesystem ordering. It does not wait for the network. Stopping
or restarting ends lease renewal; startup always begins in fallback and requires
fresh controller reconciliation. `celerity-sync` is independently restartable
and cannot write controller transport or runtime diagnostics.

On the real live netdevice, verify 1 Mbit/s Classical CAN, `LISTEN-ONLY`, and
`restart-ms 0` before starting the runtime. `vcan` cannot establish those
hardware attributes. The future electrical checks remain in the physical
deployment checklist.
