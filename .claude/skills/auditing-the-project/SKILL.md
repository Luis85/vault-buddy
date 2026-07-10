---
name: auditing-the-project
description: Use when asked to audit the codebase, hunt bugs/tech-debt/untested paths at scale, refresh docs/Gaps.md, or reconcile AGENTS.md with reality — e.g. before a release, after a large increment lands, or when doc drift is suspected.
---

# Auditing the Project

## Overview

A full-project audit is a fan-out of parallel **read-only** subagents by
domain, followed by personal verification, deduplication, and a
**docs-only** update to the two living documents: `docs/Gaps.md` (the
findings backlog) and `AGENTS.md` (whose factual tables must match the
code). Fixing what the audit finds is separate, later work.

## When to use

- Before a release, or after a large increment lands.
- When `docs/Gaps.md`'s header date/version is stale relative to `main`.
- When the user asks for an audit, a "relentless" review, or a doc refresh.

**Not for:** a single suspected bug (use superpowers:systematic-debugging)
or reviewing a diff (use the code-review workflow).

## The audit contract — the deliverable IS all six

1. `docs/Gaps.md` header carries today's date and the current version.
2. Every pre-existing GAP entry re-verified at its cited location:
   fixed → deleted, drifted line numbers → updated, still valid → left.
3. New findings appended in the existing GAP-NN shape — severity per the
   file's own definitions, location, concrete failure scenario, one-line
   remediation — numbering continuing from the highest existing entry.
4. The "Verified sound" section refreshed (add what this audit confirmed,
   remove anything no longer true).
5. `AGENTS.md` factual tables reconciled against the code: the IPC command
   table vs `generate_handler` in `src-tauri/src/lib.rs`, the events table
   vs the `app.emit` sites, the CI table vs `.github/workflows/ci.yml`,
   the documentation map vs `docs/`.
6. No source or config file changed — the audit commit(s) are docs-only.

## Procedure

1. Read `docs/Gaps.md` and the AGENTS.md sections for the areas in scope;
   `git log` since the Gaps.md header date. If nothing material changed,
   re-verify entries instead of re-auditing, and say so.
2. Fan out parallel read-only subagents, one per domain (table below), in
   a single message. Every prompt states: the exact file scope, the
   invariants AGENTS.md claims for that area, the existing GAP entries for
   it, and the finding contract — severity, `file:line`, one-sentence
   summary, concrete failure scenario, one-sentence remediation, plus
   "verify every claim against the code; report nothing speculative".
3. Personally spot-verify every High finding and any surprising Medium
   before publishing — subagents over-report plausible-sounding issues.
4. Deduplicate across agents (the same defect surfaces in 2-3 reports),
   then classify severity using Gaps.md's definitions verbatim.
5. Update the two documents per the contract. Commit as `docs(gaps)` /
   `docs(agents)` with the audit method in the body.

## Domain fan-out

| Agent | Scope |
| --- | --- |
| core crate | `src-tauri/core/src/*` — invariant enforcement, edge cases, untested branches |
| capture + transcribe | `src-tauri/capture/`, `src-tauri/transcribe/` — audio/STT engines, races, data-loss paths |
| shell | `src-tauri/src/*` + `tauri.conf.json` + `capabilities/` — threading, IPC surface, main-thread blocking |
| frontend + tests | `src/**` + `tests/**` — races, listener lifecycle, coverage map |
| CI / build / docs | `.github/`, `scripts/`, manifests, all docs — pipeline gaps and doc drift |
| architecture verify | data-flow traces + IPC/event/state inventories, every claim with a file reference |

## Common mistakes

- Treating the audit as Gaps.md-only and skipping AGENTS.md
  reconciliation — drift in its tables is a first-class audit finding.
- Publishing subagent findings without personal verification.
- Re-reporting entries already catalogued in Gaps.md as new discoveries.
- Fixing code in the audit commit.
- Re-running the full fan-out when `git log` shows nothing changed since
  the header date — a verification pass is the right size.
