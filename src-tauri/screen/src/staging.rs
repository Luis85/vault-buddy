//! Staging: where a capture lives between "stopped" and "saved into a
//! vault".
//!
//! Staging is deliberately OUTSIDE every vault (spec 10). An unedited,
//! unapproved capture is not knowledge; putting it in a vault would make
//! discard leave litter in the user's notes and would mean the app writes
//! to a vault the user never asked it to touch.
//!
//! Everything here is pure path and string logic, so it compiles and is
//! tested on Linux — which is the point: no CI runner can record a screen,
//! so every rule that CAN be checked without a screen is checked here.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The staging directory's name under the app's local data dir.
pub const STAGING_DIR_NAME: &str = "screen-captures";

const PART_SUFFIX: &str = ".mp4.part";

pub fn staging_dir(local_app_data: &Path) -> PathBuf {
    local_app_data.join(STAGING_DIR_NAME)
}

/// The hidden in-progress file, mirroring the audio domain's `.mp3.part`.
pub fn part_file_name(base: &str) -> String {
    format!(".{base}{PART_SUFFIX}")
}

/// The infix an EXPORT temp carries between its base and `.mp4.part`.
///
/// Shared, not spelled twice: `export_part_file_name` below mints the name
/// the export worker writes, and `screen_recovery::classify` recognises an
/// abandoned one by stripping exactly this. Two independent literals is how
/// a temp stops being swept and becomes permanent litter in the user's
/// staging directory, with nothing red anywhere.
pub const EXPORT_PART_INFIX: &str = ".export";

/// The hidden in-progress file an export writes into, `.<base>.export.mp4.part`.
///
/// Deliberately the capture `.part` shape with an infix rather than a new
/// suffix: `screen_recovery` classifies every name in the staging directory
/// through `base_from_part`, and a shape that does not round-trip through it
/// is a shape the sweep never sees.
pub fn export_part_file_name(base: &str) -> String {
    part_file_name(&format!("{base}{EXPORT_PART_INFIX}"))
}

/// Recover the base from a part file name, or `None` when the name is not
/// one of ours. Phase 5's recovery deletes what this recognizes, so it is
/// deliberately strict — a loose match would let recovery delete a user
/// file that happens to sit in the staging directory.
pub fn base_from_part(file_name: &str) -> Option<String> {
    let rest = file_name.strip_prefix('.')?;
    let base = rest.strip_suffix(PART_SUFFIX)?;
    if base.is_empty() {
        return None;
    }
    Some(base.to_string())
}

pub fn mp4_file_name(base: &str) -> String {
    format!("{base}.mp4")
}

pub fn sidecar_file_name(base: &str) -> String {
    format!("{base}.json")
}

/// The infix a capture's synchronized WEBCAM file carries (F-22), in both
/// its published `<base>.webcam.mp4` and its in-progress
/// `.<base>.webcam.mp4.part` shapes. Shared for the `EXPORT_PART_INFIX`
/// reason: `screen_recovery::classify` strips exactly this to find the
/// capture a webcam part belongs to, and `staging_title` refuses a title
/// ending in it.
pub const WEBCAM_INFIX: &str = ".webcam";

/// The published webcam file, `<base>.webcam.mp4` — derivable from `base`
/// alone, since a capture has at most one webcam track.
pub fn webcam_file_name(base: &str) -> String {
    mp4_file_name(&format!("{base}{WEBCAM_INFIX}"))
}

/// The hidden in-progress webcam file, `.<base>.webcam.mp4.part`: the
/// capture `.part` shape with an infix, exactly as the export temp is, so it
/// round-trips through `base_from_part` and the recovery sweep sees it.
pub fn webcam_part_file_name(base: &str) -> String {
    part_file_name(&format!("{base}{WEBCAM_INFIX}"))
}

