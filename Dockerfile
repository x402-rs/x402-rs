FROM --platform=$BUILDPLATFORM rust:trixie AS builder

ENV PORT=8080

WORKDIR /app

RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    libssl-dev \
 && rm -rf /var/lib/apt/lists/*

COPY . ./
RUN cargo build --package x402-facilitator --features full --release --locked

# --- Stage 2 ---
FROM --platform=$BUILDPLATFORM debian:trixie-slim

ENV PORT=8080

RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && \
    rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=builder /app/target/release/x402-facilitator /usr/local/bin/x402-facilitator

EXPOSE $PORT
ENV RUST_LOG=info

ENTRYPOINT ["x402-facilitator"]
