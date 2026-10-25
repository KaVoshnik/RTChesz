//! Интеграционные тесты: настоящий роутер + настоящая PostgreSQL.
//!
//! Требуют `DATABASE_URL` на сервер Postgres с правом создавать базы
//! (`#[sqlx::test]` сам создаёт чистую БД под каждый тест и прогоняет миграции).
//! В обычном `cargo test` они пропускаются, запуск:
//!
//!     cargo test -- --include-ignored

use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode, header},
    response::Response,
};
use http_body_util::BodyExt;
use rtchesz_api::{config::Config, mail::Mailer, routes::build_router, state::AppState};
use serde_json::{Value, json};
use sqlx::PgPool;
use tower::ServiceExt;

struct TestApp {
    router: Router,
    state: AppState,
}

fn app(pool: PgPool) -> TestApp {
    app_with(pool, |_| {})
}

fn app_with(pool: PgPool, tweak: impl FnOnce(&mut Config)) -> TestApp {
    let mut config = Config::for_tests(String::new());
    tweak(&mut config);

    let state = AppState::new(pool, config, Mailer::capture()).expect("state");
    let router = build_router(state.clone());

    TestApp { router, state }
}

impl TestApp {
    async fn send(&self, request: Request<Body>) -> Response {
        self.router
            .clone()
            .oneshot(request)
            .await
            .expect("response")
    }

    async fn post(&self, path: &str, body: Value) -> (StatusCode, Value) {
        let response = self.post_raw(path, body).await;
        parse(response).await
    }

    async fn post_raw(&self, path: &str, body: Value) -> Response {
        self.send(
            Request::post(path)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
    }

    async fn get(&self, path: &str, bearer: Option<&str>) -> (StatusCode, Value) {
        let mut request = Request::get(path);
        if let Some(token) = bearer {
            request = request.header(header::AUTHORIZATION, format!("Bearer {token}"));
        }

        parse(self.send(request.body(Body::empty()).unwrap()).await).await
    }

    async fn register(&self, username: &str, email: &str, password: &str) -> (StatusCode, Value) {
        self.post(
            "/auth/register",
            json!({ "username": username, "email": email, "password": password }),
        )
        .await
    }

    async fn login(&self, email: &str, password: &str) -> (StatusCode, Value) {
        self.post(
            "/auth/login",
            json!({ "email": email, "password": password }),
        )
        .await
    }

    /// Шестизначный код из последнего отправленного письма.
    fn last_code(&self) -> String {
        let mails = self.state.mailer.captured();
        let body = &mails.last().expect("no email was sent").body;

        body.split_whitespace()
            .find(|word| word.len() == 6 && word.chars().all(|c| c.is_ascii_digit()))
            .expect("no code in email")
            .to_string()
    }
}

async fn parse(response: Response) -> (StatusCode, Value) {
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);

    (status, value)
}

/// Письмо уходит в фоновой задаче - ждём, пока оно появится.
async fn wait_for_mail(app: &TestApp, count: usize) {
    for _ in 0..100 {
        if app.state.mailer.captured().len() >= count {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    panic!("email was not sent in time");
}

// ---------- health ----------

#[ignore = "нужна PostgreSQL (см. комментарий в начале файла)"]
#[sqlx::test(migrations = "./migrations")]
async fn health_reports_database_status(pool: PgPool) {
    let app = app(pool);

    let (status, body) = app.get("/health", None).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!({ "status": "ok", "db": "ok" }));
}

// ---------- регистрация ----------

#[ignore = "нужна PostgreSQL (см. комментарий в начале файла)"]
#[sqlx::test(migrations = "./migrations")]
async fn register_returns_token_that_works_for_me(pool: PgPool) {
    let app = app(pool);

    let (status, body) = app
        .register("  Alice_1 ", "Alice@Example.com ", "supersecret1")
        .await;

    assert_eq!(status, StatusCode::CREATED);
    let token = body["token"].as_str().expect("token");
    assert_eq!(body["user"]["username"], "Alice_1");
    assert_eq!(body["user"]["email"], "alice@example.com");
    assert!(body["user"].get("passwordHash").is_none());
    assert!(body["user"].get("password_hash").is_none());

    let (status, me) = app.get("/auth/me", Some(token)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(me["email"], "alice@example.com");
    assert_eq!(me["id"], body["user"]["id"]);
}

#[ignore = "нужна PostgreSQL (см. комментарий в начале файла)"]
#[sqlx::test(migrations = "./migrations")]
async fn register_rejects_duplicates_case_insensitively(pool: PgPool) {
    let app = app(pool);
    app.register("alice", "alice@example.com", "supersecret1")
        .await;

    let (status, body) = app
        .register("someone", "ALICE@example.com", "supersecret1")
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["message"], "Этот email уже зарегистрирован");
    assert_eq!(body["code"], "conflict");

    let (status, body) = app
        .register("ALICE", "other@example.com", "supersecret1")
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["message"], "Это имя пользователя уже занято");
}

