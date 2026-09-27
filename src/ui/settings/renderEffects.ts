import { useSyncExternalStore } from "react";
import { useReducedMotion } from "motion/react";
import {
  invalidatePaperPcModeCache,
  readPaperPcMode,
  setPaperPcMode,
  usePaperPcMode,
} from "./paperPcMode";
import {
  hydrateLocalJsonSetting,
  readLocalJsonSetting,
  writeLocalJsonSetting,
} from "../../internal/durableLocalSetting";

/**
 * Per-effect kill switches, for finding out which visual effect is actually costing the GPU.
 *
 * Paper-PC mode turns everything off at once, which tells you the app is cheaper without
 * effects but not *which* one was expensive. These are the same thing sliced up: flip one,
 * watch the GPU graph, flip it back.
 *
 * Every switch is a CSS override keyed off one `data-fx-off` attribute on <html> (see the
 * block in global.css) rather than a prop threaded through components — the effects live in
 * ~40 files and a debug switch is not worth touching 40 files for. `motion` is the exception:
 * Motion/React animates inline styles, which no stylesheet can override, so it is read in
 * App.tsx and fed to <MotionConfig>.
 */
export const RENDER_EFFECTS = [
  {
    id: "transitions",
    label: "CSS transitions",
    description: "Hover, colour and size fades on every control.",
  },
  {
    id: "animations",
    label: "CSS keyframe animations",
    description: "Spinners, loaders, pulses, the visualiser bars.",
  },
  {
    id: "motion",
    label: "JS motion (Motion/React)",
    description: "Panel, dialog and page enter/exit springs.",
  },
  {
    id: "backdrop",
    label: "Backdrop blur",
    description: "The frosted glass behind panels, menus and overlays. Usually the most expensive.",
  },
  {
    id: "filters",
    label: "Blur and colour filters",
    description: "Blurred artwork washes.",
  },
  {
    id: "shadows",
    label: "Shadows",
    description: "Box and text shadows.",
  },
  {
    id: "ambient",
    label: "Ambient artwork layers",
    description: "The oversized blurred cover behind the page and the lyrics screen.",
  },
  {
    id: "lyrics-drift",
    label: "Lyrics parallax drift",
    description: "The 44s scale-and-pan on the lyrics backdrop.",
  },
  {
    id: "visualizer",
    label: "Visualisers and loaders",
    description: "The playing-row bars and the bouncing audio loader.",
  },
  {
    id: "marquee",
    label: "Scrolling titles",
    description: "Marquee on track titles too long for their column.",
  },
  {
    id: "will-change",
    label: "Compositor hints (will-change)",
    description: "Removes the promotion hints that give effects their own GPU layer.",
  },
] as const;

export type RenderEffectId = (typeof RENDER_EFFECTS)[number]["id"];

const EFFECT_IDS: readonly string[] = RENDER_EFFECTS.map((effect) => effect.id);
const STORAGE_KEY = "render-effects-off";
const CHANGE_EVENT = "render-effects-change";

function isEffectIdList(value: unknown): value is RenderEffectId[] {
  return Array.isArray(value) && value.every((id) => EFFECT_IDS.includes(id as string));
}

export function readDisabledEffects(): RenderEffectId[] {
  return readLocalJsonSetting(STORAGE_KEY, isEffectIdList) ?? [];
}

/**
 * The snapshot is the attribute value, not the array: `useSyncExternalStore` compares
 * snapshots by identity, and a freshly parsed array is never identical to the last one.
 *
 * Cached, because `useReduceMotion` reads this and every album card calls it — and
 * `useSyncExternalStore` invokes `getSnapshot` more than once per render. Uncached, painting a
 * library page meant a `localStorage.getItem` and a `JSON.parse` per card per render, for a
 * value that changes when someone opens Settings.
 */
let snapshot: string | null = null;

function readSnapshot(): string {
  if (snapshot === null) snapshot = readDisabledEffects().join(" ");
  return snapshot;
}

function invalidateSnapshot(): void {
  snapshot = null;
}

/*
 * Registered at module scope so it runs before any subscriber React adds later: the cache has
 * to already be stale by the time a re-render asks for the value.
 */
if (typeof window !== "undefined") {
  window.addEventListener(CHANGE_EVENT, invalidateSnapshot);
  window.addEventListener("storage", invalidateSnapshot);
}

