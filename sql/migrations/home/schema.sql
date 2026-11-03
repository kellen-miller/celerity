CREATE TABLE IF NOT EXISTS runs (
  digest TEXT PRIMARY KEY,
  manifest_pb BLOB NOT NULL,
  completed_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE TABLE IF NOT EXISTS chunks (
  run_digest TEXT NOT NULL,
  digest TEXT NOT NULL,
  path TEXT NOT NULL,
  PRIMARY KEY(run_digest, digest)
);
CREATE TABLE IF NOT EXISTS models (
  digest TEXT PRIMARY KEY,
  path TEXT NOT NULL,
  state TEXT NOT NULL CHECK(state IN ('staged', 'rejected')),
  created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE TABLE IF NOT EXISTS settings (
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS jobs (
  id TEXT PRIMARY KEY,
  state TEXT NOT NULL CHECK(state IN (
    'queued', 'running', 'completed', 'no_change', 'rejected', 'failed'
  )),
  recipe TEXT NOT NULL,
  input_digests_json TEXT NOT NULL,
  pid INTEGER,
  pid_start_ticks TEXT,
  stdout_path TEXT,
  stderr_path TEXT,
  artifact_digest TEXT,
  terminal_summary TEXT,
  created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE TABLE IF NOT EXISTS notifications (
  event_id TEXT PRIMARY KEY,
  job_id TEXT,
  payload_pb BLOB NOT NULL,
  state TEXT NOT NULL CHECK(state IN ('pending', 'delivered', 'failed')),
  attempts INTEGER NOT NULL DEFAULT 0,
  last_error TEXT,
  FOREIGN KEY(job_id) REFERENCES jobs(id)
);
CREATE UNIQUE INDEX IF NOT EXISTS one_active_job
  ON jobs ((1)) WHERE state IN ('queued', 'running');