#[ignore = "нужна PostgreSQL (см. комментарий в начале файла)"]
#[sqlx::test(migrations = "./migrations")]
async fn register_validates_input(pool: PgPool) {
    let app = app(pool);

    let cases = [
        ("al", "a@example.com", "supersecret1"),
        ("bad name!", "a@example.com", "supersecret1"),
        ("alice", "not-an-email", "supersecret1"),
        ("alice", "a@example.com", "short"),
    ];

    for (username, email, password) in cases {
        let (status, body) = app.register(username, email, password).await;
        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "{username} {email} {password}"
        );
        assert_eq!(body["code"], "bad_request");
        assert!(body["message"].as_str().is_some_and(|m| !m.is_empty()));
    }
}

#[ignore = "нужна PostgreSQL (см. комментарий в начале файла)"]
#[sqlx::test(migrations = "./migrations")]
async fn malformed_json_gets_json_error_with_message(pool: PgPool) {
    let app = app(pool);

    let response = app
        .send(
            Request::post("/auth/login")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from("{ not json"))
                .unwrap(),
        )
        .await;
    let (status, body) = parse(response).await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["message"], "Некорректный запрос");

    // Не хватает полей.
    let (status, body) = app.post("/auth/login", json!({ "email": "a@b.co" })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["message"], "Некорректный запрос");
}

// ---------- вход ----------

#[ignore = "нужна PostgreSQL (см. комментарий в начале файла)"]
#[sqlx::test(migrations = "./migrations")]
async fn login_succeeds_and_hides_whether_email_exists(pool: PgPool) {
    let app = app(pool);
    app.register("alice", "alice@example.com", "supersecret1")
        .await;

    let (status, body) = app.login("ALICE@example.com", "supersecret1").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["token"].as_str().is_some());

    let (wrong_status, wrong_body) = app.login("alice@example.com", "wrong-password").await;
    let (unknown_status, unknown_body) = app.login("nobody@example.com", "supersecret1").await;

    assert_eq!(wrong_status, StatusCode::UNAUTHORIZED);
    assert_eq!(unknown_status, StatusCode::UNAUTHORIZED);
    assert_eq!(wrong_body, unknown_body);
    assert_eq!(wrong_body["message"], "Неверный email или пароль");
}

