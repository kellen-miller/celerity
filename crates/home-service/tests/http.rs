use std::path::PathBuf;

use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
    response::Response,
};
use celerity_proto::celerity::v1::{
    Completion, ControlDecision, Health, JobRequest, ReconcileRequest, ReconcileResponse, RunEvent,
    RunManifest, WebhookEvent, WebhookEventType, run_event,
};
use home_service::{HomeConfig, HomeResult, HomeService, storage};
use prost::Message;
use rusqlite::params;
use sha2::{Digest, Sha256};
use tower::ServiceExt;

struct HomeFixture {
    directory: tempfile::TempDir,
    home: HomeService,
}

impl HomeFixture {
    fn new() -> HomeResult<Self> {
        let directory = tempfile::tempdir()?;
        let home = HomeService::open(HomeConfig {
            storage_root: directory.path().to_path_buf(),
            vehicle_token: "secret".into(),
            webhook_url: "https://webhook.invalid/celerity".into(),
            python: PathBuf::from("python3"),
            worker_binary: PathBuf::from(env!("CARGO_BIN_EXE_celerity-home")),
        })?;
        Ok(Self { directory, home })
    }

    async fn request(
        &self,
        method: &str,
        path: &str,
        data: Vec<u8>,
        token: Option<&str>,
    ) -> HomeResult<Response> {
        let mut builder = Request::builder().method(method).uri(path);
        if let Some(token) = token {
            builder = builder.header("Authorization", token);
        }

        Ok(self
            .home
            .router()
            .oneshot(builder.body(Body::from(data))?)
            .await?)
    }
}

fn digest(data: &[u8]) -> String {
    hex::encode(Sha256::digest(data))
}

fn control_record(sequence: u64) -> Vec<u8> {
    RunEvent {
        schema_major: 1,
        sequence,
        monotonic_ns: sequence * 1_000_000,
        source: "test".into(),
        payload: Some(run_event::Payload::ControlDecision(ControlDecision {
            encoded: "{\"state\":\"accepted\"}".into(),
        })),
        ..RunEvent::default()
    }
    .encode_length_delimited_to_vec()
}

#[tokio::test]
async fn health_and_authenticated_routes_preserve_wire_contract() -> HomeResult<()> {
    let fixture = HomeFixture::new()?;
    for path in ["/health/live", "/health/ready"] {
        let response = fixture.request("GET", path, Vec::new(), None).await?;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["content-type"], "application/x-protobuf");
        assert_eq!(
            Health::decode(to_bytes(response.into_body(), usize::MAX).await?)?.status,
            path.trim_start_matches("/health/")
        );
    }

    for token in [None, Some("Bearer wrong"), Some("bearer secret")] {
        let response = fixture
            .request("POST", "/v1/reconcile", Vec::new(), token)
            .await?;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let body: serde_json::Value =
            serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await?)?;
        assert_eq!(body["detail"], "invalid vehicle token");
    }

    let response = fixture
        .request("POST", "/v1/reconcile", vec![0xff], Some("Bearer secret"))
        .await?;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    Ok(())
}

