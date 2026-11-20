# syntax=docker/dockerfile:1.7
# Server image: lyrics-api (HTTP + `migrate`) and lyrics-worker in one image.

FROM rust:1-bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock rustfmt.toml ./
COPY crates ./crates
# Migrations are embedded into the binary at compile time.
COPY migrations ./migrations
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked -p api -p worker \
 && install -m 0755 target/release/lyrics-api target/release/lyrics-worker /usr/local/bin/

FROM debian:bookworm-slim
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates curl \
 && rm -rf /var/lib/apt/lists/* \
 && useradd --system --uid 10001 --home /nonexistent --shell /usr/sbin/nologin app \
 && mkdir -p /data/mailbox /data/media && chown app /data/mailbox /data/media
COPY --from=build /usr/local/bin/lyrics-api /usr/local/bin/lyrics-worker /usr/local/bin/
USER app
ENV LYRICS_BIND_ADDR=0.0.0.0:8787 LYRICS_LOG_JSON=1
EXPOSE 8787
CMD ["lyrics-api", "serve"]
