//! Document Import IPC: Pandoc detection + conversion + settings.
//! Detection re-reads PATH from the Windows registry so Recheck sees a
//! just-installed Pandoc without an app restart. Conversion runs Pandoc
//! sandboxed + heap-capped under spawn_blocking (async command, like
//! search_vaults). Spec:
//! docs/superpowers/specs/2026-07-10-document-import-pandoc-design.md

use std::process::Command;
use vault_buddy_core::capture_config;

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PandocStatus {
    pub installed: bool,
    pub version: Option<String>,
    pub path: Option<String>,
    pub sandbox_supported: bool,
    /// The raw configured override (None → using PATH), so the settings
    /// field can seed itself without a second command.
    pub configured_path: Option<String>,
}

/// First line of `pandoc --version` is `pandoc <x.y.z>`; return (major, minor).
fn parse_pandoc_version(stdout: &str) -> Option<(u32, u32)> {
    let first = stdout.lines().next()?;
    let ver = first.split_whitespace().nth(1)?;
    let mut parts = ver.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next().unwrap_or("0").parse().ok()?;
    Some((major, minor))
}

/// `--sandbox` landed in Pandoc 2.15.
fn sandbox_supported(major: u32, minor: u32) -> bool {
    (major, minor) >= (2, 15)
}

/// Append `extra` PATH entries not already present (case-insensitive on the
/// separator platform). Keeps the process PATH first.
fn merged_path(base: &str, extra: &[String]) -> String {
    let sep = if cfg!(windows) { ';' } else { ':' };
    let mut out: Vec<String> = base.split(sep).map(str::to_string).collect();
    for e in extra {
        if !out.iter().any(|p| p.eq_ignore_ascii_case(e)) {
            out.push(e.clone());
        }
    }
    out.join(&sep.to_string())
}

/// Windows: read user + machine PATH from the registry so a just-installed
/// Pandoc is visible without restarting (a running process keeps its launch
/// PATH snapshot). Non-Windows: nothing extra (the compile gate + tests).
#[cfg(windows)]
fn registry_path_entries() -> Vec<String> {
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    use winreg::RegKey;
    let mut entries = Vec::new();
    let reads = [
        (HKEY_CURRENT_USER, "Environment"),
        (
            HKEY_LOCAL_MACHINE,
            r"SYSTEM\CurrentControlSet\Control\Session Manager\Environment",
        ),
    ];
    for (hive, sub) in reads {
        if let Ok(key) = RegKey::predef(hive).open_subkey(sub) {
            if let Ok(path) = key.get_value::<String, _>("Path") {
                entries.extend(path.split(';').map(str::to_string));
            }
        }
    }
    entries
}

#[cfg(not(windows))]
fn registry_path_entries() -> Vec<String> {
    Vec::new()
}

/// Ordered pandoc candidates to try: the configured override FIRST (if
/// non-empty), then the bare `pandoc` PATH lookup, deduped. Pure so it's
/// testable without touching the real config file — `pandoc_candidates`
/// below is the impure wrapper that feeds it the real config.
fn candidate_order(override_path: Option<&str>) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(p) = override_path {
        if !p.trim().is_empty() {
            out.push(p.to_string());
        }
    }
    if !out.iter().any(|c| c == "pandoc") {
        out.push("pandoc".to_string());
    }
    out
}

/// Ordered pandoc candidates to try: the configured override FIRST (if
/// non-empty), then the bare `pandoc` PATH lookup. Both are probed in order so
/// a stale/mistyped override does NOT hide a valid Pandoc on PATH — detection
/// falls through to PATH before reporting Not Installed (the settings contract
/// promises the override is checked first, *falling back* to PATH). Deduped so
/// an override literally equal to `pandoc` isn't probed twice.
fn pandoc_candidates() -> Vec<String> {
    candidate_order(
        capture_config::load_config()
            .document_import
            .pandoc_path
            .as_deref(),
    )
}

/// Build a Command with the registry-augmented PATH so PATH lookup sees a
/// fresh install.
fn pandoc_command(program: &str) -> Command {
    let mut cmd = Command::new(program);
    let base = std::env::var("PATH").unwrap_or_default();
    let extra = registry_path_entries();
    if !extra.is_empty() {
        cmd.env("PATH", merged_path(&base, &extra));
    }
    cmd
}

