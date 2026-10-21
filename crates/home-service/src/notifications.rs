//! Bounded protobuf outbox delivery, independent of terminal job state.

use std::{path::Path, time::Duration};

use celerity_proto::celerity::v1::{WebhookEvent, WebhookEventType};
use prost::Message;
use rusqlite::{TransactionBehavior, params};

use crate::{HomeResult, http::PROTOBUF, storage};

struct Notification {
    event_id: String,
    payload: Vec<u8>,
}

/// Build the persisted wire event shared by jobs and reconciliation.
#[must_use]
pub fn webhook_event(
    event_id: &str,
    event_type: WebhookEventType,
    summary: &str,
    job_id: Option<&str>,
    artifact_digest: Option<&str>,
) -> Vec<u8> {
    WebhookEvent {
        schema_version: 1,
        event_id: event_id.into(),
        event_type: event_type as i32,
        occurred_at: chrono::Utc::now().to_rfc3339(),
        job_id: job_id.map(str::to_owned),
        artifact_digest: artifact_digest.map(str::to_owned),
        summary: summary.into(),
    }
    .encode_to_vec()
}

/// Deliver at most 64 rows per pass, with at most five attempts per event.
///
/// # Errors
/// Returns invalid HTTPS configuration or SQLite errors. HTTP failures are
/// recorded on the outbox row and never affect its owning job.
pub fn deliver_notifications(
    database: &Path,
    webhook_url: &str,
    client: &reqwest::blocking::Client,
) -> HomeResult<usize> {
    if !webhook_url.starts_with("https://") {
        return Err("webhook URL must use HTTPS".into());
    }

    let connection = storage::connect(database)?;
    let notifications = connection.prepare("SELECT event_id, payload_pb FROM notifications WHERE state IN ('pending', 'failed') AND attempts < 5 ORDER BY event_id LIMIT 64")?.query_map([], |row| Ok(Notification { event_id: row.get(0)?, payload: row.get(1)? }))?.collect::<Result<Vec<_>, _>>()?;
    drop(connection);
    let mut delivered = 0;
    for notification in notifications {
        let response = client
            .post(webhook_url)
            .timeout(Duration::from_secs(5))
            .header(reqwest::header::CONTENT_TYPE, PROTOBUF)
            .body(notification.payload)
            .send()
            .map_err(|error| error.to_string())
            .and_then(|response| {
                if response.status().is_success() {
                    Ok(())
                } else {
                    Err(format!("webhook returned HTTP {}", response.status()))
                }
            });
        let mut connection = storage::connect(database)?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        match response {
            Ok(()) => {
                delivered += transaction.execute("UPDATE notifications SET state='delivered', attempts=attempts+1, last_error=NULL WHERE event_id=? AND state!='delivered' AND attempts < 5", [notification.event_id])?;
            }
            Err(error) => {
                transaction.execute("UPDATE notifications SET state='failed', attempts=attempts+1, last_error=? WHERE event_id=? AND state!='delivered' AND attempts < 5", params![error, notification.event_id])?;
            }
        }

        transaction.commit()?;
    }

    Ok(delivered)
}
