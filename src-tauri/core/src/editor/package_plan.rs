//! The pure half of Task 39's project files (F-40, ADR R5/R9): which
//! assets a package carries, what the files are called, and how an
//! imported project is re-keyed. No filesystem and no Tauri types -- the
//! shell's `editor::package_commands`/`package_import` own the dialogs, the
//! temp files and the store install, and call these.
//!
//! Two formats, one envelope:
//! - **portable** (`.vbproject.zip`, `package::write_package`): the saved
//!   workspace envelope plus every AVAILABLE original the project (or a
//!   retained product's snapshot, A17) depends on.
//! - **lightweight** (`.vbproject.json`): the saved workspace envelope
//!   alone; every original is reconnected later.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use super::fingerprint::assets_referenced;
use super::model::{Asset, AssetKind, Builtin, MediaType};
use super::model_cues::WorkspaceEnvelope;
use crate::device_names::is_reserved_device_name;

/// `editor_export_package`'s `format` argument and `PackageReceipt.format`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PackageFormat {
    Portable,
    Lightweight,
}

impl PackageFormat {
    /// The double extension every file of this format carries.
    pub fn suffix(self) -> &'static str {
        match self {
            Self::Portable => ".vbproject.zip",
            Self::Lightweight => ".vbproject.json",
        }
    }

    /// The format a file name names by its LAST extension (`.zip` or
    /// `.json`, any case) -- what an opened file is read as. `None` for
    /// anything else, which the import refuses rather than sniffs.
    pub fn of_file_name(name: &str) -> Option<Self> {
        let lower = name.to_ascii_lowercase();
        if lower.ends_with(".zip") {
            Some(Self::Portable)
        } else if lower.ends_with(".json") {
            Some(Self::Lightweight)
        } else {
            None
        }
    }

    /// The single last extension (`.zip`/`.json`) the double one ends in.
    fn last_extension(self) -> &'static str {
        match self {
            Self::Portable => ".zip",
            Self::Lightweight => ".json",
        }
    }
}

/// The longest file-name stem `suggested_file_name` proposes.
const MAX_SUGGESTED_STEM_CHARS: usize = 100;

/// Every definition of each asset id: the live graph's first, then each
/// retained product snapshot's, in record order. Each graph is validated on
/// its own, but nothing makes them agree (`package::cross_check`'s note).
pub fn asset_definitions(env: &WorkspaceEnvelope) -> BTreeMap<&str, Vec<&Asset>> {
    let snapshots = env
        .record
        .products
        .iter()
        .filter_map(|p| p.snapshot.as_deref());
    let mut out: BTreeMap<&str, Vec<&Asset>> = BTreeMap::new();
    for asset in std::iter::once(&env.project)
        .chain(snapshots)
        .flat_map(|p| &p.assets)
    {
        out.entry(asset.id.as_str()).or_default().push(asset);
    }
    out
}

/// Whether an asset with these definitions is backed by a real file: every
/// kind but a `card` builtin (synthesized from the project) and a linked
/// asset (detached audio, whose bytes are its source's). `package::
/// cross_check`'s rule -- a `screen` builtin IS a file (the staged capture).
fn file_backed(defs: &[&Asset]) -> bool {
    defs.iter()
        .any(|a| a.builtin != Some(Builtin::Card) && a.linked_asset.is_none())
}

/// Every file-backed asset id the envelope defines, referenced or not --
/// the ids an imported project gets a `sources.json` record for.
pub fn file_backed_asset_ids(env: &WorkspaceEnvelope) -> BTreeSet<String> {
    asset_definitions(env)
        .into_iter()
        .filter(|(_, defs)| file_backed(defs))
        .map(|(id, _)| id.to_string())
        .collect()
}

/// The file-backed assets the project or a retained snapshot actually
/// USES -- the ones a portable package carries bytes for when available
/// (`package::cross_check` refuses media nothing references).
pub fn assets_needing_media(env: &WorkspaceEnvelope) -> BTreeSet<String> {
    let defs = asset_definitions(env);
    assets_referenced(&env.project, &env.record.products)
        .into_iter()
        .filter(|id| defs.get(id.as_str()).is_some_and(|d| file_backed(d)))
        .collect()
}

