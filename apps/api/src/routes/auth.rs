use std::time::Duration;

use axum::{Json, extract::State, http::StatusCode};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    auth::{AuthUser, password, reset},
    error::AppError,
    extract::{ApiJson, ClientIp},
    mail,
    repo::{password_resets, users},
    state::AppState,
    validation::{
        PASSWORD_MAX, normalize_email, validate_email, validate_password, validate_username,
    },
};

// ---------- Лимиты запросов ----------

const MINUTE: Duration = Duration::from_secs(60);
const QUARTER_HOUR: Duration = Duration::from_secs(15 * 60);
const HOUR: Duration = Duration::from_secs(60 * 60);

const REGISTER_PER_IP: (u32, Duration) = (10, HOUR);
const LOGIN_PER_IP: (u32, Duration) = (20, MINUTE);
/// Неудачные входы на один email. Успешный вход сбрасывает счётчик.
const LOGIN_FAILS_PER_EMAIL: (u32, Duration) = (10, QUARTER_HOUR);
const FORGOT_PER_IP: (u32, Duration) = (10, QUARTER_HOUR);
const VERIFY_PER_IP: (u32, Duration) = (20, QUARTER_HOUR);
const RESET_PER_IP: (u32, Duration) = (10, QUARTER_HOUR);

fn limit(state: &AppState, key: String, (max, window): (u32, Duration)) -> Result<(), AppError> {
    state
        .limiter
        .hit(&key, max, window)
        .map_err(AppError::TooManyRequests)
}

// ---------- Запросы и ответы ----------

#[derive(Debug, Deserialize)]
pub struct RegisterRequest {
    username: String,
    email: String,
    password: String,
}

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    email: String,
    password: String,
}

#[derive(Debug, Deserialize)]
pub struct ForgotPasswordRequest {
    email: String,
}

