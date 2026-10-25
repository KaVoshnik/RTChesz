import { useEffect, useRef, useState, type FormEvent } from 'react';
import './App.css';
import {
  ApiError,
  register,
  login,
  sendResetCode,
  verifyResetCode,
  saveToken,
  getErrorMessage,
} from './api';

// Все "экраны" сайта
type Screen =
  | 'landing'       // лендинг
  | 'login'         // вход
  | 'register'      // регистрация
  | 'forgot-email'  // восстановление: ввод почты
  | 'forgot-code'   // восстановление: ввод кода
  | 'success';      // экран "вы вошли"

const MIN_PASSWORD_LENGTH = 8; // минимальная длина пароля при регистрации
const RESEND_COOLDOWN = 60;    // пауза перед повторной отправкой кода, сек

function App() {
  const bgRef = useRef<HTMLDivElement>(null);

  // Какой экран сейчас показан. Сначала лендинг.
  const [screen, setScreen] = useState<Screen>('landing');

  // Поля формы
  const [username, setUsername] = useState('');
  const [email, setEmail] = useState('');
  const [password, setPassword] = useState('');
  const [password2, setPassword2] = useState('');
  const [code, setCode] = useState('');

  // Сообщения
  const [error, setError] = useState('');
  const [info, setInfo] = useState('');

  // true, пока идёт запрос к серверу
  const [loading, setLoading] = useState(false);

  // Сколько секунд осталось до повторной отправки кода
  const [cooldown, setCooldown] = useState(0);

  // ---------- Таймер повторной отправки ----------
  // Каждую секунду уменьшаем счётчик, пока не дойдёт до 0
  useEffect(() => {
    if (cooldown <= 0) return;
    const id = setTimeout(() => setCooldown((c) => c - 1), 1000);
    return () => clearTimeout(id);
  }, [cooldown]);

  // ---------- Эффект плавающего фона ----------
  useEffect(() => {
    // Если в системе включено "уменьшить движение", анимацию не запускаем
    if (window.matchMedia('(prefers-reduced-motion: reduce)').matches) return;

    const MAX_SHIFT = 25;
    const EASE = 0.08;
    const SCALE = 1.15; // должен совпадать со scale в App.css

    const target = { x: 0, y: 0 };  // куда должна прийти картинка
    const current = { x: 0, y: 0 }; // где она сейчас
    let rafId = 0;

    // Вызывается перед каждым кадром: приближаем картинку к цели
    const animate = () => {
      current.x += (target.x - current.x) * EASE;
      current.y += (target.y - current.y) * EASE;

      if (bgRef.current) {
        bgRef.current.style.transform =
          `translate3d(${current.x}px, ${current.y}px, 0) scale(${SCALE})`;
      }

      // Останавливаем цикл, когда почти дошли до цели
      const done =
        Math.abs(target.x - current.x) < 0.01 &&
        Math.abs(target.y - current.y) < 0.01;

      rafId = done ? 0 : requestAnimationFrame(animate);
    };

    // При движении мыши только считаем новую цель
    const handleMouseMove = (e: MouseEvent) => {
      target.x = (e.clientX / window.innerWidth - 0.5) * 2 * -MAX_SHIFT;
      target.y = (e.clientY / window.innerHeight - 0.5) * 2 * -MAX_SHIFT;
      if (!rafId) rafId = requestAnimationFrame(animate);
    };

    window.addEventListener('mousemove', handleMouseMove, { passive: true });
    return () => {
      window.removeEventListener('mousemove', handleMouseMove);
      cancelAnimationFrame(rafId);
    };
  }, []);

  // ---------- Обработчики ----------

  // Переход на другой экран (заодно очищаем сообщения и пароли)
  const goTo = (next: Screen) => {
    setScreen(next);
    setError('');
    setInfo('');
    setPassword('');
    setPassword2('');
    setCode('');
  };

  // Вход / регистрация
  const handleAuthSubmit = async (e: FormEvent) => {
    e.preventDefault();
    if (loading) return; // защита от двойной отправки

    if (screen === 'register') {
      if (password.length < MIN_PASSWORD_LENGTH) {
        setError(`Пароль должен быть не короче ${MIN_PASSWORD_LENGTH} символов`);
        return;
      }
      if (password !== password2) {
        setError('Пароли не совпадают');
        return;
      }
    }

    setError('');
    setLoading(true);

    try {
      // Отправляем данные на сервер и ждём ответ
      const result =
        screen === 'login'
          ? await login({ email, password })
          : await register({ username: username.trim(), email, password });

      // Сохраняем токен, который выдал сервер
      saveToken(result.token);
      goTo('success');
    } catch (err) {
      setError(getErrorMessage(err));
    } finally {
      // выполняется в любом случае: и при успехе, и при ошибке
      setLoading(false);
    }
  };

  // Восстановление, шаг 1: попросить сервер отправить код на почту
  const handleSendCode = async (e: FormEvent) => {
    e.preventDefault();
    if (loading) return;

    setError('');
    setInfo('');
    setLoading(true);

    try {
      await sendResetCode(email);
      setCooldown(RESEND_COOLDOWN);
      setScreen('forgot-code');
    } catch (err) {
      if (err instanceof ApiError && err.retryAfter) {
        setCooldown(err.retryAfter); // сервер сам сказал, сколько ждать
      }
      setError(getErrorMessage(err));
    } finally {
      setLoading(false);
    }
  };

  // Восстановление, шаг 2: отправить введённый код на проверку
  const handleCheckCode = async (e: FormEvent) => {
    e.preventDefault();
    if (loading) return;

    if (code.length !== 6) {
      setError('Введите 6 цифр из письма');
      return;
    }

    setError('');
    setInfo('');
    setLoading(true);

    try {
      await verifyResetCode(email, code);
      // TODO: следующий шаг: экран "Новый пароль"
      setInfo('Код подтверждён');
    } catch (err) {
      setError(getErrorMessage(err));
    } finally {
      setLoading(false);
    }
  };

  // Отправить код ещё раз
  const handleResend = async () => {
    if (loading || cooldown > 0) return;

    setError('');
    setInfo('');
    setLoading(true);

    try {
      await sendResetCode(email);
      setCooldown(RESEND_COOLDOWN);
      setInfo('Код отправлен повторно');
    } catch (err) {
      if (err instanceof ApiError && err.retryAfter) {
        setCooldown(err.retryAfter);
      }
      setError(getErrorMessage(err));
    } finally {
      setLoading(false);
    }
  };

  // Вход через Google (пока заглушка: нужно согласовать с бэкендером)
  const handleGoogle = () => {
    console.log('Google sign-in');
  };

  const isLogin = screen === 'login';

  return (
    <div className="app-wrapper">
      {/* Фон: картинка + затемнение */}
      <div className="background-layer" ref={bgRef}></div>
      <div className="background-overlay" aria-hidden="true"></div>

      {/* Шапка сайта */}
      <header className="header">
        <button
          type="button"
          className="header-logo"
          onClick={() => goTo('landing')}
        >
          RTChesz
        </button>

        {/* Кнопка "Войти" видна только на лендинге */}
        {screen === 'landing' && (
          <button
            type="button"
            className="header-button"
            onClick={() => goTo('login')}
          >
            Войти
          </button>
        )}
      </header>

      {/* ---------- Лендинг ---------- */}
      {screen === 'landing' && (
        <main className="landing">
          <h1 className="landing-title">
            Здесь скоро появится лендинг страница
          </h1>
        </main>
      )}

      {/* ---------- Окно авторизации ---------- */}
      {screen !== 'landing' && (
        // key заставляет React пересоздать окно, и анимация появления срабатывает при каждой смене экрана
        <div className="auth-container" key={screen}>
          {/* ===== Вход и регистрация ===== */}
          {(screen === 'login' || screen === 'register') && (
            <>
              <h2 className="auth-title">
                {isLogin ? 'Вход' : 'Регистрация'}
              </h2>
              <p className="auth-subtitle">
                {isLogin
                  ? 'Рады видеть вас снова'
                  : 'Создайте аккаунт, чтобы начать играть'}
              </p>

              <button
                type="button"
                className="google-button"
                onClick={handleGoogle}
              >
                <svg width="20" height="20" viewBox="0 0 48 48" aria-hidden="true">
                  <path fill="#EA4335" d="M24 9.5c3.54 0 6.71 1.22 9.21 3.6l6.85-6.85C35.9 2.38 30.47 0 24 0 14.62 0 6.51 5.38 2.56 13.22l7.98 6.19C12.43 13.72 17.74 9.5 24 9.5z" />
                  <path fill="#4285F4" d="M46.98 24.55c0-1.57-.15-3.09-.38-4.55H24v9.02h12.94c-.58 2.96-2.26 5.48-4.78 7.18l7.73 6c4.51-4.18 7.09-10.36 7.09-17.65z" />
                  <path fill="#FBBC05" d="M10.53 28.59c-.48-1.45-.76-2.99-.76-4.59s.27-3.14.76-4.59l-7.98-6.19C.92 16.46 0 20.12 0 24c0 3.88.92 7.54 2.56 10.78l7.97-6.19z" />
                  <path fill="#34A853" d="M24 48c6.48 0 11.93-2.13 15.89-5.81l-7.73-6c-2.15 1.45-4.92 2.3-8.16 2.3-6.26 0-11.57-4.22-13.47-9.91l-7.98 6.19C6.51 42.62 14.62 48 24 48z" />
                </svg>
                Войти через Google
              </button>

              <div className="auth-divider">
                <span>или</span>
              </div>

              <form className="auth-form" onSubmit={handleAuthSubmit}>
                {!isLogin && (
                  <input
                    type="text"
                    placeholder="Имя пользователя"
                    className="auth-input"
                    value={username}
                    onChange={(e) => setUsername(e.target.value)}
                    autoComplete="username"
                    maxLength={32}
                    required
                  />
                )}

                <input
                  type="email"
                  placeholder="Email"
                  className="auth-input"
                  value={email}
                  onChange={(e) => setEmail(e.target.value)}
                  autoComplete="email"
                  maxLength={254}
                  required
                />

                <input
                  type="password"
                  placeholder="Пароль"
                  className="auth-input"
                  value={password}
                  onChange={(e) => setPassword(e.target.value)}
                  autoComplete={isLogin ? 'current-password' : 'new-password'}
                  minLength={isLogin ? undefined : MIN_PASSWORD_LENGTH}
                  maxLength={128}
                  required
                />

                {/* Ссылка "Забыли пароль?" только на входе */}
                {isLogin && (
                  <button
                    type="button"
                    className="auth-link forgot-link"
                    onClick={() => goTo('forgot-email')}
                  >
                    Забыли пароль?
                  </button>
                )}

                {!isLogin && (
                  <input
                    type="password"
                    placeholder="Повторите пароль"
                    className="auth-input"
                    value={password2}
                    onChange={(e) => setPassword2(e.target.value)}
                    autoComplete="new-password"
                    maxLength={128}
                    required
                  />
                )}

                {error && (
                  <div className="auth-error" role="alert">
                    {error}
                  </div>
                )}

                <button type="submit" className="auth-button" disabled={loading}>
                  {loading
                    ? 'Подождите…'
                    : isLogin
                      ? 'Войти'
                      : 'Создать аккаунт'}
                </button>
              </form>

              <p className="auth-switch">
                {isLogin ? 'Нет аккаунта?' : 'Уже есть аккаунт?'}
                <button
                  type="button"
                  className="auth-link"
                  onClick={() => goTo(isLogin ? 'register' : 'login')}
                >
                  {isLogin ? 'Зарегистрироваться' : 'Войти'}
                </button>
              </p>
            </>
          )}

          {/* ===== Восстановление пароля: ввод почты ===== */}
          {screen === 'forgot-email' && (
            <>
              <h2 className="auth-title">Восстановление пароля</h2>
              <p className="auth-subtitle">
                Введите почту, и мы отправим на неё код
              </p>

              <form className="auth-form" onSubmit={handleSendCode}>
                <input
                  type="email"
                  placeholder="Email"
                  className="auth-input"
                  value={email}
                  onChange={(e) => setEmail(e.target.value)}
                  autoComplete="email"
                  maxLength={254}
                  required
                />

                {error && (
                  <div className="auth-error" role="alert">
                    {error}
                  </div>
                )}

                <button type="submit" className="auth-button" disabled={loading}>
                  {loading ? 'Отправляем…' : 'Отправить код'}
                </button>
              </form>

              <p className="auth-switch">
                <button
                  type="button"
                  className="auth-link"
                  onClick={() => goTo('login')}
                >
                  ← Назад ко входу
                </button>
              </p>
            </>
          )}

          {/* ===== Восстановление пароля: ввод кода ===== */}
          {screen === 'forgot-code' && (
            <>
              <h2 className="auth-title">Введите код</h2>
              <p className="auth-subtitle">
                Мы отправили 6-значный код на <strong>{email}</strong>
              </p>

              <form className="auth-form" onSubmit={handleCheckCode}>
                <input
                  type="text"
                  inputMode="numeric"
                  autoComplete="one-time-code"
                  maxLength={6}
                  placeholder="000000"
                  className="auth-input code-input"
                  value={code}
                  // оставляем только цифры
                  onChange={(e) => setCode(e.target.value.replace(/\D/g, ''))}
                  required
                />

                {error && (
                  <div className="auth-error" role="alert">
                    {error}
                  </div>
                )}
                {info && (
                  <div className="auth-info" role="status">
                    {info}
                  </div>
                )}

                <button type="submit" className="auth-button" disabled={loading}>
                  {loading ? 'Проверяем…' : 'Подтвердить'}
                </button>
              </form>

              <p className="auth-switch">
                Не пришёл код?
                <button
                  type="button"
                  className="auth-link"
                  onClick={handleResend}
                  disabled={loading || cooldown > 0}
                >
                  {cooldown > 0
                    ? `Повторить через ${cooldown} с`
                    : 'Отправить ещё раз'}
                </button>
              </p>
              <p className="auth-switch">
                <button
                  type="button"
                  className="auth-link"
                  onClick={() => goTo('forgot-email')}
                >
                  ← Изменить почту
                </button>
              </p>
            </>
          )}

          {/* ===== Успешный вход ===== */}
          {screen === 'success' && (
            <>
              <h2 className="auth-title">Готово!</h2>
              <p className="auth-subtitle">Вы успешно вошли в аккаунт</p>
              <div className="auth-form">
                <button
                  type="button"
                  className="auth-button"
                  onClick={() => goTo('landing')}
                >
                  На главную
                </button>
              </div>
            </>
          )}
        </div>
      )}
    </div>
  );
}

export default App;
