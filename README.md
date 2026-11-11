# 숫자형 단축 URL 서비스

## 배포 URL

- 서비스 URL: https://numeric-short-url.numeric-short-url.workers.dev
- 테스트 단축 URL: https://numeric-short-url.numeric-short-url.workers.dev/1
- 테스트 원본 URL: https://example.org/docs?lang=ko&x=1#section-2

---

로그인 없이 긴 URL을 `/123` 같은 **숫자로만 된 단축 URL**로 바꿔 주는 서비스입니다.
화면과 서버 기능을 모두 **Dioxus 0.7 풀스택(Rust)** 으로 구현했고, Cloudflare Workers(공개 접점)와 Railway(서버 + PostgreSQL)에 배포했습니다.

용어는 [`GLOSSARY.md`](GLOSSARY.md), 공개 접점 구조의 결정 배경은 [`docs/adr/0001`](docs/adr/0001-cloudflare-worker-proxies-to-railway.md)을 참고하세요.

## 구조

```
브라우저 ──HTTPS──▶ Cloudflare Worker (numeric-short-url.*.workers.dev)
                     ├─ /, /assets/*      → Dioxus 웹 번들 (정적 자산, 사전 렌더링된 index.html)
                     └─ /{숫자}, /api/*   → 프록시 (redirect: "manual") ─▶ Railway: Dioxus 서버
                                                                           ├─ POST /api/links  (서버 함수: 생성)
                                                                           ├─ GET  /{숫자}     (302 리디렉션 / 404)
                                                                           └─ PostgreSQL (Railway)
```

| 경로 | 파일 |
|---|---|
| 화면(입력·생성·결과·복사), 생성 서버 함수, 서버 진입점 | [`src/main.rs`](src/main.rs) |
| 리디렉션·404·변경 거부 미들웨어 | [`src/server/guard.rs`](src/server/guard.rs) |
| URL 검증 | [`src/server/validate.rs`](src/server/validate.rs) |
| DB 저장·조회 (INSERT / SELECT 만 존재) | [`src/server/links.rs`](src/server/links.rs) |
| 스키마 (IDENTITY PK, 불변 트리거) | [`migrations/0001_create_links.sql`](migrations/0001_create_links.sql) |
| Cloudflare Worker / 설정 | [`worker/index.js`](worker/index.js), [`wrangler.toml`](wrangler.toml) |
| Railway 설정 | [`Dockerfile`](Dockerfile), [`railway.toml`](railway.toml) |
| Cloudflare용 웹 번들 빌드 | [`scripts/build-web.sh`](scripts/build-web.sh) |
| 환경변수 예시 | [`.env.example`](.env.example) |

## 설계

### 단축 번호 발급과 중복 방지

- 단축 번호는 PostgreSQL `links.id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY` 시퀀스가 발급합니다.
  애플리케이션은 번호를 계산하지 않고 `INSERT ... RETURNING id` 로 받기만 하므로, 동시에 여러 요청이 와도 같은 번호가 두 번 나오지 않습니다.
- `PRIMARY KEY` 제약이 중복을 최종적으로 막습니다. `GENERATED ALWAYS` 라서 INSERT 시 번호를 직접 지정할 수도 없습니다(번호 재사용 불가).
- 1부터 시작합니다. 실패한 트랜잭션 등으로 **중간 번호가 비는 것은 허용**합니다(연속성은 보장하지 않음).
- **같은 원본 URL을 다시 입력하면 매번 새 단축 번호를 발급합니다.** URL 정규화 기준(끝의 `/`, 대소문자 등)을 정할 필요가 없고, PK 하나로 동시성 문제가 끝나기 때문입니다.

### 생성 후 불변 (세 겹)

1. **API에 변경 수단이 없음**: 생성 서버 함수 `POST /api/links` 는 원본 URL만 받고 단축 번호를 입력받지 않습니다. 수정·삭제·만료 기능과 코드(UPDATE/DELETE 쿼리)가 없습니다.
2. **서버가 변경 요청을 명시적으로 거부**: `PUT`/`PATCH`/`DELETE`/`POST /{숫자}`, `PUT`/`PATCH`/`DELETE /api/links[/...]` 는 `405 Method Not Allowed` + `링크는 생성 후 수정·삭제할 수 없습니다.` 를 반환합니다 (`src/server/guard.rs`).
3. **DB 트리거**: `links` 테이블에 대한 `UPDATE`, `DELETE`, `TRUNCATE` 를 트리거가 예외로 거부합니다. DB에 직접 접속해도 링크를 바꿀 수 없습니다.

