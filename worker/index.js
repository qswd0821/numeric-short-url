// Cloudflare Worker: 서비스의 공개 접점.
//
// - `/{단축 번호}` 와 `/api/*` 요청은 Railway의 Dioxus 서버로 프록시한다.
//   `redirect: "manual"` 이므로 Railway가 보낸 302를 따라가지 않고 그대로 브라우저에 전달한다.
// - 그 외 경로(`/`, `/assets/*` 등)는 Dioxus 웹 번들(정적 자산)로 응답한다.
//
// RAILWAY_ORIGIN 은 wrangler.toml 의 [vars] 로 설정한다 (비밀 값 아님).

const PROXIED_PATH = /^\/(\d+|api(\/.*)?)$/;

export default {
  async fetch(request, env) {
    const url = new URL(request.url);
    if (PROXIED_PATH.test(url.pathname)) {
      return proxyToRailway(request, env, url);
    }
    return env.ASSETS.fetch(request);
  },
};

async function proxyToRailway(request, env, url) {
  if (!env.RAILWAY_ORIGIN) {
    return new Response("RAILWAY_ORIGIN is not configured", { status: 500 });
  }

  const target = new URL(url.pathname + url.search, env.RAILWAY_ORIGIN);
  const headers = new Headers(request.headers);
  headers.set("X-Forwarded-Host", url.host);
  headers.set("X-Forwarded-Proto", url.protocol.replace(":", ""));

  const hasBody = !["GET", "HEAD"].includes(request.method);
  return fetch(target, {
    method: request.method,
    headers,
    body: hasBody ? request.body : undefined,
    redirect: "manual",
  });
}
