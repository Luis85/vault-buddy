import knightIdle from "./assets/buddies/knight-idle.png";
import knightRun from "./assets/buddies/knight-run.png";
import wizardIdle from "./assets/buddies/wizard-idle.png";
import wizardRun from "./assets/buddies/wizard-run.png";
import elfIdle from "./assets/buddies/elf-idle.png";
import elfRun from "./assets/buddies/elf-run.png";
import dwarfIdle from "./assets/buddies/dwarf-idle.png";
import dwarfRun from "./assets/buddies/dwarf-run.png";
import lizardIdle from "./assets/buddies/lizard-idle.png";
import lizardRun from "./assets/buddies/lizard-run.png";

/** 4-frame sprite strips (16×28 px per frame) — see assets/buddies/LICENSE.md */
export interface SpriteSheet {
  idle: string;
  run: string;
}

export interface BuddyCharacter {
  id: string;
  name: string;
  /** null renders the original inline-SVG buddy */
  sprite: SpriteSheet | null;
}

export const CHARACTERS: readonly BuddyCharacter[] = [
  { id: "classic", name: "Classic", sprite: null },
  { id: "knight", name: "Knight", sprite: { idle: knightIdle, run: knightRun } },
  { id: "wizard", name: "Wizard", sprite: { idle: wizardIdle, run: wizardRun } },
  { id: "elf", name: "Elf", sprite: { idle: elfIdle, run: elfRun } },
  { id: "dwarf", name: "Dwarf", sprite: { idle: dwarfIdle, run: dwarfRun } },
  { id: "lizard", name: "Lizard", sprite: { idle: lizardIdle, run: lizardRun } },
];

export const DEFAULT_CHARACTER_ID = CHARACTERS[0].id;

/** Unknown ids (e.g. stale localStorage) fall back to the classic buddy. */
export function getCharacter(id: string): BuddyCharacter {
  return CHARACTERS.find((c) => c.id === id) ?? CHARACTERS[0];
}
