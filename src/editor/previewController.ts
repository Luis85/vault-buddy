/**
 * The editor's layered preview (Task 22; F-04, F-14, F-25; NATIVE-MEDIA.md §
 * Preview architecture: "A `PreviewController` owns decoding/playback
 * resources outside deep Vue reactivity … Cancel obsolete seeks and
 * distinguish a still-preview seek from continuous playback").
 *
 * **A plain class, NEVER stored in Pinia or wrapped in `ref`/`reactive`.**
 * It owns live media elements, an `AudioContext` and a rAF loop; a reactive
 * proxy around any of those breaks identity checks and native private
 * fields (the `markRaw(Update)` lesson `stores/updates.ts` records), and a
 * store would rebuild reactive state at frame rate. Vue sees only what the
 * controller reports through `onTime` (10 Hz) and `onPlayingChange`.
 *
 * Owns:
 * - one media element per ACTIVE clip — `<video>` for a video layer,
 *   `<img>` for an image, `<audio>` for an audio layer — POOLED: released
 *   elements are kept for reuse, and at most `MAX_ELEMENTS` (8) exist at
 *   once. Layers beyond that are not shown (top-most first), and the drop
 *   is logged once per overflow episode rather than silently;
 * - an `AudioContext` with one `GainNode` per media element for MONITORING
 *   (the workspace's `monitor_muted` and the transport's volume). Local
 *   only: nothing here ever sends an editor command;
 * - the clock: `play()` runs a rAF loop from a wall-clock anchor, `pause()`
 *   stops it, `seek(ms)` cancels any older pending seek (a token) so a
 *   scrub never lands on a stale frame.
 *
 * The frontend never constructs a media path: `resolveUrl(assetId)` is the
 * caller's (`PreviewSurface.vue` → `editor_media_url` → `convertFileSrc`).
 */
import type { Project } from "../editorTypes";
import { logWarning } from "../logging";
import type { Size } from "./previewGeometry";
import type { LayerKind, MonitorState, PreviewLayer } from "./previewLayers";
import { computeLayers } from "./previewLayers";
import { clipOutputEnd } from "./timeMap";

export const MAX_ELEMENTS = 8;
/** `timeupdate` cadence to the workspace playhead — low-rate on purpose. */
const TIME_EMIT_MS = 100;
/** A seek whose media never reports `seeked` (no source yet, a decode
 * stall) must not pin a caller forever. */
const SEEK_TIMEOUT_MS = 2000;
/** Playback drift tolerated before a playing element is re-seeked. */
const DRIFT_S = 0.25;
/** Below a millisecond two still-preview targets are the same instant. */
const STILL_EPSILON_S = 0.0005;

/** The slice of Web Audio the controller uses — injectable for tests. */
export interface GainLike {
  gain: { value: number };
  connect(target: unknown): unknown;
}
export interface AudioContextLike {
  readonly destination: unknown;
  createGain(): GainLike;
  createMediaElementSource(el: HTMLMediaElement): { connect(target: unknown): unknown };
  resume?(): Promise<void>;
  close?(): Promise<void>;
}

export interface PreviewControllerDeps {
  /** Where the controller mounts its media elements (the stage). */
  container: HTMLElement;
  /** Absolute media URL for a registered asset, or `null` when it cannot
   * be shown (missing, unauthorized). */
  resolveUrl: (assetId: string) => Promise<string | null>;
  onTime?: (ms: number) => void;
  onPlayingChange?: (playing: boolean) => void;
  createAudioContext?: () => AudioContextLike | null;
  now?: () => number;
  requestFrame?: (cb: () => void) => number;
  cancelFrame?: (id: number) => void;
}

type MediaEl = HTMLVideoElement | HTMLImageElement | HTMLAudioElement;

interface Slot {
  kind: LayerKind;
  el: MediaEl;
  gain: GainLike | null;
  assetId: string | null;
}

const TAG: Record<LayerKind, "video" | "img" | "audio"> = { video: "video", image: "img", audio: "audio" };

