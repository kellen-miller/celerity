//! Bounded protobuf outbox delivery, independent of terminal job state.

use std::{path::Path, time::Duration};

use celerity_proto::celerity::v1::{WebhookEvent, WebhookEventType};
use prost::Message;
use rusqlite::TransactionBehavior;

use celerity_sql::home as sql;

use crate::{HomeResult, storage};

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
    let notifications = sql::ListPendingNotifications.query_many(&connection)?;
    drop(connection);
    let mut delivered = 0;
    for notification in notifications {
        let response = client
            .post(webhook_url)
            .timeout(Duration::from_secs(5))
            .header(reqwest::header::CONTENT_TYPE, "application/x-protobuf")
            .body(notification.payload_pb)
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
                delivered += sql::MarkNotificationDelivered::builder()
                    .event_id(&notification.event_id)
                    .build()
                    .execute(&transaction)?;
            }
            Err(error) => {
                sql::MarkNotificationFailed::builder()
                    .event_id(&notification.event_id)
                    .error(Some(&error))
                    .build()
                    .execute(&transaction)?;
            }
        }

        transaction.commit()?;
    }

    Ok(delivered)
}
