//! Media probing and import planning — pure functions over file names,
//! sizes and bytes, never a real ffprobe/decoder call (that stays in the
//! shell, which is not `core`-testable — `AGENTS.md` "What compiles
//! where"). Three separate concerns, each with its own "never trust the
//! obvious thing" rule:
//!
//! - `classify_extension` narrows by NAME only — a hint the caller uses to
//!   decide which prober to run, never a guarantee the bytes match.
//! - `sniff_image` narrows by BYTES — a file's real magic header and its
//!   own declared dimensions, independent of whatever extension it wore.
//! - `import_plan` narrows by SIZE — the one rule this pure planner
//!   enforces before Task 25 ever touches a byte of the file.
//!
//! `ProbeFacts`/`asset_from_probe` are the seam back to the project graph:
//! whatever probed the file (ffprobe in the shell, or `sniff_image` here)
//! hands its answer to `asset_from_probe`, which is the only place that
//! knows how a probe result becomes an `Asset` (F22).

use super::{Asset, AssetKind, Map, MediaType, Num};

/// What kind of media a file's NAME suggests it might be —
/// `classify_extension`'s answer. A hint only: Task 25 still probes the
/// bytes (ffprobe for video/audio, `sniff_image` for images) before
/// trusting it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportKind {
    Video,
    Audio,
    Image,
}

/// Classify `name` by its LAST extension, case-insensitively. A double
/// extension is classified by its real (last) one: `"clip.mp4.exe"` is an
/// `.exe`, not an `.mp4` — a smuggled executable must not read as video
/// just because an earlier segment of the name looks legitimate. `None`
/// means "not on the allowlist"; the caller maps that to
/// `EditorErrorCode::UnsupportedMedia`, never a best-effort guess.
pub fn classify_extension(name: &str) -> Option<ImportKind> {
    let (_, ext) = name.rsplit_once('.')?;
    let ext = ext.to_ascii_lowercase();
    let ext = ext.as_str();
    if VIDEO_EXTENSIONS.contains(&ext) {
        Some(ImportKind::Video)
    } else if AUDIO_EXTENSIONS.contains(&ext) {
        Some(ImportKind::Audio)
    } else if IMAGE_EXTENSIONS.contains(&ext) {
        Some(ImportKind::Image)
    } else {
        None
    }
}

/// The allowlist `classify_extension` reads, one slice per kind, lowercase
/// and without the dot. Public so the import dialog's filter (Task 25)
/// offers EXACTLY what classification accepts — a second hand-typed list in
/// the shell is how the two would drift apart.
pub const VIDEO_EXTENSIONS: &[&str] = &["mp4", "m4v", "mov", "webm", "mkv"];
pub const AUDIO_EXTENSIONS: &[&str] = &["mp3", "wav", "m4a", "aac", "ogg", "opus", "flac"];
pub const IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "webp"];

/// Every importable extension, in video → audio → image order.
pub fn import_extensions() -> Vec<&'static str> {
    [VIDEO_EXTENSIONS, AUDIO_EXTENSIONS, IMAGE_EXTENSIONS].concat()
}

/// The maximum width or height `sniff_image` accepts. A header that
/// declares a larger canvas is refused outright here rather than handed
/// to a later stage (the canvas builder, the encoder) that would choke on
/// it with a far less legible error.
pub const MAX_IMAGE_DIMENSION: u32 = 16384;

/// One still-image format `sniff_image` recognises, named from the
/// file's own magic bytes — never from `classify_extension`'s guess.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageFormat {
    Png,
    Jpeg,
    WebP,
}

const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

/// Read width/height from a PNG's mandatory IHDR chunk — the first chunk
/// after the 8-byte signature. Requires the WHOLE declared chunk (length
/// field, `"IHDR"` type, its 13 bytes of data, and its CRC) to actually be
/// present on disk before trusting the two fields inside it, so a file
/// truncated partway through its own header — even after the width/height
/// bytes it happens to have already written — reads as `None` rather than
/// a plausible-looking guess (`truncated_images_are_rejected_not_panicking`).
/// Every slice is `.get()`-guarded: a short or garbled prefix returns
/// `None`, never a panic. The mutation check this reader exists to catch:
/// swap which four bytes feed `width` vs `height` and the asymmetric
/// 640x360 fixture in `png_dimensions_are_read_from_ihdr` goes red.
fn sniff_png(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.get(0..8)? != PNG_SIGNATURE {
        return None;
    }
    let chunk_len = u32::from_be_bytes(bytes.get(8..12)?.try_into().ok()?);
    if chunk_len != 13 || bytes.get(12..16)? != b"IHDR" {
        return None;
    }
    // IHDR's data is a fixed 13 bytes (width, height, bit depth, colour
    // type, compression, filter, interlace) plus a trailing 4-byte CRC.
    bytes.get(0..8 + 4 + 4 + chunk_len as usize + 4)?;
    let width = u32::from_be_bytes(bytes.get(16..20)?.try_into().ok()?);
    let height = u32::from_be_bytes(bytes.get(20..24)?.try_into().ok()?);
    Some((width, height))
}

