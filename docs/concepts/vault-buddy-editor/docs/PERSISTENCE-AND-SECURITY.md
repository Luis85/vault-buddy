# Persistence, recovery, privacy and authorization

## Separate lifecycles

A recording is staged outside every vault. A project can be saved without rendering. A render becomes a separate immutable product. Closing a view is not discarding staged work. Guide progress is an application preference, not part of video content. Every operation must honor these distinctions.

The browser reference downloads JSON/ZIP files and provides best-effort cache recovery. A requested download is not confirmation that a file reached its intended folder. Native status must come from actual save/publication receipts.

## Recommended native layout

```text
<local-app-data>/screen-captures/     existing owned capture staging
<local-app-data>/editor-sessions/<session-id>/
  session.json                      destination ID, source grants, recovery metadata
  journal/                          bounded edit/recovery transactions
  media/                            managed originals or durable source locators
  jobs/<job-id>/                     owned render temporary files + journal
<user-selected project destination>/
  <name>.vbproject...                editable project / managed portable format
<configured vault tutorial folder>/
  <collision-safe product>.mp4
  <collision-safe product>.md
```

This is a logical layout proposal. Reuse existing application directory APIs and capture naming rules. Do not copy source files unnecessarily if an approved project-managed source store provides durable references; do not keep ephemeral grants/paths as the only link in a supposedly portable project.

## Save transaction

Freeze revision `r` and collect all required current and retained-snapshot assets. Resolve the selected destination natively, reserve safe paths without overwriting unrelated user files and write a transaction record. Stream package entries rather than buffering all media. Validate manifest paths, lengths and source identities. Write to a unique same-volume owned temporary file; flush according to the supported durability policy; atomically replace only a file that this project owns or explicitly export under a new no-clobber name.

After commit, return `{sessionId,savedRevision:r,projectFileId}`. If the current edit has advanced to `r+1`, it stays dirty even though `r` was saved. Reject stale/wrong-session receipts. Failure/cancel leaves the prior good file and current edit intact. Dirty state cannot be cleared by successful serialization, a timer, a closed dialog or starting a download.

The inspected capture sidecar uses direct `std::fs::write`. It is capture metadata, not the final editor transaction layer. Implement atomic, recoverable project persistence rather than reusing it as if it already supplied crash safety. Existing capture naming and no-clobber helpers should be reconciled/reused with real concurrent-reservation tests; an existence check alone is not a race-proof reservation.

## Render product publication

One atomic rename cannot commit a movie, companion note and project record together. Use a journal with explicit steps: reserve target base, write/probe temporary output, write note candidate, publish output, publish note, commit product record, mark complete. Recovery inspects owned operation IDs and rolls forward or reports partial publication without replacing another file. A published video must not be silently deleted because its companion note failed. A record must not say a product is available before its output exists.

Retain the source snapshot/revision and output-range metadata. Resolve collisions independently of tutorial display titles and preserve safe internal links. Note text and frontmatter must be escaped/serialized as data; test quotes, newlines, Markdown delimiters and unusual filenames. Do not assume a filename suffix alone confers ownership.

## Project import and reconnection

A project or archive is untrusted input, including one found inside a vault. Bound compressed size, entry count, total expanded bytes, compression ratio and nested documents. Reject absolute/drive/UNC paths, traversal (`..`), duplicate canonical names, symlink/hardlink escapes, invalid UTF-8 policy violations and conflicting manifests. Resolve canonical destinations and guard Windows reparse points/TOCTOU cases. Do not allow imported locators to expand native permissions.

Validate graph and every product snapshot before installation. Preserve the current project on any error. Report unavailable files per asset; reconnect candidates using durable identity/probe metadata and preferably hashes, never filename alone. Batch reconnect refuses ambiguous matches and preserves existing timeline IDs/cue associations. Changes that replace a source with a materially different recording require explicit user confirmation and a new source identity.

## Access control and CSP

Native project commands validate caller window, session ownership, asset grants and destination IDs. Scope editor permissions narrowly. Tauri capability grants combine across files; inspect all capabilities that match the editor window. Registered custom app commands are broadly callable by app windows by default unless app-manifest/permission configuration restricts them, so implement that configuration and native checks together. [Official guidance](REFERENCES.md#official-guidance).

Do not grant arbitrary filesystem read/write, shell execution or wildcard home/vault media access just to show a preview. Use native file dialogs or scoped registries to authorize import. Resolve staging identifiers inside owned directories; the stopped-event path is not a reusable arbitrary file permission. `convertFileSrc` is a URL conversion, not an authorization check.

Keep production scripts bundled under CSP; do not copy the standalone HTML's inline script arrangement into the application by adding `unsafe-inline` script permission. Media/image asset and blob allowances should match actual needs. Test the final scope with approved and unauthorized sample paths, not only a normal preview success case.

## Privacy and capture

Camera/microphone access starts only after a user action. Device indicators, countdown, cancel, stop, retake and close cleanup must work even on rejection/error. Stop all camera tracks when capture UI ends; do not leave the camera live behind another dialog. Native simultaneous capture uses a coordinated session, not hidden background device startup.

Portable project files can contain uncensored originals, discarded takes still referenced by snapshots and past rendered products. A stationary mask covers pixels only while it applies to the output; it is not tracking, document sanitization or deletion of source content. Warn before sharing a portable project. Sharing a reviewed rendered product should not implicitly export originals or the whole workspace.

Default behavior is local with no content telemetry. Diagnostics expose counts/capabilities and correlation IDs, not frame/audio content, source filenames or full paths unless the user explicitly chooses an appropriate diagnostic export. Avoid accepting exception text into HTML. Keep camera errors actionable without leaking unnecessary device identifiers.

## Recovery and garbage collection

Maintain a native registry of active sessions, jobs, retained sources and ownership. Recovery only processes owned transaction markers and validates staleness/activity; do not scan a vault and delete files by extension. Defer cleanup while a writer is active. A source is collectible only when no current project, undo retention policy, product snapshot, running job or retained capture refers to it. Clearing a UI map is not native file deletion.

On app start, show recoverable work with meaningful title/date/status. Choices are Resume, Save a copy, or explicit Discard. If a malformed record cannot load, keep the original bytes and provide an actionable report; do not rewrite defaults over it. Native close/shutdown must account for pending saves, capture finalization, render publication and unsaved webcam takes.

## Acceptance threats

Test malicious archive paths, symlinks/reparse points, huge declarations, cyclic source links, unknown schema versions, XSS-like filenames/captions, duplicate IDs, unauthorized windows, wrong session IDs, replayed command IDs, stale receipts, revoked import grants, output collisions, disk-full partial writes, stale locks, interrupted publication, orphan recovery, repeated close/reopen and cancellation races. These tests belong in native release gates as well as browser validator tests.
