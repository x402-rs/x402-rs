build-all:
  cargo build --workspace

format-all:
  cargo fmt

fmt-all: format-all

clippy-all:
  cargo clippy --workspace