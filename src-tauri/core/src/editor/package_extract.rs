//! Extraction-time checks on an already-inspected package (Task 39,
//! controller carry from Task 38): the `zip` crate finds an entry's bytes
//! through the central directory's local-header OFFSET and then trusts
//! whatever LOCAL header it lands on -- it never compares the local
//! header's name with the directory's, never checks that the entry ends
//! before the directory starts, and never notices two entries sharing
//! bytes. `package::inspect_archive` validated the DIRECTORY; this proves
//! the bytes the import actually copies are the entry the directory names.
//!
//! Every function takes the archive's raw reader (`raw`) beside the
//! `ZipArchive` over the same bytes: the crate keeps its reader private, so
//! the local header is read independently. Pure -- no filesystem.

use std::io::{Read, Seek, SeekFrom, Write};
use std::ops::Range;

use zip::ZipArchive;

use super::error::{EditorError, EditorErrorCode};
use super::package::{extract_entry_bounded, ExtractedEntry};

/// A local file header's fixed part (APPNOTE 4.3.7).
const LOCAL_HEADER_LEN: u64 = 30;
const LOCAL_SIGNATURE: [u8; 4] = [0x50, 0x4b, 0x03, 0x04];
/// `validate_entry_name`'s longest name.
const MAX_NAME_BYTES: u64 = 255;

fn invalid(message: impl Into<String>) -> EditorError {
    EditorError::new(EditorErrorCode::InvalidProject, message)
}

/// The byte range entry `index` occupies, local header through its last
/// data byte, after proving its local header is the one the directory
/// describes.
fn entry_span<R: Read + Seek, S: Read + Seek>(
    raw: &mut S,
    archive: &mut ZipArchive<R>,
    index: usize,
) -> Result<Range<u64>, EditorError> {
    let directory = archive.central_directory_start();
    let (name, header, data_start, compressed) = {
        let file = archive
            .by_index_raw(index)
            .map_err(|e| invalid(format!("package entry {index} is unreadable: {e}")))?;
        (
            file.name_raw().to_vec(),
            file.header_start(),
            file.data_start(),
            file.compressed_size(),
        )
    };
    let shown = String::from_utf8_lossy(&name).into_owned();
    let mismatch = || {
        invalid(format!(
            "package entry {shown:?}: its local header does not match the central directory"
        ))
    };
    let mut fixed = [0u8; LOCAL_HEADER_LEN as usize];
    raw.seek(SeekFrom::Start(header))
        .and_then(|_| raw.read_exact(&mut fixed))
        .map_err(|_| mismatch())?;
    let name_len = u64::from(u16::from_le_bytes([fixed[26], fixed[27]]));
    let extra_len = u64::from(u16::from_le_bytes([fixed[28], fixed[29]]));
    if fixed[..4] != LOCAL_SIGNATURE || name_len != name.len() as u64 || name_len > MAX_NAME_BYTES {
        return Err(mismatch());
    }
    let mut local_name = vec![0u8; name.len()];
    raw.read_exact(&mut local_name).map_err(|_| mismatch())?;
    if local_name != name || data_start != header + LOCAL_HEADER_LEN + name_len + extra_len {
        return Err(mismatch());
    }
    let end = data_start.saturating_add(compressed);
    if end > directory {
        return Err(invalid(format!(
            "package entry {shown:?} runs into the central directory"
        )));
    }
    Ok(header..end)
}

/// Every entry's local header checked, and no two entries sharing a byte.
pub fn verify_entry_layout<R: Read + Seek, S: Read + Seek>(
    raw: &mut S,
    archive: &mut ZipArchive<R>,
) -> Result<(), EditorError> {
    let mut spans = Vec::with_capacity(archive.len());
    for index in 0..archive.len() {
        spans.push(entry_span(raw, archive, index)?);
    }
    spans.sort_by_key(|span| span.start);
    if spans.windows(2).any(|pair| pair[0].end > pair[1].start) {
        return Err(invalid(
            "two entries of the project package share bytes (an overlapping archive)",
        ));
    }
    Ok(())
}

/// `extract_entry_bounded`, after checking THIS entry's local header right
/// before its bytes are read.
pub fn extract_verified_entry<R: Read + Seek, S: Read + Seek, W: Write>(
    raw: &mut S,
    archive: &mut ZipArchive<R>,
    name: &str,
    dest: &mut W,
    max: u64,
) -> Result<ExtractedEntry, EditorError> {
    let index = archive
        .index_for_name(name)
        .ok_or_else(|| invalid(format!("the project package has no entry {name:?}")))?;
    entry_span(raw, archive, index)?;
    extract_entry_bounded(archive, name, dest, max)
}

/// An already-inspected package opened for extraction, holding the raw
/// reader beside the archive so the shell never names a `zip` type (the
/// crate is `core`'s dependency alone, ADR R9). `R` is cloned once: `&File`
/// and `Cursor<&[u8]>` are both cheap handles onto the same bytes.
pub struct PackageExtractor<R: Read + Seek + Clone> {
    raw: R,
    archive: ZipArchive<R>,
}

impl<R: Read + Seek + Clone> PackageExtractor<R> {
    /// Opens the archive and runs `verify_entry_layout` over every entry
    /// before anything is extracted.
    pub fn open(reader: R) -> Result<Self, EditorError> {
        let archive = ZipArchive::new(reader.clone()).map_err(|e| {
            invalid(format!(
                "the project package is not a readable archive: {e}"
            ))
        })?;
        let mut me = Self {
            raw: reader,
            archive,
        };
        verify_entry_layout(&mut me.raw, &mut me.archive)?;
        Ok(me)
    }

    /// `extract_verified_entry` on this archive.
    pub fn extract<W: Write>(
        &mut self,
        name: &str,
        dest: &mut W,
        max: u64,
    ) -> Result<ExtractedEntry, EditorError> {
        extract_verified_entry(&mut self.raw, &mut self.archive, name, dest, max)
    }
}

#[cfg(test)]
#[path = "package_extract_tests.rs"]
mod tests;
