export interface Vault {
  id: string;
  name: string;
  path: string;
  /** Currently open in Obsidian (from obsidian.json's `open` flag). */
  open: boolean;
}
