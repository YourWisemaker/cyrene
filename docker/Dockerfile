# Multi-stage Dockerfile for Cyrene (R33).
#
# Build stage: compiles the workspace with the Rust toolchain.
# Runtime stage: slim image with only the binary and runtime dependencies.

# ---------- Build stage ----------
FROM rust:1.82-bookworm AS builder

WORKDIR /build

COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates/ crates/

RUN cargo build --release --bin cyrene && \
    strip target/release/cyrene

# ---------- Runtime stage ----------
FROM debian:bookworm-slim AS runtime

RUN apt-get update && \
    apt-get install -y --no-install-recommends \
        ca-certificates \
        sqlite3 \
        curl \
    && rm -rf /var/lib/apt/lists/*

RUN useradd --create-home --shell /bin/bash cyrene

COPY --from=builder /build/target/release/cyrene /usr/local/bin/cyrene

COPY config.example.toml /home/cyrene/.cyrene/config.toml.example
COPY .env.example /home/cyrene/.cyrene/.env.example
COPY skills/ /home/cyrene/.cyrene/skills/
COPY optional-skills/ /home/cyrene/.cyrene/optional-skills/
COPY extensions/ /home/cyrene/.cyrene/extensions/

RUN mkdir -p /home/cyrene/.cyrene/data && \
    chown -R cyrene:cyrene /home/cyrene/.cyrene

VOLUME ["/home/cyrene/.cyrene/data"]

USER cyrene
WORKDIR /home/cyrene

EXPOSE 8080

ENTRYPOINT ["cyrene"]
CMD ["gateway"]
