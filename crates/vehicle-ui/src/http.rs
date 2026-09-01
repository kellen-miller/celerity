use std::{
    sync::{
        Arc, RwLock,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use tiny_http::{Header, Method, Response, Server, StatusCode};

use crate::StatusObserver;

const INDEX_HTML: &[u8] = include_bytes!("../assets/index.html");

/// Serves the embedded panel and the single read-only status endpoint.
///
/// # Errors
///
/// Returns an error for listener or response failures.
pub fn run_http(
    server: Server,
    observer: Arc<RwLock<StatusObserver>>,
    stopping: Arc<AtomicBool>,
) -> Result<(), String> {
    while !stopping.load(Ordering::Relaxed) {
        let Some(request) = server
            .recv_timeout(Duration::from_millis(20))
            .map_err(|error| error.to_string())?
        else {
            continue;
        };
        let host_allowed = request
            .headers()
            .iter()
            .filter(|header| header.field.equiv("Host"))
            .map(|header| header.value.as_str())
            .collect::<Vec<_>>();
        if host_allowed.len() != 1 || !valid_loopback_host(host_allowed[0]) {
            request
                .respond(text_response(403, "forbidden\n"))
                .map_err(|error| error.to_string())?;
            continue;
        }
        let known_path = matches!(request.url(), "/" | "/v1/status");
        if !known_path {
            request
                .respond(text_response(404, "not found\n"))
                .map_err(|error| error.to_string())?;
            continue;
        }
        if request.method() != &Method::Get {
            request
                .respond(
                    text_response(405, "method not allowed\n").with_header(header("Allow", "GET")),
                )
                .map_err(|error| error.to_string())?;
            continue;
        }
        if request.url() == "/" {
            request
                .respond(
                    Response::from_data(INDEX_HTML)
                        .with_header(header("Content-Type", "text/html; charset=utf-8"))
                        .with_header(header("X-Content-Type-Options", "nosniff"))
                        .with_header(header(
                            "Content-Security-Policy",
                            "default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; connect-src 'self'; img-src data:",
                        )),
                )
                .map_err(|error| error.to_string())?;
            continue;
        }
        let envelope = observer
            .read()
            .map_err(|error| error.to_string())?
            .envelope(Instant::now());
        let body = serde_json::to_vec(&envelope).map_err(|error| error.to_string())?;
        request
            .respond(
                Response::from_data(body)
                    .with_header(header("Content-Type", "application/json"))
                    .with_header(header("Cache-Control", "no-store"))
                    .with_header(header("X-Content-Type-Options", "nosniff")),
            )
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn valid_loopback_host(value: &str) -> bool {
    let (host, port) = if let Some(remainder) = value.strip_prefix("[::1]") {
        ("[::1]", remainder.strip_prefix(':'))
    } else if let Some((host, port)) = value.split_once(':') {
        (host, Some(port))
    } else {
        (value, None)
    };
    if !matches!(host, "localhost" | "127.0.0.1" | "[::1]") {
        return false;
    }
    match port {
        Some(port) => !port.is_empty() && port.parse::<u16>().is_ok_and(|port| port > 0),
        None => value == host,
    }
}

fn text_response(status: u16, body: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    Response::from_string(body)
        .with_status_code(StatusCode(status))
        .with_header(header("Content-Type", "text/plain; charset=utf-8"))
        .with_header(header("X-Content-Type-Options", "nosniff"))
}

fn header(name: &str, value: &str) -> Header {
    Header::from_bytes(name.as_bytes(), value.as_bytes()).expect("static HTTP header is ASCII")
}
