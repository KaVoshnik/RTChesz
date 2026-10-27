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
| `DB_MAX_CONNECTIONS` | `10` | размер пула соединений с БД |
| `DB_ACQUIRE_TIMEOUT_SECS` | `5` | сколько запрос ждёт свободное соединение, потом `503` |
| `DB_STATEMENT_TIMEOUT_SECS` | `10` | предел времени одного SQL-запроса (`0` - без лимита); на миграции не действует |
| `SHUTDOWN_GRACE_SECS` | `20` | сколько после SIGTERM / Ctrl-C ждать активные запросы |
| `LOG_FORMAT` | `text` | `json` - одна JSON-строка на событие (для продакшена) |
| `METRICS_ADDR` | - (выкл.) | адрес отдельного сервера с `/metrics`, например `0.0.0.0:9100` |
| `SMTP_HOST`, `SMTP_PORT`, `SMTP_SECURITY`, `SMTP_USERNAME`, `SMTP_PASSWORD`, `SMTP_FROM` | - | отправка почты (см. `.env.example`) |

## Структура

```
src/
  main.rs            запуск: конфиг, БД, миграции, сервер, graceful shutdown (SIGTERM / Ctrl-C),
                     подкоманда `healthcheck` для Docker
  lib.rs             модули (нужен, чтобы интеграционные тесты видели роутер)
  config.rs          переменные окружения
  state.rs           AppState (пул БД, JWT, лимитер, почта)
  db.rs              пул с таймаутами; отдельное соединение для миграций
  observability.rs   middleware: request ID, логи запросов, HTTP-метрики; настройка JSON-логов
  metrics.rs         рекордер Prometheus, состояние пула БД
  error.rs           AppError -> JSON { code, message } (+ Retry-After для 429 и 503)
  extract.rs         ApiJson (ошибки JSON в нашем формате), ClientIp
  validation.rs      проверки email / username / пароля
  rate_limit.rs      лимиты запросов в памяти
  mail.rs            SMTP / лог (dev) / перехват (тесты)
  auth/              пароли (Argon2id), JWT, коды сброса, extractor AuthUser
  repo/              SQL-запросы (users, password_resets)
  routes/            health, /auth/*, отдельный роутер /metrics
migrations/          0001 users, 0002 ужесточение users, 0003 password_resets, 0004 token_version
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
cargo test                          # только юнит-тесты, БД не нужна

# + интеграционные тесты на реальной PostgreSQL (на каждый тест создаётся чистая БД):
DATABASE_URL=postgres://rtchesz:change_local@localhost:5433/rtchesz cargo test -- --include-ignored
```

Пользователю из `DATABASE_URL` нужно право `CREATEDB` (в docker-compose он владелец БД, этого хватает).

В CI (`.github/workflows/ci.yml`) поднимается Postgres 18 и запускается `cargo test -- --include-ignored`,
то есть интеграционные тесты идут при каждом пуше. После `fmt`, `clippy` и тестов на `main` выполняется деплой
(см. [`docs/OPERATIONS.md`](../../docs/OPERATIONS.md)).

## Заметки

- SQL написан обычными запросами (`sqlx::query_as`), а не макросами `query!`: для сборки и CI не нужна живая БД.
- sqlx 0.9 принимает только литеральные SQL-строки - собирать запрос через `format!` не получится (это защита от инъекций).
- Лимиты запросов хранятся в памяти одного процесса. При нескольких экземплярах API их нужно переносить в Redis.
