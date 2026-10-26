#!/usr/bin/env bash
# Обновление и запуск. Запускать на сервере из любой папки:
#   bash infra/deploy.sh            - вручную
#   bash infra/deploy.sh <sha>      - из CI: убедиться, что на сервере именно этот коммит
#
# Скрипт завершается с ошибкой (код 1), если после обновления API не стал healthy.
set -euo pipefail
cd "$(dirname "$0")"

# Всё тело - в функции: bash читает скрипт по мере выполнения, а `git pull` ниже может
# заменить этот самый файл. Функция целиком разбирается до запуска и от этого защищена.
main() {
  EXPECTED_SHA="${1:-}"
  COMPOSE=(docker compose -f docker-compose.prod.yml --env-file .env)
  HEALTH_TIMEOUT_SECS="${HEALTH_TIMEOUT_SECS:-180}"

  if [[ ! -f .env ]]; then echo "Нет infra/.env - скопируйте env.prod.example в .env и заполните"; exit 1; fi

  # Два деплоя одновременно (CI + руки) ломают друг другу сборку - второй ждёт первого.
  exec 9>/tmp/rtchesz-deploy.lock
  if ! flock -w 600 9; then echo "Другой деплой идёт больше 10 минут, выхожу"; exit 1; fi

  previous="$(git -C .. rev-parse --short HEAD 2>/dev/null || echo unknown)"

  if ! git -C .. pull --ff-only; then
    if [[ -n "$EXPECTED_SHA" ]]; then
      echo "git pull не удался, а деплой из CI требует свежий код - останавливаюсь"; exit 1
    fi
    echo "(git pull пропущен)"
  fi

  if [[ -n "$EXPECTED_SHA" ]]; then
    current="$(git -C .. rev-parse HEAD)"
    if [[ "$current" != "$EXPECTED_SHA"* ]]; then
      echo "На сервере коммит $current, а ожидался $EXPECTED_SHA - деплой отменён"; exit 1
    fi
  fi

  echo "Деплой: $previous -> $(git -C .. rev-parse --short HEAD 2>/dev/null || echo unknown)"

  "${COMPOSE[@]}" up -d --build --remove-orphans
  docker image prune -f >/dev/null

  # Ждём, пока API станет healthy (healthcheck из docker-compose.prod.yml).
  api_container="$("${COMPOSE[@]}" ps -q api)"
  deadline=$((SECONDS + HEALTH_TIMEOUT_SECS))
  status="starting"
  while (( SECONDS < deadline )); do
    status="$(docker inspect --format '{{if .State.Health}}{{.State.Health.Status}}{{else}}{{.State.Status}}{{end}}' "$api_container" 2>/dev/null || echo missing)"
    [[ "$status" == "healthy" ]] && break
    [[ "$status" == "exited" || "$status" == "dead" ]] && break
    sleep 3
  done

  "${COMPOSE[@]}" ps

  if [[ "$status" != "healthy" ]]; then
    echo
    echo "ОШИБКА: API не стал healthy (статус: $status). Последние логи:"
    "${COMPOSE[@]}" logs --tail 60 api || true
    echo
    echo "Откат на предыдущую версию: git -C .. checkout $previous && bash infra/deploy.sh"
    exit 1
  fi

  echo "OK: API healthy"
}

main "$@"
exit $?
