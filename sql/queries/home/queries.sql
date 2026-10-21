-- name: Ping :one
SELECT 1 AS ready;

-- name: GetJob :one
SELECT * FROM jobs WHERE id=sqlc.arg(job_id);

-- name: GetJobInputs :one
SELECT input_digests_json FROM jobs WHERE id=sqlc.arg(job_id);

-- name: GetLatestJobInputs :one
SELECT input_digests_json FROM jobs ORDER BY created_at DESC, id DESC LIMIT 1;

-- name: CheckJobOwner :one
SELECT 1 AS owned FROM jobs WHERE id=sqlc.arg(job_id) AND state='running' AND pid=sqlc.arg(pid) AND pid_start_ticks=sqlc.arg(identity);

-- name: HasActiveJob :one
SELECT 1 AS active FROM jobs WHERE state IN ('queued', 'running') LIMIT 1;

-- name: FailOwnedJob :exec
UPDATE jobs SET state='failed', terminal_summary=sqlc.arg(summary) WHERE id=sqlc.arg(job_id) AND state='running' AND pid=sqlc.arg(pid) AND pid_start_ticks=sqlc.arg(identity);

-- name: FailOrphanedJob :exec
UPDATE jobs SET state='failed', terminal_summary=sqlc.arg(summary) WHERE id=sqlc.arg(job_id) AND state='running' AND pid IS sqlc.narg(pid) AND pid_start_ticks IS sqlc.narg(identity);

-- name: EnqueueJob :exec
INSERT INTO jobs(id, state, recipe, input_digests_json, stdout_path, stderr_path) VALUES (sqlc.arg(job_id), 'queued', sqlc.arg(recipe), sqlc.arg(inputs), sqlc.narg(stdout), sqlc.narg(stderr));

-- name: QueuedJobPaths :one
SELECT stdout_path, stderr_path FROM jobs WHERE id=sqlc.arg(job_id) AND state='queued';

-- name: ListRunningJobs :many
SELECT id, pid, pid_start_ticks FROM jobs WHERE state='running';

-- name: ListQueuedJobs :many
SELECT id FROM jobs WHERE state='queued' ORDER BY created_at, id;

-- name: ClaimJob :exec
UPDATE jobs SET state='running', pid=sqlc.narg(pid), pid_start_ticks=sqlc.narg(identity) WHERE id=sqlc.arg(job_id) AND state='queued';

-- name: CompleteOwnedJob :exec
UPDATE jobs SET state=sqlc.arg(state), artifact_digest=sqlc.narg(artifact_digest), terminal_summary=sqlc.narg(summary) WHERE id=sqlc.arg(job_id) AND state='running' AND pid=sqlc.arg(pid) AND pid_start_ticks=sqlc.arg(identity);

-- name: ListRuns :many
SELECT digest, manifest_pb FROM runs;

-- name: ListRunDigests :many
SELECT digest FROM runs;

-- name: GetRun :one
SELECT manifest_pb FROM runs WHERE digest=sqlc.arg(digest);

-- name: InsertRun :exec
INSERT OR IGNORE INTO runs(digest, manifest_pb) VALUES (sqlc.arg(digest), sqlc.arg(manifest));

-- name: InsertChunk :exec
INSERT OR IGNORE INTO chunks(run_digest, digest, path) VALUES (sqlc.arg(run_digest), sqlc.arg(digest), sqlc.arg(path));

-- name: GetStagedModel :one
SELECT path FROM models WHERE digest=sqlc.arg(digest) AND state='staged';

-- name: HasModel :one
SELECT 1 AS found FROM models WHERE digest=sqlc.arg(digest);

-- name: LatestStagedModel :one
SELECT digest FROM models WHERE state='staged' ORDER BY created_at DESC, digest DESC LIMIT 1;

-- name: RejectModel :exec
UPDATE models SET state='rejected' WHERE digest=sqlc.arg(digest) AND state!='rejected';

-- name: InsertStagedModel :exec
INSERT INTO models(digest, path, state) VALUES (sqlc.arg(digest), sqlc.arg(path), 'staged');

-- name: InsertRejectedModel :exec
INSERT OR REPLACE INTO models(digest, path, state) VALUES (sqlc.arg(digest), sqlc.arg(path), 'rejected');

-- name: GetDesiredModel :one
SELECT value FROM settings WHERE key='desired_model_digest';

-- name: UpdateDesiredModel :exec
UPDATE settings SET value=sqlc.arg(digest) WHERE key='desired_model_digest';

-- name: SetDesiredModel :exec
INSERT INTO settings(key, value) VALUES ('desired_model_digest', sqlc.arg(digest)) ON CONFLICT(key) DO UPDATE SET value=excluded.value;

-- name: DeleteDesiredModel :exec
DELETE FROM settings WHERE key='desired_model_digest';

-- name: InsertNotification :exec
INSERT OR IGNORE INTO notifications(event_id, job_id, payload_pb, state) VALUES (sqlc.arg(event_id), sqlc.narg(job_id), sqlc.arg(payload), 'pending');

-- name: ListPendingNotifications :many
SELECT event_id, payload_pb FROM notifications WHERE state IN ('pending', 'failed') AND attempts < 5 ORDER BY event_id LIMIT 64;

-- name: MarkNotificationDelivered :exec
UPDATE notifications SET state='delivered', attempts=attempts+1, last_error=NULL WHERE event_id=sqlc.arg(event_id) AND state!='delivered' AND attempts < 5;

-- name: MarkNotificationFailed :exec
UPDATE notifications SET state='failed', attempts=attempts+1, last_error=sqlc.narg(error) WHERE event_id=sqlc.arg(event_id) AND state!='delivered' AND attempts < 5;
