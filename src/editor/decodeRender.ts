/**
 * Decoders for the render and product replies (Task 46) — their own file
 * so `decode.ts` does not grow toward the 500-line cap (the
 * `decodeRelink.ts` precedent). Held to the literals `render_jobs_tests.rs`
 * pins on the Rust side.
 */
import type {
  ProductDto,
  PublishDefaults,
  PublishReceipt,
  RenderRange,
  RenderStarted,
  VaultChoice,
} from "../editorTypes";
import {
  asArray,
  asBoolean,
  asId,
  asInteger,
  asMs,
  asNullableString,
  asObject,
  asString,
  fail,
} from "./decodePrimitives";

/** `editor_start_render`'s `{ jobId, revision }`. */
export function decodeRenderStarted(value: unknown): RenderStarted {
  const v = asObject(value, "renderStarted");
  return {
    jobId: asId(v.jobId, "renderStarted.jobId"),
    revision: asInteger(v.revision, "renderStarted.revision"),
  };
}

/** Present-even-when-null: `null` is a whole render, a missing key is a
 * protocol error, and a range that ends before it starts is not a range. */
function decodeRange(value: unknown, field: string): RenderRange | null {
  if (value === null) return null;
  const v = asObject(value, field);
  const range = { startMs: asMs(v.startMs, `${field}.startMs`), endMs: asMs(v.endMs, `${field}.endMs`) };
  if (range.endMs <= range.startMs) fail(`${field} must end after it starts`);
  return range;
}

/** `editor_get_products`' `ProductDto[]`. */
export function decodeProducts(value: unknown): ProductDto[] {
  return asArray(value, "products").map((raw, i) => {
    const at = `products[${i}]`;
    const v = asObject(raw, at);
    return {
      id: asId(v.id, `${at}.id`),
      projectId: asId(v.projectId, `${at}.projectId`),
      name: asString(v.name, `${at}.name`),
      filename: asString(v.filename, `${at}.filename`),
      mime: asString(v.mime, `${at}.mime`),
      revision: asInteger(v.revision, `${at}.revision`),
      durationMs: asMs(v.durationMs, `${at}.durationMs`),
      createdAt: asString(v.createdAt, `${at}.createdAt`),
      editFingerprint: asString(v.editFingerprint, `${at}.editFingerprint`),
      renderRange: decodeRange(v.renderRange, `${at}.renderRange`),
      available: asBoolean(v.available, `${at}.available`),
    };
  });
}

/** `editor_publish_product`'s `PublishReceipt` (Task 48). `notePath` and
 * `warning` are present-even-when-null, and a receipt without a video is
 * not a receipt. */
export function decodePublishReceipt(value: unknown): PublishReceipt {
  const v = asObject(value, "publishReceipt");
  const videoPath = asString(v.videoPath, "publishReceipt.videoPath");
  if (videoPath === "") fail("publishReceipt.videoPath must not be empty");
  return {
    videoPath,
    notePath: asNullableString(v.notePath, "publishReceipt.notePath"),
    vaultId: asString(v.vaultId, "publishReceipt.vaultId"),
    vaultName: asString(v.vaultName, "publishReceipt.vaultName"),
    warning: asNullableString(v.warning, "publishReceipt.warning"),
  };
}

/** `editor_export_subtitles`' reply: the file name written, `null` for a
 * dismissed dialog. */
export function decodeNullableFileName(value: unknown): string | null {
  const name = asNullableString(value, "fileName");
  if (name === "") fail("fileName must not be empty");
  return name;
}

/** `list_vaults`' vaults, reduced to what the Publish dialog shows. */
export function decodeVaultChoices(value: unknown): VaultChoice[] {
  return asArray(value, "vaults").map((raw, i) => {
    const v = asObject(raw, `vaults[${i}]`);
    return { id: asString(v.id, `vaults[${i}].id`), name: asString(v.name, `vaults[${i}].name`) };
  });
}

/** `get_screen_capture_config`'s reply (`ScreenCaptureConfigDto`, whose
 * camelCase keys `screen_config_commands.rs` pins), reduced to the two
 * settings the Publish dialog takes its defaults from. */
export function decodePublishDefaults(value: unknown): PublishDefaults {
  const v = asObject(value, "screenConfig");
  return {
    dated: asBoolean(v.screenCaptureDateFolders, "screenConfig.screenCaptureDateFolders"),
    createNote: asBoolean(v.screenCreateNote, "screenConfig.screenCreateNote"),
  };
}
