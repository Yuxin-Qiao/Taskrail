FROM rust:1.88-bookworm AS build

WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY README.md ./README.md
COPY crates ./crates
RUN cargo build --locked --release --package taskrail

FROM debian:bookworm-slim

RUN groupadd --system --gid 10001 taskrail \
    && useradd --system --uid 10001 --gid 10001 \
        --home-dir /nonexistent --no-create-home taskrail \
    && mkdir -p /data /run/taskrail \
    && chown -R taskrail:taskrail /data /run/taskrail \
    && apt-get update \
    && apt-get install --yes --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=build /src/target/release/taskrail /usr/local/bin/taskrail

USER taskrail:taskrail
ENV TASKRAIL_MCP_BEARER_TOKEN_ENV=TASKRAIL_MCP_BEARER_TOKEN

VOLUME ["/data", "/run/taskrail"]
EXPOSE 8787
ENTRYPOINT ["taskrail"]
