# Knowledge Intake — Product Requirements Document (PRD)

- **Capability for:** Vault Buddy
- **Status:** Version 1 shipped in Vault Buddy v0.3.0 (audio recording +
  on-device transcription); later versions planned — see the roadmap
- **Version:** 1.2
- **Parent Product:** [Vault Buddy](PRD%20-%20Product%20Vision.md)

Use cases extracted from this PRD, with shipping status: [Meeting
Recording](../use-cases/meeting-recording.md), [Voice Note
Recording](../use-cases/voice-note-recording.md), [Local Speech-to-Text
Transcription](../use-cases/local-transcription.md), [Companion Note &
Follow-up Template](../use-cases/companion-note-and-follow-up-template.md),
[Recordings Browser](../use-cases/recordings-browser.md),
[Re-transcription](../use-cases/re-transcription.md), [Rename
Recording](../use-cases/rename-recording.md) (shipped, undocumented until
now), [AI-Enriched Meeting Notes](../use-cases/ai-enriched-meeting-notes.md),
[Document Import via
Pandoc](../use-cases/document-import-pandoc.md) (planned) and [Additional
Capture Providers](../use-cases/additional-capture-providers.md) (planned).
See [docs/use-cases/](../use-cases/README.md) for the full catalog.

---

## Vision

Everything worth remembering should be capturable within one click.

Knowledge workers should never lose valuable information because capturing it is cumbersome.

Vault Buddy provides a unified, local-first capture experience that enables users to collect knowledge from multiple sources and immediately store it inside the appropriate Obsidian Vault.

Knowledge Intake is the primary entry point into the user's knowledge system.

---

## Mission

Provide a frictionless, desktop-native knowledge capture experience that works independently from Obsidian while integrating seamlessly with every configured Vault.

The user should never have to think about:

- where information belongs
- which application is currently open
- how files should be organized

Vault Buddy handles these concerns automatically.

---

## Problem Statement

Most knowledge is created outside of Obsidian.

Examples include:

- Microsoft Teams meetings
- Voice notes
- Customer calls
- Brainstorming sessions
- Screenshots
- Screen recordings
- Clipboard content
- Documents
- Browser research

Capturing this information today requires multiple manual steps:

- start another application
- configure recording
- save the file
- move the file
- rename it
- import it into Obsidian
- create a note
- link everything together

This process interrupts the user's flow and often results in knowledge never being captured.

---

## Goals

### Primary Goals

- Capture knowledge with one click.
- Operate independently from Obsidian.
- Store captured knowledge directly inside the configured Vault.
- Minimize user interaction.
- Enable future AI processing.

### Secondary Goals

- Automatic metadata generation.
- Automatic note creation.
- Local transcription (shipped in v0.3.0).
- AI summarization.
- Workflow automation.

### Non-Goals (MVP)

The MVP will not include:

- Video recording
- OCR
- Cloud synchronization
- Live transcription
- Collaboration
- Mobile support

---

## Capability Overview

Knowledge Intake consists of multiple **Capture Providers**.

**Audio Recording** is the first Capture Provider, shipped in v0.3.0 (with
on-device transcription as a post-capture step).

Future providers include:

- Audio
- Document Import (Pandoc) — designed, not yet built; see below
- Screenshot
- Screen Recording
- Clipboard
- File Import (generic, unspecified — kept distinct from Document Import)
- Browser Capture
- Camera
- Email
- Mobile Upload

Every provider follows the same workflow:

```
Capture → Process → Store → Link → (optional) AI Pipeline
```

---

## MVP Feature: Audio Recording

Vault Buddy allows users to record audio directly from the desktop.

The recording is independent from Obsidian.

When recording stops, the resulting MP3 is automatically stored inside the configured Vault.

---

## User Stories

### Meeting Recording

> As a user, I want to record a Teams meeting so I can review it later.

### Voice Note

> As a user, I want to quickly record an idea without opening Obsidian.

### Customer Call

> As a consultant, I want every customer conversation to be stored in my project Vault.

### Research Session

> As a researcher, I want to capture spoken observations while working.

---

## User Experience

Each Vault displayed in Vault Buddy contains two primary actions:

- 📅 **Open Daily Note**
- 🎙 **Capture Knowledge**

Selecting **Capture** immediately starts a recording using the Vault's configured settings.

No Obsidian interaction is required.

