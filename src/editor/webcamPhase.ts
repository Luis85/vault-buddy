/**
 * What the webcam dialog shows for each recorder state (visual-parity
 * Task 22; concept spec §9.7, screen 05): the footer's buttons, the reason
 * each cannot act right now, the status row under the camera view, and the
 * reasons the take's settings wait. Pure, so `WebcamControls`/`WebcamLive`/
 * `WebcamSettings` stay flat reads of one value and every phase is testable
 * without a camera.
 *
 * The concept's idle "Try demo overlay" is browser-only and has no native
 * twin (design D10): idle offers Enable camera alone. Its "Save raw take"
 * has no native command — a finished take is already a library asset the
 * moment it lands (GAP-195) — so review offers Retake and Add to timeline.
 * While the camera is being asked for, the concept's Cancel request closes
 * the dialog (the recorder turns a late answer straight off).
 */
import type { EditorIconName } from "../components/editor/icons/conceptIcons";
import type { WebcamState } from "./webcamRecorder";

export type WebcamAction = "enable" | "cancel-request" | "record" | "cancel" | "stop" | "retake" | "add";

export interface WebcamFooterButton {
  action: WebcamAction;
  label: string;
  variant: "bordered" | "primary";
  icon: EditorIconName | null;
  /** Why this button cannot act right now, or `null`. */
  reason: string | null;
}

export interface WebcamFooter {
  buttons: WebcamFooterButton[];
  /** What the footer's status line says, or `null`. */
  note: string | null;
}

type Button = Omit<WebcamFooterButton, "reason">;

const ENABLE: Button = { action: "enable", label: "Enable camera", variant: "primary", icon: "webcam" };
const CANCEL_REQUEST: Button = { action: "cancel-request", label: "Cancel request", variant: "bordered", icon: null };
const RECORD: Button = { action: "record", label: "Start recording", variant: "primary", icon: "circle" };
const CANCEL_COUNTDOWN: Button = { action: "cancel", label: "Cancel countdown", variant: "bordered", icon: null };
const DISCARD_RECORDING: Button = { action: "cancel", label: "Discard recording", variant: "bordered", icon: null };
const STOP: Button = { action: "stop", label: "Stop & review", variant: "primary", icon: "stop" };
const RETAKE: Button = { action: "retake", label: "Retake", variant: "bordered", icon: null };
const ADD: Button = { action: "add", label: "Add to timeline", variant: "primary", icon: "layers" };

const BUTTONS: Record<WebcamState, Button[]> = {
  idle: [ENABLE],
  requesting: [CANCEL_REQUEST],
  ready: [RECORD],
  countdown: [CANCEL_COUNTDOWN],
  recording: [DISCARD_RECORDING, STOP],
  review: [RETAKE, ADD],
  committing: [RETAKE, ADD],
};

/** Why the dialog is mid-operation — the ✕ and the whole footer wait —
 * or `null` when it is not. */
export function webcamBusyReason(state: WebcamState, hasTake: boolean): string | null {
  if (state === "requesting") return "Waiting for the camera…";
  if (state === "committing") return "Adding the take…";
  return state === "review" && !hasTake ? "Finishing the take…" : null;
}

/** The footer for `state`. `placeReason` is why the chosen insert time
 * cannot be used (`insertTimeMs`); it holds back Add to timeline alone. */
export function webcamFooter(state: WebcamState, hasTake: boolean, placeReason: string | null = null): WebcamFooter {
  const busy = webcamBusyReason(state, hasTake);
  // Cancel request is the way out of a wait, so it never waits itself.
  const blocked = state === "requesting" ? null : busy;
  const reasonFor = (b: Button) => blocked ?? (b.action === "add" ? placeReason : null);
  const buttons = BUTTONS[state].map((b) => ({ ...b, reason: reasonFor(b) }));
  return { buttons, note: busy ?? (state === "review" ? placeReason : null) };
}

export type WebcamTone = "off" | "live" | "recording";

export interface WebcamStatus {
  label: string;
  tone: WebcamTone;
  /** The help line under the status row. */
  message: string;
}

const OFF: WebcamStatus = {
  label: "Camera off",
  tone: "off",
  message:
    "Nothing is accessed until you press Enable camera. Windows may ask you to allow the camera (and the microphone, if you choose it).",
};
const LIVE: WebcamStatus = {
  label: "Camera on",
  tone: "live",
  message: "Your camera is on. Recording starts after a 3-second countdown.",
};

const STATUSES: Record<WebcamState, WebcamStatus> = {
  idle: OFF,
  requesting: OFF,
  ready: LIVE,
  countdown: LIVE,
  recording: {
    label: "Recording",
    tone: "recording",
    message: "Recording on this PC. Stop & review when you are done.",
  },
  // Stop & review turns the camera and microphone off (Ruling F-1).
  review: { label: "Camera off", tone: "off", message: "Review the take, then add it to the timeline or record it again." },
  committing: { label: "Camera off", tone: "off", message: "Adding the take to the timeline…" },
};

export function webcamStatus(state: WebcamState): WebcamStatus {
  return STATUSES[state];
}

/** Why the camera and microphone choices cannot change right now, or `""`. */
export function webcamSettingsReason(state: WebcamState): string {
  if (state === "ready") return "";
  if (state === "idle" || state === "requesting") return "Choose a camera once it is on.";
  return "The camera and microphone are chosen before recording.";
}

/** Why the microphone picker waits, or `""`. */
export function micPickerReason(state: WebcamState, withMic: boolean): string {
  if (!withMic) return "Turn on Include microphone to choose one.";
  if (state === "idle" || state === "requesting") return "Choose a microphone once the camera is on.";
  return webcamSettingsReason(state);
}

/** The insert time the user typed, in seconds, as output ms — or why it
 * cannot be used: a take is placed on the project's own timeline, from its
 * start to its end. */
export function insertTimeMs(text: string | number, durationMs: number): { ms: number } | { reason: string } {
  const raw = String(text).trim();
  const seconds = Number(raw);
  const ms = Math.round(seconds * 1000);
  if (raw !== "" && Number.isFinite(seconds) && ms >= 0 && ms <= durationMs) return { ms };
  return { reason: `Choose a time between 0 and ${(durationMs / 1000).toFixed(2)} seconds.` };
}
