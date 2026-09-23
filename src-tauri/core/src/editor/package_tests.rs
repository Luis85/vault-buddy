//! Tests for `package.rs` (sibling `#[path]` file, the `validate_tests.rs`
//! precedent, so `package.rs` stays under the 800-line Rust cap). Every
//! archive is built in memory: well-formed ones through `write_package`,
//! adversarial ones through `raw_zip` (entry order, compression and names
//! chosen by the test) plus byte patches for what `zip::ZipWriter` itself
//! refuses to produce (an exact duplicate name, a lying size header).

use std::io::Cursor;

use sha2::{Digest, Sha256};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use super::*;
use crate::editor::model::{AssetKind, Builtin, Project, TrackKind};
use crate::editor::model_cues::{Product, Record, WorkspaceEnvelope};
use crate::editor::test_support::{asset, clip, minimal_project, track};
use crate::editor::{new_product, Map, WORKSPACE_SCHEMA};

const A1: &str = "media/a1.mp4";

/// A project whose every listed asset is a 5000 ms video on its own clip
/// on track t1 (clips placed 2000 ms apart so they never overlap).
fn project_using(asset_ids: &[&str]) -> Project {
    let mut project = minimal_project();
    project.id = "proj1".to_string();
    project.tracks.push(track("t1", TrackKind::Video, false));
    for (i, id) in asset_ids.iter().enumerate() {
        project.assets.push(asset(id, AssetKind::Video, 5000));
        let start = i as u64 * 2000;
        project
            .clips
            .push(clip(&format!("c{i}"), "t1", id, start, 0, 1500));
    }
    project
}

fn envelope_of(project: Project, products: Vec<Product>) -> WorkspaceEnvelope {
    WorkspaceEnvelope {
        schema: WORKSPACE_SCHEMA.to_string(),
        project,
        workspace: serde_json::json!({}),
        record: Record {
            id: "rec1".to_string(),
            revision: 3,
            created_at: "2026-09-21T00:00:00Z".to_string(),
            updated_at: "2026-09-22T00:00:00Z".to_string(),
            products,
            extra: Map::new(),
        },
        saved_at: "2026-09-23T10:00:00Z".to_string(),
        extra: Map::new(),
    }
}

/// Deterministic, non-repeating-enough bytes (a Stored entry never trips
/// the ratio guard, and a distinct length per asset catches a size mix-up).
fn bytes_of(len: usize, seed: u32) -> Vec<u8> {
    (0..len as u32)
        .map(|i| (i.wrapping_mul(7).wrapping_add(seed) % 251) as u8)
        .collect()
}

