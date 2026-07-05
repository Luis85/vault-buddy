# Transcript Statistics Footer Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Append a `## Statistics` table to every completed transcript, surfacing the frontmatter metadata plus computed word/segment/speaking-rate figures and a measured processing time.

**Architecture:** Pure `core::transcript` renders the table from `TranscriptMeta` + segments; `TranscriptMeta` gains a `processing_secs` field the `transcribe` crate measures with a wall-clock timer. One cohesive change across the two pure crates — both test on Linux.

**Tech Stack:** Rust (`vault_buddy_core`, `vault_buddy_transcribe`), unit tests inline.

## Global Constraints

- **Additive only.** The `complete` marker and all YAML frontmatter stay exactly as they are; the table is appended after the segments.
- **`render_transcript` stays pure and deterministic** — `processing_secs` is an injected value (like `duration_secs`/`generated_at`), not measured inside core.
- **Never-clobber untouched** — no change to `replace_if_ours` or the regenerable-marker logic.
- **Zero-duration guard** — speaking rate is `—` when `duration_secs == 0` (no divide-by-zero).
- **Both crates test on Linux** (`cargo test -p vault_buddy_core -p vault_buddy_transcribe`). Commit: `feat(transcribe)`.

---

### Task 1: Statistics footer

**Files:**
- Modify: `src-tauri/core/src/transcript.rs` (add `processing_secs` field, `render_stats`, append to `render_transcript`, update the `meta()` test helper, add tests)
- Modify: `src-tauri/transcribe/src/lib.rs` (measure `processing_secs`, set it on the `TranscriptMeta`, extend the sidecar test)

**Interfaces:**
- Consumes: existing `format_duration` (imported in `transcript.rs`), `Segment`, `TranscriptMeta`.
- Produces: `TranscriptMeta.processing_secs: u64`; the rendered transcript ends with a `## Statistics` GFM table.

- [ ] **Step 1: Write the failing core tests**

In `src-tauri/core/src/transcript.rs`, inside `#[cfg(test)] mod tests`, add (these use the existing `meta()` / `seg()` helpers, so they compile against the current struct and fail at runtime because no table is rendered yet):

```rust
    #[test]
    fn transcript_ends_with_a_stats_table() {
        // meta(): model "whisper-small", language "es", duration 3723s.
        let t = render_transcript(
            &meta(),
            &[
                seg(0, 1000, "hola a todos"), // 3 words
                seg(1000, 2000, "que tal"),   // 2 words
                seg(2000, 2500, "   "),       // empty → skipped
            ],
        );
        assert!(t.contains("## Statistics"));
        assert!(t.contains("| Words | 5 |"));
        assert!(t.contains("| Segments | 2 |"));
        assert!(t.contains("| Model | whisper-small |"));
        assert!(t.contains("| Language | es |"));
        assert!(t.contains("| Processing time |"));
    }

    #[test]
    fn stats_speaking_rate_computes_and_guards_zero() {
        let mut m = meta();
        m.duration_secs = 60; // 12 words over one minute → 12 wpm
        let t = render_transcript(&m, &[seg(0, 60_000, "a b c d e f g h i j k l")]);
        assert!(t.contains("| Speaking rate | 12 wpm |"));

        let mut z = meta();
        z.duration_secs = 0; // must not divide by zero
        assert!(render_transcript(&z, &[seg(0, 0, "hi there")]).contains("| Speaking rate | — |"));

        let mut a = meta();
        a.language = None; // None renders "auto" in the footer too
        assert!(render_transcript(&a, &[]).contains("| Language | auto |"));
    }
```

- [ ] **Step 2: Run the core tests to verify they fail**

Run (from `src-tauri/`): `cargo test -p vault_buddy_core transcript::`
Expected: FAIL — output has no `## Statistics` table (assertions fail).

- [ ] **Step 3: Implement the core rendering**

In `src-tauri/core/src/transcript.rs`:

(a) Add the field to `TranscriptMeta` (after `timestamps`):

```rust
pub struct TranscriptMeta {
    pub mp3_file_name: String,
    pub model_label: String,
    pub language: Option<String>,
    pub duration_secs: u64,
    pub generated_at: String,
    pub timestamps: bool,
    pub processing_secs: u64,
}
```

(b) Add the `render_stats` helper (place it right after `render_transcript`):

