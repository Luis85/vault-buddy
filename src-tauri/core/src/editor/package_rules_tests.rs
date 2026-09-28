//! Task 38 fix round 1: one test per archive rule that the first round left
//! untested (a rule could be deleted with every test still green), plus the
//! rules the round added -- reserved device names, the hand-walked central
//! directory, the per-graph asset exemption and `write_package`'s bounded
//! copy. Nested inside `package_tests.rs` (the `validate_transitions_tests.rs`
//! precedent) so its builders are in scope and that file stays under the
//! 800-line Rust cap.

use std::io::{Read, SeekFrom};

use zip::write::FullFileOptions;

use super::*;

/// Rewrites a little-endian `u16` field of the entry named `name` in both
/// its local header (`local_off`) and its central header (`central_off`).
/// APPNOTE offsets: flags local@6 central@8, method local@8 central@10.
fn patch_u16(bytes: &mut [u8], name: &str, local_off: usize, central_off: usize, value: u16) {
    let name = name.as_bytes();
    let mut patched = 0;
    for at in 0..bytes.len().saturating_sub(46) {
        let (off, n_off) = match &bytes[at..at + 4] {
            [0x50, 0x4b, 0x03, 0x04] => (local_off, 30),
            [0x50, 0x4b, 0x01, 0x02] => (central_off, 46),
            _ => continue,
        };
        if bytes.get(at + n_off..at + n_off + name.len()) == Some(name) {
            bytes[at + off..at + off + 2].copy_from_slice(&value.to_le_bytes());
            patched += 1;
        }
    }
    assert_eq!(patched, 2, "expected one local and one central header");
}

fn refused_name(name: &str, needle: &str) {
    let why = validate_entry_name(name).expect_err(name);
    assert!(
        why.contains(needle),
        "{name:?}: expected {needle:?} in {why:?}"
    );
}

// Regression (review Important 1): `CON`, `NUL`, `COM1` are VALID asset ids,
// so the manifest's own path shape accepts `media/CON.mp4` -- and on Windows
// that path is the console device, not a file.
#[test]
fn reserved_device_names_are_rejected() {
    for bad in [
        "media/CON.mp4",
        "products/COM1.mp4",
        "media/nul.wav",
        "media/Aux.mp4",
        "media/CON .mp4",
        "products/lpt9.mp4",
    ] {
        refused_name(bad, "reserved device");
    }
    let env = envelope_of(project_using(&["CON"]), Vec::new());
    let body = bytes_of(120, 2);
    let manifest = manifest_for(&env, &[("CON", &body)]);
    let bytes = raw_zip(&[
        (MANIFEST_NAME, &json(&manifest), CompressionMethod::Deflated),
        (WORKSPACE_NAME, &json(&env), CompressionMethod::Deflated),
        ("media/CON.mp4", &body, CompressionMethod::Stored),
    ]);
    rejected(&bytes, "reserved device");
}

// Only the colon rule catches these: each is otherwise a well-formed name
// under `media/` or `products/` (an NTFS alternate data stream, or a colon
// that Windows would read as one).
#[test]
fn a_colon_anywhere_is_refused_as_a_stream_separator() {
    for bad in [
        "media/a1.mp4:hidden",
        "media/a:b.mp4",
        "products/p1.mp4:$DATA",
    ] {
        refused_name(bad, "stream");
    }
}

// Only the backslash-anywhere rule catches these (none starts with `\`):
// Windows splits on `\`, so the first two traverse there.
#[test]
fn a_backslash_anywhere_is_refused() {
    for bad in ["media\\..\\..\\x", "media/..\\x", "media/a\\b.mp4"] {
        refused_name(bad, "backslash");
    }
}

#[test]
fn encrypted_entries_are_rejected() {
    let (env, b1, manifest) = base();
    let mut bytes = assembled(&json(&manifest), &env, &b1, &[]);
    patch_u16(&mut bytes, A1, 6, 8, 0x0001);
    rejected(&bytes, "encrypted");
}

#[test]
fn unsupported_compression_is_rejected() {
    let (env, b1, manifest) = base();
    let mut bytes = assembled(&json(&manifest), &env, &b1, &[]);
    patch_u16(&mut bytes, A1, 8, 10, 12); // bzip2
    rejected(&bytes, "unsupported compression");
}

