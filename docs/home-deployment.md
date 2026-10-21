# Home deployment

The home side runs the Rust `celerity-home` service, one SQLite database, and one
filesystem on a PVC. Detached Rust workers supervise the Python ML adapter. Python
receives immutable inputs and returns candidate bundles and metrics; Rust owns
all database writes, model selection and webhook delivery. The `deploy/helm/celerity-home` chart deliberately
specifies one replica with `Recreate`, resource bounds, non-root/read-only
security, liveness and readiness probes, a PVC, ConfigMap reference, and Secret
reference. It does not add a queue, object store, PostgreSQL, or model
microservice.

The chart is also the upstream package for an infrastructure-owned proxy chart.
That proxy declares `celerity-home` as a versioned dependency and places its
environment-specific overrides under the `celerity-home:` values key; Celerity
continues to own the Deployment, Service, PVC, ConfigMap, and ExternalSecret
templates.

The cluster must already provide External Secrets Operator and a 1Password SDK
`ClusterSecretStore`. The selected 1Password item has `vehicle-token` and
`webhook-url` properties by default. The infrastructure proxy supplies its
store name and may override the item and property names.

Before installation, replace the image placeholder with an immutable release
digest and select the existing `ClusterSecretStore`:

```sh
helm upgrade --install celerity-home deploy/helm/celerity-home \
  --namespace celerity \
  --create-namespace \
  --set-string image.digest='sha256:<release digest>' \
  --set-string externalSecrets.secretStoreName='<onepassword store>'
kubectl -n celerity rollout status deployment/celerity-home
```

Vehicle routes require `Authorization: Bearer <token>`. Upload is
content-addressed and idempotent. The server admits only a complete v1 manifest
whose exact bytes match the Run digest and whose referenced chunk exists with
the declared digest. Jobs record immutable input digests, recipe, PID,
stdout/stderr, and terminal result in SQLite. Webhook state is separate: bounded
delivery failure cannot change a completed job or staged model.

The rendered chart has not been installed in a cluster in the hardware-free
report.

Build the image from the repository root so the Rust workspace is available:

```sh
docker build -f services/home/Dockerfile -t celerity-home:local .
```

The image starts the Rust service directly. It retains a Python environment for
training, selected with `CELERITY_HOME_PYTHON`; the default outside the image is
`python3`. Existing storage, token-file and webhook environment variables and
HTTP paths remain the same. For local development after `uv sync --project
services/home --locked`, set `CELERITY_HOME_PYTHON` to the absolute path of
`services/home/.venv/bin/python` and run `cargo run -p home-service --bin celerity-home`.

Existing protobuf databases and legacy JSON databases upgrade in one atomic
transaction. Take the usual PVC backup before replacing an installed service;
this change does not deploy it. The API can restart while a claimed Rust worker
continues. A numerical child inherits an admission lock across exec. If its
supervisor dies, recovery keeps admission occupied until numerical work exits,
then records one failed job. A delayed child checks ownership before training;
numerical children cannot publish model/database changes. Candidates are not promoted when the
desired baseline changed during training.
