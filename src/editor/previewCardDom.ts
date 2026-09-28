/**
 * Applies `CardLayer`s (`previewCardLayer.ts`) to the preview stage as
 * plain styled `<div>`s (Task 33; F-37) — the DOM half of card rendering,
 * split out of `previewController.ts` (which was already within a few
 * lines of the frontend's 500-line cap) so that file's own growth per
 * layer kind stays a few lines: construct one `CardLayerDom`, call
 * `apply()` from the same place `PreviewController.apply()` lays out its
 * media layers, and `destroy()` on teardown.
 *
 * Unlike `PreviewController`'s media slots, card nodes are NOT pooled —
 * `MAX_ELEMENTS` exists to bound expensive decoder/audio-graph resources a
 * `<video>`/`<audio>` element owns, and a card `<div>` owns neither, so the
 * pooling machinery would add complexity without protecting anything.
 */
import type { CardLayer } from "./previewCardLayer";

export class CardLayerDom {
  private readonly container: HTMLElement;
  private readonly nodes = new Map<string, HTMLDivElement>();

  constructor(container: HTMLElement) {
    this.container = container;
  }

  /** Adds/updates/removes nodes so the DOM matches `layers` exactly —
   * `PreviewController.apply()`'s own keep-set diff, restated for cards. */
  apply(layers: readonly CardLayer[]): void {
    const keep = new Set(layers.map((l) => l.clipId));
    for (const [clipId, node] of this.nodes) {
      if (!keep.has(clipId)) {
        node.remove();
        this.nodes.delete(clipId);
      }
    }
    for (const layer of layers) this.show(layer);
  }

  destroy(): void {
    for (const node of this.nodes.values()) node.remove();
    this.nodes.clear();
  }

  private show(layer: CardLayer): void {
    let node = this.nodes.get(layer.clipId);
    if (!node) {
      node = this.build();
      this.nodes.set(layer.clipId, node);
      this.container.appendChild(node);
    }
    this.place(node, layer);
  }

  private build(): HTMLDivElement {
    const node = document.createElement("div");
    node.dataset.previewLayer = "card";
    const style = node.style;
    style.position = "absolute";
    style.display = "flex";
    style.flexDirection = "column";
    style.alignItems = "center";
    style.justifyContent = "center";
    style.textAlign = "center";
    style.boxSizing = "border-box";
    style.overflow = "hidden";
    style.borderStyle = "solid";
    style.borderWidth = "2px";

    const title = document.createElement("div");
    title.dataset.cardTitle = "";
    const subtitle = document.createElement("div");
    subtitle.dataset.cardSubtitle = "";
    node.append(title, subtitle);
    return node;
  }

  private place(node: HTMLDivElement, layer: CardLayer): void {
    const style = node.style;
    style.zIndex = String(layer.z);
    style.opacity = String(layer.opacity);
    style.left = `${layer.box.left}px`;
    style.top = `${layer.box.top}px`;
    style.width = `${layer.box.width}px`;
    style.height = `${layer.box.height}px`;
    style.background = layer.background;
    style.borderColor = layer.accent;

    const title = node.querySelector<HTMLDivElement>("[data-card-title]")!;
    title.textContent = layer.title;
    title.style.color = layer.foreground;
    const subtitle = node.querySelector<HTMLDivElement>("[data-card-subtitle]")!;
    subtitle.textContent = layer.subtitle;
    subtitle.style.color = layer.accent;
  }
}
