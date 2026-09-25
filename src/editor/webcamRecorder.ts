/**
 * A webcam take's whole life in the editor webview (Task 50; F-20; ADR R10;
 * SCREENS 05): `idle → requesting → ready → countdown → recording → review
 * → committing`. A plain, NON-reactive class — it owns a `MediaStream`, a
 * `MediaRecorder` and a promise chain, none of which may pass through a Vue
 * proxy — that reports each change through `onChange` as a fresh
 * `WebcamView`, which `WebcamDialog.vue` mirrors into a `shallowRef`.
 *
 * **Nothing is requested before `enable`.** `enable` is the ONLY place
 * `getUserMedia` is called, and only a user's *Enable camera* (or a camera/
 * microphone change after it) calls `enable`; constructing a recorder, or
 * opening the dialog, touches no device.
 *
 * **Chunks go out one at a time, in order.** Rust admits ONLY the next
 * sequence number (`core::editor::take`'s `accept_chunk`), so every
 * `dataavailable` is numbered the instant it fires and appended on ONE
 * promise chain — a later chunk never starts before the one ahead of it has
 * landed, however slow that append is. Two appends in flight would let a
 * short chunk overtake a long one and fail the whole take.
 *
 * **A finished take is never deleted** (Task 49's ruling, GAP-195): it is a
 * registered asset the moment `editor_webcam_finish` answers. So `retake`
 * after a finished take only returns to `ready` — the earlier take stays in
 * the library — and only a take that is still OPEN (recording, never
 * finished) is ever discarded, which removes its `.part`.
 *
 * **Tracks are stopped on every way out**: `dispose` (the dialog's close,
 * `pagehide`) and every error. A camera left running would keep its light
 * on for the rest of the process. A take still recording at `dispose` is
 * DROPPED in order (fix round 1): its final chunk is deliberately not sent,
 * the append already in flight is awaited (bounded by `DRAIN_LIMIT_MS`, so
 * an append that never answers cannot hold it), and only then is the take
 * discarded — never an append landing on a take Rust already removed. Any
 * append or discard that fails is logged, never swallowed.
 *
 * **ffmpeg is pre-flighted** (`deps.preflight`, the dialog's cached
 * `useFfmpegStore` probe) before the countdown, so a known-missing ffmpeg is
 * reported at once; `editor_webcam_begin`'s own `encoderUnavailable` stays
 * the authority for everything the pre-flight could not know.
 */
import type { TakeDto } from "../editorTypes";
import { logWarning } from "../logging";
import { type EditorPort,EditorPortError } from "./port";

/** The `MediaRecorder` types Rust accepts, in preference order —
 * `core::editor::take::TAKE_MIME_TYPES`, byte for byte (a test reads the
 * Rust source and compares). */
export const TAKE_MIME_TYPES = ["video/webm;codecs=vp8,opus", "video/webm;codecs=vp9,opus", "video/webm"] as const;

export const PERMISSION_DENIED_TEXT =
  "Camera access was blocked. Allow camera access for Vault Buddy in Windows Settings (Privacy & security, Camera), then press Enable camera again. Your project is unchanged.";
export const DEVICE_UNAVAILABLE_TEXT =
  "No camera could be opened. Check that one is connected and not in use by another app, then try again. Your project is unchanged.";
export const ENCODER_UNAVAILABLE_TEXT =
  "Recording a webcam take needs ffmpeg, which could not be found. Install ffmpeg (Buddy settings, Integrations shows where it is looked for), then try again. Your project is unchanged.";

/** The countdown before a take starts: 3, 2, 1, one second each. */
const COUNTDOWN = [3, 2, 1] as const;
const COUNTDOWN_STEP_MS = 1000;
/** One chunk per second (ADR R10's timeslice). */
const TIMESLICE_MS = 1000;
/** How long a dropped take waits for the append already in flight. */
const DRAIN_LIMIT_MS = 2000;

export type WebcamState = "idle" | "requesting" | "ready" | "countdown" | "recording" | "review" | "committing";

export type WebcamProblemKind = "permissionDenied" | "deviceUnavailable" | "encoderUnavailable" | "failed";

