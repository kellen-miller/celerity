from __future__ import annotations

from datetime import UTC, datetime
from pathlib import Path

import httpx

from celerity.v1.celerity_pb2 import WebhookEvent, WebhookEventType
from celerity_home.database import connect, transaction

PROTOBUF_MEDIA_TYPE = "application/x-protobuf"


def webhook_event(
    event_id: str,
    event_type: WebhookEventType,
    summary: str,
    job_id: str | None = None,
    artifact_digest: str | None = None,
) -> bytes:
    event = WebhookEvent(
        schema_version=1,
        event_id=event_id,
        event_type=event_type,
        occurred_at=datetime.now(UTC).isoformat(),
        summary=summary,
    )
    if job_id is not None:
        event.job_id = job_id
    if artifact_digest is not None:
        event.artifact_digest = artifact_digest
    return bytes(event.SerializeToString())


def deliver_notifications(database: Path, webhook_url: str, client: httpx.Client) -> int:
    """Attempt pending webhooks without changing the owning job's terminal state."""
    delivered = 0
    with connect(database) as connection:
        notifications = connection.execute(
            "SELECT event_id, payload_pb FROM notifications WHERE state IN ('pending', 'failed') "
            "AND attempts < 5 ORDER BY event_id"
        ).fetchall()
    for notification in notifications:
        try:
            response = client.post(
                webhook_url,
                content=notification["payload_pb"],
                headers={"Content-Type": PROTOBUF_MEDIA_TYPE},
            )
            response.raise_for_status()
        except httpx.HTTPError as error:
            with transaction(database) as connection:
                connection.execute(
                    "UPDATE notifications SET state='failed', attempts=attempts+1, last_error=? "
                    "WHERE event_id=?",
                    (str(error), notification["event_id"]),
                )
        else:
            with transaction(database) as connection:
                connection.execute(
                    "UPDATE notifications SET state='delivered', attempts=attempts+1, "
                    "last_error=NULL WHERE event_id=?",
                    (notification["event_id"],),
                )
            delivered += 1
    return delivered
