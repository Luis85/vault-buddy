//! A minimal ISO-BMFF (MP4) top-level box scanner.
//!
//! This exists for one question the fragmented-MP4 spike (spec §6.4) has to
//! answer with evidence rather than documentation: does the staged file
//! actually contain `moof`/`mdat` fragments, or is it a standard MP4 whose
//! index only appears once the sink is finalized?
//!
//! It is deliberately pure and byte-oriented so the *analysis* half of the
//! spike runs and is tested on Linux, leaving only "produce the bytes" on
//! Windows — the same split the rest of this crate uses (`clock`, `select`
//! pure; `engine` platform-bound).
//!
//! This is NOT a general MP4 parser. It walks only the top level and never
//! recurses, because the fragmentation question is answerable there.

/// One top-level box: its 4-character type and its declared total size.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mp4Box {
    pub kind: String,
    /// Total box size in bytes, header included, as declared in the file.
    pub size: u64,
    /// Byte offset of the box header within the buffer.
    pub offset: u64,
}

/// Why a scan stopped before consuming the whole buffer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScanEnd {
    /// Every byte was accounted for by a well-formed box.
    Complete,
    /// The buffer ends mid-box — exactly what a crashed capture looks like.
    /// Carries the offset where the truncated box begins.
    Truncated { at: u64 },
    /// A box declared a size that cannot be honoured (0 outside the
    /// last-box rule, or smaller than its own header).
    Malformed { at: u64 },
}

/// The result of walking a buffer's top-level boxes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scan {
    pub boxes: Vec<Mp4Box>,
    pub end: ScanEnd,
}

impl Scan {
    /// True when at least one movie-fragment box is present.
    ///
    /// This is the spike's headline assertion: a standard MP4 has none, and a
    /// fragmented MP4 has one per fragment.
    pub fn is_fragmented(&self) -> bool {
        self.boxes.iter().any(|b| b.kind == "moof")
    }

    /// How many fragments were written. A crashed capture keeps every
    /// fragment that closed before the crash.
    pub fn fragment_count(&self) -> usize {
        self.boxes.iter().filter(|b| b.kind == "moof").count()
    }

    /// True when a `moov` (the movie index) is present.
    ///
    /// Note this does NOT distinguish the two very different `moov`s: a
    /// fragmented file writes an *initialization* `moov` up front carrying
    /// empty tracks, whereas a standard MP4 writes a *populated* `moov`
    /// only at finalize. Presence alone therefore proves nothing about
    /// crash-safety — which is why `is_fragmented` is the assertion that
    /// matters and this one is only reported for context.
    pub fn has_moov(&self) -> bool {
        self.boxes.iter().any(|b| b.kind == "moov")
    }

    pub fn kinds(&self) -> Vec<&str> {
        self.boxes.iter().map(|b| b.kind.as_str()).collect()
    }
}

/// Smallest legal box header: 4-byte size + 4-byte type.
const HEADER: u64 = 8;
/// A size of 1 means the real size follows as a 64-bit value.
const LARGE_SIZE_SENTINEL: u32 = 1;

