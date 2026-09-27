/**
 * Self-check for the lyrics font scale. Run with the whole suite:
 *
 *   npm run check
 *
 * The scale is multiplied into a CSS `calc()`, so a value that escapes the known steps does
 * not fail loudly — it renders lyrics at some absurd size with no error anywhere. Snapping is
 * the only thing standing between a hand-edited localStorage entry and that.
 */
export {};

const store = new Map<string, string>();
Object.assign(globalThis, {
  localStorage: {
    getItem: (key: string) => store.get(key) ?? null,
    setItem: (key: string, value: string) => void store.set(key, value),
    removeItem: (key: string) => void store.delete(key),
  },
  window: { dispatchEvent: () => true, addEventListener: () => {}, removeEventListener: () => {} },
});

function check(condition: boolean, message: string): void {
  if (!condition) throw new Error(`FAILED: ${message}`);
}

function equal(actual: unknown, expected: unknown, message: string): void {
  check(actual === expected, `${message}: expected ${String(expected)}, got ${String(actual)}`);
}

const {
  DEFAULT_LYRICS_FONT_SCALE,
  LYRICS_FONT_SCALES,
  getLyricsFontScale,
  normalizeFontScale,
  setLyricsFontScale,
} = await import("./lyricsFontScale");

check(
  LYRICS_FONT_SCALES.some((option) => option.value === DEFAULT_LYRICS_FONT_SCALE),
  "the default is one of the offered steps",
);

for (const option of LYRICS_FONT_SCALES) {
  equal(normalizeFontScale(option.value), option.value, `${option.label} survives normalising`);
}

equal(normalizeFontScale(1.19), 1.2, "a near miss snaps to the closest step");
equal(normalizeFontScale(0.1), 0.7, "an absurdly small value clamps to the smallest step");
equal(normalizeFontScale(99), 1.45, "an absurdly large one clamps to the largest");
equal(normalizeFontScale(Number.NaN), DEFAULT_LYRICS_FONT_SCALE, "NaN falls back to default");
equal(
  normalizeFontScale(Number.POSITIVE_INFINITY),
  DEFAULT_LYRICS_FONT_SCALE,
  "and so does Infinity",
);

equal(getLyricsFontScale(), DEFAULT_LYRICS_FONT_SCALE, "an unset preference reads as default");

setLyricsFontScale(1.45);
equal(getLyricsFontScale(), 1.45, "a choice round-trips through storage");

setLyricsFontScale(DEFAULT_LYRICS_FONT_SCALE);
equal(getLyricsFontScale(), DEFAULT_LYRICS_FONT_SCALE, "returning to default reads back");
check(!store.has("lyrics-font-scale"), "and clears the entry rather than storing the default");

// The path that actually matters: junk on disk must not reach the stylesheet.
store.set("lyrics-font-scale", "not-a-number");
equal(getLyricsFontScale(), DEFAULT_LYRICS_FONT_SCALE, "unparseable stored text falls back");
store.set("lyrics-font-scale", "400");
equal(getLyricsFontScale(), 1.45, "an out-of-range stored value is clamped, not obeyed");

console.log("lyricsFontScale self-check passed");
