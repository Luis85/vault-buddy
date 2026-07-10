import { flushPromises, mount } from "@vue/test-utils";
import { afterEach, describe, expect, it } from "vitest";

import TranscriptionSettings from "../src/components/TranscriptionSettings.vue";

const baseValue = {
  transcribe: false,
  transcriptionModel: "small",
  transcriptionLanguage: "",
  transcriptTimestamps: true,
};

// TranscriptionSettings is a controlled component (no persistence of its
// own) — every assertion below is either "renders what the prop says" or
// "emits update:modelValue with a spread-merged object", never internal state.
let active: ReturnType<typeof mount> | null = null;
afterEach(() => {
  active?.unmount();
  active = null;
  // SelectMenu Teleports its popup to <body>; clear it between tests.
  document.body.innerHTML = "";
});

function mountWith(modelValue: typeof baseValue = baseValue) {
  active = mount(TranscriptionSettings, {
    props: { modelValue },
    attachTo: document.body,
  });
  return active;
}

// Open a SelectMenu dropdown and click one of its (Teleported) options.
async function pickOption(
  wrapper: ReturnType<typeof mount>,
  testid: string,
  value: string,
) {
  await wrapper.get(`[data-testid="${testid}"]`).trigger("click");
  (
    document.body.querySelector(`[data-testid="${testid}-option-${value}"]`) as HTMLElement
  ).click();
  await flushPromises();
}

