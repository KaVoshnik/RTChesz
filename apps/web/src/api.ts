// Адрес бэкенда берётся из файла .env (переменная должна начинаться с VITE_)
const API_URL: string = import.meta.env.VITE_API_URL ?? 'http://localhost:5173';

// Сколько ждём ответ сервера, прежде чем отменить запрос
const REQUEST_TIMEOUT_MS = 10_000;

// Своя ошибка: хранит текст для пользователя и HTTP-статус ответа
export class ApiError extends Error {
  status: number;
  retryAfter?: number; // через сколько секунд можно повторить (при статусе 429)

  constructor(message: string, status: number, retryAfter?: number) {
    super(message);
    this.status = status;
    this.retryAfter = retryAfter;
  }
}

// Универсальная функция: отправляет POST-запрос с JSON и возвращает ответ
async function post<T>(path: string, body: unknown): Promise<T> {
  let response: Response;

  try {
    response = await fetch(`${API_URL}${path}`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body), // превращаем объект в JSON-строку
      signal: AbortSignal.timeout(REQUEST_TIMEOUT_MS), // отменяем зависший запрос
    });
  } catch (err) {
    if (err instanceof DOMException && err.name === 'TimeoutError') {
      throw new ApiError('Сервер слишком долго отвечает', 0);
    }
    // сюда попадаем, если сервер выключен, нет интернета или заблокирован CORS
    throw new ApiError('Не удалось связаться с сервером', 0);
  }

  // Слишком много запросов: сервер просит подождать
  if (response.status === 429) {
    // Чтобы браузер дал прочитать этот заголовок, бэкенд должен добавить
    // Retry-After в Access-Control-Expose-Headers
    const retryAfter = Number(response.headers.get('Retry-After')) || 60;
    throw new ApiError('Слишком много попыток', 429, retryAfter);
  }

  // Читаем JSON из ответа (если тело пустое, получим null)
  const data = await response.json().catch(() => null);

  // Статусы 400, 401, 500 и т. д. считаем ошибкой
  if (!response.ok) {
    throw new ApiError(data?.message ?? 'Что-то пошло не так', response.status);
  }

  return data as T;
}

// ---------- Токен (пропуск, который сервер выдаёт после входа) ----------
const TOKEN_KEY = 'token';
export const saveToken = (token: string) => localStorage.setItem(TOKEN_KEY, token);
export const getToken = () => localStorage.getItem(TOKEN_KEY);
export const clearToken = () => localStorage.removeItem(TOKEN_KEY);

// ---------- Запросы к бэкенду ----------
interface AuthResponse {
  token: string;
}

export function register(data: {
  username: string;
  email: string;
  password: string;
}) {
  return post<AuthResponse>('/auth/register', data);
}

export function login(data: { email: string; password: string }) {
  return post<AuthResponse>('/auth/login', data);
}

// Отправить код восстановления на почту
export function sendResetCode(email: string) {
  return post<void>('/auth/forgot-password', { email });
}

// Проверить код из письма
export function verifyResetCode(email: string, code: string) {
  return post<{ resetToken?: string }>('/auth/verify-code', { email, code });
}

// Достаём понятный текст из любой ошибки
export function getErrorMessage(err: unknown): string {
  if (err instanceof ApiError) {
    return err.retryAfter
      ? `${err.message}. Повторите через ${err.retryAfter} с`
      : err.message;
  }
  return 'Неизвестная ошибка';
}
