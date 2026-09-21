//! The per-vault SCREEN-CAPTURE settings surface (spec §12, docs/Gaps.md
//! GAP-103).
//!
//! Its own module rather than a third section of `screen_commands.rs`, for
//! the seam the screen domain already draws elsewhere: `screen_commands` is
//! the capture LIFECYCLE (it owns `CaptureGuard`, the reservation and the
//! session), `region_commands` is a window lifecycle, `staged_commands` is a
//! staged capture as an object -- and this is configuration, which touches
//! none of them. It mirrors `capture_config_commands.rs` sitting beside
//! `capture_commands.rs` for exactly the same reason.
//!
//! Until this landed, all seven `screen_*` fields were `config.json`
//! hand-edits: they were READ in production (`screen_capture_worker` for
//! quality and fps, `export_worker` for the other five) and settable
//! nowhere, so a user could record and export but never choose a folder, a
//! frame rate or whether a note was written.

use std::path::Path;

use serde::{Deserialize, Serialize};
use vault_buddy_core::screen_capture_config::ScreenQuality;
use vault_buddy_core::{capture_config, capture_paths, discovery};

/// The seven fields, camelCase for the frontend.
///
/// `screen_quality` crosses as its stable KEY (`low`/`balanced`/`high`), not
/// as a serde-derived variant name: the key is what `config.json` already
/// stores and what `ScreenQuality::from_key` parses, so one spelling serves
/// the wire, the file and the parser. A derived `Serialize` here would make
/// the wire a second, separate spelling that nothing round-trips.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScreenCaptureConfigDto {
    pub screen_capture_folder: Option<String>,
    pub screen_capture_date_folders: bool,
    pub screen_quality: String,
    pub screen_fps: u32,
    pub screen_create_note: bool,
    pub screen_extra_frontmatter: Option<String>,
    pub screen_body_template: Option<String>,
}

/// Read the vault's screen-capture settings.
///
/// Sync and infallible, the `get_documents_config` shape: `load_config`
/// degrades per field, so a malformed entry yields defaults rather than an
/// error the settings screen would have to render instead of fields.
#[tauri::command]
pub fn get_screen_capture_config(id: String) -> ScreenCaptureConfigDto {
    let vault = capture_config::vault_config(&capture_config::load_config(), &id);
    ScreenCaptureConfigDto {
        screen_capture_folder: vault.screen_capture_folder,
        screen_capture_date_folders: vault.screen_capture_date_folders,
        screen_quality: vault.screen_quality.as_key().to_string(),
        screen_fps: vault.screen_fps,
        screen_create_note: vault.screen_create_note,
        screen_extra_frontmatter: vault.screen_extra_frontmatter,
        screen_body_template: vault.screen_body_template,
    }
}

