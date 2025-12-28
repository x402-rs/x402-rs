pub mod facilitator_client;
pub mod middleware;

pub use middleware::{X402, X402LayerBuilder};

/// **Note**: This module is self-contained and will be unbundled into separate
/// `x402-axum` library and example crates at a later stage.
