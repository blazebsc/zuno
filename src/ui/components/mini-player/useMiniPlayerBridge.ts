import { useEffect, useState } from "react";
import { emit, listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { WebviewWindow } from "@tauri-apps/api/webviewWindow";

export interface PlayerSync {
  status: string;
  artworkUrl: string | null;
  title: string | null;
  artist: string | null;
}

export interface TimeSync {
  currentTime: number;
  duration: number;
}

export interface VolumeSync {
  muted: boolean;
  volume: number;
}

/** Playback state the main window pushes to the mini player, shared by every skin. */
export function useMiniPlayerBridge() {
  const [playerState, setPlayerState] = useState<PlayerSync>({
    status: "idle",
    artworkUrl: null,
    title: null,
    artist: null,
  });
  const [timeState, setTimeState] = useState<TimeSync>({ currentTime: 0, duration: 0 });
  const [volumeState, setVolumeState] = useState<VolumeSync>({ muted: false, volume: 1 });
  // The last real cover, so a sync without one (mid-load) does not blank the art.
  const [cachedArtwork, setCachedArtwork] = useState<string | null>(null);

  useEffect(() => {
    const unlisteners = Promise.all([
      listen<PlayerSync>("player-state-sync", ({ payload }) => {
        if (payload.artworkUrl) setCachedArtwork(payload.artworkUrl);
        setPlayerState(payload);
      }),
      listen<TimeSync>("player-time-sync", ({ payload }) => setTimeState(payload)),
      listen<VolumeSync>("player-volume-sync", ({ payload }) => setVolumeState(payload)),
    ]).then((unlisten) => {
      /*
       * Asked for only once the listeners are live. Syncs are sent on change, so a window
       * created mid-song would otherwise sit on "Nothing playing" until the next track.
       */
      void emit("mini-player:request-sync");
      return unlisten;
    });

    return () => {
      void unlisteners.then((unlisten) => unlisten.forEach((stop) => stop()));
    };
  }, []);

  return {
    playerState,
    timeState,
    volumeState,
    artworkUrl: playerState.artworkUrl ?? cachedArtwork,
  };
}

export async function restoreMainWindow(): Promise<void> {
  await emit("mini-player:restore-main");
  /*
   * Not awaited: the main window answers by destroying this window, so a hide can land after
   * it is gone and reject — awaited, that aborted the restore below and the click did nothing.
   */
  void getCurrentWindow().hide().catch(() => {});

  const mainWin = await WebviewWindow.getByLabel("main");
  if (mainWin) {
    await mainWin.show();
    await mainWin.unminimize();
    await mainWin.setFocus();
  }
}