export interface WebcamProblem {
  kind: WebcamProblemKind;
  message: string;
}

export interface WebcamCamera {
  deviceId: string;
  label: string;
}

export interface WebcamView {
  state: WebcamState;
  /** The countdown's current number, `null` outside the countdown. */
  count: number | null;
  cameras: WebcamCamera[];
  /** The finished take, in `review`/`committing`. */
  take: TakeDto | null;
  problem: WebcamProblem | null;
}

/** The slice of `MediaRecorder` this file drives. */
export interface RecorderLike {
  ondataavailable: ((event: { data: Blob }) => void) | null;
  onstop: (() => void) | null;
  onerror: ((event: unknown) => void) | null;
  readonly state: string;
  start(timeslice: number): void;
  stop(): void;
}

export interface RecorderConstructor {
  new (stream: MediaStream, options: { mimeType: string }): RecorderLike;
  isTypeSupported(mime: string): boolean;
}

export interface WebcamDeps {
  port: Pick<EditorPort, "webcamBegin" | "webcamAppend" | "webcamFinish" | "webcamDiscard">;
  sessionId: () => string | null;
  mediaDevices: MediaDevices | undefined;
  Recorder: RecorderConstructor | undefined;
  /** The countdown's clock; `setTimeout` unless a test supplies one. */
  wait?: (ms: number) => Promise<void>;
  /** How long `dispose` waits for an in-flight append (`DRAIN_LIMIT_MS`). */
  drainLimitMs?: number;
  /** A refusal known before any take is begun (a missing ffmpeg), or `null`. */
  preflight?: () => Promise<WebcamProblem | null>;
  onChange: (view: WebcamView) => void;
}

/** A recording in progress: its take, the recorder, the append chain and
 * the next sequence number to hand out. */
interface Session {
  sessionId: string;
  takeId: string;
  recorder: RecorderLike;
  chain: Promise<void>;
  nextSeq: number;
  failure: unknown;
  stopped: Promise<void>;
  /** Being dropped by `dispose`: no further chunk is sent. */
  dropped: boolean;
}

function defaultWait(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

/** `work`, or give up waiting after `ms` (the work itself carries on). */
async function bounded(work: Promise<void>, ms: number): Promise<void> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  const limit = new Promise<void>((resolve) => (timer = setTimeout(resolve, ms)));
  await Promise.race([work, limit]);
  clearTimeout(timer);
}

/** A `getUserMedia` refusal, as the copy the user reads. */
function cameraProblem(e: unknown): WebcamProblem {
  const name = e instanceof DOMException || e instanceof Error ? e.name : "";
  if (name === "NotAllowedError" || name === "SecurityError") {
    return { kind: "permissionDenied", message: PERMISSION_DENIED_TEXT };
  }
  if (["NotFoundError", "NotReadableError", "OverconstrainedError", "AbortError"].includes(name)) {
    return { kind: "deviceUnavailable", message: DEVICE_UNAVAILABLE_TEXT };
  }
  return { kind: "failed", message: `The camera could not be started: ${String(e)}` };
}

/** A take command's refusal, as the copy the user reads. */
function takeProblem(e: unknown): WebcamProblem {
  if (e instanceof EditorPortError && e.error.code === "encoderUnavailable") {
    return { kind: "encoderUnavailable", message: ENCODER_UNAVAILABLE_TEXT };
  }
  const message = e instanceof Error ? e.message : String(e);
  return { kind: "failed", message: `The take could not be recorded. ${message}` };
}

function constraints(deviceId: string | undefined, withMic: boolean): MediaStreamConstraints {
  return { video: deviceId ? { deviceId: { exact: deviceId } } : true, audio: withMic };
}

export class WebcamRecorder {
  private state: WebcamState = "idle";
  private count: number | null = null;
  private cameras: WebcamCamera[] = [];
  private take: TakeDto | null = null;
  private problem: WebcamProblem | null = null;
  private live: MediaStream | null = null;
  private session: Session | null = null;
  /** Bumped by every cancel/dispose, so a countdown that outlives it stops. */
  private epoch = 0;

  constructor(private readonly deps: WebcamDeps) {}

