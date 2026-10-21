-- name: HasRunBackoff :one
SELECT count(*) AS matching_columns
FROM pragma_table_info('run_transfer')
WHERE name = 'next_attempt_at';

-- name: ObserveRun :exec
INSERT INTO run_transfer(run_digest, run_path)
VALUES (sqlc.arg(run_digest), sqlc.arg(run_path))
ON CONFLICT(run_digest) DO UPDATE SET run_path = excluded.run_path;

-- name: GetRunAttempt :one
SELECT attempts FROM run_transfer WHERE run_digest = sqlc.arg(run_digest);

-- name: RecordRunAttempt :exec
UPDATE run_transfer
SET attempts = attempts + 1,
    acknowledged = sqlc.arg(acknowledged),
    last_error = sqlc.narg(last_error),
    next_attempt_at = sqlc.arg(next_attempt_at)
WHERE run_digest = sqlc.arg(run_digest);

-- name: GetRunRetry :one
SELECT next_attempt_at FROM run_transfer WHERE run_digest = sqlc.arg(run_digest);

-- name: GetRunAcknowledged :one
SELECT acknowledged FROM run_transfer WHERE run_digest = sqlc.arg(run_digest);
