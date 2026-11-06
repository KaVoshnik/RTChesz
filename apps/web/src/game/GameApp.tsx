import { useCallback, useEffect, useMemo, useRef, useState, type FormEvent } from 'react';
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
import { kingInCheck, legalMoves, moveToUci, parseFen, sqIndex, sqName, type Mv } from './chess';

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

// ---------- Партия ----------

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
  const mySide: Side | null = !game
    ? null
    : game.white?.id === user.id
      ? 'white'
      : game.black?.id === user.id
        ? 'black'
        : null;
  const myTurn = !!game && game.status === 'active' && mySide !== null && game.turn === mySide;

  const legal = useMemo(() => (pos && myTurn ? legalMoves(pos) : []), [pos, myTurn]);
  const targets = useMemo(
    () => (selected === null ? [] : legal.filter((m) => m.from === selected)),
    [legal, selected]
  );

  if (notFound)
    return (
      <p className="g-muted g-pad">
        Партия не найдена. <a href="#/">В лобби</a>
      </p>
    );
  if (!game || !pos || !snap) return <p className="g-muted g-pad">Загрузка партии…</p>;

  const flipped = mySide === 'black';
  const lastUci = game.moves[game.moves.length - 1];
  const last = lastUci ? [sqIndex(lastUci.slice(0, 2)), sqIndex(lastUci.slice(2, 4))] : [];
  const checkSq = game.status === 'active' ? kingInCheck(pos) : -1;

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

  const onSquare = (sq: number) => {
    if (!myTurn || busy) return;
    const hit = targets.filter((m) => m.to === sq);
    if (hit.length) {
      if (hit.some((m) => m.promo)) setPromo({ from: selected!, to: sq });
      else submit(moveToUci(hit[0]));
      return;
    }
    const p = pos.board[sq];
    const mine = p && (mySide === 'white') === (p === p.toUpperCase());
    setSelected(mine && sq !== selected ? sq : null);
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

  const pairs: string[][] = [];
  for (let i = 0; i < game.moves.length; i += 2) pairs.push(game.moves.slice(i, i + 2));

  const link = `${window.location.origin}${window.location.pathname}#/g/${game.id}`;
  const oppSide = mySide ? other(mySide) : null;

  return (
    <main className="g-game">
      <div className="g-boardcol">
        {playerBar(top)}
        <div className="g-board" role="grid" aria-label="Шахматная доска">
          {order.map((sq) => {
            const r = Math.floor(sq / 8);
            const f = sq % 8;
            const piece = pos.board[sq];
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
              myTurn ? 'live' : '',
            ].join(' ');
            const showFile = flipped ? r === 0 : r === 7;
            const showRank = flipped ? f === 7 : f === 0;
            return (
              <div key={sq} className={cls} onClick={() => onSquare(sq)} aria-label={sqName(sq)}>
                {piece && (
                  <span className={`g-piece ${piece === piece.toUpperCase() ? 'w' : 'b'}`}>
                    {GLYPH[piece.toLowerCase()]}
                  </span>
                )}
                {showFile && <i className="g-file">{sqName(sq)[0]}</i>}
                {showRank && <i className="g-rank">{sqName(sq)[1]}</i>}
              </div>
            );
          })}
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
          {pairs.length === 0 && <p className="g-muted">Ходов ещё нет</p>}
          <ol>
            {pairs.map((p, i) => (
              <li key={i}>
                <span className="n">{i + 1}.</span>
                <span>{p[0]}</span>
                <span>{p[1] ?? ''}</span>
              </li>
            ))}
          </ol>
        </div>
      </aside>
    </main>
  );
}