// A stem's naming (`stem_file_name`, `stem_part_file_name`, the recovery
// sweep's pattern) and the sidecar's `stems` block live in `staging_stems`
// (Task 53, split at this file's 800-line cap); re-exported so every path
// is unchanged.
pub use crate::staging_stems::{
    ends_with_stem_marker, listed_stems, stem_file_name, stem_part_base, stem_part_file_name,
    StemSidecar, STEM_INFIX,
};

/// Is `name`'s STEM (the text before the first `.`) one of Windows' reserved
/// device names, case-insensitively?
///
/// Windows resolves a path component whose stem matches one of these to the
/// PHYSICAL device it names, regardless of the surrounding directory or any
/// extension — `dir.join("COM1.json")` opens the COM1 serial port, not a
/// file called that (GAP-108). Public so every caller that turns untrusted
/// text into a path component can share one list instead of drifting apart:
/// `editor_commands::is_safe_base` (phase 4) refuses such a name from the
/// frontend, and `reserve_base` renames past one when minting a base.
///
/// `staging_title::sanitize_title` deliberately does NOT consume it. That
/// function yields a FRAGMENT, which `capture_paths::base_name` prefixes
/// with `YYYY-MM-DD HHmm ` before it is ever a path component — so a window
/// titled "CON" already produces the perfectly writable
/// `2026-09-21 1430 CON.mp4`, and checking there would only cost that user
/// an underscore. The rule belongs where a NAME is minted, which is here.
pub fn is_reserved_device_stem(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or(name);
    matches!(
        stem.to_ascii_uppercase().as_str(),
        "CON"
            | "PRN"
            | "AUX"
            | "NUL"
            | "COM1"
            | "COM2"
            | "COM3"
            | "COM4"
            | "COM5"
            | "COM6"
            | "COM7"
            | "COM8"
            | "COM9"
            | "LPT1"
            | "LPT2"
            | "LPT3"
            | "LPT4"
            | "LPT5"
            | "LPT6"
            | "LPT7"
            | "LPT8"
            | "LPT9"
    )
}

/// Keep a base from naming a Windows reserved DEVICE rather than a file.
///
/// Win32 resolves a path component whose stem matches `CON`/`PRN`/`AUX`/
/// `NUL`/`COM1`-`COM9`/`LPT1`-`LPT9` to the physical device, with or without
/// an extension — `dir.join("CON.mp4")` opens the console, so the capture
/// fails at `File::create` after the user has already started recording.
///
/// **In practice this never fires, and that is the point.** Every base
/// reaching `reserve_base` is `capture_paths::base_name`'s
/// `YYYY-MM-DD HHmm <title>`, so its stem begins with the date and cannot
/// equal a reserved name however the recorded window is titled. GAP-108
/// proposed putting this in `sanitize_title` instead, which would have cost
/// a window honestly titled "CON" a file called
/// `2026-09-21 1430 CON_.mp4` in exchange for no safety at all. This is the
/// backstop that makes the property hold for any FUTURE caller that mints a
/// base without that prefix, rather than renaming today's captures.
///
/// The `_` goes on the STEM, never the tail: `is_reserved_device_stem` reads
/// the text before the first dot, so `"con.mp4_"` still names the device.
fn disambiguate_reserved_device(base: &str) -> String {
    if !is_reserved_device_stem(base) {
        return base.to_string();
    }
    match base.find('.') {
        Some(dot) => format!("{}_{}", &base[..dot], &base[dot..]),
        None => format!("{base}_"),
    }
}

