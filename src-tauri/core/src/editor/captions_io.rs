//! SRT and plain-WebVTT parsing and export (Task 36; F-34, F-35; ADR §3.2's
//! `captions_io.rs`). Pure text in, cues out -- no file I/O, no project
//! graph: the shell reads the file the native dialog granted
//! (`caption_commands.rs`), and `commands::captions` maps what this module
//! returns onto a clip.
//!
//! **Every time here is whatever the FILE says**, which for an import is
//! OUTPUT time relative to the clip's own start (the reference editor's
//! "imported timing begins at 00:00 of the visible clip") and for an
//! export is OUTPUT time on the whole timeline. This module never knows
//! which: converting either to a clip's SOURCE time is
//! `commands::captions::plan_caption_import`'s job, one level up.
//!
//! **Bounded, refused whole, never truncated** (R20). A file over
//! `limits::MAX_CAPTION_FILE_BYTES`, a 2001st cue, a cue text over
//! `MAX_CAPTION_TEXT_CHARS` characters or a time past `MAX_DURATION_MS` is
//! an error -- a shortened import is a caption track the user believes
//! arrived whole. Every error names its 1-based LINE so the user can find
//! it in their own editor (`line: 0` = the whole file); the message quotes
//! at most a short token of the file's own content and never a path.
//!
//! **Plain WebVTT only**: the header, optional cue identifiers and
//! `-->` timing lines (any cue settings after the end time are ignored);
//! `NOTE`, `STYLE` and `REGION` blocks are skipped; inline tags (`<b>`,
//! `<v Speaker>`, `<c.class>`, `<00:00:01.000>`) are stripped to their text
//! and the five common entities decoded. SRT gets the same tag stripping,
//! since SRT authoring tools write `<i>`/`<font>` freely.

use std::fmt;

use super::limits;

/// One cue as the file states it: `[start_ms, end_ms)` in the file's own
/// time base (see the module doc), text already stripped to plain lines
/// joined by `\n`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedCue {
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
}

/// Why a subtitle file was refused. `line` is 1-based; `0` means the file
/// as a whole (too big, no cues, too many cues).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptionParseError {
    pub line: usize,
    pub message: String,
}

impl fmt::Display for CaptionParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.line == 0 {
            write!(f, "{}", self.message)
        } else {
            write!(f, "Line {}: {}", self.line, self.message)
        }
    }
}

fn fail<T>(line: usize, message: impl Into<String>) -> Result<T, CaptionParseError> {
    Err(CaptionParseError {
        line,
        message: message.into(),
    })
}

/// A token of the file quoted back in an error, cut short so a hostile
/// file cannot turn one error message into megabytes.
fn quoted(token: &str) -> String {
    let short: String = token.chars().take(32).collect();
    format!("{short:?}")
}

/// A run of consecutive non-blank lines, each with its 1-based line number.
type Block<'a> = Vec<(usize, &'a str)>;

/// Size check, BOM strip, then the file split into blank-line-separated
/// blocks. A line ends at `\n`, with any `\r` before it dropped -- so a
/// Windows (`\r\n`) file numbers its lines exactly as an editor shows
/// them -- and a line holding only whitespace counts as blank, so a
/// hand-edited file with trailing spaces splits the same way.
fn blocks_of(text: &str) -> Result<Vec<Block<'_>>, CaptionParseError> {
    if text.len() as u64 > limits::MAX_CAPTION_FILE_BYTES {
        return fail(0, "Subtitle files are limited to 2 MiB.");
    }
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut blocks = Vec::new();
    let mut current: Block<'_> = Vec::new();
    for (index, line) in text.split('\n').enumerate() {
        if line.trim().is_empty() {
            if !current.is_empty() {
                blocks.push(std::mem::take(&mut current));
            }
        } else {
            current.push((index + 1, line.trim_end()));
        }
    }
    if !current.is_empty() {
        blocks.push(current);
    }
    Ok(blocks)
}

