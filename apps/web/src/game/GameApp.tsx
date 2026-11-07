import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type FormEvent,
  type PointerEvent as ReactPointerEvent,
} from 'react';
import {
  ApiError,
  clearToken,
  getErrorMessage,
  getToken,
  login,
  register,
  saveToken,
} from '../api';
import {
  abortGame,
  createGame,
  declineDraw,
  getGame,
  joinGame,
  lobby,
  me,
  mine,
  offerDraw,
  resign,
  sendMove,
  type Game,
  type GameSummary,
  type Me,
  type Side,
} from './gameApi';
import {
  captured,
  kingInCheck,
  legalMoves,
  moveToUci,
  parseFen,
  replay,
  sqIndex,
  sqName,
  type Mv,
} from './chess';
import { ArrowLayer, type Arrow, type Brush, type Ring } from './Arrows';

// ---------- Мелочи ----------

const GLYPH: Record<string, string> = {
  k: '\u265A\uFE0E',
  q: '\u265B\uFE0E',
  r: '\u265C\uFE0E',
  b: '\u265D\uFE0E',
  n: '\u265E\uFE0E',
  p: '\u265F\uFE0E',
};

const TERMINATION: Record<string, string> = {
  checkmate: 'мат',
  stalemate: 'пат',
  insufficient_material: 'недостаточно материала',
  fifty_move_rule: 'правило 50 ходов',
  threefold_repetition: 'троекратное повторение',
  resignation: 'сдача',
  timeout: 'время вышло',
  draw_agreement: 'ничья по согласию',
  aborted: 'партия отменена',
};

const PRESETS = [
  { initialSecs: 60, incrementSecs: 0 },
  { initialSecs: 180, incrementSecs: 2 },
  { initialSecs: 300, incrementSecs: 3 },
  { initialSecs: 600, incrementSecs: 0 },
  { initialSecs: 900, incrementSecs: 10 },
];

const tcLabel = (tc: { initialSecs: number; incrementSecs: number }) => {
  const mins = tc.initialSecs / 60;
  return `${Number.isInteger(mins) ? mins : `${tc.initialSecs}с`}+${tc.incrementSecs}`;
};

const goto = (hash: string) => window.location.assign(hash);
const other = (s: Side): Side => (s === 'white' ? 'black' : 'white');
const isOver = (g: Game) => g.status === 'finished' || g.status === 'aborted';

