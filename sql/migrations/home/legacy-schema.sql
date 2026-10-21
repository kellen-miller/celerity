-- Analysis only: these tables exist only after renaming a legacy database.
CREATE TABLE runs_json_legacy (digest TEXT PRIMARY KEY, manifest_json TEXT NOT NULL, completed_at TEXT NOT NULL);
CREATE TABLE notifications_json_legacy (event_id TEXT PRIMARY KEY, job_id TEXT, payload_json TEXT NOT NULL, state TEXT NOT NULL, attempts INTEGER NOT NULL, last_error TEXT);
