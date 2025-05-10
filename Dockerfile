FROM --platform=$BUILDPLATFORM rust:bullseye AS builder

ENV PORT=8080

WORKDIR /app

RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    libssl-dev \
 && rm -rf /var/lib/apt/lists/*

COPY . ./
RUN cargo build --release --locked

# --- Stage 2 ---
FROM --platform=$BUILDPLATFORM debian:bullseye-slim

ENV PORT=8080

# much smaller than full ubuntu (~22MB compressed)

RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && \
    rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=builder /app/target/release/x402-rs /usr/local/bin/x402-rs

EXPOSE $PORT
ENV RUST_LOG=info

ENTRYPOINT ["x402-rs"]