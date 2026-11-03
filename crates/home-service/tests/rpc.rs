use std::path::PathBuf;

use celerity_proto::{
    HOME_MAX_MESSAGE_BYTES,
    celerity::v1::{
        CompleteRunRequest, Completion, ControlDecision, DownloadModelRequest, GetJobRequest,
        HomeApiError, JobRequest, ReconcileRequest, RunEvent, RunManifest, UploadChunkRequest,
        WebhookEvent, WebhookEventType, home_client::HomeClient, run_event,
    },
};
use home_service::{HomeConfig, HomeResult, HomeService, storage};
use prost::Message;
use rusqlite::params;
use sha2::{Digest, Sha256};
use tonic::{Code, Request, transport::Channel};

struct HomeFixture {
    directory: tempfile::TempDir,
    home: HomeService,
    channel: Channel,
    server: tokio::task::JoinHandle<Result<(), tonic::transport::Error>>,
}

impl HomeFixture {
    async fn new() -> HomeResult<Self> {
        let directory = tempfile::tempdir()?;
        let home = HomeService::open(HomeConfig {
            storage_root: directory.path().to_path_buf(),
            vehicle_token: "secret".into(),
            webhook_url: "https://webhook.invalid/celerity".into(),
            python: PathBuf::from("python3"),
            worker_binary: PathBuf::from(env!("CARGO_BIN_EXE_celerity-home")),
        })?;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let (health, health_service) = tonic_health::server::health_reporter();
        health
            .set_service_status("", tonic_health::ServingStatus::Serving)
            .await;
        home.check_ready()?;
        health
            .set_service_status("celerity.v1.Home", tonic_health::ServingStatus::Serving)
            .await;
        let server = tonic::transport::Server::builder()
            .add_service(health_service)
            .add_service(home.grpc_server());
        let server = tokio::spawn(
            server.serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener)),
        );
        let channel = Channel::from_shared(format!("http://{address}"))?
            .connect()
            .await?;
        Ok(Self {
            directory,
            home,
            channel,
            server,
        })
    }

    fn client(&self) -> HomeClient<Channel> {
        HomeClient::new(self.channel.clone())
            .max_decoding_message_size(HOME_MAX_MESSAGE_BYTES)
            .max_encoding_message_size(HOME_MAX_MESSAGE_BYTES)
    }
}

impl Drop for HomeFixture {
    fn drop(&mut self) {
        self.server.abort();
    }
}

