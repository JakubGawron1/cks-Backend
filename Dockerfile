# syntax=docker/dockerfile:1
# UWAGA: wyłącznie pod hosting produkcyjny (Render Free).
# Lokalny development: cargo run (bez Dockera).

FROM rust:1-bookworm AS builder
WORKDIR /app

# Render Free: mniej równoległych jobów = mniejsze ryzyko OOM przy buildzie Rusta
ENV CARGO_BUILD_JOBS=2
ENV CARGO_TERM_COLOR=never

COPY Cargo.toml Cargo.lock ./
RUN mkdir src \
  && echo 'fn main() {}' > src/main.rs \
  && cargo build --release \
  && rm -rf src

COPY src ./src
RUN touch src/main.rs && cargo build --release

FROM debian:bookworm-slim
RUN apt-get update \
  && apt-get install -y --no-install-recommends ca-certificates \
  && rm -rf /var/lib/apt/lists/* \
  && useradd --system --uid 10001 --create-home --home-dir /app appuser

WORKDIR /app
COPY --from=builder /app/target/release/slavia-backend /app/slavia-backend
RUN mkdir -p /app/data && chown -R appuser:appuser /app

USER appuser

ENV HOST=0.0.0.0
ENV PORT=8080
ENV DATABASE_URL=file:/app/data/slavia.redb
ENV RUST_LOG=slavia_backend=info,tower_http=info,axum=info

EXPOSE 8080
CMD ["/app/slavia-backend"]
