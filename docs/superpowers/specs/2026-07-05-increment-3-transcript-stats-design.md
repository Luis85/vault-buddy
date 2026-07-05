# Increment 3 Polish Design — "Transcript statistics footer"

- **Date:** 2026-07-05
- **Status:** Approved
- **Source:** Follow-up to local speech-to-text. A finished transcript's
  metadata (model, language, duration, generated-at) lives in YAML
  frontmatter, which Obsidian hides in reading view and when the note embeds
  the sidecar — so it's invisible where it's read. Add a stats footer that
  surfaces that metadata and adds a few useful computed figures.

## Goal

Append a **Statistics** table to the end of every completed transcript
(`render_transcript`), covering both the otherwise-hidden metadata and figures
computed from the transcript itself. No change to the transcription engine, the
`complete` marker, or the never-clobber write rules.

## The stats

A GFM table after the transcript segments:

```markdown
## Statistics

| Metric | Value |
| --- | --- |
| Duration | 05:23 |
| Words | 812 |
| Segments | 47 |
| Speaking rate | 151 wpm |
| Model | whisper-small |
| Language | de |
| Processing time | 00:47 |
| Generated | 2026-07-05T14:30:00+00:00 |
```

| Metric | Source | Notes |
| --- | --- | --- |
| Duration | `meta.duration_secs` | via `format_duration` (already used for the frontmatter `duration`) |
| Words | sum of `segment.text.split_whitespace().count()` | over the rendered (non-empty) segments |
| Segments | count of non-empty segments | matches what's actually rendered (empty segments are skipped) |
| Speaking rate | `words × 60 / duration_secs`, integer | shown as `<n> wpm`; **zero-duration guard** → `—` |
| Model | `meta.model_label` | e.g. `whisper-small` |
| Language | `meta.language` or `auto` | the code (or `auto`), matching the frontmatter — core has no code→name map |
| Processing time | `meta.processing_secs` | via `format_duration` |
| Generated | `meta.generated_at` | the injected RFC3339 string, as-is |

## Architecture

### Core (`core/src/transcript.rs`) — pure, Linux-tested

- **`TranscriptMeta` gains `processing_secs: u64`** — a measured value injected
  by the caller, like `duration_secs`/`generated_at`, so `render_transcript`
  stays pure and deterministic.
- **`render_stats(meta: &TranscriptMeta, segments: &[Segment]) -> String`** — a
  small helper that computes words/segments/speaking-rate and formats the
  `## Statistics` table. `render_transcript` appends its output after the
  segment loop (a blank line separates the last segment from the heading).
- Word/segment counts derive from the same non-empty filter the segment loop
  uses, so the reported "Segments" equals the number rendered.

### Transcribe (`transcribe/src/lib.rs`) — measures the wall-clock

- `transcribe_recording` starts a `std::time::Instant` before `decode` and
  reads `elapsed().as_secs()` after `transcriber.transcribe(...)` returns,
  setting `meta.processing_secs`. This covers decode + inference — the real
  processing cost. `generated_at` stays injected, so the deterministic parts of
  the output are unchanged; only the (test-ignored) processing figure varies.

## Placement & rendering

The table goes at the very end, after all segments. Because the sidecar is
embedded in the meeting note, an `## Statistics` H2 gives a clean, visible break
between the transcript text and the figures — and, unlike the frontmatter, it
renders in the note.

## Edge cases

- **Zero duration** (empty/near-silent recording): speaking rate is `—`, not a
  divide-by-zero. Duration/words/segments still render (`00:00`, `0`, `0`).
- **No segments:** the table still renders with zeroes; the transcript body is
  empty above it (already possible today).

## Testing

- **Core (`transcript.rs`):** a `render_transcript` / `render_stats` unit test
  with a known `TranscriptMeta` + segments asserts each row (word count,
  segment count, speaking rate) and the zero-duration `—` guard. Pure, runs on
  Linux.
- **Transcribe (`lib.rs`):** the existing `transcribe_writes_the_sidecar` test
  keeps passing; extend it to assert the `## Statistics` heading and a stats
  row appear (processing time not asserted exactly — a fake transcriber is
  ~instant).

## Invariants preserved

- **`complete` marker + all frontmatter unchanged** — the footer is additive;
  machine-readable metadata stays in YAML, human-readable stats are surfaced
  below.
- **Never-clobber / atomic write** — the content is longer, but the write path
  (`replace_if_ours`) and the regenerable-marker logic are untouched.

## Non-goals / scope guards

- No per-segment confidence (whisper-rs's simple API doesn't expose it cleanly).
- No code→language-name mapping in core (would duplicate the frontend list).
- No new frontmatter fields; no engine/model changes.
