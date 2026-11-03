PRAGMA journal_mode=WAL;

CREATE TABLE IF NOT EXISTS run_transfer (
    run_digest TEXT PRIMARY KEY,
    run_path TEXT NOT NULL,
    acknowledged INTEGER NOT NULL DEFAULT 0,
    attempts INTEGER NOT NULL DEFAULT 0,
    last_error TEXT,
    next_attempt_at INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS model_transfer (
    digest TEXT PRIMARY KEY,
    state TEXT NOT NULL,
    last_error TEXT
);