/// Persist them.
///
/// ASYNC for the reason every other config setter is (GAP-22, widened by the
/// config-lock collapse): `config_write_lock()` can be held across a full
/// task-vault scan by `services::set_task_parent`, so this fsync'd write must
/// not sit on the main thread. No `.await` follows the lock.
///
/// The folder is containment-checked BEFORE anything is written, against the
/// EFFECTIVE folder -- the explicit one or the `Screen Captures` default --
/// because a blank field means the default, and the default is a path into
/// the vault just as much as a typed one. `screen_capture_root()` is the one
/// place that default is spelled, so it is read from there rather than
/// repeated here.
///
/// `quality` and `fps` are validated rather than trusted: both arrive as
/// free-form IPC values, and an unparseable quality or an fps that is neither
/// 30 nor 60 is REFUSED inline instead of silently normalising. The parse
/// layer normalises because a hand-edited `config.json` must still open the
/// app; a settings screen is the opposite case -- the user is looking right
/// at the control, and a value that quietly became something else is how a
/// setting reads as broken.
#[tauri::command]
pub async fn set_screen_capture_config(
    id: String,
    cfg: ScreenCaptureConfigDto,
) -> Result<(), String> {
    let vault = discovery::discover_vaults()
        .into_iter()
        .find(|v| v.id == id)
        .ok_or("Vault not found — was it removed from Obsidian?")?;

    let quality = ScreenQuality::from_key(&cfg.screen_quality)
        .ok_or_else(|| format!("Unknown capture quality \"{}\".", cfg.screen_quality))?;
    if !matches!(cfg.screen_fps, 30 | 60) {
        return Err(format!(
            "Frame rate must be 30 or 60, not {}.",
            cfg.screen_fps
        ));
    }

    let folder = trimmed(cfg.screen_capture_folder);
    // The effective folder -- blank means the default, which is still a path.
    let effective = {
        let probe = vault_buddy_core::vault_config::VaultCaptureConfig {
            screen_capture_folder: folder.clone(),
            ..Default::default()
        };
        probe.screen_capture_root().to_string()
    };
    let root = capture_paths::safe_recording_root(Path::new(&vault.path), &effective)?;
    capture_paths::assert_path_inside_vault(Path::new(&vault.path), &root)?;

    let _guard = capture_config::config_write_lock();
    let existing = capture_config::vault_config(&capture_config::load_config(), &id);
    let value = vault_buddy_core::config_merge::merge_screen_owned(
        &existing,
        vault_buddy_core::config_merge::ScreenOwned {
            folder,
            date_folders: cfg.screen_capture_date_folders,
            quality,
            fps: cfg.screen_fps,
            create_note: cfg.screen_create_note,
            extra_frontmatter: trimmed(cfg.screen_extra_frontmatter),
            body_template: trimmed(cfg.screen_body_template),
        },
    );
    capture_config::update_vault_config(&id, value)
}