fn sha(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn media_entry(asset_id: &str, bytes: &[u8]) -> PackageMedia {
    PackageMedia {
        asset_id: asset_id.to_string(),
        path: format!("media/{asset_id}.mp4"),
        size: bytes.len() as u64,
        sha256: sha(bytes),
    }
}

fn manifest_for(env: &WorkspaceEnvelope, media: &[(&str, &[u8])]) -> PackageManifest {
    PackageManifest {
        schema: crate::editor::PACKAGE_SCHEMA.to_string(),
        project_id: env.project.id.clone(),
        saved_at: env.saved_at.clone(),
        workspace: WORKSPACE_NAME.to_string(),
        media: media.iter().map(|(id, b)| media_entry(id, b)).collect(),
        products: Vec::new(),
    }
}

fn written(
    env: &WorkspaceEnvelope,
    manifest: &PackageManifest,
    media: &[(&str, &[u8])],
) -> Vec<u8> {
    let files = media
        .iter()
        .map(|(id, b)| (format!("media/{id}.mp4"), Cursor::new(b.to_vec())));
    let workspace = serde_json::to_vec(env).unwrap();
    write_package(Cursor::new(Vec::new()), manifest, &workspace, files)
        .expect("write_package")
        .into_inner()
}

/// Entries in exactly the given order, name and compression.
fn raw_zip(entries: &[(&str, &[u8], CompressionMethod)]) -> Vec<u8> {
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes, method) in entries {
        let options = SimpleFileOptions::default().compression_method(*method);
        zip.start_file(*name, options).unwrap();
        std::io::Write::write_all(&mut zip, bytes).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

fn json(value: &impl serde::Serialize) -> Vec<u8> {
    serde_json::to_vec(value).unwrap()
}

fn inspect(bytes: &[u8]) -> Result<PackageIndex, EditorError> {
    inspect_archive(Cursor::new(bytes.to_vec()))
}

fn rejected(bytes: &[u8], needle: &str) -> EditorError {
    let err = inspect(bytes).expect_err("the package must be rejected");
    assert_eq!(err.code, EditorErrorCode::InvalidProject, "{}", err.message);
    assert!(
        err.message.contains(needle),
        "expected {needle:?} in: {}",
        err.message
    );
    err
}

/// Rewrites every `(local, central)` header of the entry named `name`:
/// `Some(c)` compressed size, `Some(u)` uncompressed size. Offsets are the
/// APPNOTE's: local header compressed@18 uncompressed@22 name@30; central
/// header compressed@20 uncompressed@24 name@46.
fn patch_sizes(bytes: &mut [u8], name: &str, compressed: Option<u32>, uncompressed: Option<u32>) {
    let name = name.as_bytes();
    let mut patched = 0;
    for at in 0..bytes.len().saturating_sub(46) {
        let (c_off, u_off, n_off) = match &bytes[at..at + 4] {
            [0x50, 0x4b, 0x03, 0x04] => (18, 22, 30),
            [0x50, 0x4b, 0x01, 0x02] => (20, 24, 46),
            _ => continue,
        };
        if bytes.get(at + n_off..at + n_off + name.len()) != Some(name) {
            continue;
        }
        if let Some(c) = compressed {
            bytes[at + c_off..at + c_off + 4].copy_from_slice(&c.to_le_bytes());
        }
        if let Some(u) = uncompressed {
            bytes[at + u_off..at + u_off + 4].copy_from_slice(&u.to_le_bytes());
        }
        patched += 1;
    }
    assert_eq!(patched, 2, "expected one local and one central header");
}

// ---- The brief's named tests ---------------------------------------------

#[test]
fn round_trip_package_validates() {
    let env = envelope_of(project_using(&["a1", "a2"]), Vec::new());
    let (b1, b2) = (bytes_of(3000, 1), bytes_of(1700, 9));
    let media: [(&str, &[u8]); 2] = [("a1", &b1), ("a2", &b2)];
    let manifest = manifest_for(&env, &media);
    let bytes = written(&env, &manifest, &media);

    let index = inspect(&bytes).expect("a package write_package made must validate");
    assert_eq!(index.manifest, manifest);
    assert_eq!(index.envelope, env);
    assert!(index.missing.is_empty(), "{:?}", index.missing);

    let mut archive = ZipArchive::new(Cursor::new(bytes)).unwrap();
    assert_eq!(archive.name_for_index(0), Some(MANIFEST_NAME));
    let methods: Vec<_> = (0..archive.len())
        .map(|i| {
            let f = archive.by_index_raw(i).unwrap();
            (f.name().to_string(), f.compression())
        })
        .collect();
    assert_eq!(
        methods,
        vec![
            (MANIFEST_NAME.to_string(), CompressionMethod::Deflated),
            (WORKSPACE_NAME.to_string(), CompressionMethod::Deflated),
            (A1.to_string(), CompressionMethod::Stored),
            ("media/a2.mp4".to_string(), CompressionMethod::Stored),
        ]
    );
    let mut out = Vec::new();
    let got = extract_entry_bounded(&mut archive, "media/a2.mp4", &mut out, b2.len() as u64)
        .expect("an honest entry extracts at exactly its size");
    assert_eq!(out, b2);
    assert_eq!(got.bytes, b2.len() as u64);
    assert_eq!(got.sha256, manifest.media[1].sha256);
}

#[test]
fn traversal_names_are_rejected() {
    let env = envelope_of(project_using(&["a1"]), Vec::new());
    let b1 = bytes_of(300, 1);
    let manifest = manifest_for(&env, &[("a1", &b1)]);
    for ok in [MANIFEST_NAME, WORKSPACE_NAME, A1, "products/p1.mp4"] {
        assert_eq!(validate_entry_name(ok), Ok(()), "{ok}");
    }
    for bad in ["../x", "media/../../x", "/abs", "C:x", "\\\\server\\s"] {
        assert!(validate_entry_name(bad).is_err(), "{bad:?} must be refused");
        let bytes = raw_zip(&[
            (MANIFEST_NAME, &json(&manifest), CompressionMethod::Deflated),
            (WORKSPACE_NAME, &json(&env), CompressionMethod::Deflated),
            (A1, &b1, CompressionMethod::Stored),
            (bad, b"x", CompressionMethod::Stored),
        ]);
        let err = rejected(&bytes, "is not allowed");
        let quoted = format!("{bad:?}");
        assert!(err.message.contains(&quoted), "{}", err.message);
    }
}

#[test]
fn duplicate_case_folded_names_are_rejected() {
    // Asset ids ARE case-sensitive, so "a" and "A" are two legal assets --
    // but their files collide on Windows' case-insensitive filesystem,
    // and an extraction would let one overwrite the other.
    let env = envelope_of(project_using(&["A", "a"]), Vec::new());
    let (upper, lower) = (bytes_of(400, 2), bytes_of(401, 3));
    let manifest = manifest_for(&env, &[("A", &upper), ("a", &lower)]);
    let bytes = raw_zip(&[
        (MANIFEST_NAME, &json(&manifest), CompressionMethod::Deflated),
        (WORKSPACE_NAME, &json(&env), CompressionMethod::Deflated),
        ("media/A.mp4", &upper, CompressionMethod::Stored),
        ("media/a.mp4", &lower, CompressionMethod::Stored),
    ]);
    let err = rejected(&bytes, "duplicate");
    assert!(err.message.contains("media/a.mp4"), "{}", err.message);
}

#[test]
fn too_many_entries_are_rejected() {
    let over: Vec<String> = (0..limits::MAX_PACKAGE_ENTRIES)
        .map(|i| format!("media/x{i}.mp4"))
        .collect();
    let mut entries: Vec<(&str, &[u8], CompressionMethod)> =
        vec![(MANIFEST_NAME, b"{}", CompressionMethod::Deflated)];
    entries.extend(
        over.iter()
            .map(|n| (n.as_str(), &b""[..], CompressionMethod::Stored)),
    );
    assert_eq!(entries.len(), limits::MAX_PACKAGE_ENTRIES + 1);
    rejected(&raw_zip(&entries), "1001 entries");

    // Exactly at the limit, the count guard stays quiet (the package is
    // still refused, for its bogus manifest -- a different rule).
    entries.pop();
    let err = inspect(&raw_zip(&entries)).expect_err("still not a package");
    assert!(!err.message.contains("entries"), "{}", err.message);
}

#[test]
fn ratio_bomb_is_rejected() {
    let env = envelope_of(project_using(&["a1"]), Vec::new());
    let zeros = vec![0u8; 1024 * 1024];
    let manifest = manifest_for(&env, &[("a1", &zeros)]);
    let bytes = raw_zip(&[
        (MANIFEST_NAME, &json(&manifest), CompressionMethod::Deflated),
        (WORKSPACE_NAME, &json(&env), CompressionMethod::Deflated),
        (A1, &zeros, CompressionMethod::Deflated),
    ]);
    let err = rejected(&bytes, "ratio");
    assert!(err.message.contains(A1), "{}", err.message);
}

#[test]
fn lying_size_header_cannot_overrun_extraction() {
    let body = bytes_of(64 * 1024, 5);
    let mut bytes = raw_zip(&[(A1, &body, CompressionMethod::Deflated)]);
    // The header now claims 100 bytes; the deflate stream still holds 64 KiB.
    patch_sizes(&mut bytes, A1, None, Some(100));
    let mut archive = ZipArchive::new(Cursor::new(bytes)).unwrap();
    assert_eq!(archive.by_name(A1).unwrap().size(), 100, "the lie took");

    let mut out = Vec::new();
    let err = extract_entry_bounded(&mut archive, A1, &mut out, 100)
        .expect_err("a stream longer than the limit must fail");
    assert!(err.message.contains("100-byte limit"), "{}", err.message);
    assert!(out.len() <= 100, "wrote {} bytes past the limit", out.len());
}

#[test]
fn manifest_must_come_first() {
    let env = envelope_of(project_using(&["a1"]), Vec::new());
    let b1 = bytes_of(300, 1);
    let manifest = manifest_for(&env, &[("a1", &b1)]);
    let bytes = raw_zip(&[
        (WORKSPACE_NAME, &json(&env), CompressionMethod::Deflated),
        (MANIFEST_NAME, &json(&manifest), CompressionMethod::Deflated),
        (A1, &b1, CompressionMethod::Stored),
    ]);
    rejected(&bytes, "first entry");
}

// A17: an asset only a retained product's SNAPSHOT still uses must either
// ship in the package or be reported missing -- never pruned because the
// live edit no longer names it.
#[test]
fn snapshot_only_assets_are_required() {
    let old = project_using(&["old"]);
    let product = new_product(
        &old,
        2,
        "p1",
        "Take one",
        "take-one.mp4",
        1500,
        None,
        "2026-09-22T00:00:00Z",
    );
    let env = envelope_of(project_using(&["a1"]), vec![product]);
    let (b1, b_old) = (bytes_of(300, 1), bytes_of(500, 4));

    let media: [(&str, &[u8]); 2] = [("a1", &b1), ("old", &b_old)];
    let with_old = written(&env, &manifest_for(&env, &media), &media);
    let index = inspect(&with_old).expect("a snapshot-only asset is a legal media entry");
    assert!(index.missing.is_empty(), "{:?}", index.missing);

    let without: [(&str, &[u8]); 1] = [("a1", &b1)];
    let lightweight = written(&env, &manifest_for(&env, &without), &without);
    let index = inspect(&lightweight).expect("a missing source is reported, not fatal");
    assert_eq!(index.missing, vec!["old".to_string()]);
}

// A22: a structurally perfect archive whose workspace breaks one semantic
// rule is refused whole -- nothing about the (valid) manifest or media
// survives into a partial import.
#[test]
fn invalid_workspace_rejects_the_whole_package() {
    let b1 = bytes_of(300, 1);
    let media: [(&str, &[u8]); 1] = [("a1", &b1)];

    let mut duplicate = envelope_of(project_using(&["a1"]), Vec::new());
    duplicate
        .project
        .assets
        .push(asset("a1", AssetKind::Video, 5000));
    let bytes = written(&duplicate, &manifest_for(&duplicate, &media), &media);
    rejected(&bytes, "asset a1: duplicate id");

    let mut cyclic = envelope_of(project_using(&["a1"]), Vec::new());
    let mut a2 = asset("a2", AssetKind::Audio, 5000);
    a2.linked_asset = Some("a1".to_string());
    cyclic.project.assets[0].linked_asset = Some("a2".to_string());
    cyclic.project.assets.push(a2);
    let bytes = written(&cyclic, &manifest_for(&cyclic, &media), &media);
    rejected(&bytes, "cycle");
}

// ---- Beyond the named tests: every other rule the module doc lists ------

/// The one-asset package every defect case below starts from.
fn base() -> (WorkspaceEnvelope, Vec<u8>, PackageManifest) {
    let env = envelope_of(project_using(&["a1"]), Vec::new());
    let b1 = bytes_of(300, 1);
    let manifest = manifest_for(&env, &[("a1", &b1)]);
    (env, b1, manifest)
}

/// `[package.json, workspace.json, media/a1.mp4, extras...]`.
fn assembled(manifest: &[u8], env: &WorkspaceEnvelope, b1: &[u8], extras: &[&str]) -> Vec<u8> {
    let env_json = json(env);
    let mut entries = vec![
        (MANIFEST_NAME, manifest, CompressionMethod::Deflated),
        (WORKSPACE_NAME, &env_json[..], CompressionMethod::Deflated),
        (A1, b1, CompressionMethod::Stored),
    ];
    entries.extend(extras.iter().map(|n| (*n, b1, CompressionMethod::Stored)));
    raw_zip(&entries)
}

fn listed(id: &str, path: &str, body: &[u8]) -> PackageMedia {
    PackageMedia {
        asset_id: id.to_string(),
        path: path.to_string(),
        size: body.len() as u64,
        sha256: sha(body),
    }
}

type Defect = Box<dyn Fn(&mut PackageManifest)>;

#[test]
fn every_manifest_defect_is_rejected() {
    let (env, b1, good) = base();
    let b9 = b1.clone();
    let cases: Vec<(&str, Defect, Vec<&str>)> = vec![
        (
            "schema must be",
            Box::new(|m| m.schema = "vault-buddy-project-package/2".into()),
            vec![],
        ),
        (
            "projectId is not a valid id",
            Box::new(|m| m.project_id = "bad id".into()),
            vec![],
        ),
        (
            "savedAt",
            Box::new(|m| m.saved_at = "yesterday".into()),
            vec![],
        ),
        (
            "workspace must be",
            Box::new(|m| m.workspace = "ws.json".into()),
            vec![],
        ),
        (
            "is not its expected name",
            Box::new(|m| m.media[0].path = "media/a1".into()),
            vec![],
        ),
        (
            "is not its expected name",
            Box::new(|m| m.media[0].path = "media/a1.mp4x9abcd".into()),
            vec![],
        ),
        (
            "sha256",
            Box::new(|m| m.media[0].sha256 = m.media[0].sha256.to_uppercase()),
            vec![],
        ),
        (
            "declares size 301",
            Box::new(|m| m.media[0].size += 1),
            vec![],
        ),
        (
            "invalid or duplicate id",
            Box::new(|m| m.media.push(m.media[0].clone())),
            vec![],
        ),
        (
            "missing from the archive",
            Box::new(move |m| m.media.push(listed("a9", "media/a9.mp4", &b9))),
            vec![],
        ),
        (
            "not listed in the manifest",
            Box::new(|_| {}),
            vec!["media/extra.mp4"],
        ),
        (
            "does not match the workspace",
            Box::new(|m| m.project_id = "other".into()),
            vec![],
        ),
        (
            "too many",
            Box::new(|m| m.media = vec![m.media[0].clone(); limits::MAX_ASSETS + 1]),
            vec![],
        ),
    ];
    for (needle, defect, extras) in cases {
        let mut m = good.clone();
        defect(&mut m);
        rejected(&assembled(&json(&m), &env, &b1, &extras), needle);
    }

    let mut m = good.clone();
    m.media.push(listed("zz", "media/zz.mp4", &b1));
    rejected(
        &assembled(&json(&m), &env, &b1, &["media/zz.mp4"]),
        "is not used by",
    );
    let mut m = good.clone();
    m.products.push(PackageProduct {
        product_id: "p9".into(),
        path: "products/p9.mp4".into(),
        size: b1.len() as u64,
        sha256: sha(&b1),
    });
    rejected(
        &assembled(&json(&m), &env, &b1, &["products/p9.mp4"]),
        "not a product",
    );

    rejected(&assembled(b"{", &env, &b1, &[]), "manifest:");
    let mut unknown = serde_json::to_value(&good).unwrap();
    unknown["future"] = serde_json::json!(1);
    rejected(&assembled(&json(&unknown), &env, &b1, &[]), "unknown field");
    // Sanity: the unmodified base really is valid, so each case above
    // failed for its own defect and nothing else.
    assert!(inspect(&assembled(&json(&good), &env, &b1, &[])).is_ok());
}

// `zip::ZipArchive` keeps only ONE of two entries sharing an exact name
// (`ZipWriter` refuses to write such an archive, so the second name is
// patched in): without the end-record count cross-check, a second body
// hides behind the first and the archive reads as perfectly valid.
#[test]
fn exact_duplicate_names_are_rejected() {
    let (env, b1, manifest) = base();
    let mut bytes = assembled(&json(&manifest), &env, &b1, &["media/b1.mp4"]);
    let (from, to) = (b"media/b1.mp4", b"media/a1.mp4");
    let mut replaced = 0;
    for at in 0..bytes.len() - from.len() {
        if &bytes[at..at + from.len()] == from {
            bytes[at..at + to.len()].copy_from_slice(to);
            replaced += 1;
        }
    }
    assert_eq!(replaced, 2, "one local and one central name");
    let collapsed = ZipArchive::new(Cursor::new(bytes.clone())).unwrap().len();
    assert_eq!(collapsed, 3, "the crate collapsed the duplicate");
    rejected(&bytes, "duplicate entry names");
}

#[test]
fn symlink_entries_are_rejected() {
    let (env, _, _) = base();
    let target = "../../outside";
    let manifest = manifest_for(&env, &[("a1", target.as_bytes())]);
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    zip.start_file(MANIFEST_NAME, deflated).unwrap();
    std::io::Write::write_all(&mut zip, &json(&manifest)).unwrap();
    zip.start_file(WORKSPACE_NAME, deflated).unwrap();
    std::io::Write::write_all(&mut zip, &json(&env)).unwrap();
    zip.add_symlink(A1, target, SimpleFileOptions::default())
        .unwrap();
    let bytes = zip.finish().unwrap().into_inner();
    rejected(&bytes, "not a regular file");
}

#[test]
fn trailing_data_and_archive_comments_are_rejected() {
    let (env, b1, manifest) = base();
    let mut bytes = assembled(&json(&manifest), &env, &b1, &[]);
    bytes.extend_from_slice(b"trailing");
    rejected(&bytes, "plain ZIP end record");

    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    zip.set_comment("hello");
    zip.start_file(MANIFEST_NAME, SimpleFileOptions::default())
        .unwrap();
    rejected(&zip.finish().unwrap().into_inner(), "plain ZIP end record");
    rejected(b"PK", "too short");
}

/// `count` Stored entries whose headers DECLARE `declared` bytes each (the
/// bytes themselves stay small): the size rules read declarations, so this
/// exercises them without allocating hundreds of MiB.
fn declared(declared: u32, count: usize) -> Vec<u8> {
    let ids = ["a1", "a2", "a3"];
    let env = envelope_of(project_using(&ids), Vec::new());
    let body = bytes_of(64, 7);
    let mut manifest = manifest_for(&env, &[]);
    let mut entries: Vec<String> = Vec::new();
    for id in &ids[..count] {
        let mut m = media_entry(id, &body);
        m.size = u64::from(declared);
        manifest.media.push(m);
        entries.push(format!("media/{id}.mp4"));
    }
    let (m_json, e_json) = (json(&manifest), json(&env));
    let mut raw: Vec<(&str, &[u8], CompressionMethod)> = vec![
        (MANIFEST_NAME, &m_json, CompressionMethod::Deflated),
        (WORKSPACE_NAME, &e_json, CompressionMethod::Deflated),
    ];
    raw.extend(
        entries
            .iter()
            .map(|n| (n.as_str(), &body[..], CompressionMethod::Stored)),
    );
    let mut bytes = raw_zip(&raw);
    for name in &entries {
        patch_sizes(&mut bytes, name, Some(declared), Some(declared));
    }
    bytes
}

#[test]
fn media_past_the_portable_limit_is_rejected() {
    // 2 x 105 MiB = 210 MiB: inside the 220 MiB package allowance, past
    // the 200 MiB media limit (the ledger ruling's portable cap).
    rejected(&declared(105 * 1024 * 1024, 2), "portable limit");
}

#[test]
fn declared_expansion_past_the_package_limit_is_rejected() {
    // 3 x 80 MiB = 240 MiB declared, from an archive of a few KiB.
    rejected(&declared(80 * 1024 * 1024, 3), "expands past");
}

#[test]
fn an_oversized_archive_is_refused_before_parsing() {
    struct Huge;
    impl std::io::Read for Huge {
        fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
            panic!("an oversized archive must not be read at all")
        }
    }
    impl std::io::Seek for Huge {
        fn seek(&mut self, _: std::io::SeekFrom) -> std::io::Result<u64> {
            Ok(limits::MAX_PACKAGE_BYTES + 1)
        }
    }
    let err = inspect_archive(Huge).expect_err("refused");
    assert!(err.message.contains("past the"), "{}", err.message);
}

// Which referenced assets need bytes: a `card` builtin is synthesized from
// the project and linked (detached) audio reads its source's bytes -- but
// a `screen` builtin is natively a staged capture FILE, so a package
// without it must say so rather than treat "builtin" as "no file".
#[test]
fn only_cards_and_linked_audio_ship_without_media() {
    let mut project = project_using(&["scr", "card", "vid"]);
    project.assets[0].builtin = Some(Builtin::Screen);
    project.assets[1].builtin = Some(Builtin::Card);
    project.tracks.push(track("t2", TrackKind::Audio, false));
    let mut audio = asset("vid-audio", AssetKind::Audio, 5000);
    audio.linked_asset = Some("vid".to_string());
    project.assets.push(audio);
    project
        .clips
        .push(clip("c9", "t2", "vid-audio", 4000, 0, 1500));
    let env = envelope_of(project, Vec::new());
    let body = bytes_of(200, 3);
    let media: [(&str, &[u8]); 1] = [("vid", &body)];
    let index = inspect(&written(&env, &manifest_for(&env, &media), &media)).unwrap();
    assert_eq!(index.missing, vec!["scr".to_string()]);
}

#[test]
fn manifest_serializes_to_its_documented_literal() {
    let literal = serde_json::json!({
        "schema": "vault-buddy-project-package/1",
        "projectId": "proj1",
        "savedAt": "2026-09-23T10:00:00Z",
        "workspace": "workspace.json",
        "media": [{"assetId": "a1", "path": "media/a1.webm", "size": 3000, "sha256": "ab".repeat(32)}],
        "products": [{"productId": "p1", "path": "products/p1.mp4", "size": 17, "sha256": "cd".repeat(32)}]
    });
    let manifest = PackageManifest {
        schema: "vault-buddy-project-package/1".into(),
        project_id: "proj1".into(),
        saved_at: "2026-09-23T10:00:00Z".into(),
        workspace: "workspace.json".into(),
        media: vec![PackageMedia {
            asset_id: "a1".into(),
            path: "media/a1.webm".into(),
            size: 3000,
            sha256: "ab".repeat(32),
        }],
        products: vec![PackageProduct {
            product_id: "p1".into(),
            path: "products/p1.mp4".into(),
            size: 17,
            sha256: "cd".repeat(32),
        }],
    };
    assert_eq!(serde_json::to_value(&manifest).unwrap(), literal);
    let decoded: PackageManifest = serde_json::from_value(literal).unwrap();
    assert_eq!(decoded, manifest);
}

#[test]
fn write_package_refuses_an_inconsistent_file_set() {
    let (env, b1, manifest) = base();
    let ws = json(&env);
    let write = |files: Vec<(&str, Vec<u8>)>| {
        let files = files.into_iter().map(|(n, b)| (n, Cursor::new(b)));
        write_package(Cursor::new(Vec::new()), &manifest, &ws, files).expect_err("refused")
    };
    let unlisted = write(vec![(A1, b1.clone()), ("media/zz.mp4", b1.clone())]);
    assert!(
        unlisted.message.contains("not an unwritten manifest path"),
        "{}",
        unlisted.message
    );
    let short = write(vec![(A1, b1[..10].to_vec())]);
    assert!(short.message.contains("10 bytes"), "{}", short.message);
    let absent = write(vec![]);
    assert!(
        absent.message.contains("never written"),
        "{}",
        absent.message
    );
    let traversal = write(vec![("media/../x", b1.clone())]);
    assert!(
        traversal.message.contains("segment"),
        "{}",
        traversal.message
    );
    for err in [unlisted, short, absent, traversal] {
        assert_eq!(err.code, EditorErrorCode::Internal);
    }
}

#[test]
fn extracting_an_absent_entry_is_refused() {
    let (env, b1, manifest) = base();
    let bytes = assembled(&json(&manifest), &env, &b1, &[]);
    let mut archive = ZipArchive::new(Cursor::new(bytes)).unwrap();
    let err = extract_entry_bounded(&mut archive, "media/zz.mp4", &mut Vec::new(), 10)
        .expect_err("absent");
    assert_eq!(err.code, EditorErrorCode::InvalidProject);
}
