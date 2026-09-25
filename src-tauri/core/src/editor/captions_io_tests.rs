use super::*;

fn cue(start_ms: u64, end_ms: u64, text: &str) -> ParsedCue {
    ParsedCue {
        start_ms,
        end_ms,
        text: text.to_string(),
    }
}

// ---- the brief's named tests --------------------------------------------

#[test]
fn srt_parses_crlf_and_multiline_cues() {
    // CRLF line endings (what every Windows subtitle tool writes) and a
    // cue whose text spans two lines: both lines survive, joined by one
    // `\n`, and the CR never leaks into the text or the timestamp parse.
    let text = "1\r\n00:00:01,000 --> 00:00:02,500\r\nHello\r\nworld\r\n\r\n\
                2\r\n00:00:03,250 --> 00:00:04,000\r\nSecond\r\n";
    let cues = parse_srt(text).expect("a well-formed CRLF file parses");
    assert_eq!(
        cues,
        vec![
            cue(1_000, 2_500, "Hello\nworld"),
            cue(3_250, 4_000, "Second")
        ]
    );
}

#[test]
fn srt_error_names_the_line() {
    // Line 6 carries the malformed timestamp (a two-digit millisecond
    // field) -- the error must name THAT line, 1-based, so a user can find
    // it in their editor, not the block's first line or a byte offset.
    let text =
        "1\n00:00:01,000 --> 00:00:02,000\nFine\n\n2\n00:00:03,00 --> 00:00:04,000\nBroken\n";
    let err = parse_srt(text).unwrap_err();
    assert_eq!(err.line, 6, "wrong line in {err:?}");
    assert!(
        err.message.contains("timestamp"),
        "the message should say what is wrong: {err:?}"
    );
}

#[test]
fn vtt_skips_style_blocks_and_strips_tags() {
    let text = "WEBVTT - a title\n\n\
                STYLE\n::cue { color: yellow }\n\n\
                REGION\nid:fred width:40%\n\n\
                NOTE this is a comment\n\n\
                intro-id\n00:01.000 --> 00:02.000 align:start position:10%\n<v Roger>Hi <b>there</b> &amp; welcome\n\n\
                01:00:00.500 --> 01:00:01.000\n<c.loud>Later</c> &lt;3\n";
    let cues = parse_vtt(text).expect("plain WebVTT parses");
    assert_eq!(
        cues,
        vec![
            cue(1_000, 2_000, "Hi there & welcome"),
            cue(3_600_500, 3_601_000, "Later <3"),
        ]
    );
}

#[test]
fn bounds_are_enforced() {
    // 2001 cues: one past MAX_CAPTIONS. The parser refuses rather than
    // truncating -- a silently shortened import is a caption file the user
    // believes arrived whole (R20).
    let mut text = String::new();
    for i in 0..2_001u64 {
        let start = i * 1_000;
        text.push_str(&format!(
            "{}\n{} --> {}\nCue {i}\n\n",
            i + 1,
            srt_timestamp(start),
            srt_timestamp(start + 500)
        ));
    }
    let err = parse_srt(&text).unwrap_err();
    assert!(err.message.contains("2000"), "{err:?}");

    // Exactly 2000 is fine -- the bound, not one below it.
    let at_limit: String = text
        .split("\n\n")
        .take(2_000)
        .collect::<Vec<_>>()
        .join("\n\n");
    assert_eq!(parse_srt(&at_limit).unwrap().len(), 2_000);
}

#[test]
fn export_srt_formats_hours_and_commas() {
    let out = export_srt(&[cue(3_723_004, 3_724_000, "One"), cue(5, 900, "Two\nlines")]);
    assert_eq!(
        out,
        "1\n01:02:03,004 --> 01:02:04,000\nOne\n\n2\n00:00:00,005 --> 00:00:00,900\nTwo\nlines\n"
    );
}

// ---- the rest of the bounds and formats -----------------------------------

#[test]
fn text_over_500_characters_is_refused() {
    let long = "a".repeat(501);
    let err = parse_srt(&format!("1\n00:00:01,000 --> 00:00:02,000\n{long}\n")).unwrap_err();
    assert_eq!(err.line, 2);
    assert!(err.message.contains("500"), "{err:?}");
    let ok = "é".repeat(500); // counted in characters, not bytes
    assert!(parse_srt(&format!("1\n00:00:01,000 --> 00:00:02,000\n{ok}\n")).is_ok());
}

#[test]
fn times_past_two_hours_are_refused() {
    let err = parse_srt("1\n01:59:59,000 --> 02:00:00,001\nLate\n").unwrap_err();
    assert_eq!(err.line, 2);
    assert!(parse_srt("1\n01:59:59,000 --> 02:00:00,000\nEdge\n").is_ok());
}

#[test]
fn a_file_over_two_mib_is_refused_before_parsing() {
    let text = format!(
        "1\n00:00:01,000 --> 00:00:02,000\nx\n\n{}",
        " ".repeat(2 * 1024 * 1024)
    );
    let err = parse_srt(&text).unwrap_err();
    assert_eq!(err.line, 0, "a whole-file refusal names no line");
    assert!(err.message.contains("2 MiB"), "{err:?}");
}

#[test]
fn an_end_not_after_its_start_is_refused() {
    let err = parse_srt("1\n00:00:02,000 --> 00:00:02,000\nZero\n").unwrap_err();
    assert_eq!(err.line, 2);
}

#[test]
fn a_block_without_a_timing_line_names_its_first_line() {
    let err = parse_srt("1\n00:00:01,000 --> 00:00:02,000\nOk\n\n2\nno timing here\n").unwrap_err();
    assert_eq!(err.line, 5);
}

