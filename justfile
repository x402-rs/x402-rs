build-all:
  cargo build
  cd crates/x402-axum && cargo build
  cd crates/x402-reqwest && cargo build
  cd examples/x402-axum-example && cargo build
  cd examples/x402-reqwest-example && cargo build

format-all:
  cargo fmt
  cd crates/x402-axum && cargo fmt
  cd crates/x402-reqwest && cargo fmt
  cd examples/x402-axum-example && cargo fmt
  cd examples/x402-reqwest-example && cargo fmt

fmt-all: format-all

clippy-all:
  cargo clippy
  cd crates/x402-axum && cargo clippy
  cd crates/x402-reqwest && cargo clippy
  cd examples/x402-axum-example && cargo clippy
  cd examples/x402-reqwest-example && cargo clippy
