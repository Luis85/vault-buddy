//! Client-agnostic spec-level validation: a real MCP client (rmcp's own,
//! co-versioned with the server) drives initialize → tools/list → tools/call
//! over streamable HTTP against a temp-dir vault, and the task file actually
//! lands on disk.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use rmcp::model::{CallToolRequestParams, ClientCapabilities, ClientInfo, Implementation};
use rmcp::transport::StreamableHttpClientTransport;
use rmcp::ServiceExt;
use vault_buddy_mcp::{start, Deps};

const TOKEN: &str = "test-token-test-token-test-token-test-token";

fn fixture_deps(dir: &std::path::Path, allow_writes: bool) -> Deps {
    let vault = dir.join("MyVault");
    std::fs::create_dir_all(&vault).unwrap();
    let obsidian_json = dir.join("obsidian.json");
    std::fs::write(
        &obsidian_json,
        serde_json::json!({
            "vaults": { "deadbeef01234567": { "path": vault.to_string_lossy() } }
        })
        .to_string(),
    )
    .unwrap();
    let config_json = dir.join("config.json");
    std::fs::write(&config_json, "{}").unwrap();
    Deps {
        paths: vault_buddy_core::services::ServicePaths {
            obsidian_json: Some(obsidian_json),
            config_json: Some(config_json),
        },
        app_version: "0.0.0-test".to_string(),
        allow_writes: Arc::new(AtomicBool::new(allow_writes)),
        launch: Arc::new(|_uri: &str| Ok(())),
        on_write: Arc::new(|_ev| {}),
    }
}

fn authed_http_client() -> reqwest::Client {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        reqwest::header::AUTHORIZATION,
        format!("Bearer {TOKEN}").parse().unwrap(),
    );
    reqwest::Client::builder()
        .default_headers(headers)
        .build()
        .unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn full_round_trip_with_writes_enabled() {
    let dir = tempfile::tempdir().unwrap();
    let server = start(fixture_deps(dir.path(), true), 0, TOKEN.to_string()).unwrap();
    let url = format!("http://127.0.0.1:{}/mcp", server.port);

    let transport = StreamableHttpClientTransport::with_client(
        authed_http_client(),
        rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig::with_uri(
            url.clone(),
        ),
    );
    let client_info = ClientInfo::new(
        ClientCapabilities::default(),
        Implementation::new("roundtrip-test", "0.0.0"),
    );
    let client = client_info.serve(transport).await.expect("initialize");

    let tools = client.list_tools(Default::default()).await.unwrap();
    let mut names: Vec<String> = tools.tools.iter().map(|t| t.name.to_string()).collect();
    names.sort();
    assert_eq!(
        names,
        [
            "add_task",
            "list_recordings",
            "list_tasks",
            "list_vaults",
            "open_daily_note",
            "open_vault",
            "set_task_status"
        ]
    );

    let result = client
        .call_tool(
            CallToolRequestParams::new("add_task").with_arguments(
                serde_json::json!({ "vaultId": "deadbeef01234567", "title": "Buy milk" })
                    .as_object()
                    .cloned()
                    .unwrap(),
            ),
        )
        .await
        .unwrap();
    assert_ne!(result.is_error, Some(true), "{result:?}");

    // The write is REAL: a task document exists in the temp vault.
    let tasks_dir = dir.path().join("MyVault/Tasks");
    let files: Vec<_> = std::fs::read_dir(&tasks_dir).unwrap().collect();
    assert_eq!(files.len(), 1);

    client.cancel().await.unwrap();
    server.stop();
}