#[test]
fn a_cue_with_no_text_is_refused() {
    let err = parse_srt("1\n00:00:01,000 --> 00:00:02,000\n<i></i>\n").unwrap_err();
    assert_eq!(err.line, 2);
}

#[test]
fn an_empty_file_is_refused() {
    assert_eq!(parse_srt("\n\n").unwrap_err().line, 0);
    assert_eq!(parse_vtt("WEBVTT\n\n").unwrap_err().line, 0);
}

#[test]
fn vtt_requires_its_header() {
    let err = parse_vtt("00:01.000 --> 00:02.000\nHi\n").unwrap_err();
    assert_eq!(err.line, 1);
    // `WEBVTTX` is not the header either.
    assert!(parse_vtt("WEBVTTX\n\n00:01.000 --> 00:02.000\nHi\n").is_err());
}

#[test]
fn a_byte_order_mark_is_ignored() {
    let cues = parse_vtt("\u{feff}WEBVTT\n\n00:01.000 --> 00:02.000\nHi\n").unwrap();
    assert_eq!(cues, vec![cue(1_000, 2_000, "Hi")]);
}

#[test]
fn parse_subtitles_sniffs_the_format() {
    // A `.txt` carries no format in its name: the header decides.
    assert_eq!(
        parse_subtitles("WEBVTT\n\n00:01.000 --> 00:02.000\nHi\n").unwrap(),
        vec![cue(1_000, 2_000, "Hi")]
    );
    assert_eq!(
        parse_subtitles("1\n00:00:01,000 --> 00:00:02,000\nHi\n").unwrap(),
        vec![cue(1_000, 2_000, "Hi")]
    );
}

#[test]
fn export_vtt_escapes_markup_and_uses_dots() {
    let out = export_vtt(&[cue(61_000, 62_500, "a < b & c")]);
    assert_eq!(
        out,
        "WEBVTT\n\n1\n00:01:01.000 --> 00:01:02.500\na &lt; b &amp; c\n"
    );
    assert_eq!(
        parse_vtt(&out).unwrap(),
        vec![cue(61_000, 62_500, "a < b & c")]
    );
}

#[test]
fn export_never_emits_a_blank_line_inside_a_cue() {
    // A blank line inside a cue's text would END the block in every reader.
    let out = export_srt(&[cue(0, 1_000, "one\n\n  \ntwo")]);
    assert_eq!(out, "1\n00:00:00,000 --> 00:00:01,000\none\ntwo\n");
}

#[test]
fn srt_round_trips_through_export() {
    let cues = vec![cue(0, 1_000, "A"), cue(3_723_004, 3_724_000, "B\nC")];
    assert_eq!(parse_srt(&export_srt(&cues)).unwrap(), cues);
}

#[test]
fn error_display_names_the_line() {
    let err = CaptionParseError {
        line: 7,
        message: "bad".into(),
    };
    assert_eq!(err.to_string(), "Line 7: bad");
    let whole = CaptionParseError {
        line: 0,
        message: "bad".into(),
    };
    assert_eq!(whole.to_string(), "bad");
}

// ---- fix round 1: markup stripping never deletes caption words -------------
//
// A tutorial caption names keys and compares numbers. Stripping every `<…>`
// silently turned "Press <Ctrl> + <S> to save" into "Press  +  to save",
// and an unclosed `<` swallowed the rest of its line -- a shortened caption
// the user believes arrived whole, exactly what this module refuses to do.
// Only the tags each format actually defines are markup; anything else,
// and an unclosed `<`, is literal text.

#[test]
fn srt_keeps_angle_bracket_words_and_strips_only_srt_tags() {
    let text = "1\n00:00:01,000 --> 00:00:02,000\nPress <Ctrl> + <S> to <i>save</i> <FONT color=\"#ff0\">now</font>\n";
    assert_eq!(
        parse_srt(text).unwrap(),
        vec![cue(1_000, 2_000, "Press <Ctrl> + <S> to save now")]
    );
}

#[test]
fn srt_keeps_an_unclosed_angle_bracket() {
    let text = "1\n00:00:01,000 --> 00:00:02,000\nx < 5 means small\n";
    assert_eq!(
        parse_srt(text).unwrap(),
        vec![cue(1_000, 2_000, "x < 5 means small")]
    );
}

#[test]
fn vtt_keeps_angle_bracket_words_and_strips_only_cue_tags() {
    let text = "WEBVTT\n\n00:01.000 --> 00:02.000\n<v Ann>Press <Ctrl> + <S></v> <00:01.500><lang en>to</lang> <ruby>save<rt>s</rt></ruby>\n";
    assert_eq!(
        parse_vtt(text).unwrap(),
        vec![cue(1_000, 2_000, "Press <Ctrl> + <S> to saves")]
    );
}

#[test]
fn vtt_keeps_an_unclosed_angle_bracket() {
    let text = "WEBVTT\n\n00:01.000 --> 00:02.000\nx < 5 means small\n";
    assert_eq!(
        parse_vtt(text).unwrap(),
        vec![cue(1_000, 2_000, "x < 5 means small")]
    );
}

#[test]
fn srt_does_not_treat_webvtt_voice_tags_as_markup() {
    // `<v …>`/`</v>` are WebVTT's, not SubRip's: in an SRT file they are
    // text. (The bare closing `</v>` is what pins the tag LIST: `<v Ann>`
    // alone is also kept by the "no attributes except on font" rule.)
    let text = "1\n00:00:01,000 --> 00:00:02,000\n<v Ann>Hi</v>\n";
    assert_eq!(
        parse_srt(text).unwrap(),
        vec![cue(1_000, 2_000, "<v Ann>Hi</v>")]
    );
}
