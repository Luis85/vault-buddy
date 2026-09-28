//! Reconnecting missing originals (Task 40; F-03; A19; SCREENS 07): which
//! of the files a user picked IS which missing source, decided from facts
//! alone. Pure and Tauri-free (pre-flight ruling F32) — the shell's
//! `relink_commands.rs` opens the dialog, hashes and probes each picked
//! file, and hands the facts here.
//!
//! **The rules, in order** (the brief, verbatim in spirit):
//! - A source whose record carries a SHA-256 is identified by that hash and
//!   nothing else: an exact hash match wins, and a file with the same size
//!   and length but different bytes is NOT the original (it is reported as
//!   a mismatch, "different content"). A file's NAME is never evidence —
//!   `CandidateFacts::name` exists only so a report can say which file it
//!   means.
//! - A source with no hash (a staged capture, a lightweight project's
//!   placeholder) needs size AND duration (within `DURATION_TOLERANCE_MS`)
//!   AND kind to agree.
//! - Two or more equally good candidates are AMBIGUOUS and never selected:
//!   the user chooses (A19: "the UI reports ambiguity and preserves the edit
//!   until an explicit valid match is chosen").
//! - A candidate that identifies no source is MISMATCHED, with a reason,
//!   but only where the pairing is not a guess: when exactly one source in
//!   the request is left unresolved, every candidate nothing claimed is
//!   reported against it. With several unresolved sources and several stray
//!   files there is no honest pairing, so those sources are simply
//!   `unmatched` and the stray files are reported `unused`.
//! - A19 holds both ways (fix round 1): one file claimed by two sources,
//!   where either claim rests on size + length rather than a hash, is
//!   ambiguous for BOTH. Two hashed sources claiming one file record the
//!   same bytes (one original imported twice) and both reconnect.
//!
//! `RelinkReportDto` is the wire reply of `editor_relink_media` (ADR §3.3's
//! `RelinkReport`): the report with each candidate index turned into the
//! file's display name, the projection after the edit, and the sources
//! still missing.

use serde::Serialize;

use super::probe::ImportKind;
use super::projection::{EditorProjection, MissingMedia};

/// How far a candidate's probed length may be from the recorded one and
/// still be the same file (containers round differently).
pub const DURATION_TOLERANCE_MS: u64 = 50;

/// What a missing source's `sources.json` record says it was.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpectedSource {
    pub sha256: Option<String>,
    pub size: u64,
    pub duration_ms: u64,
    pub kind: ImportKind,
}

/// What one picked file turned out to be — hashed and probed, never
/// guessed from its name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidateFacts {
    /// Display name only; never compared.
    pub name: String,
    pub sha256: String,
    pub size: u64,
    pub duration_ms: u64,
    pub kind: ImportKind,
}

/// `match_candidates`' verdict, by candidate INDEX.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RelinkReport {
    pub matched: Vec<(String, usize)>,
    pub ambiguous: Vec<(String, Vec<usize>)>,
    pub unmatched: Vec<String>,
    pub mismatched: Vec<(String, usize, String)>,
    /// Picked files no source claimed and none was blamed on.
    pub unused: Vec<usize>,
}

fn same_identity(expected: &ExpectedSource, candidate: &CandidateFacts) -> bool {
    match &expected.sha256 {
        Some(hash) => hash.eq_ignore_ascii_case(&candidate.sha256),
        None => {
            expected.kind == candidate.kind
                && expected.size == candidate.size
                && expected.duration_ms.abs_diff(candidate.duration_ms) <= DURATION_TOLERANCE_MS
        }
    }
}

fn kind_word(kind: ImportKind) -> &'static str {
    match kind {
        ImportKind::Video => "video",
        ImportKind::Audio => "audio",
        ImportKind::Image => "image",
    }
}

fn seconds(ms: u64) -> String {
    format!("{:.1} s", ms as f64 / 1000.0)
}

fn sizes(chosen: u64, expected: u64) -> String {
    let mb = |b: u64| format!("{:.1} MB", b as f64 / 1_000_000.0);
    if mb(chosen) != mb(expected) {
        format!("{} vs {}", mb(chosen), mb(expected))
    } else {
        format!("{chosen} bytes vs {expected} bytes")
    }
}

/// Why `candidate` is not `expected`, the chosen file's value first, then
/// the original's: `"different duration: 12.4 s vs 31.0 s"`. The first
/// difference in the order kind, duration, size, content.
pub fn mismatch_reason(expected: &ExpectedSource, candidate: &CandidateFacts) -> String {
    if expected.kind != candidate.kind {
        return format!(
            "different kind: {} vs {}",
            kind_word(candidate.kind),
            kind_word(expected.kind)
        );
    }
    if expected.duration_ms.abs_diff(candidate.duration_ms) > DURATION_TOLERANCE_MS {
        return format!(
            "different duration: {} vs {}",
            seconds(candidate.duration_ms),
            seconds(expected.duration_ms)
        );
    }
    if expected.size != candidate.size {
        return format!("different size: {}", sizes(candidate.size, expected.size));
    }
    "different content: the file's fingerprint does not match the original's".to_string()
}

