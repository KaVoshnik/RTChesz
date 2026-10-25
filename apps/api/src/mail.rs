use std::sync::Mutex;

use lettre::{
    AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor,
    message::{Mailbox, header::ContentType},
    transport::smtp::authentication::Credentials,
};

use crate::config::{SmtpConfig, SmtpSecurity};

#[derive(Debug, Clone)]
pub struct Email {
    pub to: String,
    pub subject: String,
    pub body: String,
}

pub type MailError = Box<dyn std::error::Error + Send + Sync>;

/// Отправка писем.
pub enum Mailer {
    /// Настоящая отправка через SMTP.
    Smtp {
        transport: Box<AsyncSmtpTransport<Tokio1Executor>>,
        from: Mailbox,
    },
    /// SMTP не настроен: письмо целиком пишется в лог. Только для разработки!
    Log,
    /// Складывает письма в память. Нужен тестам, чтобы достать код из "письма".
    Capture(Mutex<Vec<Email>>),
}

impl Mailer {
    pub fn from_config(config: Option<&SmtpConfig>) -> Result<Self, String> {
        let Some(smtp) = config else {
            tracing::warn!(
                "SMTP_HOST is not set: emails are NOT sent, reset codes are printed to the log"
            );
            return Ok(Self::Log);
        };

        let from: Mailbox = smtp
            .from
            .parse()
            .map_err(|error| format!("SMTP_FROM is not a valid mailbox: {error}"))?;

        let builder = match smtp.security {
            SmtpSecurity::None => {
                AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(smtp.host.clone())
            }
            SmtpSecurity::StartTls => {
                AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&smtp.host)
                    .map_err(|error| format!("invalid SMTP settings: {error}"))?
            }
            SmtpSecurity::Tls => AsyncSmtpTransport::<Tokio1Executor>::relay(&smtp.host)
                .map_err(|error| format!("invalid SMTP settings: {error}"))?,
        };

        let mut builder = builder.port(smtp.port);
        if let (Some(user), Some(password)) = (&smtp.username, &smtp.password) {
            builder = builder.credentials(Credentials::new(user.clone(), password.clone()));
        }

        Ok(Self::Smtp {
            transport: Box::new(builder.build()),
            from,
        })
    }

    pub fn capture() -> Self {
        Self::Capture(Mutex::new(Vec::new()))
    }

    /// Письма, перехваченные в режиме `Capture` (в остальных режимах пусто).
    pub fn captured(&self) -> Vec<Email> {
        match self {
            Self::Capture(outbox) => outbox
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .clone(),
            _ => Vec::new(),
        }
    }

    pub async fn send(&self, email: Email) -> Result<(), MailError> {
        match self {
            Self::Smtp { transport, from } => {
                let message = Message::builder()
                    .from(from.clone())
                    .to(email.to.parse::<Mailbox>()?)
                    .subject(email.subject)
                    .header(ContentType::TEXT_PLAIN)
                    .body(email.body)?;

                transport.send(message).await?;
                Ok(())
            }
            Self::Log => {
                tracing::info!(
                    to = %email.to,
                    subject = %email.subject,
                    "[dev mail] \n{}",
                    email.body
                );
                Ok(())
            }
            Self::Capture(outbox) => {
                outbox
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .push(email);
                Ok(())
            }
        }
    }
}

pub fn reset_code_email(to: &str, code: &str, ttl_minutes: u64) -> Email {
    Email {
        to: to.to_string(),
        subject: "RTChesz: код восстановления пароля".to_string(),
        body: format!(
            "Ваш код восстановления пароля: {code}\n\n\
             Код действует {ttl_minutes} мин. Никому его не сообщайте.\n\n\
             Если вы не запрашивали восстановление пароля, просто проигнорируйте это письмо."
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reset_email_contains_code_and_ttl() {
        let email = reset_code_email("user@example.com", "042517", 10);

        assert_eq!(email.to, "user@example.com");
        assert!(email.body.contains("042517"));
        assert!(email.body.contains("10 мин"));
    }

    #[tokio::test]
    async fn capture_mailer_stores_messages() {
        let mailer = Mailer::capture();
        mailer
            .send(reset_code_email("a@b.co", "123456", 10))
            .await
            .unwrap();

        let sent = mailer.captured();
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].to, "a@b.co");
    }

    #[test]
    fn missing_smtp_config_falls_back_to_log_mailer() {
        assert!(matches!(Mailer::from_config(None), Ok(Mailer::Log)));
    }
}
