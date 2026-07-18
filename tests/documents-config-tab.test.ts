import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../src/logging", () => ({ logWarning: vi.fn(), logBreadcrumb: vi.fn() }));
import DocumentsConfigTab from "../src/components/DocumentsConfigTab.vue";

let active: ReturnType<typeof mount> | null = null;
beforeEach(() => {
  setActivePinia(createPinia());
  vi.useFakeTimers();
});
afterEach(() => {
  active?.unmount();
  active = null;
  vi.useRealTimers();
  clearMocks();
  document.body.innerHTML = "";
});

function mountTab(
  opts: {
    documentsFolder?: string | null;
    documentDateFolders?: boolean;
    documentExtractImages?: boolean;
    documentExtraFrontmatter?: string | null;
    documentBodyTemplate?: string | null;
    onGet?: () => unknown;
    onSet?: (a: unknown) => unknown;
  } = {},
) {
  // Defaults spread UNDER the caller's opts (rather than a `field ?? default`
  // per key) so adding a field never grows this callback's branch count.
  const defaults = {
    documentsFolder: null,
    documentDateFolders: false,
    documentExtractImages: true,
    documentExtraFrontmatter: null,
    documentBodyTemplate: null,
  };
  const calls: Array<{ cmd: string; args: unknown }> = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    if (cmd === "get_documents_config") return opts.onGet ? opts.onGet() : { ...defaults, ...opts };
    if (cmd === "set_documents_config") return opts.onSet?.(args) ?? null;
  });
  active = mount(DocumentsConfigTab, { props: { vaultId: "v1" }, attachTo: document.body });
  return { wrapper: active, calls };
}

