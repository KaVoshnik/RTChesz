#!/usr/bin/env bash
# Дамп базы в infra/backups, хранит последние 14. Ручной запуск:  bash infra/backup-db.sh
# По расписанию (каждую ночь в 04:00):  crontab -e  и добавить строку
#   0 4 * * * bash /opt/rtchesz/infra/backup-db.sh >> /var/log/rtchesz-backup.log 2>&1
set -euo pipefail
cd "$(dirname "$0")"
mkdir -p backups
set -a; source .env; set +a
file="backups/rtchesz-$(date +%F-%H%M).sql.gz"
docker compose -f docker-compose.prod.yml exec -T db \
  pg_dump -U "${POSTGRES_USER:-rtchesz}" "${POSTGRES_DB:-rtchesz}" | gzip > "$file"
ls -1t backups/*.sql.gz | tail -n +15 | xargs -r rm --
echo "OK: $file"