/// The most of a file `sniff_image` will ever read looking for a JPEG SOF
/// marker — a real encoder writes it in the first few hundred bytes, and
/// an unbounded scan of a large or adversarial file would be an unbounded
/// loop over untrusted bytes.
const JPEG_SCAN_CAP: usize = 256 * 1024;

/// Walk a JPEG's segment headers — never its entropy-coded scan data —
/// for the first SOF0/SOF1/SOF2 marker (baseline / extended sequential /
/// progressive DCT), reading its height-then-width fields (JPEG's own
/// byte order, the opposite of PNG's). Every segment's declared length is
/// what advances the scan, so a marker with no payload (RST0-7, the
/// standalone `0x01`) is skipped by exactly one byte and everything else
/// is skipped by its own declared span — `pos` strictly increases every
/// iteration, so a malformed file terminates via a bounds failure
/// (`None`) rather than looping. Bounded to `JPEG_SCAN_CAP`; every read is
/// `.get()`-guarded.
fn sniff_jpeg(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.get(0..2)? != [0xFF, 0xD8] {
        return None;
    }
    let limit = bytes.len().min(JPEG_SCAN_CAP);
    let mut pos = 2usize;
    while pos < limit {
        if *bytes.get(pos)? != 0xFF {
            return None;
        }
        pos += 1;
        // Fill bytes (0xFF padding between markers, legal in arbitrary
        // runs per Annex B.1.1.5). Bounded by `limit` like the outer
        // loop: a run that never ends before the cap must fail the scan
        // there rather than walking past it — an adversarial or corrupt
        // file could otherwise carry the scan past `JPEG_SCAN_CAP` inside
        // this one inner loop, defeating the bound entirely.
        while pos < limit && bytes.get(pos) == Some(&0xFF) {
            pos += 1;
        }
        if pos >= limit {
            return None;
        }
        let marker = *bytes.get(pos)?;
        pos += 1;
        // Markers with no length/payload at all.
        if marker == 0x01 || (0xD0..=0xD7).contains(&marker) {
            continue;
        }
        if marker == 0xD9 {
            // EOI: reached the end of the image with no SOF marker seen.
            return None;
        }
        let seg_len = u16::from_be_bytes(bytes.get(pos..pos + 2)?.try_into().ok()?) as usize;
        if seg_len < 2 {
            return None;
        }
        if (0xC0..=0xC2).contains(&marker) {
            let payload = bytes.get(pos + 2..pos + seg_len)?;
            // SOF payload: precision(1), height(2 BE), width(2 BE), ...
            let height = u16::from_be_bytes(payload.get(1..3)?.try_into().ok()?) as u32;
            let width = u16::from_be_bytes(payload.get(3..5)?.try_into().ok()?) as u32;
            return Some((width, height));
        }
        pos += seg_len;
    }
    None
}

fn le24(b0: u8, b1: u8, b2: u8) -> u32 {
    u32::from(b0) | (u32::from(b1) << 8) | (u32::from(b2) << 16)
}

