/**
 * What the webcam dialog shows for each recorder state (visual-parity
 * Task 22; concept spec §9.7, screen 05): the footer's buttons, the reason
 * they cannot act right now, and the status row under the camera view.
 * Pure, so `WebcamControls`/`WebcamLive` stay flat reads of one value and
 * every phase is testable without a camera.
 *
 * The concept's idle "Try demo overlay" is browser-only and has no native
 * twin (design D10): idle offers Enable camera alone. Its "Save raw take"
 * has none either — a finished take is already a library asset the moment
 * it lands (GAP-195) — so review offers Retake and Add to timeline.
 */
import type { EditorIconName } from "../components/editor/icons/conceptIcons";
import type { WebcamState } from "./webcamRecorder";

export type WebcamAction = "enable" | "record" | "cancel" | "stop" | "retake" | "add";

export interface WebcamFooterButton {
  action: WebcamAction;
  label: string;
  variant: "bordered" | "primary";
  icon: EditorIconName | null;
}

export interface WebcamFooter {
  buttons: WebcamFooterButton[];
  /** Why the buttons cannot act right now, or `null`. */
  reason: string | null;
}

const ENABLE: WebcamFooterButton = { action: "enable", label: "Enable camera", variant: "primary", icon: "webcam" };
const RECORD: WebcamFooterButton = { action: "record", label: "Start recording", variant: "primary", icon: "circle" };
const CANCEL_COUNTDOWN: WebcamFooterButton = { action: "cancel", label: "Cancel countdown", variant: "bordered", icon: null };
const DISCARD_RECORDING: WebcamFooterButton = { action: "cancel", label: "Discard recording", variant: "bordered", icon: null };
const STOP: WebcamFooterButton = { action: "stop", label: "Stop & review", variant: "primary", icon: "stop" };
const RETAKE: WebcamFooterButton = { action: "retake", label: "Retake", variant: "bordered", icon: null };
const ADD: WebcamFooterButton = { action: "add", label: "Add to timeline", variant: "primary", icon: "layers" };

const FOOTERS: Record<WebcamState, WebcamFooter> = {
  idle: { buttons: [ENABLE], reason: null },
  requesting: { buttons: [ENABLE], reason: "Waiting for the camera…" },
  ready: { buttons: [RECORD], reason: null },
  countdown: { buttons: [CANCEL_COUNTDOWN], reason: null },
  recording: { buttons: [DISCARD_RECORDING, STOP], reason: null },
  review: { buttons: [RETAKE, ADD], reason: null },
  committing: { buttons: [RETAKE, ADD], reason: "Adding the take…" },
};

/** The footer for `state`; a review whose take Rust is still finishing
 * shows its buttons disabled, with that reason. */
export function webcamFooter(state: WebcamState, hasTake: boolean): WebcamFooter {
  if (state === "review" && !hasTake) return { buttons: [RETAKE, ADD], reason: "Finishing the take…" };
  return FOOTERS[state];
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
  review: { label: "Camera on", tone: "live", message: "Review the take, then add it to the timeline or record it again." },
  committing: { label: "Camera on", tone: "live", message: "Adding the take to the timeline…" },
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
