import { describe, expect, it } from "vitest";

import {
  decodeCaptionImportResult,
  decodeEditorError,
  decodeJobProgress,
  decodeJobRecords,
  decodeJobStarted,
  decodeOpenResult,
  decodeProject,
  decodeProjection,
  decodeProjectSummaries,
  decodeSaveReceipt,
  decodeSnapshot,
  ProtocolError,
} from "../src/editor/decode";

// Every literal below is copied verbatim from the matching Rust pin test
// (global-constraints: "Every new DTO gets a literal-JSON pin test in
// Rust and a decoder test in TS, never a struct re-serialized against
// itself") — a wire-shape regression on either side reddens this file
// even when Serialize/Deserialize still agree with each other in Rust.

const SNAPSHOT_LITERAL = {
  sessionId: "s",
  projectId: "p",
  revision: 3,
  persistedRevision: null,
  title: "Demo",
  durationMs: 5000,
  canUndo: true,
  canRedo: false,
  undoLabel: "Rename",
  redoLabel: null,
};

// core/src/editor/projection.rs's `minimal_project_json` — "captions" is
// absent (Rust's `skip_serializing_if` on `None`), which `decodeProject`
// must normalize to `null` rather than leaving it `undefined`.
const MINIMAL_PROJECT_LITERAL = {
  schema: "vault-buddy-video-project/3",
  id: "project",
  title: "Project",
  canvas: { width: 1280, height: 720, fps: 30 },
  master_gain: 1.0,
  assets: [],
  tracks: [],
  clips: [],
  effects: [],
  markers: [],
  transitions: [],
  destination: { vault: "", folder: "", dated: false },
};

// core/src/editor/projection.rs's `snapshot_json` (session ses-1/revision 3).
const PROJECTION_SNAPSHOT_LITERAL = {
  sessionId: "ses-1",
  projectId: "project",
  revision: 3,
  persistedRevision: null,
  title: "Project",
  durationMs: 0,
  canUndo: true,
  canRedo: false,
  undoLabel: "Rename",
  redoLabel: null,
};

describe("decodeSnapshot", () => {
  it("accepts the Rust literal (session.rs snapshot_wire_literal)", () => {
    expect(decodeSnapshot(SNAPSHOT_LITERAL)).toEqual({
      sessionId: "s",
      projectId: "p",
      revision: 3,
      persistedRevision: null,
      title: "Demo",
      durationMs: 5000,
      canUndo: true,
      canRedo: false,
      undoLabel: "Rename",
      redoLabel: null,
    });
  });

  it("rejects persistedRevision greater than revision", () => {
    // MUTATION CHECK (this task's brief): dropping the
    // `persistedRevision > revision` comparison in decodeSnapshot makes
    // this pass silently — a save receipt racing ahead of the session's
    // own revision would then read as already saved.
    const bad = { ...SNAPSHOT_LITERAL, revision: 3, persistedRevision: 4 };
    expect(() => decodeSnapshot(bad)).toThrow(ProtocolError);
  });

  it("rejects unsafe integers", () => {
    const bad = { ...SNAPSHOT_LITERAL, revision: 2 ** 53 };
    expect(() => decodeSnapshot(bad)).toThrow(ProtocolError);
  });

  it("rejects an absent undoLabel key (present-even-when-null)", () => {
    const { undoLabel: _undoLabel, ...withoutUndoLabel } = SNAPSHOT_LITERAL;
    expect(() => decodeSnapshot(withoutUndoLabel)).toThrow(ProtocolError);
  });
});

describe("decodeProjection", () => {
  it("accepts the Rust literal (projection.rs editor_projection_serializes_the_contract_shape)", () => {
    const decoded = decodeProjection({
      snapshot: PROJECTION_SNAPSHOT_LITERAL,
      project: MINIMAL_PROJECT_LITERAL,
    });
    expect(decoded.snapshot.sessionId).toBe("ses-1");
    expect(decoded.project.id).toBe("project");
    expect(decoded.project.master_gain).toBe(1.0);
    // The absent "captions" key normalizes to null, not undefined.
    expect(decoded.project.captions).toBeNull();
  });
});

