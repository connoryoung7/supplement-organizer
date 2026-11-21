# syntax=docker/dockerfile:1

# ---- Build stage ----
FROM rust:1-slim-trixie AS builder
WORKDIR /app

# Build dependencies first so they're cached across source-only changes.
COPY Cargo.toml Cargo.lock* ./
RUN mkdir src && echo "fn main() {}" > src/main.rs \
    && cargo build --release \
    && rm -rf src

COPY src ./src
# Touch main.rs so cargo rebuilds the real binary instead of the stub.
RUN touch src/main.rs && cargo build --release

# ---- Runtime stage ----
FROM debian:trixie-slim AS runtime

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 --no-create-home app

COPY --from=builder /app/target/release/supplement-organizer /usr/local/bin/supplement-organizer

USER app
ENV PORT=3000 \
    RUST_LOG=info
EXPOSE 3000

HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
    CMD curl -fsS "http://127.0.0.1:${PORT}/health" || exit 1

CMD ["supplement-organizer"]