function isMedia(el: MediaEl): el is HTMLVideoElement | HTMLAudioElement {
  return !(el instanceof HTMLImageElement);
}

function defaultAudioContext(): AudioContextLike | null {
  const Ctor = (globalThis as { AudioContext?: new () => AudioContextLike }).AudioContext;
  return typeof Ctor === "function" ? new Ctor() : null;
}

/** Wait for one element's `seeked`, or give up after `SEEK_TIMEOUT_MS`. */
function waitSeeked(el: HTMLMediaElement): Promise<void> {
  return new Promise((resolve) => {
    const done = () => {
      clearTimeout(timer);
      el.removeEventListener("seeked", done);
      resolve();
    };
    const timer = setTimeout(done, SEEK_TIMEOUT_MS);
    el.addEventListener("seeked", done);
  });
}

export class PreviewController {
  private readonly deps: PreviewControllerDeps;
  private readonly active = new Map<string, Slot>();
  private readonly free: Slot[] = [];
  private readonly urls = new Map<string, string | null>();
  private audio: AudioContextLike | null | undefined = undefined;
  private project: Project | null = null;
  private stage: Size = { width: 0, height: 0 };
  private monitor: MonitorState = { muted: false, volume: 1 };
  private rate = 1;
  private time = 0;
  private isPlaying = false;
  private frame: number | null = null;
  private anchorWall = 0;
  private anchorTime = 0;
  private lastEmit = -Infinity;
  private seekToken = 0;
  private cancelPending: (() => void) | null = null;
  private overflowLogged = false;

  constructor(deps: PreviewControllerDeps) {
    this.deps = deps;
  }

  get timeMs(): number {
    return this.time;
  }
  get playing(): boolean {
    return this.isPlaying;
  }

  setStage(size: Size): void {
    this.stage = size;
    this.relayout();
  }
  setMonitor(monitor: MonitorState): void {
    this.monitor = monitor;
    this.relayout();
  }
  setRate(rate: number): void {
    this.reanchor();
    this.rate = rate;
    this.relayout();
  }
  setProject(project: Project | null): void {
    this.project = project;
    this.time = Math.min(this.time, this.duration());
    this.relayout();
  }

  /** Lay out `project` at `t` and apply it to the media elements. Returns
   * the layers actually shown (top-most first, at most `MAX_ELEMENTS`). */
  layout(project: Project, t: number): PreviewLayer[] {
    this.project = project;
    this.time = t;
    return this.apply(false);
  }

  /** Move to `ms` (playing or still), cancelling any older pending seek.
   * Resolves `true` once
   * every active element reports `seeked`, `false` if a newer seek
   * superseded this one first. */
  seek(ms: number): Promise<boolean> {
    const token = ++this.seekToken;
    this.cancelPending?.();
    this.anchorAt(Math.max(0, Math.min(ms, this.duration())));
    this.apply(true);
    const waits = [...this.active.values()]
      .map((slot) => slot.el)
      .filter(isMedia)
      .map(waitSeeked);
    // No `onTime` echo: a seek is REQUESTED by whoever owns the playhead
    // (the timeline, the ruler), and writing the controller's own clamped
    // value back would overwrite a playhead the user just set.
    return new Promise<boolean>((resolve) => {
      this.cancelPending = () => resolve(false);
      void Promise.all(waits).then(() => {
        if (token !== this.seekToken) return;
        this.cancelPending = null;
        resolve(true);
      });
    });
  }

  play(): void {
    if (this.isPlaying || !this.project) return;
    this.anchorAt(this.time >= this.duration() ? 0 : this.time);
    this.isPlaying = true;
    void this.audioContext()?.resume?.();
    this.apply(true);
    this.deps.onPlayingChange?.(true);
    this.schedule();
  }

