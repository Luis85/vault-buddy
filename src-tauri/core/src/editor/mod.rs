//! Tutorial editor: the pure interchange model for a multi-track video
//! project (R2 — `docs/superpowers/specs/2026-09-21-tutorial-editor-integration-design.md`).
//!
//! `vault_buddy_core::editor` owns the project graph and, in later tasks,
//! its semantic validation, time mapping, commands, history, migrations,
//! checks, caption parsing, the render plan and the companion note. It has
//! no Tauri types and is tested on Linux. `core::timeline` and
//! `screen::select` are unrelated and stay as they are (R2) — this is a
//! new aggregate, not an extension of either.
//!
//! Kept as a thin module root: later tasks add sibling modules under
//! `editor/` (validation, time mapping, migration, …) rather than growing
//! this file.

pub mod captions_io;
pub mod commands;
pub mod error;
pub mod fingerprint;
pub mod history;
pub mod ids;
pub mod import_io;
pub mod migrate;
pub mod model;
pub mod model_cues;
pub mod package;
mod package_archive;
pub mod package_extract;
pub mod package_plan;
pub mod peaks;
pub mod probe;
pub mod projection;
pub mod relink;
pub mod render_plan;
mod render_plan_audio;
pub mod session;
#[cfg(test)]
pub(crate) mod test_support;
pub mod time;
pub mod validate;
mod validate_media;
pub mod workspace;

pub use commands::{EditorCommand, InternalCommand};
pub use error::{EditorError, EditorErrorCode};
pub use fingerprint::{assets_referenced, canonical_json, edit_fingerprint, new_product};
pub use history::History;
pub use ids::{is_valid_id, new_entity_id, new_project_id};
pub use model::*;
pub use model_cues::*;
pub use projection::{CaptionImportResult, EditorOpenResult, EditorProjection, MissingMedia};
pub use session::{EditorSession, EditorSnapshot, ExecuteRequest};
pub use validate::{validate_envelope, validate_project};
pub use workspace::{sanitize, DeleteMode, Selected, Theme, Workspace};

/// A raw JSON object of fields this crate does not model, preserved
/// verbatim on every entity via `#[serde(flatten)]` so an interchange
/// document round-trips byte-for-byte even when it carries fields a newer
/// or older schema revision added. `DATA-MODEL.md` § Validation order:
/// "Unknown fields must follow a documented round-trip or reject policy" —
/// this crate's policy is round-trip, never drop (R3).
pub type Map = serde_json::Map<String, serde_json::Value>;

/// A raw JSON number, used for every "number"-typed (float-capable) field
/// in the interchange schema. The reference document mixes integer and
/// fractional literals for the *same* field across instances — e.g. a
/// track's `volume` is `1` on one track and `0.7` on another in
/// `tests/fixtures/editor/reference-workspace.example.json` — so a plain
/// `f64` field loses which one it was on deserialize: re-serializing turns
/// an integer literal into a float one (`serde_json::Number`'s `PartialEq`
/// treats `PosInt(1)` and `Float(1.0)` as unequal), which breaks the
/// byte-for-byte round trip R3 requires. `serde_json::Number` preserves
/// the original representation instead. Semantic range/finiteness
/// validation is a later task's job, not this interchange layer's.
pub type Num = serde_json::Number;

/// `vault-buddy-video-project/3` — the project document's schema id.
pub const PROJECT_SCHEMA: &str = "vault-buddy-video-project/3";
/// `vault-buddy-workspace/1` — the saved workspace envelope's schema id.
pub const WORKSPACE_SCHEMA: &str = "vault-buddy-workspace/1";
/// `vault-buddy-project-package/1` — the portable package's schema id.
pub const PACKAGE_SCHEMA: &str = "vault-buddy-project-package/1";

/// Contract reference safety limits (`global-constraints.md` § Contract
/// reference). Not enforced here — this module is the interchange layer;
/// a later validation task reads these to reject an over-sized document.
pub mod limits {
    pub const MAX_ASSETS: usize = 200;
    pub const MAX_TRACKS: usize = 32;
    pub const MAX_CLIPS: usize = 600;
    pub const MAX_EFFECTS: usize = 1200;
    pub const MAX_MARKERS: usize = 300;
    pub const MAX_TRANSITIONS: usize = 300;
    pub const MAX_CAPTIONS: usize = 2000;
    pub const MAX_PRODUCTS: usize = 40;
    pub const MAX_DURATION_MS: u64 = 7_200_000;
    pub const MAX_PROJECT_JSON_BYTES: u64 = 8 * 1024 * 1024;
    pub const MAX_PACKAGE_MEDIA_BYTES: u64 = 200 * 1024 * 1024;
    pub const MAX_PACKAGE_BYTES: u64 = 220 * 1024 * 1024;
    pub const MAX_PACKAGE_ENTRIES: usize = 1000;
    pub const MAX_ENTRY_RATIO: u64 = 100;
    pub const MAX_HISTORY: usize = 100;
    pub const MAX_RECENT_COMMANDS: usize = 256;
    pub const MAX_CAPTION_FILE_BYTES: u64 = 2 * 1024 * 1024;
    pub const MAX_TITLE_CHARS: usize = 160;
    pub const MAX_NAME_CHARS: usize = 300;
    pub const MAX_EFFECT_TEXT_CHARS: usize = 1000;
    pub const MAX_CAPTION_TEXT_CHARS: usize = 500;
    /// `workspace.schema.json`'s `captions.font_size` (`minimum 18,
    /// maximum 56`) -- the reference `captions.js` refuses the same range.
    /// Declared here (GAP-179, Task 38) so `validate_project` and the
    /// `setCaptionSettings` command read ONE pair of numbers.
    pub const CAPTION_FONT_SIZE_MIN: f64 = 18.0;
    pub const CAPTION_FONT_SIZE_MAX: f64 = 56.0;

    /// The four supported canvases, `(width, height)`, all at 30 fps.
    pub const CANVASES: [(u32, u32); 4] = [(1280, 720), (720, 1280), (720, 720), (960, 720)];
    pub const CANVAS_FPS: u32 = 30;
    pub const SPEED_MIN: f64 = 0.25;
    pub const SPEED_MAX: f64 = 4.0;

    /// Task 21 (F14): the shortest OUTPUT duration a clip may end up with
    /// after `trimClip`/`splitClip`. Enforced by those two commands
    /// directly (`commands::clips`) -- this module still only DECLARES
    /// limits, the way every other constant above does. A numeric
    /// inspector entry bypasses the timeline's own drag-preview clamp, so
    /// without this the frontend was the only thing standing between a
    /// hand-typed `outMs` and a zero- or negative-length clip.
    pub const MIN_CLIP_MS: u64 = 100;
}
