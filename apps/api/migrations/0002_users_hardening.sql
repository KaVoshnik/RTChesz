-- Email всегда хранится в нижнем регистре (приложение нормализует его при записи).
-- Сначала приводим к нижнему регистру то, что могло быть вставлено вручную.
UPDATE users SET email = lower(email) WHERE email <> lower(email);

ALTER TABLE users
    ADD CONSTRAINT users_email_lowercase CHECK (email = lower(email));

-- Имя пользователя уникально без учёта регистра: "Alice" и "alice" - один и тот же ник.
CREATE UNIQUE INDEX users_username_lower_unique ON users (lower(username));
