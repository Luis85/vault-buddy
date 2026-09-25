/**
 * The open project's Rendered Products as the webview knows them (Task 47;
 * F-41; SCREENS 09): Rust's product ledger (`editor_get_products`), read
 * whenever the library shows and again whenever a render's `complete`
 * terminal names a new `productId` (`editorJobs.install`).
 *
 * Nothing here is ever edited locally: a product is immutable once the
 * ledger records it (ADR invariant 5). The one verb, Restore this edit,
 * goes through `editorProject.restoreProduct` — an ordinary acknowledged
 * edit (a new revision, one undo step), so the product itself is untouched.
 *
 * The list is scoped to the session it was read for: after a session
 * switch `current` is empty until the next read, never the previous
 * project's products. A read that lands after a newer one was asked for,
 * or after the session changed, is dropped.
 */
import { defineStore } from "pinia";

import type { EditorError, ProductDto } from "../editorTypes";
import { toEditorError, useEditorProjectStore } from "./editorProject";

export const useEditorProductsStore = defineStore("editorProducts", {
  state: () => ({
    /** The session `products` was read for. */
    sessionId: null as string | null,
    products: [] as ProductDto[],
    /** The last failed read or refused restore. */
    error: null as EditorError | null,
    /** Bumped per read; only the newest read installs. */
    ticket: 0,
  }),
  getters: {
    /** The open session's products, oldest first (the ledger's order). */
    current(state): ProductDto[] {
      return state.sessionId !== null && state.sessionId === useEditorProjectStore().sessionId
        ? state.products
        : [];
    },
  },
  actions: {
    /** Re-read the ledger for the open session. */
    async refresh(): Promise<void> {
      const project = useEditorProjectStore();
      const sessionId = project.sessionId;
      if (!sessionId) return;
      this.ticket += 1;
      const ticket = this.ticket;
      try {
        const products = await project.port.getProducts(sessionId);
        if (ticket !== this.ticket || project.sessionId !== sessionId) return;
        this.sessionId = sessionId;
        this.products = products;
        this.error = null;
      } catch (e) {
        if (ticket === this.ticket) this.error = toEditorError(e);
      }
    },
    /** Make `productId`'s frozen edit the live one. Resolves whether Rust
     * acknowledged it; a refusal is kept in `error` for the library. */
    async restore(productId: string): Promise<boolean> {
      const project = useEditorProjectStore();
      const ok = await project.restoreProduct(productId);
      this.error = ok ? null : project.lastError;
      return ok;
    },
  },
});