/// Read dimensions from a WebP's RIFF container — `VP8X` (the extended
/// header, canvas size stored minus one, 24-bit little-endian), `VP8L`
/// (lossless, a packed 32-bit little-endian field) or `VP8 ` (lossy, note
/// the trailing space in the fourCC — the VP8 bitstream's own 14-bit
/// fields after its 3-byte sync code). Every offset is `.get()`-guarded.
fn sniff_webp(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.get(0..4)? != b"RIFF" || bytes.get(8..12)? != b"WEBP" {
        return None;
    }
    let chunk_id = bytes.get(12..16)?;
    // RIFF chunk payload starts after the 4-byte fourCC + 4-byte size.
    let payload = bytes.get(20..)?;
    match chunk_id {
        b"VP8X" => {
            let w = le24(*payload.get(4)?, *payload.get(5)?, *payload.get(6)?) + 1;
            let h = le24(*payload.get(7)?, *payload.get(8)?, *payload.get(9)?) + 1;
            Some((w, h))
        }
        b"VP8L" => {
            if *payload.first()? != 0x2F {
                return None;
            }
            let bits = u32::from(*payload.get(1)?)
                | (u32::from(*payload.get(2)?) << 8)
                | (u32::from(*payload.get(3)?) << 16)
                | (u32::from(*payload.get(4)?) << 24);
            let width = (bits & 0x3FFF) + 1;
            let height = ((bits >> 14) & 0x3FFF) + 1;
            Some((width, height))
        }
        b"VP8 " => {
            if payload.get(3..6)? != [0x9d, 0x01, 0x2a] {
                return None;
            }
            let w = u16::from_le_bytes(payload.get(6..8)?.try_into().ok()?) & 0x3FFF;
            let h = u16::from_le_bytes(payload.get(8..10)?.try_into().ok()?) & 0x3FFF;
            Some((u32::from(w), u32::from(h)))
        }
        _ => None,
    }
}

fn dims_in_bounds(width: u32, height: u32) -> bool {
    width > 0 && height > 0 && width <= MAX_IMAGE_DIMENSION && height <= MAX_IMAGE_DIMENSION
}

/// Sniff `bytes` for a PNG/JPEG/WebP signature and read its real pixel
/// dimensions from the format's own header — never from a file name
/// (`classify_extension` only narrows by name; this is the "never trust
/// names" half for images, mirroring what real ffprobe output does for
/// video/audio in Task 25). Returns `None` for an unrecognised, truncated
/// or oversized (> `MAX_IMAGE_DIMENSION`) image; never panics.
pub fn sniff_image(bytes: &[u8]) -> Option<(ImageFormat, u32, u32)> {
    if let Some((w, h)) = sniff_png(bytes) {
        return dims_in_bounds(w, h).then_some((ImageFormat::Png, w, h));
    }
    if let Some((w, h)) = sniff_jpeg(bytes) {
        return dims_in_bounds(w, h).then_some((ImageFormat::Jpeg, w, h));
    }
    if let Some((w, h)) = sniff_webp(bytes) {
        return dims_in_bounds(w, h).then_some((ImageFormat::WebP, w, h));
    }
    None
}

/// The per-file size cap `import_plan` enforces — 4 GiB.
pub const MAX_IMPORT_FILE_BYTES: u64 = 4 * 1024 * 1024 * 1024;

/// One file cleared to import: its name, its size and the kind the
/// caller resolved it to (via `classify_extension`/`sniff_image` or real
/// probing — this planner trusts whatever it is handed).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedImport {
    pub display_name: String,
    pub size: u64,
    pub kind: ImportKind,
}

/// Plan an import of `files`, one outcome per file, in the same order —
/// NEVER all-or-nothing: a single oversized file in a batch drop must not
/// cost every other file's plan (`import_plan_reports_per_file`). The
/// only rule this pure planner enforces is the size cap; `kind` is
/// trusted from the caller.
pub fn import_plan(files: &[(&str, u64, ImportKind)]) -> Vec<Result<PlannedImport, String>> {
    files
        .iter()
        .map(|(name, size, kind)| {
            if *size > MAX_IMPORT_FILE_BYTES {
                Err(format!(
                    "\"{name}\" is {size} bytes, over the {MAX_IMPORT_FILE_BYTES}-byte import limit"
                ))
            } else {
                Ok(PlannedImport {
                    display_name: (*name).to_string(),
                    size: *size,
                    kind: *kind,
                })
            }
        })
        .collect()
}

/// What a real probe learned about a source file — a plain core-owned
/// type (F22): core cannot name the shell's `ffmpeg.rs::SourceFacts`,
/// which is `pub(crate)` to the shell crate and carries a shell-specific
/// field (`audio_rate`) this function has no use for. The shell converts
/// its own `SourceFacts` into one of these at the single call site that
/// needs an `Asset` (Task 25).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProbeFacts {
    pub duration_ms: u64,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub has_video: bool,
    pub has_audio: bool,
}

