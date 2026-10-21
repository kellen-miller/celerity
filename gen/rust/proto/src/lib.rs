/// Allow every message representable by the gRPC frame length.
/// Recorder rotation preserves oversized single records without splitting them.
pub const HOME_MAX_MESSAGE_BYTES: usize = u32::MAX as usize;

pub mod celerity {
    pub mod v1 {
        include!("generated/celerity.v1.rs");
    }
}
