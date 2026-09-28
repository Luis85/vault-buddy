/**
 * The webcam dialog's "Mic level preview" (visual-parity Task 22 fix
 * round 1, Ruling T22-1; concept spec §9.7's `.webcam-meter`): the SAMPLE
 * PEAK of the live stream's microphone, read through a Web Audio
 * `AnalyserNode` — the preview mixer's own measure (`MixerPeakMeter`), a
 * peak and never a loudness figure.
 *
 * The analyser is fed from the stream and connected to nothing else: live
 * audio is never played through the speakers. Like the camera itself, the
 * measurement has one way out, `dispose`, which disconnects both nodes and
 * closes the `AudioContext`; `WebcamMicMeter` calls it whenever the stream
 * goes (Stop, a new camera, the dialog's close) and when it unmounts.
 */

interface NodeLike {
  connect?(node: unknown): unknown;
  disconnect(): void;
}

interface AnalyserLike extends NodeLike {
  fftSize: number;
  getFloatTimeDomainData(buffer: Float32Array): void;
}

interface AudioContextLike {
  createMediaStreamSource(stream: MediaStream): NodeLike;
  createAnalyser(): AnalyserLike;
  close(): Promise<void>;
}

type AudioContextCtor = new () => AudioContextLike;

const FFT_SIZE = 1024;

export class MicLevel {
  private readonly buffer = new Float32Array(FFT_SIZE);

  private constructor(
    private readonly context: AudioContextLike,
    private readonly source: NodeLike,
    private readonly analyser: AnalyserLike,
  ) {}

  /** A meter for `stream`'s microphone, or `null` when it carries none or
   * this window has no Web Audio. */
  static create(stream: MediaStream | null): MicLevel | null {
    const Ctor = (globalThis as { AudioContext?: AudioContextCtor }).AudioContext;
    if (!stream || !Ctor || !stream.getTracks().some((t) => t.kind === "audio")) return null;
    const context = new Ctor();
    const source = context.createMediaStreamSource(stream);
    const analyser = context.createAnalyser();
    analyser.fftSize = FFT_SIZE;
    source.connect?.(analyser);
    return new MicLevel(context, source, analyser);
  }

  /** The newest window's sample peak, `0..1`. */
  read(): number {
    this.analyser.getFloatTimeDomainData(this.buffer);
    let peak = 0;
    for (const sample of this.buffer) peak = Math.max(peak, Math.abs(sample));
    return Math.min(1, peak);
  }

  dispose(): void {
    this.source.disconnect();
    this.analyser.disconnect();
    void this.context.close();
  }
}
