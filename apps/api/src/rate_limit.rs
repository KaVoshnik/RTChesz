use std::{
    collections::HashMap,
    sync::Mutex,
    time::{Duration, Instant},
};

struct Bucket {
    count: u32,
    window: Duration,
    started: Instant,
}

impl Bucket {
    fn expired(&self, now: Instant) -> bool {
        now.duration_since(self.started) >= self.window
    }

    fn retry_after(&self, now: Instant) -> u64 {
        let left = self.window.saturating_sub(now.duration_since(self.started));
        // Округляем вверх, чтобы клиент не пришёл на долю секунды раньше времени.
        left.as_secs() + u64::from(left.subsec_nanos() > 0)
    }
}

/// Простой лимитер с фиксированным окном в памяти процесса.
///
/// Ключ - произвольная строка (`"login:ip:1.2.3.4"`). Состояние не переживает
/// перезапуск и не делится между несколькими экземплярами API: для одного
/// сервера этого достаточно, при масштабировании нужно переехать на Redis.
#[derive(Default)]
pub struct RateLimiter {
    buckets: Mutex<HashMap<String, Bucket>>,
}

impl RateLimiter {
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Bucket>> {
        // Отравленный мьютекс не повод ронять сервер: данные лимитера не критичны.
        self.buckets
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Засчитывает одну попытку. `Err(секунды)` - лимит исчерпан.
    pub fn hit(&self, key: &str, limit: u32, window: Duration) -> Result<(), u64> {
        let now = Instant::now();
        let mut buckets = self.lock();

        let bucket = buckets.entry(key.to_string()).or_insert(Bucket {
            count: 0,
            window,
            started: now,
        });

        if bucket.expired(now) {
            *bucket = Bucket {
                count: 0,
                window,
                started: now,
            };
        }

        if bucket.count >= limit {
            return Err(bucket.retry_after(now).max(1));
        }

        bucket.count += 1;
        Ok(())
    }

    /// Проверяет, исчерпан ли лимит, не засчитывая попытку.
    pub fn check(&self, key: &str, limit: u32) -> Result<(), u64> {
        let now = Instant::now();
        let buckets = self.lock();

        match buckets.get(key) {
            Some(bucket) if !bucket.expired(now) && bucket.count >= limit => {
                Err(bucket.retry_after(now).max(1))
            }
            _ => Ok(()),
        }
    }

    pub fn reset(&self, key: &str) {
        self.lock().remove(key);
    }

    /// Удаляет истёкшие записи, чтобы память не росла бесконечно.
    pub fn purge_expired(&self) {
        let now = Instant::now();
        self.lock().retain(|_, bucket| !bucket.expired(now));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_after_limit_and_reports_retry_after() {
        let limiter = RateLimiter::new();
        let window = Duration::from_secs(60);

        assert!(limiter.hit("k", 2, window).is_ok());
        assert!(limiter.hit("k", 2, window).is_ok());

        let retry = limiter.hit("k", 2, window).unwrap_err();
        assert!((1..=60).contains(&retry));
    }

    #[test]
    fn keys_are_independent() {
        let limiter = RateLimiter::new();
        let window = Duration::from_secs(60);

        assert!(limiter.hit("a", 1, window).is_ok());
        assert!(limiter.hit("a", 1, window).is_err());
        assert!(limiter.hit("b", 1, window).is_ok());
    }

    #[test]
    fn window_expires() {
        let limiter = RateLimiter::new();
        let window = Duration::from_millis(30);

        assert!(limiter.hit("k", 1, window).is_ok());
        assert!(limiter.hit("k", 1, window).is_err());

        std::thread::sleep(Duration::from_millis(50));
        assert!(limiter.hit("k", 1, window).is_ok());
    }

    #[test]
    fn check_does_not_count_and_reset_clears() {
        let limiter = RateLimiter::new();
        let window = Duration::from_secs(60);

        assert!(limiter.check("k", 1).is_ok());
        assert!(limiter.check("k", 1).is_ok());

        limiter.hit("k", 1, window).unwrap();
        assert!(limiter.check("k", 1).is_err());

        limiter.reset("k");
        assert!(limiter.check("k", 1).is_ok());
    }

    #[test]
    fn purge_removes_expired_entries() {
        let limiter = RateLimiter::new();
        limiter.hit("k", 1, Duration::from_millis(10)).unwrap();
        std::thread::sleep(Duration::from_millis(30));

        limiter.purge_expired();
        assert!(limiter.lock().is_empty());
    }
}
