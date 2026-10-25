#!/usr/bin/env bash
# Обновление и запуск. Запускать на сервере из любой папки:  bash infra/deploy.sh
set -euo pipefail
cd "$(dirname "$0")"

if [[ ! -f .env ]]; then echo "Нет infra/.env - скопируйте env.prod.example в .env и заполните"; exit 1; fi

git -C .. pull --ff-only || echo "(git pull пропущен)"
docker compose -f docker-compose.prod.yml --env-file .env up -d --build
docker image prune -f >/dev/null
docker compose -f docker-compose.prod.yml ps
