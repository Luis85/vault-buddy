//! What the asset protocol's scope actually ADMITS, file by file (ADR R7,
//! review finding S-10).
//!
//! `tray.rs` pins the scope's LIST; this module pins what the list lets
//! through. The two are different claims, and the gap between them is how
//! the staging entry came to leak: `$APPLOCALDATA/screen-captures/*` reads
//! like "the staged videos", but a staging directory also holds each
//! capture's sidecar (`<base>.json`: its vault id, its source window's
//! title and its project pin) and every hidden in-progress file
//! (`.<base>.mp4.part`, the webcam and stem parts, an older build's export
//! temp) — and Tauri's `require_literal_leading_dot` is set on Unix only,
//! so on Windows a bare `*` matches a leading dot too. Every one of those
//! files was fetchable over `asset://` from all six windows. The entry is
//! now two entries, `*.mp4` (the capture and its `<base>.webcam.mp4`) and
//! `*.m4a` (its `<base>.stem-<n>.m4a`), which is exactly the set
//! `project_store::resolve_source` can answer with for a staged source.
//!
//! **The matcher here is a mirror, not Tauri's.** Tauri 2 builds its scope
//! from `glob::Pattern` and matches with `glob::MatchOptions {
//! case_sensitive: true, require_literal_separator: true,
//! require_literal_leading_dot: <false on Windows> }` (tauri
//! `scope/fs.rs`, `Scope::new`). `glob` is not a dependency of this crate
//! and Tauri's `Scope` needs a live `Manager` to resolve `$APPLOCALDATA`,
//! so `glob_admits` re-implements exactly that subset — `*` matches any
//! run of characters except a separator, a leading dot is NOT special
//! (the Windows setting, the one that ships), matching is case-sensitive —
//! and refuses (by assertion) any pattern using a glob feature it does not
//! model (`?`, `[`, `**`), so a later scope edit cannot quietly outgrow it.
//!
//! Test-only: `lib.rs` gates the module on `cfg(test)`, the
//! `config_lock_guard` / `window_close_guard` precedent.

use std::path::Path;

use crate::editor::project_store::{resolve_source, SourceLocator, SourceMediaKind, SourceRecord};
use vault_buddy_screen::staging;

/// The scope list as `tauri.conf.json` declares it.
fn scope() -> Vec<String> {
    let conf: serde_json::Value =
        serde_json::from_str(include_str!("../tauri.conf.json")).expect("tauri.conf.json");
    conf["app"]["security"]["assetProtocol"]["scope"]
        .as_array()
        .expect("assetProtocol.scope must be an array")
        .iter()
        .map(|v| v.as_str().expect("scope entry").to_string())
        .collect()
}

/// `glob::Pattern::matches_path_with` under Tauri's Windows options, for
/// the subset of glob syntax the scope uses (see the module doc). Both
/// sides are compared with `/` separators, as Tauri's own `push_pattern`
/// and `is_allowed` normalise them to one platform separator.
fn glob_admits(pattern: &str, path: &str) -> bool {
    assert!(
        !pattern.contains(['?', '[']) && !pattern.contains("**"),
        "glob_admits models only `*`; the scope pattern {pattern:?} uses more"
    );
    fn go(p: &[u8], s: &[u8]) -> bool {
        match p.split_first() {
            None => s.is_empty(),
            Some((b'*', rest)) => {
                let mut i = 0;
                loop {
                    if go(rest, &s[i..]) {
                        return true;
                    }
                    if i == s.len() || s[i] == b'/' {
                        return false;
                    }
                    i += 1;
                }
            }
            Some((c, rest)) => s.first() == Some(c) && go(rest, &s[1..]),
        }
    }
    go(
        pattern.replace('\\', "/").as_bytes(),
        path.replace('\\', "/").as_bytes(),
    )
}

/// Whether ANY scope entry admits `file`, with `$APPLOCALDATA` standing for
/// `root` (the app's local data dir, which is what Tauri resolves it to).
fn scope_admits(root: &Path, file: &Path) -> bool {
    let root = root.to_string_lossy().replace('\\', "/");
    let file = file.to_string_lossy();
    scope()
        .iter()
        .any(|p| glob_admits(&p.replace("$APPLOCALDATA", &root), &file))
}

