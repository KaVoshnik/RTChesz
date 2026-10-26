use chrono::{DateTime, Utc};
use sqlx::PgExecutor;
use uuid::Uuid;

use crate::error::AppError;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct User {
    pub id: Uuid,
    pub email: String,
    pub username: String,
    pub password_hash: String,
    pub token_version: i32,
    pub created_at: DateTime<Utc>,
}

/// Создаёт пользователя. Уникальность проверяет сама БД (нет гонки "проверил - вставил").
pub async fn create(
    executor: impl PgExecutor<'_>,
    email: &str,
    username: &str,
    password_hash: &str,
) -> Result<User, AppError> {
    let result = sqlx::query_as::<_, User>(
        "INSERT INTO users (email, username, password_hash) VALUES ($1, $2, $3) \
         RETURNING id, email, username, password_hash, token_version, created_at",
    )
    .bind(email)
    .bind(username)
    .bind(password_hash)
    .fetch_one(executor)
    .await;

    match result {
        Ok(user) => Ok(user),
        Err(sqlx::Error::Database(db_error)) if db_error.is_unique_violation() => {
            let message = match db_error.constraint() {
                Some("users_email_unique") => "Этот email уже зарегистрирован",
                Some("users_username_unique" | "users_username_lower_unique") => {
                    "Это имя пользователя уже занято"
                }
                _ => "Пользователь с такими данными уже существует",
            };
            Err(AppError::Conflict(message.to_string()))
        }
        Err(error) => Err(error.into()),
    }
}

pub async fn find_by_email(
    executor: impl PgExecutor<'_>,
    email: &str,
) -> Result<Option<User>, sqlx::Error> {
    sqlx::query_as::<_, User>(
        "SELECT id, email, username, password_hash, token_version, created_at FROM users WHERE email = $1",
    )
    .bind(email)
    .fetch_optional(executor)
    .await
}

pub async fn find_by_id(
    executor: impl PgExecutor<'_>,
    id: Uuid,
) -> Result<Option<User>, sqlx::Error> {
    sqlx::query_as::<_, User>(
        "SELECT id, email, username, password_hash, token_version, created_at FROM users WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(executor)
    .await
}

pub async fn set_password(
    executor: impl PgExecutor<'_>,
    id: Uuid,
    password_hash: &str,
) -> Result<(), sqlx::Error> {
    // Версия токенов растёт вместе с паролем: все выданные ранее JWT перестают работать.
    sqlx::query(
        "UPDATE users SET password_hash = $2, token_version = token_version + 1 WHERE id = $1",
    )
    .bind(id)
    .bind(password_hash)
    .execute(executor)
    .await?;

    Ok(())
}

/// Текущая версия токенов пользователя. `None` - пользователя нет (удалён).
pub async fn token_version(
    executor: impl PgExecutor<'_>,
    id: Uuid,
) -> Result<Option<i32>, sqlx::Error> {
    sqlx::query_scalar::<_, i32>("SELECT token_version FROM users WHERE id = $1")
        .bind(id)
        .fetch_optional(executor)
        .await
}

/// Отзывает все выданные токены пользователя (выход на всех устройствах).
pub async fn revoke_tokens(executor: impl PgExecutor<'_>, id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE users SET token_version = token_version + 1 WHERE id = $1")
        .bind(id)
        .execute(executor)
        .await?;

    Ok(())
}
