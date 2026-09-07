from __future__ import annotations

from pathlib import Path

import httpx

from celerity_home.database import connect, transaction


def deliver_notifications(database: Path, webhook_url: str, client: httpx.Client) -> int:
    """Attempt pending webhooks without changing the owning job's terminal state."""
    delivered = 0
    with connect(database) as connection:
        notifications = connection.execute(
            "SELECT event_id, payload_json FROM notifications WHERE state IN ('pending', 'failed') "
            "AND attempts < 5 ORDER BY event_id"
        ).fetchall()
    for notification in notifications:
        try:
            response = client.post(webhook_url, content=notification["payload_json"])
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