#[ignore = "нужна PostgreSQL (см. комментарий в начале файла)"]
#[sqlx::test(migrations = "./migrations")]
async fn login_locks_email_after_too_many_failures_until_success_resets(pool: PgPool) {
    let app = app(pool);
    app.register("alice", "alice@example.com", "supersecret1")
        .await;

    for _ in 0..10 {
        let (status, _) = app.login("alice@example.com", "wrong-password").await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    // Лимит исчерпан: даже верный пароль не принимается, пока не пройдёт окно.
    let response = app
        .post_raw(
            "/auth/login",
            json!({ "email": "alice@example.com", "password": "supersecret1" }),
        )
        .await;
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    let retry_after: u64 = response.headers()[header::RETRY_AFTER]
        .to_str()
        .unwrap()
        .parse()
        .unwrap();
    assert!((1..=900).contains(&retry_after));
}

#[ignore = "нужна PostgreSQL (см. комментарий в начале файла)"]
#[sqlx::test(migrations = "./migrations")]
async fn successful_login_resets_failure_counter(pool: PgPool) {
    let app = app(pool);
    app.register("alice", "alice@example.com", "supersecret1")
        .await;

    for _ in 0..9 {
        app.login("alice@example.com", "wrong-password").await;
    }
    let (status, _) = app.login("alice@example.com", "supersecret1").await;
    assert_eq!(status, StatusCode::OK);

    // Счётчик обнулён: ещё 9 ошибок не блокируют вход.
    for _ in 0..9 {
        app.login("alice@example.com", "wrong-password").await;
    }
    let (status, _) = app.login("alice@example.com", "supersecret1").await;
    assert_eq!(status, StatusCode::OK);
}

// ---------- токен ----------

#[ignore = "нужна PostgreSQL (см. комментарий в начале файла)"]
#[sqlx::test(migrations = "./migrations")]
async fn me_requires_valid_bearer_token(pool: PgPool) {
    let app = app(pool);

    let (status, body) = app.get("/auth/me", None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["code"], "unauthorized");

    let (status, _) = app.get("/auth/me", Some("garbage.token.value")).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let response = app
        .send(Request::get("/auth/me").body(Body::empty()).unwrap())
        .await;
    assert_eq!(response.headers()[header::WWW_AUTHENTICATE], "Bearer");
}

// ---------- восстановление пароля ----------

#[ignore = "нужна PostgreSQL (см. комментарий в начале файла)"]
#[sqlx::test(migrations = "./migrations")]
async fn full_password_reset_flow(pool: PgPool) {
    let app = app(pool);
    app.register("alice", "alice@example.com", "old-password-1")
        .await;

    let (status, _) = app
        .post(
            "/auth/forgot-password",
            json!({ "email": "Alice@Example.com" }),
        )
        .await;
    assert_eq!(status, StatusCode::OK);

    wait_for_mail(&app, 1).await;
    let code = app.last_code();
    assert_eq!(app.state.mailer.captured()[0].to, "alice@example.com");

    // Неверный код.
    let wrong = if code == "000000" { "111111" } else { "000000" };
    let (status, body) = app
        .post(
            "/auth/verify-code",
            json!({ "email": "alice@example.com", "code": wrong }),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["message"], "Неверный или просроченный код");

    // Верный код -> resetToken.
    let (status, body) = app
        .post(
            "/auth/verify-code",
            json!({ "email": "alice@example.com", "code": code }),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let reset_token = body["resetToken"].as_str().expect("resetToken").to_string();

    // Тот же код второй раз уже не работает.
    let (status, _) = app
        .post(
            "/auth/verify-code",
            json!({ "email": "alice@example.com", "code": code }),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // Слишком короткий новый пароль.
    let (status, _) = app
        .post(
            "/auth/reset-password",
            json!({ "resetToken": reset_token, "password": "short" }),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // Меняем пароль.
    let (status, body) = app
        .post(
            "/auth/reset-password",
            json!({ "resetToken": reset_token, "password": "brand-new-password" }),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["message"], "Пароль изменён");

    // Токен одноразовый.
    let (status, _) = app
        .post(
            "/auth/reset-password",
            json!({ "resetToken": reset_token, "password": "another-password" }),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // Старый пароль не работает, новый - работает.
    let (status, _) = app.login("alice@example.com", "old-password-1").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = app.login("alice@example.com", "brand-new-password").await;
    assert_eq!(status, StatusCode::OK);
}

#[ignore = "нужна PostgreSQL (см. комментарий в начале файла)"]
#[sqlx::test(migrations = "./migrations")]
async fn forgot_password_does_not_reveal_unknown_emails_and_has_cooldown(pool: PgPool) {
    let app = app(pool);
    app.register("alice", "alice@example.com", "supersecret1")
        .await;

    let (known_status, known_body) = app
        .post(
            "/auth/forgot-password",
            json!({ "email": "alice@example.com" }),
        )
        .await;
    let (unknown_status, unknown_body) = app
        .post(
            "/auth/forgot-password",
            json!({ "email": "ghost@example.com" }),
        )
        .await;

    assert_eq!(known_status, StatusCode::OK);
    assert_eq!(unknown_status, StatusCode::OK);
    assert_eq!(known_body, unknown_body);

    wait_for_mail(&app, 1).await;
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    assert_eq!(
        app.state.mailer.captured().len(),
        1,
        "письмо только для существующего email"
    );

    // Повтор раньше cooldown -> 429 + Retry-After, одинаково для известных и неизвестных.
    for email in ["alice@example.com", "ghost@example.com"] {
        let response = app
            .post_raw("/auth/forgot-password", json!({ "email": email }))
            .await;
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS, "{email}");

        let retry_after: u64 = response.headers()[header::RETRY_AFTER]
            .to_str()
            .unwrap()
            .parse()
            .unwrap();
        assert!((1..=60).contains(&retry_after), "{retry_after}");
    }
}

#[ignore = "нужна PostgreSQL (см. комментарий в начале файла)"]
#[sqlx::test(migrations = "./migrations")]
async fn reset_code_dies_after_max_wrong_attempts(pool: PgPool) {
    let app = app(pool);
    app.register("alice", "alice@example.com", "supersecret1")
        .await;
    app.post(
        "/auth/forgot-password",
        json!({ "email": "alice@example.com" }),
    )
    .await;
    wait_for_mail(&app, 1).await;
    let code = app.last_code();
    let wrong = if code == "000000" { "111111" } else { "000000" };

    for _ in 0..5 {
        let (status, _) = app
            .post(
                "/auth/verify-code",
                json!({ "email": "alice@example.com", "code": wrong }),
            )
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    // Попытки закончились: верный код уже не принимается, нужно запросить новый.
    let (status, _) = app
        .post(
            "/auth/verify-code",
            json!({ "email": "alice@example.com", "code": code }),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[ignore = "нужна PostgreSQL (см. комментарий в начале файла)"]
#[sqlx::test(migrations = "./migrations")]
async fn new_code_invalidates_previous_one(pool: PgPool) {
    // cooldown = 0, чтобы запросить второй код сразу.
    let app = app_with(pool, |config| {
        config.reset_resend_cooldown = std::time::Duration::from_millis(1);
    });
    app.register("alice", "alice@example.com", "supersecret1")
        .await;

    app.post(
        "/auth/forgot-password",
        json!({ "email": "alice@example.com" }),
    )
    .await;
    wait_for_mail(&app, 1).await;
    let first = app.last_code();

    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
    app.post(
        "/auth/forgot-password",
        json!({ "email": "alice@example.com" }),
    )
    .await;
    wait_for_mail(&app, 2).await;
    let second = app.last_code();

    if first != second {
        let (status, _) = app
            .post(
                "/auth/verify-code",
                json!({ "email": "alice@example.com", "code": first }),
            )
            .await;
        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "старый код должен быть погашен"
        );
    }

    let (status, body) = app
        .post(
            "/auth/verify-code",
            json!({ "email": "alice@example.com", "code": second }),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["resetToken"].is_string());
}

#[ignore = "нужна PostgreSQL (см. комментарий в начале файла)"]
#[sqlx::test(migrations = "./migrations")]
async fn verify_code_validates_format(pool: PgPool) {
    let app = app(pool);

    for code in ["12345", "1234567", "12a456", ""] {
        let (status, body) = app
            .post(
                "/auth/verify-code",
                json!({ "email": "alice@example.com", "code": code }),
            )
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{code}");
        assert_eq!(body["message"], "Введите 6 цифр из письма");
    }
}

#[ignore = "нужна PostgreSQL (см. комментарий в начале файла)"]
#[sqlx::test(migrations = "./migrations")]
async fn reset_password_rejects_unknown_token(pool: PgPool) {
    let app = app(pool);

    let (status, _) = app
        .post(
            "/auth/reset-password",
            json!({ "resetToken": "0".repeat(64), "password": "brand-new-password" }),
        )
        .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
}

// ---------- CORS и прочее ----------

#[ignore = "нужна PostgreSQL (см. комментарий в начале файла)"]
#[sqlx::test(migrations = "./migrations")]
async fn cors_allows_frontend_and_exposes_retry_after(pool: PgPool) {
    let app = app(pool);

    // Preflight от фронта.
    let preflight = app
        .send(
            Request::options("/auth/login")
                .header(header::ORIGIN, "http://localhost:5173")
                .header(header::ACCESS_CONTROL_REQUEST_METHOD, "POST")
                .header(header::ACCESS_CONTROL_REQUEST_HEADERS, "content-type")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert!(preflight.status().is_success());
    assert_eq!(
        preflight.headers()[header::ACCESS_CONTROL_ALLOW_ORIGIN],
        "http://localhost:5173"
    );

    // Обычный ответ отдаёт Retry-After в expose-headers (нужно для 429).
    let response = app
        .send(
            Request::get("/health")
                .header(header::ORIGIN, "http://localhost:5173")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(
        response.headers()[header::ACCESS_CONTROL_EXPOSE_HEADERS],
        "retry-after"
    );

    // Чужой origin не получает разрешения.
    let foreign = app
        .send(
            Request::get("/health")
                .header(header::ORIGIN, "http://evil.example")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert!(
        foreign
            .headers()
            .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
            .is_none()
    );
}

#[ignore = "нужна PostgreSQL (см. комментарий в начале файла)"]
#[sqlx::test(migrations = "./migrations")]
async fn unknown_route_returns_json_404(pool: PgPool) {
    let app = app(pool);

    let (status, body) = app.get("/nope", None).await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["code"], "not_found");
}

#[ignore = "нужна PostgreSQL (см. комментарий в начале файла)"]
#[sqlx::test(migrations = "./migrations")]
async fn registration_is_rate_limited_per_ip(pool: PgPool) {
    let app = app(pool);

    for i in 0..10 {
        let (status, _) = app
            .register(
                &format!("user{i}"),
                &format!("user{i}@example.com"),
                "supersecret1",
            )
            .await;
        assert_eq!(status, StatusCode::CREATED);
    }

    let response = app
        .post_raw(
            "/auth/register",
            json!({ "username": "user11", "email": "user11@example.com", "password": "supersecret1" }),
        )
        .await;
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    assert!(response.headers().contains_key(header::RETRY_AFTER));
}