#[derive(Debug, Deserialize)]
pub struct VerifyCodeRequest {
    email: String,
    code: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResetPasswordRequest {
    reset_token: String,
    password: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserResponse {
    id: Uuid,
    username: String,
    email: String,
    created_at: DateTime<Utc>,
}

impl From<users::User> for UserResponse {
    fn from(user: users::User) -> Self {
        Self {
            id: user.id,
            username: user.username,
            email: user.email,
            created_at: user.created_at,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct AuthResponse {
    token: String,
    user: UserResponse,
}

#[derive(Debug, Serialize)]
pub struct MessageResponse {
    message: &'static str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifyCodeResponse {
    reset_token: String,
}

// ---------- Обработчики ----------

/// POST /auth/register
pub async fn register(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    ApiJson(body): ApiJson<RegisterRequest>,
) -> Result<(StatusCode, Json<AuthResponse>), AppError> {
    limit(&state, format!("register:ip:{ip}"), REGISTER_PER_IP)?;

    let username = body.username.trim().to_string();
    let email = normalize_email(&body.email);

    validate_username(&username)?;
    validate_email(&email)?;
    validate_password(&body.password)?;

    let password_hash = password::hash_async(body.password).await?;
    let user = users::create(&state.pool, &email, &username, &password_hash).await?;
    let token = state.jwt.issue(user.id)?;

    Ok((
        StatusCode::CREATED,
        Json(AuthResponse {
            token,
            user: user.into(),
        }),
    ))
}

/// POST /auth/login
pub async fn login(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    ApiJson(body): ApiJson<LoginRequest>,
) -> Result<Json<AuthResponse>, AppError> {
    limit(&state, format!("login:ip:{ip}"), LOGIN_PER_IP)?;

    let invalid = || AppError::unauthorized("Неверный email или пароль");

    let email = normalize_email(&body.email);
    if email.len() > 254 || body.password.chars().count() > PASSWORD_MAX {
        return Err(invalid());
    }

    let fails_key = format!("login:fails:{email}");
    state
        .limiter
        .check(&fails_key, LOGIN_FAILS_PER_EMAIL.0)
        .map_err(AppError::TooManyRequests)?;

    let user = users::find_by_email(&state.pool, &email).await?;

    // Для несуществующего email проверяем пароль по хэшу-пустышке: время ответа
    // одинаковое, и по нему нельзя узнать, зарегистрирован ли адрес.
    let hash = match &user {
        Some(user) => user.password_hash.clone(),
        None => state.dummy_hash.as_ref().clone(),
    };
    let password_ok = password::verify_async(body.password, hash).await;

    match user {
        Some(user) if password_ok => {
            state.limiter.reset(&fails_key);
            let token = state.jwt.issue(user.id)?;

            Ok(Json(AuthResponse {
                token,
                user: user.into(),
            }))
        }
        _ => {
            // Ошибку лимита здесь игнорируем: сама попытка уже отклонена.
            let _ = state
                .limiter
                .hit(&fails_key, LOGIN_FAILS_PER_EMAIL.0, LOGIN_FAILS_PER_EMAIL.1);
            Err(invalid())
        }
    }
}

/// POST /auth/forgot-password
///
/// Всегда отвечает одинаково, существует email или нет (не раскрываем список аккаунтов).
/// Повторный запрос на тот же email раньше чем через cooldown получает 429 + `Retry-After`.
pub async fn forgot_password(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    ApiJson(body): ApiJson<ForgotPasswordRequest>,
) -> Result<Json<MessageResponse>, AppError> {
    limit(&state, format!("forgot:ip:{ip}"), FORGOT_PER_IP)?;

    let email = normalize_email(&body.email);
    validate_email(&email)?;

    limit(
        &state,
        format!("forgot:email:{email}"),
        (1, state.config.reset_resend_cooldown),
    )?;

    if let Some(user) = users::find_by_email(&state.pool, &email).await? {
        let code = reset::generate_code()?;
        let code_hash = reset::hash_code(&state.config.jwt_secret, user.id, &code);

        password_resets::create(
            &state.pool,
            user.id,
            &code_hash,
            state.config.reset_code_ttl,
        )
        .await?;

        // Письмо уходит в фоне: ответ не ждёт SMTP, и по времени не видно, есть ли аккаунт.
        let ttl_minutes = state.config.reset_code_ttl.as_secs() / 60;
        let email_message = mail::reset_code_email(&user.email, &code, ttl_minutes);
        let mailer = state.mailer.clone();

        tokio::spawn(async move {
            if let Err(error) = mailer.send(email_message).await {
                tracing::error!(%error, "failed to send password reset email");
            }
        });
    }

    Ok(Json(MessageResponse {
        message: "Если аккаунт с таким email существует, мы отправили на него код",
    }))
}

/// POST /auth/verify-code
///
/// Проверяет 6-значный код. После верного кода возвращает одноразовый `resetToken`
/// для `POST /auth/reset-password`. На код даётся ограниченное число попыток.
pub async fn verify_code(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    ApiJson(body): ApiJson<VerifyCodeRequest>,
) -> Result<Json<VerifyCodeResponse>, AppError> {
    limit(&state, format!("verify:ip:{ip}"), VERIFY_PER_IP)?;

    let code = body.code.trim();
    if code.len() != reset::CODE_LEN || !code.chars().all(|c| c.is_ascii_digit()) {
        return Err(AppError::bad_request("Введите 6 цифр из письма"));
    }

    let invalid = || AppError::bad_request("Неверный или просроченный код");

    let email = normalize_email(&body.email);
    let user = users::find_by_email(&state.pool, &email)
        .await?
        .ok_or_else(invalid)?;

    let attempt =
        password_resets::register_attempt(&state.pool, user.id, state.config.reset_max_attempts)
            .await?
            .ok_or_else(invalid)?;

    if !reset::verify_code(&state.config.jwt_secret, user.id, code, &attempt.code_hash) {
        return Err(invalid());
    }

    let token = reset::generate_token()?;
    let issued = password_resets::issue_token(
        &state.pool,
        attempt.id,
        &reset::hash_token(&token),
        state.config.reset_token_ttl,
    )
    .await?;

    if !issued {
        return Err(invalid());
    }

    Ok(Json(VerifyCodeResponse { reset_token: token }))
}

/// POST /auth/reset-password
///
/// Меняет пароль по `resetToken` из `verify-code`. Токен одноразовый.
pub async fn reset_password(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    ApiJson(body): ApiJson<ResetPasswordRequest>,
) -> Result<Json<MessageResponse>, AppError> {
    limit(&state, format!("reset:ip:{ip}"), RESET_PER_IP)?;

    validate_password(&body.password)?;

    let mut tx = state.pool.begin().await?;

    // Сначала гасим токен (дёшево), и только для валидного токена считаем тяжёлый хэш:
    // так мусорными токенами нельзя нагрузить процессор.
    let user_id = password_resets::consume_token(&mut *tx, &reset::hash_token(&body.reset_token))
        .await?
        .ok_or_else(|| {
            AppError::bad_request("Ссылка для смены пароля недействительна или устарела")
        })?;

    let password_hash = password::hash_async(body.password).await?;

    users::set_password(&mut *tx, user_id, &password_hash).await?;
    password_resets::invalidate_all_for_user(&mut *tx, user_id).await?;

    tx.commit().await?;

    Ok(Json(MessageResponse {
        message: "Пароль изменён",
    }))
}

/// GET /auth/me (нужен заголовок `Authorization: Bearer <token>`)
pub async fn me(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<UserResponse>, AppError> {
    let user = users::find_by_id(&state.pool, auth.id)
        .await?
        .ok_or_else(|| AppError::unauthorized("Пользователь не найден"))?;

    Ok(Json(user.into()))
}
