use subtle::ConstantTimeEq;

/// Hard cap on request bodies (1 MiB): tool calls are small JSON; anything
/// bigger is a misbehaving client.
pub const MAX_BODY_BYTES: u64 = 1_048_576;

/// Verdict for a request's body bound. POST is the only body-carrying MCP
/// method, so it must present a parseable Content-Length within the cap —
/// otherwise a chunked body (no Content-Length) would bypass the limit
/// entirely. GET/DELETE carry no body and pass without a header.
pub enum BodyBound {
    Ok,
    /// Body-carrying method without a Content-Length → 411.
    MissingLength,
    /// Oversize or unparseable Content-Length → 413.
    TooLarge,
}

pub fn body_bound(method: &str, content_length: Option<&str>) -> BodyBound {
    let needs_bound = method.eq_ignore_ascii_case("POST");
    match content_length {
        None if needs_bound => BodyBound::MissingLength,
        None => BodyBound::Ok,
        Some(v) => match v.parse::<u64>() {
            Ok(n) if n <= MAX_BODY_BYTES => BodyBound::Ok,
            _ => BodyBound::TooLarge,
        },
    }
}

/// MCP-spec DNS-rebinding defense: no Origin (CLI clients) or a localhost
/// origin passes; any web origin is rejected before auth work.
pub fn origin_ok(origin: Option<&str>) -> bool {
    let Some(origin) = origin else {
        return true;
    };
    let rest = if let Some(r) = origin.strip_prefix("http://") {
        r
    } else if let Some(r) = origin.strip_prefix("https://") {
        r
    } else {
        return false;
    };
    let host = rest.split('/').next().unwrap_or("");
    // Strip a port; [::1] needs the bracket form kept intact.
    let host = if let Some(h) = host.strip_prefix('[') {
        h.split(']').next().unwrap_or("")
    } else {
        host.split(':').next().unwrap_or("")
    };
    matches!(host, "localhost" | "127.0.0.1" | "::1")
}

/// Constant-time bearer check. An empty configured token never matches —
/// "not yet generated" must not mean "open".
pub fn auth_ok(header: Option<&str>, token: &str) -> bool {
    if token.is_empty() {
        return false;
    }
    let Some(presented) = header.and_then(|h| h.strip_prefix("Bearer ")) else {
        return false;
    };
    presented.as_bytes().ct_eq(token.as_bytes()).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origin_absent_or_localhost_passes_everything_else_fails() {
        assert!(origin_ok(None)); // CLI clients send no Origin
        for ok in [
            "http://localhost",
            "http://localhost:5173",
            "https://localhost:1234",
            "http://127.0.0.1:8080",
            "http://[::1]:9000",
        ] {
            assert!(origin_ok(Some(ok)), "{ok} should pass");
        }
        for bad in [
            "http://evil.test",
            "https://localhost.evil.test",
            "null",
            "file://x",
        ] {
            assert!(!origin_ok(Some(bad)), "{bad} should fail");
        }
    }

    #[test]
    fn auth_requires_the_exact_bearer_token() {
        assert!(auth_ok(Some("Bearer sekret"), "sekret"));
        assert!(!auth_ok(Some("Bearer wrong"), "sekret"));
        assert!(!auth_ok(Some("sekret"), "sekret")); // scheme required
        assert!(!auth_ok(None, "sekret"));
        assert!(!auth_ok(Some("Bearer "), "sekret"));
        assert!(!auth_ok(Some("Bearer sekret"), "")); // empty token never matches
    }

    #[test]
    fn post_bodies_must_carry_a_parseable_in_cap_content_length() {
        // A chunked POST (no Content-Length) must NOT bypass the cap.
        assert!(matches!(body_bound("POST", None), BodyBound::MissingLength));
        assert!(matches!(body_bound("post", None), BodyBound::MissingLength));
        assert!(matches!(body_bound("POST", Some("1048576")), BodyBound::Ok));
        assert!(matches!(
            body_bound("POST", Some("1048577")),
            BodyBound::TooLarge
        ));
        assert!(matches!(
            body_bound("POST", Some("not-a-number")),
            BodyBound::TooLarge
        ));
        // GET (the SSE stream) and DELETE carry no body — no length required.
        assert!(matches!(body_bound("GET", None), BodyBound::Ok));
        assert!(matches!(body_bound("DELETE", None), BodyBound::Ok));
    }
}
