# ── Build Stage ────────────────────────────────────────────────────────────
FROM rust:1.75-bookworm AS builder

ARG CRATE

WORKDIR /app

# Copy manifests first for better caching
COPY Cargo.toml Cargo.lock* ./
COPY crates/shared/Cargo.toml crates/shared/Cargo.toml
COPY crates/proxy/Cargo.toml crates/proxy/Cargo.toml
COPY crates/gateway/Cargo.toml crates/gateway/Cargo.toml
COPY crates/auth/Cargo.toml crates/auth/Cargo.toml
COPY crates/control-plane/Cargo.toml crates/control-plane/Cargo.toml
COPY crates/storage-service/Cargo.toml crates/storage-service/Cargo.toml
COPY crates/wal-service/Cargo.toml crates/wal-service/Cargo.toml
COPY crates/branch-service/Cargo.toml crates/branch-service/Cargo.toml

# Create dummy src files for dependency caching
RUN mkdir -p crates/shared/src crates/proxy/src crates/gateway/src crates/auth/src crates/control-plane/src crates/storage-service/src crates/wal-service/src crates/branch-service/src && \
    echo 'pub fn dummy() {}' > crates/shared/src/lib.rs && \
    echo 'fn main() {}' > crates/proxy/src/main.rs && \
    echo 'fn main() {}' > crates/gateway/src/main.rs && \
    echo 'fn main() {}' > crates/auth/src/main.rs && \
    echo 'fn main() {}' > crates/control-plane/src/main.rs && \
    echo 'fn main() {}' > crates/storage-service/src/main.rs && \
    echo 'fn main() {}' > crates/wal-service/src/main.rs && \
    echo 'fn main() {}' > crates/branch-service/src/main.rs

# Build dependencies
RUN cargo build --release --bin ${CRATE} 2>/dev/null || true

# Copy actual source code
COPY crates/ crates/

# Build the actual binary
RUN cargo build --release --bin ${CRATE}

# ── Runtime Stage ──────────────────────────────────────────────────────────
FROM debian:bookworm-slim AS runtime

RUN apt-get update && apt-get install -y \
    curl \
    ca-certificates \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

# Install PostgreSQL client tools (for pg_basebackup, pg_ctl, etc.)
RUN echo "deb http://apt.postgresql.org/pub/repos/apt bookworm-pgdg main" > /etc/apt/sources.list.d/pgdg.list && \
    curl -fsSL https://www.postgresql.org/media/keys/ACCC4CF8.asc | gpg --dearmor -o /etc/apt/trusted.gpg.d/pgdg.gpg && \
    apt-get update && apt-get install -y postgresql-client-16 && \
    rm -rf /var/lib/apt/lists/*

ARG CRATE

WORKDIR /app

COPY --from=builder /app/target/release/${CRATE} /app/freebuff-service

ENV RUST_LOG=info

ENTRYPOINT ["/app/freebuff-service"]
