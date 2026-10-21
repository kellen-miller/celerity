-- name: ListLegacyRuns :many
SELECT digest, manifest_json, completed_at FROM runs_json_legacy;

-- name: ListLegacyNotifications :many
SELECT event_id, job_id, payload_json, state, attempts, last_error FROM notifications_json_legacy;
