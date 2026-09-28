//! The per-vault SCREEN-CAPTURE fields' parse, serialize and accessor —
//! split out of `vault_config` (Task 53).
//!
//! Why its own module: `vault_config.rs` is an allowlisted hotspot (it
//! crossed the 800-line cap long before screen capture landed, and its
//! baseline entry may only shrink), so the screen fields could not grow
//! there. The seam is the one the screen domain draws everywhere else —
//! these fields are owned by exactly one settings surface
//! (`set_screen_capture_config`, through `config_merge::merge_screen_owned`)
//! and read only by the screen capture and its export.
//!
//! The FIELDS stay on `VaultCaptureConfig` itself (every caller reads
//! `cfg.screen_fps` and friends, and a nested struct would churn all of
//! them); what moved is how they are read from and written to `config.json`.
//! `vault_config::vault_entry` takes them through struct-update syntax from
//! [`parsed`], and `serialize_vault_entry` hands its map to [`serialize`].

use serde_json::{json, Map, Value};

use crate::screen_capture_config::{normalize_fps, ScreenQuality, DEFAULT_SCREEN_FOLDER};
use crate::vault_config::{template_field, VaultCaptureConfig};

impl VaultCaptureConfig {
    /// The vault's screen-capture folder, defaulting to "Screen Captures".
    /// Mirrors `documents_root` / `tasks_root`.
    pub fn screen_capture_root(&self) -> &str {
        self.screen_capture_folder
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(DEFAULT_SCREEN_FOLDER)
    }
}

/// A config carrying `entry`'s screen fields — each parsed defensively, one
/// malformed value defaulting only itself — and the defaults everywhere
/// else. `vault_entry` takes the screen fields from it with `..`.
pub(crate) fn parsed(entry: &Value) -> VaultCaptureConfig {
    let defaults = VaultCaptureConfig::default();
    VaultCaptureConfig {
        screen_capture_folder: entry
            .get("screenCaptureFolder")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        screen_capture_date_folders: entry
            .get("screenCaptureDateFolders")
            .and_then(|v| v.as_bool())
            .unwrap_or(defaults.screen_capture_date_folders),
        screen_quality: entry
            .get("screenQuality")
            .and_then(|v| v.as_str())
            .and_then(ScreenQuality::from_key)
            .unwrap_or(defaults.screen_quality),
        screen_fps: entry
            .get("screenFps")
            .and_then(|v| v.as_u64())
            // Validate the u64 BEFORE narrowing to u32, not after: a
            // hand-edited `"screenFps": 4294967356` truncates to 60 under
            // `as u32` (4294967356 - 2^32 == 60), which `normalize_fps`
            // would then read as the legal 60fps value instead of falling
            // to the 30 default — the narrowing silently aliased an
            // out-of-range value onto a real one. `u32::try_from` rejects
            // anything that doesn't fit before `normalize_fps` ever sees it.
            .map(|v| u32::try_from(v).map_or(30, normalize_fps))
            .unwrap_or(defaults.screen_fps),
        screen_create_note: entry
            .get("screenCreateNote")
            .and_then(|v| v.as_bool())
            .unwrap_or(defaults.screen_create_note),
        screen_extra_frontmatter: template_field(entry, "screenExtraFrontmatter"),
        screen_body_template: template_field(entry, "screenBodyTemplate"),
        // Only a real JSON `true` turns stems on: a hand-edited "yes" or 1 is
        // malformed, and malformed defaults to OFF — the capture a vault made
        // before this setting existed.
        screen_audio_stems: entry
            .get("screenAudioStems")
            .and_then(|v| v.as_bool())
            .unwrap_or(defaults.screen_audio_stems),
        ..defaults
    }
}

