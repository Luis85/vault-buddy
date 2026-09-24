//! Migrating a staged screen capture into a tutorial project (R2, R6, ADR
//! §4).
//!
//! `core` cannot depend on `vault_buddy_screen` (nor on any Tauri type), so
//! this module takes plain inputs (`StagedInput`) rather than the staged
//! capture's own shell/screen-crate types — a later task's shell command is
//! the one that translates a real staged capture into one of these.

use super::ids;
use super::model::{
    Asset, AssetKind, Builtin, Canvas, Clip, Destination, FadeCurve, Fit, FrameShape, Project,
    Track, TrackKind,
};
use super::{limits, Map, Num};
use crate::timeline::Timeline;

#[path = "migrate_stems.rs"]
mod stems;
pub use stems::stem_asset_id;

/// One per-input audio stem recorded beside a staged capture (Task 53,
/// F-05, F26): input `index` (1-based) named `input`, in the staging file
/// `file` (`<base>.stem-<index>.m4a`) — the shell registers it as a
/// `StagingFile { base, file }` source for `stem_asset_id(index)`.
#[derive(Debug, Clone)]
pub struct StemInput {
    pub index: u32,
    pub input: String,
    pub file: String,
}

/// A staged capture's own SYNCHRONIZED webcam track (F-22, F26): recorded
/// on the capture's shared clock, into the staging file `file`
/// (`<base>.webcam.mp4`), its first frame `offset_ms` into the capture.
#[derive(Debug, Clone)]
pub struct WebcamInput {
    pub duration_ms: u64,
    pub width: u32,
    pub height: u32,
    /// The staging file NAME — the shell registers it as a
    /// `StagingFile { base, file }` source for `WEBCAM_ASSET_ID`.
    pub file: String,
    /// Where the webcam's first frame sits on the capture's clock; negative
    /// when the device delivered before the screen's first frame.
    pub offset_ms: i64,
}

/// The asset id a synchronized webcam track migrates to, and so the key
/// its `sources.json` entry lives under.
pub const WEBCAM_ASSET_ID: &str = "webcam";

/// `asset.extra[CAPTURE_SYNC_KEY] = SHARED_CLOCK` marks the ONE asset kind
/// whose timing is synchronized with the capture by construction (F-22) —
/// never a later webcam take (`take::take_asset`), which is placed by hand.
pub const CAPTURE_SYNC_KEY: &str = "capture_sync";
pub const SHARED_CLOCK: &str = "shared-clock";

/// The presenter placement (ADR §4; F-21; pre-flight F35) — the SAME
/// constant as `src/editor/layoutGeometry.ts`'s `PRESENTER_CORNER`. Both
/// languages are pinned to ONE table,
/// `tests/fixtures/editor-presenter-placement.json`, read by
/// `migrate_webcam_tests.rs` and `tests/editorWebcamDialog.test.ts`.
/// `cornerPreset`'s generic margin is a different place.
pub const PRESENTER_X: f64 = 0.775;
pub const PRESENTER_Y: f64 = 0.06;
pub const PRESENTER_W: f64 = 0.19;
pub const PRESENTER_FRAME_SHAPE: FrameShape = FrameShape::Circle;
pub const PRESENTER_FIT: Fit = Fit::Cover;

/// The presenter's height on a `(width, height)` canvas: a circle, so it is
/// square in PIXELS (`presenterBox`'s `aspectHeight(w, canvas, 1)`),
/// rounded to four places as `roundBox` does — `0.3378` on 1280x720, the
/// ADR's own figure.
pub fn presenter_height(canvas_width: u32, canvas_height: u32) -> f64 {
    if canvas_height == 0 {
        return PRESENTER_W;
    }
    let square = PRESENTER_W * f64::from(canvas_width) / f64::from(canvas_height);
    (square * 10_000.0).round() / 10_000.0
}

/// The plain inputs a staged screen capture supplies for migration. No
/// `screen` or Tauri types — see the module doc.
pub struct StagedInput<'a> {
    pub base: &'a str,
    pub vault_id: &'a str,
    pub source_title: &'a str,
    pub duration_ms: u64,
    pub width: u32,
    pub height: u32,
    pub has_audio: bool,
    pub legacy_timeline: Option<&'a serde_json::Value>,
    pub stems: &'a [StemInput],
    /// How many audio inputs the capture recorded (its sidecar's `inputs`).
    /// Stems replace the embedded mix only when EVERY input has one.
    pub input_count: usize,
    pub webcam: Option<WebcamInput>,
}

