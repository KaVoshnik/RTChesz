# Сборка Rust-API. Контекст сборки - КОРЕНЬ репозитория (см. docker-compose.prod.yml).
FROM rust:1-bookworm AS build
WORKDIR /app

# 1) Сначала только зависимости - этот слой кэшируется, пока не меняется Cargo.lock.
COPY apps/api/Cargo.toml apps/api/Cargo.lock ./
RUN mkdir src && echo "" > src/lib.rs && echo "fn main() {}" > src/main.rs \
    && cargo build --release --locked \
    && rm -rf src

# 2) Теперь настоящий код (migrations нужны на этапе компиляции: sqlx::migrate!).
COPY apps/api/migrations ./migrations
COPY apps/api/src ./src
RUN touch src/lib.rs src/main.rs && cargo build --release --locked

# --- Рантайм: маленький образ без компилятора ---
FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --no-create-home --shell /usr/sbin/nologin rtchesz
COPY --from=build /app/target/release/rtchesz-api /usr/local/bin/rtchesz-api
USER rtchesz
ENV HOST=0.0.0.0 PORT=3000
EXPOSE 3000
CMD ["rtchesz-api"]
