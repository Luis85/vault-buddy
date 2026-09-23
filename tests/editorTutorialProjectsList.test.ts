/**
 * The Tutorial projects block (Task 37 Part B, F4): `TutorialProjectsList.vue`,
 * mounted beside `StagedCaptureList.vue` on the Record Screen picker,
 * listing every `list_tutorial_projects()` result — title, updated date, a
 * "Has unsaved changes" badge from `hasRecovery`, and Resume only. It is its
 * OWN component (not a second section inside `StagedCaptureList.vue`)
 * because adding that section there crossed this repo's template-complexity
 * quality ratchet — `TutorialProjectsList.vue`'s own doc explains why.
 *
 * F4 keeps Discard out of the panel entirely: discarding a project is a
 * session-scoped, revision-aware operation (`editor_close_session
 * (discardProject)`) that only the editor's own close/recovery UI (Task 37
 * Part A) performs — a button here would have nothing safe to call.
 *
 * Presentational, the `stagedCaptureList.test.ts` precedent: no `invoke`, no
 * store, no IPC mock. `ScreenSourcePicker` owns `list_tutorial_projects` /
 * `open_project_editor` behind it.
 */
import { mount } from "@vue/test-utils";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import TutorialProjectsList from "../src/components/TutorialProjectsList.vue";
import type { ProjectSummaryDto } from "../src/editorTypes";

/** Every age assertion is relative to this instant, matching the staged-list
 * suite's own fixed clock so `updatedAt` fixtures name real RFC3339
 * timestamps (what Rust actually sends) rather than an offset. */
const NOW = "2026-09-20T12:00:00Z";

function project(over: Partial<ProjectSummaryDto> = {}): ProjectSummaryDto {
  return {
    projectFileId: "proj1",
    title: "Intro to Figma",
    updatedAt: "2026-09-20T10:00:00Z",
    persistedRevision: 3,
    hasRecovery: false,
    sourceBase: "2026-09-20 1000 Figma",
    ...over,
  };
}

function list(projects: ProjectSummaryDto[]) {
  return mount(TutorialProjectsList, { props: { projects } });
}

const row = (id: string) => `[data-testid="project-row-${id}"]`;
const resume = (id: string) => `[data-testid="project-resume-${id}"]`;

describe("TutorialProjectsList", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date(NOW));
  });
  afterEach(() => vi.useRealTimers());

  it("lists projects with title, updated date, a Resume button and no Discard button", async () => {
    const w = list([project()]);
    const r = w.get(row("proj1"));
    expect(r.text()).toContain("Intro to Figma");
    // The updated date renders as a relative age, the staged-list precedent.
    expect(r.text()).toContain("2h ago");
    // Fix round 1 (review Minor): a guessed `data-testid="project-discard-…"`
    // can never fail — the component defines no such id under ANY
    // circumstance, so a real Discard control added under a DIFFERENT name
    // would sail past that check. Asserting the row's own button COUNT and
    // label instead catches a Discard button regardless of what it is
    // called.
    expect(r.findAll("button")).toHaveLength(1);
    expect(r.get("button").text()).toBe("Resume");

    await w.get(resume("proj1")).trigger("click");
    expect(w.emitted("resumeProject")).toEqual([["proj1"]]);
  });

  // The paired negative for a second row: Resume must address the row that
  // was clicked, not always the first one.
  it("emits resumeProject for the row that was clicked, not the first one", async () => {
    const w = list([project(), project({ projectFileId: "proj2", title: "Second project" })]);
    await w.get(resume("proj2")).trigger("click");
    expect(w.emitted("resumeProject")).toEqual([["proj2"]]);
  });

  it("shows a recovery badge for a project with hasRecovery, and hides it otherwise", () => {
    const withRecovery = list([project({ hasRecovery: true })]);
    expect(withRecovery.get(row("proj1")).text()).toContain("Has unsaved changes");

    const without = list([project({ hasRecovery: false })]);
    expect(without.get(row("proj1")).text()).not.toContain("Has unsaved changes");
  });

  it("renders no tutorial-projects block when there are none", () => {
    const w = list([]);
    expect(w.find('[data-testid="tutorial-projects-list"]').exists()).toBe(false);
    expect(w.text()).toBe("");
  });
});
