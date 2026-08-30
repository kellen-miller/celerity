# Home deployment

The home side is one FastAPI/Uvicorn process, one SQLite database, and one
filesystem on a PVC. The `deploy/helm/celerity-home` chart deliberately
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
