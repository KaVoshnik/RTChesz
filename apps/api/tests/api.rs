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
use rtchesz_api::{
    config::Config,
    db,
    mail::Mailer,
    routes::{build_metrics_router, build_router},
    state::AppState,
};
use serde_json::{Value, json};
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

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

#[ignore = "нужна PostgreSQL (см. комментарий в начале файла)"]
#[sqlx::test(migrations = "./migrations")]
async fn logout_revokes_all_tokens_of_the_user(pool: PgPool) {
    let app = app(pool);
    let (_, body) = app
        .register("alice", "alice@example.com", "password-123")
        .await;
    let first = body["token"].as_str().unwrap().to_string();

    let (_, body) = app.login("alice@example.com", "password-123").await;
    let second = body["token"].as_str().unwrap().to_string();

    // Другой пользователь выход Alice не затрагивает.
    let (_, body) = app.register("bob", "bob@example.com", "password-123").await;
    let bob = body["token"].as_str().unwrap().to_string();

    let response = app
        .send(
            Request::post("/auth/logout")
                .header(header::AUTHORIZATION, format!("Bearer {first}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);

    // Отозваны оба токена Alice (и тот, которым вышли, и выданный при другом входе).
    assert_eq!(
        app.get("/auth/me", Some(&first)).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        app.get("/auth/me", Some(&second)).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(app.get("/auth/me", Some(&bob)).await.0, StatusCode::OK);

    // Новый вход выдаёт рабочий токен.
    let (_, body) = app.login("alice@example.com", "password-123").await;
    let fresh = body["token"].as_str().unwrap();
    assert_eq!(app.get("/auth/me", Some(fresh)).await.0, StatusCode::OK);
}

#[ignore = "нужна PostgreSQL (см. комментарий в начале файла)"]
#[sqlx::test(migrations = "./migrations")]
async fn logout_requires_authorization(pool: PgPool) {
    let app = app(pool);

    let response = app
        .send(Request::post("/auth/logout").body(Body::empty()).unwrap())
        .await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[ignore = "нужна PostgreSQL (см. комментарий в начале файла)"]
#[sqlx::test(migrations = "./migrations")]
async fn token_without_ver_claim_issued_before_update_still_works(pool: PgPool) {
    use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
    use hmac::{Hmac, KeyInit, Mac};
    use sha2::Sha256;

    let app = app(pool);
    let (_, body) = app
        .register("alice", "alice@example.com", "password-123")
        .await;
    let user_id = body["user"]["id"].as_str().unwrap().to_string();

    // Токен в старом формате: без claim `ver`.
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let header = URL_SAFE_NO_PAD.encode(r#"{"typ":"JWT","alg":"HS256"}"#);
    let payload =
        URL_SAFE_NO_PAD.encode(json!({ "sub": user_id, "iat": now, "exp": now + 600 }).to_string());
    let mut mac = Hmac::<Sha256>::new_from_slice(app.state.config.jwt_secret.as_bytes()).unwrap();
    mac.update(format!("{header}.{payload}").as_bytes());
    let signature = URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes());
    let legacy = format!("{header}.{payload}.{signature}");

    assert_eq!(app.get("/auth/me", Some(&legacy)).await.0, StatusCode::OK);
}

#[ignore = "нужна PostgreSQL (см. комментарий в начале файла)"]
#[sqlx::test(migrations = "./migrations")]
async fn token_of_deleted_user_is_rejected(pool: PgPool) {
    let app = app(pool.clone());
    let (_, body) = app
        .register("alice", "alice@example.com", "password-123")
        .await;
    let token = body["token"].as_str().unwrap().to_string();

    sqlx::query("DELETE FROM users")
        .execute(&pool)
        .await
        .unwrap();

    assert_eq!(
        app.get("/auth/me", Some(&token)).await.0,
        StatusCode::UNAUTHORIZED
    );
}

// ---------- восстановление пароля ----------

#[ignore = "нужна PostgreSQL (см. комментарий в начале файла)"]
#[sqlx::test(migrations = "./migrations")]
async fn full_password_reset_flow(pool: PgPool) {
    let app = app(pool);
    let (_, body) = app
        .register("alice", "alice@example.com", "old-password-1")
        .await;
    let old_token = body["token"].as_str().unwrap().to_string();
    assert_eq!(
        app.get("/auth/me", Some(&old_token)).await.0,
        StatusCode::OK
    );

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
    let (status, body) = app.login("alice@example.com", "brand-new-password").await;
    assert_eq!(status, StatusCode::OK);

    // Смена пароля отзывает токены, выданные до неё; новый токен работает.
    assert_eq!(
        app.get("/auth/me", Some(&old_token)).await.0,
        StatusCode::UNAUTHORIZED
    );
    let new_token = body["token"].as_str().unwrap();
    assert_eq!(app.get("/auth/me", Some(new_token)).await.0, StatusCode::OK);
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

    // Обычный ответ отдаёт Retry-After (нужен для 429) и X-Request-Id в expose-headers.
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
        "retry-after,x-request-id"
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

// ---------- request ID ----------

#[ignore = "нужна PostgreSQL (см. комментарий в начале файла)"]
#[sqlx::test(migrations = "./migrations")]
async fn every_response_has_request_id_and_safe_incoming_id_is_kept(pool: PgPool) {
    let app = app(pool);

    // Своего ID нет - генерируем (UUID), и он разный у разных запросов.
    let first = app
        .send(Request::get("/health").body(Body::empty()).unwrap())
        .await;
    let second = app
        .send(Request::get("/health").body(Body::empty()).unwrap())
        .await;
    let first_id = first.headers()["x-request-id"]
        .to_str()
        .unwrap()
        .to_string();
    let second_id = second.headers()["x-request-id"].to_str().unwrap();
    assert_eq!(first_id.len(), 36);
    assert_ne!(first_id, second_id);

    // Безопасный чужой ID сохраняется - так можно сквозно искать запрос в логах.
    let response = app
        .send(
            Request::get("/health")
                .header("x-request-id", "trace-abc_123")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(response.headers()["x-request-id"], "trace-abc_123");

    // Мусор (пробелы, спецсимволы, слишком длинный) заменяется на наш UUID.
    for bad in ["has space", "inj\"ect{}", &"a".repeat(65)] {
        let response = app
            .send(
                Request::get("/health")
                    .header("x-request-id", bad)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await;
        assert_eq!(response.headers()["x-request-id"].len(), 36, "{bad}");
    }

    // Ошибки тоже помечены ID (404 и 401).
    let response = app
        .send(Request::get("/nope").body(Body::empty()).unwrap())
        .await;
    assert!(response.headers().contains_key("x-request-id"));
}

// ---------- метрики ----------

#[ignore = "нужна PostgreSQL (см. комментарий в начале файла)"]
#[sqlx::test(migrations = "./migrations")]
async fn metrics_endpoint_reports_http_auth_and_pool_metrics(pool: PgPool) {
    let app = app(pool.clone());

    app.register("alice", "alice@example.com", "password-123")
        .await;
    app.login("alice@example.com", "wrong-password").await;
    app.login("alice@example.com", "password-123").await;
    app.send(
        Request::get("/definitely/unknown/1234")
            .body(Body::empty())
            .unwrap(),
    )
    .await;

    let metrics_router = build_metrics_router(app.state.clone());
    let response = metrics_router
        .oneshot(Request::get("/metrics").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        response.headers()[header::CONTENT_TYPE]
            .to_str()
            .unwrap()
            .starts_with("text/plain")
    );

    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let text = String::from_utf8(bytes.to_vec()).unwrap();

    // Метки пути - шаблон маршрута; неизвестные пути схлопываются в "unmatched".
    assert!(
        text.contains(r#"http_requests_total{method="POST",path="/auth/login",status="401"}"#),
        "{text}"
    );
    assert!(
        text.contains(r#"path="/auth/register",status="201""#),
        "{text}"
    );
    assert!(text.contains(r#"path="unmatched",status="404""#), "{text}");
    assert!(!text.contains("definitely/unknown"), "{text}");
    assert!(
        text.contains("http_request_duration_seconds_bucket"),
        "{text}"
    );
    assert!(
        text.contains(r#"auth_logins_total{result="failure"}"#),
        "{text}"
    );
    assert!(
        text.contains(r#"auth_logins_total{result="success"}"#),
        "{text}"
    );
    assert!(text.contains("auth_registrations_total"), "{text}");
    assert!(
        text.contains(r#"db_pool_connections{state="max"}"#),
        "{text}"
    );
    assert!(text.contains("rtchesz_api_build_info"), "{text}");

    // /metrics нет на публичном роутере API.
    let (status, _) = app.get("/metrics", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

// ---------- таймауты БД ----------

#[ignore = "нужна PostgreSQL (см. комментарий в начале файла)"]
#[sqlx::test(migrations = "./migrations")]
async fn exhausted_db_pool_gives_503_with_retry_after(pool: PgPool) {
    let mut config = Config::for_tests(String::new());
    config.db_max_connections = 1;
    config.db_acquire_timeout = std::time::Duration::from_millis(200);

    let tight = db::connect_with((*pool.connect_options()).clone(), &config)
        .await
        .expect("pool");
    let state = AppState::new(tight.clone(), config, Mailer::capture()).expect("state");
    let app = TestApp {
        router: build_router(state.clone()),
        state,
    };

    // Единственное соединение занято - запросу не из чего взять БД.
    let _held = tight.acquire().await.expect("connection");

    let response = app
        .post_raw(
            "/auth/login",
            json!({ "email": "alice@example.com", "password": "password-123" }),
        )
        .await;

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(response.headers()[header::RETRY_AFTER], "5");
    let (_, body) = parse(response).await;
    assert_eq!(body["code"], "unavailable");
}

#[ignore = "нужна PostgreSQL (см. комментарий в начале файла)"]
#[sqlx::test(migrations = "./migrations")]
async fn statement_timeout_cancels_slow_queries(pool: PgPool) {
    let mut config = Config::for_tests(String::new());
    config.db_statement_timeout = std::time::Duration::from_millis(150);

    let limited = db::connect_with((*pool.connect_options()).clone(), &config)
        .await
        .expect("pool");

    let started = std::time::Instant::now();
    let error = sqlx::query("SELECT pg_sleep(5)")
        .execute(&limited)
        .await
        .expect_err("query must be cancelled");

    assert!(started.elapsed() < std::time::Duration::from_secs(2));
    // 57014 - query_canceled; API отвечает на такое 503.
    let sqlx::Error::Database(db_error) = &error else {
        panic!("unexpected error: {error}");
    };
    assert_eq!(db_error.code().as_deref(), Some("57014"));
    assert!(matches!(
        rtchesz_api::error::AppError::from(error),
        rtchesz_api::error::AppError::Unavailable
    ));
}

// ---------- партии и часы ----------

impl TestApp {
    async fn post_auth(&self, path: &str, body: Value, token: &str) -> (StatusCode, Value) {
        parse(
            self.send(
                Request::post(path)
                    .header(header::CONTENT_TYPE, "application/json")
                    .header(header::AUTHORIZATION, format!("Bearer {token}"))
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await,
        )
        .await
    }

    /// Регистрирует игрока и возвращает его токен.
    async fn player(&self, name: &str) -> String {
        let (status, body) = self
            .register(name, &format!("{name}@example.com"), "password123")
            .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");

        body["token"].as_str().unwrap().to_string()
    }

    /// Белые - `white`, чёрные - `black`; партия уже началась. Возвращает id.
    async fn started_game(&self, white: &str, black: &str, initial: i64, increment: i64) -> String {
        let (status, body) = self
            .post_auth(
                "/games",
                json!({
                    "timeControl": { "initialSecs": initial, "incrementSecs": increment },
                    "color": "white"
                }),
                white,
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        let id = body["id"].as_str().unwrap().to_string();

        let (status, body) = self
            .post_auth(&format!("/games/{id}/join"), json!({}), black)
            .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["status"], "active");

        id
    }

    async fn play(&self, id: &str, token: &str, mv: &str) -> (StatusCode, Value) {
        self.post_auth(&format!("/games/{id}/moves"), json!({ "move": mv }), token)
            .await
    }

    async fn game(&self, id: &str, token: &str) -> Value {
        let (status, body) = self.get(&format!("/games/{id}"), Some(token)).await;
        assert_eq!(status, StatusCode::OK, "{body}");

        body
    }

    /// Двигает назад момент, с которого тикают часы: имитация "прошло время".
    async fn age_clock(&self, id: &str, secs: i64) {
        sqlx::query(
            "UPDATE games SET turn_started_at = turn_started_at - make_interval(secs => $2), \
                 last_move_at = last_move_at - make_interval(secs => $2) WHERE id = $1::uuid",
        )
        .bind(id)
        .bind(secs as f64)
        .execute(&self.state.pool)
        .await
        .unwrap();
    }

    /// Играет ходы по очереди, начиная с белых.
    async fn play_all(&self, id: &str, white: &str, black: &str, moves: &[&str]) {
        for (index, mv) in moves.iter().enumerate() {
            let token = if index % 2 == 0 { white } else { black };
            let (status, body) = self.play(id, token, mv).await;
            assert_eq!(status, StatusCode::OK, "move {mv}: {body}");
        }
    }
}

#[ignore = "нужна PostgreSQL (см. комментарий в начале файла)"]
#[sqlx::test(migrations = "./migrations")]
async fn games_require_auth(pool: PgPool) {
    let app = app(pool);

    let (status, _) = app.get("/games/lobby", None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let (status, _) = app
        .post("/games", json!({ "timeControl": { "initialSecs": 300 } }))
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[ignore = "нужна PostgreSQL (см. комментарий в начале файла)"]
#[sqlx::test(migrations = "./migrations")]
async fn create_validates_time_control_and_color(pool: PgPool) {
    let app = app(pool);
    let alice = app.player("alice").await;

    for body in [
        json!({ "timeControl": { "initialSecs": 5 } }),
        json!({ "timeControl": { "initialSecs": 999_999 } }),
        json!({ "timeControl": { "initialSecs": 300, "incrementSecs": 500 } }),
        json!({ "timeControl": { "initialSecs": 300, "incrementSecs": -1 } }),
        json!({ "timeControl": { "initialSecs": 300 }, "color": "green" }),
        json!({}),
    ] {
        let (status, response) = app.post_auth("/games", body.clone(), &alice).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}: {response}");
    }

    // Минимум: только время, без добавления и цвета.
    let (status, body) = app
        .post_auth(
            "/games",
            json!({ "timeControl": { "initialSecs": 60 } }),
            &alice,
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["status"], "waiting");
    assert_eq!(body["timeControl"]["initialSecs"], 60);
    assert_eq!(body["timeControl"]["incrementSecs"], 0);
    assert_eq!(body["clock"]["whiteMs"], 60_000);
    assert_eq!(body["clock"]["running"], Value::Null);
}

#[ignore = "нужна PostgreSQL (см. комментарий в начале файла)"]
#[sqlx::test(migrations = "./migrations")]
async fn lobby_join_and_open_challenge_limit(pool: PgPool) {
    let app = app(pool);
    let alice = app.player("alice").await;
    let bob = app.player("bob").await;

    let (_, created) = app
        .post_auth(
            "/games",
            json!({ "timeControl": { "initialSecs": 300 }, "color": "black" }),
            &alice,
        )
        .await;
    let id = created["id"].as_str().unwrap();
    assert_eq!(created["black"]["username"], "alice");
    assert_eq!(created["white"], Value::Null);

    // Свой вызов в лобби не показывается, чужой - да.
    let (_, lobby) = app.get("/games/lobby", Some(&alice)).await;
    assert_eq!(lobby["games"].as_array().unwrap().len(), 0);
    let (_, lobby) = app.get("/games/lobby", Some(&bob)).await;
    assert_eq!(lobby["games"][0]["id"], id);

    // Против самого себя нельзя.
    let (status, _) = app
        .post_auth(&format!("/games/{id}/join"), json!({}), &alice)
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, joined) = app
        .post_auth(&format!("/games/{id}/join"), json!({}), &bob)
        .await;
    assert_eq!(status, StatusCode::OK, "{joined}");
    assert_eq!(joined["white"]["username"], "bob");
    assert_eq!(joined["turn"], "white");
    assert_eq!(
        joined["fen"],
        "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1"
    );

    // Третий присоединиться уже не может; из лобби партия исчезла.
    let carol = app.player("carol").await;
    let (status, _) = app
        .post_auth(&format!("/games/{id}/join"), json!({}), &carol)
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    let (_, lobby) = app.get("/games/lobby", Some(&carol)).await;
    assert_eq!(lobby["games"].as_array().unwrap().len(), 0);

    // Больше пяти открытых вызовов у одного игрока не бывает.
    for _ in 0..5 {
        let (status, body) = app
            .post_auth(
                "/games",
                json!({ "timeControl": { "initialSecs": 300 } }),
                &carol,
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
    }
    let (status, _) = app
        .post_auth(
            "/games",
            json!({ "timeControl": { "initialSecs": 300 } }),
            &carol,
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
}

#[ignore = "нужна PostgreSQL (см. комментарий в начале файла)"]
#[sqlx::test(migrations = "./migrations")]
async fn mine_lists_own_games_and_filters_status(pool: PgPool) {
    let app = app(pool);
    let alice = app.player("alice").await;
    let bob = app.player("bob").await;
    let id = app.started_game(&alice, &bob, 300, 0).await;

    let (_, mine) = app.get("/games/mine", Some(&bob)).await;
    assert_eq!(mine["games"][0]["id"], id);

    let (_, active) = app.get("/games/mine?status=active", Some(&alice)).await;
    assert_eq!(active["games"].as_array().unwrap().len(), 1);
    let (_, finished) = app.get("/games/mine?status=finished", Some(&alice)).await;
    assert_eq!(finished["games"].as_array().unwrap().len(), 0);

    let (status, _) = app.get("/games/mine?status=bogus", Some(&alice)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[ignore = "нужна PostgreSQL (см. комментарий в начале файла)"]
#[sqlx::test(migrations = "./migrations")]
async fn full_game_to_checkmate(pool: PgPool) {
    let app = app(pool);
    let alice = app.player("alice").await;
    let bob = app.player("bob").await;
    let id = app.started_game(&alice, &bob, 300, 0).await;

    // Детский мат.
    app.play_all(
        &id,
        &alice,
        &bob,
        &["e2e4", "e7e5", "d1h5", "b8c6", "f1c4", "g8f6", "h5f7"],
    )
    .await;

    let game = app.game(&id, &bob).await;
    assert_eq!(game["status"], "finished");
    assert_eq!(game["result"], "white");
    assert_eq!(game["termination"], "checkmate");
    assert_eq!(game["turn"], Value::Null);
    assert_eq!(game["clock"]["running"], Value::Null);
    assert_eq!(game["ply"], 7);
    assert_eq!(game["moves"][6], "h5f7");

    // После конца ходить нельзя.
    let (status, _) = app.play(&id, &bob, "e8e7").await;
    assert_eq!(status, StatusCode::CONFLICT);
}

#[ignore = "нужна PostgreSQL (см. комментарий в начале файла)"]
#[sqlx::test(migrations = "./migrations")]
async fn move_validation_and_turn_order(pool: PgPool) {
    let app = app(pool);
    let alice = app.player("alice").await;
    let bob = app.player("bob").await;
    let carol = app.player("carol").await;
    let id = app.started_game(&alice, &bob, 300, 0).await;

    // Чужой ход.
    let (status, _) = app.play(&id, &bob, "e7e5").await;
    assert_eq!(status, StatusCode::CONFLICT);
    // Не участник.
    let (status, _) = app.play(&id, &carol, "e2e4").await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    // Незаконный ход и мусор вместо хода.
    let (status, _) = app.play(&id, &alice, "e2e5").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    for junk in ["", "e2", "zz99", "e2e4e5", "ё2ё4"] {
        let (status, _) = app.play(&id, &alice, junk).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{junk}");
    }
    // Несуществующая партия и битый id.
    let (status, _) = app.play(&Uuid::new_v4().to_string(), &alice, "e2e4").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = app.get("/games/not-a-uuid", Some(&alice)).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Ни один неудачный ход ничего не записал.
    assert_eq!(app.game(&id, &alice).await["ply"], 0);

    let (status, body) = app.play(&id, &alice, "e2e4").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["turn"], "black");
    assert_eq!(
        body["fen"],
        "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq e3 0 1"
    );
}

#[ignore = "нужна PostgreSQL (см. комментарий в начале файла)"]
#[sqlx::test(migrations = "./migrations")]
async fn clocks_start_after_first_two_moves_and_add_increment(pool: PgPool) {
    let app = app(pool);
    let alice = app.player("alice").await;
    let bob = app.player("bob").await;
    let id = app.started_game(&alice, &bob, 300, 5).await;

    // До двух первых ходов часы стоят.
    app.play_all(&id, &alice, &bob, &["e2e4"]).await;
    let game = app.game(&id, &alice).await;
    assert_eq!(game["clock"]["running"], Value::Null);
    assert_eq!(game["clock"]["whiteMs"], 300_000);

    app.play_all(&id, &bob, &alice, &["e7e5"]).await;
    let game = app.game(&id, &alice).await;
    assert_eq!(game["clock"]["running"], "white");
    assert_eq!(game["clock"]["blackMs"], 300_000);

    // Ход при идущих часах: тратится время, добавляется 5 секунд.
    let (status, body) = app.play(&id, &alice, "g1f3").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let white_ms = body["clock"]["whiteMs"].as_i64().unwrap();
    assert!((300_000..=305_000).contains(&white_ms), "{white_ms}");
    assert_eq!(body["clock"]["running"], "black");
}

#[ignore = "нужна PostgreSQL (см. комментарий в начале файла)"]
#[sqlx::test(migrations = "./migrations")]
async fn flag_is_applied_when_game_is_read(pool: PgPool) {
    let app = app(pool);
    let alice = app.player("alice").await;
    let bob = app.player("bob").await;
    let id = app.started_game(&alice, &bob, 60, 0).await;
    app.play_all(&id, &alice, &bob, &["e2e4", "e7e5"]).await;

    // Белые "думают" две минуты при минуте на партию.
    app.age_clock(&id, 120).await;

    let game = app.game(&id, &bob).await;
    assert_eq!(game["status"], "finished");
    assert_eq!(game["result"], "black");
    assert_eq!(game["termination"], "timeout");
    assert_eq!(game["clock"]["whiteMs"], 0);
    assert_eq!(game["clock"]["running"], Value::Null);
}

#[ignore = "нужна PostgreSQL (см. комментарий в начале файла)"]
#[sqlx::test(migrations = "./migrations")]
async fn move_after_flag_is_rejected_and_result_is_saved(pool: PgPool) {
    let app = app(pool);
    let alice = app.player("alice").await;
    let bob = app.player("bob").await;
    let id = app.started_game(&alice, &bob, 60, 0).await;
    app.play_all(&id, &alice, &bob, &["e2e4", "e7e5"]).await;
    app.age_clock(&id, 120).await;

    let (status, body) = app.play(&id, &alice, "g1f3").await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");

    // Транзакция с ошибкой не откатила результат: время вышло окончательно.
    let game = app.game(&id, &alice).await;
    assert_eq!(game["status"], "finished");
    assert_eq!(game["termination"], "timeout");
    assert_eq!(game["ply"], 2);
}

#[ignore = "нужна PostgreSQL (см. комментарий в начале файла)"]
#[sqlx::test(migrations = "./migrations")]
async fn background_sweep_flags_games_nobody_reads(pool: PgPool) {
    let app = app(pool);
    let alice = app.player("alice").await;
    let bob = app.player("bob").await;
    let id = app.started_game(&alice, &bob, 60, 0).await;
    app.play_all(&id, &alice, &bob, &["e2e4", "e7e5", "g1f3"])
        .await;
    app.age_clock(&id, 120).await;

    rtchesz_api::games::service::sweep(&app.state).await;

    let (status, finished): (String, Option<String>) =
        sqlx::query_as("SELECT status, result FROM games WHERE id = $1::uuid")
            .bind(&id)
            .fetch_one(&app.state.pool)
            .await
            .unwrap();
    assert_eq!(status, "finished");
    // Ход был за чёрными - они и просрочили.
    assert_eq!(finished.as_deref(), Some("white"));
}

#[ignore = "нужна PostgreSQL (см. комментарий в начале файла)"]
#[sqlx::test(migrations = "./migrations")]
async fn missing_first_move_aborts_the_game(pool: PgPool) {
    let app = app(pool);
    let alice = app.player("alice").await;
    let bob = app.player("bob").await;
    let id = app.started_game(&alice, &bob, 300, 0).await;

    // У партии без ходов `turn_started_at` пуст: двигаем время старта.
    sqlx::query(
        "UPDATE games SET last_move_at = last_move_at - INTERVAL '10 minutes' WHERE id = $1::uuid",
    )
    .bind(&id)
    .execute(&app.state.pool)
    .await
    .unwrap();

    let game = app.game(&id, &alice).await;
    assert_eq!(game["status"], "aborted");
    assert_eq!(game["result"], Value::Null);
    assert_eq!(game["termination"], "aborted");
}

#[ignore = "нужна PostgreSQL (см. комментарий в начале файла)"]
#[sqlx::test(migrations = "./migrations")]
async fn resign_ends_the_game_for_participants_only(pool: PgPool) {
    let app = app(pool);
    let alice = app.player("alice").await;
    let bob = app.player("bob").await;
    let carol = app.player("carol").await;
    let id = app.started_game(&alice, &bob, 300, 0).await;

    let (status, _) = app
        .post_auth(&format!("/games/{id}/resign"), json!({}), &carol)
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // Сдаться можно и не в свой ход.
    let (status, body) = app
        .post_auth(&format!("/games/{id}/resign"), json!({}), &bob)
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["status"], "finished");
    assert_eq!(body["result"], "white");
    assert_eq!(body["termination"], "resignation");

    let (status, _) = app
        .post_auth(&format!("/games/{id}/resign"), json!({}), &alice)
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
}

#[ignore = "нужна PostgreSQL (см. комментарий в начале файла)"]
#[sqlx::test(migrations = "./migrations")]
async fn draw_offer_accept_decline_and_cleared_by_move(pool: PgPool) {
    let app = app(pool);
    let alice = app.player("alice").await;
    let bob = app.player("bob").await;
    let id = app.started_game(&alice, &bob, 300, 0).await;
    let draw = format!("/games/{id}/draw");

    // В самом начале ничью предлагать рано.
    let (status, _) = app.post_auth(&draw, json!({}), &alice).await;
    assert_eq!(status, StatusCode::CONFLICT);

    app.play_all(&id, &alice, &bob, &["e2e4", "e7e5"]).await;

    let (status, body) = app.post_auth(&draw, json!({}), &alice).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["drawOfferedBy"], "white");

    // Повторно тот же игрок предложить не может, а отклонить может только... любой участник.
    let (status, _) = app.post_auth(&draw, json!({}), &alice).await;
    assert_eq!(status, StatusCode::CONFLICT);

    let (status, body) = app
        .post_auth(&format!("{draw}/decline"), json!({}), &bob)
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["drawOfferedBy"], Value::Null);
    let (status, _) = app
        .post_auth(&format!("{draw}/decline"), json!({}), &bob)
        .await;
    assert_eq!(status, StatusCode::CONFLICT);

    // Ход снимает предложение.
    app.post_auth(&draw, json!({}), &bob).await;
    let (_, body) = app.play(&id, &alice, "g1f3").await;
    assert_eq!(body["drawOfferedBy"], Value::Null);

    // Согласие: соперник предлагает в ответ - партия вничью.
    app.post_auth(&draw, json!({}), &alice).await;
    let (status, body) = app.post_auth(&draw, json!({}), &bob).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["status"], "finished");
    assert_eq!(body["result"], "draw");
    assert_eq!(body["termination"], "draw_agreement");
}

#[ignore = "нужна PostgreSQL (см. комментарий в начале файла)"]
#[sqlx::test(migrations = "./migrations")]
async fn abort_rules(pool: PgPool) {
    let app = app(pool);
    let alice = app.player("alice").await;
    let bob = app.player("bob").await;

    // Свой вызов можно отменить; он удаляется.
    let (_, created) = app
        .post_auth(
            "/games",
            json!({ "timeControl": { "initialSecs": 300 } }),
            &alice,
        )
        .await;
    let waiting = created["id"].as_str().unwrap();
    let (status, _) = app
        .post_auth(&format!("/games/{waiting}/abort"), json!({}), &bob)
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = app
        .post_auth(&format!("/games/{waiting}/abort"), json!({}), &alice)
        .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = app.get(&format!("/games/{waiting}"), Some(&alice)).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Идущую партию без ходов можно отменить, после двух ходов - уже нет.
    let first = app.started_game(&alice, &bob, 300, 0).await;
    let (status, body) = app
        .post_auth(&format!("/games/{first}/abort"), json!({}), &bob)
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["status"], "aborted");

    let second = app.started_game(&alice, &bob, 300, 0).await;
    app.play_all(&second, &alice, &bob, &["e2e4", "e7e5"]).await;
    let (status, _) = app
        .post_auth(&format!("/games/{second}/abort"), json!({}), &alice)
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
}

#[ignore = "нужна PostgreSQL (см. комментарий в начале файла)"]
#[sqlx::test(migrations = "./migrations")]
async fn concurrent_moves_do_not_corrupt_the_game(pool: PgPool) {
    let app = app(pool);
    let alice = app.player("alice").await;
    let bob = app.player("bob").await;
    let id = app.started_game(&alice, &bob, 300, 0).await;

    // Один и тот же ход одновременно: засчитан ровно один.
    let (first, second) =
        tokio::join!(app.play(&id, &alice, "e2e4"), app.play(&id, &alice, "e2e4"));
    let mut statuses = [first.0, second.0];
    statuses.sort();
    assert_eq!(statuses, [StatusCode::OK, StatusCode::CONFLICT]);
    assert_eq!(app.game(&id, &alice).await["ply"], 1);
}