  pause(): void {
    if (!this.isPlaying) return;
    this.reanchor();
    this.isPlaying = false;
    if (this.frame !== null) (this.deps.cancelFrame ?? cancelAnimationFrame)(this.frame);
    this.frame = null;
    for (const slot of this.active.values()) if (isMedia(slot.el)) slot.el.pause();
    this.emitTime(true);
    this.deps.onPlayingChange?.(false);
  }

  destroy(): void {
    this.pause();
    this.cancelPending?.();
    for (const slot of [...this.active.values(), ...this.free]) this.teardown(slot);
    this.active.clear();
    this.free.length = 0;
    void this.audio?.close?.();
  }

  // ---- internals -----------------------------------------------------------

  private now(): number {
    return (this.deps.now ?? (() => performance.now()))();
  }

  private duration(): number {
    const clips = this.project?.clips ?? [];
    return clips.reduce((max, c) => Math.max(max, clipOutputEnd({ ...c, speed: c.speed ?? 1 })), 0);
  }

  /** Re-anchor the wall clock at the current time, so a rate change or a
   * seek during playback continues from where the clock is NOW. */
  /** Clamped to the project's end: a pause landing after the last frame's
   * tick must not report a time past the timeline. */
  private reanchor(): void {
    this.anchorAt(this.isPlaying ? Math.min(this.clockTime(), this.duration()) : this.time);
  }

  private anchorAt(t: number): void {
    this.time = t;
    this.anchorWall = this.now();
    this.anchorTime = t;
  }

  private clockTime(): number {
    return this.anchorTime + (this.now() - this.anchorWall) * this.rate;
  }

  private schedule(): void {
    const raf = this.deps.requestFrame ?? requestAnimationFrame;
    this.frame = raf(() => this.tick());
  }

  private tick(): void {
    if (!this.isPlaying) return;
    const end = this.duration();
    this.time = Math.min(this.clockTime(), end);
    this.apply(false);
    this.emitTime(false);
    if (this.time >= end) this.pause();
    else this.schedule();
  }

  private emitTime(force: boolean): void {
    const now = this.now();
    if (!force && now - this.lastEmit < TIME_EMIT_MS) return;
    this.lastEmit = now;
    this.deps.onTime?.(Math.round(this.time));
  }

  private relayout(): void {
    if (this.project) this.apply(false);
  }

  private apply(forceSeek: boolean): PreviewLayer[] {
    if (!this.project) return [];
    const all = computeLayers(this.project, this.time, this.stage, this.monitor);
    const shown = all.slice(0, MAX_ELEMENTS);
    this.noteOverflow(all.length);
    const keep = new Set(shown.map((l) => l.clipId));
    for (const [clipId, slot] of this.active) {
      if (!keep.has(clipId)) this.release(clipId, slot);
    }
    for (const layer of shown) this.show(layer, forceSeek);
    return shown;
  }

  private noteOverflow(count: number): void {
    if (count <= MAX_ELEMENTS) {
      this.overflowLogged = false;
      return;
    }
    if (this.overflowLogged) return;
    this.overflowLogged = true;
    logWarning(`preview: ${count} active layers, showing the top ${MAX_ELEMENTS}`);
  }

  private show(layer: PreviewLayer, forceSeek: boolean): void {
    let slot = this.active.get(layer.clipId);
    if (slot && slot.kind !== layer.kind) {
      this.release(layer.clipId, slot);
      slot = undefined;
    }
    slot ??= this.acquire(layer.kind);
    this.active.set(layer.clipId, slot);
    this.bindSource(slot, layer.assetId);
    this.place(slot.el, layer);
    this.mix(slot, layer);
    if (isMedia(slot.el)) this.syncMedia(slot.el, layer, forceSeek);
  }