---

## Vault Settings

Every configured Vault receives its own Capture configuration.

### General

- Vault Name
- Icon
- Color
- Default Vault

### Capture

- Recording Folder
- Voice Notes Folder
- Screenshots Folder
- Inbox Folder
- Temporary Folder
- Automatic Cleanup

### Audio

- Default Microphone
- Default Output Device
- Recording Mode
- Sample Rate
- Channels
- Bitrate
- Noise Reduction
- Normalize Audio
- Automatic Gain Control

### Transcription (shipped)

- Automatically Transcribe — per-vault opt-in
- Transcription Model — base / small / medium, downloaded on demand
- Language — or auto-detect per recording
- Segment Timestamps
- Follow-up Template — append a `## Follow-up` scaffold to the companion note

### AI (planned)

- Generate Summary
- Extract Tasks
- Generate Meeting Note (AI-enriched)
- Preferred LLM

### Document Import (planned)

- Documents Folder — per-vault, default `Documents`

App-global (not per-vault, since Pandoc is one system-wide binary):

- Pandoc Path — manual override for non-`PATH` installs
- Pandoc Status — detected version, with a Recheck action

---

## Recording Modes

| Mode | Sources |
| --- | --- |
| **Voice Note** | Microphone only |
| **Meeting** | Microphone + Desktop Audio |
| **Desktop Audio** | Desktop output only |
| **Custom** | User selects devices manually |

---

## Recording Workflow

```
User clicks Capture
  ↓
Load Vault Configuration
  ↓
Initialize Audio Devices
  ↓
Start Recording
  ↓
Display Recording Status
  ↓
Stop Recording
  ↓
Encode MP3
  ↓
Generate Metadata
  ↓
Store inside Vault
  ↓
(optional) Create Markdown Note
  ↓
(optional) Start AI Processing
```

---

## File Organization

The recording should automatically follow a predictable folder structure.

### Example

```
Meetings/
  2026/
    07/
      2026-07-04 Sprint Planning.mp3
      2026-07-04 Sprint Planning.md

Voice Notes/
  2026/
    07/
      Idea about Vault Buddy.mp3
```

---

## Metadata

Every recording should generate metadata.

### Example

- Recording Date
- Duration
- Vault
- Recording Type
- Input Devices
- Output Devices
- Language
- Creation Timestamp

---

## Optional Meeting Note

When enabled (the default, per-vault `createNote`), Vault Buddy writes a
Markdown companion note alongside the recording.

The document contains:

- Metadata (date, duration, type, devices, language)
- The embedded recording
- The embedded transcript, once transcription completes
- A `## Follow-up` scaffold (action items, decisions, notes) when the per-vault
  Follow-up Template is on (the default)

Metadata and the audio embed are always present. **Transcription** (shipped in
v0.3.0) runs on-device after the recording and writes a `<name>.transcript.md`
sidecar the note embeds — no cloud, no API. The Follow-up scaffold is a static,
ready-to-fill section, not AI output. AI-produced summaries, decisions, and
action-item extraction remain a future pipeline (see the roadmap); the note
never contains empty AI placeholder sections.

This transforms an audio file into immediately usable knowledge.

---

## Recordings List

Past recordings are browsable in the panel: from the record chooser, **Browse
recordings** opens a read-only list of the vault's captures, grouped by type,
each row showing title, date, duration, and transcript status. Selecting a row
opens its companion note in Obsidian. The list never writes into the vault.

---

## Re-transcription

Every recording row offers a **re-transcribe** action that regenerates its
transcript on demand — useful after switching to a larger, more accurate model,
or to recover a failed transcript. It confirms before replacing a finished
transcript, bypasses the vault's automatic-transcription toggle (an explicit
per-recording opt-in), and overwrites only the transcript sidecar — never the
audio or the note.

---

## Document Import via Pandoc (planned)

A second Capture Provider, complementary to Audio Recording: turn a
`.docx` / `.odt` / `.rtf` file into a vault note. Design complete, not yet
built — see [Document Import via
Pandoc](../use-cases/document-import-pandoc.md) and its [design
spec](../superpowers/specs/2026-07-10-document-import-pandoc-design.md)
for the full detail. Summary:

- **Trigger**: drag-and-drop a supported file onto the buddy (opens a
  vault picker, since the buddy icon isn't vault-specific), or a new
  "Import Document" action in the record chooser (vault already known).
- **Gate, not bundle**: Vault Buddy stays local-first and license-clean by
  requiring a **user-installed Pandoc** rather than shipping one — Pandoc
  is GPL-2 and a ~150–200MB Windows binary, neither of which fits an MIT,
  lightweight-installer app. A new "Document Import" Buddy-settings
  section detects Pandoc on `PATH` (or a manually configured path), shows
  install status, a Recheck button, and a link to Pandoc's install page.
  Both triggers are disabled until Pandoc is detected.
- **Output**: `<vault>/<DocumentsFolder>/YYYY/MM/YYYY-MM-DD <Original
  Name>.md`, `type: Document` / `tags: [vault-buddy-import]` frontmatter
  recording source path, import date, and original format; embedded
  images extract to a same-named sibling folder when present. Same
  collision-safe atomic write discipline as every other sanctioned vault
  write (Tasks, Recordings, Transcripts) — never clobbers.
- **Failure handling**: a toast and nothing written to the vault on any
  conversion error, mirroring `capture:failed`; success is a silent save
  + toast, no auto-open.
- **Not in this version**: batch import, formats beyond the three above,
  bundling/auto-installing Pandoc, a watched Inbox folder, OS
  file-association integration, or any AI pipeline step on the imported
  content.

---

## Functional Requirements

### Audio Capture

- Support microphone recording.
- Support desktop audio recording.
- Support simultaneous recording.
- Support pause.
- Support resume.
- Support stop.
- Support recording indicator.

### File Management

- Automatic folder creation.
- Automatic filename generation.
- Duplicate handling.
- Configurable naming templates.

### Settings

- Per-Vault configuration.
- Device selection.
- Output folder selection.
- Encoding configuration.

### Notifications

- Recording started.
- Recording stopped.
- Recording saved.
- Recording failed.

---

## Non-Functional Requirements

### Performance

| Operation | Target |
| --- | --- |
| Recording startup | < 2 seconds |
| Save time | < 5 seconds |

### Reliability

- No recording loss.
- Graceful recovery after crash.
- Atomic file writing.

### Security

- Local-only processing.
- No cloud upload.
- No telemetry.
- No hidden recordings.
- Visible recording indicator.

---

## Technical Architecture

Knowledge Intake becomes its own bounded context.

```
Knowledge Intake
├── Capture Engine
├── Audio Provider
├── File Manager
├── Metadata Generator
├── AI Pipeline
└── Notification Service
```

The Capture Engine is independent from Obsidian.

Obsidian is only responsible for consuming the resulting files.

---

## Future Roadmap

### Version 1 — shipped (v0.3.0)

- Audio Recording (Meeting / Voice Note)
- Local on-device Transcription (model download, language, timestamps)
- Companion Meeting Notes + Follow-up template
- Recordings browser + Re-transcribe

### Version 2 — planned

- Summaries
- Task Extraction
- AI-enriched Meeting Notes (decisions, action items, open questions)
- Document Import via Pandoc (docx / odt / rtf → Markdown; design
  complete, see [Document Import via
  Pandoc](../use-cases/document-import-pandoc.md))

### Version 3

- Screenshot Capture
- Clipboard Capture
- File Import (generic, unspecified — distinct from Document Import above)

### Version 4

- Screen Recording
- Browser Capture
- Camera Capture

### Version 5

- Continuous Capture
- Meeting Detection
- Automatic Project Assignment
- Knowledge Inbox
- Workflow Automation

---

## Success Metrics

- Time to start recording
- Recordings per day
- Percentage of recordings stored successfully
- Number of generated meeting notes
- User satisfaction
- AI processing adoption
- Reduction in manual capture workflow

Consistent with the Security requirements (local-only processing, no
telemetry), these metrics are never collected or transmitted
automatically. They are measured through local-only means the user
controls — on-device logs and counters the user can inspect, manual
benchmarks, and explicit user feedback — or through opt-in research
sessions.

---

## Long-Term Vision

Knowledge Intake becomes the universal entry point for every piece of information entering a user's personal knowledge system.

Regardless of whether the source is speech, documents, screenshots, browser content or future AI conversations, the capture experience remains identical:

```
Capture → Process → Organize → Understand → Retrieve
```

Vault Buddy removes the friction between experiencing information and preserving knowledge.