#[tokio::test]
async fn uploads_over_two_megabytes_are_digest_checked_and_immutable() -> HomeResult<()> {
    let fixture = HomeFixture::new()?;
    let data = vec![42; 3 * 1024 * 1024];
    let chunk_digest = digest(&data);
    let run_digest = "a".repeat(64);
    let path = format!("/v1/runs/{run_digest}/chunks/{chunk_digest}");
    for _ in 0..2 {
        assert_eq!(
            fixture
                .request("PUT", &path, data.clone(), Some("Bearer secret"))
                .await?
                .status(),
            StatusCode::NO_CONTENT
        );
    }

    assert_eq!(
        fixture
            .request("PUT", &path, vec![1], Some("Bearer secret"))
            .await?
            .status(),
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let stored = fixture
        .directory
        .path()
        .join("runs")
        .join(&run_digest)
        .join(&chunk_digest);
    std::fs::write(stored, b"corrupt immutable data")?;
    assert_eq!(
        fixture
            .request("PUT", &path, data, Some("Bearer secret"))
            .await?
            .status(),
        StatusCode::CONFLICT
    );
    assert_eq!(
        fixture
            .request(
                "PUT",
                "/v1/runs/invalid/chunks/invalid",
                Vec::new(),
                Some("Bearer secret")
            )
            .await?
            .status(),
        StatusCode::UNPROCESSABLE_ENTITY
    );
    Ok(())
}

#[tokio::test]
async fn canonical_completion_is_idempotent_and_rejection_does_not_admit_a_run() -> HomeResult<()> {
    let fixture = HomeFixture::new()?;
    for valid in [true, false] {
        let mut chunk = control_record(2);
        chunk.extend(control_record(if valid { 3 } else { 1 }));
        let chunk_digest = digest(&chunk);
        let manifest = RunManifest {
            schema_version: 1,
            completion: Completion::Complete as i32,
            chunk_sha256: chunk_digest.clone(),
            ..RunManifest::default()
        }
        .encode_to_vec();
        let run_digest = digest(&manifest);
        assert_eq!(
            fixture
                .request(
                    "PUT",
                    &format!("/v1/runs/{run_digest}/chunks/{chunk_digest}"),
                    chunk,
                    Some("Bearer secret")
                )
                .await?
                .status(),
            StatusCode::NO_CONTENT
        );
        for _ in 0..2 {
            let response = fixture
                .request(
                    "POST",
                    &format!("/v1/runs/{run_digest}/complete"),
                    manifest.clone(),
                    Some("Bearer secret"),
                )
                .await?;
            assert_eq!(
                response.status(),
                if valid {
                    StatusCode::OK
                } else {
                    StatusCode::UNPROCESSABLE_ENTITY
                }
            );
        }
    }

    let count: i64 = storage::connect(&fixture.home.database())?.query_row(
        "SELECT count(*) FROM runs",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(count, 1);
    Ok(())
}

#[tokio::test]
async fn reconcile_rolls_back_and_emits_idempotent_protobuf_events() -> HomeResult<()> {
    let fixture = HomeFixture::new()?;
    let rejected = "a".repeat(64);
    let known_good = "b".repeat(64);
    let connection = storage::connect(&fixture.home.database())?;
    for digest in [&rejected, &known_good] {
        connection.execute(
            "INSERT INTO models(digest, path, state) VALUES (?, 'unused.zip', 'staged')",
            [digest],
        )?;
    }

    connection.execute(
        "INSERT INTO settings(key, value) VALUES ('desired_model_digest', ?)",
        [&rejected],
    )?;
    let request = ReconcileRequest {
        schema_version: 1,
        rejected_model_digests: vec![rejected.clone()],
        completed_run_digests: vec!["c".repeat(64)],
        ..ReconcileRequest::default()
    }
    .encode_to_vec();
    for _ in 0..2 {
        let response = fixture
            .request(
                "POST",
                "/v1/reconcile",
                request.clone(),
                Some("Bearer secret"),
            )
            .await?;
        assert_eq!(response.status(), StatusCode::OK);
        let response =
            ReconcileResponse::decode(to_bytes(response.into_body(), usize::MAX).await?)?;
        assert_eq!(
            response.desired_model_digest.as_deref(),
            Some(known_good.as_str())
        );
        assert_eq!(response.missing_run_digests, vec!["c".repeat(64)]);
    }

    let payloads = connection
        .prepare("SELECT payload_pb FROM notifications ORDER BY event_id")?
        .query_map([], |row| row.get::<_, Vec<u8>>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    let events = payloads
        .iter()
        .map(|bytes| WebhookEvent::decode(bytes.as_slice()))
        .collect::<Result<Vec<_>, _>>()?;
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].event_type, WebhookEventType::Demotion as i32);
    assert_eq!(events[1].event_type, WebhookEventType::Rollback as i32);
    assert_eq!(
        fixture
            .request(
                "GET",
                &format!("/v1/models/{rejected}"),
                Vec::new(),
                Some("Bearer secret")
            )
            .await?
            .status(),
        StatusCode::NOT_FOUND
    );
    Ok(())
}

#[tokio::test]
async fn inconsistent_reported_contracts_rollback_demotion_and_job_missing_detail_is_preserved()
-> HomeResult<()> {
    let fixture = HomeFixture::new()?;
    let connection = storage::connect(&fixture.home.database())?;
    for name in ["first", "second"] {
        let manifest = RunManifest {
            model_abi: name.into(),
            ..RunManifest::default()
        }
        .encode_to_vec();
        connection.execute(
            "INSERT INTO runs(digest, manifest_pb) VALUES (?, ?)",
            params![name, manifest],
        )?;
    }

    connection.execute(
        "INSERT INTO models(digest, path, state) VALUES ('candidate', 'unused.zip', 'staged')",
        [],
    )?;
    let request = ReconcileRequest {
        schema_version: 1,
        completed_run_digests: vec!["first".into(), "second".into()],
        rejected_model_digests: vec!["candidate".into()],
        ..ReconcileRequest::default()
    }
    .encode_to_vec();
    assert_eq!(
        fixture
            .request("POST", "/v1/reconcile", request, Some("Bearer secret"))
            .await?
            .status(),
        StatusCode::CONFLICT
    );
    let state: String = connection.query_row(
        "SELECT state FROM models WHERE digest='candidate'",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(state, "staged");
    assert_eq!(
        connection.query_row("SELECT count(*) FROM notifications", [], |row| row
            .get::<_, i64>(0))?,
        0
    );
    let request = JobRequest {
        recipe: "causal-tcn-v1".into(),
        run_digests: vec!["missing".into()],
    }
    .encode_to_vec();
    let response = fixture
        .request("POST", "/v1/jobs", request, Some("Bearer secret"))
        .await?;
    assert_eq!(response.status(), StatusCode::CONFLICT);
    let body: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await?)?;
    assert_eq!(
        body["detail"]["incomplete_runs"],
        serde_json::json!(["missing"])
    );
    Ok(())
}
