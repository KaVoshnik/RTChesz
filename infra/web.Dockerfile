# Сборка фронта + раздача через Caddy (он же берёт HTTPS-сертификат).
# Контекст сборки - КОРЕНЬ репозитория.
FROM node:22-alpine AS build
WORKDIR /web
RUN corepack enable
COPY apps/web/package.json apps/web/pnpm-lock.yaml ./
RUN pnpm install --frozen-lockfile
COPY apps/web ./
# Фронт ходит в API по относительному пути: Caddy проксирует /api/* на бэкенд.
ARG VITE_API_URL=/api
ENV VITE_API_URL=$VITE_API_URL
RUN pnpm run build

FROM caddy:2-alpine
COPY infra/Caddyfile /etc/caddy/Caddyfile
COPY --from=build /web/dist /srv
