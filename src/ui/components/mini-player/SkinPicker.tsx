import { cn } from "@/lib/utils";
import {
  MINI_PLAYER_SKINS,
  setMiniPlayerSkin,
  useMiniPlayerSkin,
} from "../../settings/miniPlayer";
import { useReduceMotion } from "../../settings/renderEffects";
import { shallowEqual, usePlayerSelector } from "../../../player/playerStore";
import { TrackArtwork } from "../TrackArtwork";
import { SKIN_COMPONENTS, type MiniSkinProps } from "./skins";

const PREVIEW_HEIGHT = 124;
const PREVIEW_WIDTH = 152;
/** Room each skin leaves around itself in its window for the shadow. */
const WINDOW_PADDING = 16;
const noop = () => {};

/**
 * Skin choice, each previewed live with whatever is playing now — a skin is judged by how it
 * looks with your music, not a placeholder.
 */
export function MiniPlayerSkinPicker({ labelledBy }: { labelledBy: string }) {
  const selected = useMiniPlayerSkin();
  const reduceMotion = useReduceMotion();
  const now = usePlayerSelector(
    (player) => ({ track: player.currentTrack, status: player.status }),
    shallowEqual,
  );
  const duration = now.track?.durationSec || 210;
  const preview: MiniSkinProps = {
    title: now.track?.title ?? "Midnight City",
    artist: now.track?.artist ?? "M83",
    artworkUrl: now.track?.artworkUrl ?? null,
    isPlaying: now.status === "playing",
    isLoading: false,
    isError: false,
    currentTime: duration * 0.4,
    duration,
    volume: 0.7,
    volumeVisible: false,
    reduceMotion,
    onTogglePlay: noop,
    onNext: noop,
    onPrevious: noop,
    onSeek: noop,
    onVolume: noop,
    onRestore: noop,
    onClose: noop,
  };

  return (
    <div
      role="group"
      aria-labelledby={labelledBy}
      className="grid gap-3 [grid-template-columns:repeat(auto-fill,minmax(11rem,1fr))]"
    >
      {MINI_PLAYER_SKINS.map((skin) => {
        const isSelected = skin.id === selected;
        const Skin = skin.id === "classic" ? null : SKIN_COMPONENTS[skin.id];
        const width = skin.width - WINDOW_PADDING;
        const height = skin.height - WINDOW_PADDING;
        const scale = Math.min(1, PREVIEW_WIDTH / width, PREVIEW_HEIGHT / height);

        return (
          <div
            key={skin.id}
            className={cn(
              "relative flex flex-col gap-2 rounded-2xl p-3 transition-colors",
              isSelected ? "bg-primary/15" : "hover:bg-muted",
            )}
          >
            {/* Decorative: the skins' own buttons must not be reachable from here. */}
            <div
              className="pointer-events-none grid place-items-center overflow-hidden"
              style={{ height: PREVIEW_HEIGHT }}
              aria-hidden="true"
              inert
            >
              {/* Boxed at the scaled size: scale() alone keeps the full-size layout box, which
                  overflows the card and gets aligned to its top edge instead of centred. */}
              <div style={{ width: width * scale, height: height * scale }}>
                <div
                  className="grid place-items-center"
                  style={{ width, height, transform: `scale(${scale})`, transformOrigin: "top left" }}
                >
                  {Skin ? <Skin {...preview} /> : <ClassicPreview {...preview} />}
                </div>
              </div>
            </div>
            <span className="flex flex-col gap-0.5">
              <strong className="text-sm font-medium text-foreground">{skin.name}</strong>
              <span className="text-xs text-muted-foreground">{skin.description}</span>
            </span>
            {/* A sibling overlay rather than a wrapping button: the preview holds buttons,
                and a button inside a button is invalid. */}
            <button
              type="button"
              aria-pressed={isSelected}
              aria-label={`${skin.name} skin`}
              onClick={() => setMiniPlayerSkin(skin.id)}
              className="absolute inset-0 rounded-2xl focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
            />
          </div>
        );
      })}
    </div>
  );
}

/** Classic owns its window logic, so its card shows a still of the collapsed capsule. */
function ClassicPreview({ title, artist, artworkUrl }: MiniSkinProps) {
  return (
    <div className="flex h-11 w-40 items-center gap-2 rounded-full bg-popover pl-1.5 pr-3 ring-1 ring-border">
      <TrackArtwork
        artworkUrl={artworkUrl ?? undefined}
        className="size-[34px] shrink-0 rounded-full bg-muted"
        iconSize={15}
        size={34}
      />
      <span className="min-w-0 flex-1">
        <span className="block truncate text-[12px] font-semibold text-popover-foreground">{title}</span>
        <span className="block truncate text-[10px] text-muted-foreground">{artist}</span>
      </span>
    </div>
  );
}
