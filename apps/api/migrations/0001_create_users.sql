CREATE TABLE users (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),

    email TEXT NOT NULL,
    username TEXT NOT NULL,
    password_hash TEXT NOT NULL,

    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    CONSTRAINT users_email_unique UNIQUE (email),
    CONSTRAINT users_username_unique UNIQUE (username),

    CONSTRAINT users_email_length
        CHECK (char_length(email) BETWEEN 3 AND 254),

    CONSTRAINT users_username_length
        CHECK (char_length(username) BETWEEN 3 AND 24)
);