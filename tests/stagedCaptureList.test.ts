/**
 * The resume-or-discard list (spec §10): "Opening Record Screen with staged
 * captures present shows them first: each with its source, duration and age,
 * offering *Resume editing* or *Discard*."
 *
 * Presentational — no `invoke`, no store, no IPC mock. `ScreenSourcePicker`
 * owns the two commands behind it and `tests/screenSourcePicker.test.ts`
 * pins that half.
 */
import { mount } from "@vue/test-utils";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import StagedCaptureList from "../src/components/StagedCaptureList.vue";
import type { StagedCaptureSummary } from "../src/types";
import { relativeAgeLabel } from "../src/utils/relativeAge";

/** Every age assertion is relative to this instant, so the fixtures can name
 * a real RFC3339 timestamp (what Rust actually sends) rather than an offset. */
const NOW = "2026-09-20T12:00:00Z";

function capture(over: Partial<StagedCaptureSummary> = {}): StagedCaptureSummary {
  return {
    base: "2026-09-20 1000 Figma",
    vaultId: "v1",
    sourceTitle: "Figma — Design system",
    durationMs: 60_000,
    outputDurationMs: 60_000,
    recordedAt: "2026-09-20T10:00:00Z",
    width: 1920,
    height: 1080,
    edited: false,
    recovered: false,
    ...over,
  };
}

function list(captures: StagedCaptureSummary[], busyBase: string | null = null) {
  return mount(StagedCaptureList, { props: { captures, busyBase } });
}

const row = (base: string) => `[data-testid="staged-row-${base}"]`;
const resume = (base: string) => `[data-testid="staged-resume-${base}"]`;
const discard = (base: string) => `[data-testid="staged-discard-${base}"]`;

describe("relativeAgeLabel", () => {
  const now = Date.parse(NOW);

  it("counts in the largest unit that fits", () => {
    expect(relativeAgeLabel("2026-09-20T11:59:30Z", now)).toBe("just now");
    expect(relativeAgeLabel("2026-09-20T11:55:00Z", now)).toBe("5m ago");
    expect(relativeAgeLabel("2026-09-20T09:00:00Z", now)).toBe("3h ago");
    expect(relativeAgeLabel("2026-09-18T12:00:00Z", now)).toBe("2d ago");
  });

  // The sidecar is hand-editable, so `recordedAt` is not a guarantee. A
  // naive subtraction renders the literal string "NaN ago", which reads as a
  // corrupted recording rather than as an unknown timestamp.
  it("says nothing at all about an unreadable timestamp", () => {
    expect(relativeAgeLabel("whenever", now)).toBe("");
  });

  // A clock that went backwards (a resync, a DST jump) must not report a
  // recording made in the future.
  it("clamps a timestamp from the future to just now", () => {
    expect(relativeAgeLabel("2026-09-21T12:00:00Z", now)).toBe("just now");
  });
});

