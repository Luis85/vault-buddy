use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Vault {
    pub id: String,
    pub name: String,
    pub path: String,
}

/// Parses the content of Obsidian's own `obsidian.json` vault registry.
/// Any malformed input degrades to an empty list — never an error.
pub fn parse_obsidian_config(json: &str) -> Vec<Vault> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(json) else {
        return Vec::new();
    };
    let Some(vaults) = value.get("vaults").and_then(|v| v.as_object()) else {
        return Vec::new();
    };
    let mut result: Vec<Vault> = vaults
        .iter()
        .filter_map(|(id, entry)| {
            let path = entry.get("path")?.as_str()?;
            let name = vault_name_from_path(path)?;
            Some(Vault {
                id: id.clone(),
                name,
                path: path.to_string(),
            })
        })
        .collect();
    result.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    result
}

/// Last path component. Splits on both `/` and `\` because obsidian.json
/// on Windows stores backslash paths and this code also runs in tests on Unix.
fn vault_name_from_path(path: &str) -> Option<String> {
    path.rsplit(['/', '\\'])
        .find(|s| !s.is_empty())
        .map(|s| s.to_string())
}

/// `%APPDATA%\obsidian\obsidian.json` on Windows (`~/.config/obsidian/...` on Unix).
pub fn obsidian_config_path() -> Option<PathBuf> {
    dirs::config_dir().map(|dir| dir.join("obsidian").join("obsidian.json"))
}

pub fn discover_vaults() -> Vec<Vault> {
    let Some(path) = obsidian_config_path() else {
        return Vec::new();
    };
    discover_vaults_from(&path)
}

pub fn discover_vaults_from(config_path: &Path) -> Vec<Vault> {
    match std::fs::read_to_string(config_path) {
        Ok(json) => parse_obsidian_config(&json),
        Err(_) => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{
        "vaults": {
            "a1b2c3": { "path": "C:\\Users\\luis\\vaults\\Work", "ts": 1700000000000, "open": true },
            "d4e5f6": { "path": "C:\\Users\\luis\\vaults\\Personal", "ts": 1700000000001 }
        }
    }"#;

    #[test]
    fn parses_vaults_sorted_by_name() {
        let vaults = parse_obsidian_config(SAMPLE);
        assert_eq!(
            vaults,
            vec![
                Vault {
                    id: "d4e5f6".into(),
                    name: "Personal".into(),
                    path: "C:\\Users\\luis\\vaults\\Personal".into()
                },
                Vault {
                    id: "a1b2c3".into(),
                    name: "Work".into(),
                    path: "C:\\Users\\luis\\vaults\\Work".into()
                },
            ]
        );
    }

    #[test]
    fn vault_name_comes_from_last_path_component_unix_too() {
        let vaults = parse_obsidian_config(
            r#"{ "vaults": { "x": { "path": "/home/luis/vaults/Notes" } } }"#,
        );
        assert_eq!(vaults[0].name, "Notes");
    }

    #[test]
    fn malformed_json_yields_empty() {
        assert_eq!(parse_obsidian_config("not json {"), Vec::new());
    }

    #[test]
    fn missing_vaults_key_yields_empty() {
        assert_eq!(parse_obsidian_config(r#"{ "other": 1 }"#), Vec::new());
    }

    #[test]
    fn entry_without_path_is_skipped() {
        let vaults = parse_obsidian_config(
            r#"{ "vaults": { "bad": { "ts": 1 }, "good": { "path": "/v/Ok" } } }"#,
        );
        assert_eq!(vaults.len(), 1);
        assert_eq!(vaults[0].name, "Ok");
    }

    #[test]
    fn missing_config_file_yields_empty() {
        let dir = tempfile::tempdir().unwrap();
        let vaults = discover_vaults_from(&dir.path().join("nope.json"));
        assert_eq!(vaults, Vec::new());
    }
}
