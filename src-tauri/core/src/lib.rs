pub mod app_diagnostics;
pub mod capture_config;
pub mod capture_note;
pub mod capture_paths;
pub mod checkpoint;
pub mod companion_placement;
pub mod crash;
pub mod daily_notes;
pub mod discovery;
pub mod document_import;
pub mod process;
pub mod recordings;
pub mod search;
pub mod services;
pub mod sync_util;
pub mod tasks;
pub mod throttle;
pub mod transcript;
pub mod uri;
pub mod vault_walk;

use chrono::NaiveDate;
use std::path::Path;

/// The vault-relative daily-note path (no `.md`) for `date`, and whether the
/// note file already exists. Split from `daily_note_uri` so callers that must
/// gate creation (the MCP `open_daily_note` tool) can decide BEFORE a URI is
/// built.
pub fn daily_note_target(vault_path: &Path, date: NaiveDate) -> (String, bool) {
    let settings = daily_notes::load_settings(vault_path);
    let rel = daily_notes::daily_note_rel_path(&settings, date);
    let exists = vault_path.join(format!("{rel}.md")).exists();
    (rel, exists)
}

/// Builds the URI that opens today's daily note for a vault:
/// `obsidian://open` if the note file already exists, `obsidian://new`
/// otherwise — Obsidian itself performs the creation. Vault Buddy never
/// writes into a vault. `vault_id` is the unique key from obsidian.json.
pub fn daily_note_uri(vault_id: &str, vault_path: &Path, date: NaiveDate) -> String {
    let (rel, exists) = daily_note_target(vault_path, date);
    if exists {
        uri::open_file_uri(vault_id, &rel)
    } else {
        uri::new_file_uri(vault_id, &rel)
    }
}

/// The `obsidian://open` URI for an imported note. `note` is the path
/// `convert_document` returns — vault-relative on success, or an absolute
/// fallback when it couldn't strip the vault prefix — so a relative note is
/// resolved under `vault_root` first. The final extension is dropped (Obsidian
/// resolves `Documents/2026/07/Report` to `Report.md`). Returns `None` when the
/// note resolves outside the vault, so the caller can refuse rather than open
/// something unexpected. The vault is never written — this only opens.
pub fn imported_note_uri(vault_id: &str, vault_root: &Path, note: &str) -> Option<String> {
    let p = Path::new(note);
    let abs = if p.is_absolute() {
        p.to_path_buf()
    } else {
        vault_root.join(p)
    };
    let rel = uri::vault_relative_no_ext(&abs, vault_root)?;
    Some(uri::open_file_uri(vault_id, &rel))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn date() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 7, 3).unwrap()
    }

    #[test]
    fn imported_note_uri_from_relative_path_drops_the_extension() {
        // convert_document returns a vault-relative path on success; the open
        // URI resolves it under the vault and drops the final `.md`.
        assert_eq!(
            imported_note_uri("a1b2c3", Path::new("/vault"), "Documents/2026/07/Report.md")
                .as_deref(),
            Some("obsidian://open?vault=a1b2c3&file=Documents%2F2026%2F07%2FReport"),
        );
    }

    #[test]
    fn imported_note_uri_accepts_an_absolute_path_inside_the_vault() {
        // convert_document falls back to an absolute path when it can't strip
        // the vault prefix; opening it must still work.
        assert_eq!(
            imported_note_uri(
                "a1b2c3",
                Path::new("/vault"),
                "/vault/Documents/2026/07/Report.md"
            )
            .as_deref(),
            Some("obsidian://open?vault=a1b2c3&file=Documents%2F2026%2F07%2FReport"),
        );
    }

    #[test]
    fn imported_note_uri_outside_the_vault_is_none() {
        assert_eq!(
            imported_note_uri("a1b2c3", Path::new("/vault"), "/elsewhere/Report.md"),
            None,
        );
    }

    #[test]
    fn existing_note_uses_open_uri() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("2026-07-03.md"), "hello").unwrap();
        let uri = daily_note_uri("a1b2c3", dir.path(), date());
        assert!(uri.starts_with("obsidian://open?"), "got: {uri}");
    }

    #[test]
    fn missing_note_uses_new_uri() {
        let dir = tempfile::tempdir().unwrap();
        let uri = daily_note_uri("a1b2c3", dir.path(), date());
        assert!(uri.starts_with("obsidian://new?"), "got: {uri}");
    }

    #[test]
    fn respects_vault_daily_note_settings() {
        let dir = tempfile::tempdir().unwrap();
        let obsidian_dir = dir.path().join(".obsidian");
        std::fs::create_dir_all(&obsidian_dir).unwrap();
        std::fs::write(
            obsidian_dir.join("daily-notes.json"),
            r#"{ "folder": "Journal", "format": "YYYY-MM-DD" }"#,
        )
        .unwrap();
        std::fs::create_dir_all(dir.path().join("Journal")).unwrap();
        std::fs::write(dir.path().join("Journal/2026-07-03.md"), "x").unwrap();
        let uri = daily_note_uri("a1b2c3", dir.path(), date());
        assert_eq!(
            uri,
            "obsidian://open?vault=a1b2c3&file=Journal%2F2026%2D07%2D03"
        );
    }
}