/// Find a free base in `dir`, suffixing ` (N)` on collision.
///
/// A base is free only when EVERY name it mints a file under is free — the
/// staged `.mp4`, the `.json` sidecar, the hidden `.mp4.part`, and (F-22)
/// the webcam file and its part. This is the pairwise reservation the
/// audio domain uses, widened to five plus any stem file (Task 53, by
/// pattern): checking only the `.mp4` would let a
/// second capture adopt a base whose in-progress `.part` still exists, and
/// the two captures would then write the same file; a leftover
/// `<base>.webcam.mp4` would be adopted as the new capture's own webcam
/// track.
///
/// It also refuses to hand back a base whose stem is a Windows reserved
/// device name (GAP-108), because this is where a base becomes the name
/// every one of those files is created under — putting the check at any one
/// of those writes would leave the others.
pub fn reserve_base(dir: &Path, base: &str) -> String {
    let base = &disambiguate_reserved_device(base);
    let stems = crate::staging_stems::stem_bases(dir);
    let free = |candidate: &str| {
        !stems.contains(candidate)
            && !dir.join(mp4_file_name(candidate)).exists()
            && !dir.join(sidecar_file_name(candidate)).exists()
            && !dir.join(part_file_name(candidate)).exists()
            && !dir.join(webcam_file_name(candidate)).exists()
            && !dir.join(webcam_part_file_name(candidate)).exists()
    };
    if free(base) {
        return base.to_string();
    }
    for n in 2..10_000 {
        let candidate = format!("{base} ({n})");
        if free(&candidate) {
            return candidate;
        }
    }
    // Astronomically unreachable; a timestamped fallback beats a panic in a
    // capture-start path.
    format!("{base} ({})", std::process::id())
}

/// What the editor needs to resume a staged capture (spec 10).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StagedSidecar {
    pub base: String,
    pub vault_id: String,
    pub source_title: String,
    /// `"screen"` or `"window"` — a string, not an enum, so a sidecar
    /// written by a future version that adds a source kind still
    /// deserializes here instead of failing the whole file.
    pub source_kind: String,
    pub inputs: Vec<String>,
    pub duration_ms: u64,
    pub paused_ms: u64,
    pub width: u32,
    pub height: u32,
    pub recorded_at: String,
    /// The in-progress edit, saved on each editor operation (phase 4).
    /// `None` until the editor touches it.
    #[serde(default)]
    pub timeline: Option<serde_json::Value>,
    /// The synchronized webcam track recorded beside the screen (F-22), or
    /// `None` — every sidecar before this build, and every capture made
    /// without a webcam. Skipped when absent so those sidecars stay
    /// byte-identical on a rewrite.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub webcam: Option<WebcamSidecar>,
    /// The capture's per-input audio stems (Task 53), in input order, or
    /// empty — every sidecar before this build and every capture recorded
    /// with stems off. Skipped when empty for the `webcam` reason. Read
    /// through `stem_files`, never trusted as paths.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stems: Vec<StemSidecar>,
    /// Every key this build does not declare, carried through verbatim.
    ///
    /// The same forward-compatibility goal `source_kind`'s own doc states,
    /// applied to the whole file rather than one field. Phase 4 made the
    /// sidecar a READ-MODIFY-WRITE surface (its timeline write rewrote it on
    /// every editor operation until Task 59; the pin write still does), and
    /// a plain struct round-trip drops
    /// whatever it does not declare — so a downgrade, a rollback, or a
    /// mixed-version sync folder would have this build silently erase a
    /// newer one's fields on the user's next keystroke. Flattening them into
    /// a catch-all makes the rewrite a genuine patch instead.
    #[serde(flatten, default)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl StagedSidecar {
    /// Set the typed `webcam` block, dropping any raw `webcam` value the
    /// lenient read parked in `extra` — both would serialize under the same
    /// key, and a JSON object with a duplicate key is read differently by
    /// different parsers.
    pub fn set_webcam(&mut self, webcam: WebcamSidecar) {
        self.extra.remove("webcam");
        self.webcam = Some(webcam);
    }
}

