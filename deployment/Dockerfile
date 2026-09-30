# Build separately for linux/amd64 and linux/arm64. Publish only after native tests.
FROM rust:1.90.0-bookworm AS build
RUN apt-get update && apt-get install -y --no-install-recommends clang libclang-dev cmake pkg-config protobuf-compiler libssl-dev libsqlite3-dev && rm -rf /var/lib/apt/lists/*
WORKDIR /src
ENV CARGO_BUILD_JOBS=2
COPY . .
ENV BARK_WEB_DIST=/src/web
RUN cargo build --locked --release -p bark-cli --features barkd-web-ui --bins

FROM debian:bookworm-slim AS runtime
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates libssl3 libsqlite3-0 libgcc-s1 python3 tini && rm -rf /var/lib/apt/lists/* && useradd --uid 1000 --create-home paperclip
COPY --from=build /src/target/release/paperclip-wallet /src/target/release/paperclip-walletd /usr/local/bin/
COPY deployment/container-entrypoint.py /usr/local/lib/paperclip/entrypoint.py
COPY deployment/startos-action.py deployment/startos-entrypoint.py /usr/local/lib/paperclip/
COPY deployment/health.py /usr/local/lib/paperclip/
USER 1000:1000
ENV BARKD_DATADIR=/data/wallet BARKD_BIND_HOST=0.0.0.0 BARKD_BIND_PORT=3000
ENV BARKD_UI_DEFAULT_ARK_SERVER=https://ark.paperclippool.xyz
EXPOSE 3000
HEALTHCHECK --interval=30s --timeout=10s --start-period=60s CMD ["python3", "/usr/local/lib/paperclip/health.py"]
ENTRYPOINT ["/usr/bin/tini", "--", "python3", "/usr/local/lib/paperclip/entrypoint.py"]
