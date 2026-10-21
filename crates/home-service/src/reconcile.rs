use std::collections::{BTreeMap, BTreeSet};

use celerity_proto::celerity::v1::{
    ReconcileRequest, ReconcileResponse, RunManifest, WebhookEventType,
};
use prost::Message;
use rusqlite::TransactionBehavior;

use celerity_sql::home as sql;

use crate::{
    HomeService, jobs, notifications,
    rpc::{ApiError, ApiResult},
    storage,
    training::ModelContract,
};

pub(crate) fn reconcile(
    home: &HomeService,
    request: ReconcileRequest,
) -> ApiResult<ReconcileResponse> {
    if request.schema_version != 1 {
        return Err(ApiError::invalid("unsupported schema_version"));
    }

    home.recover_jobs().map_err(ApiError::internal)?;
    let mut connection = storage::connect(&home.database()).map_err(ApiError::internal)?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let manifests: BTreeMap<String, Vec<u8>> = sql::ListRuns
        .query_many(&transaction)?
        .into_iter()
        .map(|row| (row.digest, row.manifest_pb))
        .collect();
    let rejected: BTreeSet<_> = request.rejected_model_digests.into_iter().collect();
    for digest in &rejected {
        let changed = sql::RejectModel::builder()
            .digest(digest)
            .build()
            .execute(&transaction)?;
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
        sql::InsertNotification::builder()
            .event_id(&event_id)
            .job_id(None)
            .payload(&payload)
            .build()
            .execute(&transaction)?;
    }

    let mut desired = sql::GetDesiredModel
        .query_opt(&transaction)?
        .map(|row| row.value);
    if let Some(demoted) = desired.as_ref().filter(|digest| rejected.contains(*digest)) {
        let previous = demoted.clone();
        desired = sql::LatestStagedModel
            .query_opt(&transaction)?
            .map(|row| row.digest);
        if let Some(digest) = &desired {
            sql::UpdateDesiredModel::builder()
                .digest(digest)
                .build()
                .execute(&transaction)?;
            let event_id = format!("rollback:{previous}:{digest}");
            let payload = notifications::webhook_event(
                &event_id,
                WebhookEventType::Rollback,
                "known-good model selected after vehicle rejection",
                None,
                Some(digest),
            );
            sql::InsertNotification::builder()
                .event_id(&event_id)
                .job_id(None)
                .payload(&payload)
                .build()
                .execute(&transaction)?;
        } else {
            sql::DeleteDesiredModel.execute(&transaction)?;
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
            let active = sql::HasActiveJob.query_opt(&transaction)?.is_some();
            let latest = sql::GetLatestJobInputs
                .query_opt(&transaction)?
                .map(|row| row.input_digests_json);
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
    Ok(response)
}