/// A staged capture's synchronized webcam track (F-22): which staging file
/// holds it, its pixel size, the device it came from, and where its first
/// frame sits on the capture's shared clock.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebcamSidecar {
    /// A staging file NAME (`webcam_file_name(base)`), never a path.
    pub file: String,
    pub width: u32,
    pub height: u32,
    pub device_label: String,
    /// Milliseconds on the capture's `CaptureClock` at which the webcam's
    /// first frame arrived — where migration starts its clip. Signed: a
    /// device that delivered before the screen's first frame is negative.
    pub offset_ms: i64,
    /// The webcam file's MEASURED length (GAP-199): the end of its last
    /// sample, in the file's own time. Absent on a block written before it
    /// existed, where migration falls back to deriving it from the capture.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    /// Keys a newer build adds to this block, carried through verbatim for
    /// the same reason `StagedSidecar::extra` exists.
    #[serde(flatten, default)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// Write `sidecar` as `<base>.json` inside `dir`, atomically.
///
/// **`base` is a separate argument on purpose (GAP-108).** The path is
/// derived from the CALLER's base, never from `sidecar.base`, because a
/// sidecar is a file on disk that `read_sidecar`'s own doc says may be
/// hand-edited — so in a read-modify-write (the tutorial editor's pin
/// write; phase 4's timeline write before Task 59) that field is untrusted
/// input that would
/// otherwise become a path. `dir.join("../../obsidian/obsidian.json")` lands
/// outside staging entirely, and on Windows `"C:Windows"` reaches
/// drive-C-relative and `"COM1"` reaches the serial port. The caller
/// validates its base (`sanitize_title` + `reserve_base` at capture time,
/// `editor_commands::is_safe_base` at edit time); the two assertions below
/// are the backstop that makes that a property of this function rather than
/// of every present and future caller remembering.
///
/// A disagreement between `base` and `sidecar.base` is REFUSED rather than
/// silently corrected: the two naming the same capture is the invariant the
/// read side already enforces (`load_from_staging_dir`'s mismatch refusal),
/// and writing the struct under the caller's name would leave a file whose
/// own `base` no longer matches it — un-loadable, i.e. the user's edit lost
/// anyway, but quietly.
///
/// The write itself is temp + fsync + replacing rename
/// (`capture_note::write_atomic_replacing`, the same writer
/// `core::transcript`'s sidecar uses). A plain `fs::write` truncates the
/// file before the first new byte lands, so a crash inside that window
/// leaves an empty sidecar, `read_sidecar` returns `None`, and the staged
/// `.mp4` is orphaned with no recovery sweep to find it (GAP-115) — a crash
/// mid-edit losing the WHOLE capture instead of spec 10's "at most the last
/// operation".
pub fn write_sidecar(dir: &Path, base: &str, sidecar: &StagedSidecar) -> std::io::Result<PathBuf> {
    if sidecar.base != base {
        log::warn!(
            "screen staging: refusing to write a sidecar carrying base {:?} as {:?}",
            sidecar.base,
            base
        );
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "sidecar base {:?} does not match the name it would be written under ({base:?})",
                sidecar.base
            ),
        ));
    }
    let path = dir.join(sidecar_file_name(base));
    if path.parent() != Some(dir) {
        // Containment, structurally: a base carrying a separator, a `..`, or
        // a Windows drive prefix moves the joined path out of `dir`, and
        // `join` REPLACES `self` outright for the drive-prefix case. Comparing
        // the parent catches all three without re-spelling the caller's
        // validation rules here.
        log::warn!(
            "screen staging: refusing a sidecar base that escapes the staging directory: {base:?}"
        );
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("sidecar base {base:?} does not name a file inside the staging directory"),
        ));
    }
    // The backstop for a caller that set a typed block without
    // `set_webcam`/`set_stems`: never write one key twice.
    let deduped;
    let webcam_twice = sidecar.webcam.is_some() && sidecar.extra.contains_key("webcam");
    let stems_twice = !sidecar.stems.is_empty() && sidecar.extra.contains_key("stems");
    let sidecar = if webcam_twice || stems_twice {
        let mut copy = sidecar.clone();
        if webcam_twice {
            copy.extra.remove("webcam");
        }
        if stems_twice {
            copy.extra.remove("stems");
        }
        deduped = copy;
        &deduped
    } else {
        sidecar
    };
    let json = serde_json::to_string_pretty(sidecar)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    vault_buddy_core::capture_note::write_atomic_replacing(&path, &json)?;
    Ok(path)
}

