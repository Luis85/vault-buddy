//! Whisper ggml model registry and on-disk cache. Models are downloaded on
//! first use (never bundled) from Hugging Face into %APPDATA%\vault-buddy\
//! models — the only network access added for local transcription (the
//! pre-existing updater also talks to the network, for app updates).

use crate::CancelToken;
use sha2::{Digest, Sha256};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ModelTier {
    Base,
    Small,
    Medium,
}

impl ModelTier {
    /// Infallible by design (unrecognized input defaults to `Small`), so this
    /// intentionally isn't `std::str::FromStr` — that trait's `from_str`
    /// returns a `Result`, which doesn't fit "always resolves to a tier."
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> ModelTier {
        match s {
            "base" => ModelTier::Base,
            "medium" => ModelTier::Medium,
            _ => ModelTier::Small, // small is the default tier
        }
    }
    pub fn as_str(&self) -> &'static str {
        match self {
            ModelTier::Base => "base",
            ModelTier::Small => "small",
            ModelTier::Medium => "medium",
        }
    }
    /// Label recorded in transcript frontmatter.
    pub fn label(&self) -> String {
        format!("whisper-{}", self.as_str())
    }
    pub fn file_name(&self) -> &'static str {
        match self {
            ModelTier::Base => "ggml-base.bin",
            ModelTier::Small => "ggml-small.bin",
            ModelTier::Medium => "ggml-medium.bin",
        }
    }
    pub fn url(&self) -> String {
        format!(
            "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/{}",
            self.file_name()
        )
    }
    /// Canonical SHA-256 of the ggml file on Hugging Face
    /// (ggerganov/whisper.cpp). Verified during download so a complete-but-
    /// corrupt fetch is rejected instead of cached and reloaded forever.
    pub fn sha256(&self) -> &'static str {
        match self {
            ModelTier::Base => "60ed5bc3dd14eea856493d334349b405782ddcaf0028d4b5df4088345fba2efe",
            ModelTier::Small => "1be3a9b2063867b937e64e2ec7483364a79917e157fa98c5d94b5c1fffea987b",
            ModelTier::Medium => "6c14d5adee5f86394037b4e4e8b59f1673b6cee10e3cf0b11bbdbee79c156208",
        }
    }
    /// A sanity floor (not a checksum): a downloaded file far below this is a
    /// partial/failed transfer. A corrupt-but-large file is caught when the
    /// engine fails to load it (retryable).
    pub fn min_size(&self) -> u64 {
        match self {
            ModelTier::Base => 100_000_000,     // ~142 MB
            ModelTier::Small => 300_000_000,    // ~466 MB
            ModelTier::Medium => 1_000_000_000, // ~1.5 GB
        }
    }
}

/// `%APPDATA%\vault-buddy\models` — nested inside the same app folder as
/// `config.json` (`vault_buddy_core::capture_config::app_config_dir`), so
/// transcription reuses the existing app folder and never creates a second
/// top-level AppData folder. App-side, never inside a vault.
pub fn model_dir() -> Option<PathBuf> {
    vault_buddy_core::capture_config::app_config_dir().map(|d| d.join("models"))
}

pub fn model_path(tier: ModelTier) -> Option<PathBuf> {
    model_dir().map(|d| d.join(tier.file_name()))
}

/// Download the tier's ggml model with progress, `.part`-then-rename. Skips
/// if already present. `on_progress(received, total)` is called per chunk.
/// `cancel` is polled before the request and on every chunk so a cancel
/// during a first-use download (up to ~1.5 GB for `medium`) aborts promptly
/// instead of running to completion — an aborted download returns `Err`,
/// which the caller disambiguates from a real failure via the same token.
pub fn download_model(
    tier: ModelTier,
    cancel: &CancelToken,
    on_progress: &mut dyn FnMut(u64, Option<u64>),
) -> Result<PathBuf, String> {
    // Already cancelled: do no work and open no connection.
    if cancel.is_cancelled() {
        return Err("cancelled".to_string());
    }
    let dir = model_dir().ok_or("cannot resolve model directory")?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("create model dir: {e}"))?;
    let dest = dir.join(tier.file_name());
    if dest.exists() {
        return Ok(dest);
    }
    let agent = model_download_agent();
    download_stream(
        &agent,
        &tier.url(),
        &dir,
        tier.file_name(),
        tier.min_size(),
        tier.sha256(),
        cancel,
        on_progress,
    )
}

