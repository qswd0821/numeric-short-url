#!/usr/bin/env bash
# Cloudflare Worker 에 올릴 Dioxus 웹 번들(dist/public)을 만든다.
#
# Dioxus 풀스택 웹 번들은 서버가 그린 HTML 을 이어받는(hydration) 방식으로 동작한다.
# 그래서 빈 index.html 이 아니라, 서버로 `/` 를 한 번 렌더링한 결과를 dist/public/index.html 로 저장한다.
# `/` 렌더링은 DB 를 쓰지 않으므로 DB 없이(SKIP_MIGRATIONS=1, 지연 연결) 실행한다.
set -euo pipefail
cd "$(dirname "$0")/.."

PORT="${PRERENDER_PORT:-18999}"

dx bundle --web --fullstack --release --out-dir dist

SERVER=./dist/server
[ -x "$SERVER" ] || SERVER=./dist/server.exe

DATABASE_URL="postgres://prerender:prerender@127.0.0.1:1/prerender" \
SKIP_MIGRATIONS=1 \
PUBLIC_BASE_URL="https://prerender.invalid" \
IP=127.0.0.1 PORT="$PORT" \
"$SERVER" &
SERVER_PID=$!
trap 'kill $SERVER_PID 2>/dev/null || true' EXIT

for _ in $(seq 1 60); do
  if curl -fsS -o dist/public/index.html "http://127.0.0.1:$PORT/"; then
    echo "prerendered dist/public/index.html"
    exit 0
  fi
  sleep 1
done

echo "prerender failed: server did not respond on :$PORT" >&2
exit 1
