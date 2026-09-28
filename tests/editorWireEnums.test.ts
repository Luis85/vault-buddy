/**
 * The editor's IPC error and job-progress enums, pinned against their Rust
 * declarations (Task 20, hardening review M-V9/M-V10). `decode.ts`'s
 * `ERROR_CODES`/`JOB_PHASES`/`JOB_KINDS` gate what a reply is allowed to
 * carry: an unrecognized error code silently downgrades to `internal`
 * (`isEditorError`/`toPortError`), but an unrecognized job kind or phase
 * fails the WHOLE `editor_get_jobs` reply — so a variant Rust gains and this
 * file does not would either mask a real error code or break job
 * reconciliation for every session. `rustVariants`
 * (`tests/helpers/rustSource.ts`, the `editorCheckWireSpellings.test.ts`
 * precedent) reads the Rust enums directly, so the two can never drift
 * silently.
 */
import { describe, expect, it } from "vitest";

import { ERROR_CODES, JOB_KINDS, JOB_PHASES } from "../src/editor/decode";
import { rustVariants } from "./helpers/rustSource";

const ERROR_RS = "src-tauri/core/src/editor/error.rs";
const JOBS_RS = "src-tauri/src/editor/media_jobs.rs";

describe("the editor's wire enums match their Rust declarations", () => {
  it.each([
    ["EditorErrorCode", ERROR_RS, ERROR_CODES],
    ["JobKind", JOBS_RS, JOB_KINDS],
    ["JobPhase", JOBS_RS, JOB_PHASES],
  ] as const)("%s", (enumName, file, listed) => {
    const variants = rustVariants(enumName, file);
    expect(variants.length).toBeGreaterThan(2);
    expect([...listed].sort()).toEqual(variants);
  });
});