describe("decodeOpenResult", () => {
  it("accepts the Rust literal (projection.rs editor_open_result_serializes_the_contract_shape)", () => {
    const decoded = decodeOpenResult({
      snapshot: PROJECTION_SNAPSHOT_LITERAL,
      project: MINIMAL_PROJECT_LITERAL,
      workspace: {},
      missing: [{ assetId: "src", name: "Demo", expectedSize: 7, expectedDurationMs: 9 }],
      sourceBase: "2026-09-20 1432 Demo",
      recovered: false,
    });
    expect(decoded.sourceBase).toBe("2026-09-20 1432 Demo");
    expect(decoded.missing).toEqual([
      { assetId: "src", name: "Demo", expectedSize: 7, expectedDurationMs: 9 },
    ]);
    expect(decoded.recovered).toBe(false);
  });

  it("decodes the MissingMedia Rust literal (projection.rs missing_media_serializes_camel_case)", () => {
    const decoded = decodeOpenResult({
      snapshot: PROJECTION_SNAPSHOT_LITERAL,
      project: MINIMAL_PROJECT_LITERAL,
      workspace: {},
      missing: [{ assetId: "src", name: "Demo", expectedSize: 4096, expectedDurationMs: 61500 }],
      sourceBase: null,
      recovered: false,
    });
    expect(decoded.missing).toEqual([
      { assetId: "src", name: "Demo", expectedSize: 4096, expectedDurationMs: 61500 },
    ]);
  });

  it("accepts a null sourceBase and an empty missing list", () => {
    const decoded = decodeOpenResult({
      snapshot: PROJECTION_SNAPSHOT_LITERAL,
      project: MINIMAL_PROJECT_LITERAL,
      workspace: {},
      missing: [],
      sourceBase: null,
      recovered: false,
    });
    expect(decoded.sourceBase).toBeNull();
    expect(decoded.missing).toEqual([]);
  });
});

describe("decodeSaveReceipt", () => {
  it("accepts the Rust literal (save_commands_tests.rs save_receipt_wire_literal)", () => {
    expect(
      decodeSaveReceipt({ sessionId: "ses-1", savedRevision: 4, projectFileId: "proj-1" }),
    ).toEqual({ sessionId: "ses-1", savedRevision: 4, projectFileId: "proj-1" });
  });
});

describe("decodeProjectSummaries", () => {
  it("accepts the Rust literal (store_io.rs project_summary_dto_serializes_camel_case_literal)", () => {
    const decoded = decodeProjectSummaries([
      {
        projectFileId: "abc123",
        title: "My Tutorial",
        updatedAt: "2026-09-21T10:00:00+02:00",
        persistedRevision: 3,
        hasRecovery: true,
        sourceBase: "2026-09-20 1432 Demo",
      },
    ]);
    expect(decoded).toEqual([
      {
        projectFileId: "abc123",
        title: "My Tutorial",
        updatedAt: "2026-09-21T10:00:00+02:00",
        persistedRevision: 3,
        hasRecovery: true,
        sourceBase: "2026-09-20 1432 Demo",
      },
    ]);
  });

  it("accepts a null sourceBase (store_io.rs project_summary_dto_carries_a_null_source_base_when_none)", () => {
    const decoded = decodeProjectSummaries([
      {
        projectFileId: "abc123",
        title: "Untitled",
        updatedAt: "2026-09-21T10:00:00+02:00",
        persistedRevision: 1,
        hasRecovery: false,
        sourceBase: null,
      },
    ]);
    expect(decoded[0].sourceBase).toBeNull();
  });
});

describe("decodeEditorError", () => {
  it("accepts the Rust literal (error.rs error_serializes_camel_case_literal)", () => {
    const decoded = decodeEditorError({
      code: "revisionConflict",
      message: "m",
      retryable: true,
      operationId: "op-x",
    });
    expect(decoded).toEqual({
      code: "revisionConflict",
      message: "m",
      retryable: true,
      operationId: "op-x",
    });
    expect(decoded.retainedAssetIds).toBeUndefined();
  });

  it("rejects an unrecognized error code", () => {
    expect(() =>
      decodeEditorError({ code: "bogus", message: "m", retryable: false, operationId: "op-x" }),
    ).toThrow(ProtocolError);
  });
});

