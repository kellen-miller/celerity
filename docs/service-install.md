# Vehicle service installation

Build release binaries for the target. Create the read-only diagnostics group
and unprivileged viewer identity idempotently:

```sh
getent group celerity-diagnostics >/dev/null || \
  sudo groupadd --system celerity-diagnostics
id celerity-ui >/dev/null 2>&1 || \
  sudo useradd --system --no-create-home --shell /usr/sbin/nologin \
    --gid celerity-diagnostics celerity-ui
```

Install `celerityd`, `celerityctl`, `celerity-sync`, and `celerity-ui` under
`/usr/local/bin`; install the validated bundle at
`/etc/celerity/celerity.toml`; install all three exact units from
`deploy/systemd` under `/etc/systemd/system`. Install
`deploy/tmpfiles.d/celerity.conf` as `/etc/tmpfiles.d/celerity.conf`, then
provision the viewer-group runtime ownership and sole shared state root.
celerityd remains the only systemd `StateDirectory=celerity` owner and its
unit-owned `RuntimeDirectory=celerity` self-provisions on every start:

```sh
sudo systemd-tmpfiles --create /etc/tmpfiles.d/celerity.conf
sudo install -d -m 0700 -o root -g root /var/lib/celerity
```

```sh
sudo systemctl daemon-reload
sudo systemctl enable --now celerityd.service celerity-sync.service
sudo systemctl enable --now celerity-ui.service
systemctl status celerityd.service celerity-sync.service celerity-ui.service
```

`celerityd` uses `Type=notify`, a systemd watchdog, bounded stopping, restart
limits, and local-filesystem ordering. It does not wait for the network. Stopping
or restarting ends lease renewal; startup always begins in fallback and requires
fresh controller reconciliation. Its root primary group keeps the managed state
root `0700 root:root`; best-effort post-bind ownership repair exposes only a
`0660 root:celerity-diagnostics` socket to the provisioned viewer group.
If the viewer group or tmpfiles fragment is absent, celerityd still starts with
`0750 root:root` on its runtime directory and `0660 root:root` on the socket;
the viewer is unavailable, but vehicle authority is unaffected.
`celerity-sync` is independently restartable and cannot write controller
transport or runtime diagnostics.

`celerity-ui` is independently enabled and never ordered as a celerityd
dependency. It reads only `/run/celerity/diagnostics.sock` through membership
in `celerity-diagnostics`, cannot traverse `/var/lib/celerity`, and listens
only on `127.0.0.1:8080`. Open `http://127.0.0.1:8080/` on the vehicle host;
there is no remote bind or proxy. To authorize another local operator identity
for the read-only CLI, add that identity to the diagnostics group, re-login,
then run:

```sh
celerityctl status --socket /run/celerity/diagnostics.sock
```

Group membership grants status-socket access, not Run, CAN, control, or Home
credential access. Restarting or disabling the viewer does not restart or
delay celerityd.

On the real live netdevice, verify 1 Mbit/s Classical CAN, `LISTEN-ONLY`, and
`restart-ms 0` before starting the runtime. `vcan` cannot establish those
hardware attributes. The future electrical checks remain in the physical
deployment checklist.
