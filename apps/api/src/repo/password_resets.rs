use std::time::Duration;

use sqlx::{PgExecutor, PgPool};
use uuid::Uuid;

/// Верная ли попытка, решает вызывающий код; здесь только учёт попыток и сроков.
/// Все сроки считает сама БД (`NOW()`), чтобы не зависеть от часов сервера API.
#[derive(Debug, sqlx::FromRow)]
pub struct CodeAttempt {
    pub id: Uuid,
    pub code_hash: Vec<u8>,
}

/// Создаёт новый код; все предыдущие неиспользованные коды пользователя гасятся.
pub async fn create(
    pool: &PgPool,
    user_id: Uuid,
    code_hash: &[u8],
    ttl: Duration,
) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;

    invalidate_all_for_user(&mut *tx, user_id).await?;

    sqlx::query(
        "INSERT INTO password_resets (user_id, code_hash, expires_at) \
         VALUES ($1, $2, NOW() + make_interval(secs => $3))",
    )
    .bind(user_id)
    .bind(code_hash)
    .bind(ttl.as_secs_f64())
    .execute(&mut *tx)
    .await?;

    tx.commit().await
}

/// Атомарно списывает одну попытку у актуального кода пользователя и возвращает его хэш.
/// `None` - активного кода нет (не запрашивали / истёк / попытки закончились / уже подтверждён).
///
/// Повторная проверка `attempts < max` во внешнем `WHERE` важна: при параллельных
/// запросах Postgres перечитает строку и не даст превысить лимит попыток.
pub async fn register_attempt(
    executor: impl PgExecutor<'_>,
    user_id: Uuid,
    max_attempts: i16,
) -> Result<Option<CodeAttempt>, sqlx::Error> {
    sqlx::query_as::<_, CodeAttempt>(
        "UPDATE password_resets SET attempts = attempts + 1 \
         WHERE id = ( \
             SELECT id FROM password_resets \
             WHERE user_id = $1 \
               AND used_at IS NULL \
               AND reset_token_hash IS NULL \
               AND expires_at > NOW() \
               AND attempts < $2 \
             ORDER BY created_at DESC \
             LIMIT 1 \
         ) AND attempts < $2 \
         RETURNING id, code_hash",
    )
    .bind(user_id)
    .bind(max_attempts)
    .fetch_optional(executor)
    .await
}

/// Привязывает к подтверждённому коду одноразовый reset-токен.
pub async fn issue_token(
    executor: impl PgExecutor<'_>,
    id: Uuid,
    token_hash: &[u8],
    ttl: Duration,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE password_resets \
         SET reset_token_hash = $2, reset_token_expires_at = NOW() + make_interval(secs => $3) \
         WHERE id = $1 AND reset_token_hash IS NULL AND used_at IS NULL",
    )
    .bind(id)
    .bind(token_hash)
    .bind(ttl.as_secs_f64())
    .execute(executor)
    .await?;

    Ok(result.rows_affected() == 1)
}

/// Гасит reset-токен (одноразовый) и возвращает владельца. `None` - токен неверный или истёк.
pub async fn consume_token(
    executor: impl PgExecutor<'_>,
    token_hash: &[u8],
) -> Result<Option<Uuid>, sqlx::Error> {
    sqlx::query_scalar::<_, Uuid>(
        "UPDATE password_resets SET used_at = NOW() \
         WHERE reset_token_hash = $1 \
           AND used_at IS NULL \
           AND reset_token_expires_at > NOW() \
         RETURNING user_id",
    )
    .bind(token_hash)
    .fetch_optional(executor)
    .await
}

pub async fn invalidate_all_for_user(
    executor: impl PgExecutor<'_>,
    user_id: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE password_resets SET used_at = NOW() WHERE user_id = $1 AND used_at IS NULL",
    )
    .bind(user_id)
    .execute(executor)
    .await?;

    Ok(())
}

/// Удаляет записи старше суток (их коды и токены давно истекли).
pub async fn purge_old(executor: impl PgExecutor<'_>) -> Result<u64, sqlx::Error> {
    let result =
        sqlx::query("DELETE FROM password_resets WHERE created_at < NOW() - INTERVAL '1 day'")
            .execute(executor)
            .await?;

    Ok(result.rows_affected())
}
