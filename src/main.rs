//! 숫자형 단축 URL 서비스 (Dioxus 풀스택).
//!
//! - `web` feature: 브라우저 화면 (Cloudflare Workers 정적 자산으로 배포)
//! - `server` feature: SSR + 서버 함수 + `/{단축 번호}` 리디렉션 (Railway)

use dioxus::prelude::*;
use serde::{Deserialize, Serialize};

#[cfg(feature = "server")]
mod server;

const MAIN_CSS: Asset = asset!("/assets/main.css");

/// 생성 결과. 화면에 보여줄 단축 URL을 서버가 `PUBLIC_BASE_URL`로 조립해 돌려준다.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CreatedLink {
    pub id: i64,
    pub short_url: String,
    pub original_url: String,
}

/// 링크 생성 서버 함수 (`POST /api/links`). 로그인 없이 누구나 호출할 수 있다.
/// 입력은 원본 URL 하나뿐이며 단축 번호는 서버(DB)가 발급한다.
#[post("/api/links")]
async fn create_link(original_url: String) -> Result<CreatedLink, ServerFnError> {
    server::create_link(&original_url).await
}

fn main() {
    #[cfg(not(feature = "server"))]
    dioxus::launch(App);

    #[cfg(feature = "server")]
    dioxus::serve(|| async move {
        use dioxus::server::axum::middleware::from_fn_with_state;

        let state = server::init().await?;
        Ok(dioxus::server::router(App)
            .layer(from_fn_with_state(state.pool.clone(), server::guard::short_url_guard)))
    });
}

#[component]
fn App() -> Element {
    let mut input = use_signal(String::new);
    let mut created = use_signal(|| None::<CreatedLink>);
    let mut error = use_signal(|| None::<String>);
    let mut pending = use_signal(|| false);
    let mut copy_state = use_signal(|| CopyState::Idle);

    let submit = move |evt: FormEvent| async move {
        evt.prevent_default();
        if pending() {
            return;
        }
        pending.set(true);
        error.set(None);
        copy_state.set(CopyState::Idle);

        match create_link(input()).await {
            Ok(link) => {
                created.set(Some(link));
                input.set(String::new());
            }
            Err(err) => {
                created.set(None);
                error.set(Some(user_message(err)));
            }
        }
        pending.set(false);
    };

    let copy = move |_| async move {
        let Some(link) = created() else { return };
        let eval = document::eval(
            r#"
            const text = await dioxus.recv();
            try {
                await navigator.clipboard.writeText(text);
                return true;
            } catch (_) {
                const el = document.getElementById("short-url");
                if (el) { el.focus(); el.select(); }
                return false;
            }
            "#,
        );
        let _ = eval.send(link.short_url);
        let copied = eval.join::<bool>().await.unwrap_or(false);
        copy_state.set(if copied { CopyState::Copied } else { CopyState::SelectManually });
    };

    rsx! {
        document::Title { "숫자형 단축 URL" }
        document::Stylesheet { href: MAIN_CSS }

        main { class: "container",
            h1 { "숫자형 단축 URL" }
            p { class: "lead", "긴 URL을 입력하면 숫자로 된 짧은 주소를 만들어 드립니다. 로그인은 필요 없습니다." }

            form { class: "create-form", onsubmit: submit,
                label { r#for: "original-url", class: "sr-only", "원본 URL" }
                input {
                    id: "original-url",
                    r#type: "text",
                    inputmode: "url",
                    autocomplete: "off",
                    placeholder: "https://example.org/아주/긴/주소?query=1#section",
                    value: "{input}",
                    oninput: move |e| input.set(e.value()),
                    disabled: pending(),
                }
                button { r#type: "submit", disabled: pending() || input().trim().is_empty(),
                    if pending() { "만드는 중…" } else { "단축하기" }
                }
            }

            if let Some(message) = error() {
                p { class: "error", role: "alert", "{message}" }
            }

            if let Some(link) = created() {
                section { class: "result", aria_live: "polite",
                    label { r#for: "short-url", "단축 URL" }
                    div { class: "result-row",
                        input { id: "short-url", r#type: "text", readonly: true, value: "{link.short_url}" }
                        button { r#type: "button", onclick: copy, "복사" }
                    }
                    match copy_state() {
                        CopyState::Idle => rsx! {},
                        CopyState::Copied => rsx! { p { class: "hint ok", "복사됨" } },
                        CopyState::SelectManually => rsx! {
                            p { class: "hint", "자동 복사를 사용할 수 없습니다. 선택된 주소를 Ctrl+C로 복사하세요." }
                        },
                    }
                    p { class: "original", "원본: " a { href: "{link.original_url}", rel: "noopener noreferrer", "{link.original_url}" } }
                }
            }

            footer { "한 번 만든 단축 URL은 수정·삭제·만료되지 않습니다." }
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum CopyState {
    Idle,
    Copied,
    SelectManually,
}

fn user_message(err: ServerFnError) -> String {
    match err {
        ServerFnError::ServerError { message, .. } => message,
        _ => "서버에 연결하지 못했습니다. 잠시 후 다시 시도해 주세요.".to_owned(),
    }
}
