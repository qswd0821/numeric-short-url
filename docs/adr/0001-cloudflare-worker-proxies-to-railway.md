# 0001. Cloudflare Worker가 화면을 제공하고 `/단축 번호`와 `/api/*`를 Railway로 프록시한다

- 상태: 채택
- 날짜: 2026-11-11

## 맥락

과제는 웹 화면·공개 접점을 Cloudflare에, Rust 서버(생성 API·리디렉션)를 Railway에 두라고 정한다. 그런데 공개 주소의 `/단축 번호`는 서버의 HTTP 리디렉션으로 이동해야 하고, 도메인은 구매하지 않는다.

## 결정

Cloudflare **Workers**(`*.workers.dev`) 하나를 공개 접점으로 둔다.

- Dioxus 웹 번들은 Worker의 정적 자산으로 제공한다.
- `/^\d+$/` 경로와 서버 함수 경로 `/api/*`는 `RAILWAY_ORIGIN`으로 프록시한다.
- 프록시는 `redirect: "manual"`로 요청해 Railway의 302 응답을 따라가지 않고 그대로 브라우저에 전달한다.

## 검토한 대안

- **Pages `_redirects`**: 외부 출처로의 프록시(200 rewrite)를 지원하지 않아 `/단축 번호`를 Railway로 보낼 수 없다.
- **브라우저가 Railway API를 직접 호출**: CORS 설정이 필요하고, 공개 주소의 `/단축 번호`는 여전히 프록시가 필요하다.
- **Cloudflare DNS 프록시로 Railway 앞단 구성**: 소유한 도메인이 필요하다.

## 결과

- 화면과 API가 같은 출처라 CORS가 없다.
- 단축 URL의 기준 주소는 Railway가 아니라 Worker 주소이므로, 서버는 `PUBLIC_BASE_URL` 환경변수로 단축 URL을 조립한다.
- Worker 라우팅 스크립트 한 개를 Dioxus 바깥에 둔다. 화면과 서버 기능 자체는 모두 Dioxus 풀스택 코드에 있다.