describe("decodeProject", () => {
  it("rejects an unknown effect kind", () => {
    const project = {
      ...MINIMAL_PROJECT_LITERAL,
      effects: [
        {
          id: "e1",
          clip_id: "c1",
          kind: "blur",
          start_ms: 0,
          end_ms: 100,
          x: 0,
          y: 0,
          color: "#fff",
        },
      ],
    };
    expect(() => decodeProject(project)).toThrow(ProtocolError);
  });

  it("accepts the minimal Rust literal", () => {
    const decoded = decodeProject(MINIMAL_PROJECT_LITERAL);
    expect(decoded.captions).toBeNull();
    expect(decoded.destination).toEqual({ vault: "", folder: "", dated: false });
  });
});

describe("decodeJobProgress", () => {
  const VALID = {
    sessionId: "ses-1",
    jobId: "job-1",
    kind: "render",
    sequence: 1,
    phase: "rendering",
    fraction: 0.5,
    terminal: null,
  };

  it("accepts a well-formed progress event", () => {
    expect(decodeJobProgress(VALID)).toEqual(VALID);
  });

  it("rejects fraction 1.01", () => {
    expect(() => decodeJobProgress({ ...VALID, fraction: 1.01 })).toThrow(ProtocolError);
  });

  it('rejects phase "done"', () => {
    expect(() => decodeJobProgress({ ...VALID, phase: "done" })).toThrow(ProtocolError);
  });

  // `media_jobs.rs`' `job_progress_wire_shape_is_pinned` terminal literal,
  // byte for byte: absent terminal fields are ABSENT, not null.
  it("decodes the Rust import terminal literal", () => {
    const literal = {
      sessionId: "ses-a", jobId: "job-b", kind: "import",
      sequence: 3, phase: "complete", fraction: 1.0,
      terminal: {
        assetIds: ["asset-1"],
        perFile: [{ name: "broken.mov", error: "damaged" }],
      },
    };
    expect(decodeJobProgress(literal)).toEqual(literal);
  });

  it("rejects an absent terminal key (present-even-when-null)", () => {
    const { terminal: _terminal, ...noTerminal } = VALID;
    expect(() => decodeJobProgress(noTerminal)).toThrow(ProtocolError);
  });
});

describe("decodeJobRecords / decodeJobStarted", () => {
  // `media_jobs.rs`' `JobRecordDto` literal (`job_progress_wire_shape_is_pinned`).
  it("decodes the Rust registry row literal", () => {
    const rows = [{ jobId: "job-b", kind: "import", phase: "cancelled", fraction: 0.25, terminal: {} }];
    expect(decodeJobRecords(rows)).toEqual(rows);
  });

  it("rejects a non-array reply and a malformed row", () => {
    expect(() => decodeJobRecords(undefined)).toThrow(ProtocolError);
    expect(() =>
      decodeJobRecords([{ jobId: "job-b", kind: "import", phase: "nope", fraction: 0, terminal: null }]),
    ).toThrow(ProtocolError);
  });

  it("decodes { jobId } and refuses an invalid id", () => {
    expect(decodeJobStarted({ jobId: "job-b" })).toEqual({ jobId: "job-b" });
    expect(() => decodeJobStarted({ jobId: "../x" })).toThrow(ProtocolError);
  });
});

describe("decodeCaptionImportResult", () => {
  it("accepts the Rust literal (projection.rs caption_import_result_serializes_the_contract_shape)", () => {
    const decoded = decodeCaptionImportResult({
      projection: { snapshot: PROJECTION_SNAPSHOT_LITERAL, project: MINIMAL_PROJECT_LITERAL },
      imported: 12,
      skipped: 3,
    });
    expect(decoded).not.toBeNull();
    expect(decoded?.imported).toBe(12);
    expect(decoded?.skipped).toBe(3);
    expect(decoded?.projection.snapshot.revision).toBe(3);
  });

  it("decodes a cancelled dialog (null) as null", () => {
    expect(decodeCaptionImportResult(null)).toBeNull();
  });

  it("rejects a negative or fractional count and a missing projection", () => {
    const projection = { snapshot: PROJECTION_SNAPSHOT_LITERAL, project: MINIMAL_PROJECT_LITERAL };
    expect(() => decodeCaptionImportResult({ projection, imported: -1, skipped: 0 })).toThrow(ProtocolError);
    expect(() => decodeCaptionImportResult({ projection, imported: 1.5, skipped: 0 })).toThrow(ProtocolError);
    expect(() => decodeCaptionImportResult({ projection, imported: 1 })).toThrow(ProtocolError);
    expect(() => decodeCaptionImportResult({ imported: 1, skipped: 0 })).toThrow(ProtocolError);
  });
});