/// Why this envelope's asset ids cannot become package file names, if they
/// cannot: two ids that differ only by case (`media/Ab.mp4` and
/// `media/ab.mp4` are one file on Windows, and `inspect_archive` refuses
/// case-folded duplicates), or an id that is a Windows device name
/// (`media/CON.mp4` opens the console). Checked BEFORE anything is written.
pub fn asset_id_problem(env: &WorkspaceEnvelope) -> Option<String> {
    let mut folded: BTreeMap<String, &str> = BTreeMap::new();
    for id in asset_definitions(env).into_keys() {
        if is_reserved_device_name(id) {
            return Some(format!(
                "The asset id {id:?} is a reserved Windows device name, so its media cannot be saved as a file."
            ));
        }
        if let Some(other) = folded.insert(id.to_lowercase(), id) {
            return Some(format!(
                "The asset ids {other:?} and {id:?} differ only by case, so their media cannot share a project file."
            ));
        }
    }
    None
}

/// A file name's final extension, lowercased, when it is 1-8 ASCII
/// alphanumerics (`package::validate_entry_name`'s media rule); `None`
/// otherwise.
pub fn media_extension(file_name: &str) -> Option<String> {
    let (_, ext) = file_name.rsplit_once('.')?;
    ((1..=8).contains(&ext.len()) && ext.bytes().all(|b| b.is_ascii_alphanumeric()))
        .then(|| ext.to_ascii_lowercase())
}

/// `<assetId>.<ext>` for a source whose bytes are not here (a lightweight
/// import, or an original the portable file did not carry): the extension
/// of the name the asset was imported under when it has a usable one,
/// else one for its kind. Only a placeholder -- reconnecting replaces it.
pub fn placeholder_file_name(asset: &Asset) -> String {
    let ext = asset
        .original_name
        .as_deref()
        .and_then(media_extension)
        .or_else(|| media_extension(&asset.name))
        .unwrap_or_else(|| {
            let by_kind = match (asset.media_type, asset.kind) {
                (Some(MediaType::Image), _) => "png",
                (None, AssetKind::Audio) => "m4a",
                (None, AssetKind::Video) => "mp4",
            };
            by_kind.to_string()
        });
    format!("{}.{ext}", asset.id)
}

/// The file name a chosen save target is normalized to: it always ends in
/// the format's double extension, so a user who typed `Demo` or picked
/// `Demo.zip` gets `Demo.vbproject.zip`.
pub fn package_file_name(chosen: &str, format: PackageFormat) -> String {
    let lower = chosen.to_ascii_lowercase();
    if lower.ends_with(format.suffix()) {
        return chosen.to_string();
    }
    let stem = if lower.ends_with(format.last_extension()) {
        &chosen[..chosen.len() - format.last_extension().len()]
    } else {
        chosen
    };
    format!("{stem}{}", format.suffix())
}

/// The save dialog's proposed name, from the project title: characters no
/// Windows file name may hold become `-`, trailing dots/spaces go, a
/// device-name stem is prefixed, and an empty result falls back.
pub fn suggested_file_name(title: &str, format: PackageFormat) -> String {
    let mapped: String = title
        .chars()
        .map(|c| {
            if c.is_control() || r#"<>:"/\|?*"#.contains(c) {
                '-'
            } else {
                c
            }
        })
        .take(MAX_SUGGESTED_STEM_CHARS)
        .collect();
    let trimmed = mapped.trim().trim_end_matches(['.', ' ']);
    let stem = if trimmed.is_empty() {
        "Tutorial project".to_string()
    } else if is_reserved_device_name(trimmed) {
        format!("Project {trimmed}")
    } else {
        trimmed.to_string()
    };
    format!("{stem}{}", format.suffix())
}

/// Re-key an imported envelope onto `new_id` (the manifest's id already
/// names a project in this store): the project, its record, each retained
/// product's owner and each snapshot's own id.
pub fn rekey_envelope(env: &mut WorkspaceEnvelope, new_id: &str) {
    env.project.id = new_id.to_string();
    env.record.id = new_id.to_string();
    for product in &mut env.record.products {
        product.project_id = new_id.to_string();
        if let Some(snapshot) = product.snapshot.as_mut() {
            snapshot.id = new_id.to_string();
        }
    }
}

#[cfg(test)]
#[path = "package_plan_tests.rs"]
mod tests;