function fmtClock(ms: number): string {
  const total = Math.max(0, ms);
  if (total < 10_000) return `0:${(total / 1000).toFixed(1).padStart(4, '0')}`;
  const s = Math.floor(total / 1000);
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`;
}

function resultText(g: Game, mySide: Side | null): string {
  const why = g.termination ? (TERMINATION[g.termination] ?? g.termination) : '';
  if (g.status === 'aborted') return 'Партия отменена';
  if (g.result === 'draw') return `Ничья — ${why}`;
  const winner = g.result === 'white' ? 'Белые' : 'Чёрные';
  if (mySide) return `${g.result === mySide ? 'Вы победили' : 'Вы проиграли'} — ${why}`;
  return `${winner} победили — ${why}`;
}

function useHash(): string {
  const [hash, setHash] = useState(window.location.hash);
  useEffect(() => {
    const on = () => setHash(window.location.hash);
    window.addEventListener('hashchange', on);
    return () => window.removeEventListener('hashchange', on);
  }, []);
  return hash;
}

// ---------- Корень: проверка токена ----------

export default function GameApp() {
  const [user, setUser] = useState<Me | null>(null);
  const [checking, setChecking] = useState(() => getToken() !== null);
  const hash = useHash();

  useEffect(() => {
    if (!getToken()) return;
    me()
      .then(setUser)
      .catch(() => clearToken())
      .finally(() => setChecking(false));
  }, []);

  const logout = useCallback(() => {
    clearToken();
    setUser(null);
  }, []);

  if (checking)
    return (
      <div className="g-page">
        <p className="g-muted">Загрузка…</p>
      </div>
    );
  if (!user) return <AuthForm onDone={(u) => setUser(u)} />;

  const m = /^#\/g\/([0-9a-fA-F-]{36})$/.exec(hash);
  return (
    <div className="g-page">
      <header className="g-header">
        <a className="g-logo" href="#/">
          RTChesz
        </a>
        <span className="g-user">
          {user.username}
          <button type="button" className="g-link" onClick={logout}>
            Выйти
          </button>
        </span>
      </header>
      {m ? (
        <GameView key={m[1]} id={m[1]} user={user} onLogout={logout} />
      ) : (
        <Lobby onLogout={logout} />
      )}
    </div>
  );
}

// ---------- Вход / регистрация ----------

function AuthForm({ onDone }: { onDone: (u: Me) => void }) {
  const [mode, setMode] = useState<'login' | 'register'>('login');
  const [username, setUsername] = useState('');
  const [email, setEmail] = useState('');
  const [password, setPassword] = useState('');
  const [error, setError] = useState('');
  const [loading, setLoading] = useState(false);

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    if (loading) return;
    setError('');
    setLoading(true);
    try {
      const res =
        mode === 'login'
          ? await login({ email, password })
          : await register({ username: username.trim(), email, password });
      saveToken(res.token);
      onDone(await me());
    } catch (err) {
      setError(getErrorMessage(err));
    } finally {
      setLoading(false);
    }
  };

  return (
    <div className="g-page g-center">
      <form className="g-card g-auth" onSubmit={submit}>
        <h2>{mode === 'login' ? 'Вход' : 'Регистрация'}</h2>
        {mode === 'register' && (
          <input
            className="g-input"
            placeholder="Имя пользователя"
            value={username}
            onChange={(e) => setUsername(e.target.value)}
            maxLength={24}
            required
          />
        )}
        <input
          className="g-input"
          type="email"
          placeholder="Email"
          value={email}
          onChange={(e) => setEmail(e.target.value)}
          required
        />
        <input
          className="g-input"
          type="password"
          placeholder="Пароль"
          value={password}
          onChange={(e) => setPassword(e.target.value)}
          minLength={mode === 'register' ? 8 : undefined}
          maxLength={128}
          required
        />
        {error && (
          <div className="g-error" role="alert">
            {error}
          </div>
        )}
        <button className="g-btn" type="submit" disabled={loading}>
          {loading ? 'Подождите…' : mode === 'login' ? 'Войти' : 'Создать аккаунт'}
        </button>
        <button
          type="button"
          className="g-link"
          onClick={() => {
            setMode(mode === 'login' ? 'register' : 'login');
            setError('');
          }}
        >
          {mode === 'login' ? 'Нет аккаунта? Зарегистрироваться' : 'Уже есть аккаунт? Войти'}
        </button>
      </form>
    </div>
  );
}

// ---------- Лобби ----------

function Lobby({ onLogout }: { onLogout: () => void }) {
  const [open, setOpen] = useState<GameSummary[]>([]);
  const [own, setOwn] = useState<GameSummary[]>([]);
  const [preset, setPreset] = useState(2);
  const [color, setColor] = useState<Side | 'random'>('random');
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);

  const fail = useCallback(
    (err: unknown) => {
      if (err instanceof ApiError && err.status === 401) onLogout();
      else setError(getErrorMessage(err));
    },
    [onLogout]
  );

  useEffect(() => {
    let stop = false;
    const load = async () => {
      try {
        const [l, m] = await Promise.all([lobby(), mine()]);
        if (stop) return;
        setOpen(l.games);
        setOwn(m.games);
      } catch (err) {
        if (!stop) fail(err);
      }
    };
    load();
    const t = setInterval(load, 3000);
    return () => {
      stop = true;
      clearInterval(t);
    };
  }, [fail]);

  const create = async () => {
    if (busy) return;
    setBusy(true);
    setError('');
    try {
      const g = await createGame(PRESETS[preset], color);
      goto(`#/g/${g.id}`);
    } catch (err) {
      fail(err);
    } finally {
      setBusy(false);
    }
  };

  const join = async (id: string) => {
    if (busy) return;
    setBusy(true);
    setError('');
    try {
      await joinGame(id);
      goto(`#/g/${id}`);
    } catch (err) {
      fail(err);
    } finally {
      setBusy(false);
    }
  };

  const ongoing = own.filter((g) => g.status === 'active' || g.status === 'waiting');
  const history = own.filter((g) => g.status === 'finished' || g.status === 'aborted').slice(0, 10);

  return (
    <main className="g-lobby">
      {error && (
        <div className="g-error" role="alert">
          {error}
        </div>
      )}

      <section className="g-card">
        <h3>Новая партия</h3>
        <div className="g-chips">
          {PRESETS.map((p, i) => (
            <button
              key={i}
              type="button"
              className={`g-chip${i === preset ? ' on' : ''}`}
              onClick={() => setPreset(i)}
            >
              {tcLabel(p)}
            </button>
          ))}
        </div>
        <div className="g-chips">
          {(['white', 'random', 'black'] as const).map((c) => (
            <button
              key={c}
              type="button"
              className={`g-chip${c === color ? ' on' : ''}`}
              onClick={() => setColor(c)}
            >
              {c === 'white' ? 'Белые' : c === 'black' ? 'Чёрные' : 'Случайно'}
            </button>
          ))}
        </div>
        <button className="g-btn" type="button" onClick={create} disabled={busy}>
          Создать вызов
        </button>
      </section>

      <section className="g-card">
        <h3>Открытые вызовы</h3>
        {open.length === 0 && <p className="g-muted">Пока никого. Создайте свой вызов.</p>}
        {open.map((g) => {
          const host = g.white ?? g.black;
          return (
            <div className="g-row" key={g.id}>
              <span>
                <b>{host?.username ?? '—'}</b> · {tcLabel(g.timeControl)} · играет{' '}
                {g.white ? 'белыми' : 'чёрными'}
              </span>
              <button
                className="g-btn small"
                type="button"
                onClick={() => join(g.id)}
                disabled={busy}
              >
                Принять
              </button>
            </div>
          );
        })}
      </section>

      <section className="g-card">
        <h3>Мои партии</h3>
        {ongoing.length === 0 && history.length === 0 && (
          <p className="g-muted">Партий пока нет.</p>
        )}
        {ongoing.map((g) => (
          <div className="g-row" key={g.id}>
            <span>
              {g.status === 'waiting'
                ? 'Ждёт соперника'
                : `${g.white?.username} — ${g.black?.username}`}{' '}
              · {tcLabel(g.timeControl)}
            </span>
            <a className="g-btn small" href={`#/g/${g.id}`}>
              Открыть
            </a>
          </div>
        ))}
        {history.map((g) => (
          <div className="g-row dim" key={g.id}>
            <span>
              {g.white?.username ?? '?'} — {g.black?.username ?? '?'} ·{' '}
              {g.status === 'aborted'
                ? 'отменена'
                : g.result === 'draw'
                  ? '½–½'
                  : g.result === 'white'
                    ? '1–0'
                    : '0–1'}
            </span>
            <a className="g-link" href={`#/g/${g.id}`}>
              Смотреть
            </a>
          </div>
        ))}
      </section>
    </main>
  );
}

