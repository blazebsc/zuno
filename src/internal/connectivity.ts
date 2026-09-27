import { useSyncExternalStore } from "react";

/**
 * Whether the app can reach the network, as last measured by PlayerBar's probe.
 *
 * A module rather than PlayerBar state because the player needs it too: offline, it has to
 * step over songs that are not downloaded instead of failing on each one.
 */
let online = typeof navigator === "undefined" ? true : navigator.onLine;
const listeners = new Set<() => void>();

export function isOnline(): boolean {
  return online;
}

export function setOnline(next: boolean): void {
  if (next === online) return;
  online = next;
  for (const listener of listeners) listener();
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function useOnline(): boolean {
  return useSyncExternalStore(subscribe, isOnline, () => true);
}

/** Runs `callback` each time the app comes back online. */
export function onReconnect(callback: () => void): () => void {
  return subscribe(() => {
    if (online) callback();
  });
}

/** A failed load's message, saying so plainly when the real cause is being offline. */
export function loadFailedMessage(message: string): string {
  return online ? message : "You're offline. Reconnect and try again.";
}
