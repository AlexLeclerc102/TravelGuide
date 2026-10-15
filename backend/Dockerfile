# --- Build ---
FROM rust:1-bookworm AS build
WORKDIR /src
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release --bin api && cp target/release/api /usr/local/bin/api

# --- Runtime ---
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 app
COPY --from=build /usr/local/bin/api /usr/local/bin/api
USER app
EXPOSE 8080
HEALTHCHECK --interval=10s --timeout=3s --retries=5 \
    CMD curl -fsS http://localhost:8080/api/v1/health || exit 1
CMD ["api"]