/// `[H+:]MM:SS[,.]mmm` -> milliseconds. Minutes and seconds are exactly two
/// digits below 60, milliseconds exactly three; hours are optional (WebVTT
/// omits them under an hour) and at most three digits. Comma and dot are
/// both accepted, since tools mix the two conventions freely.
fn parse_timestamp(token: &str) -> Option<u64> {
    let (clock, millis) = token.rsplit_once([',', '.'])?;
    if millis.len() != 3 || !millis.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let parts: Vec<&str> = clock.split(':').collect();
    let (hours, minutes, seconds) = match parts.as_slice() {
        [m, s] => ("0", *m, *s),
        [h, m, s] if (1..=3).contains(&h.len()) => (*h, *m, *s),
        _ => return None,
    };
    let two_digits = |v: &str| v.len() == 2 && v.bytes().all(|b| b.is_ascii_digit());
    if !two_digits(minutes) || !two_digits(seconds) || !hours.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let minutes: u64 = minutes.parse().ok()?;
    let seconds: u64 = seconds.parse().ok()?;
    if minutes > 59 || seconds > 59 {
        return None;
    }
    let hours: u64 = hours.parse().ok()?;
    let millis: u64 = millis.parse().ok()?;
    Some(((hours * 60 + minutes) * 60 + seconds) * 1_000 + millis)
}

/// `start --> end [settings]` on line `line`.
fn parse_timing(line: usize, value: &str) -> Result<(u64, u64), CaptionParseError> {
    let (left, right) = value.split_once("-->").ok_or_else(|| CaptionParseError {
        line,
        message: "Expected a timing line such as 00:00:01,000 --> 00:00:02,000.".into(),
    })?;
    let start_token = left.trim();
    let end_token = right.split_whitespace().next().unwrap_or("");
    let start = parse_timestamp(start_token).ok_or_else(|| bad_timestamp(line, start_token))?;
    let end = parse_timestamp(end_token).ok_or_else(|| bad_timestamp(line, end_token))?;
    if end <= start {
        return fail(line, "A cue must end after it starts.");
    }
    if end > limits::MAX_DURATION_MS {
        return fail(line, "Subtitle times must not go past 2 hours.");
    }
    Ok((start, end))
}

fn bad_timestamp(line: usize, token: &str) -> CaptionParseError {
    CaptionParseError {
        line,
        message: format!("Invalid timestamp {}.", quoted(token)),
    }
}