  get view(): WebcamView {
    return { state: this.state, count: this.count, cameras: this.cameras, take: this.take, problem: this.problem };
  }

  /** The camera's live stream, for the dialog's preview. */
  get stream(): MediaStream | null {
    return this.live;
  }

  private set(patch: Partial<Omit<WebcamView, "cameras">>): void {
    if (patch.state !== undefined) this.state = patch.state;
    if (patch.count !== undefined) this.count = patch.count;
    if (patch.take !== undefined) this.take = patch.take;
    if (patch.problem !== undefined) this.problem = patch.problem;
    this.deps.onChange(this.view);
  }

  /** Ask for the camera (and optionally the microphone) — the ONLY
   * `getUserMedia` call anywhere in the editor. A second call (another
   * camera, the microphone toggled) stops the previous stream first. */
  async enable(deviceId?: string, withMic = false): Promise<void> {
    this.stopTracks();
    this.set({ state: "requesting", problem: null });
    try {
      const mediaDevices = this.deps.mediaDevices;
      if (!mediaDevices) throw new DOMException("no media devices", "NotFoundError");
      this.live = await mediaDevices.getUserMedia(constraints(deviceId, withMic));
    } catch (e) {
      this.fail(cameraProblem(e));
      return;
    }
    this.cameras = await this.listDevices();
    this.set({ state: "ready" });
  }

  /** The cameras, labelled — readable only once permission was granted
   * (before it, the platform hides labels). */
  async listDevices(): Promise<WebcamCamera[]> {
    try {
      const all = (await this.deps.mediaDevices?.enumerateDevices()) ?? [];
      return all
        .filter((d) => d.kind === "videoinput")
        .map((d, i) => ({ deviceId: d.deviceId, label: d.label || `Camera ${i + 1}` }));
    } catch (e) {
      logWarning(`webcam: listing cameras failed: ${String(e)}`);
      return [];
    }
  }

  /** Count down 3-2-1 (cancellable), then begin the take and record. */
  async start(): Promise<void> {
    if (this.state !== "ready") return;
    const epoch = this.epoch;
    const refused = await this.preflight();
    if (refused) return this.fail(refused);
    if (epoch !== this.epoch) return;
    const wait = this.deps.wait ?? defaultWait;
    for (const n of COUNTDOWN) {
      this.set({ state: "countdown", count: n, problem: null });
      await wait(COUNTDOWN_STEP_MS);
      if (epoch !== this.epoch) return;
    }
    this.set({ count: null });
    await this.record(epoch);
  }

  private async preflight(): Promise<WebcamProblem | null> {
    try {
      return (await this.deps.preflight?.()) ?? null;
    } catch (e) {
      // A broken pre-flight blocks nothing: the native refusal still decides.
      logWarning(`webcam: ffmpeg pre-flight failed: ${String(e)}`);
      return null;
    }
  }

  private async record(epoch: number): Promise<void> {
    const sessionId = this.deps.sessionId();
    const Recorder = this.deps.Recorder;
    const mimeType = TAKE_MIME_TYPES.find((m) => Recorder?.isTypeSupported(m));
    if (!sessionId || !Recorder || !this.live || !mimeType) {
      this.fail({ kind: "failed", message: "This window cannot record video." });
      return;
    }
    let takeId: string;
    try {
      takeId = (await this.deps.port.webcamBegin(sessionId, mimeType)).takeId;
    } catch (e) {
      this.fail(takeProblem(e));
      return;
    }
    if (epoch !== this.epoch) {
      this.discardOpen(sessionId, takeId);
      return;
    }
    this.session = this.open(sessionId, takeId, new Recorder(this.live, { mimeType }));
    this.set({ state: "recording" });
  }

  private open(sessionId: string, takeId: string, recorder: RecorderLike): Session {
    let stopped!: () => void;
    const session: Session = {
      sessionId,
      takeId,
      recorder,
      chain: Promise.resolve(),
      nextSeq: 0,
      failure: null,
      stopped: new Promise<void>((resolve) => (stopped = resolve)),
      dropped: false,
    };
    recorder.ondataavailable = (event) => this.enqueue(session, event.data);
    recorder.onstop = () => stopped();
    recorder.onerror = (event) => {
      session.failure ??= event;
      stopped();
    };
    recorder.start(TIMESLICE_MS);
    return session;
  }

