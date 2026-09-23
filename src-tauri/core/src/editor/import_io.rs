//! The pure halves of the shell's media import (Task 25, F-02): streaming a
//! source into its owned copy while hashing it, and deciding what a probe
//! result actually IS once the bytes have spoken. No Tauri types and no
//! ffprobe call — the shell (`src-tauri/src/editor/media_import.rs`) owns
//! the file system, the prober and the session; this owns the rules, so
//! they are tested on every platform.

use std::io::{self, Read, Write};

use sha2::{Digest, Sha256};

use super::limits;
use super::probe::{ImportKind, ProbeFacts};

/// Copy `reader` into `writer`, hashing every byte on the way through, and
/// return `(bytes copied, lowercase hex SHA-256)`. One pass over the source:
/// the import copies the file anyway, and reading a multi-gigabyte original
/// a second time just to hash it would double the slowest step. Any read or
/// write error is returned as-is — the caller owns the half-written copy
/// and is the one that must remove it.
pub fn copy_hashing(reader: &mut dyn Read, writer: &mut dyn Write) -> io::Result<(u64, String)> {
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 256 * 1024];
    let mut total = 0u64;
    loop {
        let n = match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        writer.write_all(&buf[..n])?;
        hasher.update(&buf[..n]);
        total += n as u64;
    }
    writer.flush()?;
    Ok((total, hex(&hasher.finalize())))
}

/// The digest `hashing_reader` accumulates, readable after the reader
/// itself has been handed away (Task 39 fix round 1: `write_package`
/// consumes each media reader, and the export must still compare what it
/// actually WROTE with the manifest it wrote first).
pub struct StreamDigest(std::rc::Rc<std::cell::RefCell<(Sha256, u64)>>);

/// A reader that hashes every byte read through it.
pub struct HashingReader<R> {
    inner: R,
    state: std::rc::Rc<std::cell::RefCell<(Sha256, u64)>>,
}

pub fn hashing_reader<R: Read>(inner: R) -> (HashingReader<R>, StreamDigest) {
    let state = std::rc::Rc::new(std::cell::RefCell::new((Sha256::new(), 0)));
    (
        HashingReader {
            inner,
            state: state.clone(),
        },
        StreamDigest(state),
    )
}

impl<R: Read> Read for HashingReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.inner.read(buf)?;
        let mut state = self.state.borrow_mut();
        state.0.update(&buf[..n]);
        state.1 += n as u64;
        Ok(n)
    }
}

impl StreamDigest {
    /// `(bytes read so far, lowercase hex SHA-256 of them)`.
    pub fn finish(&self) -> (u64, String) {
        let state = self.0.borrow();
        (state.1, hex(&state.0.clone().finalize()))
    }
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        write!(out, "{b:02x}").expect("writing to a String is infallible");
    }
    out
}

/// The asset name an imported file gets: its own file name, cut to
/// `limits::MAX_NAME_CHARS` characters (never bytes — a multi-byte name
/// must not be split mid-character). A longer name would make the WHOLE
/// batch's `AddAssets` fail validation, costing every other file.
pub fn display_name(file_name: &str) -> String {
    file_name.chars().take(limits::MAX_NAME_CHARS).collect()
}

/// Decide what a probed audio/video file really is (the bytes, not the
/// name, have the last word — `classify_extension` only chose the prober):
///
/// - a `.mp4` with no video stream but real audio imports as AUDIO rather
///   than being refused;
/// - an audio file whose container also reports a video stream (an MP3's
///   embedded cover art is a one-frame `mjpeg` stream to ffprobe) stays
///   AUDIO, and its picture's dimensions are dropped rather than becoming
///   an audio asset's width/height;
/// - a file with neither, a zero length (a truncated or corrupt file
///   ffprobe could only partly read), or a length over
///   `limits::MAX_DURATION_MS` is refused here, per file, instead of failing
///   the whole batch's `AddAssets` validation later.
///
/// `Image` is not something this function settles (images are sniffed,
/// never probed), so it is refused rather than guessed at.
pub fn settle_av_import(
    declared: ImportKind,
    facts: ProbeFacts,
) -> Result<(ImportKind, ProbeFacts), String> {
    let kind = match declared {
        ImportKind::Video if facts.has_video => ImportKind::Video,
        ImportKind::Video | ImportKind::Audio if facts.has_audio => ImportKind::Audio,
        ImportKind::Image => return Err("An image is not probed as video or audio.".to_string()),
        _ => return Err("The file has no playable video or audio stream.".to_string()),
    };
    if facts.duration_ms == 0 {
        return Err("The file has no measurable length; it may be damaged.".to_string());
    }
    if facts.duration_ms > limits::MAX_DURATION_MS {
        return Err(format!(
            "The file is longer than the {} minute limit.",
            limits::MAX_DURATION_MS / 60_000
        ));
    }
    let settled = match kind {
        ImportKind::Audio => ProbeFacts {
            width: None,
            height: None,
            has_video: false,
            ..facts
        },
        _ => facts,
    };
    Ok((kind, settled))
}

