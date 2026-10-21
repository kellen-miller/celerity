CREATE TABLE run_transfer (
    run_digest TEXT PRIMARY KEY,
    run_path TEXT NOT NULL,
    acknowledged INTEGER NOT NULL DEFAULT 0,
    attempts INTEGER NOT NULL DEFAULT 0,
    last_error TEXT
);