/// `from_staged`'s result. Wraps `Project` rather than returning it bare
/// (F6) so a dropped backwards segment is visible to the caller — and to the
/// test — rather than silently vanishing into a shorter timeline.
pub struct MigrationResult {
    pub project: Project,
    pub dropped_segments: u32,
}

/// The nearest of the four supported canvases (`limits::CANVASES`) by
/// ASPECT RATIO, never by raw pixel distance — a capture is never actually
/// the same size as a canvas preset, so this is always choosing among
/// mismatches and aspect ratio is the one that determines how badly a
/// letterbox/pillarbox will show.
///
/// Degenerate input (`height == 0`) cannot produce a meaningful ratio; it
/// falls back to the first canvas in `limits::CANVASES` (`(1280, 720)`)
/// rather than panicking — a corrupt capture must still migrate to
/// *something* reviewable, never crash the migration.
pub fn nearest_canvas(width: u32, height: u32) -> (u32, u32) {
    if height == 0 {
        return limits::CANVASES[0];
    }
    let source_ratio = f64::from(width) / f64::from(height);
    let mut best = limits::CANVASES[0];
    let mut best_diff = f64::INFINITY;
    for &(cw, ch) in limits::CANVASES.iter() {
        let diff = (f64::from(cw) / f64::from(ch) - source_ratio).abs();
        if diff < best_diff {
            best_diff = diff;
            best = (cw, ch);
        }
    }
    best
}

/// True when `(width, height)`'s aspect ratio matches its nearest canvas'
/// aspect ratio EXACTLY (F5) — never the raw dimensions, and never a float
/// comparison of the two ratios (cross-multiplication keeps this an exact
/// integer check). An untouched 1080p capture (`1920x1080`) migrated onto
/// its nearest `1280x720` canvas must still read as exact so Task 41's
/// `is_identity` can remux it at source resolution instead of always
/// re-encoding — comparing raw dimensions instead would make the identity
/// path unreachable for every real capture, since a capture's pixel size
/// essentially never equals a canvas preset's.
pub fn canvas_is_exact(width: u32, height: u32) -> bool {
    let (cw, ch) = nearest_canvas(width, height);
    u64::from(width) * u64::from(ch) == u64::from(height) * u64::from(cw)
}

/// Resolves a reference project's destination vault, recorded on disk as a
/// name or an id, against the vaults actually registered today (R3): an id
/// match wins outright (ids are stable and unambiguous), else a UNIQUE name
/// match; anything else (no match, or more than one vault sharing that
/// name) is `None` rather than a guess.
pub fn map_reference_destination(name_or_id: &str, vaults: &[(String, String)]) -> Option<String> {
    if vaults.iter().any(|(id, _)| id == name_or_id) {
        return Some(name_or_id.to_string());
    }
    let mut by_name = vaults.iter().filter(|(_, name)| name == name_or_id);
    let first = by_name.next()?;
    if by_name.next().is_some() {
        return None;
    }
    Some(first.0.clone())
}

