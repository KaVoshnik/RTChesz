#!/usr/bin/env bash
# Первичная настройка чистого Ubuntu 24.04. Запускать ОДИН раз от root:
#   bash setup-server.sh
set -euo pipefail

if [[ $EUID -ne 0 ]]; then echo "Запустите от root"; exit 1; fi

export DEBIAN_FRONTEND=noninteractive
apt-get update
apt-get -y upgrade
apt-get install -y git ufw fail2ban unattended-upgrades curl ca-certificates openssl

# Docker из репозитория Ubuntu (работает и когда download.docker.com недоступен)
apt-get install -y docker.io docker-compose-v2
apt-get install -y docker-buildx || true
systemctl enable --now docker

# Файрвол: только SSH, HTTP, HTTPS
ufw allow OpenSSH
ufw allow 80/tcp
ufw allow 443/tcp
ufw allow 443/udp
ufw --force enable

# Swap 2 ГБ: сборка Rust на маленьком VPS без него может упасть по памяти
if ! swapon --show | grep -q .; then
  fallocate -l 2G /swapfile
  chmod 600 /swapfile
  mkswap /swapfile
  swapon /swapfile
  echo '/swapfile none swap sw 0 0' >> /etc/fstab
fi

systemctl enable --now fail2ban
echo "Готово. Дальше: клонируйте репозиторий, заполните infra/.env и запустите infra/deploy.sh"
