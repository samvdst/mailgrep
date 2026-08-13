# ---- web build ----
FROM node:24-bookworm-slim AS web
WORKDIR /app/web
COPY web/package.json web/package-lock.json* ./
RUN npm ci
COPY web/ ./
RUN npm run build

# ---- rust build ----
# rust:bookworm (buildpack-deps) already carries pkg-config, OpenSSL headers
# and CA certs, so no apt is needed, which also keeps rootless builds happy.
FROM rust:1.97-bookworm AS build
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --release
# stage the OpenSSL runtime libs at an arch-neutral path so the same
# Dockerfile builds on amd64 and arm64
RUN mkdir /ssl-libs && cp /usr/lib/*-linux-gnu/libssl.so.3 /usr/lib/*-linux-gnu/libcrypto.so.3 /ssl-libs/

# ---- runtime ----
FROM debian:bookworm-slim
# OpenSSL runtime + CA bundle lifted from the (same-release) build image.
# /usr/lib is on the dynamic linker's default search path.
COPY --from=build /ssl-libs/ /usr/lib/
COPY --from=build /etc/ssl/certs /etc/ssl/certs
WORKDIR /app
COPY --from=build /app/target/release/mailgrep /usr/local/bin/mailgrep
COPY --from=web /app/web/dist /app/web/dist
ENV MAILGREP_DATA=/data \
    MAILGREP_WEB=/app/web/dist \
    MAILGREP_BIND=0.0.0.0:8025
# All state lives in one directory: mount a named volume here (on WSL2,
# never a bind mount from a Windows drive, where small random I/O is ~10x slower
# there, which is exactly the index and database workload).
VOLUME /data
EXPOSE 8025
CMD ["mailgrep"]
