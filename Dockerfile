# Railway 배포용 이미지: Dioxus 풀스택 서버 (SSR + 서버 함수 + /{단축 번호} 리디렉션)

# ── build ───────────────────────────────────────────────────────────────
FROM rust:1-trixie AS build
WORKDIR /app

RUN rustup target add wasm32-unknown-unknown \
 && curl -L --proto '=https' --tlsv1.2 -sSf \
      https://raw.githubusercontent.com/cargo-bins/cargo-binstall/main/install-from-binstall-release.sh | bash \
 && cargo binstall -y dioxus-cli@0.7.10

COPY . .
RUN dx bundle --web --fullstack --release --out-dir /app/dist

# ── runtime ─────────────────────────────────────────────────────────────
FROM debian:trixie-slim AS runtime
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates \
 && rm -rf /var/lib/apt/lists/*
WORKDIR /app

COPY --from=build /app/dist/ /app/

# Railway 가 PORT 를 주입한다. Dioxus 서버는 IP / PORT 환경변수로 바인딩한다.
ENV IP=0.0.0.0 \
    PORT=8080
EXPOSE 8080

CMD ["/app/server"]
