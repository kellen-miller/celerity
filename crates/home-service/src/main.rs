use std::{path::PathBuf, time::Duration};

use home_service::{HomeConfig, HomeResult, HomeService, jobs, notifications};

#[derive(Debug)]
struct WorkerArguments {
    storage_root: PathBuf,
    job_id: String,
    python: PathBuf,
}

fn worker_arguments(arguments: &[String]) -> HomeResult<WorkerArguments> {
    let mut storage_root = None;
    let mut job_id = None;
    let mut python = None;
    let mut arguments = arguments.iter();
    while let Some(flag) = arguments.next() {
        let value = arguments.next().ok_or("worker option requires a value")?;
        match flag.as_str() {
            "--storage-root" => storage_root = Some(PathBuf::from(value)),
            "--job-id" => job_id = Some(value.clone()),
            "--python" => python = Some(PathBuf::from(value)),
            _ => return Err(format!("unknown worker option {flag}").into()),
        }
    }

    Ok(WorkerArguments {
        storage_root: storage_root.ok_or("worker requires --storage-root")?,
        job_id: job_id.ok_or("worker requires --job-id")?,
        python: python.ok_or("worker requires --python")?,
    })
}

fn main() -> HomeResult<()> {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    if arguments
        .first()
        .is_some_and(|argument| argument == "worker")
    {
        let arguments = worker_arguments(&arguments[1..])?;
        return jobs::run_worker(
            &arguments.storage_root,
            &arguments.job_id,
            &arguments.python,
            &std::env::current_exe()?,
        );
    }

    if let [command, root, job_id, python, pid, identity] = arguments.as_slice()
        && command == "trainer"
    {
        return jobs::run_trainer(
            std::path::Path::new(root),
            job_id,
            std::path::Path::new(python),
            pid.parse()?,
            identity.clone(),
        );
    }

    serve(&arguments)
}

#[tokio::main]
async fn serve(arguments: &[String]) -> HomeResult<()> {
    let bind = match arguments {
        [] => "0.0.0.0:8080",
        [flag, value] if flag == "--bind" => value,
        _ => return Err("usage: celerity-home [--bind IP:PORT]".into()),
    };
    let token_path = PathBuf::from(std::env::var("CELERITY_HOME_TOKEN_PATH")?);
    let home = HomeService::open(HomeConfig {
        storage_root: PathBuf::from(std::env::var("CELERITY_HOME_STORAGE_ROOT")?),
        vehicle_token: std::fs::read_to_string(token_path)?,
        webhook_url: std::env::var("CELERITY_HOME_WEBHOOK_URL")?,
        python: std::env::var_os("CELERITY_HOME_PYTHON")
            .map_or_else(|| PathBuf::from("python3"), PathBuf::from),
        worker_binary: std::env::current_exe()?,
    })?;
    home.recover_jobs()?;

    // Delivery has its own blocking client and bounded passes. Dropping the
    // sender stops delivery; durable training children keep their own ownership.
    let (stop_delivery, stopped) = std::sync::mpsc::channel::<()>();
    let database = home.database();
    let webhook_url = home.config.webhook_url.clone();
    let (delivery_finished, delivery_done) = std::sync::mpsc::channel::<()>();
    let delivery = std::thread::spawn(move || {
        let client = match reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(5))
            .redirect(reqwest::redirect::Policy::none())
            .build()
        {
            Ok(client) => client,
            Err(error) => {
                eprintln!("webhook client initialization failed: {error}");
                if delivery_finished.send(()).is_err() {
                    eprintln!("webhook shutdown receiver closed");
                }

                return;
            }
        };
        loop {
            if let Err(error) =
                notifications::deliver_notifications(&database, &webhook_url, &client)
            {
                eprintln!("webhook delivery deferred: {error}");
            }

            if stopped.recv_timeout(Duration::from_secs(5))
                != Err(std::sync::mpsc::RecvTimeoutError::Timeout)
            {
                break;
            }
        }

        if delivery_finished.send(()).is_err() {
            eprintln!("webhook shutdown receiver closed");
        }
    });
    let listener = tokio::net::TcpListener::bind(bind).await?;
    let (health, health_service) = tonic_health::server::health_reporter();
    health
        .set_service_status("", tonic_health::ServingStatus::Serving)
        .await;
    let home_ready = home.clone();
    let readiness_health = health.clone();
    let readiness = tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(1));
        loop {
            interval.tick().await;
            let status = match home_ready.check_ready() {
                Ok(()) => tonic_health::ServingStatus::Serving,
                Err(error) => {
                    eprintln!("Home readiness check failed: {error}");
                    tonic_health::ServingStatus::NotServing
                }
            };
            readiness_health
                .set_service_status("celerity.v1.Home", status)
                .await;
        }
    });
    let (stop_server, stopped) = tokio::sync::oneshot::channel::<()>();
    let mut server = Box::pin(
        tonic::transport::Server::builder()
            .add_service(health_service)
            .add_service(home.grpc_server())
            .serve_with_incoming_shutdown(
                tokio_stream::wrappers::TcpListenerStream::new(listener),
                async {
                    let _shutdown_result = stopped.await;
                },
            ),
    );
    let result = tokio::select! {
        result = &mut server => Some(result),
        () = shutdown() => None,
    };
    readiness.abort();
    if let Err(error) = readiness.await
        && !error.is_cancelled()
    {
        eprintln!("Home readiness task failed: {error}");
    }

    let result = if let Some(result) = result {
        result
    } else {
        // Join readiness first so it cannot overwrite the shutdown state.
        for service in ["celerity.v1.Home", ""] {
            health
                .set_service_status(service, tonic_health::ServingStatus::NotServing)
                .await;
        }

        drop(stop_server);
        tokio::time::timeout(Duration::from_secs(5), &mut server)
            .await
            .unwrap_or_else(|_| {
                eprintln!("Home gRPC drain timed out; closing active streams");
                Ok(())
            })
    };
    // Health Watch and other long-lived streams cannot postpone delivery cleanup.
    drop(server);
    drop(stop_delivery);
    // A slow endpoint must not hold shutdown through a full delivery batch.
    // Undelivered rows remain retryable on startup; training owns its process.
    if delivery_done.recv_timeout(Duration::from_secs(6)).is_ok() && delivery.join().is_err() {
        eprintln!("webhook delivery thread panicked");
    }

    result?;
    Ok(())
}

async fn shutdown() {
    #[cfg(unix)]
    {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut terminate) => {
                tokio::select! {
                    result = tokio::signal::ctrl_c() => {
                        if let Err(error) = result { eprintln!("shutdown signal failed: {error}"); }
                    }
                    _ = terminate.recv() => {}
                }
            }
            Err(error) => eprintln!("shutdown signal setup failed: {error}"),
        }
    }

    #[cfg(not(unix))]
    if let Err(error) = tokio::signal::ctrl_c().await {
        eprintln!("shutdown signal failed: {error}");
    }
}
