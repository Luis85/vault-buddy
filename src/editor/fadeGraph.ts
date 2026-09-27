/**
 * The Fades tab's envelope picture (visual-parity Task 15; concept spec §5
 * "Fades", `editor.js: function fadeGraph`): a 240×70 box, the clip's span
 * between x 10 and 230, full level at y 15 and silence at y 55. The fade in
 * rises from the left edge over its share of the clip, the fade out falls
 * to the right edge; each eases with a quadratic curve, and an edge with no
 * fade is a straight vertical line. The knees — where each fade meets full
 * level — carry the dots.
 */

const SPAN = 220;
const LEFT = 10;
const RIGHT = LEFT + SPAN;

export interface FadeGraph {
  envelope: string;
  fill: string;
  knees: [number, number];
}

/** Two decimals at most, so the paths stay readable. */
function n(value: number): number {
  return Math.round(value * 100) / 100;
}

export function fadeGraphPaths(fadeInMs: number, fadeOutMs: number, durationMs: number): FadeGraph {
  const d = Math.max(1, durationMs);
  const a = n((fadeInMs / d) * SPAN);
  const b = n(SPAN - (fadeOutMs / d) * SPAN);
  const rise = fadeInMs ? `Q ${n(LEFT + a * 0.25)} 15 ${n(LEFT + a)} 15` : `L${LEFT} 15`;
  const fall = fadeOutMs ? `Q ${n(LEFT + b + (SPAN - b) * 0.75)} 15 ${RIGHT} 55` : `L${RIGHT} 15`;
  return {
    envelope: `M${LEFT} 55 ${rise} L${n(LEFT + b)} 15 ${fall}`,
    fill: `M${LEFT} 55 L${n(LEFT + a)} 15 H${n(LEFT + b)} L${RIGHT} 55Z`,
    knees: [n(LEFT + a), n(LEFT + b)],
  };
}
