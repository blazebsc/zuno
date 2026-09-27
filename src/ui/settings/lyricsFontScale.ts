import { useSyncExternalStore } from "react";

/**
 * Multiplier applied to the lyric type scale.
 *
 * A multiplier rather than a set of sizes: the underlying scale is a `clamp()` driven by the
 * container's width, so it already adapts to the queue panel opening and to the window. A
 * fixed size per option would throw that away and reintroduce the overflow it prevents.
 */
const STORAGE_KEY = "lyrics-font-scale";
const CHANGE_EVENT = "lyrics-font-scale-change";

export const DEFAULT_LYRICS_FONT_SCALE = 1;

export const LYRICS_FONT_SCALES = [
  { value: 0.7, label: "Extra small" },
  { value: 0.85, label: "Small" },
  { value: 1, label: "Default" },
  { value: 1.2, label: "Large" },
  { value: 1.45, label: "Extra large" },
];

/** Snapped to a known step: a hand-edited localStorage value must not produce 40rem lyrics. */
export function normalizeFontScale(value: number): number {
  if (!Number.isFinite(value)) return DEFAULT_LYRICS_FONT_SCALE;
  return LYRICS_FONT_SCALES.reduce((closest, option) =>
    Math.abs(option.value - value) < Math.abs(closest.value - value) ? option : closest,
  ).value;
}

export function getLyricsFontScale(): number {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    return raw === null ? DEFAULT_LYRICS_FONT_SCALE : normalizeFontScale(Number(raw));
  } catch {
    return DEFAULT_LYRICS_FONT_SCALE;
  }
}

export function setLyricsFontScale(value: number): void {
  const scale = normalizeFontScale(value);
  try {
    if (scale === DEFAULT_LYRICS_FONT_SCALE) localStorage.removeItem(STORAGE_KEY);
    else localStorage.setItem(STORAGE_KEY, String(scale));
  } catch {
    // Quota or a locked profile: the choice still applies for this session.
  }
  window.dispatchEvent(new Event(CHANGE_EVENT));
}

function subscribe(listener: () => void): () => void {
  window.addEventListener(CHANGE_EVENT, listener);
  // Not optional in a multi-window app: without it the other window never sees the change.
  window.addEventListener("storage", listener);
  return () => {
    window.removeEventListener(CHANGE_EVENT, listener);
    window.removeEventListener("storage", listener);
  };
}

export function useLyricsFontScale(): number {
  return useSyncExternalStore(
    subscribe,
    getLyricsFontScale,
    () => DEFAULT_LYRICS_FONT_SCALE,
  );
}
