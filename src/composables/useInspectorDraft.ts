/**
 * The inspector's shared draft-buffer composable (Task 19; F-48/F-49;
 * ARCHITECTURE-AND-STACK.md: "`Inspector` and six panels … Dirty form
 * buffers and validation precede one command; do not bind uncontrolled
 * inputs straight to native domain state"; SCREENS-AND-INTERACTIONS.md §04:
 * "Numeric entry is the alternative to dragging … Invalid input stays
 * visible with a correction; do not silently clamp to a radically
 * different edit without feedback").
 *
 * Every per-category inspector section (Clip/Layout/Fades/Audio/Speed/
 * Color — Task 20 onward) that lets the user type a value into a field
 * calls this once per field: it owns a LOCAL string `draft`, never the
 * projection itself, so a `v-model="draft"` in the template can never write
 * to `editorProject.project` directly (R14). The draft commits to ONE
 * `editorProject.execute` call on Enter/blur, only once per distinct edit —
 * a blur that follows an Enter for the same unchanged text is a no-op, not
 * a second command — and Escape reverts the draft to the field's last
 * known-good (committed) value without sending anything.
 *
 * `field.parse` is where a caller mirrors a Rust validation rule (e.g.
 * `core::editor::mod::limits::{SPEED_MIN,SPEED_MAX}` for a speed field —
 * read the Rust source for the bound, never invent one): an out-of-range or
 * unparsable draft returns `{ok:false,message}`, which `error` surfaces
 * inline while the invalid text stays exactly as the user left it.
 */
import type { Ref } from "vue";
import { ref } from "vue";

/** A field's read/format/parse trio — the composable never assumes it knows
 * what kind of value it is editing beyond what this describes. */
export interface InspectorField<T> {
  /** The projection's current, authoritative value for this control. */
  value: () => T;
  /** Render a committed value into the draft buffer's string form. */
  format: (value: T) => string;
  /** Parse a draft string back into a value, or explain why it can't. */
  parse: (raw: string) => InspectorParseResult<T>;
}

export type InspectorParseResult<T> =
  | { ok: true; value: T }
  | { ok: false; message: string };

export interface InspectorDraft {
  draft: Ref<string>;
  error: Ref<string | null>;
  /** Bound to Enter and blur: parses the current draft and, only when it
   * both validates AND differs from the last successful commit, sends ONE
   * command. An invalid draft sets `error` and leaves the draft untouched
   * (R20: no silent clamp). */
  submit: () => void;
  /** Bound to Escape: discards the draft, restoring the field's current
   * committed value with no command sent. */
  revert: () => void;
}

/** Builds a plain numeric field: a value getter, an inclusive `[min,max]`
 * range mirroring a Rust bound, and the human-readable range phrase used in
 * the out-of-range message. An empty or non-finite draft is refused as "not
 * a number" before the range check ever runs. */
export function numberField(opts: {
  value: () => number;
  label: string;
  min: number;
  max: number;
  rangeLabel: string;
  format?: (value: number) => string;
}): InspectorField<number> {
  const format = opts.format ?? ((v: number) => String(v));
  return {
    value: opts.value,
    format,
    parse(raw: string): InspectorParseResult<number> {
      const trimmed = raw.trim();
      const n = Number(trimmed);
      if (trimmed === "" || !Number.isFinite(n)) {
        return { ok: false, message: `${opts.label} must be a number.` };
      }
      if (n < opts.min || n > opts.max) {
        return { ok: false, message: `${opts.label} must be between ${opts.rangeLabel}` };
      }
      return { ok: true, value: n };
    },
  };
}

export function useInspectorDraft<T>(
  field: InspectorField<T>,
  commit: (value: T) => void,
): InspectorDraft {
  const draft = ref(field.format(field.value()));
  const error = ref<string | null>(null);
  // The raw text of the last successful commit (or the initial value) — a
  // submit whose draft still equals this is nothing new to send, which is
  // what keeps a blur that follows an already-handled Enter from firing a
  // second, redundant `editor_execute` for the identical edit.
  let lastCommittedRaw = draft.value;

  function submit(): void {
    if (draft.value === lastCommittedRaw) return;
    const result = field.parse(draft.value);
    if (!result.ok) {
      error.value = result.message;
      return;
    }
    error.value = null;
    draft.value = field.format(result.value);
    lastCommittedRaw = draft.value;
    commit(result.value);
  }

  function revert(): void {
    draft.value = field.format(field.value());
    lastCommittedRaw = draft.value;
    error.value = null;
  }

  return { draft, error, submit, revert };
}
