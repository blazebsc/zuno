import { BrandIcon, SERVICE_ICON } from "./brandIcons";
import { TRACK, usePlayer } from "../audio/demoPlayer";
import { formatTime } from "../audio/track";
import { cn } from "./ui";

/** Discord and Last.fm, following the demo track: the card's clock and the scrobble count are live. */
export function Presence() {
  const { playing, time, duration, scrobbles } = usePlayer();
  const progress = duration > 0 ? (time / duration) * 100 : 0;

  return (
    <div className="flex flex-col gap-3">
      <div className={cn("rounded-2xl bg-background/80 p-4 transition-opacity duration-500", playing ? "opacity-100" : "opacity-55")}>
        <p className="flex items-center gap-2 text-[11px] font-semibold uppercase tracking-[0.1em] text-muted-foreground">
          <BrandIcon icon={SERVICE_ICON.discord} width={14} height={11} />
          {playing ? "Listening to Zuno" : "Paused"}
        </p>
        <div className="mt-3 flex items-center gap-3">
          <img src={TRACK.cover} alt="" className="size-14 shrink-0 rounded-xl bg-card p-1.5" />
          <div className="min-w-0">
            <p className="truncate text-[15px] font-semibold">{TRACK.title}</p>
            <p className="truncate text-[13px] text-muted-foreground">by {TRACK.artist}</p>
          </div>
        </div>
        <div className="mt-4 flex items-center gap-2.5 font-mono text-[11px] tabular-nums text-muted-foreground">
          <span>{formatTime(time)}</span>
          <span className="h-1 flex-1 overflow-hidden rounded-full bg-muted">
            <span className="block h-full rounded-full bg-foreground" style={{ width: `${progress}%` }} />
          </span>
          <span>{formatTime(duration)}</span>
        </div>
      </div>

      <div className="flex items-center justify-between gap-3 rounded-2xl bg-background/80 px-4 py-3.5">
        <BrandIcon icon={SERVICE_ICON.lastfm} width={52} height={13} />
        <span className="font-mono text-[12px] tabular-nums text-muted-foreground" role="status" aria-live="polite">
          {scrobbles === 0 ? "scrobbles at halfway" : `${scrobbles} scrobbled`}
        </span>
      </div>
    </div>
  );
}
