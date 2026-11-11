//! 원본 URL 서버 검증.

use url::Url;

pub const MAX_ORIGINAL_URL_LEN: usize = 2048;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvalidUrl {
    Empty,
    TooLong,
    Malformed,
    UnsupportedScheme,
    MissingHost,
    SelfReference,
}

impl InvalidUrl {
    pub fn message(self) -> &'static str {
        match self {
            InvalidUrl::Empty => "URL을 입력하세요.",
            InvalidUrl::TooLong => "URL은 2048자 이하여야 합니다.",
            InvalidUrl::Malformed | InvalidUrl::UnsupportedScheme | InvalidUrl::MissingHost => {
                "http:// 또는 https:// 로 시작하는 올바른 URL을 입력하세요."
            }
            InvalidUrl::SelfReference => "이 서비스의 단축 URL은 다시 단축할 수 없습니다.",
        }
    }
}

/// 입력값을 검증하고, 앞뒤 공백만 제거한 원본 URL을 돌려준다(그 외 정규화 없음).
///
/// `service_host`가 주어지면 그 호스트를 가리키는 URL은 리디렉션 루프를 막기 위해 거부한다.
pub fn validate_original_url(raw: &str, service_host: Option<&str>) -> Result<String, InvalidUrl> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(InvalidUrl::Empty);
    }
    if trimmed.chars().count() > MAX_ORIGINAL_URL_LEN {
        return Err(InvalidUrl::TooLong);
    }
    if trimmed.chars().any(char::is_control) || trimmed.contains(' ') {
        return Err(InvalidUrl::Malformed);
    }

    let parsed = Url::parse(trimmed).map_err(|_| InvalidUrl::Malformed)?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(InvalidUrl::UnsupportedScheme);
    }
    let host = match parsed.host_str() {
        Some(h) if !h.is_empty() => h,
        _ => return Err(InvalidUrl::MissingHost),
    };
    if let Some(service_host) = service_host {
        if host.eq_ignore_ascii_case(service_host) {
            return Err(InvalidUrl::SelfReference);
        }
    }

    Ok(trimmed.to_owned())
}

/// 저장된 원본 URL을 `Location` 헤더에 넣을 수 있는 형태로 만든다.
/// 경로·쿼리·프래그먼트는 그대로 유지되고, 비 ASCII 문자만 퍼센트 인코딩된다.
pub fn to_location(original_url: &str) -> Option<String> {
    Url::parse(original_url).ok().map(String::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_http_and_https_and_keeps_input_as_is() {
        assert_eq!(validate_original_url("https://example.org", None).unwrap(), "https://example.org");
        assert_eq!(
            validate_original_url("  http://a.com/x/y?q=1&r=2#frag  ", None).unwrap(),
            "http://a.com/x/y?q=1&r=2#frag"
        );
    }

    #[test]
    fn rejects_empty_and_blank() {
        assert_eq!(validate_original_url("", None), Err(InvalidUrl::Empty));
        assert_eq!(validate_original_url("   ", None), Err(InvalidUrl::Empty));
    }

    #[test]
    fn rejects_other_schemes() {
        for raw in ["ftp://a.com", "javascript:alert(1)", "mailto:a@b.com", "file:///etc/passwd", "data:text/html,hi"] {
            assert_eq!(validate_original_url(raw, None), Err(InvalidUrl::UnsupportedScheme), "{raw}");
        }
    }

    #[test]
    fn rejects_malformed() {
        for raw in ["abc", "example.org", "http//a.com", "https://exa mple.org", "http://"] {
            assert!(validate_original_url(raw, None).is_err(), "{raw}");
        }
    }

    #[test]
    fn rejects_too_long() {
        let ok = format!("https://a.com/{}", "x".repeat(MAX_ORIGINAL_URL_LEN - "https://a.com/".len()));
        assert!(validate_original_url(&ok, None).is_ok());
        let too_long = format!("{ok}x");
        assert_eq!(validate_original_url(&too_long, None), Err(InvalidUrl::TooLong));
    }

    #[test]
    fn rejects_service_own_host() {
        assert_eq!(
            validate_original_url("https://Short.Example.workers.dev/12", Some("short.example.workers.dev")),
            Err(InvalidUrl::SelfReference)
        );
        assert!(validate_original_url("https://other.dev/12", Some("short.example.workers.dev")).is_ok());
    }

    #[test]
    fn location_keeps_path_query_fragment() {
        assert_eq!(
            to_location("https://a.com/p/q?x=1&y=%20#sec-2").unwrap(),
            "https://a.com/p/q?x=1&y=%20#sec-2"
        );
        assert_eq!(to_location("https://a.com/한글?q=값#끝").unwrap(), "https://a.com/%ED%95%9C%EA%B8%80?q=%EA%B0%92#%EB%81%9D");
    }
}