// A Stored entry that DECLARES more than it stores. The manifest lists the
// declared size, so the Stored-size rule is the only one this trips.
#[test]
fn a_stored_entry_with_inconsistent_sizes_is_rejected() {
    let (env, b1, mut manifest) = base();
    manifest.media[0].size = b1.len() as u64 + 10;
    let mut bytes = assembled(&json(&manifest), &env, &b1, &[]);
    patch_sizes(&mut bytes, A1, None, Some(b1.len() as u32 + 10));
    rejected(&bytes, "inconsistent sizes");
}

// A name without the UTF-8 flag is decoded as CP437, so 0x81 reads as `ü`
// -- a name that would pass every later rule it reached, but that is not
// the bytes the archive holds.
#[test]
fn a_name_that_is_not_utf8_is_rejected() {
    let (env, b1, manifest) = base();
    let mut bytes = assembled(&json(&manifest), &env, &b1, &["media/x1.mp4"]);
    let (from, to) = (b"media/x1.mp4", b"media/\x811.mp4");
    let mut replaced = 0;
    for at in 0..bytes.len() - from.len() {
        if &bytes[at..at + from.len()] == from {
            bytes[at..at + to.len()].copy_from_slice(to);
            replaced += 1;
        }
    }
    assert_eq!(replaced, 2);
    rejected(&bytes, "not UTF-8");
}

// Review Minor 1 / the zip-4 re-check: when the final end record's
// directory fails to parse, `ZipArchive::new` walks BACK through every
// earlier end-record candidate (media bytes can hold one, ZIP64 and all).
// Walking the directory ourselves first means the crate is only ever
// handed a directory it will accept.
#[test]
fn a_corrupt_central_record_is_refused_before_the_zip_reader_runs() {
    let (env, b1, manifest) = base();
    let mut bytes = assembled(&json(&manifest), &env, &b1, &[]);
    let central: Vec<usize> = (0..bytes.len() - 4)
        .filter(|&at| bytes[at..at + 4] == [0x50, 0x4b, 0x01, 0x02])
        .collect();
    assert_eq!(central.len(), 3);
    bytes[central[1] + 3] = 0x03;
    rejected(&bytes, "central directory record 2");
}

#[test]
fn a_central_record_with_an_extra_field_is_refused() {
    let (env, b1, manifest) = base();
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    zip.start_file(MANIFEST_NAME, deflated).unwrap();
    std::io::Write::write_all(&mut zip, &json(&manifest)).unwrap();
    zip.start_file(WORKSPACE_NAME, deflated).unwrap();
    std::io::Write::write_all(&mut zip, &json(&env)).unwrap();
    let mut stored = FullFileOptions::default().compression_method(CompressionMethod::Stored);
    stored
        .add_extra_data(0xCAFE, vec![1, 2, 3].into_boxed_slice(), true)
        .unwrap();
    zip.start_file(A1, stored).unwrap();
    std::io::Write::write_all(&mut zip, &b1).unwrap();
    rejected(&zip.finish().unwrap().into_inner(), "extra field");
}

// Review Minor 3: the exemption from `missing` must hold for EVERY
// definition of the asset. Here the live edit reuses id `x` for a card
// while a retained product's snapshot used `x` for the real screen
// recording -- a first-wins lookup exempted the recording.
#[test]
fn an_exemption_must_hold_in_every_graph_that_defines_the_asset() {
    let mut old = project_using(&["x"]);
    old.assets[0].builtin = Some(Builtin::Screen);
    let product = new_product(
        &old,
        2,
        "p1",
        "Take",
        "take.mp4",
        1500,
        None,
        "2026-09-22T00:00:00Z",
    );
    let mut current = project_using(&["x"]);
    current.assets[0].builtin = Some(Builtin::Card);
    let env = envelope_of(current, vec![product]);
    let index = inspect(&written(&env, &manifest_for(&env, &[]), &[])).unwrap();
    assert_eq!(index.missing, vec!["x".to_string()]);
}

/// Counts every byte a reader hands out, so a test can prove how much of
/// its input a writer consumed.
struct Counting {
    remaining: u64,
    read: std::rc::Rc<std::cell::Cell<u64>>,
}

