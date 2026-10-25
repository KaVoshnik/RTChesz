-- Восстановление пароля в два шага:
--   1) код из письма (хранится только HMAC от него, максимум несколько попыток);
--   2) после верного кода выдаётся одноразовый reset-токен (хранится только его SHA-256).
CREATE TABLE password_resets (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),

    user_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,

    code_hash BYTEA NOT NULL,
    attempts SMALLINT NOT NULL DEFAULT 0,
    expires_at TIMESTAMPTZ NOT NULL,

    reset_token_hash BYTEA,
    reset_token_expires_at TIMESTAMPTZ,

    used_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX password_resets_user_id_idx ON password_resets (user_id);

CREATE UNIQUE INDEX password_resets_token_hash_idx
    ON password_resets (reset_token_hash)
    WHERE reset_token_hash IS NOT NULL;