#[tokio::test(flavor = "multi_thread")]
async fn writes_off_hides_and_rejects_write_tools() {
    let dir = tempfile::tempdir().unwrap();
    let server = start(fixture_deps(dir.path(), false), 0, TOKEN.to_string()).unwrap();
    let url = format!("http://127.0.0.1:{}/mcp", server.port);

    let transport = StreamableHttpClientTransport::with_client(
        authed_http_client(),
        rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig::with_uri(
            url.clone(),
        ),
    );
    let client_info = ClientInfo::new(
        ClientCapabilities::default(),
        Implementation::new("roundtrip-test", "0.0.0"),
    );
    let client = client_info.serve(transport).await.expect("initialize");

    let tools = client.list_tools(Default::default()).await.unwrap();
    let names: Vec<String> = tools.tools.iter().map(|t| t.name.to_string()).collect();
    assert!(!names.contains(&"add_task".to_string()), "{names:?}");
    assert!(names.contains(&"list_vaults".to_string()));

    // Codex review catch: hidden-from-the-router must NOT mean silent. A
    // client that calls a write tool anyway (cached name from before a
    // revocation + reconnect, or a manual call) must get the audited
    // WRITES_DISABLED TOOL error — not rmcp's generic unknown-tool protocol
    // error, which carries no explanation and produces no audit line.
    let result = client
        .call_tool(
            CallToolRequestParams::new("add_task").with_arguments(
                serde_json::json!({ "vaultId": "deadbeef01234567", "title": "Nope" })
                    .as_object()
                    .cloned()
                    .unwrap(),
            ),
        )
        .await
        .expect("denial must be a tool error, not a protocol unknown-tool error");
    assert_eq!(result.is_error, Some(true));
    let text = serde_json::to_string(&result.content).unwrap_or_default();
    assert!(
        text.contains("Vault writes are disabled in Vault Buddy settings."),
        "{text}"
    );
    assert!(
        std::fs::read_dir(dir.path().join("MyVault"))
            .unwrap()
            .next()
            .is_none(),
        "the denied call must not have written anything"
    );

    // The denial path must not depend on parseable arguments (there is no
    // vaultId to audit — it falls back to "-"): still the same tool error.
    let result = client
        .call_tool(CallToolRequestParams::new("set_task_status"))
        .await
        .expect("argument-free denial must still be a tool error");
    assert_eq!(result.is_error, Some(true));
    let text = serde_json::to_string(&result.content).unwrap_or_default();
    assert!(
        text.contains("Vault writes are disabled in Vault Buddy settings."),
        "{text}"
    );

    // A genuinely unknown tool stays the router's unknown-tool protocol
    // error — the write-tool intercept must not over-match.
    let err = client
        .call_tool(CallToolRequestParams::new("does_not_exist"))
        .await;
    assert!(
        err.is_err(),
        "unknown tools must keep failing as protocol errors, got: {err:?}"
    );

    client.cancel().await.unwrap();
    server.stop();
}

#[tokio::test(flavor = "multi_thread")]
async fn requests_without_the_token_or_with_an_evil_origin_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let server = start(fixture_deps(dir.path(), false), 0, TOKEN.to_string()).unwrap();
    let url = format!("http://127.0.0.1:{}/mcp", server.port);
    let body = serde_json::json!({ "jsonrpc": "2.0", "id": 1, "method": "ping" });

    let plain = reqwest::Client::new();
    let resp = plain.post(&url).json(&body).send().await.unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::UNAUTHORIZED);

    let resp = plain
        .post(&url)
        .header("Authorization", format!("Bearer {TOKEN}"))
        .header("Origin", "http://evil.test")
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::FORBIDDEN);

    // A chunked POST (no Content-Length) must not bypass the body cap.
    let chunks: Vec<Result<bytes::Bytes, std::io::Error>> =
        vec![Ok(bytes::Bytes::from_static(b"{}"))];
    let resp = plain
        .post(&url)
        .header("Authorization", format!("Bearer {TOKEN}"))
        .header("Content-Type", "application/json")
        .body(reqwest::Body::wrap_stream(futures_util::stream::iter(
            chunks,
        )))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::LENGTH_REQUIRED);

    server.stop();
}