#[cfg(test)]
mod tests {
    // Fix round 1: the digest covers exactly the bytes that went through
    // the reader, however the consumer chunked its reads.
    #[test]
    fn a_hashing_reader_digests_what_was_read() {
        let bytes: Vec<u8> = (0..70_000u32).map(|i| (i % 251) as u8).collect();
        let (mut reader, digest) = hashing_reader(&bytes[..]);
        let mut sink = Vec::new();
        let mut small = [0u8; 777];
        loop {
            let n = reader.read(&mut small).unwrap();
            if n == 0 {
                break;
            }
            sink.extend_from_slice(&small[..n]);
        }
        let expected = copy_hashing(&mut &bytes[..], &mut io::sink()).unwrap();
        assert_eq!(digest.finish(), expected);
        assert_eq!(sink, bytes);
    }

    use super::*;

    fn facts(video: bool, audio: bool, duration_ms: u64) -> ProbeFacts {
        ProbeFacts {
            duration_ms,
            width: video.then_some(1280),
            height: video.then_some(720),
            has_video: video,
            has_audio: audio,
        }
    }

    struct FailAfter(usize);
    impl Read for FailAfter {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            if self.0 == 0 {
                return Err(io::Error::other("disk went away"));
            }
            let n = self.0.min(buf.len()).min(3);
            buf[..n].fill(7);
            self.0 -= n;
            Ok(n)
        }
    }

    #[test]
    fn copy_hashing_copies_every_byte_and_hashes_them() {
        let mut out = Vec::new();
        let (n, sha) = copy_hashing(&mut &b"abc"[..], &mut out).unwrap();
        assert_eq!(n, 3);
        assert_eq!(out, b"abc");
        // The published SHA-256 of "abc" (FIPS 180-2 appendix B.1).
        assert_eq!(
            sha,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn copy_hashing_reports_a_read_failure_instead_of_a_short_success() {
        let mut out = Vec::new();
        let err = copy_hashing(&mut FailAfter(5), &mut out).unwrap_err();
        assert_eq!(err.to_string(), "disk went away");
    }

    #[test]
    fn display_name_is_cut_by_characters_not_bytes() {
        let long: String = "é".repeat(limits::MAX_NAME_CHARS + 5);
        let cut = display_name(&long);
        assert_eq!(cut.chars().count(), limits::MAX_NAME_CHARS);
        assert_eq!(display_name("clip.mp4"), "clip.mp4");
    }

    #[test]
    fn a_video_container_with_only_audio_imports_as_audio() {
        let (kind, settled) =
            settle_av_import(ImportKind::Video, facts(false, true, 4_000)).unwrap();
        assert_eq!(kind, ImportKind::Audio);
        assert!(!settled.has_video);
    }

    #[test]
    fn cover_art_does_not_turn_an_audio_file_into_video() {
        let (kind, settled) =
            settle_av_import(ImportKind::Audio, facts(true, true, 4_000)).unwrap();
        assert_eq!(kind, ImportKind::Audio);
        assert_eq!((settled.width, settled.height), (None, None));
        assert!(!settled.has_video && settled.has_audio);
    }

    #[test]
    fn a_real_video_keeps_its_dimensions() {
        let (kind, settled) =
            settle_av_import(ImportKind::Video, facts(true, false, 9_000)).unwrap();
        assert_eq!(kind, ImportKind::Video);
        assert_eq!((settled.width, settled.height), (Some(1280), Some(720)));
        assert_eq!(settled.duration_ms, 9_000);
    }

    #[test]
    fn unplayable_empty_and_overlong_files_are_refused() {
        assert!(settle_av_import(ImportKind::Video, facts(false, false, 4_000)).is_err());
        assert!(settle_av_import(ImportKind::Audio, facts(true, false, 4_000)).is_err());
        assert!(settle_av_import(ImportKind::Video, facts(true, true, 0)).is_err());
        assert!(settle_av_import(
            ImportKind::Video,
            facts(true, true, limits::MAX_DURATION_MS + 1)
        )
        .is_err());
        assert!(settle_av_import(
            ImportKind::Video,
            facts(true, true, limits::MAX_DURATION_MS)
        )
        .is_ok());
        assert!(settle_av_import(ImportKind::Image, facts(true, true, 4_000)).is_err());
    }
}