describe("TranscriptionSettings", () => {
  it("reflects the transcribe toggle from modelValue", () => {
    const off = mountWith({ ...baseValue, transcribe: false });
    expect(
      off.get<HTMLInputElement>('[data-testid="transcribe-toggle"]').element.checked,
    ).toBe(false);
    off.unmount();

    const on = mountWith({ ...baseValue, transcribe: true });
    expect(
      on.get<HTMLInputElement>('[data-testid="transcribe-toggle"]').element.checked,
    ).toBe(true);
  });

  it("hides the model/language/timestamps controls while transcribe is off", () => {
    const wrapper = mountWith({ ...baseValue, transcribe: false });
    expect(wrapper.find('[data-testid="transcription-model-select"]').exists()).toBe(false);
    expect(wrapper.find('[data-testid="transcription-language-select"]').exists()).toBe(false);
    expect(wrapper.find('[data-testid="transcript-timestamps-toggle"]').exists()).toBe(false);
  });

  it("shows the model/language/timestamps controls, reflecting modelValue, once transcribe is on", () => {
    const wrapper = mountWith({
      transcribe: true,
      transcriptionModel: "medium",
      transcriptionLanguage: "es",
      transcriptTimestamps: false,
    });
    expect(wrapper.get('[data-testid="transcription-model-select"]').text()).toContain("Medium");
    expect(wrapper.get('[data-testid="transcription-language-select"]').text()).toContain(
      "Spanish",
    );
    expect(
      wrapper.get<HTMLInputElement>('[data-testid="transcript-timestamps-toggle"]').element
        .checked,
    ).toBe(false);
  });

  it("toggling transcribe emits update:modelValue with the merged object and transcribe flipped", async () => {
    const wrapper = mountWith({ ...baseValue, transcribe: false });
    await wrapper.get('[data-testid="transcribe-toggle"]').setValue(true);
    expect(wrapper.emitted("update:modelValue")).toEqual([
      [{ ...baseValue, transcribe: true }],
    ]);
  });

  it("picking a model emits update:modelValue with only transcriptionModel changed", async () => {
    const modelValue = { ...baseValue, transcribe: true };
    const wrapper = mountWith(modelValue);
    await pickOption(wrapper, "transcription-model-select", "medium");
    expect(wrapper.emitted("update:modelValue")).toEqual([
      [{ ...modelValue, transcriptionModel: "medium" }],
    ]);
  });

  it("picking a language emits update:modelValue with only transcriptionLanguage changed", async () => {
    const modelValue = { ...baseValue, transcribe: true };
    const wrapper = mountWith(modelValue);
    await pickOption(wrapper, "transcription-language-select", "es");
    expect(wrapper.emitted("update:modelValue")).toEqual([
      [{ ...modelValue, transcriptionLanguage: "es" }],
    ]);
  });

  it("toggling timestamps emits update:modelValue with only transcriptTimestamps changed", async () => {
    const modelValue = { ...baseValue, transcribe: true, transcriptTimestamps: true };
    const wrapper = mountWith(modelValue);
    await wrapper.get('[data-testid="transcript-timestamps-toggle"]').setValue(false);
    expect(wrapper.emitted("update:modelValue")).toEqual([
      [{ ...modelValue, transcriptTimestamps: false }],
    ]);
  });

  it("uses today's exact unprefixed ids when idPrefix is omitted", () => {
    // Locks the default so two existing consumers (CaptureSettings.vue,
    // RecordMode.vue) that don't pass idPrefix keep rendering identical
    // markup — no id churn from this change.
    const wrapper = mountWith({ ...baseValue, transcribe: true });
    expect(
      wrapper.get('[data-testid="transcribe-toggle"]').attributes("id"),
    ).toBe("capture-transcribe-toggle");
    expect(
      wrapper.get('[data-testid="transcript-timestamps-toggle"]').attributes("id"),
    ).toBe("capture-transcript-timestamps-toggle");
  });

  it("scopes every id/for pair with idPrefix so two instances can't collide (C3)", () => {
    active = mount(TranscriptionSettings, {
      props: { modelValue: { ...baseValue, transcribe: true }, idPrefix: "record-" },
      attachTo: document.body,
    });
    const wrapper = active;
    const toggle = wrapper.get('[data-testid="transcribe-toggle"]');
    expect(toggle.attributes("id")).toBe("record-capture-transcribe-toggle");
    const timestamps = wrapper.get('[data-testid="transcript-timestamps-toggle"]');
    expect(timestamps.attributes("id")).toBe("record-capture-transcript-timestamps-toggle");
    // Each label's `for` must match its control's scoped `id`, not the
    // unprefixed default — otherwise clicking the label would target nothing.
    // `.get()` itself throws (a clear failure) if the selector matches nothing.
    wrapper.get(`label[for="${toggle.attributes("id")}"]`);
    wrapper.get(`label[for="${timestamps.attributes("id")}"]`);
  });

  it("scopes the SelectMenu-backed id/for pairs with idPrefix too (C3)", () => {
    // The C3 test above only covers the two native <input> pairs; the model
    // and language controls are SelectMenu.vue instances, which forward
    // `:id` to their root button — that forwarding path needs its own
    // coverage or a regression there would slip through silently.
    active = mount(TranscriptionSettings, {
      props: { modelValue: { ...baseValue, transcribe: true }, idPrefix: "record-" },
      attachTo: document.body,
    });
    const wrapper = active;
    const model = wrapper.get('[data-testid="transcription-model-select"]');
    expect(model.attributes("id")).toBe("record-capture-transcription-model");
    const language = wrapper.get('[data-testid="transcription-language-select"]');
    expect(language.attributes("id")).toBe("record-capture-transcription-language");
    // `.get()` itself throws (a clear failure) if the selector matches nothing.
    wrapper.get(`label[for="${model.attributes("id")}"]`);
    wrapper.get(`label[for="${language.attributes("id")}"]`);
  });

  it("never mutates the modelValue prop object", async () => {
    const modelValue = { ...baseValue, transcribe: true };
    const frozen = Object.freeze({ ...modelValue });
    const wrapper = mountWith(frozen as typeof modelValue);
    // Would throw under freeze if the component mutated the prop in place.
    await expect(
      wrapper.get('[data-testid="transcript-timestamps-toggle"]').setValue(false),
    ).resolves.not.toThrow();
    expect(wrapper.emitted("update:modelValue")).toEqual([
      [{ ...frozen, transcriptTimestamps: false }],
    ]);
  });
});
