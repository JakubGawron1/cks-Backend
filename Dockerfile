# syntax=docker/dockerfile:1
# Hosting produkcyjny: Hugging Face Space (koliber/cks-slavia) lub Render.
# Lokalny development: cargo run (bez Dockera).

FROM rust:1-bookworm AS builder
WORKDIR /app

# Oszczędność RAM przy buildzie (HF / Render Free)
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
# Produkcja: ustaw w Space secrets PRODUCTION_MODE=production, DATABASE_URL, TURSO_AUTH_TOKEN
ENV PRODUCTION_MODE=production
ENV RUST_LOG=slavia_backend=info,tower_http=info,axum=info

EXPOSE 8080
CMD ["/app/slavia-backend"]
