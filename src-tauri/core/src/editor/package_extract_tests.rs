//! Tests for `package_extract.rs`. Archives are built in memory with
//! `ZipWriter` (Stored, so byte offsets are predictable) and then patched
//! the way `ZipWriter` itself never writes: a local name that disagrees
//! with the directory, a directory offset aimed at another entry's header,
//! an entry smuggled inside another's bytes, a size running into the
//! directory.

use std::io::{Cursor, Write};

use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use super::*;

const A: &str = "media/a1.mp4";
const B: &str = "media/b2.mp4";

fn zip_of(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    for (name, bytes) in entries {
        zip.start_file(*name, stored).unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

/// Where `name`'s central directory record starts.
fn central_record(bytes: &[u8], name: &str) -> usize {
    (0..bytes.len() - 46)
        .find(|&i| {
            bytes[i..i + 4] == [0x50, 0x4b, 0x01, 0x02]
                && bytes[i + 46..].starts_with(name.as_bytes())
        })
        .unwrap_or_else(|| panic!("no central record for {name}"))
}

fn u32_at(bytes: &[u8], i: usize) -> u32 {
    u32::from_le_bytes(bytes[i..i + 4].try_into().unwrap())
}

fn local_offset(bytes: &[u8], name: &str) -> u32 {
    u32_at(bytes, central_record(bytes, name) + 42)
}

fn set_u32(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

/// A bare Stored local header (CRC left 0: the crate checks the central
/// directory's CRC, so a smuggled header needs none of its own).
fn local_header(name: &str, len: u32) -> Vec<u8> {
    let mut h = vec![0x50, 0x4b, 0x03, 0x04, 20, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    h.extend_from_slice(&0u32.to_le_bytes());
    h.extend_from_slice(&len.to_le_bytes());
    h.extend_from_slice(&len.to_le_bytes());
    h.extend_from_slice(&(name.len() as u16).to_le_bytes());
    h.extend_from_slice(&0u16.to_le_bytes());
    h.extend_from_slice(name.as_bytes());
    h
}

fn verify(bytes: &[u8]) -> Result<(), EditorError> {
    let mut raw = Cursor::new(bytes);
    let mut archive = ZipArchive::new(Cursor::new(bytes)).unwrap();
    verify_entry_layout(&mut raw, &mut archive)
}

fn extract(bytes: &[u8], name: &str) -> Result<Vec<u8>, EditorError> {
    let mut raw = Cursor::new(bytes);
    let mut archive = ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut out = Vec::new();
    extract_verified_entry(&mut raw, &mut archive, name, &mut out, 1 << 20).map(|_| out)
}

fn refused(result: Result<impl std::fmt::Debug, EditorError>, needle: &str) {
    let err = result.expect_err("the archive must be refused");
    assert_eq!(err.code, EditorErrorCode::InvalidProject, "{}", err.message);
    assert!(
        err.message.contains(needle),
        "expected {needle:?} in {}",
        err.message
    );
}

fn good() -> Vec<u8> {
    zip_of(&[("package.json", b"{}"), (A, b"alpha-bytes"), (B, b"bravo")])
}

#[test]
fn a_well_formed_archive_passes_and_extracts_the_named_bytes() {
    let bytes = good();
    verify(&bytes).expect("a ZipWriter archive is well formed");
    assert_eq!(extract(&bytes, B).unwrap(), b"bravo");
    assert_eq!(extract(&bytes, A).unwrap(), b"alpha-bytes");
}

// The crate alone copies these bytes without a word: the local header says
// `media/z1.mp4` while the directory (and so the manifest check) says
// `media/a1.mp4`, and another unzip tool would name the file differently.
#[test]
fn a_local_header_naming_another_entry_is_refused_at_extraction() {
    let mut bytes = good();
    let local = local_offset(&bytes, A) as usize;
    bytes[local + 30..local + 30 + A.len()].copy_from_slice(b"media/z1.mp4");
    let mut archive = ZipArchive::new(Cursor::new(&bytes[..])).unwrap();
    let mut blind = Vec::new();
    extract_entry_bounded(&mut archive, A, &mut blind, 1 << 20)
        .expect("the zip crate does not compare the two names");
    refused(extract(&bytes, A), "local header");
    refused(verify(&bytes), "local header");
    assert_eq!(
        extract(&bytes, B).unwrap(),
        b"bravo",
        "B itself is untouched"
    );
}

#[test]
fn a_directory_offset_aimed_at_another_entrys_header_is_refused() {
    let mut bytes = good();
    let a_offset = local_offset(&bytes, A);
    let b_record = central_record(&bytes, B);
    set_u32(&mut bytes, b_record + 42, a_offset);
    refused(extract(&bytes, B), "local header");
}

// The overlapping-entry shape (one entry's bytes hide another's header):
// the smuggled header's name and sizes are right, so only the byte ranges
// give it away.
#[test]
fn entries_sharing_bytes_are_refused() {
    let mut inner = local_header(B, 5);
    inner.extend_from_slice(b"bravo");
    let mut bytes = zip_of(&[("package.json", b"{}"), (A, &inner), (B, b"bravo")]);
    let a_data = local_offset(&bytes, A) as usize + 30 + A.len();
    let b_record = central_record(&bytes, B);
    set_u32(&mut bytes, b_record + 42, a_data as u32);
    assert_eq!(
        extract(&bytes, B).unwrap(),
        b"bravo",
        "B's own header is consistent"
    );
    refused(verify(&bytes), "share bytes");
}

#[test]
fn an_entry_running_into_the_central_directory_is_refused() {
    let mut bytes = good();
    let b_record = central_record(&bytes, B);
    set_u32(&mut bytes, b_record + 20, 5 + 40);
    set_u32(&mut bytes, b_record + 24, 5 + 40);
    refused(verify(&bytes), "central directory");
}

#[test]
fn the_extractor_verifies_the_layout_before_it_opens() {
    let bytes = good();
    let mut ok = PackageExtractor::open(Cursor::new(&bytes[..])).expect("well formed");
    let mut out = Vec::new();
    let got = ok.extract(A, &mut out, 1 << 20).unwrap();
    assert_eq!((out.as_slice(), got.bytes), (&b"alpha-bytes"[..], 11));
    let mut bad = good();
    let local = local_offset(&bad, B) as usize;
    bad[local + 30] = b'X';
    refused(
        PackageExtractor::open(Cursor::new(&bad[..])).map(|_| ()),
        "local header",
    );
}
