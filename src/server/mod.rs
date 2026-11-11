//! 서버 전용 코드(Railway에서 실행). `server` feature로만 컴파일된다.

pub mod guard;
pub mod links;
pub mod validate;

use std::sync::OnceLock;

use anyhow::Context;
use dioxus::prelude::ServerFnError;
use sqlx::{postgres::PgPoolOptions, PgPool};

use crate::CreatedLink;

pub struct AppState {
    pub pool: PgPool,
    /// 단축 URL의 기준 주소. 예: `https://numeric-short-url.example.workers.dev`
    pub public_base_url: String,
    /// `public_base_url`의 호스트. 자기 자신을 가리키는 URL을 거부하는 데 쓴다.
    pub public_host: Option<String>,
}

static STATE: OnceLock<AppState> = OnceLock::new();

/// 환경변수를 읽어 DB 풀을 만들고 마이그레이션을 적용한다. 두 번째 호출부터는 기존 상태를 돌려준다.
pub async fn init() -> anyhow::Result<&'static AppState> {
    if let Some(state) = STATE.get() {
        return Ok(state);
    }

    let database_url = std::env::var("DATABASE_URL").context("DATABASE_URL 환경변수가 필요합니다")?;
    let public_base_url = std::env::var("PUBLIC_BASE_URL")
        .context("PUBLIC_BASE_URL 환경변수가 필요합니다 (예: https://numeric-short-url.<계정>.workers.dev)")?
        .trim_end_matches('/')
        .to_owned();
    let public_host = url::Url::parse(&public_base_url)
        .context("PUBLIC_BASE_URL이 올바른 URL이 아닙니다")?
        .host_str()
        .map(str::to_owned);

    let pool = PgPoolOptions::new()
        .max_connections(10)
        .connect_lazy(&database_url)
        .context("DATABASE_URL 형식이 올바르지 않습니다")?;

    if std::env::var("SKIP_MIGRATIONS").as_deref() != Ok("1") {
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .context("DB 마이그레이션 실패")?;
    }

    Ok(STATE.get_or_init(|| AppState { pool, public_base_url, public_host }))
}

fn state() -> Result<&'static AppState, ServerFnError> {
    STATE.get().ok_or_else(|| server_error(500, "서버가 아직 준비되지 않았습니다."))
}

fn server_error(code: u16, message: &str) -> ServerFnError {
    ServerFnError::ServerError { message: message.to_owned(), code, details: None }
}

/// 생성 서버 함수의 실제 구현. 원본 URL만 받고, 단축 번호는 절대 입력받지 않는다.
pub async fn create_link(raw_url: &str) -> Result<CreatedLink, ServerFnError> {
    let state = state()?;
    let original_url = validate::validate_original_url(raw_url, state.public_host.as_deref())
        .map_err(|e| server_error(400, e.message()))?;

    let id = links::insert(&state.pool, &original_url).await.map_err(|err| {
        dioxus::logger::tracing::error!("link insert failed: {err}");
        server_error(500, "일시적인 오류로 단축 URL을 만들지 못했습니다. 잠시 후 다시 시도해 주세요.")
    })?;

    Ok(CreatedLink { id, short_url: format!("{}/{id}", state.public_base_url), original_url })
}
