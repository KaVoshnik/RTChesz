use std::{env, str::FromStr, time::Duration};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SmtpSecurity {
    /// Подключение без шифрования (только для локального mail-catcher вроде MailHog).
    None,
    /// Обычное подключение + STARTTLS (порт 587).
    StartTls,
    /// Сразу TLS (порт 465).
    Tls,
}

#[derive(Clone)]
pub struct SmtpConfig {
    pub host: String,
    pub port: u16,
    pub username: Option<String>,
    pub password: Option<String>,
    pub from: String,
    pub security: SmtpSecurity,
}

#[derive(Clone)]
pub struct Config {
    pub database_url: String,
    pub host: String,
    pub port: u16,

    /// Секрет для подписи JWT и HMAC кодов восстановления.
    pub jwt_secret: String,
    pub jwt_ttl: Duration,

    /// Разрешённые источники для CORS (адреса фронта).
    pub cors_origins: Vec<String>,
    /// Брать IP клиента из X-Forwarded-For (включать только за доверенным прокси).
    pub trust_proxy: bool,

    pub reset_code_ttl: Duration,
    pub reset_token_ttl: Duration,
    pub reset_resend_cooldown: Duration,
    pub reset_max_attempts: i16,

    /// Если не задан, письма не отправляются, а код пишется в лог (режим разработки).
    pub smtp: Option<SmtpConfig>,
}

const MIN_JWT_SECRET_LEN: usize = 32;

impl Config {
    pub fn from_env() -> Result<Self, String> {
        let database_url =
            env::var("DATABASE_URL").map_err(|_| "DATABASE_URL is not set".to_string())?;

        let host = env::var("HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
        let port = parse_env("PORT", 3000u16)?;

        let jwt_secret = env::var("JWT_SECRET").map_err(|_| "JWT_SECRET is not set".to_string())?;
        if jwt_secret.len() < MIN_JWT_SECRET_LEN {
            return Err(format!(
                "JWT_SECRET must be at least {MIN_JWT_SECRET_LEN} characters long \
                 (generate one: openssl rand -hex 32)"
            ));
        }

        let cors_origins = env::var("CORS_ORIGINS")
            .unwrap_or_else(|_| "http://localhost:5173,http://127.0.0.1:5173".to_string())
            .split(',')
            .map(|origin| origin.trim().trim_end_matches('/').to_string())
            .filter(|origin| !origin.is_empty())
            .collect();

        Ok(Self {
            database_url,
            host,
            port,
            jwt_secret,
            jwt_ttl: Duration::from_secs(parse_env("JWT_TTL_HOURS", 168u64)? * 3600),
            cors_origins,
            trust_proxy: parse_env("TRUST_PROXY", false)?,
            reset_code_ttl: Duration::from_secs(parse_env("RESET_CODE_TTL_MINUTES", 10u64)? * 60),
            reset_token_ttl: Duration::from_secs(parse_env("RESET_TOKEN_TTL_MINUTES", 15u64)? * 60),
            reset_resend_cooldown: Duration::from_secs(parse_env(
                "RESET_RESEND_COOLDOWN_SECS",
                60u64,
            )?),
            reset_max_attempts: parse_env("RESET_MAX_ATTEMPTS", 5i16)?,
            smtp: SmtpConfig::from_env()?,
        })
    }

    /// Конфигурация для интеграционных тестов (без SMTP, быстрые лимиты).
    #[doc(hidden)]
    pub fn for_tests(database_url: String) -> Self {
        Self {
            database_url,
            host: "127.0.0.1".to_string(),
            port: 0,
            jwt_secret: "test_secret_test_secret_test_secret_123".to_string(),
            jwt_ttl: Duration::from_secs(3600),
            cors_origins: vec!["http://localhost:5173".to_string()],
            trust_proxy: false,
            reset_code_ttl: Duration::from_secs(600),
            reset_token_ttl: Duration::from_secs(900),
            reset_resend_cooldown: Duration::from_secs(60),
            reset_max_attempts: 5,
            smtp: None,
        }
    }
}

impl SmtpConfig {
    fn from_env() -> Result<Option<Self>, String> {
        let Some(host) = env::var("SMTP_HOST").ok().filter(|h| !h.trim().is_empty()) else {
            return Ok(None);
        };

        let from = env::var("SMTP_FROM")
            .map_err(|_| "SMTP_FROM is required when SMTP_HOST is set".to_string())?;

        let username = env::var("SMTP_USERNAME").ok().filter(|v| !v.is_empty());
        let password = env::var("SMTP_PASSWORD").ok().filter(|v| !v.is_empty());
        if username.is_some() != password.is_some() {
            return Err("SMTP_USERNAME and SMTP_PASSWORD must be set together".to_string());
        }

        let security = match env::var("SMTP_SECURITY")
            .unwrap_or_else(|_| "starttls".to_string())
            .to_lowercase()
            .as_str()
        {
            "none" => SmtpSecurity::None,
            "starttls" => SmtpSecurity::StartTls,
            "tls" => SmtpSecurity::Tls,
            _ => return Err("SMTP_SECURITY must be one of: none, starttls, tls".to_string()),
        };

        let default_port = match security {
            SmtpSecurity::Tls => 465u16,
            SmtpSecurity::StartTls => 587,
            SmtpSecurity::None => 25,
        };

        Ok(Some(Self {
            host,
            port: parse_env("SMTP_PORT", default_port)?,
            username,
            password,
            from,
            security,
        }))
    }
}

fn parse_env<T: FromStr>(name: &str, default: T) -> Result<T, String> {
    match env::var(name) {
        Ok(value) => value
            .trim()
            .parse::<T>()
            .map_err(|_| format!("{name} has an invalid value: '{value}'")),
        Err(_) => Ok(default),
    }
}