impl Read for Counting {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = (buf.len() as u64).min(self.remaining) as usize;
        buf[..n].fill(7);
        self.remaining -= n as u64;
        self.read.set(self.read.get() + n as u64);
        Ok(n)
    }
}

// Review Minor 4: an oversized input is refused after at most one byte
// past its listed size, never streamed to its end first.
#[test]
fn write_package_stops_reading_an_oversized_file_one_byte_past_its_size() {
    let (env, _, manifest) = base(); // media/a1.mp4 is listed at 300 bytes
    let read = std::rc::Rc::new(std::cell::Cell::new(0));
    let file = Counting {
        remaining: 50 * 1024 * 1024,
        read: read.clone(),
    };
    let err = write_package(
        Cursor::new(Vec::new()),
        &manifest,
        &json(&env),
        [(A1, file)],
    )
    .expect_err("an oversized file is refused");
    assert_eq!(err.code, EditorErrorCode::Internal);
    assert!(err.message.contains("larger than"), "{}", err.message);
    assert_eq!(
        read.get(),
        301,
        "read exactly one byte past the listed size"
    );
}

#[test]
fn write_package_refuses_a_manifest_listing_a_path_twice() {
    let (env, b1, mut manifest) = base();
    manifest.media.push(manifest.media[0].clone());
    let files = [(A1, Cursor::new(b1))];
    let err = write_package(Cursor::new(Vec::new()), &manifest, &json(&env), files)
        .expect_err("a duplicate manifest path is refused");
    assert_eq!(err.code, EditorErrorCode::Internal);
    assert!(err.message.contains("twice"), "{}", err.message);
}

// S-11 (post-merge review): a refusal names an entry by POSITION, never by
// its name -- a name is the package author's text, and a bidi control in it
// (U+202E renders `cod\u{202E}4pm.exe` as `codexe.mp4`) makes a message say
// something else than it holds. Such a name is refused outright too.
#[test]
fn a_bidi_control_in_an_entry_name_is_refused_without_echoing_it() {
    // U+200E/U+200F/U+061C (LRM, RLM, ALM) are the implicit marks: they
    // reorder neutral characters around them just as surely (carried from
    // hardening Task 5).
    for c in [
        '\u{202A}', '\u{202D}', '\u{202E}', '\u{2066}', '\u{2069}', '\u{200E}', '\u{200F}',
        '\u{061C}',
    ] {
        refused_name(&format!("media/a{c}1.mp4"), "bidirectional control");
    }
    let (env, b1, manifest) = base();
    let bad = "media/cod\u{202E}4pm.exe";
    let err = inspect(&assembled(&json(&manifest), &env, &b1, &[bad])).expect_err("refused");
    assert_eq!(err.code, EditorErrorCode::InvalidProject);
    assert_eq!(
        err.message,
        "package entry 4 is not allowed: contains a bidirectional control character"
    );
}

// S-11: every other per-entry refusal names the position too.
#[test]
fn per_entry_refusals_name_the_position_not_the_name() {
    let (env, b1, manifest) = base();
    let err = inspect(&assembled(&json(&manifest), &env, &b1, &["media/zz.mp4"]))
        .expect_err("an unlisted entry");
    assert_eq!(err.message, "package entry 4 is not listed in the manifest");
    let mut m = manifest.clone();
    m.media[0].path = "media/a1.mp4x9abcd".into();
    let err = inspect(&assembled(&json(&m), &env, &b1, &[])).expect_err("a bad path");
    assert_eq!(
        err.message,
        "manifest media file 1: its path is not its expected name"
    );
}

