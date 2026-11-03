use std::{
    convert::Infallible,
    future::{Ready, ready},
    net::TcpListener,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    task::{Context, Poll},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use celerity_proto::celerity::v1::ReconcileRequest;
use http_body::{Body, Frame};
use sync::{GrpcHomeApi, HomeApi};
use tokio::sync::oneshot;
use tokio_stream::wrappers::TcpListenerStream;
use tonic::{
    body::Body as GrpcBody,
    codegen::{Bytes, Service, http},
    server::NamedService,
    transport::Server,
};

type FixtureResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

struct StalledBody(Arc<AtomicBool>);

impl Body for StalledBody {
    type Data = Bytes;
    type Error = Infallible;

    fn poll_frame(
        self: Pin<&mut Self>,
        _context: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        self.0.store(true, Ordering::SeqCst);
        Poll::Pending
    }
}

#[derive(Clone)]
struct StalledHome(Arc<AtomicBool>);

impl NamedService for StalledHome {
    const NAME: &'static str = "celerity.v1.Home";
}

impl Service<http::Request<GrpcBody>> for StalledHome {
    type Response = http::Response<GrpcBody>;
    type Error = Infallible;
    type Future = Ready<Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, _context: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, request: http::Request<GrpcBody>) -> Self::Future {
        assert_eq!(request.uri().path(), "/celerity.v1.Home/Reconcile");
        assert_eq!(request.headers()["authorization"], "Bearer fixture-token");
        let mut response = http::Response::new(GrpcBody::new(StalledBody(Arc::clone(&self.0))));
        response.headers_mut().insert(
            http::header::CONTENT_TYPE,
            http::HeaderValue::from_static("application/grpc"),
        );
        ready(Ok(response))
    }
}

struct FixtureServer {
    shutdown: Option<oneshot::Sender<()>>,
    worker: Option<JoinHandle<FixtureResult>>,
}

impl Drop for FixtureServer {
    fn drop(&mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _shutdown_result = shutdown.send(());
        }

        if let Some(worker) = self.worker.take() {
            match worker.join() {
                Ok(Ok(())) => {}
                Ok(Err(error)) => eprintln!("deadline fixture stopped: {error}"),
                Err(_) => eprintln!("deadline fixture panicked"),
            }
        }
    }
}

#[test]
fn full_rpc_deadline_covers_headers_then_stalled_body() -> Result<(), Box<dyn std::error::Error>> {
    let temporary = tempfile::tempdir()?;
    let token = temporary.path().join("token");
    std::fs::write(&token, "fixture-token")?;
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let address = listener.local_addr()?;
    listener.set_nonblocking(true)?;
    let body_polled = Arc::new(AtomicBool::new(false));
    let home = StalledHome(Arc::clone(&body_polled));
    let (shutdown, stopping) = oneshot::channel();
    let worker = thread::spawn(move || -> FixtureResult {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        runtime.block_on(async {
            let listener = tokio::net::TcpListener::from_std(listener)?;
            // A broken client still returns when this safety bound closes the socket.
            tokio::time::timeout(
                Duration::from_secs(45),
                Server::builder()
                    .add_service(home)
                    .serve_with_incoming_shutdown(TcpListenerStream::new(listener), async {
                        let _shutdown_result = stopping.await;
                    }),
            )
            .await??;
            Ok(())
        })
    });
    let _server = FixtureServer {
        shutdown: Some(shutdown),
        worker: Some(worker),
    };
    let client = GrpcHomeApi::new(&format!("http://{address}"), &token)?;
    let started = Instant::now();
    let error = client
        .reconcile(&ReconcileRequest::default())
        .err()
        .ok_or("expected full RPC deadline")?;
    let elapsed = started.elapsed();
    assert!(body_polled.load(Ordering::SeqCst));
    assert!(
        error.to_string().contains("full RPC exceeded 30 seconds"),
        "{error}"
    );
    assert!(elapsed >= Duration::from_secs(29), "{elapsed:?}");
    assert!(elapsed < Duration::from_secs(35), "{elapsed:?}");
    Ok(())
}
