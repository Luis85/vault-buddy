/** A byte count as a person reads it.
 *
 * Its own util rather than a second copy of `TranscriptionModelsCard`'s
 * local `formatSize`, which is deliberately left alone: that one's ladder
 * starts at MB because a whisper model is never smaller than one, and
 * widening it would change what an existing, asserted card renders for no
 * benefit. This one has to span the whole range — staging legitimately holds
 * nothing at all, or a single short capture.
 *
 * Binary units (1024), because that is what Windows' own File Explorer shows
 * for the very same directory; reporting 1.1 GB where Explorer says 1.0 GB
 * invites the user to conclude one of the two is lying.
 */
export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return "0 KB";
  if (bytes >= 1024 ** 3) return `${(bytes / 1024 ** 3).toFixed(2)} GB`;
  if (bytes >= 1024 ** 2) return `${Math.round(bytes / 1024 ** 2)} MB`;
  return `${Math.max(1, Math.round(bytes / 1024))} KB`;
}
