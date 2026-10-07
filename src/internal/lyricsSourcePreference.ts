import { useSyncExternalStore } from "react";
import { getAppSetting, setAppSetting } from "./appSettings";

/**
 * Which lyric source the listener wants tried first.
 *
 * A *preference*, not a restriction: the chosen source is promoted to the front of the
 * table and raced in the first wave, but if it has nothing for this song the rest still run.
 * Locking to one source would trade a lyric sheet that is merely not their favourite for no
 * lyric sheet at all, which nobody wants from a setting called "preferred".
 *
 * Lives in `internal` rather than `ui/settings` because both sides read it: the settings
 * screen writes it, and the data source consults it while building its fetch plan. Putting
 * it under `ui` would have the datasource layer importing upwards.
 */
const STORAGE_KEY = "lyrics-source-preference";
const CHANGE_EVENT = "lyrics-source-preference-change";

export const AUTO_LYRICS_SOURCE = "auto";

/**
 * Returns `AUTO_LYRICS_SOURCE` or a source id. Ids are not validated here — an id that no
 * longer exists in the table simply finds no match when the plan is built, which degrades to
 * the default order instead of throwing on a stale localStorage value from an older build.
 */
export function getPreferredLyricsSourceId(): string {
  try {
    return localStorage.getItem(STORAGE_KEY) || AUTO_LYRICS_SOURCE;
  } catch {
    return AUTO_LYRICS_SOURCE;
  }
}

export function setPreferredLyricsSourceId(id: string): void {
  try {
    if (id === AUTO_LYRICS_SOURCE) localStorage.removeItem(STORAGE_KEY);
    else localStorage.setItem(STORAGE_KEY, id);
  } catch {
    // Quota or a locked profile: the choice still applies for this session.
  }
  window.dispatchEvent(new Event(CHANGE_EVENT));
  void setAppSetting(STORAGE_KEY, id);
}

/** Webview storage alone lost this on every restart for some Windows users (#153). */
export async function hydratePreferredLyricsSourceId(): Promise<void> {
  const stored = await getAppSetting<unknown>(STORAGE_KEY);
  // No durable copy yet: backfill it from local storage.
  setPreferredLyricsSourceId(typeof stored === "string" ? stored : getPreferredLyricsSourceId());
}

function subscribe(listener: () => void): () => void {
  window.addEventListener(CHANGE_EVENT, listener);
  // Not optional in a multi-window app: without it the mini-player never sees the change.
  window.addEventListener("storage", listener);
  return () => {
    window.removeEventListener(CHANGE_EVENT, listener);
    window.removeEventListener("storage", listener);
  };
}

export function usePreferredLyricsSourceId(): string {
  return useSyncExternalStore(subscribe, getPreferredLyricsSourceId, () => AUTO_LYRICS_SOURCE);
}