/// Migrates a staged screen capture into a tutorial `Project` (R6, ADR §4).
///
/// One asset (`src`, the capture itself), two tracks (`v1` "Screen" video,
/// `a1` "Audio" audio — `a1` starts empty; it exists for a later detached-
/// audio edit, not because this task puts anything on it). The legacy
/// sidecar `timeline` is read through `Timeline::from_sidecar_value` — the
/// SAME degrade `export_commands::timeline_from_sidecar` uses, since both
/// now call the one reader in `core::timeline` — and each surviving segment
/// becomes a clip `c<n>` on `v1`, laid end to end from output `0`.
///
/// A segment whose migrated `in_ms >= out_ms` (the `saturating_sub`
/// backwards-segment case a hand-edited sidecar can produce) is DROPPED
/// rather than installed as a zero/negative-length clip `validate_project`
/// would then reject outright — `dropped_segments` counts how many.
pub fn from_staged(input: &StagedInput<'_>, project_id: &str) -> MigrationResult {
    let (canvas_width, canvas_height) = nearest_canvas(input.width, input.height);

    let asset = Asset {
        id: "src".to_string(),
        kind: AssetKind::Video,
        name: input.source_title.to_string(),
        duration_ms: input.duration_ms,
        width: Some(Num::from(input.width)),
        height: Some(Num::from(input.height)),
        size: None,
        // GAP-175: this is the one `Builtin` this codebase mints with a real
        // `sources.json` record — keep `src/editor/previewLayers.ts`'s
        // `BUILTIN_HAS_FILE` in sync with any change here.
        builtin: Some(Builtin::Screen),
        media_type: None,
        linked_asset: None,
        original_name: None,
        extra: Map::new(),
    };

    let mut assets = vec![asset];
    let mut tracks = vec![
        track("v1", TrackKind::Video, "Screen"),
        track("a1", TrackKind::Audio, "Audio"),
    ];

    let timeline = match input.legacy_timeline {
        Some(value) => Timeline::from_sidecar_value(value, input.duration_ms),
        None => Timeline::whole(input.duration_ms),
    };

    let mut clips = Vec::new();
    // Every placed segment as `(output start, source start, source end)` —
    // the one mapping the webcam's clips must follow too.
    let mut placed = Vec::new();
    let mut cursor_ms = 0u64;
    let mut dropped_segments = 0u32;
    let mut surviving = 0u32;
    for segment in &timeline.segments {
        if segment.source_start_ms >= segment.source_end_ms {
            dropped_segments += 1;
            continue;
        }
        surviving += 1;
        let duration_ms = segment.source_end_ms - segment.source_start_ms;
        clips.push(plain_clip(
            format!("c{surviving}"),
            "src",
            "v1",
            format!("Clip {surviving}"),
            cursor_ms,
            (segment.source_start_ms, segment.source_end_ms),
        ));
        placed.push((cursor_ms, segment.source_start_ms, segment.source_end_ms));
        cursor_ms += duration_ms;
    }

    // The webcam's track and clips come after; reserve their share.
    let webcam_share = input.webcam.as_ref().map_or((0, 0), |_| (1, placed.len()));
    let parts = stems::stem_parts(
        input.stems,
        input.input_count,
        &placed,
        input.duration_ms,
        (tracks.len() + webcam_share.0, clips.len() + webcam_share.1),
    );
    if let Some((stem_assets, stem_tracks, stem_clips)) = parts {
        // Every input is laid out as a stem: the capture's MIX is muted.
        for clip in &mut clips {
            clip.muted = true;
        }
        assets.extend(stem_assets);
        tracks.extend(stem_tracks);
        clips.extend(stem_clips);
    }

    if let Some(webcam) = &input.webcam {
        let (asset, track, webcam_clips) =
            webcam_parts(webcam, &placed, canvas_width, canvas_height);
        assets.push(asset);
        // Index 0 is the TOP layer: the presenter sits over the screen.
        tracks.insert(0, track);
        clips.extend(webcam_clips);
    }

    let title: String = input
        .source_title
        .chars()
        .take(limits::MAX_TITLE_CHARS)
        .collect();

    let project = Project {
        schema: super::PROJECT_SCHEMA.to_string(),
        id: project_id.to_string(),
        title,
        canvas: Canvas {
            width: canvas_width,
            height: canvas_height,
            fps: limits::CANVAS_FPS,
            extra: Map::new(),
        },
        master_gain: 1.0,
        assets,
        tracks,
        clips,
        effects: Vec::new(),
        markers: Vec::new(),
        transitions: Vec::new(),
        captions: None,
        destination: Destination {
            vault: input.vault_id.to_string(),
            folder: String::new(),
            dated: false,
            extra: Map::new(),
        },
        extra: Map::new(),
    };

    // `_ = ids::is_valid_id;` keeps the import honest without generating a
    // fresh id here: the project id is a caller-supplied argument (the
    // caller draws it via `ids::new_project_id`, typically before it also
    // needs to name the on-disk project directory), not something this
    // function invents.
    debug_assert!(
        ids::is_valid_id(project_id),
        "project_id must be a valid entity id"
    );

    MigrationResult {
        project,
        dropped_segments,
    }
}

/// An ordinary, unhidden, unit-volume track.
fn track(id: &str, kind: TrackKind, name: &str) -> Track {
    Track {
        id: id.to_string(),
        kind,
        name: name.to_string(),
        visible: true,
        locked: false,
        muted: false,
        solo: false,
        volume: Num::from(1),
        extra: Map::new(),
    }
}

