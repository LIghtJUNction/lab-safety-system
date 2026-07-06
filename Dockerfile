FROM rust:1.96-slim AS build

LABEL org.opencontainers.image.title="lab-safety-system"
LABEL org.opencontainers.image.description="Rust backend for the Laboratory Safety Management Information System"
LABEL org.opencontainers.image.source="https://github.com/LIghtJUNction/lab-safety-system"

WORKDIR /app

RUN apt-get update \
    && apt-get install -y --no-install-recommends pkg-config libssl-dev \
    && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --release

FROM debian:bookworm-slim

WORKDIR /app

RUN groupadd --system app && useradd --system --gid app --create-home app \
    && mkdir -p /app/uploads \
    && chown -R app:app /app/uploads \
    && apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=build /app/target/release/lab-safety-system /usr/local/bin/lab-safety-system

ENV APP_ENV=production
ENV APP_HOST=0.0.0.0
ENV APP_PORT=8080

EXPOSE 8080

USER app

HEALTHCHECK --interval=30s --timeout=5s --start-period=20s --retries=3 CMD /usr/local/bin/lab-safety-system --healthcheck

CMD ["lab-safety-system"]