  /** Number the chunk NOW and append it after every chunk ahead of it. */
  private enqueue(session: Session, data: Blob): void {
    if (data.size === 0 || session.dropped) return;
    const seq = session.nextSeq;
    session.nextSeq += 1;
    session.chain = session.chain.then(async () => {
      if (session.failure) return;
      try {
        const bytes = new Uint8Array(await data.arrayBuffer());
        await this.deps.port.webcamAppend(session.sessionId, session.takeId, seq, bytes);
      } catch (e) {
        session.failure = e;
        logWarning(`webcam: take ${session.takeId} chunk ${seq} was not appended: ${String(e)}`);
      }
    });
  }

  /** Stop recording and finish the take; `review` shows it. */
  async stop(): Promise<void> {
    const session = this.session;
    if (!session || this.state !== "recording") return;
    this.set({ state: "review", take: null });
    await this.drain(session);
    if (session.failure || session.nextSeq === 0) {
      this.session = null;
      this.discardOpen(session.sessionId, session.takeId);
      this.fail(session.failure ? takeProblem(session.failure) : { kind: "failed", message: "Nothing was recorded." });
      return;
    }
    try {
      const take = await this.deps.port.webcamFinish(session.sessionId, session.takeId, session.nextSeq - 1);
      this.session = null;
      this.set({ take });
    } catch (e) {
      this.session = null;
      this.fail(takeProblem(e));
    }
  }

  private async drain(session: Session): Promise<void> {
    if (session.recorder.state !== "inactive") session.recorder.stop();
    await session.stopped;
    await session.chain;
  }

  /** Leave the countdown or the recording: the countdown just stops; a
   * recording's unfinished take is discarded (its `.part` removed). */
  async cancel(): Promise<void> {
    this.epoch += 1;
    const session = this.session;
    this.session = null;
    if (session) {
      await this.drain(session);
      this.discardOpen(session.sessionId, session.takeId);
    }
    this.set({ state: this.live ? "ready" : "idle", count: null });
  }

  /** Record again. A finished take is NOT deleted (GAP-195) — it stays in
   * the library; only the dialog stops offering it. */
  async retake(): Promise<void> {
    if (this.state !== "review" || this.session) return;
    this.set({ state: this.live ? "ready" : "idle", take: null });
  }

  /** Run `place` (the dialog's timeline insertion) as `committing`; back
   * to `review` when it did not land, so the user can try again. */
  async commit(place: (take: TakeDto) => Promise<boolean>): Promise<boolean> {
    const take = this.take;
    if (this.state !== "review" || !take) return false;
    this.set({ state: "committing" });
    const placed = await place(take);
    this.set({ state: "review" });
    return placed;
  }

  /** Stop every track and drop any take still recording. Safe to call
   * twice; called on the dialog's close, on `pagehide`, and on errors. */
  dispose(): void {
    this.epoch += 1;
    const session = this.session;
    this.session = null;
    if (session) void this.drop(session);
    this.stopTracks();
    this.set({ state: "idle", count: null, take: null });
  }

  /** Stop a take still recording without sending its final chunk, let the
   * append in flight land (bounded), then discard it. */
  private async drop(session: Session): Promise<void> {
    session.dropped = true;
    if (session.recorder.state !== "inactive") session.recorder.stop();
    await bounded(session.chain, this.deps.drainLimitMs ?? DRAIN_LIMIT_MS);
    this.discardOpen(session.sessionId, session.takeId);
  }

  private fail(problem: WebcamProblem): void {
    this.dispose();
    this.set({ problem });
  }

  private stopTracks(): void {
    for (const track of this.live?.getTracks() ?? []) track.stop();
    this.live = null;
  }

  /** Remove an unfinished take's `.part`; a failure is logged (the session's
   * own close removes it too). */
  private discardOpen(sessionId: string, takeId: string): void {
    this.deps.port.webcamDiscard(sessionId, takeId).catch((e: unknown) => {
      logWarning(`webcam: discarding take ${takeId} failed: ${String(e)}`);
    });
  }
}
