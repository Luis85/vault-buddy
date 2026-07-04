# Knowledge Intake — Product Requirements Document (PRD)

- **Capability for:** Vault Buddy
- **Status:** Draft
- **Version:** 1.0
- **Parent Product:** [Vault Buddy](./PRD.md)

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
- AI transcription.
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

Initially only **Audio Recording** will be implemented.

Future providers include:

- Audio
- Screenshot
- Screen Recording
- Clipboard
- File Import
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

### AI

- Automatically Transcribe
- Generate Summary
- Extract Tasks
- Generate Meeting Note
- Language
- Preferred LLM

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

When enabled, Vault Buddy automatically creates a Markdown document.

The document contains:

- Metadata
- Embedded recording
- Transcript
- Summary
- Decisions
- Action Items
- Open Questions

This transforms an audio file into immediately usable knowledge.

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

### Version 1

- Audio Recording

### Version 2

- AI Transcription
- Meeting Notes
- Task Extraction
- Summaries

### Version 3

- Screenshot Capture
- Clipboard Capture
- File Import

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