/// A full-frame clip playing `source` (`[in, out)`) of `asset_id` from
/// output `start_ms`, every property a migration does not decide at its
/// default.
fn plain_clip(
    id: String,
    asset_id: &str,
    track_id: &str,
    name: String,
    start_ms: u64,
    source: (u64, u64),
) -> Clip {
    Clip {
        id,
        asset_id: asset_id.to_string(),
        track_id: track_id.to_string(),
        name,
        start_ms,
        in_ms: source.0,
        out_ms: source.1,
        fade_in_ms: 0,
        fade_out_ms: 0,
        fade_curve: FadeCurve::Linear,
        opacity: Num::from(1),
        volume: Num::from(1),
        muted: false,
        x: Num::from(0),
        y: Num::from(0),
        w: Num::from(1),
        h: Num::from(1),
        speed: None,
        rotation: None,
        frame_shape: None,
        fit: None,
        mirror: None,
        flip_y: None,
        preserve_pitch: None,
        group_id: None,
        crop_zoom: None,
        crop_x: None,
        crop_y: None,
        adjustments: None,
        card: None,
        extra: Map::new(),
    }
}

/// A finite layout fraction as the document's number. Every input is one of
/// the constants above or a quotient of two canvas sizes, so it is finite.
fn fraction(v: f64) -> Num {
    Num::from_f64(v).unwrap_or_else(|| Num::from(0))
}

/// The synchronized webcam track (F-22, F26): its asset (marked
/// `SHARED_CLOCK`, the only place that claim is ever made), its own track
/// `v2`, and its clips placed as the presenter.
///
/// "Synchronized" has to survive the legacy timeline the screen clips
/// follow (review fix round 1): one webcam clip per placed screen segment,
/// at the SAME output start and mapped through the same source -> output
/// mapping, so a cut or a reorder moves the presenter with its screen. Each
/// is clipped to what the webcam file covers — capture ms `offset_ms ..
/// offset_ms + duration_ms` (pre-flight F35: the plan's real offset where ADR
/// §4's example shows `0`; a NEGATIVE offset is a device that delivered
/// before the screen's first frame, whose early head is simply never
/// placed). A segment the webcam never covered places nothing.
fn webcam_parts(
    webcam: &WebcamInput,
    placed: &[(u64, u64, u64)],
    canvas_width: u32,
    canvas_height: u32,
) -> (Asset, Track, Vec<Clip>) {
    let mut extra = Map::new();
    extra.insert(
        CAPTURE_SYNC_KEY.to_string(),
        serde_json::Value::String(SHARED_CLOCK.to_string()),
    );
    let asset = Asset {
        id: WEBCAM_ASSET_ID.to_string(),
        kind: AssetKind::Video,
        name: "Webcam".to_string(),
        duration_ms: webcam.duration_ms,
        width: Some(Num::from(webcam.width)),
        height: Some(Num::from(webcam.height)),
        size: None,
        builtin: None,
        media_type: None,
        linked_asset: None,
        original_name: Some(webcam.file.clone()),
        extra,
    };
    let offset = i128::from(webcam.offset_ms);
    let covered = (offset, offset + i128::from(webcam.duration_ms));
    let clips = placed
        .iter()
        .filter_map(|&(start, source_start, source_end)| {
            let lo = i128::from(source_start).max(covered.0);
            let hi = i128::from(source_end).min(covered.1);
            // Every value below is non-negative and within u64 whenever the
            // intersection is non-empty: `lo >= source_start >= 0`,
            // `lo >= offset`, `hi <= offset + duration`.
            if lo >= hi {
                return None;
            }
            let as_u64 = |v: i128| u64::try_from(v).ok();
            Some((
                as_u64(i128::from(start) + (lo - i128::from(source_start)))?,
                as_u64(lo - offset)?,
                as_u64(hi - offset)?,
            ))
        })
        .enumerate()
        .map(|(i, (start_ms, in_ms, out_ms))| Clip {
            x: fraction(PRESENTER_X),
            y: fraction(PRESENTER_Y),
            w: fraction(PRESENTER_W),
            h: fraction(presenter_height(canvas_width, canvas_height)),
            frame_shape: Some(PRESENTER_FRAME_SHAPE),
            fit: Some(PRESENTER_FIT),
            ..plain_clip(
                format!("w{}", i + 1),
                WEBCAM_ASSET_ID,
                "v2",
                format!("Webcam {}", i + 1),
                start_ms,
                (in_ms, out_ms),
            )
        })
        .collect();
    (asset, track("v2", TrackKind::Video, "Presenter"), clips)
}

#[cfg(test)]
#[path = "migrate_webcam_tests.rs"]
mod webcam_tests;

#[cfg(test)]
#[path = "migrate_stem_tests.rs"]
mod stem_tests;

#[cfg(test)]
#[path = "migrate_tests.rs"]
mod tests;
