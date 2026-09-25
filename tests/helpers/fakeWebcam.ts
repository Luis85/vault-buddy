import { vi } from "vitest";

/**
 * Stand-ins for the browser's camera surface (Task 50): a
 * `navigator.mediaDevices` whose `getUserMedia` records every request and
 * can be told to refuse with a named `DOMException`, streams whose tracks
 * remember being stopped, and a `MediaRecorder` a test drives chunk by
 * chunk. Shared by `webcamRecorder.test.ts` and `editorWebcamDialog.test.ts`
 * so the two suites cannot disagree about what the platform does.
 */
export class FakeTrack {
  stopped = false;
  constructor(readonly kind: "video" | "audio") {}
  stop(): void {
    this.stopped = true;
  }
}

/** A real (happy-dom) `MediaStream` subclass, so `video.srcObject` accepts
 * it the way the platform accepts a camera stream. */
export class FakeStream extends MediaStream {
  constructor(readonly tracks: FakeTrack[]) {
    super();
  }
  override getTracks(): MediaStreamTrack[] {
    return this.tracks as unknown as MediaStreamTrack[];
  }
}

export interface FakeDevices {
  mediaDevices: MediaDevices;
  requests: MediaStreamConstraints[];
  streams: FakeStream[];
  /** Every track any request ever handed out. */
  tracks: () => FakeTrack[];
}

/** `deny` names the `DOMException` every request fails with. */
export function fakeMediaDevices(deny: string | null = null): FakeDevices {
  const requests: MediaStreamConstraints[] = [];
  const streams: FakeStream[] = [];
  const getUserMedia = vi.fn((constraints: MediaStreamConstraints) => {
    requests.push(constraints);
    if (deny) return Promise.reject(new DOMException("refused", deny));
    const stream = new FakeStream([new FakeTrack("video"), ...(constraints.audio ? [new FakeTrack("audio")] : [])]);
    streams.push(stream);
    return Promise.resolve(stream);
  });
  const enumerateDevices = () =>
    Promise.resolve([
      { kind: "videoinput", deviceId: "cam-front", label: "Front camera", groupId: "g1" },
      { kind: "audioinput", deviceId: "mic-1", label: "Microphone", groupId: "g1" },
      { kind: "videoinput", deviceId: "cam-usb", label: "", groupId: "g2" },
    ]);
  return {
    mediaDevices: { getUserMedia, enumerateDevices } as unknown as MediaDevices,
    requests,
    streams,
    tracks: () => streams.flatMap((s) => s.tracks),
  };
}

interface ChunkEvent {
  data: Blob;
}

/** A `MediaRecorder` whose chunks the test emits by hand; `stop()` emits
 * the final chunk and then `stop`, the order the platform uses. */
export class FakeRecorder {
  static instances: FakeRecorder[] = [];
  /** vp8 is deliberately unsupported, so "the first supported type" and
   * "the first listed type" are different answers. */
  static supported = new Set(["video/webm;codecs=vp9,opus", "video/webm"]);
  /** The chunk `stop()` flushes; `[]` flushes an empty one (nothing recorded). */
  static finalBytes: number[] = [0xff];
  static isTypeSupported(mime: string): boolean {
    return FakeRecorder.supported.has(mime);
  }

  ondataavailable: ((event: ChunkEvent) => void) | null = null;
  onstop: (() => void) | null = null;
  onerror: ((event: unknown) => void) | null = null;
  state: "inactive" | "recording" = "inactive";
  timeslice: number | null = null;
  readonly mimeType: string;

  constructor(
    readonly stream: unknown,
    options: { mimeType: string },
  ) {
    this.mimeType = options.mimeType;
    FakeRecorder.instances.push(this);
  }

  start(timeslice: number): void {
    this.timeslice = timeslice;
    this.state = "recording";
  }

  emit(bytes: number[]): void {
    this.ondataavailable?.({ data: new Blob([new Uint8Array(bytes)]) });
  }

  stop(): void {
    if (this.state === "inactive") return;
    this.state = "inactive";
    this.emit(FakeRecorder.finalBytes);
    this.onstop?.();
  }
}

export function resetFakeRecorder(): void {
  FakeRecorder.instances = [];
  FakeRecorder.finalBytes = [0xff];
  FakeRecorder.supported = new Set(["video/webm;codecs=vp9,opus", "video/webm"]);
}

/** A promise the test settles by hand. */
export function deferred<T = void>(): { promise: Promise<T>; resolve: (v: T) => void; reject: (e: unknown) => void } {
  let resolve!: (v: T) => void;
  let reject!: (e: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}
