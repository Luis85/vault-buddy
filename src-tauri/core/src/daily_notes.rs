use chrono::NaiveDate;
use std::path::Path;

pub const DEFAULT_FORMAT: &str = "YYYY-MM-DD";

#[derive(Debug, Clone, PartialEq)]
pub struct DailyNoteSettings {
    pub folder: String,
    pub format: String,
}

impl Default for DailyNoteSettings {
    fn default() -> Self {
        Self {
            folder: String::new(),
            format: DEFAULT_FORMAT.to_string(),
        }
    }
}

/// Parses the content of a vault's `.obsidian/daily-notes.json`.
/// Any malformed input degrades to the defaults.
pub fn parse_settings(json: &str) -> DailyNoteSettings {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(json) else {
        return DailyNoteSettings::default();
    };
    let folder = value
        .get("folder")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim_matches('/')
        .to_string();
    let format = value
        .get("format")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .unwrap_or(DEFAULT_FORMAT)
        .to_string();
    DailyNoteSettings { folder, format }
}

pub fn load_settings(vault_path: &Path) -> DailyNoteSettings {
    let path = vault_path.join(".obsidian").join("daily-notes.json");
    match std::fs::read_to_string(path) {
        Ok(json) => parse_settings(&json),
        Err(_) => DailyNoteSettings::default(),
    }
}

/// Renders `format` for `date`, substituting the supported moment tokens
/// `YYYY`, `MM`, `DD`. Every run of consecutive letters must be exactly one
/// supported token; any other run (`dddd`, `MMMM`, `YYYYMMDD`, …) means the
/// format uses moment features we don't support — fall back to the default
/// format rather than risk telling Obsidian to create a misnamed note.
/// (Naive `str::replace` would consume `MMMM` as two `MM`s: "2026-0707-03".)
pub fn render_format(format: &str, date: NaiveDate) -> String {
    substitute_tokens(format, date).unwrap_or_else(|| {
        substitute_tokens(DEFAULT_FORMAT, date).expect("default format is valid")
    })
}

/// `None` when the format contains a letter run that is not exactly one
/// supported token.
fn substitute_tokens(format: &str, date: NaiveDate) -> Option<String> {
    let chars: Vec<char> = format.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_ascii_alphabetic() {
            let start = i;
            while i < chars.len() && chars[i].is_ascii_alphabetic() {
                i += 1;
            }
            let run: String = chars[start..i].iter().collect();
            match run.as_str() {
                "YYYY" => out.push_str(&date.format("%Y").to_string()),
                "MM" => out.push_str(&date.format("%m").to_string()),
                "DD" => out.push_str(&date.format("%d").to_string()),
                _ => return None,
            }
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    Some(out)
}

/// Vault-relative path of the daily note, without the `.md` extension —
/// the form the `obsidian://` URI `file` parameter expects.
pub fn daily_note_rel_path(settings: &DailyNoteSettings, date: NaiveDate) -> String {
    let name = render_format(&settings.format, date);
    if settings.folder.is_empty() {
        name
    } else {
        format!("{}/{}", settings.folder, name)
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
    fn renders_default_format() {
        assert_eq!(render_format("YYYY-MM-DD", date()), "2026-07-03");
    }

    #[test]
    fn renders_format_with_subfolders() {
        assert_eq!(
            render_format("YYYY/MM/YYYY-MM-DD", date()),
            "2026/07/2026-07-03"
        );
    }

    #[test]
    fn unsupported_tokens_fall_back_to_default() {
        // "dddd" (weekday name) is a moment token we don't support; a wrong
        // literal path would make Obsidian create a misnamed note.
        assert_eq!(render_format("YYYY-MM-DD dddd", date()), "2026-07-03");
    }

    #[test]
    fn repeated_token_runs_fall_back_instead_of_double_substituting() {
        // "MMMM" is moment's full month name. Consuming it as two "MM"s
        // would render "2026-0707-03" — a misnamed note that dodges the
        // fallback check. Letter runs must match a supported token exactly.
        assert_eq!(render_format("YYYY-MMMM-DD", date()), "2026-07-03");
        assert_eq!(render_format("YYYYMMDD", date()), "2026-07-03");
    }

    #[test]
    fn parses_settings() {
        let s = parse_settings(r#"{ "folder": "Daily Notes", "format": "YYYY-MM-DD" }"#);
        assert_eq!(s.folder, "Daily Notes");
        assert_eq!(s.format, "YYYY-MM-DD");
    }

    #[test]
    fn trims_slashes_from_folder() {
        let s = parse_settings(r#"{ "folder": "/Daily/" }"#);
        assert_eq!(s.folder, "Daily");
    }

    #[test]
    fn malformed_settings_fall_back_to_default() {
        assert_eq!(parse_settings("garbage"), DailyNoteSettings::default());
        assert_eq!(
            parse_settings(r#"{ "format": "" }"#),
            DailyNoteSettings::default()
        );
    }

    #[test]
    fn rel_path_joins_folder_and_name() {
        let s = DailyNoteSettings {
            folder: "Daily".into(),
            format: "YYYY-MM-DD".into(),
        };
        assert_eq!(daily_note_rel_path(&s, date()), "Daily/2026-07-03");
    }

    #[test]
    fn rel_path_without_folder_is_just_the_name() {
        assert_eq!(
            daily_note_rel_path(&DailyNoteSettings::default(), date()),
            "2026-07-03"
        );
    }

    #[test]
    fn missing_settings_file_yields_defaults() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(load_settings(dir.path()), DailyNoteSettings::default());
    }

    #[test]
    fn loads_settings_from_vault() {
        let dir = tempfile::tempdir().unwrap();
        let obsidian_dir = dir.path().join(".obsidian");
        std::fs::create_dir_all(&obsidian_dir).unwrap();
        std::fs::write(
            obsidian_dir.join("daily-notes.json"),
            r#"{ "folder": "Journal", "format": "YYYY/MM-DD" }"#,
        )
        .unwrap();
        let s = load_settings(dir.path());
        assert_eq!(s.folder, "Journal");
        assert_eq!(s.format, "YYYY/MM-DD");
    }
}