// ---------- Срубленные фигуры ----------

const CAP_ORDER = 'qrbnp';

function jelly(e: ReactPointerEvent<HTMLElement>) {
  const el = e.currentTarget;
  el.classList.remove('jelly');
  void el.offsetWidth; // перезапустить анимацию, даже если она ещё играет
  el.classList.add('jelly');
}

const VALUE: Record<string, number> = { p: 1, n: 3, b: 3, r: 5, q: 9 };
const points = (pieces: string[]) => pieces.reduce((sum, p) => sum + VALUE[p.toLowerCase()], 0);

function CapturedRow({ pieces, lead }: { pieces: string[]; lead: number }) {
  const groups = [...CAP_ORDER]
    .map((k) => pieces.filter((p) => p.toLowerCase() === k))
    .filter((g) => g.length > 0);
  return (
    <div className="g-caprow" aria-label="Срубленные фигуры">
      {groups.map((g) => (
        <span className="g-capgrp" key={g[0].toLowerCase()}>
          {g.map((p, i) => (
            <span
              key={i}
              className={`g-cap ${p === p.toUpperCase() ? 'w' : 'b'}`}
              onPointerEnter={jelly}
              onAnimationEnd={(e) => e.currentTarget.classList.remove('jelly')}
            >
              {GLYPH[p.toLowerCase()]}
            </span>
          ))}
        </span>
      ))}
      <span className="g-score" title="Очки за срубленные фигуры">
        {points(pieces)}
        {lead > 0 && <b className="g-lead">+{lead}</b>}
      </span>
    </div>
  );
}

// ---------- Партия ----------

