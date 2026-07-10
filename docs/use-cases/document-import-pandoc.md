---
type: UseCase
status: planned
domain: knowledge-intake
source_prd: "docs/prds/knowledge-intake.md"
related_specs:
  - "docs/superpowers/specs/2026-07-10-document-import-pandoc-design.md"
tags: [use-case, knowledge-intake]
---

# Document Import via Pandoc

> Turn a `.docx` / `.odt` / `.rtf` file into a vault note in one or two clicks — drag it onto the buddy or pick it from the record chooser — using a user-installed Pandoc. Vault Buddy never bundles Pandoc itself; the feature is gated behind detecting it and guides the user to install it from Settings.

## Source

Knowledge Intake PRD, [Capability Overview](../prds/knowledge-intake.md) and [Vault Settings](../prds/knowledge-intake.md): a new Capture Provider alongside Audio Recording, tracked separately from the generic "File Import" Version 3 placeholder because it's fully specified and nearer-term.

## Status: Not started (design complete)

Design: [2026-07-10-document-import-pandoc-design.md](../superpowers/specs/2026-07-10-document-import-pandoc-design.md). No code exists yet.

## Summary

- **Trigger**: drag-and-drop a supported file onto the buddy (opens a vault picker, since the buddy icon isn't vault-specific), or a new "Import Document" action in the record chooser (vault already known). Both converge on one `convert_document(vaultId, sourcePath)` command.
- **Gate**: Pandoc must be detected on `PATH` (or at a manually configured path) before either trigger is enabled. A new "Document Import" Buddy-settings section shows install status, a Recheck button, a link to Pandoc's install page, and the manual path override — Vault Buddy never downloads or runs the Pandoc installer itself.
- **Formats**: `.docx`, `.odt`, `.rtf` — one shared conversion path per format via Pandoc's reader flag.
- **Output**: `<vault>/<DocumentsFolder>/YYYY/MM/YYYY-MM-DD <Original Name>.md`, with `type: Document` / `tags: [vault-buddy-import]` / `source` / `imported` / `format` frontmatter; embedded images (if any) extract to a same-named sibling folder. Same collision-safe atomic write discipline as Tasks/Recordings — never clobbers.
- **Failure**: toast + nothing written to the vault, mirroring `capture:failed`. Success is a silent save + toast (no auto-open).
- **Non-goals (this version)**: batch import, formats beyond the three above, bundling/auto-installing Pandoc, a watched Inbox folder or OS file-association integration, auto-opening the note, any AI pipeline step.

## Related use-cases

- [Meeting Recording](meeting-recording.md) / [Voice Note Recording](voice-note-recording.md) (the shipped capture provider this one follows the conventions of)
- [Additional Capture Providers](additional-capture-providers.md) (the generic, still-unspecified backlog this is deliberately kept separate from)
