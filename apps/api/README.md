# RTChesz API

Бэкенд на Rust: axum 0.8 + sqlx (PostgreSQL). Описание эндпоинтов - в [`docs/API.md`](../../docs/API.md).

## Быстрый старт

```bash
# 1. База (из корня репозитория)
cp .env.example .env              # задайте POSTGRES_PASSWORD
docker compose -f infra/docker-compose.yml --env-file .env up -d

# 2. API
cd apps/api
cp .env.example .env              # JWT_SECRET: openssl rand -hex 32
cargo run                         # миграции применяются сами при старте
```

API слушает `http://127.0.0.1:3000`. Для фронта в `apps/web/.env`:
`VITE_API_URL=http://localhost:3000`.

Пока `SMTP_HOST` не задан, письма **не отправляются**: код восстановления пароля
печатается в лог сервера (`[dev mail]`). Для проверки отправки подойдёт MailHog/Mailpit:
`SMTP_HOST=localhost`, `SMTP_PORT=1025`, `SMTP_SECURITY=none`, `SMTP_FROM=noreply@rtchesz.local`.

## Переменные окружения

| Переменная | По умолчанию | Описание |
| --- | --- | --- |
| `DATABASE_URL` | - (обязательна) | строка подключения к PostgreSQL |
| `JWT_SECRET` | - (обязательна) | секрет подписи токенов, **минимум 32 символа** |
| `HOST`, `PORT` | `127.0.0.1`, `3000` | адрес сервера |
| `CORS_ORIGINS` | `http://localhost:5173,http://127.0.0.1:5173` | адреса фронта через запятую |
| `TRUST_PROXY` | `false` | брать IP из `X-Forwarded-For` (только за своим прокси) |
| `JWT_TTL_HOURS` | `168` | срок жизни токена входа |
| `RESET_CODE_TTL_MINUTES` | `10` | срок жизни кода из письма |
| `RESET_TOKEN_TTL_MINUTES` | `15` | срок жизни токена смены пароля |
| `RESET_RESEND_COOLDOWN_SECS` | `60` | пауза между письмами на один email |
| `RESET_MAX_ATTEMPTS` | `5` | попыток ввода одного кода |
| `SMTP_HOST`, `SMTP_PORT`, `SMTP_SECURITY`, `SMTP_USERNAME`, `SMTP_PASSWORD`, `SMTP_FROM` | - | отправка почты (см. `.env.example`) |

## Структура

```
src/
  main.rs            запуск: конфиг, БД, миграции, сервер, graceful shutdown
  lib.rs             модули (нужен, чтобы интеграционные тесты видели роутер)
  config.rs          переменные окружения
  state.rs           AppState (пул БД, JWT, лимитер, почта)
  error.rs           AppError -> JSON { code, message } (+ Retry-After для 429)
  extract.rs         ApiJson (ошибки JSON в нашем формате), ClientIp
  validation.rs      проверки email / username / пароля
  rate_limit.rs      лимиты запросов в памяти
  mail.rs            SMTP / лог (dev) / перехват (тесты)
  auth/              пароли (Argon2id), JWT, коды сброса, extractor AuthUser
  repo/              SQL-запросы (users, password_resets)
  routes/            health и /auth/*
migrations/          0001 users, 0002 ужесточение users, 0003 password_resets
tests/api.rs         интеграционные тесты
```

Закрыть новый маршрут токеном - добавить аргумент `AuthUser` в обработчик:

```rust
async fn my_games(State(state): State<AppState>, auth: AuthUser) -> Result<..., AppError> {
    // auth.id - UUID пользователя
}
```

## Тесты

```bash
cargo test                          # юнит-тесты (как в CI, БД не нужна)

# + интеграционные тесты на реальной PostgreSQL (на каждый тест создаётся чистая БД):
DATABASE_URL=postgres://rtchesz:change_local@localhost:5433/rtchesz cargo test -- --include-ignored
```

Пользователю из `DATABASE_URL` нужно право `CREATEDB` (в docker-compose он владелец БД, этого хватает).

Чтобы интеграционные тесты шли и в CI, добавьте в `.github/workflows/ci.yml` сервис Postgres
и замените шаг `cargo test`:

```yaml
    services:
      postgres:
        image: postgres:18
        env:
          POSTGRES_PASSWORD: postgres
        ports: ["5432:5432"]
        options: >-
          --health-cmd pg_isready --health-interval 5s --health-timeout 5s --health-retries 10
    env:
      DATABASE_URL: postgres://postgres:postgres@localhost:5432/postgres
    # ...
      - name: Test
        working-directory: ./apps/api
        run: cargo test -- --include-ignored
```

## Заметки

- SQL написан обычными запросами (`sqlx::query_as`), а не макросами `query!`: для сборки и CI не нужна живая БД.
- sqlx 0.9 принимает только литеральные SQL-строки - собирать запрос через `format!` не получится (это защита от инъекций).
- Лимиты запросов хранятся в памяти одного процесса. При нескольких экземплярах API их нужно переносить в Redis.