/// Walk the top-level boxes of an ISO-BMFF buffer.
///
/// Tolerant by design: a truncated tail is reported, not an error, because
/// the whole point of the spike is to inspect a file that was cut off
/// mid-write. Whatever boxes completed before the cut are still returned.
pub fn scan(bytes: &[u8]) -> Scan {
    let total = bytes.len() as u64;
    let mut boxes = Vec::new();
    let mut pos: u64 = 0;

    loop {
        let remaining = total.saturating_sub(pos);
        if remaining == 0 {
            return Scan {
                boxes,
                end: ScanEnd::Complete,
            };
        }
        if remaining < HEADER {
            // Not even a header survived — the file was cut inside one.
            return Scan {
                boxes,
                end: ScanEnd::Truncated { at: pos },
            };
        }

        let p = pos as usize;
        let size32 = u32::from_be_bytes([bytes[p], bytes[p + 1], bytes[p + 2], bytes[p + 3]]);
        let kind = match std::str::from_utf8(&bytes[p + 4..p + 8]) {
            Ok(k) if k.chars().all(|c| c.is_ascii_graphic() || c == ' ') => k.to_string(),
            // A non-printable type means we are not looking at a box header
            // at all; treat it as corruption rather than inventing a name.
            _ => {
                return Scan {
                    boxes,
                    end: ScanEnd::Malformed { at: pos },
                }
            }
        };

        let (size, header_len) = if size32 == LARGE_SIZE_SENTINEL {
            if remaining < HEADER + 8 {
                return Scan {
                    boxes,
                    end: ScanEnd::Truncated { at: pos },
                };
            }
            let mut buf = [0u8; 8];
            buf.copy_from_slice(&bytes[p + 8..p + 16]);
            (u64::from_be_bytes(buf), HEADER + 8)
        } else if size32 == 0 {
            // Size 0 is legal ONLY for the final box and means "to EOF".
            // A fragmented file being written can legitimately end this way.
            boxes.push(Mp4Box {
                kind,
                size: remaining,
                offset: pos,
            });
            return Scan {
                boxes,
                end: ScanEnd::Complete,
            };
        } else {
            (size32 as u64, HEADER)
        };

        if size < header_len {
            return Scan {
                boxes,
                end: ScanEnd::Malformed { at: pos },
            };
        }

        boxes.push(Mp4Box {
            kind,
            size,
            offset: pos,
        });

        if size > remaining {
            // The box declares more bytes than the file holds: the capture
            // was cut off inside it. This is the expected shape of a crashed
            // fragmented recording's final fragment.
            return Scan {
                boxes,
                end: ScanEnd::Truncated { at: pos },
            };
        }
        pos += size;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a top-level box with a 32-bit size header.
    fn bx(kind: &str, payload: &[u8]) -> Vec<u8> {
        let size = (HEADER as usize + payload.len()) as u32;
        let mut v = size.to_be_bytes().to_vec();
        v.extend_from_slice(kind.as_bytes());
        v.extend_from_slice(payload);
        v
    }

    #[test]
    fn an_empty_buffer_scans_to_nothing() {
        let s = scan(&[]);
        assert!(s.boxes.is_empty());
        assert_eq!(s.end, ScanEnd::Complete);
        assert!(!s.is_fragmented());
    }

    #[test]
    fn a_standard_mp4_shape_is_not_fragmented() {
        let mut f = bx("ftyp", b"isom");
        f.extend(bx("mdat", &[0u8; 32]));
        f.extend(bx("moov", &[0u8; 16]));
        let s = scan(&f);
        assert_eq!(s.kinds(), vec!["ftyp", "mdat", "moov"]);
        assert_eq!(s.end, ScanEnd::Complete);
        assert!(!s.is_fragmented(), "no moof means not fragmented");
        assert!(s.has_moov());
        assert_eq!(s.fragment_count(), 0);
    }

    #[test]
    fn a_fragmented_mp4_shape_is_detected_and_counted() {
        let mut f = bx("ftyp", b"iso5");
        f.extend(bx("moov", &[0u8; 16])); // init moov, empty tracks
        for _ in 0..3 {
            f.extend(bx("moof", &[0u8; 24]));
            f.extend(bx("mdat", &[0u8; 64]));
        }
        let s = scan(&f);
        assert!(s.is_fragmented());
        assert_eq!(s.fragment_count(), 3, "one moof per fragment");
        assert_eq!(s.end, ScanEnd::Complete);
    }

    // Regression: this is the crashed-capture case the whole fMP4 decision
    // exists for. A file cut off mid-fragment must still report every
    // fragment that closed before the cut, rather than failing the scan and
    // losing the recording.
    #[test]
    fn a_truncated_tail_keeps_the_fragments_that_closed() {
        let mut f = bx("ftyp", b"iso5");
        f.extend(bx("moov", &[0u8; 16]));
        f.extend(bx("moof", &[0u8; 24]));
        f.extend(bx("mdat", &[0u8; 64]));
        let complete_len = f.len();
        // A fourth box that claims 4096 bytes but only 10 arrive.
        f.extend_from_slice(&4096u32.to_be_bytes());
        f.extend_from_slice(b"moof");
        f.extend_from_slice(&[0u8; 2]);

        let s = scan(&f);
        assert_eq!(
            s.end,
            ScanEnd::Truncated {
                at: complete_len as u64
            }
        );
        assert!(s.is_fragmented());
        assert_eq!(
            s.fragment_count(),
            2,
            "the closed fragment plus the truncated one are both seen"
        );
    }

    // Regression: a header split across the cut must not panic or index out
    // of bounds. A crash can land anywhere, including inside the 8 header
    // bytes.
    #[test]
    fn a_cut_inside_a_box_header_is_truncated_not_a_panic() {
        let mut f = bx("ftyp", b"iso5");
        let at = f.len();
        f.extend_from_slice(&[0u8, 0u8, 0u8]); // 3 bytes: less than a header
        let s = scan(&f);
        assert_eq!(s.end, ScanEnd::Truncated { at: at as u64 });
        assert_eq!(s.kinds(), vec!["ftyp"]);
    }

    #[test]
    fn a_64_bit_large_size_box_is_followed() {
        let payload = [0u8; 8];
        let mut f = Vec::new();
        f.extend_from_slice(&LARGE_SIZE_SENTINEL.to_be_bytes());
        f.extend_from_slice(b"mdat");
        f.extend_from_slice(&(24u64).to_be_bytes()); // 16 header + 8 payload
        f.extend_from_slice(&payload);
        f.extend(bx("moof", &[0u8; 8]));
        let s = scan(&f);
        assert_eq!(s.kinds(), vec!["mdat", "moof"]);
        assert_eq!(s.boxes[0].size, 24);
        assert_eq!(s.end, ScanEnd::Complete);
    }

    // Regression: a size of 0 means "runs to EOF" and is legal only as the
    // last box. Treating it as a zero-length box would spin forever.
    #[test]
    fn a_zero_size_final_box_runs_to_eof_and_terminates() {
        let mut f = bx("ftyp", b"iso5");
        f.extend_from_slice(&0u32.to_be_bytes());
        f.extend_from_slice(b"mdat");
        f.extend_from_slice(&[0u8; 40]);
        let s = scan(&f);
        assert_eq!(s.kinds(), vec!["ftyp", "mdat"]);
        assert_eq!(s.boxes[1].size, 48, "8 header + 40 payload, to EOF");
        assert_eq!(s.end, ScanEnd::Complete);
    }

    // Regression: a declared size smaller than the header would make `pos`
    // stand still and loop forever.
    #[test]
    fn an_impossibly_small_declared_size_is_malformed_not_an_infinite_loop() {
        let mut f = Vec::new();
        f.extend_from_slice(&3u32.to_be_bytes()); // smaller than the 8-byte header
        f.extend_from_slice(b"junk");
        f.extend_from_slice(&[0u8; 16]);
        let s = scan(&f);
        assert_eq!(s.end, ScanEnd::Malformed { at: 0 });
    }

    #[test]
    fn a_non_printable_box_type_is_malformed() {
        let mut f = bx("ftyp", b"iso5");
        let at = f.len();
        f.extend_from_slice(&16u32.to_be_bytes());
        f.extend_from_slice(&[0x00, 0x01, 0x02, 0x03]);
        f.extend_from_slice(&[0u8; 8]);
        let s = scan(&f);
        assert_eq!(s.end, ScanEnd::Malformed { at: at as u64 });
    }
}