/// The largest sidecar `read_sidecar` reads (final review M4): far above
/// any this app writes, far below reading an arbitrary file whole.
pub const MAX_SIDECAR_BYTES: u64 = 1024 * 1024;

/// A sidecar's bytes: a plain file (never read through a link wearing its
/// name) within `MAX_SIDECAR_BYTES`, bounded again while reading.
fn read_sidecar_bytes(path: &Path) -> std::io::Result<Vec<u8>> {
    use std::io::Read;
    let refused = || std::io::Error::new(std::io::ErrorKind::InvalidData, "not a sidecar");
    let meta = std::fs::symlink_metadata(path)?;
    if !meta.is_file() || meta.len() > MAX_SIDECAR_BYTES {
        return Err(refused());
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(MAX_SIDECAR_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_SIDECAR_BYTES {
        return Err(refused());
    }
    Ok(bytes)
}

/// Read a sidecar, degrading to `None` on anything unreadable.
///
/// Same defensive-read posture as the rest of the vault domain: a
/// hand-edited or truncated sidecar must make ONE capture un-resumable,
/// never fail the whole staging scan.
pub fn read_sidecar(path: &Path) -> Option<StagedSidecar> {
    let bytes = read_sidecar_bytes(path)
        .map_err(|e| {
            log::warn!(
                "screen staging: cannot read sidecar {}: {e}",
                path.display()
            )
        })
        .ok()?;
    match serde_json::from_slice(&bytes) {
        Ok(sidecar) => Some(sidecar),
        Err(e) => without_unreadable_blocks(&bytes, path).or_else(|| {
            log::warn!("screen staging: malformed sidecar {}: {e}", path.display());
            None
        }),
    }
}

/// The per-field defensive read, for the nested blocks (review fix round 1
/// for `webcam`; Task 53 for `stems`): a malformed or future-shaped block
/// must cost the capture that block, never the whole capture — a strict read
/// made the recording vanish from the staged list and refuse to open. Each
/// unreadable block degrades to its empty value and its raw JSON moves into
/// `extra`, which writes it back under the same key, so the next rewrite (a
/// pin, a timeline save) does not erase what a newer build wrote. `None`
/// when the sidecar is unreadable for any other reason.
fn without_unreadable_blocks(bytes: &[u8], path: &Path) -> Option<StagedSidecar> {
    let mut value: serde_json::Value = serde_json::from_slice(bytes).ok()?;
    let object = value.as_object_mut()?;
    let mut parked = Vec::new();
    for key in ["webcam", "stems"] {
        let reads = match (key, object.get(key)) {
            (_, None) => true,
            ("webcam", Some(raw)) => serde_json::from_value::<WebcamSidecar>(raw.clone()).is_ok(),
            (_, Some(raw)) => crate::staging_stems::stems_block_reads(raw),
        };
        if !reads {
            parked.extend(object.remove(key).map(|raw| (key, raw)));
        }
    }
    if parked.is_empty() {
        return None; // the blocks were fine; something else is malformed
    }
    let mut sidecar: StagedSidecar = serde_json::from_value(value).ok()?;
    for (key, raw) in parked {
        log::warn!(
            "screen staging: {} has a {key} block this build cannot read; it is ignored",
            path.display()
        );
        sidecar.extra.insert(key.to_string(), raw);
    }
    Some(sidecar)
}

#[cfg(test)]
#[path = "staging_webcam_tests.rs"]
mod webcam_tests;

#[cfg(test)]
#[path = "staging_read_tests.rs"]
mod read_tests;

#[cfg(test)]
#[path = "staging_tests.rs"]
mod tests;