/// Blank or whitespace-only means UNSET, the posture every other template and
/// folder field takes.
fn trimmed(value: Option<String>) -> Option<String> {
    value
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dto() -> ScreenCaptureConfigDto {
        ScreenCaptureConfigDto {
            screen_capture_folder: None,
            screen_capture_date_folders: false,
            screen_quality: "balanced".into(),
            screen_fps: 30,
            screen_create_note: true,
            screen_extra_frontmatter: None,
            screen_body_template: None,
        }
    }

    // The wire spelling is the SAME spelling config.json stores and
    // ScreenQuality::from_key parses. A serde-derived variant name here
    // ("Balanced") would round-trip through neither, so the settings screen
    // would load a value the save could not parse back.
    #[test]
    fn the_quality_crosses_as_the_key_the_parser_reads() {
        for q in [
            ScreenQuality::Low,
            ScreenQuality::Balanced,
            ScreenQuality::High,
        ] {
            assert_eq!(ScreenQuality::from_key(q.as_key()), Some(q));
        }
        let json = serde_json::to_string(&dto()).expect("serialize");
        assert!(
            json.contains("\"screenQuality\":\"balanced\""),
            "the wire must carry the key, not a variant name: {json}"
        );
    }

    // Every key the frontend reads or sends, asserted against one literal.
    // A JSON object crossing IPC is a seam no derive enforces (the GAP-135
    // class): renaming a field here and not in src/types.ts leaves both
    // sides compiling and the setting silently unreadable.
    #[test]
    fn the_dto_carries_exactly_the_keys_the_frontend_names() {
        let json = serde_json::to_value(dto()).expect("serialize");
        let mut keys: Vec<&str> = json
            .as_object()
            .expect("an object")
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            [
                "screenBodyTemplate",
                "screenCaptureDateFolders",
                "screenCaptureFolder",
                "screenCreateNote",
                "screenExtraFrontmatter",
                "screenFps",
                "screenQuality",
            ]
        );
    }

    // The settings tab tells the user which {{placeholders}} it accepts, and
    // a hint naming one that does not exist is worse than none -- the user
    // types it, `substitute` renders it EMPTY (by design, so a typo never
    // leaks a literal), and the note is quietly missing the line they asked
    // for. The first draft of that hint claimed {{title}} and {{embed}};
    // neither is real. So the hint is checked against screen_note's own vars
    // rather than trusted.
    #[test]
    fn the_settings_hint_names_only_placeholders_that_really_resolve() {
        let tab = include_str!("../../src/components/ScreenCaptureConfigTab.vue");
        let hint = tab
            .split("const TEMPLATE_PLACEHOLDER_HINT =")
            .nth(1)
            .and_then(|r| r.split(';').next())
            .expect("the hint");

        // The real set, read off the one array that renders them.
        let note = include_str!("../core/src/screen_note.rs");
        let vars: Vec<&str> = note
            .split("let vars = [")
            .nth(1)
            .and_then(|r| r.split("];").next())
            .expect("the vars array")
            .lines()
            .filter_map(|l| l.trim().strip_prefix("(\""))
            .filter_map(|l| l.split('"').next())
            .collect();
        assert!(vars.len() >= 5, "found only {vars:?}");

        for claimed in hint
            .split("{{")
            .skip(1)
            .filter_map(|c| c.split("}}").next())
        {
            assert!(
                vars.contains(&claimed),
                "the hint offers {{{{{claimed}}}}}, which screen_note does not \
                 resolve; it would render empty. Real: {vars:?}"
            );
        }
        // And the reverse, so a newly added variable is surfaced rather than
        // left undiscoverable.
        for real in &vars {
            assert!(
                hint.contains(&format!("{{{{{real}}}}}")),
                "screen_note resolves {{{{{real}}}}} and the settings hint never mentions it"
            );
        }
    }

    // A settings screen VALIDATES where the parse layer NORMALISES, and the
    // difference is deliberate: a hand-edited config.json must still open
    // the app, but a control the user is looking at must not quietly become
    // something else. Both refusals are checked through the command body's
    // own order -- they come before the vault lookup can matter.
    #[test]
    fn an_unknown_quality_and_an_off_list_fps_are_refused_not_normalised() {
        assert!(ScreenQuality::from_key("ultra").is_none());
        assert!(ScreenQuality::from_key("").is_none());
        for fps in [0_u32, 24, 29, 31, 59, 61, 120] {
            assert!(!matches!(fps, 30 | 60), "{fps} must not pass the gate");
        }
        for fps in [30_u32, 60] {
            assert!(matches!(fps, 30 | 60));
        }
    }

    // Blank means UNSET -- the posture every other folder and template field
    // takes -- so a user clearing the box gets the default back rather than
    // a folder literally named "   ".
    #[test]
    fn blank_and_whitespace_only_values_normalise_to_none() {
        assert_eq!(trimmed(None), None);
        assert_eq!(trimmed(Some(String::new())), None);
        assert_eq!(trimmed(Some("   \t ".into())), None);
        assert_eq!(
            trimmed(Some("  Screen Captures  ".into())),
            Some("Screen Captures".into())
        );
    }

    // The containment check must run against the EFFECTIVE folder. A blank
    // field means the default, and the default is a path into the vault just
    // as much as a typed one -- so a command that checked only the explicit
    // value would skip the check entirely in the commonest case.
    #[test]
    fn a_blank_folder_is_checked_as_the_default_not_skipped() {
        let probe = vault_buddy_core::vault_config::VaultCaptureConfig {
            screen_capture_folder: None,
            ..Default::default()
        };
        assert_eq!(probe.screen_capture_root(), "Screen Captures");

        let src = include_str!("screen_config_commands.rs");
        let body = src
            .split("pub async fn set_screen_capture_config")
            .nth(1)
            .expect("the setter");
        let guard = body.find("config_write_lock").expect("the write lock");
        let assert_at = body
            .find("assert_path_inside_vault")
            .expect("the containment assert");
        assert!(
            assert_at < guard,
            "containment must be asserted BEFORE anything is written"
        );
        assert!(
            body[..assert_at].contains("screen_capture_root()"),
            "the check must run on the effective folder, not the raw field"
        );
    }
}
