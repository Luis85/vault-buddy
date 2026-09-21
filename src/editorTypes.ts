/**
 * The tutorial editor's TS declaration site (F10; the `screenTypes.ts`
 * precedent for a domain-scoped split of `types.ts`).
 *
 * Created here with only the entity type `core::editor::time` /
 * `src/editor/timeMap.ts` need; later tasks (8, 13) extend this file with
 * the rest of the editor's commands, envelopes and entities rather than
 * growing `types.ts` or forking a second declaration site.
 *
 * Field spelling: the IPC ENVELOPE wraps everything in camelCase
 * (`sessionId`, `projectId`, …), but the `project` graph it carries keeps
 * the interchange document's OWN spelling, which is snake_case (R3,
 * `core::editor::model`'s module doc: "Field names are the interchange
 * document's own snake_case spelling"). `ClipSpan` below describes a
 * fragment of that graph, so it is snake_case too — not a TypeScript
 * convention violation, but the one place it would be wrong to camelCase.
 */

/** A clip's time-mapping span: output start, half-open source range
 * `[in_ms, out_ms)`, and speed. Mirrors `core::editor::time::ClipSpan`
 * field-for-field so a `Project`'s clip entity, read straight off the
 * wire, can be passed to `timeMap.ts` without any renaming. */
export interface ClipSpan {
  start_ms: number;
  in_ms: number;
  out_ms: number;
  speed: number;
}
