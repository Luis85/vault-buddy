//! The raw ZIP structure checks `package::inspect_archive` runs BEFORE the
//! `zip` crate is handed the archive (Task 38; split out of `package.rs` in
//! fix round 1 at its 800-line cap). Two facts about `zip::ZipArchive::new`
//! (4.6.1, the same in 2.4.2) make them necessary rather than redundant:
//!
//! - It indexes entries by NAME in an `IndexMap`, so of two entries sharing
//!   an exact name it silently keeps one; a second `workspace.json` could
//!   hide behind the first. Only a count read independently of the crate
//!   can see the collapse.
//! - When the final end record's directory fails to parse, it walks BACK
//!   through every earlier end-record signature in the file -- and media
//!   bytes can hold one, ZIP64 fields included, which then sizes a
//!   `Vec::with_capacity` from attacker-chosen counts. Pinning the end
//!   record to the file's final 22 bytes, refusing every ZIP64 sentinel and
//!   walking the whole directory here first means the crate is only ever
//!   given a directory it will accept on its first candidate.
//!
//! Everything here reads at most 22 bytes plus a directory bounded by
//! `MAX_PACKAGE_ENTRIES` records of at most `46 + 255` bytes each.

use std::io::{Read, Seek, SeekFrom};

use super::error::{EditorError, EditorErrorCode};
use super::limits;

/// The end-of-central-directory record's fixed size (APPNOTE 4.3.16).
const END_RECORD_LEN: u64 = 22;
/// A central directory file header's fixed part (APPNOTE 4.3.12).
const CENTRAL_HEADER_LEN: usize = 46;
/// The longest entry name `validate_entry_name` accepts, in bytes.
pub(super) const MAX_ENTRY_NAME_BYTES: usize = 255;

fn invalid(message: impl Into<String>) -> EditorError {
    EditorError::new(EditorErrorCode::InvalidProject, message)
}

fn unreadable(e: impl std::fmt::Display) -> EditorError {
    invalid(format!(
        "the project package is not a readable archive: {e}"
    ))
}

fn u16_at(b: &[u8], i: usize) -> u64 {
    u64::from(u16::from_le_bytes([b[i], b[i + 1]]))
}

fn u32_at(b: &[u8], i: usize) -> u64 {
    u64::from(u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]))
}

/// What the archive's own end record says, read without the zip crate.
pub(super) struct EndRecord {
    pub entries: u64,
    pub directory_offset: u64,
    pub directory_size: u64,
}

/// Reads the end record from the archive's final 22 bytes (so no comment
/// and no trailing data, which `write_package` never emits), refusing a
/// multi-disk or ZIP64 archive and a directory that does not end exactly
/// where the record begins.
pub(super) fn read_end_record<R: Read + Seek>(
    reader: &mut R,
    len: u64,
) -> Result<EndRecord, EditorError> {
    if len < END_RECORD_LEN {
        return Err(invalid("the project package is too short to be an archive"));
    }
    let mut rec = [0u8; END_RECORD_LEN as usize];
    reader
        .seek(SeekFrom::Start(len - END_RECORD_LEN))
        .and_then(|_| reader.read_exact(&mut rec))
        .map_err(unreadable)?;
    if rec[..4] != [0x50, 0x4b, 0x05, 0x06] || u16_at(&rec, 20) != 0 {
        return Err(invalid(
            "the project package must end with a plain ZIP end record (no comment or trailing data)",
        ));
    }
    let entries = u16_at(&rec, 10);
    let (directory_size, directory_offset) = (u32_at(&rec, 12), u32_at(&rec, 16));
    if u16_at(&rec, 4) != 0 || u16_at(&rec, 6) != 0 || u16_at(&rec, 8) != entries {
        return Err(invalid("the project package spans several disks"));
    }
    if entries == 0xFFFF || directory_offset == 0xFFFF_FFFF || directory_size == 0xFFFF_FFFF {
        return Err(invalid(
            "the project package uses ZIP64, which no package needs",
        ));
    }
    if directory_offset + directory_size != len - END_RECORD_LEN {
        return Err(invalid(
            "the project package's central directory does not end at its end record",
        ));
    }
    Ok(EndRecord {
        entries,
        directory_offset,
        directory_size,
    })
}

/// Walks every central directory record the end record announces, from
/// the raw bytes. Refuses a malformed or overlong record, an entry that is
/// encrypted (flag bit 0 or 6), compressed with anything but Stored (0) or
/// Deflated (8) -- which also keeps AES (99), whose parse the crate can
/// fail, away from it -- one that carries an extra field or a comment
/// (`write_package` writes neither), one starting on another disk, and a
/// directory with bytes left over. The caller has already bounded
/// `end.entries` by `MAX_PACKAGE_ENTRIES`.
pub(super) fn walk_central_directory<R: Read + Seek>(
    reader: &mut R,
    end: &EndRecord,
) -> Result<(), EditorError> {
    let most = end.entries * (CENTRAL_HEADER_LEN + MAX_ENTRY_NAME_BYTES) as u64;
    if end.entries > limits::MAX_PACKAGE_ENTRIES as u64 || end.directory_size > most {
        return Err(invalid(
            "the project package's central directory is larger than its entries can need",
        ));
    }
    let mut dir = vec![0u8; end.directory_size as usize];
    reader
        .seek(SeekFrom::Start(end.directory_offset))
        .and_then(|_| reader.read_exact(&mut dir))
        .map_err(unreadable)?;
    let mut at = 0usize;
    for record in 1..=end.entries {
        let malformed = || invalid(format!("central directory record {record} is malformed"));
        let head = dir.get(at..at + CENTRAL_HEADER_LEN).ok_or_else(malformed)?;
        if head[..4] != [0x50, 0x4b, 0x01, 0x02] {
            return Err(malformed());
        }
        let name_len = u16_at(head, 28) as usize;
        let name = dir
            .get(at + CENTRAL_HEADER_LEN..at + CENTRAL_HEADER_LEN + name_len)
            .ok_or_else(malformed)?;
        let name = String::from_utf8_lossy(name);
        let entry = |why: &str| invalid(format!("package entry {name:?} {why}"));
        if name_len > MAX_ENTRY_NAME_BYTES {
            return Err(malformed());
        }
        if u16_at(head, 8) & 0b0100_0001 != 0 {
            return Err(entry("is encrypted"));
        }
        let method = u16_at(head, 10);
        if method != 0 && method != 8 {
            return Err(entry(&format!("uses unsupported compression {method}")));
        }
        if u16_at(head, 30) != 0 || u16_at(head, 32) != 0 {
            return Err(entry("carries a ZIP extra field or comment"));
        }
        if u16_at(head, 34) != 0 {
            return Err(entry("starts on another disk"));
        }
        at += CENTRAL_HEADER_LEN + name_len;
    }
    if at != dir.len() {
        return Err(invalid(
            "the project package's central directory has bytes past its last record",
        ));
    }
    Ok(())
}
