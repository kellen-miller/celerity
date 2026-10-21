#![cfg(unix)]

use std::time::Duration;

use home_service::{HomeConfig, HomeResult, HomeService, notifications, storage};

#[test]
fn notification_failures_are_bounded_and_do_not_change_terminal_jobs() -> HomeResult<()> {
    let directory = tempfile::tempdir()?;
    let home = HomeService::open(HomeConfig {
        storage_root: directory.path().to_path_buf(),
        vehicle_token: "secret".into(),
        webhook_url: "https://127.0.0.1:1/webhook".into(),
        python: "unused".into(),
        worker_binary: env!("CARGO_BIN_EXE_celerity-home").into(),
    })?;
    let connection = storage::connect(&home.database())?;
    connection.execute("INSERT INTO jobs(id, state, recipe, input_digests_json) VALUES ('completed', 'completed', 'causal-tcn-v1', '[]')", [])?;
    connection.execute("INSERT INTO notifications(event_id, job_id, payload_pb, state) VALUES ('event', 'completed', X'00', 'pending')", [])?;
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_millis(100))
        .build()?;
    for _ in 0..7 {
        assert_eq!(
            notifications::deliver_notifications(
                &home.database(),
                &home.config.webhook_url,
                &client
            )?,
            0
        );
    }

    let state: String =
        connection.query_row("SELECT state FROM jobs WHERE id='completed'", [], |row| {
            row.get(0)
        })?;
    assert_eq!(state, "completed");
    let attempts: u32 =
        connection.query_row("SELECT attempts FROM notifications", [], |row| row.get(0))?;
    assert_eq!(attempts, 5);
    Ok(())
}
