# Build from the repository root:  docker build -f deploy/Dockerfile -t ramzes .

FROM node:24-bookworm-slim AS ui
WORKDIR /src/ui
COPY ui/package.json ui/package-lock.json ui/.npmrc ./
RUN npm ci
COPY ui/ ./
RUN npm run build

FROM rust:1.98-slim-bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
# The binary embeds the built UI from ui/build.
COPY --from=ui /src/ui/build ./ui/build
RUN cargo build --release --locked -p ramzes-server

FROM debian:bookworm-slim
# Root certificates are needed to reach HTTPS services such as Slack.
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 --home-dir /data --shell /usr/sbin/nologin ramzes \
    && mkdir /data \
    && chown ramzes:ramzes /data
COPY --from=build /src/target/release/ramzes /usr/local/bin/ramzes
USER ramzes
ENV RAMZES_LISTEN=0.0.0.0:7269 \
    RAMZES_DATA_DIR=/data
VOLUME /data
EXPOSE 7269
# The image has no curl; bash opens the connection itself.
HEALTHCHECK --interval=10s --timeout=3s --start-period=5s \
    CMD bash -c 'exec 3<>/dev/tcp/127.0.0.1/7269 && printf "GET /healthz HTTP/1.0\r\n\r\n" >&3 && grep -q "200 OK" <&3'
ENTRYPOINT ["ramzes"]
