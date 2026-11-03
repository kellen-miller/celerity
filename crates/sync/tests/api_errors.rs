use std::{net::TcpListener, thread};

use celerity_proto::{
    HOME_MAX_MESSAGE_BYTES,
    celerity::v1::{
        CompleteRunRequest, CompletedRun, DownloadModelRequest, DownloadModelResponse,
        GetJobRequest, JobRequest, JobStarted, JobStatus, ReconcileRequest, ReconcileResponse,
        UploadChunkRequest, UploadChunkResponse,
        home_server::{Home, HomeServer},
    },
};
use sync::{GrpcHomeApi, HomeApi};
use tokio::sync::oneshot;
use tokio_stream::wrappers::TcpListenerStream;
use tonic::{Request, Response, Status, transport::Server};

const LARGE_ARTIFACT_BYTES: usize = 65 * 1024 * 1024;

struct FixtureHome;

fn require_token<T>(request: &Request<T>) -> Result<(), Status> {
    if request
        .metadata()
        .get("authorization")
        .is_some_and(|value| value == "Bearer fixture-token")
    {
        return Ok(());
    }

    Err(Status::unauthenticated("invalid vehicle token"))
}

#[tonic::async_trait]
impl Home for FixtureHome {
    async fn reconcile(
        &self,
        request: Request<ReconcileRequest>,
    ) -> Result<Response<ReconcileResponse>, Status> {
        require_token(&request)?;
        Err(Status::permission_denied("vehicle is suspended"))
    }

    async fn upload_chunk(
        &self,
        request: Request<UploadChunkRequest>,
    ) -> Result<Response<UploadChunkResponse>, Status> {
        require_token(&request)?;
        let request = request.into_inner();
        assert_eq!(request.run_digest, "run");
        assert_eq!(request.chunk_digest, "chunk");
        assert_eq!(request.data, vec![0xa5; LARGE_ARTIFACT_BYTES]);
        Ok(Response::new(UploadChunkResponse {}))
    }

    async fn complete_run(
        &self,
        request: Request<CompleteRunRequest>,
    ) -> Result<Response<CompletedRun>, Status> {
        require_token(&request)?;
        let request = request.into_inner();
        assert_eq!(request.run_digest, "run");
        assert_eq!(request.manifest_pb, [0, 0xff, 8, 1]);
        Ok(Response::new(CompletedRun {
            run_digest: request.run_digest,
        }))
    }

    async fn download_model(
        &self,
        request: Request<DownloadModelRequest>,
    ) -> Result<Response<DownloadModelResponse>, Status> {
        require_token(&request)?;
        if request.into_inner().digest != "model" {
            return Err(Status::not_found("model bundle is missing"));
        }

        Ok(Response::new(DownloadModelResponse {
            bundle_zip: vec![0x5a; LARGE_ARTIFACT_BYTES],
        }))
    }

    async fn start_job(
        &self,
        _request: Request<JobRequest>,
    ) -> Result<Response<JobStarted>, Status> {
        Err(Status::unimplemented("unused fixture method"))
    }

    async fn get_job(
        &self,
        _request: Request<GetJobRequest>,
    ) -> Result<Response<JobStatus>, Status> {
        Err(Status::unimplemented("unused fixture method"))
    }
}

#[test]
fn home_rpc_preserves_status_authentication_and_large_raw_artifacts()
-> Result<(), Box<dyn std::error::Error>> {
    let temporary = tempfile::tempdir()?;
    let token = temporary.path().join("token");
    std::fs::write(&token, "fixture-token\n")?;
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let address = listener.local_addr()?;
    listener.set_nonblocking(true)?;
    let (shutdown, stopping) = oneshot::channel();
    let server = thread::spawn(
        move || -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?;
            runtime.block_on(async {
                let listener = tokio::net::TcpListener::from_std(listener)?;
                Server::builder()
                    .add_service(
                        HomeServer::new(FixtureHome)
                            .max_encoding_message_size(HOME_MAX_MESSAGE_BYTES)
                            .max_decoding_message_size(HOME_MAX_MESSAGE_BYTES),
                    )
                    .serve_with_incoming_shutdown(TcpListenerStream::new(listener), async {
                        let _shutdown_result = stopping.await;
                    })
                    .await?;
                Ok(())
            })
        },
    );
    let client = GrpcHomeApi::new(&format!("http://{address}"), &token)?;
    let error = client
        .reconcile(&ReconcileRequest::default())
        .err()
        .ok_or("expected permission denial")?;
    assert!(error.to_string().contains("PermissionDenied"), "{error}");
    assert!(
        error.to_string().contains("vehicle is suspended"),
        "{error}"
    );

    let chunk = temporary.path().join("chunk");
    std::fs::write(&chunk, vec![0xa5; LARGE_ARTIFACT_BYTES])?;
    client.put_chunk("run", "chunk", &chunk)?;
    assert_eq!(client.complete("run", &[0, 0xff, 8, 1])?.run_digest, "run");
    assert_eq!(client.get_model("model")?, vec![0x5a; LARGE_ARTIFACT_BYTES]);
    let error = client
        .get_model("absent")
        .err()
        .ok_or("expected missing model")?;
    assert!(error.to_string().contains("NotFound"), "{error}");
    assert!(
        error.to_string().contains("model bundle is missing"),
        "{error}"
    );

    std::fs::write(&token, "wrong-token")?;
    let unauthorized = GrpcHomeApi::new(&format!("http://{address}"), &token)?;
    let error = unauthorized
        .reconcile(&ReconcileRequest::default())
        .err()
        .ok_or("expected unauthenticated status")?;
    assert!(error.to_string().contains("Unauthenticated"), "{error}");
    assert!(
        error.to_string().contains("invalid vehicle token"),
        "{error}"
    );
    drop(unauthorized);
    drop(client);
    shutdown.send(()).map_err(|()| "server stopped early")?;
    server
        .join()
        .map_err(|_| "RPC fixture panicked")?
        .map_err(|error| error.to_string())?;
    Ok(())
}

#[test]
fn synchronous_client_rejects_nested_runtime() -> Result<(), Box<dyn std::error::Error>> {
    let temporary = tempfile::tempdir()?;
    let token = temporary.path().join("token");
    std::fs::write(&token, "fixture-token")?;
    let client = GrpcHomeApi::new("http://127.0.0.1:9", &token)?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async {
        let error = client
            .reconcile(&ReconcileRequest::default())
            .err()
            .ok_or("expected nested runtime rejection")?;
        assert!(error.to_string().contains("outside a Tokio runtime"));
        assert!(GrpcHomeApi::new("http://127.0.0.1:9", &token).is_err());
        Ok::<(), Box<dyn std::error::Error>>(())
    })?;
    Ok(())
}
