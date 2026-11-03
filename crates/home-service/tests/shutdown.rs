#![cfg(unix)]

use std::{
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

use home_service::HomeResult;
use tonic::transport::Channel;
use tonic_health::pb::{
    HealthCheckRequest, health_check_response::ServingStatus, health_client::HealthClient,
};

struct HomeProcess {
    child: Child,
}

impl Drop for HomeProcess {
    fn drop(&mut self) {
        let _kill_result = self.child.kill();
        let _wait_result = self.child.wait();
    }
}

#[tokio::test]
async fn sigterm_bounds_shutdown_with_open_public_health_watch() -> HomeResult<()> {
    let directory = tempfile::tempdir()?;
    let token_path = directory.path().join("token");
    std::fs::write(&token_path, "secret")?;
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    let address = listener.local_addr()?;
    drop(listener);
    let mut home = HomeProcess {
        child: Command::new(env!("CARGO_BIN_EXE_celerity-home"))
            .args(["--bind", &address.to_string()])
            .env("CELERITY_HOME_TOKEN_PATH", token_path)
            .env(
                "CELERITY_HOME_STORAGE_ROOT",
                directory.path().join("storage"),
            )
            .env("CELERITY_HOME_WEBHOOK_URL", "https://127.0.0.1:1/webhook")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .spawn()?,
    };
    let endpoint = Channel::from_shared(format!("http://{address}"))?
        .connect_timeout(Duration::from_millis(100))
        .timeout(Duration::from_secs(2));
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut health = loop {
        assert!(
            home.child.try_wait()?.is_none(),
            "Home exited before becoming ready"
        );
        assert!(Instant::now() < deadline, "Home did not become ready");
        if let Ok(channel) = endpoint.connect().await {
            let mut health = HealthClient::new(channel);
            if let Ok(response) = health
                .check(HealthCheckRequest {
                    service: "celerity.v1.Home".into(),
                })
                .await
                && response.into_inner().status == ServingStatus::Serving as i32
            {
                break health;
            }
        }

        tokio::time::sleep(Duration::from_millis(20)).await;
    };
    let mut watch = health
        .watch(HealthCheckRequest {
            service: String::new(),
        })
        .await?
        .into_inner();
    let initial = tokio::time::timeout(Duration::from_secs(2), watch.message())
        .await??
        .ok_or("health Watch ended before shutdown")?;
    assert_eq!(initial.status, ServingStatus::Serving as i32);
    let shutdown_started = Instant::now();
    assert!(
        Command::new("/bin/kill")
            .args(["-TERM", &home.child.id().to_string()])
            .status()?
            .success()
    );
    let stopped = tokio::time::timeout(Duration::from_secs(2), watch.message())
        .await??
        .ok_or("health Watch ended before NotServing")?;
    assert_eq!(stopped.status, ServingStatus::NotServing as i32);
    loop {
        if let Some(status) = home.child.try_wait()? {
            assert!(status.success(), "Home shutdown failed: {status}");
            break;
        }

        assert!(
            shutdown_started.elapsed() < Duration::from_secs(12),
            "open health Watch prevented bounded shutdown"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    // Keep the stream alive until the process exits to exercise forced draining.
    drop(watch);
    Ok(())
}
