pub mod app_diagnostics;
pub mod capture_config;
pub mod capture_note;
pub mod capture_paths;
pub mod checkpoint;
pub mod crash;
pub mod daily_notes;
pub mod discovery;
pub mod process;
pub mod sync_util;
pub mod transcript;
pub mod uri;

use chrono::NaiveDate;
use std::path::Path;

/// Builds the URI that opens today's daily note for a vault:
/// `obsidian://open` if the note file already exists, `obsidian://new`
/// otherwise — Obsidian itself performs the creation. Vault Buddy never
/// writes into a vault. `vault_id` is the unique key from obsidian.json.
pub fn daily_note_uri(vault_id: &str, vault_path: &Path, date: NaiveDate) -> String {
    let settings = daily_notes::load_settings(vault_path);
    let rel = daily_notes::daily_note_rel_path(&settings, date);
    let exists = vault_path.join(format!("{rel}.md")).exists();
    if exists {
        uri::open_file_uri(vault_id, &rel)
    } else {
        uri::new_file_uri(vault_id, &rel)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn date() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 7, 3).unwrap()
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