// S-13 (post-merge review): a manifest media entry for an asset that needs
// no file (a `card`, synthesized from the project) was extracted into
// `media\` and then recorded nowhere -- bytes the project can never use.
#[test]
fn media_listed_for_a_card_is_refused() {
    let mut project = project_using(&["a1", "card1"]);
    project.assets[1].builtin = Some(Builtin::Card);
    let env = envelope_of(project, Vec::new());
    let (b1, card) = (bytes_of(300, 1), bytes_of(64, 4));
    let media: [(&str, &[u8]); 2] = [("a1", &b1), ("card1", &card)];
    let err = inspect(&written(&env, &manifest_for(&env, &media), &media))
        .expect_err("a card's media is refused");
    assert_eq!(err.code, EditorErrorCode::InvalidProject);
    assert_eq!(err.message, "The project file lists media no clip can use.");
    let only_a1: [(&str, &[u8]); 1] = [("a1", &b1)];
    inspect(&written(&env, &manifest_for(&env, &only_a1), &only_a1))
        .expect("positive control: without the card's media it is valid");
}

/// A reader over bytes the test can change after it was handed out: every
/// clone reads the same buffer, as every handle onto one file does.
#[derive(Clone)]
struct Shared {
    bytes: std::rc::Rc<std::cell::RefCell<Vec<u8>>>,
    pos: u64,
}

impl Read for Shared {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let bytes = self.bytes.borrow();
        let start = (self.pos as usize).min(bytes.len());
        let n = buf.len().min(bytes.len() - start);
        buf[..n].copy_from_slice(&bytes[start..start + n]);
        self.pos += n as u64;
        Ok(n)
    }
}

impl std::io::Seek for Shared {
    fn seek(&mut self, to: SeekFrom) -> std::io::Result<u64> {
        let len = self.bytes.borrow().len() as i64;
        self.pos = match to {
            SeekFrom::Start(n) => n,
            SeekFrom::End(d) => (len + d) as u64,
            SeekFrom::Current(d) => (self.pos as i64 + d) as u64,
        };
        Ok(self.pos)
    }
}

// S-3 (post-merge review): the extractor used to parse the archive a SECOND
// time, without `package_archive`'s raw pre-checks, and trust whatever
// directory that parse found. Here the directory changes after inspection
// (entry 3 renamed): extraction must still follow the directory that was
// validated, so a re-parse (the old `PackageExtractor::open`) goes red.
#[test]
fn extraction_follows_the_directory_inspect_archive_validated() {
    let (env, b1, manifest) = base();
    let bytes = written(&env, &manifest, &[("a1", &b1)]);
    let shared = Shared {
        bytes: std::rc::Rc::new(std::cell::RefCell::new(bytes)),
        pos: 0,
    };
    let inspected = inspect_archive(shared.clone()).expect("a valid package");
    {
        let mut bytes = shared.bytes.borrow_mut();
        let record = (0..bytes.len() - 46)
            .find(|&i| {
                bytes[i..i + 4] == [0x50, 0x4b, 0x01, 0x02]
                    && bytes[i + 46..].starts_with(A1.as_bytes())
            })
            .expect("a1's central record");
        bytes[record + 46..record + 46 + A1.len()].copy_from_slice(b"media/z1.mp4");
    }
    let mut extractor =
        crate::editor::package_extract::PackageExtractor::from_validated(inspected.archive)
            .expect("the validated layout still holds");
    let mut out = Vec::new();
    extractor
        .extract(A1, &mut out, 300)
        .expect("the validated entry");
    assert_eq!(out, b1);
}

// S-3: the brief's own shape -- the FILE at the chosen path is replaced
// after inspection; extraction reads the handle that was validated, so the
// bytes are the original ones.
#[test]
fn extraction_reads_the_validated_handle_after_the_file_is_swapped() {
    let (env, b1, manifest) = base();
    let b2 = bytes_of(300, 9);
    let other: [(&str, &[u8]); 1] = [("a1", &b2)];
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("p.vbproject.zip");
    std::fs::write(&path, written(&env, &manifest, &[("a1", &b1)])).unwrap();
    let file = std::fs::File::open(&path).unwrap();
    let inspected = inspect_archive(&file).expect("a valid package");
    std::fs::rename(&path, dir.path().join("moved-away")).unwrap();
    std::fs::write(&path, written(&env, &manifest_for(&env, &other), &other)).unwrap();
    let mut extractor =
        crate::editor::package_extract::PackageExtractor::from_validated(inspected.archive)
            .unwrap();
    let mut out = Vec::new();
    extractor.extract(A1, &mut out, 300).unwrap();
    assert_eq!(out, b1, "the original bytes, not the swapped file's");
}
