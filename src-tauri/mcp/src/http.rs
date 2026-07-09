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

use std::sync::Arc;

use axum::extract::State;
use axum::http::{header, HeaderMap, StatusCode};
use axum::middleware::{self, Next};
use axum::response::Response;
use rmcp::transport::streamable_http_server::{
    session::local::LocalSessionManager, StreamableHttpServerConfig, StreamableHttpService,
};
use tokio_util::sync::CancellationToken;

use crate::service::{Deps, VaultBuddyMcp};

#[derive(Clone)]
struct Guard {
    token: Arc<String>,
}

fn header_str(headers: &HeaderMap, name: header::HeaderName) -> Option<&str> {
    headers.get(name).and_then(|v| v.to_str().ok())
}

async fn guard(
    State(g): State<Guard>,
    req: axum::extract::Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let headers = req.headers();
    if !origin_ok(header_str(headers, header::ORIGIN)) {
        return Err(StatusCode::FORBIDDEN);
    }
    if !auth_ok(header_str(headers, header::AUTHORIZATION), &g.token) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    match body_bound(
        req.method().as_str(),
        header_str(headers, header::CONTENT_LENGTH),
    ) {
        BodyBound::Ok => {}
        BodyBound::MissingLength => return Err(StatusCode::LENGTH_REQUIRED),
        BodyBound::TooLarge => return Err(StatusCode::PAYLOAD_TOO_LARGE),
    }
    Ok(next.run(req).await)
}

/// How long in-flight requests get after a shutdown request before the
/// listener is closed by force. Bounds `RunningServer::stop()` by
/// construction — the disable/regenerate path must be able to PROVE the old
/// socket (and old token) are gone before reporting success.
const DRAIN_GRACE: std::time::Duration = std::time::Duration::from_secs(3);

/// A live server: the bound port plus the handles to stop it. Dropping
/// without `stop()` leaves the thread running until process exit — fine for
/// app shutdown (the OS reclaims the listener), wrong for a settings change.
pub struct RunningServer {
    pub port: u16,
    cancel: CancellationToken,
    join: std::thread::JoinHandle<()>,
}

impl RunningServer {
    /// Cancel + join. The join is bounded by DRAIN_GRACE by construction
    /// (the runner force-closes after the drain), so this returns promptly
    /// and only once the listener is actually released.
    pub fn stop(self) {
        self.cancel.cancel();
        if self.join.join().is_err() {
            log::error!("mcp-server thread panicked during shutdown");
        }
    }
}

/// Bind 127.0.0.1:`port` (0 = ephemeral) and serve MCP on a dedicated named
/// thread with its own current-thread tokio runtime. Returns only after the
/// bind outcome is known, so "port already in use" is a synchronous,
/// user-visible error — never a silently dead server.
pub fn start(deps: Deps, port: u16, token: String) -> Result<RunningServer, String> {
    let cancel = CancellationToken::new();
    let ct = cancel.clone();
    // std channel: the caller is synchronous (a Tauri command / setup), the
    // sender is inside the runtime — a oneshot over threads.
    let (bind_tx, bind_rx) = std::sync::mpsc::channel::<Result<u16, String>>();

    let join = std::thread::Builder::new()
        .name("mcp-server".into())
        .spawn(move || {
            let rt = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(e) => {
                    let _ = bind_tx.send(Err(format!("tokio runtime: {e}")));
                    return;
                }
            };
            rt.block_on(async move {
                let listener = match tokio::net::TcpListener::bind(("127.0.0.1", port)).await {
                    Ok(l) => l,
                    Err(e) => {
                        let _ = bind_tx.send(Err(format!("could not bind 127.0.0.1:{port}: {e}")));
                        return;
                    }
                };
                let actual = match listener.local_addr() {
                    Ok(a) => a.port(),
                    Err(e) => {
                        let _ = bind_tx.send(Err(format!("local_addr: {e}")));
                        return;
                    }
                };
                let service = StreamableHttpService::new(
                    move || Ok(VaultBuddyMcp::new(deps.clone())),
                    LocalSessionManager::default().into(),
                    StreamableHttpServerConfig::default().with_cancellation_token(ct.child_token()),
                );
                let router = axum::Router::new().nest_service("/mcp", service).layer(
                    middleware::from_fn_with_state(
                        Guard {
                            token: Arc::new(token),
                        },
                        guard,
                    ),
                );
                let _ = bind_tx.send(Ok(actual));
                log::info!("mcp: serving on 127.0.0.1:{actual}/mcp");
                let shutdown = ct.clone();
                let serve = axum::serve(listener, router)
                    .with_graceful_shutdown(async move { shutdown.cancelled().await });
                let forced = ct.clone();
                tokio::select! {
                    result = serve => {
                        if let Err(e) = result {
                            log::error!("mcp: server exited with error: {e}");
                        }
                    }
                    _ = async {
                        forced.cancelled().await;
                        tokio::time::sleep(DRAIN_GRACE).await;
                    } => {
                        // Dropping the serve future hard-closes the listener and
                        // every connection. A client pinning an SSE stream open
                        // must not keep the old endpoint (and old token) alive
                        // after the UI reports the server stopped.
                        log::warn!("mcp: graceful drain timed out; forcing close");
                    }
                }
                log::info!("mcp: server stopped");
            });
        })
        .map_err(|e| format!("could not spawn mcp-server thread: {e}"))?;

    match bind_rx.recv_timeout(std::time::Duration::from_secs(10)) {
        Ok(Ok(port)) => Ok(RunningServer { port, cancel, join }),
        Ok(Err(e)) => {
            // The thread sent its error and is exiting — a plain join is prompt.
            let _ = join.join();
            Err(e)
        }
        Err(_) => {
            // The thread is delayed, not necessarily dead (Codex review catch):
            // without this, it could finish binding LATER and serve as an
            // orphan no RunningServer handle can ever stop. Cancel now — the
            // serve stage sees an already-cancelled token and shuts straight
            // down (bounded by DRAIN_GRACE) — and reap the join on a named
            // helper so this error path returns promptly instead of blocking
            // on a wedged thread.
            cancel.cancel();
            let reap = std::thread::Builder::new()
                .name("mcp-server-reaper".into())
                .spawn(move || {
                    if join.join().is_err() {
                        log::error!("mcp-server thread panicked after bind-report timeout");
                    }
                });
            if let Err(e) = reap {
                log::warn!("could not spawn mcp-server-reaper: {e}");
            }
            Err("mcp-server did not report its bind status within 10s".to_string())
        }
    }
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