// Что сейчас делает указатель на доске: тянет фигуру (ЛКМ) или рисует стрелку (ПКМ)
type Session =
  | { kind: 'drag'; from: number; sx: number; sy: number; moved: boolean }
  | { kind: 'draw'; from: number; brush: Brush; ply: number };

function GameView({ id, user, onLogout }: { id: string; user: Me; onLogout: () => void }) {
  const [snap, setSnap] = useState<{ game: Game; at: number } | null>(null);
  const [error, setError] = useState('');
  const [notFound, setNotFound] = useState(false);
  const [busy, setBusy] = useState(false);
  const [selected, setSelected] = useState<number | null>(null);
  const [promo, setPromo] = useState<{ from: number; to: number } | null>(null);
  const [now, setNow] = useState(() => Date.now());
  const [copied, setCopied] = useState(false);
  const finished = useRef(false);
  // null = смотрим текущую позицию; число = смотрим позицию после стольких полуходов
  const [viewPly, setViewPly] = useState<number | null>(null);
  const [drag, setDrag] = useState<{
    from: number;
    x: number;
    y: number;
    over: number | null;
  } | null>(null);
  const [marks, setMarks] = useState<{ ply: number; arrows: Arrow[]; rings: Ring[] } | null>(null);
  const [drawing, setDrawing] = useState<Arrow | null>(null);
  const boardRef = useRef<HTMLDivElement>(null);
  const movesRef = useRef<HTMLDivElement>(null);
  const session = useRef<Session | null>(null);
  const stopSession = useRef<(() => void) | null>(null);
  const justDragged = useRef(false);

  const accept = useCallback((g: Game) => {
    finished.current = isOver(g);
    setSnap((prev) =>
      prev && prev.game.moves.length > g.moves.length ? prev : { game: g, at: Date.now() }
    ); // устаревший ответ опроса не должен откатить партию назад
  }, []);

  const fail = useCallback(
    (err: unknown) => {
      if (err instanceof ApiError && err.status === 401) onLogout();
      else if (err instanceof ApiError && err.status === 404) setNotFound(true);
      else setError(getErrorMessage(err));
    },
    [onLogout]
  );

  // Опрос партии раз в секунду, пока она не закончилась
  useEffect(() => {
    let stop = false;
    let inFlight = false;
    const tick = async () => {
      if (inFlight) return;
      inFlight = true;
      try {
        const g = await getGame(id);
        if (!stop) {
          accept(g);
          setError('');
        }
      } catch (err) {
        if (!stop) fail(err);
      } finally {
        inFlight = false;
      }
    };
    tick();
    const t = setInterval(() => {
      if (!finished.current) tick();
    }, 1000);
    return () => {
      stop = true;
      clearInterval(t);
    };
  }, [id, accept, fail]);

  // Часы тикают локально
  useEffect(() => {
    const t = setInterval(() => setNow(Date.now()), 100);
    return () => clearInterval(t);
  }, []);

  const game = snap?.game ?? null;
  const pos = useMemo(() => (game ? parseFen(game.fen) : null), [game]);
  const total = game?.moves.length ?? 0;
  const cur = viewPly ?? total; // сколько полуходов показано на доске
  const viewing = cur < total;
  const positions = useMemo(() => (game ? replay(game.moves) : []), [game]);
  const mySide: Side | null = !game
    ? null
    : game.white?.id === user.id
      ? 'white'
      : game.black?.id === user.id
        ? 'black'
        : null;
  const myTurn = !!game && game.status === 'active' && mySide !== null && game.turn === mySide;

  const canPlay = myTurn && !viewing; // ходить можно только из текущей позиции
  const flipped = mySide === 'black';

  const legal = useMemo(() => (pos && canPlay ? legalMoves(pos) : []), [pos, canPlay]);
  const targets = useMemo(
    () => (selected === null ? [] : legal.filter((m) => m.from === selected)),
    [legal, selected]
  );

  const goTo = useCallback(
    (ply: number) => {
      const p = Math.max(0, Math.min(ply, total));
      setViewPly(p >= total ? null : p);
      setSelected(null);
      setPromo(null);
    },
    [total]
  );

  // Стрелки на клавиатуре: ← → по ходам, Home/End в начало/конец
  useEffect(() => {
    const on = (e: KeyboardEvent) => {
      if (e.altKey || e.ctrlKey || e.metaKey || e.shiftKey) return;
      const t = e.target;
      if (
        t instanceof HTMLElement &&
        (t.isContentEditable || /^(INPUT|TEXTAREA|SELECT)$/.test(t.tagName))
      )
        return;
      const next =
        e.key === 'ArrowLeft'
          ? cur - 1
          : e.key === 'ArrowRight'
            ? cur + 1
            : e.key === 'Home'
              ? 0
              : e.key === 'End'
                ? total
                : null;
      if (next === null) return;
      e.preventDefault();
      goTo(next);
    };
    window.addEventListener('keydown', on);
    return () => window.removeEventListener('keydown', on);
  }, [cur, total, goTo]);

  // Держим выбранный ход в видимой части списка (только внутри списка, страницу не двигаем)
  useEffect(() => {
    const box = movesRef.current;
    const el = box?.querySelector<HTMLElement>('.cur');
    if (!box || !el) return;
    if (el.offsetTop < box.scrollTop) box.scrollTop = el.offsetTop;
    else if (el.offsetTop + el.offsetHeight > box.scrollTop + box.clientHeight)
      box.scrollTop = el.offsetTop + el.offsetHeight - box.clientHeight;
  }, [cur, total]);

  // Если ушли со страницы посреди перетаскивания - снять слушатели
  useEffect(() => () => stopSession.current?.(), []);

  if (notFound)
    return (
      <p className="g-muted g-pad">
        Партия не найдена. <a href="#/">В лобби</a>
      </p>
    );
  if (!game || !pos || !snap) return <p className="g-muted g-pad">Загрузка партии…</p>;

  const view = (viewing ? positions[cur] : null) ?? pos; // позиция, которая нарисована на доске
  const lastUci = cur > 0 ? game.moves[cur - 1] : undefined;
  const last = lastUci ? [sqIndex(lastUci.slice(0, 2)), sqIndex(lastUci.slice(2, 4))] : [];
  const checkSq = viewing ? kingInCheck(view) : game.status === 'active' ? kingInCheck(pos) : -1;
  const shownMarks = marks && marks.ply === cur ? marks : null;

  const clockMs = (side: Side) => {
    const base = side === 'white' ? game.clock.whiteMs : game.clock.blackMs;
    return game.clock.running === side ? base - (now - snap.at) : base;
  };

  const run = async (fn: () => Promise<Game | { message: string }>, after?: () => void) => {
    if (busy) return;
    setBusy(true);
    setError('');
    try {
      const res = await fn();
      if ('fen' in res) accept(res);
      after?.();
    } catch (err) {
      fail(err);
      getGame(id)
        .then(accept)
        .catch(() => {}); // подтянуть актуальное состояние
    } finally {
      setBusy(false);
    }
  };

  const submit = (uci: string) => {
    setSelected(null);
    setPromo(null);
    run(() => sendMove(id, uci));
  };

  // Пробует сделать ход from -> to (и кликом, и перетаскиванием). false, если ход нелегальный.
  const tryMove = (from: number, to: number): boolean => {
    const hit = legal.filter((m) => m.from === from && m.to === to);
    if (!hit.length) return false;
    if (hit.some((m) => m.promo)) setPromo({ from, to });
    else submit(moveToUci(hit[0]));
    return true;
  };

  const isMine = (p: string) => (mySide === 'white') === (p === p.toUpperCase());

  const onSquare = (sq: number) => {
    if (justDragged.current) return; // клик, которым закончилось перетаскивание
    if (!canPlay || busy) return;
    if (selected !== null && tryMove(selected, sq)) return;
    const p = pos.board[sq];
    setSelected(p && isMine(p) && sq !== selected ? sq : null);
  };

  const squareAt = (x: number, y: number): number | null => {
    const el = boardRef.current;
    if (!el) return null;
    const r = el.getBoundingClientRect();
    const col = Math.floor(((x - r.left) / r.width) * 8);
    const row = Math.floor(((y - r.top) / r.height) * 8);
    if (col < 0 || col > 7 || row < 0 || row > 7) return null;
    return flipped ? 63 - (row * 8 + col) : row * 8 + col;
  };

  const toggleMark = (s: Extract<Session, { kind: 'draw' }>, to: number) =>
    setMarks((prev) => {
      const base = prev && prev.ply === s.ply ? prev : { ply: s.ply, arrows: [], rings: [] };
      if (to === s.from) {
        const had = base.rings.find((r) => r.sq === to);
        const rings = base.rings.filter((r) => r.sq !== to);
        if (!had || had.brush !== s.brush) rings.push({ sq: to, brush: s.brush });
        return { ...base, rings };
      }
      const same = (a: Arrow) => a.from === s.from && a.to === to;
      const had = base.arrows.find(same);
      const arrows = base.arrows.filter((a) => !same(a));
      if (!had || had.brush !== s.brush) arrows.push({ from: s.from, to, brush: s.brush });
      return { ...base, arrows };
    });

  // ЛКМ по своей фигуре - тянем её; ПКМ - рисуем стрелку (Shift - красная, Ctrl - синяя, Alt - жёлтая)
  const onBoardDown = (e: ReactPointerEvent<HTMLDivElement>) => {
    if (session.current || promo) return;
    const sq = squareAt(e.clientX, e.clientY);
    if (sq === null) return;

    if (e.button === 2) {
      const brush: Brush = e.shiftKey ? 'red' : e.ctrlKey ? 'blue' : e.altKey ? 'yellow' : 'green';
      session.current = { kind: 'draw', from: sq, brush, ply: cur };
      setDrawing({ from: sq, to: sq, brush });
    } else if (e.button === 0) {
      setMarks(null); // обычный клик стирает стрелки
      const p = view.board[sq];
      if (!canPlay || busy || !p || !isMine(p) || !legal.some((m) => m.from === sq)) return;
      session.current = { kind: 'drag', from: sq, sx: e.clientX, sy: e.clientY, moved: false };
    } else return;

    const move = (ev: PointerEvent) => {
      const s = session.current;
      if (!s) return;
      if (s.kind === 'drag') {
        if (!s.moved && Math.hypot(ev.clientX - s.sx, ev.clientY - s.sy) < 5) return;
        if (!s.moved) {
          s.moved = true;
          setSelected(s.from); // показать, куда можно пойти
        }
        setDrag({
          from: s.from,
          x: ev.clientX,
          y: ev.clientY,
          over: squareAt(ev.clientX, ev.clientY),
        });
      } else {
        const to = squareAt(ev.clientX, ev.clientY);
        if (to !== null) setDrawing((d) => (d && d.to !== to ? { ...d, to } : d));
      }
    };
    const up = (ev: PointerEvent) => {
      const s = session.current;
      stop();
      if (!s) return;
      const to = squareAt(ev.clientX, ev.clientY);
      if (s.kind === 'draw') {
        if (to !== null) toggleMark(s, to);
      } else if (s.moved) {
        justDragged.current = true; // следом прилетит click на ту же клетку - его игнорируем
        setTimeout(() => {
          justDragged.current = false;
        }, 0);
        if (to !== null && to !== s.from) tryMove(s.from, to);
      }
    };
    const stop = () => {
      window.removeEventListener('pointermove', move);
      window.removeEventListener('pointerup', up);
      window.removeEventListener('pointercancel', stop);
      session.current = null;
      stopSession.current = null;
      setDrag(null);
      setDrawing(null);
    };
    window.addEventListener('pointermove', move);
    window.addEventListener('pointerup', up);
    window.addEventListener('pointercancel', stop);
    stopSession.current = stop;
  };

  const order = flipped ? [...Array(64).keys()].reverse() : [...Array(64).keys()];
  const top: Side = flipped ? 'white' : 'black';
  const bottom: Side = other(top);
  const name = (s: Side) => (s === 'white' ? game.white : game.black)?.username ?? '—';
  const over = isOver(game);

  const playerBar = (side: Side) => (
    <div className={`g-player${game.turn === side && !over ? ' turn' : ''}`}>
      <span className={`g-dot ${side}`} />
      <span className="g-pname">{name(side)}</span>
      <span
        className={`g-clock${game.clock.running === side ? ' run' : ''}${clockMs(side) < 10_000 && game.status === 'active' ? ' low' : ''}`}
      >
        {fmtClock(clockMs(side))}
      </span>
    </div>
  );

  const caps = captured(positions, game.moves, cur);
  const capturedBy = (side: Side) =>
    caps.filter((p) => (p === p.toLowerCase()) === (side === 'white')); // белые рубят чёрные (строчные)

  const diff = points(capturedBy('white')) - points(capturedBy('black'));
  const leadOf = (side: Side) => Math.max(0, side === 'white' ? diff : -diff);

  const pairs: string[][] = [];
  for (let i = 0; i < game.moves.length; i += 2) pairs.push(game.moves.slice(i, i + 2));

  const link = `${window.location.origin}${window.location.pathname}#/g/${game.id}`;
  const oppSide = mySide ? other(mySide) : null;

  return (
    <main className="g-game">
      <div className="g-boardcol">
        <CapturedRow pieces={capturedBy(top)} lead={leadOf(top)} />
        {playerBar(top)}
        <div
          ref={boardRef}
          className={`g-board${viewing ? ' review' : ''}${drag ? ' dragging' : ''}`}
          role="grid"
          aria-label="Шахматная доска"
          onPointerDown={onBoardDown}
          onContextMenu={(e) => e.preventDefault()}
        >
          {order.map((sq) => {
            const r = Math.floor(sq / 8);
            const f = sq % 8;
            const piece = view.board[sq];
            const target = targets.find((m) => m.to === sq);
            const cls = [
              'g-sq',
              (r + f) % 2 === 0 ? 'light' : 'dark',
              sq === selected ? 'sel' : '',
              last.includes(sq) ? 'last' : '',
              sq === checkSq ? 'check' : '',
              target
                ? piece || (pos.ep === sq && pos.board[selected!]?.toLowerCase() === 'p')
                  ? 'cap'
                  : 'mv'
                : '',
              drag?.over === sq && target ? 'over' : '',
              canPlay ? 'live' : '',
            ].join(' ');
            const showFile = flipped ? r === 0 : r === 7;
            const showRank = flipped ? f === 7 : f === 0;
            return (
              <div key={sq} className={cls} onClick={() => onSquare(sq)} aria-label={sqName(sq)}>
                {piece && (
                  <span
                    className={`g-piece ${piece === piece.toUpperCase() ? 'w' : 'b'}${drag?.from === sq ? ' lifted' : ''}`}
                  >
                    {GLYPH[piece.toLowerCase()]}
                  </span>
                )}
                {showFile && <i className="g-file">{sqName(sq)[0]}</i>}
                {showRank && <i className="g-rank">{sqName(sq)[1]}</i>}
              </div>
            );
          })}
          <ArrowLayer
            arrows={shownMarks?.arrows ?? []}
            rings={shownMarks?.rings ?? []}
            preview={drawing}
            flipped={flipped}
          />
          {promo && (
            <div className="g-promo" onClick={() => setPromo(null)}>
              <div className="g-card" onClick={(e) => e.stopPropagation()}>
                <p>Превратить пешку в:</p>
                <div className="g-chips">
                  {(['q', 'r', 'b', 'n'] as const).map((k) => (
                    <button
                      key={k}
                      type="button"
                      className={`g-chip big ${mySide === 'white' ? 'w' : 'b'}`}
                      onClick={() =>
                        submit(moveToUci({ from: promo.from, to: promo.to, promo: k } as Mv))
                      }
                    >
                      {GLYPH[k]}
                    </button>
                  ))}
                </div>
              </div>
            </div>
          )}
        </div>
        {playerBar(bottom)}
        <CapturedRow pieces={capturedBy(bottom)} lead={leadOf(bottom)} />
        {drag && view.board[drag.from] && (
          <span
            className={`g-piece g-ghost ${view.board[drag.from] === view.board[drag.from]!.toUpperCase() ? 'w' : 'b'}`}
            style={{ left: drag.x, top: drag.y }}
          >
            {GLYPH[view.board[drag.from]!.toLowerCase()]}
          </span>
        )}
      </div>

      <aside className="g-side">
        <div className="g-card">
          <div className="g-status">
            {game.status === 'waiting' && 'Ожидание соперника…'}
            {game.status === 'active' &&
              (mySide
                ? myTurn
                  ? 'Ваш ход'
                  : 'Ход соперника'
                : `Ходят ${game.turn === 'white' ? 'белые' : 'чёрные'}`)}
            {over && resultText(game, mySide)}
          </div>
          <div className="g-muted">
            {tcLabel(game.timeControl)} · {game.ruleset}
          </div>
          {error && (
            <div className="g-error" role="alert">
              {error}
            </div>
          )}

          {game.status === 'waiting' && mySide && (
            <>
              <p className="g-muted">Отправьте ссылку другу или ждите соперника из лобби.</p>
              <button
                className="g-btn"
                type="button"
                onClick={() =>
                  navigator.clipboard.writeText(link).then(() => {
                    setCopied(true);
                    setTimeout(() => setCopied(false), 1500);
                  })
                }
              >
                {copied ? 'Скопировано' : 'Скопировать ссылку'}
              </button>
              <button
                className="g-btn ghost"
                type="button"
                onClick={() =>
                  run(
                    () => abortGame(id),
                    () => {
                      goto('#/');
                    }
                  )
                }
              >
                Отменить вызов
              </button>
            </>
          )}

          {game.status === 'waiting' && !mySide && (
            <button
              className="g-btn"
              type="button"
              disabled={busy}
              onClick={() => run(() => joinGame(id))}
            >
              Принять вызов
            </button>
          )}

          {game.status === 'active' && mySide && (
            <>
              {game.drawOfferedBy && game.drawOfferedBy === oppSide && (
                <div className="g-offer">
                  Соперник предлагает ничью
                  <div className="g-chips">
                    <button
                      className="g-btn small"
                      type="button"
                      disabled={busy}
                      onClick={() => run(() => offerDraw(id))}
                    >
                      Принять
                    </button>
                    <button
                      className="g-btn small ghost"
                      type="button"
                      disabled={busy}
                      onClick={() => run(() => declineDraw(id))}
                    >
                      Отклонить
                    </button>
                  </div>
                </div>
              )}
              {game.drawOfferedBy === mySide && (
                <div className="g-offer">
                  Вы предложили ничью
                  <button
                    className="g-btn small ghost"
                    type="button"
                    disabled={busy}
                    onClick={() => run(() => declineDraw(id))}
                  >
                    Забрать
                  </button>
                </div>
              )}
              <div className="g-chips">
                {game.ply >= 2 && !game.drawOfferedBy && (
                  <button
                    className="g-btn ghost"
                    type="button"
                    disabled={busy}
                    onClick={() => run(() => offerDraw(id))}
                  >
                    Предложить ничью
                  </button>
                )}
                {game.ply < 2 ? (
                  <button
                    className="g-btn ghost"
                    type="button"
                    disabled={busy}
                    onClick={() => run(() => abortGame(id))}
                  >
                    Отменить партию
                  </button>
                ) : (
                  <button
                    className="g-btn danger"
                    type="button"
                    disabled={busy}
                    onClick={() => {
                      if (window.confirm('Сдаться?')) run(() => resign(id));
                    }}
                  >
                    Сдаться
                  </button>
                )}
              </div>
            </>
          )}

          {over && (
            <a className="g-btn" href="#/">
              В лобби
            </a>
          )}
        </div>

        <div className="g-card g-moves">
          <h3>Ходы</h3>
          <div className="g-nav">
            {(
              [
                ['«', 'В начало (Home)', 0, cur === 0],
                ['‹', 'Назад (←)', cur - 1, cur === 0],
                ['›', 'Вперёд (→)', cur + 1, !viewing],
                ['»', 'К текущей позиции (End)', total, !viewing],
              ] as const
            ).map(([label, title, to, off]) => (
              <button
                key={label}
                type="button"
                className="g-btn small ghost"
                title={title}
                aria-label={title}
                disabled={off}
                onClick={() => goTo(to)}
              >
                {label}
              </button>
            ))}
          </div>
          {viewing && (
            <div className="g-viewbar">
              Просмотр: ход {cur} из {total}
              <button type="button" className="g-link" onClick={() => goTo(total)}>
                К текущей
              </button>
            </div>
          )}
          {pairs.length === 0 && <p className="g-muted">Ходов ещё нет</p>}
          <div className="g-movelist" ref={movesRef}>
            <ol>
              {pairs.map((p, i) => (
                <li key={i}>
                  <span className="n">{i + 1}.</span>
                  {[0, 1].map((j) => {
                    const ply = i * 2 + j + 1;
                    return p[j] === undefined ? (
                      <span key={j} />
                    ) : (
                      <button
                        key={j}
                        type="button"
                        className={`g-mv${cur === ply ? ' cur' : ''}`}
                        onClick={() => goTo(ply)}
                      >
                        {p[j]}
                      </button>
                    );
                  })}
                </li>
              ))}
            </ol>
          </div>
        </div>
      </aside>
    </main>
  );
}