/// Write the screen fields into a vault entry. Defaults are omitted, so an
/// existing `config.json` gains no keys just because the app learned about
/// screen capture.
pub(crate) fn serialize(v: &VaultCaptureConfig, entry: &mut Map<String, Value>) {
    if let Some(folder) = &v.screen_capture_folder {
        entry.insert("screenCaptureFolder".to_string(), json!(folder));
    }
    if v.screen_capture_date_folders {
        entry.insert("screenCaptureDateFolders".to_string(), json!(true));
    }
    if v.screen_quality != ScreenQuality::default() {
        entry.insert(
            "screenQuality".to_string(),
            json!(v.screen_quality.as_key()),
        );
    }
    if v.screen_fps != 30 {
        entry.insert("screenFps".to_string(), json!(v.screen_fps));
    }
    if !v.screen_create_note {
        entry.insert("screenCreateNote".to_string(), json!(false));
    }
    if let Some(t) = &v.screen_extra_frontmatter {
        entry.insert("screenExtraFrontmatter".to_string(), json!(t));
    }
    if let Some(t) = &v.screen_body_template {
        entry.insert("screenBodyTemplate".to_string(), json!(t));
    }
    if v.screen_audio_stems {
        entry.insert("screenAudioStems".to_string(), json!(true));
    }
}

#[cfg(test)]
mod tests {
    use crate::vault_config::{serialize_vault_entry, vault_entry, VaultCaptureConfig};

    // The split's own pin (Task 53): EVERY screen field — the seven that
    // moved plus `screenAudioStems` — survives parse -> serialize with no key
    // lost, renamed or added. Compared as JSON, not as a struct against
    // itself, so a key `serialize` forgets fails here even though the struct
    // round trip below would still agree with itself.
    #[test]
    fn vault_config_round_trips_every_screen_field_after_the_split() {
        let entry = serde_json::json!({
            "mode": "voice-note",
            "screenCaptureFolder": "Demos/Screens",
            "screenCaptureDateFolders": true,
            "screenQuality": "high",
            "screenFps": 60,
            "screenCreateNote": false,
            "screenExtraFrontmatter": "project: acme",
            "screenBodyTemplate": "## Notes",
            "screenAudioStems": true
        });
        let written = serialize_vault_entry(&vault_entry(&entry));
        for (key, value) in entry.as_object().expect("an object") {
            assert_eq!(
                written.get(key),
                Some(value),
                "{key} did not survive the split"
            );
        }
        let screen_keys = written.keys().filter(|k| k.starts_with("screen")).count();
        assert_eq!(screen_keys, 8, "exactly the eight screen keys: {written:?}");
    }

    // Stems are OFF unless the user turned them on (Task 53): a default
    // config, an absent key and a malformed value all read as off, so every
    // existing vault keeps recording exactly what it recorded before — and
    // only an explicit `true` turns them on.
    #[test]
    fn stems_default_off() {
        assert!(!VaultCaptureConfig::default().screen_audio_stems);
        assert!(!vault_entry(&serde_json::json!({})).screen_audio_stems);
        assert!(!vault_entry(&serde_json::json!({ "screenAudioStems": "yes" })).screen_audio_stems);
        assert!(!vault_entry(&serde_json::json!({ "screenAudioStems": 1 })).screen_audio_stems);
        assert!(
            vault_entry(&serde_json::json!({ "screenAudioStems": true })).screen_audio_stems,
            "an explicit true is the one way on"
        );
    }

    #[test]
    fn screen_capture_defaults_are_flat_balanced_30_with_a_note() {
        let v = VaultCaptureConfig::default();
        assert_eq!(v.screen_capture_folder, None);
        assert_eq!(v.screen_capture_root(), "Screen Captures");
        assert!(!v.screen_capture_date_folders, "flat is the default layout");
        assert_eq!(
            v.screen_quality,
            crate::screen_capture_config::ScreenQuality::Balanced
        );
        assert_eq!(v.screen_fps, 30);
        assert!(v.screen_create_note);
        assert_eq!(v.screen_extra_frontmatter, None);
        assert_eq!(v.screen_body_template, None);
    }

    #[test]
    fn screen_capture_fields_parse() {
        let entry = serde_json::json!({
            "screenCaptureFolder": "Demos",
            "screenCaptureDateFolders": true,
            "screenQuality": "high",
            "screenFps": 60,
            "screenCreateNote": false,
            "screenExtraFrontmatter": "project: acme",
            "screenBodyTemplate": "## Notes"
        });
        let v = vault_entry(&entry);
        assert_eq!(v.screen_capture_folder.as_deref(), Some("Demos"));
        assert_eq!(v.screen_capture_root(), "Demos");
        assert!(v.screen_capture_date_folders);
        assert_eq!(
            v.screen_quality,
            crate::screen_capture_config::ScreenQuality::High
        );
        assert_eq!(v.screen_fps, 60);
        assert!(!v.screen_create_note);
        assert_eq!(v.screen_extra_frontmatter.as_deref(), Some("project: acme"));
        assert_eq!(v.screen_body_template.as_deref(), Some("## Notes"));
    }

