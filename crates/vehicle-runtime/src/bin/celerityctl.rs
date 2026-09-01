use std::{env, io, path::Path};

#[cfg(unix)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let arguments = env::args().skip(1).collect::<Vec<_>>();
    let [command, flag, socket] = arguments.as_slice() else {
        return Err(io::Error::other("usage: celerityctl status --socket <path>").into());
    };
    if command != "status" || flag != "--socket" {
        return Err(io::Error::other("usage: celerityctl status --socket <path>").into());
    }
    let snapshot =
        vehicle_diagnostics::read_diagnostics(Path::new(socket)).map_err(io::Error::other)?;
    println!("{}", serde_json::to_string_pretty(&snapshot)?);
    Ok(())
}

#[cfg(not(unix))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    Err(io::Error::other("celerityctl requires Unix sockets").into())
}
