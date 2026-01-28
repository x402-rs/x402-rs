build-all:
  cd facilitator/ && cargo build
  cd crates/chains/x402-chain-eip155 && cargo build
  cd crates/chains/x402-chain-solana && cargo build
  cd crates/x402-types && cargo build
  cd crates/x402-axum && cargo build
  cd crates/x402-reqwest && cargo build
  cd examples/x402-axum-example && cargo build
  cd examples/x402-reqwest-example && cargo build

format-all:
  cd facilitator/ && cargo fmt
  cd crates/chains/x402-chain-eip155 && cargo fmt
  cd crates/chains/x402-chain-solana && cargo fmt
  cd crates/x402-types && cargo fmt
  cd crates/x402-axum && cargo fmt
  cd crates/x402-reqwest && cargo fmt
  cd examples/x402-axum-example && cargo fmt
  cd examples/x402-reqwest-example && cargo fmt

fmt-all: format-all

clippy-all:
  cd facilitator/ && cargo clippy
  cd crates/chains/x402-chain-eip155 && cargo clippy
  cd crates/chains/x402-chain-solana && cargo clippy
  cd crates/x402-types && cargo clippy
  cd crates/x402-axum && cargo clippy
  cd crates/x402-reqwest && cargo clippy
  cd examples/x402-axum-example && cargo clippy
  cd examples/x402-reqwest-example && cargo clippy

check-all:
  cd facilitator/ && cargo check
  cd crates/chains/x402-chain-eip155 && cargo check
  cd crates/chains/x402-chain-solana && cargo check
  cd crates/x402-types && cargo check
  cd crates/x402-axum && cargo check
  cd crates/x402-reqwest && cargo check
  cd examples/x402-axum-example && cargo check
  cd examples/x402-reqwest-example && cargo check

test-all:
  cd facilitator/ && cargo test
  cd crates/chains/x402-chain-eip155 && cargo test
  cd crates/chains/x402-chain-solana && cargo test
  cd crates/x402-axum && cargo test
  cd crates/x402-reqwest && cargo test
  cd examples/x402-axum-example && cargo test
  cd examples/x402-reqwest-example && cargo test