  private acquire(kind: LayerKind): Slot {
    const i = this.free.findIndex((s) => s.kind === kind);
    if (i >= 0) {
      const [slot] = this.free.splice(i, 1);
      this.deps.container.appendChild(slot.el);
      return slot;
    }
    const el = document.createElement(TAG[kind]) as MediaEl;
    el.dataset.previewLayer = kind;
    el.style.position = "absolute";
    if (isMedia(el)) {
      // The asset protocol answers with the window's own origin in
      // Access-Control-Allow-Origin, so a CORS-mode element is readable by
      // Web Audio — without it a MediaElementSource outputs silence.
      el.crossOrigin = "anonymous";
      el.preload = "auto";
      if (el instanceof HTMLVideoElement) el.playsInline = true;
    }
    if (kind === "audio") el.style.display = "none";
    this.deps.container.appendChild(el);
    return { kind, el, gain: isMedia(el) ? this.connect(el) : null, assetId: null };
  }

  private release(clipId: string, slot: Slot): void {
    this.active.delete(clipId);
    if (isMedia(slot.el)) slot.el.pause();
    slot.el.remove();
    if (this.free.length + this.active.size < MAX_ELEMENTS) this.free.push(slot);
    else this.teardown(slot);
  }

  private teardown(slot: Slot): void {
    if (isMedia(slot.el)) {
      slot.el.pause();
      slot.el.removeAttribute("src");
    }
    slot.el.remove();
  }

  private audioContext(): AudioContextLike | null {
    if (this.audio === undefined) {
      try {
        this.audio = (this.deps.createAudioContext ?? defaultAudioContext)();
      } catch (e) {
        logWarning(`preview: no AudioContext, monitoring falls back to element volume (${String(e)})`);
        this.audio = null;
      }
    }
    return this.audio;
  }

  private connect(el: HTMLMediaElement): GainLike | null {
    const ctx = this.audioContext();
    if (!ctx) return null;
    try {
      const gain = ctx.createGain();
      ctx.createMediaElementSource(el).connect(gain);
      gain.connect(ctx.destination);
      return gain;
    } catch (e) {
      logWarning(`preview: could not route a layer through Web Audio (${String(e)})`);
      return null;
    }
  }

  private bindSource(slot: Slot, assetId: string): void {
    if (slot.assetId === assetId) return;
    slot.assetId = assetId;
    slot.el.removeAttribute("src");
    const known = this.urls.get(assetId);
    if (known !== undefined) {
      if (known) slot.el.src = known;
      return;
    }
    void this.deps.resolveUrl(assetId).then((url) => {
      this.urls.set(assetId, url);
      if (url && slot.assetId === assetId) slot.el.src = url;
    });
  }

  private place(el: MediaEl, layer: PreviewLayer): void {
    const style = el.style;
    style.zIndex = String(layer.z);
    style.opacity = String(layer.opacity);
    if (!layer.box) return;
    style.left = `${layer.box.left}px`;
    style.top = `${layer.box.top}px`;
    style.width = `${layer.box.width}px`;
    style.height = `${layer.box.height}px`;
    style.objectFit = layer.fit;
  }

  private mix(slot: Slot, layer: PreviewLayer): void {
    if (!isMedia(slot.el)) return;
    if (slot.gain) {
      slot.el.muted = false;
      slot.gain.gain.value = layer.gain;
    } else {
      slot.el.muted = layer.muted;
      slot.el.volume = Math.max(0, Math.min(1, layer.gain));
    }
  }

  private syncMedia(el: HTMLVideoElement | HTMLAudioElement, layer: PreviewLayer, forceSeek: boolean): void {
    const target = layer.sourceMs / 1000;
    el.playbackRate = layer.speed * this.rate;
    // Playing: re-seek only past the drift tolerance (a seek per frame
    // would stall decoding). Still: re-seek whenever the frame differs, but
    // never re-set the same instant (a relayout on resize is not a seek).
    const tolerance = this.isPlaying ? DRIFT_S : STILL_EPSILON_S;
    if (forceSeek || Math.abs(el.currentTime - target) > tolerance) {
      el.currentTime = target;
    }
    if (this.isPlaying && el.paused) {
      el.play().catch((e: unknown) => logWarning(`preview: a layer would not play (${String(e)})`));
    } else if (!this.isPlaying && !el.paused) {
      el.pause();
    }
  }
}
