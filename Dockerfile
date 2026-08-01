# syntax=docker/dockerfile:1
# UWAGA: ten Dockerfile jest WYŁĄCZNIE pod hosting Hugging Face.
# Lokalny development: cargo run (bez Dockera).

FROM rust:1.88-bookworm AS builder
WORKDIR /app
RUN apt-get update && apt-get install -y --no-install-recommends clang libclang-dev cmake \
  && rm -rf /var/lib/apt/lists/*
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
  && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=builder /app/target/release/slavia-backend /app/slavia-backend
RUN mkdir -p /app/data
ENV HOST=0.0.0.0
ENV PORT=8080
ENV DATABASE_URL=file:/app/data/slavia.redb
EXPOSE 8080
CMD ["/app/slavia-backend"]
