//! Client-agnostic spec-level validation: a real MCP client (rmcp's own,
//! co-versioned with the server) drives initialize → tools/list → tools/call
//! over streamable HTTP against a temp-dir vault, and the task file actually
//! lands on disk.

use std::sync::atomic::AtomicBool;
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

#[test]
fn a_taken_port_is_a_synchronous_error() {
    let dir = tempfile::tempdir().unwrap();
    let a = start(fixture_deps(dir.path(), false), 0, TOKEN.to_string()).unwrap();
    let err = start(fixture_deps(dir.path(), false), a.port, TOKEN.to_string());
    assert!(err.is_err(), "second bind on port {} must fail", a.port);
    a.stop();
}