/// A still image's assigned length on the timeline. An image has no real
/// clock to probe, so this fixed default stands in — well inside
/// `limits::MAX_DURATION_MS`, the same "still image" convention the
/// reference format documents.
const IMAGE_DEFAULT_DURATION_MS: u64 = 5_000;

/// Build the `Asset` a successful import registers. `kind` decides the
/// shape: an `Image` becomes `AssetKind::Video` with `media_type: image`
/// set (the reference format has no separate image asset kind — a still
/// is a video-kind asset backed by a picture rather than a frame clock,
/// per `model.rs`'s `MediaType` doc) and its dimensions come from
/// `image_dims` — the `sniff_image` result — NEVER from `facts`, because
/// an image is never handed to ffprobe. `Video`/`Audio` take their
/// duration and dimensions straight from `facts` and ignore
/// `image_dims`.
pub fn asset_from_probe(
    asset_id: String,
    display_name: String,
    kind: ImportKind,
    facts: ProbeFacts,
    image_dims: Option<(u32, u32)>,
) -> Asset {
    let is_image = matches!(kind, ImportKind::Image);
    let asset_kind = match kind {
        ImportKind::Video | ImportKind::Image => AssetKind::Video,
        ImportKind::Audio => AssetKind::Audio,
    };
    let (width, height, duration_ms) = if is_image {
        let (w, h) = image_dims.unzip();
        (
            w.map(Num::from),
            h.map(Num::from),
            IMAGE_DEFAULT_DURATION_MS,
        )
    } else {
        (
            facts.width.map(Num::from),
            facts.height.map(Num::from),
            facts.duration_ms,
        )
    };
    Asset {
        id: asset_id,
        kind: asset_kind,
        name: display_name,
        duration_ms,
        width,
        height,
        size: None,
        builtin: None,
        media_type: is_image.then_some(MediaType::Image),
        linked_asset: None,
        original_name: None,
        extra: Map::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Generalized so the chunk-type/chunk-length rejection tests can build
    // a deliberately WRONG header without duplicating the byte layout.
    fn png_fixture_custom(
        chunk_len: u32,
        chunk_type: &[u8; 4],
        width: u32,
        height: u32,
    ) -> Vec<u8> {
        let mut bytes = PNG_SIGNATURE.to_vec();
        bytes.extend_from_slice(&chunk_len.to_be_bytes());
        bytes.extend_from_slice(chunk_type);
        bytes.extend_from_slice(&width.to_be_bytes());
        bytes.extend_from_slice(&height.to_be_bytes());
        bytes.extend_from_slice(&[8, 6, 0, 0, 0]); // depth/colour/compr/filter/interlace
        bytes.extend_from_slice(&[0, 0, 0, 0]); // CRC (never validated)
        bytes
    }

    fn png_fixture_dims(width: u32, height: u32) -> Vec<u8> {
        png_fixture_custom(13, b"IHDR", width, height)
    }

    fn png_fixture() -> Vec<u8> {
        png_fixture_dims(640, 360)
    }

    // JPEG: SOI, then an SOF0 segment with one component. Height is
    // encoded before width in the bitstream itself, so an asymmetric
    // fixture (800x480) catches a reader that reads them in PNG's order.
    fn jpeg_fixture() -> Vec<u8> {
        vec![
            0xFF, 0xD8, // SOI
            0xFF, 0xC0, // SOF0
            0x00, 0x0B, // segment length = 11
            0x08, // precision
            0x01, 0xE0, // height = 480
            0x03, 0x20, // width = 800
            0x01, // 1 component
            0x01, 0x22, 0x00, // component id/sampling/qtable
        ]
    }

    // SOI followed by a 0xFF fill run that runs PAST `JPEG_SCAN_CAP` before
    // the first real marker byte, then a valid SOF0 segment. Finding this
    // SOF would mean the fill-byte skip walked past the documented 256 KiB
    // bound — the exact defect this fixture regression-tests.
    fn jpeg_fill_run_past_cap_fixture() -> Vec<u8> {
        let mut bytes = vec![0xFF, 0xD8];
        bytes.extend(std::iter::repeat_n(0xFFu8, JPEG_SCAN_CAP + 1000));
        bytes.extend_from_slice(&[
            0xC0, // SOF0 marker byte (not 0xFF, ends the fill run)
            0x00, 0x0B, // segment length = 11
            0x08, // precision
            0x01, 0xE0, // height = 480
            0x03, 0x20, // width = 800
            0x01, // 1 component
            0x01, 0x22, 0x00, // component id/sampling/qtable
        ]);
        bytes
    }

    fn webp_vp8x_fixture(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = b"RIFF".to_vec();
        bytes.extend_from_slice(&0u32.to_le_bytes()); // RIFF size, unchecked
        bytes.extend_from_slice(b"WEBP");
        bytes.extend_from_slice(b"VP8X");
        bytes.extend_from_slice(&10u32.to_le_bytes()); // extended-header chunk size
        bytes.push(0x00); // flags
        bytes.extend_from_slice(&[0, 0, 0]); // reserved
        bytes.extend_from_slice(&(width - 1).to_le_bytes()[0..3]);
        bytes.extend_from_slice(&(height - 1).to_le_bytes()[0..3]);
        bytes
    }

    // VP8L: a 1-byte signature (0x2F) then a packed 32-bit little-endian
    // field: 14 bits width-minus-1, 14 bits height-minus-1, alpha bit,
    // 3-bit version — none of which this reader validates but the width
    // field.
    fn webp_vp8l_fixture(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = b"RIFF".to_vec();
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(b"WEBP");
        bytes.extend_from_slice(b"VP8L");
        bytes.extend_from_slice(&5u32.to_le_bytes()); // 1 sig byte + 4 packed bytes
        bytes.push(0x2F); // VP8L signature
        let bits: u32 = ((width - 1) & 0x3FFF) | (((height - 1) & 0x3FFF) << 14);
        bytes.extend_from_slice(&bits.to_le_bytes());
        bytes
    }

    // VP8 (lossy — note the trailing space in the fourCC): a 3-byte frame
    // tag (unchecked by this reader), the 3-byte sync code, then
    // little-endian width/height each in the low 14 bits of a u16.
    fn webp_vp8_lossy_fixture(width: u16, height: u16) -> Vec<u8> {
        let mut bytes = b"RIFF".to_vec();
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(b"WEBP");
        bytes.extend_from_slice(b"VP8 ");
        bytes.extend_from_slice(&10u32.to_le_bytes()); // 3 tag + 3 sync + 2 w + 2 h
        bytes.extend_from_slice(&[0x00, 0x00, 0x00]); // frame tag, unchecked
        bytes.extend_from_slice(&[0x9d, 0x01, 0x2a]); // sync code
        bytes.extend_from_slice(&width.to_le_bytes());
        bytes.extend_from_slice(&height.to_le_bytes());
        bytes
    }

    #[test]
    fn classify_extension_is_case_insensitive_and_allowlisted() {
        assert_eq!(classify_extension("clip.MP4"), Some(ImportKind::Video));
        assert_eq!(classify_extension("clip.mp4"), Some(ImportKind::Video));
        assert_eq!(classify_extension("clip.mov"), Some(ImportKind::Video));
        assert_eq!(classify_extension("voice.mp3"), Some(ImportKind::Audio));
        assert_eq!(classify_extension("voice.FLAC"), Some(ImportKind::Audio));
        assert_eq!(classify_extension("still.png"), Some(ImportKind::Image));
        assert_eq!(classify_extension("still.JPEG"), Some(ImportKind::Image));
        assert_eq!(classify_extension("installer.exe"), None);
        // A double extension is classified by its real (last) one, never
        // an earlier-looking segment of the name.
        assert_eq!(classify_extension("clip.mp4.exe"), None);
        assert_eq!(classify_extension("no-extension-at-all"), None);
    }

    // The import dialog's filter is `import_extensions()`: every entry must
    // classify (a filter offering a type classification then refuses is a
    // dead end), and the list must hold all sixteen, lowercase, dotless.
    #[test]
    fn import_extensions_are_exactly_what_classification_accepts() {
        let all = import_extensions();
        assert_eq!(all.len(), 16);
        for ext in &all {
            assert_eq!(*ext, ext.to_ascii_lowercase());
            assert!(!ext.contains('.'), "{ext}");
            assert!(classify_extension(&format!("x.{ext}")).is_some(), "{ext}");
        }
        assert_eq!(classify_extension("x.webp"), Some(ImportKind::Image));
        assert_eq!(classify_extension("x.opus"), Some(ImportKind::Audio));
        assert_eq!(classify_extension("x.mkv"), Some(ImportKind::Video));
    }

    #[test]
    fn png_dimensions_are_read_from_ihdr() {
        let bytes = png_fixture();
        assert_eq!(bytes.len(), 33);
        assert_eq!(sniff_image(&bytes), Some((ImageFormat::Png, 640, 360)));
    }

    #[test]
    fn jpeg_sof_scan_finds_dimensions() {
        assert_eq!(
            sniff_image(&jpeg_fixture()),
            Some((ImageFormat::Jpeg, 800, 480))
        );
    }

    #[test]
    fn webp_vp8x_dimensions() {
        assert_eq!(
            sniff_image(&webp_vp8x_fixture(500, 300)),
            Some((ImageFormat::WebP, 500, 300))
        );
    }

    #[test]
    fn webp_vp8l_dimensions() {
        assert_eq!(
            sniff_image(&webp_vp8l_fixture(300, 150)),
            Some((ImageFormat::WebP, 300, 150))
        );
    }

    #[test]
    fn webp_vp8_lossy_dimensions() {
        assert_eq!(
            sniff_image(&webp_vp8_lossy_fixture(720, 480)),
            Some((ImageFormat::WebP, 720, 480))
        );
    }

    // The fill-byte run must not walk the scan past `JPEG_SCAN_CAP` — a
    // valid SOF sitting beyond the cap must never be found. Regression
    // test for a bug where only the OUTER loop was bounded: the inner
    // "skip 0xFF padding" loop had no limit of its own, so a long enough
    // fill run carried `pos` straight through the cap in one pass.
    #[test]
    fn a_jpeg_fill_byte_run_past_the_scan_cap_does_not_defeat_it() {
        assert_eq!(sniff_image(&jpeg_fill_run_past_cap_fixture()), None);
    }

    // RST0 (a zero-length marker, skipped by exactly one byte) followed by
    // EOI with no SOF ever seen — covers the "no length/payload" skip AND
    // the EOI early-return in one fixture.
    #[test]
    fn jpeg_scan_skips_rst_markers_and_stops_cleanly_at_eoi() {
        let no_sof = [0xFF, 0xD8, 0xFF, 0xD0, 0xFF, 0xD9];
        assert_eq!(sniff_image(&no_sof), None);
    }

    // A byte that isn't 0xFF where the scan expects a marker introducer is
    // a corrupt or foreign file — must return None, never panic.
    #[test]
    fn jpeg_scan_rejects_a_non_marker_byte_where_a_marker_is_expected() {
        let misaligned = [0xFF, 0xD8, 0x00, 0x00];
        assert_eq!(sniff_image(&misaligned), None);
    }

    // Every prefix of the PNG fixture short of its full, declared 33
    // bytes must be REJECTED, never panic, and never decode to a
    // plausible-but-wrong answer — only the complete header (through its
    // own CRC) is trusted.
    #[test]
    fn truncated_images_are_rejected_not_panicking() {
        let full = png_fixture();
        assert_eq!(full.len(), 33);
        for n in 0..full.len() {
            assert_eq!(
                sniff_image(&full[..n]),
                None,
                "prefix len {n} must not decode"
            );
        }
        assert_eq!(sniff_image(&full), Some((ImageFormat::Png, 640, 360)));
    }

    #[test]
    fn oversized_dimensions_are_rejected() {
        let bytes = png_fixture_dims(MAX_IMAGE_DIMENSION + 1, 100);
        assert_eq!(sniff_image(&bytes), None);
    }

    // The width case above only exercises the first half of `dims_in_bounds`;
    // an over-TALL image must be caught too, not just an over-wide one.
    #[test]
    fn oversized_height_is_also_rejected() {
        let bytes = png_fixture_dims(100, MAX_IMAGE_DIMENSION + 1);
        assert_eq!(sniff_image(&bytes), None);
    }

    // The brief's bound is "≤ 16384": the exact maximum must be ACCEPTED,
    // and one past it REJECTED — an off-by-one in `dims_in_bounds` would
    // pass every other test here.
    #[test]
    fn the_maximum_dimension_is_accepted_and_one_past_it_is_not() {
        let at_max = png_fixture_dims(MAX_IMAGE_DIMENSION, MAX_IMAGE_DIMENSION);
        assert_eq!(
            sniff_image(&at_max),
            Some((ImageFormat::Png, MAX_IMAGE_DIMENSION, MAX_IMAGE_DIMENSION))
        );
        let over_max = png_fixture_dims(MAX_IMAGE_DIMENSION + 1, MAX_IMAGE_DIMENSION);
        assert_eq!(sniff_image(&over_max), None);
    }

    #[test]
    fn png_with_the_wrong_chunk_type_is_rejected() {
        // A well-formed length but a chunk that isn't IHDR at all — a PNG's
        // first chunk is ALWAYS IHDR, so this is a foreign or corrupt file.
        let bytes = png_fixture_custom(13, b"IDAT", 640, 360);
        assert_eq!(sniff_image(&bytes), None);
    }

    #[test]
    fn png_with_a_mismatched_ihdr_chunk_length_is_rejected() {
        // IHDR's data is always exactly 13 bytes; a header that claims
        // otherwise is not trustworthy even though the type field is right.
        let bytes = png_fixture_custom(12, b"IHDR", 640, 360);
        assert_eq!(sniff_image(&bytes), None);
    }

    #[test]
    fn import_plan_reports_per_file() {
        let files = [
            ("clip.mp4", 100u64, ImportKind::Video),
            ("huge.mov", MAX_IMPORT_FILE_BYTES + 1, ImportKind::Video),
            ("voice.mp3", 2_000u64, ImportKind::Audio),
        ];
        let plan = import_plan(&files);
        assert_eq!(plan.len(), 3);
        // Never all-or-nothing: the oversized middle file's rejection
        // must not swallow the other two outcomes.
        let first = plan[0].as_ref().expect("small file should plan fine");
        assert_eq!(first.display_name, "clip.mp4");
        assert_eq!(first.size, 100);
        assert_eq!(first.kind, ImportKind::Video);
        assert!(plan[1].is_err());
        let third = plan[2]
            .as_ref()
            .expect("second small file should plan fine");
        assert_eq!(third.kind, ImportKind::Audio);
    }

    #[test]
    fn asset_from_probe_builds_video_and_audio_assets_from_facts() {
        let video_facts = ProbeFacts {
            duration_ms: 12_345,
            width: Some(1920),
            height: Some(1080),
            has_video: true,
            has_audio: true,
        };
        let video = asset_from_probe(
            "asset-1".into(),
            "clip.mp4".into(),
            ImportKind::Video,
            video_facts,
            None,
        );
        assert_eq!(video.kind, AssetKind::Video);
        assert_eq!(video.media_type, None);
        assert_eq!(video.duration_ms, 12_345);
        assert_eq!(video.width, Some(Num::from(1920u32)));
        assert_eq!(video.height, Some(Num::from(1080u32)));

        let audio_facts = ProbeFacts {
            duration_ms: 9_000,
            width: None,
            height: None,
            has_video: false,
            has_audio: true,
        };
        let audio = asset_from_probe(
            "asset-2".into(),
            "voice.mp3".into(),
            ImportKind::Audio,
            audio_facts,
            None,
        );
        assert_eq!(audio.kind, AssetKind::Audio);
        assert_eq!(audio.media_type, None);
        assert_eq!(audio.duration_ms, 9_000);
        assert!(audio.width.is_none());
        assert!(audio.height.is_none());
    }

    #[test]
    fn asset_from_probe_gives_a_still_image_the_default_duration_and_its_own_dimensions() {
        // An image never gets an ffprobe pass, so `facts` here is
        // deliberately wrong in every field an image would otherwise take
        // from it — proving the image branch ignores it entirely rather
        // than the two branches happening to agree.
        let facts = ProbeFacts {
            duration_ms: 999_999,
            width: Some(1),
            height: Some(1),
            has_video: false,
            has_audio: false,
        };
        let asset = asset_from_probe(
            "asset-3".into(),
            "still.png".into(),
            ImportKind::Image,
            facts,
            Some((640, 360)),
        );
        assert_eq!(asset.kind, AssetKind::Video);
        assert_eq!(asset.media_type, Some(MediaType::Image));
        assert_eq!(asset.duration_ms, IMAGE_DEFAULT_DURATION_MS);
        assert_eq!(asset.width, Some(Num::from(640u32)));
        assert_eq!(asset.height, Some(Num::from(360u32)));
    }
}
