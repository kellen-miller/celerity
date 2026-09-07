from __future__ import annotations

import sqlite3
from collections.abc import Iterator
from contextlib import contextmanager
from pathlib import Path


def connect(database: Path) -> sqlite3.Connection:
    connection = sqlite3.connect(database, timeout=5)
    connection.row_factory = sqlite3.Row
    connection.execute("PRAGMA foreign_keys=ON")
    connection.execute("PRAGMA busy_timeout=5000")
    return connection


def migrate(database: Path) -> None:
    """Create the explicit schema owned by the single home application."""
    database.parent.mkdir(parents=True, exist_ok=True)
    with connect(database) as connection:
        connection.execute("PRAGMA journal_mode=WAL")
        connection.executescript(
            """
            CREATE TABLE IF NOT EXISTS runs (
              digest TEXT PRIMARY KEY,
              manifest_json BLOB NOT NULL,
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
              payload_json TEXT NOT NULL,
              state TEXT NOT NULL CHECK(state IN ('pending', 'delivered', 'failed')),
              attempts INTEGER NOT NULL DEFAULT 0,
              last_error TEXT,
              FOREIGN KEY(job_id) REFERENCES jobs(id)
            );
            CREATE UNIQUE INDEX IF NOT EXISTS one_active_job
              ON jobs ((1)) WHERE state IN ('queued', 'running');
            """
        )


@contextmanager
def transaction(database: Path) -> Iterator[sqlite3.Connection]:
    connection = connect(database)
    try:
        connection.execute("BEGIN IMMEDIATE")
        yield connection
        connection.commit()
    except BaseException:
        connection.rollback()
        raise
    finally:
        connection.close()