### 리디렉션

- `GET`/`HEAD /{숫자}` → 링크가 있으면 **302 Found**, `Location` 에 저장된 원본 URL 전체를 넣습니다. 경로·쿼리 문자열·프래그먼트(`#...`)가 그대로 유지됩니다(비 ASCII 문자만 퍼센트 인코딩).
- 없는 번호, `i64` 범위를 넘는 숫자 → **404** + 한국어 안내 페이지(새로 만들기 링크).
- 숫자가 아닌 경로는 Dioxus 화면 라우터로 넘어갑니다.
- 연결은 불변이라 301도 의미상 맞지만, 브라우저 영구 캐시로 디버깅이 어려워지는 것을 피하려고 302를 씁니다.

### 서버 URL 검증

앞뒤 공백 제거 후 다음을 모두 만족해야 저장합니다. 저장값은 정규화하지 않은 입력 그대로입니다.

- `url` 크레이트로 파싱 가능, 공백·제어 문자 없음
- 스킴이 `http` 또는 `https` (`javascript:`, `ftp:`, `data:` 등 거부)
- 호스트 존재, 2048자 이하
- 이 서비스 자신의 호스트(`PUBLIC_BASE_URL`)가 아님 — 리디렉션 루프 방지

실패하면 서버 함수가 400과 안내 문구를 돌려주고 화면에 그대로 표시합니다.

### 인증

회원가입·로그인이 없습니다. 생성(`POST /api/links`)과 이동(`GET /{숫자}`) 모두 누구나 사용할 수 있습니다.

### 범위 밖

요청 횟수 제한·캡차·커스텀 별칭·통계는 과제 요구사항에 없어 구현하지 않았습니다.

## 환경변수

| 이름 | 위치 | 설명 |
|---|---|---|
| `DATABASE_URL` | Railway `web` 서비스 | `${{Postgres.DATABASE_URL}}` 참조 변수 |
| `PUBLIC_BASE_URL` | Railway `web` 서비스 | 단축 URL 기준 주소 = Cloudflare Worker 주소 |
| `PORT` | Railway 자동 주입 | 서버 포트 (`IP=0.0.0.0` 은 Dockerfile에 설정) |
| `SKIP_MIGRATIONS` | 선택 | `1` 이면 시작 시 마이그레이션 생략 (사전 렌더링용) |
| `RAILWAY_ORIGIN` | `wrangler.toml` `[vars]` | Railway 공개 도메인 (비밀 값 아님) |

비밀 값(DB 비밀번호, 토큰)은 저장소와 브라우저 코드에 들어가지 않습니다. `DATABASE_URL` 은 Railway 대시보드의 서비스 변수로만 존재합니다.

## 로컬 실행

필요한 도구: Rust stable + `wasm32-unknown-unknown` 타깃, `dioxus-cli` 0.7, Docker, Node.js(wrangler).

```bash
# 1) DB 준비 (PostgreSQL 17)
docker run -d --name nsu-postgres -e POSTGRES_PASSWORD=postgres -e POSTGRES_DB=short_url -p 55432:5432 postgres:17-alpine

# 2) 환경변수 (.env.example 참고)
export DATABASE_URL=postgres://postgres:postgres@localhost:55432/short_url
export PUBLIC_BASE_URL=http://127.0.0.1:8080

# 3) 개발 서버 (마이그레이션은 서버 시작 시 자동 적용)
dx serve
```

`http://127.0.0.1:8080` 에서 화면과 `/{숫자}` 리디렉션을 모두 확인할 수 있습니다.

## 배포

### Railway (서버 + DB)

```bash
railway init --name numeric-short-url
railway add --database postgres
railway add --service web \
  --variables 'DATABASE_URL=${{Postgres.DATABASE_URL}}' \
  --variables 'PUBLIC_BASE_URL=https://<worker 이름>.<서브도메인>.workers.dev'
railway domain --service web          # 예: https://web-production-xxxx.up.railway.app
railway up --service web --detach     # Dockerfile 로 빌드 (railway.toml)
```

Dockerfile 은 `dx bundle --web --fullstack --release` 로 서버 바이너리를 만들고, 시작 시 `migrations/` 를 자동 적용합니다.

### Cloudflare Workers (공개 접점)

```bash
# wrangler.toml 의 RAILWAY_ORIGIN 을 위 Railway 도메인으로 설정한 뒤
bash scripts/build-web.sh   # dist/public 생성 (index.html 사전 렌더링)
wrangler deploy
```