fn authenticated<T>(message: T) -> Request<T> {
    let mut request = Request::new(message);
    request.metadata_mut().insert(
        "authorization",
        tonic::metadata::MetadataValue::from_static("Bearer secret"),
    );
    request
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

#[test]
fn reject_empty_tokens_before_creating_storage() -> HomeResult<()> {
    let directory = tempfile::tempdir()?;
    for token in ["", " \n\t"] {
        let storage_root = directory.path().join("unopened");
        let result = HomeService::open(HomeConfig {
            storage_root: storage_root.clone(),
            vehicle_token: token.into(),
            webhook_url: "https://webhook.invalid/celerity".into(),
            python: PathBuf::from("python3"),
            worker_binary: PathBuf::from(env!("CARGO_BIN_EXE_celerity-home")),
        });
        assert!(result.is_err());
        assert!(!storage_root.exists());
    }

    Ok(())
}

#[tokio::test]
async fn health_is_public_and_home_requires_exact_bearer_metadata() -> HomeResult<()> {
    let fixture = HomeFixture::new().await?;
    let mut health = tonic_health::pb::health_client::HealthClient::new(fixture.channel.clone());
    for service in ["", "celerity.v1.Home"] {
        let response = health
            .check(tonic_health::pb::HealthCheckRequest {
                service: service.into(),
            })
            .await?;
        assert_eq!(
            response.into_inner().status,
            tonic_health::pb::health_check_response::ServingStatus::Serving as i32
        );
    }

    for token in [
        None,
        Some("Bearer wrong"),
        Some("bearer secret"),
        Some("secret"),
    ] {
        let mut request = Request::new(ReconcileRequest {
            schema_version: 1,
            ..ReconcileRequest::default()
        });
        if let Some(token) = token {
            request
                .metadata_mut()
                .insert("authorization", token.parse()?);
        }

        let error = fixture
            .client()
            .reconcile(request)
            .await
            .expect_err("authentication must fail");
        assert_eq!(error.code(), Code::Unauthenticated);
        let body = HomeApiError::decode(error.details())?;
        assert_eq!(body.schema_version, 1);
        assert_eq!(body.message, "invalid vehicle token");
    }

    let error = fixture
        .client()
        .reconcile(authenticated(ReconcileRequest::default()))
        .await
        .expect_err("unsupported schema");
    assert_eq!(error.code(), Code::InvalidArgument);
    assert_eq!(HomeApiError::decode(error.details())?.schema_version, 1);
    for result in [
        fixture
            .client()
            .get_job(authenticated(GetJobRequest {
                job_id: "absent".into(),
            }))
            .await
            .map(|_| ()),
        fixture
            .client()
            .download_model(authenticated(DownloadModelRequest {
                digest: "a".repeat(64),
            }))
            .await
            .map(|_| ()),
    ] {
        let error = result.expect_err("missing resource");
        assert_eq!(error.code(), Code::NotFound);
        assert_eq!(HomeApiError::decode(error.details())?.schema_version, 1);
    }

    Ok(())
}

#[tokio::test]
async fn oversized_single_record_upload_is_complete_and_immutable() -> HomeResult<()> {
    let fixture = HomeFixture::new().await?;
    let data = RunEvent {
        schema_major: 1,
        sequence: 1,
        monotonic_ns: 1,
        source: "test".into(),
        payload: Some(run_event::Payload::ControlDecision(ControlDecision {
            encoded: "x".repeat(65 * 1024 * 1024),
        })),
        ..RunEvent::default()
    }
    .encode_length_delimited_to_vec();
    let chunk_digest = digest(&data);
    let manifest = RunManifest {
        schema_version: 1,
        completion: Completion::Complete as i32,
        chunk_sha256: chunk_digest.clone(),
        ..RunManifest::default()
    }
    .encode_to_vec();
    let run_digest = digest(&manifest);
    let request = UploadChunkRequest {
        run_digest: run_digest.clone(),
        chunk_digest: chunk_digest.clone(),
        data,
    };
    for _ in 0..2 {
        fixture
            .client()
            .upload_chunk(authenticated(request.clone()))
            .await?;
    }

    let completed = fixture
        .client()
        .complete_run(authenticated(CompleteRunRequest {
            run_digest: run_digest.clone(),
            manifest_pb: manifest,
        }))
        .await?
        .into_inner();
    assert_eq!(completed.run_digest, run_digest);

    let error = fixture
        .client()
        .upload_chunk(authenticated(UploadChunkRequest {
            data: vec![1],
            ..request.clone()
        }))
        .await
        .expect_err("digest mismatch");
    assert_eq!(error.code(), Code::InvalidArgument);
    let stored = fixture
        .directory
        .path()
        .join("runs")
        .join(&run_digest)
        .join(&chunk_digest);
    std::fs::write(stored, b"corrupt immutable data")?;
    let error = fixture
        .client()
        .upload_chunk(authenticated(request))
        .await
        .expect_err("immutable conflict");
    assert_eq!(error.code(), Code::FailedPrecondition);
    let error = fixture
        .client()
        .upload_chunk(authenticated(UploadChunkRequest {
            run_digest: "invalid".into(),
            chunk_digest: "invalid".into(),
            data: Vec::new(),
        }))
        .await
        .expect_err("invalid digest");
    assert_eq!(error.code(), Code::InvalidArgument);
    Ok(())
}

#[tokio::test]
async fn canonical_completion_is_idempotent_and_rejection_does_not_admit_a_run() -> HomeResult<()> {
    let fixture = HomeFixture::new().await?;
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
        fixture
            .client()
            .upload_chunk(authenticated(UploadChunkRequest {
                run_digest: run_digest.clone(),
                chunk_digest,
                data: chunk,
            }))
            .await?;
        for _ in 0..2 {
            let result = fixture
                .client()
                .complete_run(authenticated(CompleteRunRequest {
                    run_digest: run_digest.clone(),
                    manifest_pb: manifest.clone(),
                }))
                .await;
            if valid {
                assert_eq!(result?.into_inner().run_digest, run_digest);
            } else {
                assert_eq!(
                    result.expect_err("sequence regression").code(),
                    Code::InvalidArgument
                );
            }
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
    let fixture = HomeFixture::new().await?;
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
    };
    for _ in 0..2 {
        let response = fixture
            .client()
            .reconcile(authenticated(request.clone()))
            .await?
            .into_inner();
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
    let error = fixture
        .client()
        .download_model(authenticated(DownloadModelRequest { digest: rejected }))
        .await
        .expect_err("rejected model");
    assert_eq!(error.code(), Code::NotFound);
    Ok(())
}

#[tokio::test]
async fn inconsistent_contracts_rollback_and_missing_job_runs_have_typed_details() -> HomeResult<()>
{
    let fixture = HomeFixture::new().await?;
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
    };
    let error = fixture
        .client()
        .reconcile(authenticated(request))
        .await
        .expect_err("conflicting contracts");
    assert_eq!(error.code(), Code::FailedPrecondition);
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
    };
    let error = fixture
        .client()
        .start_job(authenticated(request))
        .await
        .expect_err("incomplete runs");
    assert_eq!(error.code(), Code::FailedPrecondition);
    let body = HomeApiError::decode(error.details())?;
    assert_eq!(body.schema_version, 1);
    assert_eq!(body.incomplete_run_digests, ["missing"]);
    Ok(())
}