#[tokio::test(flavor = "multi_thread")]
async fn cors_preflight_and_response_headers_for_localhost_browser_clients() {
    // Codex catch: a browser-hosted localhost MCP client (the spec's MCP
    // Inspector validation target) sends an OPTIONS CORS preflight WITHOUT
    // the Authorization header — browsers never attach credentials to
    // preflights — so auth-before-CORS 401'd the preflight and the real POST
    // never happened, even though origin_ok explicitly allows localhost
    // origins. The guard now answers preflights for origins origin_ok
    // admits — and ONLY those (an evil origin still 403s before the CORS
    // arm) — and reflects ACAO + exposes Mcp-Session-Id on actual responses
    // so browser JS can read the session id and continue. Narrows nothing,
    // opens nothing: the bearer token stays required on every real request.
    let dir = tempfile::tempdir().unwrap();
    let server = start(fixture_deps(dir.path(), false), 0, TOKEN.to_string()).unwrap();
    let url = format!("http://127.0.0.1:{}/mcp", server.port);
    let plain = reqwest::Client::new();

    // 1. Preflight with a localhost Origin and NO auth → 204 + CORS grant.
    let resp = plain
        .request(reqwest::Method::OPTIONS, &url)
        .header("Origin", "http://localhost:6274")
        .header("Access-Control-Request-Method", "POST")
        .header(
            "Access-Control-Request-Headers",
            "authorization, content-type",
        )
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::NO_CONTENT);
    let h = resp.headers();
    assert_eq!(
        h.get("access-control-allow-origin").expect("ACAO"),
        "http://localhost:6274"
    );
    assert_eq!(
        h.get("access-control-allow-methods").expect("methods"),
        "GET, POST, DELETE"
    );
    assert_eq!(
        h.get("access-control-allow-headers").expect("headers"),
        "authorization, content-type, mcp-session-id, mcp-protocol-version, last-event-id"
    );
    assert_eq!(h.get("access-control-max-age").expect("max-age"), "86400");

    // 2. Preflight with an evil Origin → 403: the origin gate fires before
    //    the CORS arm, so a hostile page never even gets a preflight answer.
    let resp = plain
        .request(reqwest::Method::OPTIONS, &url)
        .header("Origin", "http://evil.test")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::FORBIDDEN);

    // 3. A real POST with a localhost Origin + valid auth → ACAO reflected
    //    and Mcp-Session-Id exposed, or browser JS could not continue the
    //    session it just initialized.
    let init = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2025-06-18",
            "capabilities": {},
            "clientInfo": { "name": "cors-test", "version": "0.0.0" }
        }
    });
    let resp = plain
        .post(&url)
        .header("Authorization", format!("Bearer {TOKEN}"))
        .header("Origin", "http://localhost:6274")
        .header("Accept", "application/json, text/event-stream")
        .json(&init)
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success(), "initialize: {}", resp.status());
    assert_eq!(
        resp.headers()
            .get("access-control-allow-origin")
            .expect("ACAO on the real response"),
        "http://localhost:6274"
    );
    assert_eq!(
        resp.headers()
            .get("access-control-expose-headers")
            .expect("expose-headers"),
        "mcp-session-id"
    );
    assert!(resp.headers().get("mcp-session-id").is_some());

    // 4. No Origin (a non-browser client) → no CORS headers at all; the
    //    non-browser path is byte-for-byte unchanged.
    let resp = plain
        .post(&url)
        .header("Authorization", format!("Bearer {TOKEN}"))
        .header("Accept", "application/json, text/event-stream")
        .json(&init)
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success(), "initialize: {}", resp.status());
    assert!(resp.headers().get("access-control-allow-origin").is_none());
    assert!(resp
        .headers()
        .get("access-control-expose-headers")
        .is_none());

    server.stop();
}

#[tokio::test(flavor = "multi_thread")]
async fn stop_closes_the_listener_even_with_a_pinned_open_stream() {
    // Codex review catch: a client holding a streamable-HTTP stream open must
    // not keep the old endpoint (and old token) alive past a disable/restart.
    let dir = tempfile::tempdir().unwrap();
    let server = start(fixture_deps(dir.path(), false), 0, TOKEN.to_string()).unwrap();
    let port = server.port;
    let url = format!("http://127.0.0.1:{port}/mcp");
    // Pin a GET (rmcp's standalone SSE notification stream) and never read it
    // to completion; if the pinned GET is refused sessionless (4xx), holding
    // the response still exercises an open connection.
    let client = authed_http_client();
    let _pinned = client
        .get(&url)
        .header("Accept", "text/event-stream")
        .send()
        .await;
    let started = std::time::Instant::now();
    tokio::task::spawn_blocking(move || server.stop())
        .await
        .unwrap();
    assert!(
        started.elapsed() < std::time::Duration::from_secs(8),
        "stop() must be bounded by the drain grace"
    );
    // The port must actually be free again — the old listener is gone.
    let rebind = tokio::net::TcpListener::bind(("127.0.0.1", port)).await;
    assert!(rebind.is_ok(), "old listener still owns the port");
}

