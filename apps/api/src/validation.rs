use crate::error::AppError;

pub const USERNAME_MIN: usize = 3;
pub const USERNAME_MAX: usize = 24;
pub const PASSWORD_MIN: usize = 8;
pub const PASSWORD_MAX: usize = 128;
const EMAIL_MAX: usize = 254;

/// Email хранится в нижнем регистре и без пробелов по краям.
pub fn normalize_email(raw: &str) -> String {
    raw.trim().to_lowercase()
}

pub fn validate_email(email: &str) -> Result<(), AppError> {
    let invalid = || AppError::bad_request("Введите корректный email");

    if email.len() > EMAIL_MAX || email.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(invalid());
    }

    let (local, domain) = email.split_once('@').ok_or_else(invalid)?;

    if local.is_empty() || local.len() > 64 || domain.contains('@') {
        return Err(invalid());
    }

    let labels: Vec<&str> = domain.split('.').collect();
    let labels_ok = labels.len() >= 2
        && labels
            .iter()
            .all(|l| !l.is_empty() && !l.starts_with('-') && !l.ends_with('-'));

    if !labels_ok {
        return Err(invalid());
    }

    Ok(())
}

pub fn validate_username(username: &str) -> Result<(), AppError> {
    let len = username.chars().count();

    if !(USERNAME_MIN..=USERNAME_MAX).contains(&len) {
        return Err(AppError::bad_request(format!(
            "Имя пользователя должно быть от {USERNAME_MIN} до {USERNAME_MAX} символов"
        )));
    }

    if !username
        .chars()
        .all(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | '.'))
    {
        return Err(AppError::bad_request(
            "Имя пользователя может содержать только буквы, цифры и символы _ - .",
        ));
    }

    Ok(())
}

pub fn validate_password(password: &str) -> Result<(), AppError> {
    let len = password.chars().count();

    if len < PASSWORD_MIN {
        return Err(AppError::bad_request(format!(
            "Пароль должен быть не короче {PASSWORD_MIN} символов"
        )));
    }

    if len > PASSWORD_MAX {
        return Err(AppError::bad_request(format!(
            "Пароль должен быть не длиннее {PASSWORD_MAX} символов"
        )));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_email() {
        assert_eq!(normalize_email("  User@Example.COM "), "user@example.com");
    }

    #[test]
    fn accepts_valid_emails() {
        for email in [
            "a@b.co",
            "john.doe+tag@mail.example.com",
            "x_y@sub-domain.org",
        ] {
            assert!(validate_email(email).is_ok(), "{email}");
        }
    }

    #[test]
    fn rejects_invalid_emails() {
        for email in [
            "",
            "plain",
            "@example.com",
            "user@",
            "user@localhost",
            "user@@example.com",
            "us er@example.com",
            "user@example..com",
            "user@-example.com",
        ] {
            assert!(validate_email(email).is_err(), "{email}");
        }
        assert!(validate_email(&format!("{}@example.com", "a".repeat(250))).is_err());
    }

    #[test]
    fn username_rules() {
        assert!(validate_username("abc").is_ok());
        assert!(validate_username("Иван_99").is_ok());
        assert!(validate_username("ab").is_err());
        assert!(validate_username(&"a".repeat(25)).is_err());
        assert!(validate_username("bad name").is_err());
        assert!(validate_username("<script>").is_err());
    }

    #[test]
    fn password_rules() {
        assert!(validate_password("12345678").is_ok());
        assert!(validate_password("1234567").is_err());
        assert!(validate_password(&"a".repeat(129)).is_err());
        assert!(validate_password(&"я".repeat(128)).is_ok());
    }
}