const BASE: &str = "2026-09-24 1015 Demo";

#[test]
fn the_mirror_matches_like_tauri_windows_glob_options() {
    // `*` never crosses a separator (require_literal_separator)…
    assert!(glob_admits("/r/sc/*", "/r/sc/a.mp4"));
    assert!(!glob_admits("/r/sc/*", "/r/sc/sub/a.mp4"));
    // …a leading dot is NOT special on Windows (the leak's own mechanism)…
    assert!(glob_admits("/r/sc/*", "/r/sc/.a.mp4.part"));
    // …an extension pattern is anchored at the end, and case-sensitive.
    assert!(glob_admits("/r/sc/*.mp4", "/r/sc/a.webcam.mp4"));
    assert!(!glob_admits("/r/sc/*.mp4", "/r/sc/a.mp4.part"));
    assert!(!glob_admits("/r/sc/*.mp4", "/r/sc/a.MP4"));
    // Backslash paths compare as their `/` form.
    assert!(glob_admits("C:/r/sc/*.m4a", r"C:\r\sc\a.stem-1.m4a"));
}

// S-10: nothing in the staging directory but the media a project plays may
// be served. Every name below is one this app writes there.
#[test]
fn the_scope_admits_no_staged_sidecar_or_in_progress_file() {
    let root = Path::new("C:/Users/u/AppData/Local/com.vaultbuddy.desktop");
    let dir = staging::staging_dir(root);
    for name in [
        staging::sidecar_file_name(BASE),
        staging::part_file_name(BASE),
        staging::export_part_file_name(BASE),
        staging::webcam_part_file_name(BASE),
        staging::stem_part_file_name(BASE, 1),
        // Not names this app mints, but the shapes a looser pattern admits.
        format!("{BASE}.mp4.part"),
        "x.json".to_string(),
    ] {
        assert!(
            !scope_admits(root, &dir.join(&name)),
            "the asset protocol scope must not admit the staged file {name:?} -- \
             only a capture's .mp4 media and its .m4a stems"
        );
    }
}

// The narrowing must not cost the preview anything: the three kinds of
// staged source a project plays still resolve to a path the scope admits.
#[test]
fn every_staged_source_resolves_inside_the_scope() {
    let root = tempfile::tempdir().unwrap();
    let dir = staging::staging_dir(root.path());
    std::fs::create_dir_all(&dir).unwrap();
    let stem = staging::stem_file_name(BASE, 1);
    let sidecar = serde_json::json!({
        "base": BASE, "vaultId": "v", "sourceTitle": "Demo", "sourceKind": "screen",
        "inputs": ["USB Mic"], "durationMs": 1000, "pausedMs": 0, "width": 640,
        "height": 360, "recordedAt": "2026-09-24T10:15:00Z",
        "stems": [{ "index": 1, "input": "USB Mic", "file": stem }]
    });
    std::fs::write(
        dir.join(staging::sidecar_file_name(BASE)),
        serde_json::to_vec(&sidecar).unwrap(),
    )
    .unwrap();
    let record = |locator: SourceLocator| SourceRecord {
        locator,
        sha256: None,
        size: 0,
        duration_ms: 0,
        width: None,
        height: None,
        has_audio: false,
        has_video: true,
        media_kind: SourceMediaKind::Video,
        replaced_from: None,
    };
    for (what, locator) in [
        ("the capture", SourceLocator::Staging { base: BASE.into() }),
        (
            "its webcam file",
            SourceLocator::StagingFile {
                base: BASE.into(),
                file: staging::webcam_file_name(BASE),
            },
        ),
        (
            "a stem",
            SourceLocator::StagingFile {
                base: BASE.into(),
                file: stem.clone(),
            },
        ),
    ] {
        let path = resolve_source(root.path(), "proj1", &record(locator))
            .unwrap_or_else(|| panic!("{what} must resolve"));
        assert!(
            scope_admits(root.path(), &path),
            "{what} resolves to {path:?}, which the asset protocol scope refuses -- \
             the editor preview could not play it"
        );
    }
}
