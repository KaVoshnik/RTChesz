import { useEffect, useRef, useState, type FormEvent } from 'react';
import './App.css';

// Все "экраны" сайта
type Screen =
  | 'landing'       // лендинг
  | 'login'         // вход
  | 'register'      // регистрация
  | 'forgot-email'  // восстановление: ввод почты
  | 'forgot-code';  // восстановление: ввод кода

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

  // ---------- Эффект плавающего фона ----------
  useEffect(() => {
    const MAX_SHIFT = 25;
    const EASE = 0.08;
    const SCALE = 1.15; // должен совпадать со scale в App.css

    const target = { x: 0, y: 0 };
    const current = { x: 0, y: 0 };
    let rafId = 0;

    const animate = () => {
      current.x += (target.x - current.x) * EASE;
      current.y += (target.y - current.y) * EASE;

      if (bgRef.current) {
        bgRef.current.style.transform =
          `translate3d(${current.x}px, ${current.y}px, 0) scale(${SCALE})`;
      }

      const done =
        Math.abs(target.x - current.x) < 0.01 &&
        Math.abs(target.y - current.y) < 0.01;

      rafId = done ? 0 : requestAnimationFrame(animate);
    };

    const handleMouseMove = (e: MouseEvent) => {
      target.x = (e.clientX / window.innerWidth - 0.5) * 2 * -MAX_SHIFT;
      target.y = (e.clientY / window.innerHeight - 0.5) * 2 * -MAX_SHIFT;
      if (!rafId) rafId = requestAnimationFrame(animate);
    };

    window.addEventListener('mousemove', handleMouseMove);
    return () => {
      window.removeEventListener('mousemove', handleMouseMove);
      cancelAnimationFrame(rafId);
    };
  }, []);

  // ---------- Обработчики ----------

  // Переход на другой экран (заодно очищаем ошибки и пароли)
  const goTo = (next: Screen) => {
    setScreen(next);
    setError('');
    setInfo('');
    setPassword('');
    setPassword2('');
    setCode('');
  };

  // Вход / регистрация
  const handleAuthSubmit = (e: FormEvent) => {
    e.preventDefault(); // не перезагружать страницу

    if (screen === 'register' && password !== password2) {
      setError('Пароли не совпадают');
      return;
    }

    setError('');
    // TODO: запрос на сервер
    console.log(screen === 'login' ? 'Вход' : 'Регистрация', {
      username,
      email,
      password,
    });
  };

  // Восстановление, шаг 1: отправить код на почту
  const handleSendCode = (e: FormEvent) => {
    e.preventDefault();
    // TODO: запрос на сервер, чтобы он отправил письмо с кодом
    console.log('Отправляем код на', email);
    setError('');
    setInfo('');
    setScreen('forgot-code');
  };

  // Восстановление, шаг 2: проверить код
  const handleCheckCode = (e: FormEvent) => {
    e.preventDefault();

    if (code.length !== 6) {
      setError('Введите 6 цифр из письма');
      return;
    }

    setError('');
    // TODO: проверить код на сервере, затем показать форму нового пароля
    console.log('Проверяем код', code, 'для', email);
  };

  // Отправить код ещё раз
  const handleResend = () => {
    // TODO: повторный запрос на сервер
    console.log('Отправляем код повторно на', email);
    setError('');
    setInfo('Код отправлен повторно');
  };

  // Вход через Google (заглушка)
  const handleGoogle = () => {
    // TODO: подключить настоящую авторизацию Google
    console.log('Google sign-in');
  };

  const isLogin = screen === 'login';

  return (
    <div className="app-wrapper">
      {/* Фон: картинка + затемнение */}
      <div className="background-layer" ref={bgRef}></div>
      <div className="background-overlay"></div>

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
                    required
                  />
                )}

                <input
                  type="email"
                  placeholder="Email"
                  className="auth-input"
                  value={email}
                  onChange={(e) => setEmail(e.target.value)}
                  required
                />

                <input
                  type="password"
                  placeholder="Пароль"
                  className="auth-input"
                  value={password}
                  onChange={(e) => setPassword(e.target.value)}
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
                    required
                  />
                )}

                {error && <div className="auth-error">{error}</div>}

                <button type="submit" className="auth-button">
                  {isLogin ? 'Войти' : 'Создать аккаунт'}
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
                  required
                />

                <button type="submit" className="auth-button">
                  Отправить код
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

                {error && <div className="auth-error">{error}</div>}
                {info && <div className="auth-info">{info}</div>}

                <button type="submit" className="auth-button">
                  Подтвердить
                </button>
              </form>

              <p className="auth-switch">
                Не пришёл код?
                <button
                  type="button"
                  className="auth-link"
                  onClick={handleResend}
                >
                  Отправить ещё раз
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
        </div>
      )}
    </div>
  );
}

export default App;