/// Agent used for model downloads. A *fully stalled* socket must surface as an
/// `Err` (which the caller already turns into a retryable failure) instead of
/// wedging the single background transcription worker for the whole app
/// session — the cancel token is only polled between reads, so a blocked
/// `read()` can never be interrupted. `timeout_recv_body` reads as a
/// whole-body deadline in ureq's docs, but is empirically a per-read (idle)
/// timeout that resets on every chunk received — confirmed against a local
/// server that trickles bytes slower than the configured timeout but never
/// stalls for longer than it, which completes successfully; a genuine stall
/// past the timeout still errors. So a healthy but slow transfer of the
/// up-to-~1.5 GB `medium` model keeps going as long as bytes keep flowing;
/// only a socket silent for a full minute trips it. A true whole-body
/// deadline is deliberately avoided: it would abort a legitimately slow
/// multi-hundred-MB download.
fn model_download_agent() -> ureq::Agent {
    let config = ureq::Agent::config_builder()
        .timeout_connect(Some(std::time::Duration::from_secs(30)))
        .timeout_recv_body(Some(std::time::Duration::from_secs(60)))
        .build();
    ureq::Agent::new_with_config(config)
}

/// `ureq::http::Response` has no `.header()` convenience method (unlike the
/// pre-3.x API) — headers live behind the `http` crate's typed `HeaderMap`.
fn header<'a>(resp: &'a ureq::http::Response<ureq::Body>, name: &str) -> Option<&'a str> {
    resp.headers().get(name).and_then(|v| v.to_str().ok())
}