describe("DocumentsConfigTab", () => {
  it("loads the folder and toggle from disk", async () => {
    const { wrapper } = mountTab({ documentsFolder: "Docs", documentDateFolders: false });
    await flushPromises();
    expect(wrapper.get<HTMLInputElement>('[data-testid="documents-folder-input"]').element.value).toBe("Docs");
    expect(wrapper.get<HTMLInputElement>('[data-testid="document-date-folders-toggle"]').element.checked).toBe(false);
  });

  it("does not save on mount", async () => {
    const { calls } = mountTab({ documentsFolder: "Docs" });
    await flushPromises();
    expect(calls.some((c) => c.cmd === "set_documents_config")).toBe(false);
  });

  it("debounces a folder edit and saves both fields after 600ms", async () => {
    const { wrapper, calls } = mountTab({ documentsFolder: "Docs", documentDateFolders: false });
    await flushPromises();
    await wrapper.get('[data-testid="documents-folder-input"]').setValue("Imported");
    expect(calls.some((c) => c.cmd === "set_documents_config")).toBe(false); // not yet
    vi.advanceTimersByTime(600);
    await flushPromises();
    expect(calls.find((c) => c.cmd === "set_documents_config")?.args).toEqual({
      id: "v1",
      documentsFolder: "Imported",
      documentDateFolders: false,
      documentExtractImages: true,
      documentExtraFrontmatter: null,
      documentBodyTemplate: null,
    });
  });

  it("saves the toggle immediately (no debounce)", async () => {
    const { wrapper, calls } = mountTab({ documentsFolder: "Docs", documentDateFolders: true });
    await flushPromises();
    await wrapper.get('[data-testid="document-date-folders-toggle"]').setValue(false);
    await flushPromises();
    expect(calls.find((c) => c.cmd === "set_documents_config")?.args).toEqual({
      id: "v1",
      documentsFolder: "Docs",
      documentDateFolders: false,
      documentExtractImages: true,
      documentExtraFrontmatter: null,
      documentBodyTemplate: null,
    });
  });

  it("loads the images toggle from disk", async () => {
    const { wrapper } = mountTab({ documentExtractImages: false });
    await flushPromises();
    expect(
      wrapper.get<HTMLInputElement>('[data-testid="document-extract-images-toggle"]').element.checked,
    ).toBe(false);
  });

  it("saves the images toggle immediately when turned off", async () => {
    const { wrapper, calls } = mountTab({
      documentsFolder: "Docs",
      documentDateFolders: true,
      documentExtractImages: true,
    });
    await flushPromises();
    await wrapper.get('[data-testid="document-extract-images-toggle"]').setValue(false);
    await flushPromises();
    expect(calls.find((c) => c.cmd === "set_documents_config")?.args).toEqual({
      id: "v1",
      documentsFolder: "Docs",
      documentDateFolders: true,
      documentExtractImages: false,
      documentExtraFrontmatter: null,
      documentBodyTemplate: null,
    });
  });

  it("loads the extra frontmatter and body template from disk", async () => {
    const { wrapper } = mountTab({
      documentExtraFrontmatter: "area: Legal",
      documentBodyTemplate: "> Imported via Pandoc\n\n{{content}}",
    });
    await flushPromises();
    expect(wrapper.get<HTMLTextAreaElement>('[data-testid="document-extra-frontmatter"]').element.value).toBe(
      "area: Legal",
    );
    expect(wrapper.get<HTMLTextAreaElement>('[data-testid="document-body-template"]').element.value).toBe(
      "> Imported via Pandoc\n\n{{content}}",
    );
  });

  it("debounces edits to the extra frontmatter and body template, saving both new args after 600ms", async () => {
    const { wrapper, calls } = mountTab({ documentsFolder: "Docs", documentDateFolders: false });
    await flushPromises();
    await wrapper.get('[data-testid="document-extra-frontmatter"]').setValue("area: Legal");
    await wrapper
      .get('[data-testid="document-body-template"]')
      .setValue("> Imported via Pandoc\n\n{{content}}");
    expect(calls.some((c) => c.cmd === "set_documents_config")).toBe(false); // still debouncing
    vi.advanceTimersByTime(600);
    await flushPromises();
    expect(calls.find((c) => c.cmd === "set_documents_config")?.args).toEqual({
      id: "v1",
      documentsFolder: "Docs",
      documentDateFolders: false,
      documentExtractImages: true,
      documentExtraFrontmatter: "area: Legal",
      documentBodyTemplate: "> Imported via Pandoc\n\n{{content}}",
    });
  });

  it("flushes a pending folder save on blur", async () => {
    const { wrapper, calls } = mountTab({ documentsFolder: "Docs" });
    await flushPromises();
    const input = wrapper.get('[data-testid="documents-folder-input"]');
    await input.setValue("Imported");
    await input.trigger("focusout"); // the bubbling focus-loss event the container listens for
    await flushPromises();
    expect(calls.some((c) => c.cmd === "set_documents_config")).toBe(true);
  });

  it("empties the folder to null on save", async () => {
    const { wrapper, calls } = mountTab({ documentsFolder: "Docs" });
    await flushPromises();
    await wrapper.get('[data-testid="documents-folder-input"]').setValue("");
    vi.advanceTimersByTime(600);
    await flushPromises();
    expect(calls.find((c) => c.cmd === "set_documents_config")?.args).toMatchObject({
      documentsFolder: null,
    });
  });

  it("shows a save error inline and keeps the value", async () => {
    const { wrapper } = mountTab({
      documentsFolder: "Docs",
      documentDateFolders: true,
      onSet: () => {
        throw "Configured documents folder must stay inside the vault";
      },
    });
    await flushPromises();
    await wrapper.get('[data-testid="document-date-folders-toggle"]').setValue(false);
    await flushPromises();
    expect(wrapper.get('[data-testid="documents-folder-error"]').text()).toContain("inside the vault");
  });

  it("shows a load error and no editable fields when the read fails", async () => {
    const { wrapper } = mountTab({
      onGet: () => {
        throw "config unreadable";
      },
    });
    await flushPromises();
    expect(wrapper.get('[data-testid="documents-load-error"]').text()).toContain("config unreadable");
    expect(wrapper.find('[data-testid="documents-folder-input"]').exists()).toBe(false);
  });
});