function subscribe(callback: () => void) {
  window.addEventListener(CHANGE_EVENT, callback);
  window.addEventListener("storage", callback);
  return () => {
    window.removeEventListener(CHANGE_EVENT, callback);
    window.removeEventListener("storage", callback);
  };
}

export function applyRenderEffects(disabled = readDisabledEffects()): void {
  document.documentElement.setAttribute("data-fx-off", disabled.join(" "));
}

export async function hydrateRenderEffects(): Promise<void> {
  await hydrateLocalJsonSetting(STORAGE_KEY, isEffectIdList);

  /*
   * Migration, one launch only.
   *
   * Potato PC used to kill animation, transition, shadow and backdrop-filter from its own CSS
   * block. That block is gone — the switches do it now — so an install that had the mode on
   * and has never seen this list would come back up with every effect running. Anyone who has
   * touched the switches since has a stored list and is left alone.
   */
  invalidatePaperPcModeCache();
  if (readPaperPcMode() && readLocalJsonSetting(STORAGE_KEY, isEffectIdList) === null) {
    writeLocalJsonSetting(STORAGE_KEY, [...(EFFECT_IDS as RenderEffectId[])]);
  }

  invalidateSnapshot();
  applyRenderEffects();
  window.dispatchEvent(new Event(CHANGE_EVENT));
}

/**
 * The one switch in Settings. Opaque palette plus every effect off.
 *
 * Order matters on Linux, where `setPaperPcMode` reloads the window: the effect list has to be
 * written before the reload takes the process with it.
 */
export function setPotatoPcMode(enabled: boolean): void {
  setAllEffectsDisabled(enabled);
  setPaperPcMode(enabled);
}

/** True only when the blunt mode is fully on — one effect switched back on unsets it. */
export function usePotatoPcMode(): boolean {
  const paperPcMode = usePaperPcMode();
  const allDisabled = useAllEffectsDisabled();
  return paperPcMode && allDisabled;
}

function save(disabled: RenderEffectId[]): void {
  writeLocalJsonSetting(STORAGE_KEY, disabled);
  invalidateSnapshot();
  applyRenderEffects(disabled);
  window.dispatchEvent(new Event(CHANGE_EVENT));
}

export function setEffectDisabled(id: RenderEffectId, disabled: boolean): void {
  const current = readDisabledEffects();
  if (disabled === current.includes(id)) return;
  save(disabled ? [...current, id] : current.filter((entry) => entry !== id));
}

export function setAllEffectsDisabled(disabled: boolean): void {
  save(disabled ? [...(EFFECT_IDS as RenderEffectId[])] : []);
}

export function useDisabledEffects(): string {
  return useSyncExternalStore(subscribe, readSnapshot, () => "");
}

export function useEffectDisabled(id: RenderEffectId): boolean {
  return useDisabledEffects().split(" ").includes(id);
}

export function useAllEffectsDisabled(): boolean {
  const disabled = useDisabledEffects().split(" ");
  return EFFECT_IDS.every((id) => disabled.includes(id));
}

/**
 * Should this component skip its motion? The app-wide answer, and the one to use.
 *
 * Motion's own `useReducedMotion()` reads the OS media query and *only* that — it does not
 * consult `<MotionConfig reducedMotion>` (that prop is read by a separate internal hook, and
 * only for the automatic reduction motion components do to their own transforms). So a
 * component that branches on the bare hook — picking a variant, gating a
 * `requestAnimationFrame` loop, choosing `scrollTo` behaviour — never sees Reduced motion mode
 * or the switch above, and keeps doing the work in the mode meant to stop it.
 *
 * The vendored beUI components under src/components/motion still call the bare hook. That is
 * fine: their `reduce` only ever picks between two Motion transitions, which `<MotionConfig>`
 * already reduces. This is for the app's own components, where `reduce` gates real work.
 */
export function useReduceMotion(): boolean {
  // All three unconditionally, then combined: `a || useHook()` short-circuits, which would
  // skip a hook call on the renders where it matters most.
  const prefersReduced = useReducedMotion() ?? false;
  const paperPcMode = usePaperPcMode();
  const motionDisabled = useEffectDisabled("motion");
  return prefersReduced || paperPcMode || motionDisabled;
}
