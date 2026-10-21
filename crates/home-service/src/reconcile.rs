use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use axum::{body::Bytes, extract::State, http::StatusCode, response::Response};
use celerity_proto::celerity::v1::{
    ReconcileRequest, ReconcileResponse, RunManifest, WebhookEventType,
};
use prost::Message;
use rusqlite::{OptionalExtension, TransactionBehavior, params};

use crate::{
    HomeService,
    http::{ApiError, ApiResult, protobuf},
    jobs, notifications, storage,
    training::ModelContract,
};

pub(crate) async fn reconcile(
    State(home): State<Arc<HomeService>>,
    data: Bytes,
) -> ApiResult<Response> {
    let request = ReconcileRequest::decode(data.as_ref())
        .map_err(|_| ApiError::invalid("invalid reconcile request"))?;
    if request.schema_version != 1 {
        return Err(ApiError::invalid("unsupported schema_version"));
    }

    home.recover_jobs().map_err(ApiError::internal)?;
    let mut connection = storage::connect(&home.database()).map_err(ApiError::internal)?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let manifests: BTreeMap<String, Vec<u8>> = transaction
        .prepare("SELECT digest, manifest_pb FROM runs")?
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<Result<_, _>>()?;
    let rejected: BTreeSet<_> = request.rejected_model_digests.into_iter().collect();
    for digest in &rejected {
        let changed = transaction.execute(
            "UPDATE models SET state='rejected' WHERE digest=? AND state!='rejected'",
            [digest],
        )?;
        if changed == 0 {
            continue;
        }

        let event_id = format!("demotion:{digest}");
        let payload = notifications::webhook_event(
            &event_id,
            WebhookEventType::Demotion,
            "vehicle rejected staged model evidence",
            None,
            Some(digest),
        );
        transaction.execute("INSERT OR IGNORE INTO notifications(event_id, job_id, payload_pb, state) VALUES (?, NULL, ?, 'pending')", params![event_id, payload])?;
    }

    let mut desired: Option<String> = transaction
        .query_row(
            "SELECT value FROM settings WHERE key='desired_model_digest'",
            [],
            |row| row.get(0),
        )
        .optional()?;
    if let Some(demoted) = desired.as_ref().filter(|digest| rejected.contains(*digest)) {
        let previous = demoted.clone();
        desired = transaction.query_row("SELECT digest FROM models WHERE state='staged' ORDER BY created_at DESC, digest DESC LIMIT 1", [], |row| row.get(0)).optional()?;
        if let Some(digest) = &desired {
            transaction.execute(
                "UPDATE settings SET value=? WHERE key='desired_model_digest'",
                [digest],
            )?;
            let event_id = format!("rollback:{previous}:{digest}");
            let payload = notifications::webhook_event(
                &event_id,
                WebhookEventType::Rollback,
                "known-good model selected after vehicle rejection",
                None,
                Some(digest),
            );
            transaction.execute("INSERT OR IGNORE INTO notifications(event_id, job_id, payload_pb, state) VALUES (?, NULL, ?, 'pending')", params![event_id, payload])?;
        } else {
            transaction.execute("DELETE FROM settings WHERE key='desired_model_digest'", [])?;
        }
    }

    let reported: BTreeSet<_> = request.completed_run_digests.into_iter().collect();
    let mut queued = None;
    if !reported.is_empty() && reported.iter().all(|digest| manifests.contains_key(digest)) {
        let mut decoded = BTreeMap::new();
        for (digest, bytes) in &manifests {
            decoded.insert(
                digest,
                RunManifest::decode(bytes.as_slice()).map_err(ApiError::internal)?,
            );
        }

        let first = reported
            .iter()
            .next()
            .and_then(|digest| decoded.get(digest));
        if let Some(first) = first {
            let contract = ModelContract::from(first);
            if reported.iter().any(|digest| {
                decoded
                    .get(digest)
                    .is_none_or(|manifest| ModelContract::from(manifest) != contract)
            }) {
                return Err(ApiError::conflict("reported Run contracts disagree"));
            }

            let corpus: Vec<String> = decoded
                .iter()
                .filter(|(_, manifest)| ModelContract::from(*manifest) == contract)
                .map(|(digest, _)| (*digest).clone())
                .collect();
            let active = transaction
                .query_row(
                    "SELECT 1 FROM jobs WHERE state IN ('queued', 'running') LIMIT 1",
                    [],
                    |_| Ok(()),
                )
                .optional()?
                .is_some();
            let latest: Option<String> = transaction
                .query_row(
                    "SELECT input_digests_json FROM jobs ORDER BY created_at DESC, id DESC LIMIT 1",
                    [],
                    |row| row.get(0),
                )
                .optional()?;
            let latest_inputs: Vec<String> =
                latest.map_or_else(|| Ok(Vec::new()), |json| serde_json::from_str(&json))?;
            if !active && latest_inputs != corpus {
                queued = Some(
                    jobs::enqueue(
                        &transaction,
                        &home.config.storage_root,
                        &corpus,
                        "causal-tcn-v1",
                    )
                    .map_err(ApiError::internal)?,
                );
            }
        }
    }

    transaction.commit()?;
    if let Some(job_id) = queued {
        jobs::launch(&home.config, &job_id).map_err(ApiError::internal)?;
    }

    let response = ReconcileResponse {
        missing_run_digests: reported
            .into_iter()
            .filter(|digest| !manifests.contains_key(digest))
            .collect(),
        desired_model_digest: desired,
    };
    Ok(protobuf(&response, StatusCode::OK))
}
