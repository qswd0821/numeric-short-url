//! `/{단축 번호}` 리디렉션과 링크 변경 요청 거부를 처리하는 axum 미들웨어.
//!
//! - `GET`/`HEAD /{숫자}`: 링크가 있으면 302 + `Location: 원본 URL`, 없으면 404 안내 페이지.
//! - 그 밖의 메서드로 `/{숫자}` 또는 `/api/links[/...]` 를 바꾸려는 요청(PUT/PATCH/DELETE 등): 405.
//! - 나머지 경로는 Dioxus 화면/서버 함수 라우터로 넘긴다.

use dioxus::server::axum::{
    body::Body,
    extract::{Request, State},
    http::{header, HeaderValue, Method, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use sqlx::PgPool;

use super::{links, validate};

pub const IMMUTABLE_MESSAGE: &str = "링크는 생성 후 수정·삭제할 수 없습니다.";

pub async fn short_url_guard(State(pool): State<PgPool>, req: Request, next: Next) -> Response {
    let path = req.uri().path();
    let method = req.method().clone();

    if let Some(digits) = numeric_segment(path) {
        return match method {
            Method::GET | Method::HEAD => redirect(&pool, digits).await,
            _ => reject_modification(),
        };
    }

    if is_link_api(path) && !matches!(method, Method::POST | Method::GET | Method::HEAD | Method::OPTIONS) {
        return reject_modification();
    }

    next.run(req).await
}

/// `/123` 처럼 숫자로만 된 단일 경로 세그먼트를 꺼낸다.
fn numeric_segment(path: &str) -> Option<&str> {
    let rest = path.strip_prefix('/')?;
    (!rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit())).then_some(rest)
}

fn is_link_api(path: &str) -> bool {
    path == "/api/links" || path.starts_with("/api/links/")
}

async fn redirect(pool: &PgPool, digits: &str) -> Response {
    // i64 범위를 넘는 숫자는 발급된 적이 없으므로 404.
    let Ok(id) = digits.parse::<i64>() else {
        return not_found();
    };
    match links::find_original_url(pool, id).await {
        Ok(Some(original_url)) => match validate::to_location(&original_url)
            .and_then(|loc| HeaderValue::from_str(&loc).ok())
        {
            Some(location) => {
                let mut res = StatusCode::FOUND.into_response();
                res.headers_mut().insert(header::LOCATION, location);
                res.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
                res
            }
            None => server_error(),
        },
        Ok(None) => not_found(),
        Err(err) => {
            dioxus::logger::tracing::error!("link lookup failed: {err}");
            server_error()
        }
    }
}

fn reject_modification() -> Response {
    let mut res = (StatusCode::METHOD_NOT_ALLOWED, IMMUTABLE_MESSAGE).into_response();
    res.headers_mut().insert(header::ALLOW, HeaderValue::from_static("GET, HEAD"));
    res
}

fn not_found() -> Response {
    html(StatusCode::NOT_FOUND, NOT_FOUND_HTML)
}

fn server_error() -> Response {
    html(StatusCode::INTERNAL_SERVER_ERROR, SERVER_ERROR_HTML)
}

fn html(status: StatusCode, body: &'static str) -> Response {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
        .header(header::CACHE_CONTROL, "no-store")
        .body(Body::from(body))
        .unwrap()
}

const NOT_FOUND_HTML: &str = r#"<!doctype html>
<html lang="ko"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>존재하지 않는 단축 URL</title>
<style>body{font-family:system-ui,sans-serif;max-width:32rem;margin:15vh auto;padding:0 1rem;color:#222}a{color:#2563eb}</style>
</head><body>
<h1>404 · 존재하지 않는 단축 URL입니다</h1>
<p>입력한 단축 번호로 등록된 링크가 없습니다. 주소를 다시 확인해 주세요.</p>
<p><a href="/">새 단축 URL 만들기</a></p>
</body></html>"#;

const SERVER_ERROR_HTML: &str = r#"<!doctype html>
<html lang="ko"><head><meta charset="utf-8"><title>일시적인 오류</title></head>
<body><h1>일시적인 오류가 발생했습니다</h1><p>잠시 후 다시 시도해 주세요.</p><p><a href="/">처음으로</a></p></body></html>"#;

#[cfg(test)]
mod tests {
    use super::*;
    use dioxus::server::axum::{middleware::from_fn_with_state, routing::get, Router};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    fn app(pool: PgPool) -> Router {
        Router::new()
            .route("/", get(|| async { "home" }))
            .fallback(|| async { (StatusCode::IM_A_TEAPOT, "fallthrough") })
            .layer(from_fn_with_state(pool, short_url_guard))
    }

    async fn send(pool: &PgPool, method: Method, uri: &str) -> Response {
        let req = Request::builder().method(method).uri(uri).body(Body::empty()).unwrap();
        app(pool.clone()).oneshot(req).await.unwrap()
    }

    async fn body_text(res: Response) -> String {
        String::from_utf8(res.into_body().collect().await.unwrap().to_bytes().to_vec()).unwrap()
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn redirects_with_302_and_keeps_path_query_fragment(pool: PgPool) {
        let original = "https://example.org/docs/page?lang=ko&x=1#section-2";
        let id = links::insert(&pool, original).await.unwrap();

        let res = send(&pool, Method::GET, &format!("/{id}")).await;
        assert_eq!(res.status(), StatusCode::FOUND);
        assert_eq!(res.headers()[header::LOCATION], original);

        let head = send(&pool, Method::HEAD, &format!("/{id}")).await;
        assert_eq!(head.status(), StatusCode::FOUND);
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn unknown_or_out_of_range_number_is_404_with_guidance(pool: PgPool) {
        for uri in ["/999999", "/0", "/99999999999999999999999"] {
            let res = send(&pool, Method::GET, uri).await;
            assert_eq!(res.status(), StatusCode::NOT_FOUND, "{uri}");
            assert!(body_text(res).await.contains("존재하지 않는 단축 URL"), "{uri}");
        }
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn modification_requests_are_rejected(pool: PgPool) {
        let id = links::insert(&pool, "https://example.org").await.unwrap();
        for method in [Method::PUT, Method::PATCH, Method::DELETE, Method::POST] {
            let res = send(&pool, method.clone(), &format!("/{id}")).await;
            assert_eq!(res.status(), StatusCode::METHOD_NOT_ALLOWED, "{method} /{id}");
        }
        for method in [Method::PUT, Method::PATCH, Method::DELETE] {
            for uri in ["/api/links".to_string(), format!("/api/links/{id}")] {
                let res = send(&pool, method.clone(), &uri).await;
                assert_eq!(res.status(), StatusCode::METHOD_NOT_ALLOWED, "{method} {uri}");
                assert_eq!(body_text(res).await, IMMUTABLE_MESSAGE);
            }
        }
        assert_eq!(links::find_original_url(&pool, id).await.unwrap().as_deref(), Some("https://example.org"));
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn non_numeric_paths_fall_through_to_the_app(pool: PgPool) {
        assert_eq!(send(&pool, Method::GET, "/").await.status(), StatusCode::OK);
        for uri in ["/abc", "/12a", "/12/34", "/favicon.ico"] {
            assert_eq!(send(&pool, Method::GET, uri).await.status(), StatusCode::IM_A_TEAPOT, "{uri}");
        }
    }
}