/// Match `candidates` to `expected` (module doc for every rule). Every
/// expected source lands in exactly one of `matched`, `ambiguous`,
/// `unmatched` or — as the lone unresolved source — `mismatched`.
pub fn match_candidates(
    expected: &[(String, ExpectedSource)],
    candidates: &[CandidateFacts],
) -> RelinkReport {
    let mut report = RelinkReport::default();
    let hits: Vec<Vec<usize>> = expected
        .iter()
        .map(|(_, source)| {
            (0..candidates.len())
                .filter(|&i| same_identity(source, &candidates[i]))
                .collect()
        })
        .collect();
    // A19 both ways (fix round 1): a file claimed by two sources where any
    // claim rests on size + length alone proves neither.
    let mut claims = vec![0usize; candidates.len()];
    let mut by_shape = vec![false; candidates.len()];
    for ((_, source), mine) in expected.iter().zip(&hits) {
        for &i in mine {
            claims[i] += 1;
            by_shape[i] |= source.sha256.is_none();
        }
    }
    let contested = |i: usize| claims[i] > 1 && by_shape[i];
    let mut unresolved: Vec<&(String, ExpectedSource)> = Vec::new();
    for (entry, mine) in expected.iter().zip(&hits) {
        match mine.as_slice() {
            [] => unresolved.push(entry),
            [one] if !contested(*one) => report.matched.push((entry.0.clone(), *one)),
            many => report.ambiguous.push((entry.0.clone(), many.to_vec())),
        }
    }
    let strays: Vec<usize> = (0..candidates.len()).filter(|&i| claims[i] == 0).collect();
    match (unresolved.as_slice(), strays.is_empty()) {
        ([(asset_id, source)], false) => {
            for i in strays {
                let reason = mismatch_reason(source, &candidates[i]);
                report.mismatched.push((asset_id.clone(), i, reason));
            }
        }
        _ => {
            report
                .unmatched
                .extend(unresolved.iter().map(|(id, _)| id.clone()));
            report.unused = strays;
        }
    }
    report
}

/// A source reconnected to (or replaced by) one picked file.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RelinkedFile {
    pub asset_id: String,
    pub file: String,
}

/// A source two or more picked files match equally well — left untouched.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AmbiguousSource {
    pub asset_id: String,
    pub files: Vec<String>,
}

/// A picked file that is not the source, and why.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MismatchedFile {
    pub asset_id: String,
    pub file: String,
    pub reason: String,
}

/// A source whose matching file could not be copied in, and why — its
/// real cause, never "no file matched" (fix round 1).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RelinkFailure {
    pub asset_id: String,
    pub file: String,
    pub error: String,
}

/// A source a batch left out because it cannot be reconnected here, and
/// why (fix round 1: one such source must not refuse the whole batch).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExcludedSource {
    pub asset_id: String,
    pub reason: String,
}

/// A picked file that could not be examined at all.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RelinkFileProblem {
    pub name: String,
    pub error: String,
}

/// `editor_relink_media`'s reply (ADR §3.3 `RelinkReport`; `null` on the
/// wire when the dialog was dismissed). Every list is always present.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RelinkReportDto {
    pub projection: EditorProjection,
    pub missing: Vec<MissingMedia>,
    pub matched: Vec<RelinkedFile>,
    pub replaced: Vec<RelinkedFile>,
    pub ambiguous: Vec<AmbiguousSource>,
    pub unmatched: Vec<String>,
    pub mismatched: Vec<MismatchedFile>,
    pub failed: Vec<RelinkFailure>,
    pub unused: Vec<String>,
    pub excluded: Vec<ExcludedSource>,
    pub per_file: Vec<RelinkFileProblem>,
}

/// The report's candidate indices as file names — the half of the reply
/// `match_candidates` decides. `replaced` names the ONE confirmed
/// replacement, which the caller has already taken out of `mismatched`.
pub struct NamedReport {
    pub matched: Vec<RelinkedFile>,
    pub ambiguous: Vec<AmbiguousSource>,
    pub unmatched: Vec<String>,
    pub mismatched: Vec<MismatchedFile>,
    pub unused: Vec<String>,
}

impl NamedReport {
    pub fn of(report: &RelinkReport, candidates: &[CandidateFacts]) -> Self {
        let name = |i: usize| candidates[i].name.clone();
        Self {
            matched: report
                .matched
                .iter()
                .map(|(asset_id, i)| RelinkedFile {
                    asset_id: asset_id.clone(),
                    file: name(*i),
                })
                .collect(),
            ambiguous: report
                .ambiguous
                .iter()
                .map(|(asset_id, all)| AmbiguousSource {
                    asset_id: asset_id.clone(),
                    files: all.iter().map(|&i| name(i)).collect(),
                })
                .collect(),
            unmatched: report.unmatched.clone(),
            mismatched: report
                .mismatched
                .iter()
                .map(|(asset_id, i, reason)| MismatchedFile {
                    asset_id: asset_id.clone(),
                    file: name(*i),
                    reason: reason.clone(),
                })
                .collect(),
            unused: report.unused.iter().map(|&i| name(i)).collect(),
        }
    }
}

#[cfg(test)]
#[path = "relink_tests.rs"]
mod tests;