/// Streams the response body into a `.part` file, then renames it into place.
/// Split out from `download_model` so the timeout behavior can be exercised by
/// a short-timeout agent against a localhost server in tests. `dest` is assumed
/// not to exist yet (the caller returns early when it does), which is what makes
/// the plain rename below safe.
#[allow(clippy::too_many_arguments)]
fn download_stream(
    agent: &ureq::Agent,
    url: &str,
    dir: &Path,
    file_name: &str,
    min_size: u64,
    expected_sha256: &str,
    cancel: &CancelToken,
    on_progress: &mut dyn FnMut(u64, Option<u64>),
) -> Result<PathBuf, String> {
    let mut resp = agent
        .get(url)
        .call()
        .map_err(|e| format!("request model: {e}"))?;
    let total: Option<u64> = header(&resp, "Content-Length").and_then(|v| v.parse().ok());
    // A hedge for the completeness check below. In practice ureq strips BOTH
    // Content-Encoding and Content-Length whenever it transparently decompresses
    // a body (see its response.rs), so a decoded body reaches us with
    // total=None and the check is skipped regardless — but were a future ureq to
    // keep the length, comparing our decoded byte count to the *encoded*
    // Content-Length would false-positive, so we still guard on it. Absent
    // header == identity. Read before into_body() consumes the response.
    let encoded_body = header(&resp, "Content-Encoding")
        .map(|v| !v.eq_ignore_ascii_case("identity"))
        .unwrap_or(false);
    let part = dir.join(format!("{file_name}.part"));
    let dest = dir.join(file_name);
    let mut reader = resp.body_mut().as_reader();
    let mut file = std::fs::File::create(&part).map_err(|e| format!("create model temp: {e}"))?;
    let mut buf = [0u8; 64 * 1024];
    let mut received: u64 = 0;
    let mut hasher = Sha256::new();
    loop {
        if cancel.is_cancelled() {
            // Close our handle before removing the temp — Windows refuses to
            // unlink a still-open file (matches the drop-before-remove the
            // incomplete-transfer branch below already relies on).
            drop(file);
            let _ = std::fs::remove_file(&part);
            return Err("cancelled".to_string());
        }
        let n = match std::io::Read::read(&mut reader, &mut buf) {
            Ok(n) => n,
            Err(e) => {
                // A mid-body read error includes the truncation case: when the
                // sender closes early on a Content-Length response, ureq's
                // length-limited reader surfaces it as an UnexpectedEof rather
                // than a clean end-of-stream, so the loop never reaches the
                // completeness check below. Clean up the temp here too (drop
                // before remove — Windows won't unlink an open file) so a
                // truncated transfer leaves no `.part` behind, same as cancel.
                drop(file);
                let _ = std::fs::remove_file(&part);
                return Err(format!("read stream: {e}"));
            }
        };
        if n == 0 {
            break;
        }
        if let Err(e) = std::io::Write::write_all(&mut file, &buf[..n]) {
            drop(file);
            let _ = std::fs::remove_file(&part);
            return Err(format!("write model: {e}"));
        }
        received += n as u64;
        hasher.update(&buf[..n]);
        on_progress(received, total);
    }
    // Best-effort durability: the integrity check below re-hashes the bytes
    // already handed to `write_all`, not a re-read from disk, so a flush/sync
    // failure can't make a corrupt file pass verification — but it's still a
    // real durability gap (an unflushed `.part` lost to a crash before
    // rename), worth a warning rather than silence.
    if let Err(e) = std::io::Write::flush(&mut file) {
        log::warn!("model download: flush failed for {}: {e}", part.display());
    }
    if let Err(e) = file.sync_all() {
        log::warn!("model download: fsync failed for {}: {e}", part.display());
    }
    drop(file);
    if received < min_size {
        let _ = std::fs::remove_file(&part);
        return Err(format!("model download incomplete: {received} bytes"));
    }
    // Completeness, not just a floor: a sender can close after advertising a
    // Content-Length but before sending all of it, leaving a body that still
    // clears min_size (e.g. base stops at 120 MB of ~142 MB) which, cached,
    // would fail to load forever. Defense-in-depth: on the current ureq a
    // truncated Content-Length body actually surfaces earlier as an
    // UnexpectedEof from the length-limited reader (cleaned up on the read-error
    // path above), so this equality catches only the residual case of a clean
    // short EOF (or a future reader that returns Ok(0) early). Skipped for a
    // content-encoded body (the advertised length is then the encoded size, see
    // `encoded_body`); no `total` (close-delimited) keeps the min_size-only
    // behavior. `file` is already dropped above, so a bare remove is fine.
    if let Some(total) = total {
        if !encoded_body && received != total {
            let _ = std::fs::remove_file(&part);
            return Err(format!(
                "model download incomplete: {received} of {total} bytes"
            ));
        }
    }
    // Integrity: a complete-but-corrupt body clears both the size floor and the
    // Content-Length check, so only the published hash can reject it. An empty
    // expected hash means "unverified" (kept for tests / a hypothetical future
    // tier); every real tier supplies one. `file` is already dropped above.
    if !expected_sha256.is_empty() {
        let digest = hasher.finalize();
        let mut actual = String::with_capacity(digest.len() * 2);
        for b in digest.iter() {
            write!(actual, "{b:02x}").expect("writing to a String is infallible");
        }
        if !actual.eq_ignore_ascii_case(expected_sha256) {
            let _ = std::fs::remove_file(&part);
            return Err(format!("model checksum mismatch: got {actual}"));
        }
    }
    // We own `part` and `dest` didn't exist above — a plain rename is fine.
    std::fs::rename(&part, &dest).map_err(|e| format!("finalize model: {e}"))?;
    Ok(dest)
}