/// Removes every `<...>` tag, then decodes the five common entities --
/// `&amp;` LAST, so `&amp;lt;` stays the literal text `&lt;`.
fn strip_markup(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut in_tag = false;
    for ch in line.chars() {
        match ch {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&nbsp;", " ")
        .replace("&quot;", "\"")
        .replace("&amp;", "&")
}

/// The cue text after the timing line: each line stripped and trimmed,
/// blank results dropped, joined by `\n`, bounded in characters.
fn cue_text(timing_line: usize, lines: &[(usize, &str)]) -> Result<String, CaptionParseError> {
    let text = lines
        .iter()
        .map(|(_, l)| strip_markup(l).trim().to_string())
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    if text.is_empty() {
        return fail(timing_line, "This cue has no text.");
    }
    if text.chars().count() > limits::MAX_CAPTION_TEXT_CHARS {
        return fail(
            timing_line,
            format!(
                "Cue text is limited to {} characters.",
                limits::MAX_CAPTION_TEXT_CHARS
            ),
        );
    }
    Ok(text)
}

/// One cue block: an optional identifier line, the timing line, the text.
fn parse_cue_block(block: &[(usize, &str)]) -> Result<ParsedCue, CaptionParseError> {
    let timing_at = if block[0].1.contains("-->") {
        0
    } else if block.len() > 1 && block[1].1.contains("-->") {
        1
    } else {
        return fail(
            block[0].0,
            "Expected a timing line such as 00:00:01,000 --> 00:00:02,000.",
        );
    };
    let (line, value) = block[timing_at];
    let (start_ms, end_ms) = parse_timing(line, value)?;
    let text = cue_text(line, &block[timing_at + 1..])?;
    Ok(ParsedCue {
        start_ms,
        end_ms,
        text,
    })
}

/// Parses every cue block in order, refusing a 2001st cue at its own line
/// and an input with none at all.
fn parse_blocks(blocks: &[Block<'_>]) -> Result<Vec<ParsedCue>, CaptionParseError> {
    let mut cues = Vec::new();
    for block in blocks {
        if cues.len() == limits::MAX_CAPTIONS {
            return fail(
                block[0].0,
                format!(
                    "A subtitle file may hold at most {} cues.",
                    limits::MAX_CAPTIONS
                ),
            );
        }
        cues.push(parse_cue_block(block)?);
    }
    if cues.is_empty() {
        return fail(0, "The file holds no subtitle cues.");
    }
    Ok(cues)
}

/// SubRip (`.srt`): blocks of an index line, a timing line and text.
pub fn parse_srt(text: &str) -> Result<Vec<ParsedCue>, CaptionParseError> {
    parse_blocks(&blocks_of(text)?)
}

/// `true` when `first` is a WebVTT header line: `WEBVTT` alone or followed
/// by a space or tab and a title.
fn is_vtt_header(first: &str) -> bool {
    match first.strip_prefix("WEBVTT") {
        Some(rest) => rest.is_empty() || rest.starts_with([' ', '\t']),
        None => false,
    }
}

/// `true` for a block whose first line opens a `NOTE`/`STYLE`/`REGION`.
fn is_vtt_metadata(first: &str) -> bool {
    ["NOTE", "STYLE", "REGION"].iter().any(|word| {
        first
            .strip_prefix(word)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with([' ', '\t']))
    })
}

/// Plain WebVTT (`.vtt`) -- see the module doc for what "plain" covers.
pub fn parse_vtt(text: &str) -> Result<Vec<ParsedCue>, CaptionParseError> {
    let blocks = blocks_of(text)?;
    let header = blocks.first().map(|b| b[0]);
    if !header.is_some_and(|(line, value)| line == 1 && is_vtt_header(value)) {
        return fail(1, "A WebVTT file must begin with WEBVTT.");
    }
    let cues: Vec<Block<'_>> = blocks
        .into_iter()
        .skip(1)
        .filter(|b| !is_vtt_metadata(b[0].1))
        .collect();
    parse_blocks(&cues)
}

/// Either format, decided by the header: a `.txt` names no format, and a
/// `.srt` holding WebVTT (or the reverse) is common enough to forgive.
pub fn parse_subtitles(text: &str) -> Result<Vec<ParsedCue>, CaptionParseError> {
    let body = text.strip_prefix('\u{feff}').unwrap_or(text);
    if body.starts_with("WEBVTT") {
        parse_vtt(text)
    } else {
        parse_srt(text)
    }
}

fn timestamp(ms: u64, separator: char) -> String {
    format!(
        "{:02}:{:02}:{:02}{separator}{:03}",
        ms / 3_600_000,
        ms / 60_000 % 60,
        ms / 1_000 % 60,
        ms % 1_000
    )
}

/// `HH:MM:SS,mmm` -- SubRip's own spelling.
fn srt_timestamp(ms: u64) -> String {
    timestamp(ms, ',')
}

/// `HH:MM:SS.mmm` -- WebVTT's.
fn vtt_timestamp(ms: u64) -> String {
    timestamp(ms, '.')
}

/// Non-blank lines only: a blank line inside a cue would end its block in
/// every reader.
fn export_lines(text: &str) -> impl Iterator<Item = &str> {
    text.split('\n').map(str::trim).filter(|l| !l.is_empty())
}

fn export_blocks(
    cues: &[ParsedCue],
    stamp: fn(u64) -> String,
    escape: fn(&str) -> String,
) -> String {
    cues.iter()
        .enumerate()
        .map(|(i, cue)| {
            let text = export_lines(&cue.text)
                .map(escape)
                .collect::<Vec<_>>()
                .join("\n");
            format!(
                "{}\n{} --> {}\n{text}\n",
                i + 1,
                stamp(cue.start_ms),
                stamp(cue.end_ms)
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// SubRip text for cues already in OUTPUT time (Task 48's
/// `editor_export_subtitles` maps them there first).
pub fn export_srt(cues_in_output_time: &[ParsedCue]) -> String {
    export_blocks(cues_in_output_time, srt_timestamp, str::to_string)
}

/// WebVTT text for cues already in OUTPUT time. `&`, `<` and `>` are
/// escaped -- WebVTT reads them as markup, and an escaped `>` also means a
/// cue's text can never contain a literal `-->`.
pub fn export_vtt(cues_in_output_time: &[ParsedCue]) -> String {
    fn escape(line: &str) -> String {
        line.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
    }
    format!(
        "WEBVTT\n\n{}",
        export_blocks(cues_in_output_time, vtt_timestamp, escape)
    )
}

#[cfg(test)]
#[path = "captions_io_tests.rs"]
mod tests;