```rust
/// The `## Statistics` footer: metadata that's otherwise hidden in the note's
/// frontmatter embed, plus figures computed from the transcript. Pure — every
/// value comes from `meta`/`segments`, so it's deterministic and unit-tested.
pub fn render_stats(meta: &TranscriptMeta, segments: &[Segment]) -> String {
    let mut words = 0usize;
    let mut segment_count = 0usize;
    for s in segments {
        let t = s.text.trim();
        if t.is_empty() {
            continue;
        }
        segment_count += 1;
        words += t.split_whitespace().count();
    }
    let speaking_rate = if meta.duration_secs > 0 {
        format!("{} wpm", (words as u64 * 60) / meta.duration_secs)
    } else {
        "—".to_string()
    };
    let language = meta.language.as_deref().unwrap_or("auto");
    format!(
        "## Statistics\n\n\
         | Metric | Value |\n\
         | --- | --- |\n\
         | Duration | {} |\n\
         | Words | {words} |\n\
         | Segments | {segment_count} |\n\
         | Speaking rate | {speaking_rate} |\n\
         | Model | {} |\n\
         | Language | {language} |\n\
         | Processing time | {} |\n\
         | Generated | {} |\n",
        format_duration(meta.duration_secs),
        meta.model_label,
        format_duration(meta.processing_secs),
        meta.generated_at,
    )
}
```

(c) Append the footer at the end of `render_transcript` — change its tail from:

```rust
    for s in segments {
        let text = s.text.trim();
        if text.is_empty() {
            continue;
        }
        if meta.timestamps {
            out.push_str(&format!("{} {text}\n\n", format_timestamp(s.start_ms)));
        } else {
            out.push_str(&format!("{text}\n\n"));
        }
    }
    out
}
```

to:

```rust
    for s in segments {
        let text = s.text.trim();
        if text.is_empty() {
            continue;
        }
        if meta.timestamps {
            out.push_str(&format!("{} {text}\n\n", format_timestamp(s.start_ms)));
        } else {
            out.push_str(&format!("{text}\n\n"));
        }
    }
    out.push_str(&render_stats(meta, segments));
    out
}
```

(d) Add `processing_secs` to the `meta()` test helper so the existing tests still compile:

```rust
    fn meta() -> TranscriptMeta {
        TranscriptMeta {
            mp3_file_name: "2026-07-04 1405 Meeting.mp3".into(),
            model_label: "whisper-small".into(),
            language: Some("es".into()),
            duration_secs: 3723,
            generated_at: "2026-07-04T15:10:00+02:00".into(),
            timestamps: true,
            processing_secs: 47,
        }
    }
```

- [ ] **Step 4: Run the core tests to verify they pass**

Run (from `src-tauri/`): `cargo test -p vault_buddy_core transcript::`
Expected: PASS (new tests + the existing `render_transcript` tests, which still match their frontmatter/segment assertions).

Note: the `transcribe` crate won't compile yet — its `TranscriptMeta` literal is missing `processing_secs`. Step 5 fixes it.

- [ ] **Step 5: Measure processing time in `transcribe_recording`**

In `src-tauri/transcribe/src/lib.rs`, change `transcribe_recording` to time decode+inference and set the field. Replace:

```rust
    let samples = decode::decode_to_16k_mono(mp3)?;
    let duration_secs = samples.len() as u64 / decode::WHISPER_RATE as u64;
    let segments = transcriber.transcribe(&samples, opts.language.as_deref())?;
    let mp3_file_name = mp3
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let meta = TranscriptMeta {
        mp3_file_name,
        model_label: opts.model_label.clone(),
        language: opts.language.clone(),
        duration_secs,
        generated_at: generated_at.to_string(),
        timestamps: opts.timestamps,
    };
```

with:

```rust
    let started = std::time::Instant::now();
    let samples = decode::decode_to_16k_mono(mp3)?;
    let duration_secs = samples.len() as u64 / decode::WHISPER_RATE as u64;
    let segments = transcriber.transcribe(&samples, opts.language.as_deref())?;
    // Wall-clock of the actual work (decode + inference). Measured here, not
    // in core, so render_transcript stays clock-free and deterministic.
    let processing_secs = started.elapsed().as_secs();
    let mp3_file_name = mp3
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let meta = TranscriptMeta {
        mp3_file_name,
        model_label: opts.model_label.clone(),
        language: opts.language.clone(),
        duration_secs,
        generated_at: generated_at.to_string(),
        timestamps: opts.timestamps,
        processing_secs,
    };
```

Then extend the existing `transcribe_writes_the_sidecar` test — after its current assertions, add:

```rust
        assert!(text.contains("## Statistics"));
        assert!(text.contains("| Model | whisper-small |"));
```

- [ ] **Step 6: Run the transcribe tests to verify they pass**

Run (from `src-tauri/`): `cargo test -p vault_buddy_transcribe`
Expected: PASS (incl. the extended `transcribe_writes_the_sidecar`; processing time is ~0 for the instant fake and is not asserted exactly).

- [ ] **Step 7: Format + clippy**

Run (from `src-tauri/`): `cargo fmt --check` and
`cargo clippy -p vault_buddy_core -p vault_buddy_transcribe --all-targets -- -D warnings`
Expected: both clean.

- [ ] **Step 8: Commit**

```bash
git add src-tauri/core/src/transcript.rs src-tauri/transcribe/src/lib.rs
git commit -m "feat(transcribe): append a statistics table to finished transcripts"
```

---

## Self-Review

**Spec coverage:**
- Statistics table (Duration/Words/Segments/Speaking rate/Model/Language/Processing time/Generated) → Step 3 `render_stats`. ✅
- `processing_secs` measured in `transcribe_recording` → Step 5. ✅
- Zero-duration guard → Step 3 (`—`) + Step 1 test. ✅
- Additive / frontmatter + marker unchanged → Step 3 appends only; existing `render_transcript` tests still assert the frontmatter and pass. ✅
- Testing (pure core test + extend transcribe test) → Steps 1, 5. ✅

**Placeholder scan:** none — every step has concrete code/commands.

**Type consistency:** `TranscriptMeta.processing_secs: u64` defined in Step 3, set in Step 5, and included in the `meta()` helper (Step 3d). `render_stats(&TranscriptMeta, &[Segment]) -> String` defined and called in `render_transcript` (Step 3). ✅