**라우팅 구성**: Worker 가 모든 요청을 먼저 받습니다(`run_worker_first = true`). `/^\/(\d+|api(\/.*)?)$/` 는 `RAILWAY_ORIGIN` 으로 프록시하고, 그 외는 정적 자산으로 응답합니다.
프록시는 `redirect: "manual"` 이라 Railway 의 302 를 따라가지 않고 그대로 브라우저에 전달하므로, 공개 주소의 `/{숫자}` 가 서버의 HTTP 리디렉션으로 이동합니다.
화면과 API 가 같은 출처이므로 CORS 설정이 없습니다. Pages `_redirects` 는 외부 출처 프록시를 지원하지 않아 Workers 를 선택했습니다([ADR 0001](docs/adr/0001-cloudflare-worker-proxies-to-railway.md)).

**사전 렌더링 이유**: Dioxus 풀스택 웹 번들은 서버가 그린 HTML 을 이어받는(hydration) 방식이라 빈 `index.html` 로는 동작하지 않습니다. `scripts/build-web.sh` 가 서버로 `/` 를 한 번 렌더링해 `dist/public/index.html` 로 저장합니다(`/` 렌더링은 DB 를 쓰지 않음).

## 테스트

```bash
docker run -d --name nsu-postgres -e POSTGRES_PASSWORD=postgres -e POSTGRES_DB=short_url -p 55432:5432 postgres:17-alpine
DATABASE_URL=postgres://postgres:postgres@localhost:55432/short_url \
  cargo test --no-default-features --features server
```

`#[sqlx::test]` 가 테스트마다 새 DB 를 만들고 마이그레이션을 적용합니다. 15개 테스트:

- **URL 검증**: http/https 허용, 다른 스킴·형식 오류·공백·2048자 초과·자기 호스트 거부, `Location` 이 경로·쿼리·프래그먼트 보존
- **DB**: 저장·조회, 같은 URL 재입력 시 새 번호, **동시 생성 50건의 번호가 모두 다르고 기존 링크가 덮어써지지 않음**, `UPDATE`/`DELETE`/`TRUNCATE`/번호 지정 `INSERT` 거부
- **리디렉션**: 302 + `Location` 보존, HEAD, 없는 번호·범위 초과 404 안내, `PUT`/`PATCH`/`DELETE`/`POST` 405 거부, 숫자가 아닌 경로는 화면으로 통과

## 확인 시나리오 결과 (2026-11-11, 배포 주소 기준)

| 시나리오 | 결과 |
|---|---|
| 새 브라우저 세션에서 URL 입력 → 숫자형 주소 생성·복사 | `https://example.org/path?q=1#frag` → `/22` 생성, 결과창과 복사 버튼 표시. 클립보드 권한이 없는 환경에서는 주소를 선택 상태로 두고 Ctrl+C 안내 |
| 생성한 주소를 다른 클라이언트에서 열기 | `GET /1` → `302 Location: https://example.org/docs?lang=ko&x=1#section-2` (쿼리·프래그먼트 유지) |
| 서로 다른 URL 20개 동시 생성 | 20개 모두 서로 다른 번호, 각 번호가 자기 원본 URL로 이동(덮어쓰기 없음) |
| Railway 재배포 후 기존 주소 | 재배포 후 `/1`, `/22` 가 같은 원본 URL 로 302 |
| 잘못된 URL (`ftp://bad`, `javascript:alert(1)`, 서비스 자기 주소) | 400 + "http:// 또는 https:// 로 시작하는 올바른 URL을 입력하세요." / 자기 주소 거부 |
| 존재하지 않는 숫자 (`/987654321`) | 404 + "존재하지 않는 단축 URL입니다" 안내 |
| 기존 연결 변경 요청 (`PUT`/`PATCH`/`DELETE /1`, `DELETE /api/links/1`) | 모두 405, 이후에도 `/1` 의 원본 URL 동일 |

직접 재현하려면:

```bash
B=https://numeric-short-url.numeric-short-url.workers.dev
curl -s -X POST $B/api/links -H 'content-type: application/json' -d '{"original_url":"https://example.org/a?b=1#c"}'
curl -sI $B/1                       # 302 + Location
curl -s -o /dev/null -w '%{http_code}\n' $B/987654321          # 404
curl -s -X DELETE -w ' %{http_code}\n' $B/1                    # 405
```
