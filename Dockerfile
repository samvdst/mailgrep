# ---- web build ----
FROM node:24-bookworm-slim AS web
WORKDIR /app/web
COPY web/package.json web/package-lock.json* ./
RUN npm install
COPY web/ ./
RUN npm run build

# ---- rust build ----
# rust:bookworm (buildpack-deps) already carries pkg-config, OpenSSL headers
# and CA certs — no apt needed, which also keeps rootless builds happy.
FROM rust:1.97-bookworm AS build
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --release

# ---- runtime ----
FROM debian:bookworm-slim
# OpenSSL runtime + CA bundle lifted from the (same-release) build image.
COPY --from=build /usr/lib/x86_64-linux-gnu/libssl.so.3 /usr/lib/x86_64-linux-gnu/
COPY --from=build /usr/lib/x86_64-linux-gnu/libcrypto.so.3 /usr/lib/x86_64-linux-gnu/
COPY --from=build /etc/ssl/certs /etc/ssl/certs
WORKDIR /app
COPY --from=build /app/target/release/mailgrep /usr/local/bin/mailgrep
COPY --from=web /app/web/dist /app/web/dist
ENV MAILGREP_DATA=/data \
    MAILGREP_WEB=/app/web/dist \
    MAILGREP_BIND=0.0.0.0:8025
# All state lives in one directory: mount a named volume here (never a
# Windows bind mount — see DESIGN.md §13).
VOLUME /data
EXPOSE 8025
CMD ["mailgrep"]