/// Discard a cached model so the next download re-fetches it. A model that
/// downloaded but won't load is corrupt (a large-but-broken file can clear the
/// `min_size` floor and be cached anyway); without a way to drop it the shell
/// would short-circuit on `dest.exists()` and hand back the same broken file
/// forever. Best-effort: an unresolvable model dir is a no-op (nothing to
/// remove), and an already-absent file is success.
pub fn remove_model(tier: ModelTier) -> std::io::Result<()> {
    match model_dir() {
        Some(dir) => remove_cached(&dir, tier.file_name()),
        None => Ok(()),
    }
}

/// Delete `dir/file_name`, treating an already-absent file as success — the
/// contract is "the path is clear", not "we did the deleting". Split from
/// `remove_model` so it can be tested against a tempdir instead of the real
/// %APPDATA% models dir.
fn remove_cached(dir: &Path, file_name: &str) -> std::io::Result<()> {
    match std::fs::remove_file(dir.join(file_name)) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tier_from_str_defaults_to_small() {
        assert_eq!(ModelTier::from_str("base"), ModelTier::Base);
        assert_eq!(ModelTier::from_str("medium"), ModelTier::Medium);
        assert_eq!(ModelTier::from_str("small"), ModelTier::Small);
        assert_eq!(ModelTier::from_str("garbage"), ModelTier::Small);
    }

    #[test]
    fn tier_files_urls_and_labels() {
        assert_eq!(ModelTier::Small.file_name(), "ggml-small.bin");
        assert!(ModelTier::Small.url().ends_with("/ggml-small.bin"));
        assert!(ModelTier::Small
            .url()
            .starts_with("https://huggingface.co/ggerganov/whisper.cpp"));
        assert_eq!(ModelTier::Base.label(), "whisper-base");
        assert_eq!(ModelTier::Small.as_str(), "small");
    }

    #[test]
    fn model_path_ends_with_the_tier_file() {
        if let Some(p) = model_path(ModelTier::Small) {
            assert_eq!(p.file_name().unwrap().to_string_lossy(), "ggml-small.bin");
        }
    }

    #[test]
    fn model_dir_nests_in_the_shared_app_config_dir() {
        // Models must live inside the same %APPDATA%\vault-buddy folder as
        // config.json (core::capture_config::app_config_dir), never a second
        // top-level app folder. Deriving both from that one helper is what
        // keeps them from drifting into separate folders.
        if let (Some(models), Some(app)) = (
            model_dir(),
            vault_buddy_core::capture_config::app_config_dir(),
        ) {
            assert_eq!(models.parent(), Some(app.as_path()));
            assert_eq!(models.file_name().unwrap(), "models");
        }
    }

    #[test]
    fn stalled_download_errors_instead_of_hanging() {
        // Regression (M1): ureq's default agent has no read timeout, so a
        // server that stalls mid-body blocks the single transcription worker
        // for the whole app session — and a user "cancel" appears to do
        // nothing, because the token is only polled *between* reads and the
        // blocked read() is never reached again. The streaming download must
        // surface a fully-stalled socket as an Err via a per-read (idle)
        // timeout on the agent. We drive the seam with a short-timeout agent
        // against a localhost server that sends a header advertising a large
        // body, dribbles a few bytes, then goes silent (never closing), so the
        // ONLY way to finish is the read timeout — not an EOF.
        use std::io::{Read, Write};
        use std::net::TcpListener;
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::{mpsc, Arc};
        use std::time::Duration;

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
        let port = listener.local_addr().unwrap().port();

        // Hold the socket open and silent well past the client's recv bound, so
        // a missing read timeout manifests as a hang (not an early EOF that
        // would pass for the wrong reason). `stop` lets the healthy path tear
        // the server down immediately instead of waiting out the deadline.
        let stop = Arc::new(AtomicBool::new(false));
        let stop_srv = stop.clone();
        let server = std::thread::spawn(move || {
            if let Ok((mut sock, _)) = listener.accept() {
                let mut req = [0u8; 1024];
                let _ = sock.read(&mut req);
                let _ =
                    sock.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 1000000000\r\n\r\nabcd");
                let _ = sock.flush();
                let deadline = std::time::Instant::now() + Duration::from_secs(10);
                while !stop_srv.load(Ordering::SeqCst) && std::time::Instant::now() < deadline {
                    std::thread::sleep(Duration::from_millis(25));
                }
            }
        });

        let config = ureq::Agent::config_builder()
            .timeout_recv_body(Some(Duration::from_millis(500)))
            .build();
        let agent = ureq::Agent::new_with_config(config);
        let url = format!("http://127.0.0.1:{port}/ggml-base.bin");
        let dir = tempfile::tempdir().expect("tempdir");
        let dir_path = dir.path().to_path_buf();

        // Run the download on its own thread so a HANG fails the test with a
        // clear message via recv_timeout instead of wedging the whole suite.
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let cancel = crate::CancelToken::new();
            let mut progress = |_received: u64, _total: Option<u64>| {};
            let res = download_stream(
                &agent,
                &url,
                &dir_path,
                "ggml-base.bin",
                ModelTier::Base.min_size(),
                "",
                &cancel,
                &mut progress,
            );
            let _ = tx.send(res.is_err());
        });

        let outcome = rx.recv_timeout(Duration::from_secs(3));
        stop.store(true, Ordering::SeqCst);
        let _ = server.join();
        match outcome {
            Ok(is_err) => assert!(is_err, "a stalled download must return Err, not Ok"),
            Err(_) => {
                panic!("download did not return in time — a stalled read is not surfacing as Err")
            }
        }
    }

    #[test]
    fn truncated_download_under_content_length_errors_and_leaves_no_files() {
        // Regression (M2a): a transfer that ends early yet still clears the
        // min_size floor must never be cached. The server advertises a
        // 1000-byte body, sends 4 bytes, then closes; with min_size = 0 the
        // floor cannot reject it. On the current ureq the early close surfaces
        // as an UnexpectedEof from the length-limited reader, so the read-error
        // cleanup is what rejects it (the post-loop received != total check is
        // the belt-and-suspenders for a clean short EOF). Either way a truncated
        // file must not become `dest` — which the shell would then short-circuit
        // on via dest.exists() and never re-download, failing to load forever —
        // and the .part temp must be gone too: a rejected transfer leaves no
        // litter.
        use std::io::{Read, Write};
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
        let port = listener.local_addr().unwrap().port();

        // Single accept, then close after a short body — an EOF (not a stall),
        // so no read-timeout/thread guard is needed (contrast the stalled test).
        let server = std::thread::spawn(move || {
            if let Ok((mut sock, _)) = listener.accept() {
                let mut req = [0u8; 1024];
                let _ = sock.read(&mut req);
                let _ = sock.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 1000\r\n\r\nabcd");
                let _ = sock.flush();
                // sock drops here -> client sees EOF after 4 of 1000 bytes.
            }
        });

        let agent = model_download_agent();
        let url = format!("http://127.0.0.1:{port}/ggml-base.bin");
        let dir = tempfile::tempdir().expect("tempdir");
        let cancel = crate::CancelToken::new();
        let mut progress = |_r: u64, _t: Option<u64>| {};
        let res = download_stream(
            &agent,
            &url,
            dir.path(),
            "ggml-base.bin",
            0, // floor disabled: only the completeness check can reject
            "",
            &cancel,
            &mut progress,
        );
        let _ = server.join();

        assert!(
            res.is_err(),
            "a body short of its Content-Length must be an Err, got {res:?}"
        );
        assert!(
            !dir.path().join("ggml-base.bin").exists(),
            "a truncated download must never be renamed into place"
        );
        assert!(
            !dir.path().join("ggml-base.bin.part").exists(),
            "the .part temp must not be left behind on a truncated download"
        );
    }

    #[test]
    fn download_without_content_length_still_succeeds() {
        // Guards against the completeness check over-triggering: a length-less
        // (connection-close-delimited) response advertises no total to compare
        // against, so the existing min_size-only behavior must still accept it.
        use std::io::{Read, Write};
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
        let port = listener.local_addr().unwrap().port();

        let server = std::thread::spawn(move || {
            if let Ok((mut sock, _)) = listener.accept() {
                let mut req = [0u8; 1024];
                let _ = sock.read(&mut req);
                // No Content-Length: the body is delimited by the socket close.
                let _ = sock.write_all(b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\nhello world");
                let _ = sock.flush();
            }
        });

        let agent = model_download_agent();
        let url = format!("http://127.0.0.1:{port}/ggml-base.bin");
        let dir = tempfile::tempdir().expect("tempdir");
        let cancel = crate::CancelToken::new();
        let mut progress = |_r: u64, _t: Option<u64>| {};
        let res = download_stream(
            &agent,
            &url,
            dir.path(),
            "ggml-base.bin",
            0,
            "",
            &cancel,
            &mut progress,
        );
        let _ = server.join();

        assert!(
            res.is_ok(),
            "a length-less body must still succeed: {res:?}"
        );
        assert!(
            dir.path().join("ggml-base.bin").exists(),
            "the finalized model file must exist"
        );
    }

    #[test]
    fn download_matching_sha256_succeeds() {
        // The full body hashes to the expected value → finalized into place.
        use std::io::{Read, Write};
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            if let Ok((mut sock, _)) = listener.accept() {
                let mut req = [0u8; 1024];
                let _ = sock.read(&mut req);
                let _ = sock.write_all(b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\nhello world");
                let _ = sock.flush();
            }
        });
        // sha256("hello world")
        let expected = "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9";
        let agent = model_download_agent();
        let url = format!("http://127.0.0.1:{port}/ggml-base.bin");
        let dir = tempfile::tempdir().expect("tempdir");
        let cancel = crate::CancelToken::new();
        let mut progress = |_r: u64, _t: Option<u64>| {};
        let res = download_stream(
            &agent,
            &url,
            dir.path(),
            "ggml-base.bin",
            0,
            expected,
            &cancel,
            &mut progress,
        );
        let _ = server.join();
        assert!(
            res.is_ok(),
            "a body matching its hash must finalize: {res:?}"
        );
        assert!(
            dir.path().join("ggml-base.bin").exists(),
            "the verified model is finalized to its .bin path"
        );
    }

    #[test]
    fn download_wrong_sha256_is_rejected_and_leaves_no_files() {
        // Regression: a complete-but-corrupt model (right length, wrong bytes)
        // passes the size/length checks, so only a checksum can reject it. A
        // mismatch must delete the .part and never finalize `dest` — otherwise the
        // shell short-circuits on dest.exists() and loads a corrupt model forever.
        use std::io::{Read, Write};
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            if let Ok((mut sock, _)) = listener.accept() {
                let mut req = [0u8; 1024];
                let _ = sock.read(&mut req);
                let _ = sock.write_all(b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\nhello world");
                let _ = sock.flush();
            }
        });
        let wrong = "0000000000000000000000000000000000000000000000000000000000000000";
        let agent = model_download_agent();
        let url = format!("http://127.0.0.1:{port}/ggml-base.bin");
        let dir = tempfile::tempdir().expect("tempdir");
        let cancel = crate::CancelToken::new();
        let mut progress = |_r: u64, _t: Option<u64>| {};
        let res = download_stream(
            &agent,
            &url,
            dir.path(),
            "ggml-base.bin",
            0,
            wrong,
            &cancel,
            &mut progress,
        );
        let _ = server.join();
        assert!(res.is_err(), "a hash mismatch must be an Err, got {res:?}");
        assert!(
            !dir.path().join("ggml-base.bin").exists(),
            "corrupt model must not finalize"
        );
        assert!(
            !dir.path().join("ggml-base.bin.part").exists(),
            "the .part must be cleaned up"
        );
    }

    #[test]
    fn tier_sha256_values_are_lowercase_hex_of_expected_length() {
        for t in [ModelTier::Base, ModelTier::Small, ModelTier::Medium] {
            let h = t.sha256();
            assert_eq!(h.len(), 64, "sha256 hex is 64 chars for {t:?}");
            assert!(h
                .chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
        }
    }

    #[test]
    fn precancelled_download_bails_without_touching_the_network() {
        // Regression: a cancel during a first-use model download used to be
        // ignored until the entire file had been fetched. A pre-cancelled
        // token must return Err at the very top — before ureq::get — so this
        // is hermetic (no network in CI) precisely because the abort happens
        // before any request is made.
        let cancel = crate::CancelToken::new();
        cancel.cancel();
        let mut progress = |_received: u64, _total: Option<u64>| {};
        assert!(
            download_model(ModelTier::Base, &cancel, &mut progress).is_err(),
            "a pre-cancelled download must not proceed to the network"
        );
    }

    #[test]
    fn cancelling_mid_download_cleans_up_the_part_file() {
        // Regression: only a PRE-cancelled token (above) was covered — a
        // cancel that arrives AFTER at least one chunk has already landed on
        // disk must still remove the `.part`. The loop's cancel check sits at
        // the TOP of the loop, so a cancel signalled from inside the progress
        // callback (which runs once per received chunk) is exactly what a
        // real mid-download cancel looks like: the very next iteration must
        // see it and clean up rather than reading to completion.
        use std::io::{Read, Write};
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
        let port = listener.local_addr().unwrap().port();
        let body = vec![b'a'; 128 * 1024]; // several 64 KiB read-buffer chunks
        let server = std::thread::spawn(move || {
            if let Ok((mut sock, _)) = listener.accept() {
                let mut req = [0u8; 1024];
                let _ = sock.read(&mut req);
                let header = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n", body.len());
                let _ = sock.write_all(header.as_bytes());
                let _ = sock.write_all(&body);
                let _ = sock.flush();
            }
        });

        let agent = model_download_agent();
        let url = format!("http://127.0.0.1:{port}/ggml-base.bin");
        let dir = tempfile::tempdir().expect("tempdir");
        let cancel = crate::CancelToken::new();
        let cancel_from_progress = cancel.clone();
        let mut progress = move |_r: u64, _t: Option<u64>| cancel_from_progress.cancel();
        let res = download_stream(
            &agent,
            &url,
            dir.path(),
            "ggml-base.bin",
            0,
            "",
            &cancel,
            &mut progress,
        );
        let _ = server.join();

        assert!(res.is_err(), "a mid-download cancel must return Err");
        assert!(
            !dir.path().join("ggml-base.bin").exists(),
            "a cancelled download must never finalize"
        );
        assert!(
            !dir.path().join("ggml-base.bin.part").exists(),
            "the .part temp must be cleaned up after a mid-download cancel"
        );
    }

    #[test]
    fn remove_cached_deletes_then_is_ok_when_absent() {
        // A model that downloaded but won't load is corrupt; remove_cached
        // clears the path so the next attempt re-downloads, and treats an
        // already-absent file as success (the goal is a clear path, not proof
        // we did the deleting). Only remove_cached is exercised — remove_model
        // resolves the REAL %APPDATA%\vault-buddy\models dir and must never run
        // in a test, or it would delete a user's actual model.
        let dir = tempfile::tempdir().expect("tempdir");
        let name = "ggml-base.bin";
        std::fs::write(dir.path().join(name), b"corrupt").expect("seed file");
        assert!(dir.path().join(name).exists());

        remove_cached(dir.path(), name).expect("removing an existing file must succeed");
        assert!(
            !dir.path().join(name).exists(),
            "the cached file must be gone after remove_cached"
        );

        remove_cached(dir.path(), name).expect("removing an absent file must still be Ok");
    }
}
