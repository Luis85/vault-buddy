export type Daypart = "morning" | "afternoon" | "evening" | "night";

// Local-time hour buckets, contiguous and half-open so every hour maps to
// exactly one daypart:
//   morning   05:00–11:59
//   afternoon 12:00–16:59
//   evening   17:00–21:59
//   night     22:00–04:59
export function daypartFor(date: Date): Daypart {
  const h = date.getHours();
  if (h >= 5 && h < 12) return "morning";
  if (h >= 12 && h < 17) return "afternoon";
  if (h >= 17 && h < 22) return "evening";
  return "night";
}

// getDay(): 0 = Sunday … 6 = Saturday.
export function isWeekend(date: Date): boolean {
  const d = date.getDay();
  return d === 0 || d === 6;
}

// Character-neutral greetings. Exactly 3 phrasings per cell so repeated
// launches don't feel canned; the count is asserted in the tests.
const GREETINGS: Record<Daypart, { weekday: string[]; weekend: string[] }> = {
  morning: {
    weekday: [
      "Good morning! Ready to dive into your notes?",
      "Morning! A fresh day, a fresh page.",
      "Rise and shine — your vault awaits.",
    ],
    weekend: [
      "Good morning! Enjoy a relaxed start to your weekend.",
      "Weekend morning — no rush, just your notes and a coffee.",
      "Morning! A perfect time for some unhurried thinking.",
    ],
  },
  afternoon: {
    weekday: [
      "Good afternoon! Let's keep the momentum going.",
      "Afternoon! Time to capture a thought or two?",
      "Hope your day's going well — your vault's right here.",
    ],
    weekend: [
      "Good afternoon! A calm weekend for tending your notes.",
      "Afternoon! Weekend projects, meet your vault.",
      "Hope you're having a lovely weekend afternoon.",
    ],
  },
  evening: {
    weekday: [
      "Good evening! Winding down or wrapping up?",
      "Evening! A good time to review the day's notes.",
      "Evening! Let's tie up any loose ends.",
    ],
    weekend: [
      "Good evening! Enjoy a cozy weekend night.",
      "Evening! The weekend's still going — savor it.",
      "Good evening! Relax; your notes will keep.",
    ],
  },
  night: {
    weekday: [
      "Working late? Your vault's here whenever you need it.",
      "It's getting late — one more note, then rest.",
      "Late-night thoughts? Let's jot them down.",
    ],
    weekend: [
      "Late weekend night — the quiet's good for ideas.",
      "Still up? Your vault doesn't mind the hour.",
      "Night-owl mode: your notes are ready when you are.",
    ],
  },
};

/**
 * Pick one greeting for the given moment. `pick(n)` returns an index in
 * [0, n); it is injected in tests for determinism and defaults to a
 * Math.random-based choice.
 */
export function greetingFor(
  date: Date,
  pick: (n: number) => number = (n) => Math.floor(Math.random() * n),
): string {
  const cell = GREETINGS[daypartFor(date)];
  const pool = isWeekend(date) ? cell.weekend : cell.weekday;
  return pool[pick(pool.length)];
}
