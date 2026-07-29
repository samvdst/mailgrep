# ---- web build ----
FROM node:24-bookworm-slim AS web
WORKDIR /app/web
COPY web/package.json web/package-lock.json* ./
RUN npm install
COPY web/ ./
RUN npm run build

# ---- rust build ----
FROM rust:1.97-bookworm AS build
WORKDIR /app
RUN apt-get update && apt-get install -y --no-install-recommends pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --release

# ---- runtime ----
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates libssl3 && rm -rf /var/lib/apt/lists/*
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