#[tokio::test(flavor = "multi_thread")]
async fn stop_terminates_a_session_bound_sse_stream_within_the_drain_bound() {
    // Reviewer follow-up to the pinned-GET test above, which may be refused
    // sessionless before it is ever served: this one pins a stream the server
    // GENUINELY serves — the session-bound standalone SSE notification stream,
    // reached through a full initialize handshake — and asserts the property
    // that matters for disable/regenerate: stop() is bounded, the port is
    // freed, AND the pinned stream itself dies, so a stale connection cannot
    // keep honoring the old token. Two layers enforce that death (cancel ends
    // rmcp's SSE bodies via take_until — the normal path; the per-`start`
    // runtime teardown kills any straggler — see the shutdown comment in
    // http.rs). This test pins the PROPERTY rather than either mechanism, so
    // a refactor that breaks the guarantee (e.g. a shared runtime plus
    // streams no longer tied to the token) fails here instead of shipping.
    let dir = tempfile::tempdir().unwrap();
    let server = start(fixture_deps(dir.path(), false), 0, TOKEN.to_string()).unwrap();
    let port = server.port;
    let url = format!("http://127.0.0.1:{port}/mcp");
    let client = authed_http_client();

    // Raw JSON-RPC handshake, shapes as rmcp's own client sends them: the
    // initialize response carries the session id in the Mcp-Session-Id header
    // (rmcp handle_post), and notifications/initialized completes the session.
    let init = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2025-06-18",
            "capabilities": {},
            "clientInfo": { "name": "pin-test", "version": "0.0.0" }
        }
    });
    let resp = client
        .post(&url)
        .header("Accept", "application/json, text/event-stream")
        .json(&init)
        .send()
        .await
        .unwrap();
    assert!(
        resp.status().is_success(),
        "initialize failed: {}",
        resp.status()
    );
    let session_id = resp
        .headers()
        .get("Mcp-Session-Id")
        .expect("initialize response carries a session id")
        .to_str()
        .unwrap()
        .to_string();
    drop(resp); // request-scoped response stream; the session outlives it

    let resp = client
        .post(&url)
        .header("Accept", "application/json, text/event-stream")
        .header("Mcp-Session-Id", &session_id)
        .json(&serde_json::json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::ACCEPTED);

    // The session-bound standalone SSE stream: rmcp serves this GET and holds
    // it open indefinitely (15s keep-alive pings between events).
    let mut pinned = client
        .get(&url)
        .header("Accept", "text/event-stream")
        .header("Mcp-Session-Id", &session_id)
        .send()
        .await
        .unwrap();
    assert_eq!(
        pinned.status(),
        reqwest::StatusCode::OK,
        "the pinned GET must be genuinely served, not refused"
    );
    // Prove the stream is LIVE before stop: rmcp primes standalone streams
    // with an immediate SSE retry event, so a first chunk arrives promptly.
    let first = tokio::time::timeout(std::time::Duration::from_secs(5), pinned.chunk())
        .await
        .expect("pinned stream must be readable before stop()")
        .unwrap();
    assert!(first.is_some(), "expected the SSE priming event");

    let started = std::time::Instant::now();
    tokio::task::spawn_blocking(move || server.stop())
        .await
        .unwrap();
    assert!(
        started.elapsed() < std::time::Duration::from_secs(8),
        "stop() must be bounded by the drain grace"
    );
    let rebind = tokio::net::TcpListener::bind(("127.0.0.1", port)).await;
    assert!(rebind.is_ok(), "old listener still owns the port");
    // The pinned stream must DIE, not linger: after stop() the connection is
    // gone (its task was torn down with the runtime), so reading reaches
    // end-of-body or a connection error within the bound — it must not block
    // on a socket that would still be serving under the old token.
    let terminated = tokio::time::timeout(std::time::Duration::from_secs(8), async {
        loop {
            match pinned.chunk().await {
                Ok(Some(_)) => continue, // drain whatever was in flight
                Ok(None) => break,       // clean end of body
                Err(_) => break,         // connection reset/aborted — dead too
            }
        }
    })
    .await;
    assert!(
        terminated.is_ok(),
        "pinned session stream still alive after stop() — a stale connection would keep honoring the old token"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn stop_does_not_wait_for_in_flight_blocking_tool_work() {
    // Final-review catch (executor starvation): tool handlers used to run
    // their synchronous work — registry reads, walks, fsync writes, the
    // launch closure — INLINE on the runtime's only thread. While any of it
    // ran, nothing polled: the drain select couldn't see a cancel, and
    // stop() was bounded by (slowest vault I/O + grace) instead of by
    // construction. Handlers now offload that work to the blocking pool and
    // teardown is shutdown_timeout-bounded, so stop() must return within
    // DRAIN_GRACE + SHUTDOWN_TIMEOUT + slack even while a tool call is stuck
    // inside a launch that outlives them both — the wedged task is LEAKED
    // (documented decision in http.rs), never awaited.
    let dir = tempfile::tempdir().unwrap();
    let mut deps = fixture_deps(dir.path(), false);
    let launch_entered = Arc::new(AtomicBool::new(false));
    let entered = launch_entered.clone();
    // Sleeps longer than DRAIN_GRACE (3s) + SHUTDOWN_TIMEOUT (2s) combined —
    // and longer than the stop() assert bound below, so a stop() that waits
    // for tool-work completion cannot sneak under it.
    deps.launch = Arc::new(move |_uri: &str| {
        entered.store(true, Ordering::SeqCst);
        std::thread::sleep(std::time::Duration::from_secs(12));
        Ok(())
    });
    let server = start(deps, 0, TOKEN.to_string()).unwrap();
    let port = server.port;
    let url = format!("http://127.0.0.1:{port}/mcp");

    let transport = StreamableHttpClientTransport::with_client(
        authed_http_client(),
        rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig::with_uri(url),
    );
    let client_info = ClientInfo::new(
        ClientCapabilities::default(),
        Implementation::new("starvation-test", "0.0.0"),
    );
    let client = client_info.serve(transport).await.expect("initialize");
    // Fire open_vault but do NOT await its completion — the call sits inside
    // the sleeping launch closure server-side for the rest of the test.
    let call = tokio::spawn(async move {
        let _ = client
            .call_tool(
                CallToolRequestParams::new("open_vault").with_arguments(
                    serde_json::json!({ "vaultId": "deadbeef01234567" })
                        .as_object()
                        .cloned()
                        .unwrap(),
                ),
            )
            .await;
        client // keep the session alive for the duration of the call
    });
    // Wait until the launch closure has actually STARTED — the in-flight
    // state this test is about — bounded so a broken path can't hang the
    // suite.
    let entered_deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !launch_entered.load(Ordering::SeqCst) {
        assert!(
            std::time::Instant::now() < entered_deadline,
            "open_vault never reached the launch closure"
        );
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }

    let started = std::time::Instant::now();
    tokio::task::spawn_blocking(move || server.stop())
        .await
        .unwrap();
    // Derived bound: DRAIN_GRACE (3s) + SHUTDOWN_TIMEOUT (2s) + slack = 8s.
    // The launch sleeps 12s, so passing PROVES stop() is independent of tool
    // work completing.
    assert!(
        started.elapsed() < std::time::Duration::from_secs(8),
        "stop() waited on in-flight blocking tool work ({} ms)",
        started.elapsed().as_millis()
    );
    let rebind = tokio::net::TcpListener::bind(("127.0.0.1", port)).await;
    assert!(rebind.is_ok(), "old listener still owns the port");
    call.abort();
}

#[test]
fn a_taken_port_is_a_synchronous_error() {
    let dir = tempfile::tempdir().unwrap();
    let a = start(fixture_deps(dir.path(), false), 0, TOKEN.to_string()).unwrap();
    let err = start(fixture_deps(dir.path(), false), a.port, TOKEN.to_string());
    assert!(err.is_err(), "second bind on port {} must fail", a.port);
    a.stop();
}
