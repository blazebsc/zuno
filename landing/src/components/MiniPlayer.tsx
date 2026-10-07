import { useEffect, useRef, useState, type CSSProperties, type PointerEvent } from "react";
import { PauseIcon, PlayIcon, SkipIcon } from "./icons";
import { FOCUS_RING, cn } from "./ui";
import { TRACK, seek, togglePlayback, usePlayer } from "../audio/demoPlayer";

// The app's mini player capsule (src/ui/components/mini-player), driving the page's demo track.
const WIDTH = 160;
const HEIGHT = 44;

const clamp = (value: number, min: number, max: number) => Math.min(Math.max(value, min), Math.max(min, max));

const BUTTON = cn(
  "flex size-7 shrink-0 items-center justify-center rounded-full text-muted-foreground transition-colors hover:bg-foreground/10 hover:text-foreground disabled:opacity-40",
  FOCUS_RING,
);

export function MiniPlayer() {
  const { playing, time, duration } = usePlayer();
  // Anchored by its bottom edge, so opening grows it upward instead of off the bottom of the screen.
  const [position, setPosition] = useState({ left: 20, bottom: 20 });
  const [dragging, setDragging] = useState(false);
  const grab = useRef<{ x: number; y: number } | null>(null);
  const progress = duration > 0 ? Math.min(100, (time / duration) * 100) : 0;

  // A resize must never strand the capsule off-screen, where nothing could bring it back.
  useEffect(() => {
    const keepInView = () =>
      setPosition((current) => ({
        left: clamp(current.left, 8, window.innerWidth - WIDTH - 8),
        bottom: clamp(current.bottom, 8, window.innerHeight - HEIGHT - 8),
      }));
    window.addEventListener("resize", keepInView);
    return () => window.removeEventListener("resize", keepInView);
  }, []);

  const startDrag = (event: PointerEvent<HTMLDivElement>) => {
    if (event.target instanceof Element && event.target.closest("button, input")) return;
    const box = event.currentTarget.getBoundingClientRect();
    grab.current = { x: event.clientX - box.left, y: box.bottom - event.clientY };
    event.currentTarget.setPointerCapture(event.pointerId);
    setDragging(true);
  };

  const drag = (event: PointerEvent<HTMLDivElement>) => {
    if (!grab.current) return;
    const box = event.currentTarget.getBoundingClientRect();
    setPosition({
      left: clamp(event.clientX - grab.current.x, 8, window.innerWidth - box.width - 8),
      bottom: clamp(window.innerHeight - event.clientY - grab.current.y, 8, window.innerHeight - box.height - 8),
    });
  };

  const endDrag = () => {
    grab.current = null;
    setDragging(false);
  };

  return (
    <div className="mini-dock fixed z-50 max-sm:hidden" style={position}>
      <div
        className={cn(
          "group flex h-11 w-40 flex-col overflow-hidden rounded-[22px] bg-popover/90 shadow-2xl backdrop-blur-xl",
          "transition-[height,width,padding] duration-300 [transition-timing-function:var(--ease-out-expo)]",
          "hover:h-24 hover:w-[260px] hover:p-1.5 focus-within:h-24 focus-within:w-[260px] focus-within:p-1.5",
          dragging ? "cursor-grabbing select-none" : "cursor-grab",
        )}
        onPointerDown={startDrag}
        onPointerMove={drag}
        onPointerUp={endDrag}
        onPointerCancel={endDrag}
      >
        <div className="flex h-11 shrink-0 items-center gap-2 pl-1.5 pr-3">
          <button
            type="button"
            onClick={togglePlayback}
            aria-label={playing ? `Pause ${TRACK.title}` : `Play ${TRACK.title}`}
            className={cn("relative grid size-9 shrink-0 place-items-center rounded-full", FOCUS_RING)}
          >
            {/* Progress as a ring: the collapsed pill has no width to spare for a bar. */}
            <span
              className="progress-ring absolute inset-0 rounded-full"
              style={{
                background: `conic-gradient(var(--color-foreground) ${progress}%, color-mix(in oklch, var(--color-foreground) 16%, transparent) 0)`,
              }}
              aria-hidden="true"
            />
            <img src={TRACK.cover} alt="" className="size-7 rounded-full bg-card p-0.5" />
          </button>
          <div className="min-w-0 flex-1">
            <p className="truncate text-[12px] font-semibold leading-tight">{TRACK.title}</p>
            <p className="truncate text-[10px] leading-tight text-muted-foreground">{TRACK.artist}</p>
          </div>
        </div>

        <div className="pointer-events-none flex h-10 items-center gap-2 px-3 opacity-0 transition-opacity duration-200 group-focus-within:pointer-events-auto group-focus-within:opacity-100 group-hover:pointer-events-auto group-hover:opacity-100">
          <button type="button" className={BUTTON} onClick={() => seek(0)} aria-label="Restart the track">
            <SkipIcon back size={14} />
          </button>
          <button
            type="button"
            onClick={togglePlayback}
            aria-label={playing ? "Pause" : "Play"}
            className={cn("grid size-8 shrink-0 place-items-center rounded-full bg-foreground text-background", FOCUS_RING)}
          >
            {playing ? <PauseIcon size={14} /> : <PlayIcon size={14} />}
          </button>
          <button type="button" className={BUTTON} disabled aria-label="Next track">
            <SkipIcon size={14} />
          </button>
          <input
            type="range"
            min={0}
            max={duration || 1}
            step="any"
            value={time}
            onChange={(event) => seek(Number(event.currentTarget.value))}
            aria-label="Seek"
            className={cn("seek min-w-0 flex-1", FOCUS_RING)}
            style={{ "--seek": `${progress}%` } as CSSProperties}
          />
        </div>
      </div>
    </div>
  );
}