describe("StagedCaptureList", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date(NOW));
  });
  afterEach(() => vi.useRealTimers());

  it("names each capture's source, its length and how long ago it was recorded", () => {
    const w = list([capture()]);
    const text = w.get(row("2026-09-20 1000 Figma")).text();
    expect(text).toContain("Figma — Design system");
    expect(text).toContain("1:00");
    expect(text).toContain("2h ago");
  });

  // The list is how a user finds work they abandoned. A capture carrying an
  // edit must say so, or they cannot tell which one they were part-way
  // through — and the number they are deciding about is what the export will
  // PRODUCE, not what the camera recorded.
  it("marks an edited capture and shows the length it will export to", () => {
    const w = list([capture({ edited: true, durationMs: 60_000, outputDurationMs: 20_000 })]);
    expect(w.text()).toContain("edited");
    expect(w.text()).toContain("0:20");
  });

  // The paired negative. Without it a row that prints "edited" unconditionally
  // — or that shows the output duration twice — passes the test above while
  // telling the user nothing.
  it("says nothing about an edit for a capture that carries none", () => {
    const w = list([capture({ edited: false, durationMs: 60_000, outputDurationMs: 60_000 })]);
    expect(w.text()).not.toContain("edited");
  });

  // The recorded length and the age share one line. Either can be absent —
  // an unedited capture has nothing to say about its recorded length, and a
  // hand-edited `recordedAt` has no age at all — so a line built by
  // concatenation leaves a stranded separator saying nothing.
  it("leaves no stranded separator when one half of the detail line is missing", () => {
    const w = list([capture({ recordedAt: "whenever" })]);
    expect(w.get(row("2026-09-20 1000 Figma")).text()).not.toContain("·");
  });

  it("emits resume for the row that was clicked, not the first one", async () => {
    const w = list([capture(), capture({ base: "other", sourceTitle: "Terminal" })]);
    await w.get(resume("other")).trigger("click");
    expect(w.emitted("resume")).toEqual([["other"]]);
  });

  // Discard is irreversible and destroys the only copy of a recording
  // (spec §10). One click must not delete anything.
  it("requires a second click to discard a staged capture", async () => {
    const w = list([capture()]);
    await w.get(discard("2026-09-20 1000 Figma")).trigger("click");
    expect(w.emitted("discard")).toBeUndefined();
    await w.get(discard("2026-09-20 1000 Figma")).trigger("click");
    expect(w.emitted("discard")).toEqual([["2026-09-20 1000 Figma"]]);
  });

  // An armed confirm that cannot be disarmed is a trap: the button deletes
  // the recording on the next stray click, for the rest of the view's life.
  it("lets an armed discard be backed out of", async () => {
    const w = list([capture()]);
    const base = "2026-09-20 1000 Figma";
    await w.get(discard(base)).trigger("click");
    await w.get(`[data-testid="staged-keep-${base}"]`).trigger("click");
    expect(w.find(`[data-testid="staged-keep-${base}"]`).exists()).toBe(false);
    await w.get(discard(base)).trigger("click");
    expect(w.emitted("discard")).toBeUndefined();
  });

  // A single shared `armed` flag arms EVERY row's discard at once, so the
  // next click on any other row deletes a recording the user never confirmed.
  it("arms only the row that was clicked", async () => {
    const w = list([capture(), capture({ base: "other", sourceTitle: "Terminal" })]);
    await w.get(discard("2026-09-20 1000 Figma")).trigger("click");
    expect(w.find('[data-testid="staged-keep-other"]').exists()).toBe(false);
    await w.get(discard("other")).trigger("click");
    expect(w.emitted("discard")).toBeUndefined();
  });

  it("disables both actions on the row whose write is in flight, and only that row", () => {
    const w = list([capture(), capture({ base: "other", sourceTitle: "Terminal" })], "other");
    expect(w.get(resume("other")).attributes("disabled")).toBeDefined();
    expect(w.get(discard("other")).attributes("disabled")).toBeDefined();
    expect(w.get(resume("2026-09-20 1000 Figma")).attributes("disabled")).toBeUndefined();
    expect(w.get(discard("2026-09-20 1000 Figma")).attributes("disabled")).toBeUndefined();
  });

  // REGRESSION (fix wave): a capture rebuilt by `screen_recovery` after an
  // interrupted session knows neither its vault nor its duration, so it
  // rendered as "edited · 0:00" beside a source title that was only its own
  // base name — two falsehoods — while the ONE fact the user needs (that it
  // was recovered, and cannot be saved) was dropped by the DTO.
  it("says a recovered capture is recovered rather than claiming an edit", () => {
    const w = list([capture({ recovered: true, durationMs: 0, outputDurationMs: 0 })]);
    const text = w.get(row("2026-09-20 1000 Figma")).text();
    expect(text).toContain("recovered");
    expect(text).not.toContain("edited");
    // ...and it does not claim a length it does not know.
    expect(text).not.toContain("0:00");
    expect(text).toContain("length unknown");
  });

  // Save refuses a recovered capture outright (`export_worker::prepare`:
  // it carries no vault id), and the editor it would open has a zero-length
  // timeline. Offering Resume is offering a dead end.
  it("does not offer to resume a capture that can never be saved", () => {
    const w = list([capture({ recovered: true, durationMs: 0, outputDurationMs: 0 })]);
    expect(w.find(resume("2026-09-20 1000 Figma")).exists()).toBe(false);
    // Discard is still offered — it is the only thing left to do with it.
    expect(w.find(discard("2026-09-20 1000 Figma")).exists()).toBe(true);
  });

  // The paired negative: an ORDINARY capture keeps Resume and says nothing
  // about recovery. Without it, a component that hid Resume for every row
  // and printed "recovered" on all of them passes both tests above.
  it("keeps resume and says nothing about recovery for an ordinary capture", () => {
    const w = list([capture()]);
    expect(w.find(resume("2026-09-20 1000 Figma")).exists()).toBe(true);
    expect(w.get(row("2026-09-20 1000 Figma")).text()).not.toContain("recovered");
  });

  // FIX (recovered-capture honesty): a crash-recovered capture is a REAL
  // recording — `screen_recovery` promotes a `.part` only once `mp4_boxes`
  // confirms it holds footage — but the row offered it as unknown-length with
  // a two-click permanent Discard as its only action, and said nothing about
  // the playable file. The one honest sentence in the feature
  // (`export_worker::mod.rs`'s recovered-note body) sits behind a Save button
  // this row never renders. Without this line the user's only readable option
  // is to destroy the recording.
  it("tells the user a recovered capture's video is on disk, and where", () => {
    const w = list([capture({ recovered: true, durationMs: 0, outputDurationMs: 0 })]);
    const text = w.get(row("2026-09-20 1000 Figma")).text();
    // The FACT: the file survived and plays.
    expect(text).toContain("still on disk");
    expect(text).toContain("plays");
    // The PLACE: the staging folder, and the file's own name inside it. The
    // folder alone leaves the user reading a directory of bases they cannot
    // match to this row.
    expect(text).toContain("%LOCALAPPDATA%\\com.vaultbuddy.desktop\\screen-captures");
    expect(text).toContain("2026-09-20 1000 Figma.mp4");
  });

  // The path has to be SELECTABLE — there is no IPC command that opens the
  // staging folder (`open_logs_folder` reveals its sibling), so copying the
  // text is the only way to reach the file from here. A truncating,
  // unselectable span reads as decoration.
  it("renders the recovered capture's path as selectable text", () => {
    const w = list([capture({ recovered: true })]);
    const path = w.get('[data-testid="staged-path-2026-09-20 1000 Figma"]');
    expect(path.classes()).toContain("select-all");
  });

  // The paired negative: an ordinary capture can be saved from the editor, so
  // naming a staging path there would advertise an internal folder as the
  // place its recording lives.
  it("says nothing about the staging folder for a capture that can still be saved", () => {
    const w = list([capture()]);
    expect(w.get(row("2026-09-20 1000 Figma")).text()).not.toContain("screen-captures");
    expect(w.find('[data-testid="staged-path-2026-09-20 1000 Figma"]').exists()).toBe(false);
  });

  // FIX (unpinned disarm): deleting the disarm-on-relist watch entirely left
  // every other case in this file green. An armed Discard that survives the
  // list being re-read underneath it deletes whichever recording now occupies
  // that base on the next single click.
  it("disarms a row when the list is re-read underneath it", async () => {
    const w = list([capture()]);
    const base = "2026-09-20 1000 Figma";
    await w.get(discard(base)).trigger("click");
    expect(w.find(`[data-testid="staged-keep-${base}"]`).exists()).toBe(true);
    // A NEW array with equal contents — exactly what `loadStaged` assigns.
    await w.setProps({ captures: [capture()] });
    expect(w.find(`[data-testid="staged-keep-${base}"]`).exists()).toBe(false);
    await w.get(discard(base)).trigger("click");
    expect(w.emitted("discard")).toBeUndefined();
  });

  // The refusal case the watch's comment CLAIMED to cover and could not: a
  // refused discard leaves the very same array on screen ("the list on screen
  // is still true"), so nothing re-keys and the row stayed armed. The picker
  // bumps `disarmNonce` instead.
  it("disarms a row when the picker bumps the disarm nonce", async () => {
    const w = list([capture()]);
    const base = "2026-09-20 1000 Figma";
    await w.get(discard(base)).trigger("click");
    await w.setProps({ disarmNonce: 1 });
    expect(w.find(`[data-testid="staged-keep-${base}"]`).exists()).toBe(false);
    await w.get(discard(base)).trigger("click");
    expect(w.emitted("discard")).toBeUndefined();
  });

  // The picker's own heading, tabs and Start button must not be pushed down
  // by an empty block on the overwhelmingly common path where nothing is
  // staged at all.
  it("renders nothing at all when there are no staged captures", () => {
    const w = list([]);
    expect(w.find('[data-testid="staged-list"]').exists()).toBe(false);
    expect(w.text()).toBe("");
  });
});
