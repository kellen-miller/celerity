//! Vehicle-local, read-only status presentation.

mod http;
mod status;

pub use http::run_http;
pub use status::{PresentationState, StatusEnvelope, StatusObserver};
