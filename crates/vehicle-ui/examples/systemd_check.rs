use std::{env, error::Error, fs};

use celerity_proto::celerity::v1::{PresentationState, StatusEnvelope};
use prost::Message;
use vehicle_diagnostics::DiagnosticsSnapshot;

fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    let mut arguments = env::args().skip(1);
    let command = arguments.next().ok_or("missing systemd check command")?;
    match command.as_str() {
        "protobuf" => {
            let path = arguments.next().ok_or("missing response path")?;
            let expected = match arguments.next().as_deref() {
                Some("current") => PresentationState::Current,
                Some("stale") => PresentationState::Stale,
                Some("unavailable") => PresentationState::Unavailable,
                _ => return Err("expected current, stale or unavailable state".into()),
            };
            let snapshot = match arguments.next().as_deref() {
                Some("present") => true,
                Some("absent") => false,
                _ => return Err("expected present or absent snapshot".into()),
            };
            if arguments.next().is_some() {
                return Err("unexpected protobuf check argument".into());
            }

            let status = StatusEnvelope::decode(fs::read(path)?.as_slice())?;
            if status.schema_version != 1
                || status.state != expected as i32
                || status.snapshot.is_some() != snapshot
            {
                return Err(format!("unexpected status response: {status:?}").into());
            }
        }

        "diagnostics" => {
            let path = arguments.next().ok_or("missing diagnostics path")?;
            if arguments.next().is_some() {
                return Err("unexpected diagnostics check argument".into());
            }

            let status: DiagnosticsSnapshot = serde_json::from_slice(&fs::read(path)?)?;
            if status.schema_version != 2 {
                return Err("unexpected diagnostics schema version".into());
            }
        }

        "hold-port" => {
            let address = arguments.next().ok_or("missing listener address")?;
            if arguments.next().is_some() {
                return Err("unexpected listener argument".into());
            }

            let server = tiny_http::Server::http(address)?;
            for request in server.incoming_requests() {
                request.respond(tiny_http::Response::empty(200))?;
            }
        }

        _ => return Err("expected protobuf, diagnostics or hold-port command".into()),
    }

    Ok(())
}