    // `screen_capture_root`'s `.map(str::trim).filter(|s| !s.is_empty())`
    // pair was previously asserted only at its two endpoints (None → default,
    // an already-clean "Demos" → "Demos") — never the trim/blank branch in
    // between. Without this, a refactor that dropped the trim-or-filter step
    // would pass every existing test while a whitespace-only folder value
    // silently targeted a literal blank-named folder on disk (this accessor
    // is the one Phase 2's capture path builder depends on for every write).
    #[test]
    fn screen_capture_root_trims_and_treats_blank_as_unset() {
        let blank = vault_entry(&serde_json::json!({ "screenCaptureFolder": "   " }));
        assert_eq!(
            blank.screen_capture_root(),
            "Screen Captures",
            "whitespace-only folder falls back to the default"
        );

        let padded = vault_entry(&serde_json::json!({ "screenCaptureFolder": " Demos " }));
        assert_eq!(
            padded.screen_capture_root(),
            "Demos",
            "surrounding whitespace is trimmed off a real value"
        );
    }

    // Per-field defensive parse: one malformed value defaults ONLY itself.
    // A derived deserializer would reject the whole entry and silently reset
    // every other setting in the vault.
    #[test]
    fn malformed_screen_fields_default_locally_not_globally() {
        let entry = serde_json::json!({
            "screenQuality": "ultra",
            "screenFps": 144,
            "screenCaptureDateFolders": "yes",
            "screenCaptureFolder": "Demos"
        });
        let v = vault_entry(&entry);
        assert_eq!(
            v.screen_quality,
            crate::screen_capture_config::ScreenQuality::Balanced
        );
        assert_eq!(v.screen_fps, 30);
        assert!(!v.screen_capture_date_folders);
        assert_eq!(
            v.screen_capture_folder.as_deref(),
            Some("Demos"),
            "the valid sibling survives"
        );
    }

    // Regression: `screenFps` validation must run on the raw u64 BEFORE
    // narrowing to u32, not after. `4294967356 as u32` truncates to 60
    // (4294967356 - 2^32 == 60), which `normalize_fps` would then read as
    // the legal 60fps value — an out-of-range hand-edited value silently
    // aliasing onto a real one instead of falling to the 30 default.
    #[test]
    fn an_out_of_range_screen_fps_falls_to_the_default_not_a_truncated_alias() {
        let entry = serde_json::json!({ "screenFps": 4_294_967_356u64 });
        let v = vault_entry(&entry);
        assert_eq!(
            v.screen_fps, 30,
            "a u64 that doesn't fit u32 must default, never truncate onto a legal value"
        );
    }

    #[test]
    fn screen_capture_fields_round_trip_through_serialize() {
        let entry = serde_json::json!({
            "screenCaptureFolder": "Demos",
            "screenCaptureDateFolders": true,
            "screenQuality": "low",
            "screenFps": 60,
            "screenCreateNote": false,
            "screenExtraFrontmatter": "project: acme",
            "screenBodyTemplate": "## Notes"
        });
        let v = vault_entry(&entry);
        let round_tripped = vault_entry(&serde_json::Value::Object(serialize_vault_entry(&v)));
        assert_eq!(round_tripped, v);
    }

    // Regression: an existing config.json must not gain keys just because the
    // app learned about screen capture. Defaults are omitted, matching how
    // every other optional field is serialized.
    #[test]
    fn default_screen_fields_emit_no_keys() {
        let entry = serialize_vault_entry(&VaultCaptureConfig::default());
        for key in [
            "screenCaptureFolder",
            "screenCaptureDateFolders",
            "screenQuality",
            "screenFps",
            "screenCreateNote",
            "screenExtraFrontmatter",
            "screenBodyTemplate",
            "screenAudioStems",
        ] {
            assert!(
                !entry.contains_key(key),
                "{key} should be omitted at its default"
            );
        }
    }
}
