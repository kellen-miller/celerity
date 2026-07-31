# Home deployment

The home side is one FastAPI/Uvicorn process, one SQLite database, and one
filesystem on a PVC. `deploy/kubernetes/home.yaml` deliberately specifies one
replica with `Recreate`, resource bounds, non-root/read-only security, liveness
and readiness probes, a PVC, ConfigMap reference, and Secret reference. It does
not add a queue, object store, PostgreSQL, or model microservice.

Before rendering, replace the image placeholder with an immutable release
digest, create the manually rotatable vehicle token, and configure the single
HTTPS lifecycle webhook:

```sh
kubectl -n celerity create secret generic celerity-home-vehicle-token \
  --from-literal=token='<random token>'
kubectl -n celerity create secret generic celerity-home-webhook \
  --from-literal=url='https://alerts.example.invalid/celerity'
kubectl apply -k deploy/kubernetes
kubectl -n celerity rollout status deployment/celerity-home
```

Vehicle routes require `Authorization: Bearer <token>`. Upload is
content-addressed and idempotent. The server admits only a complete v1 manifest
whose exact bytes match the Run digest and whose referenced chunk exists with
the declared digest. Jobs record immutable input digests, recipe, PID,
stdout/stderr, and terminal result in SQLite. Webhook state is separate: bounded
delivery failure cannot change a completed job or staged model.

The manifests have not been applied to a cluster in the hardware-free report.
