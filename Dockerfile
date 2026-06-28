# Stage 1: Build frontend
FROM node:22-slim AS frontend-builder
WORKDIR /app/frontend
COPY frontend/package.json frontend/package-lock.json ./
RUN npm ci
COPY frontend/ ./
RUN npm run build

# Stage 2: Build Rust backend
FROM rust:1.87-slim AS rust-builder
RUN apt-get update && apt-get install -y pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src/ ./src/
RUN cargo build --release

# Stage 3: Final image
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y \
    ipmitool \
    openssh-client \
    sshpass \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=rust-builder /app/target/release/servmgr /usr/local/bin/servmgr
COPY --from=frontend-builder /app/frontend/build /static

ENV PORT=8080
ENV CONFIG_DIR=/config
ENV STATIC_DIR=/static

VOLUME /config
EXPOSE 8080

ENTRYPOINT ["servmgr"]