/// Probe one candidate: run `<program> --version`. On success, return the
/// program string with its parsed (major, minor). None if it can't run or
/// exits non-zero (so the caller falls through to the next candidate).
fn probe_pandoc(program: &str) -> Option<(String, u32, u32)> {
    let out = pandoc_command(program).arg("--version").output().ok()?;
    if !out.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    let (major, minor) = parse_pandoc_version(&stdout)?;
    Some((program.to_string(), major, minor))
}

/// First working pandoc across the ordered candidates (override → PATH), with
/// its full first `--version` line for display. None if no candidate runs.
fn resolve_working_pandoc() -> Option<(String, u32, u32, String)> {
    for program in pandoc_candidates() {
        // Re-run to capture the version line too; probe_pandoc already proved
        // it succeeds, so this second call is cheap and only on the winner.
        if let Some((prog, major, minor)) = probe_pandoc(&program) {
            let line = pandoc_command(&prog)
                .arg("--version")
                .output()
                .ok()
                .and_then(|o| {
                    String::from_utf8_lossy(&o.stdout)
                        .lines()
                        .next()
                        .map(|l| l.trim().to_string())
                });
            return Some((prog, major, minor, line.unwrap_or_default()));
        }
    }
    None
}

/// Detect Pandoc on demand (settings-open + Recheck). Async + spawn_blocking:
/// spawning a subprocess is blocking I/O and must stay off the main thread.
#[tauri::command]
pub async fn detect_pandoc() -> PandocStatus {
    tauri::async_runtime::spawn_blocking(|| {
        let configured = capture_config::load_config()
            .document_import
            .pandoc_path
            .filter(|p| !p.trim().is_empty());
        // Try the override, then PATH — a stale override must not hide a valid
        // PATH Pandoc (Codex review).
        match resolve_working_pandoc() {
            Some((program, major, minor, version_line)) => PandocStatus {
                installed: true,
                version: Some(version_line),
                path: Some(program),
                sandbox_supported: sandbox_supported(major, minor),
                configured_path: configured,
            },
            None => PandocStatus {
                installed: false,
                version: None,
                path: None,
                sandbox_supported: false,
                configured_path: configured,
            },
        }
    })
    .await
    .unwrap_or(PandocStatus {
        installed: false,
        version: None,
        path: None,
        sandbox_supported: false,
        configured_path: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_pandoc_version_line() {
        assert_eq!(
            parse_pandoc_version("pandoc 3.1.9\nCompiled with..."),
            Some((3, 1))
        );
        assert_eq!(parse_pandoc_version("pandoc.exe 2.14.2"), Some((2, 14)));
        assert_eq!(parse_pandoc_version("not pandoc"), None);
    }

    #[test]
    fn sandbox_requires_2_15_or_newer() {
        assert!(!sandbox_supported(2, 14));
        assert!(sandbox_supported(2, 15));
        assert!(sandbox_supported(3, 1));
        assert!(sandbox_supported(2, 20));
    }

    #[test]
    fn merged_path_appends_registry_entries_without_dupes() {
        let merged = merged_path("/usr/bin:/bin", &["/usr/bin".into(), "/opt/pandoc".into()]);
        assert!(merged.contains("/opt/pandoc"));
        // existing entry not duplicated
        assert_eq!(merged.matches("/usr/bin").count(), 1);
    }

    #[test]
    fn candidates_try_override_then_path_deduped() {
        // No override: just PATH.
        assert_eq!(candidate_order(None), vec!["pandoc".to_string()]);
        // Override present: override first, then PATH.
        assert_eq!(
            candidate_order(Some("/custom/pandoc")),
            vec!["/custom/pandoc".to_string(), "pandoc".to_string()]
        );
        // Blank override is treated as unset.
        assert_eq!(candidate_order(Some("   ")), vec!["pandoc".to_string()]);
        // Override literally "pandoc" is deduped, not probed twice.
        assert_eq!(candidate_order(Some("pandoc")), vec!["pandoc".to_string()]);
    }
}
