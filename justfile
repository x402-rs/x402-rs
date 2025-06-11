build-all:
  cargo build
  cd crates/x402-axum && cargo build
  cd examples/x402-axum-example && cargo build

format-all:
  cargo fmt
  cd crates/x402-axum && cargo fmt
  cd examples/x402-axum-example && cargo fmt
