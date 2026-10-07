import { useId, useRef, useState, type PointerEvent, type ReactNode } from "react";
import { cn } from "@/lib/utils";
import { SpinnerSteps } from "@/components/motion/loader";
import {
  ArrowUpIcon,
  CloseIcon,
  PauseActiveIcon,
  PlayActiveIcon,
  SkipNextIcon,
  SkipPreviousIcon,
} from "@/ui/icons";
import { TrackArtwork } from "../../TrackArtwork";
import {
  clampProgress,
  formatSkinTime,
  SKIN_FOCUS,
  useSeekPreview,
  useSpin,
  type MiniSkinProps,
} from "./shared";

/*
 * Tonearm angles, clockwise from straight down. The pivot sits in the top-right corner, so a
 * larger angle swings the stylus inward: ~3° on the outer groove, ~26° by the label.
 */
const ARM_REST = -10;
const ARM_START = 3;
const ARM_SWEEP = 23;
/** Presses this far out (as a share of the radius) grab the needle instead of the window. */
const SEEK_BAND_INNER = 0.8;

/** Curved text cannot truncate itself, so long titles are cut to what the arc holds. */
function fitArc(text: string, max: number): string {
  return text.length > max ? `${text.slice(0, max - 1).trimEnd()}…` : text;
}

/**
 * A turntable on a brushed-metal deck. The tonearm tracks the song from the outer groove to
 * the label, and dragging around the rim drops the needle anywhere in it.
 */
export function VinylSkin({
  title,
  artist,
  artworkUrl,
  isPlaying,
  isLoading,
  isError,
  currentTime,
  duration,
  volume,
  volumeVisible,
  reduceMotion,
  onTogglePlay,
  onNext,
  onPrevious,
  onSeek,
  onRestore,
  onClose,
}: MiniSkinProps) {
  // useId can contain characters that are not safe in an SVG `href` fragment.
  const arcId = `vinyl-${useId().replace(/[^\w-]/g, "")}`;
  const seek = useSeekPreview(currentTime, onSeek);
  const [dragging, setDragging] = useState(false);
  const lastProgressRef = useRef<number | null>(null);
  const deckRef = useRef<HTMLDivElement | null>(null);
  const recordRef = useSpin<HTMLDivElement>(isPlaying, reduceMotion, 1.8);

  const shownTime = seek.preview ?? currentTime;
  const progress = clampProgress(shownTime, duration);
  const hasTrack = title !== null;
  const armAngle = hasTrack ? ARM_START + ARM_SWEEP * progress : ARM_REST;
  // The arm rises off the record whenever it is not playing, as a real one does.
  const lifted = !isPlaying || dragging;
  const hud = volumeVisible
    ? `Vol ${Math.round(volume * 100)}%`
    : dragging
      ? `${formatSkinTime(shownTime)} / ${formatSkinTime(duration)}`
      : duration > 0
        ? `-${formatSkinTime(duration - shownTime)}`
        : null;

  const pointerOnDeck = (event: PointerEvent<HTMLDivElement>) => {
    const rect = deckRef.current?.getBoundingClientRect();
    if (!rect) return null;
    const dx = event.clientX - (rect.left + rect.width / 2);
    const dy = event.clientY - (rect.top + rect.height / 2);
    return {
      radius: Math.hypot(dx, dy) / (rect.width / 2),
      progress: (Math.atan2(dx, -dy) / (2 * Math.PI) + 1) % 1,
    };
  };

  const handlePointerDown = (event: PointerEvent<HTMLDivElement>) => {
    const point = pointerOnDeck(event);
    if (!point || duration <= 0 || point.radius < SEEK_BAND_INNER || point.radius > 1) return;
    // Prevented so the host never sees a mousedown here and starts dragging the window.
    event.preventDefault();
    event.currentTarget.setPointerCapture(event.pointerId);
    lastProgressRef.current = point.progress;
    setDragging(true);
    seek.setPreview(point.progress * duration);
  };

  const handlePointerMove = (event: PointerEvent<HTMLDivElement>) => {
    const point = pointerOnDeck(event);
    if (!point) return;
    if (!dragging) {
      const onBand = duration > 0 && point.radius >= SEEK_BAND_INNER && point.radius <= 1;
      event.currentTarget.style.cursor = onBand ? "pointer" : "";
      return;
    }
    let next = point.progress;
    const last = lastProgressRef.current;
    // Crossing twelve o'clock pins to the start or end instead of wrapping round.
    if (last !== null && Math.abs(next - last) > 0.5) next = last > 0.5 ? 1 : 0;
    lastProgressRef.current = next;
    seek.setPreview(next * duration);
  };

  const finishDrag = (commit: boolean) => {
    if (!dragging) return;
    setDragging(false);
    lastProgressRef.current = null;
    if (commit && seek.preview !== null) seek.commit(seek.preview);
    else seek.setPreview(null);
  };

  return (
    <div
      ref={deckRef}
      data-mini-skin="vinyl"
      className="group/vinyl relative size-[200px] select-none"
      onPointerDown={handlePointerDown}
      onPointerMove={handlePointerMove}
      onPointerUp={() => finishDrag(true)}
      onPointerCancel={() => finishDrag(false)}
    >
      {/* Keyboard and screen readers seek here; the rim is the pointer's way in. */}
      <input
        type="range"
        min={0}
        max={duration || 1}
        step="any"
        value={shownTime}
        disabled={duration <= 0}
        onChange={(event) => seek.setPreview(Number(event.currentTarget.value))}
        onKeyUp={(event) => seek.commit(Number(event.currentTarget.value))}
        aria-label="Song position"
        aria-valuetext={`${formatSkinTime(shownTime)} of ${formatSkinTime(duration)}`}
        className="peer sr-only"
      />

      {/* Brushed aluminium: fine radial strokes, a glint, and a bevelled edge. */}
      <div
        className={cn(
          "absolute inset-0 rounded-full",
          "bg-[radial-gradient(circle_at_30%_22%,var(--skin-metal-glint),transparent_55%),repeating-conic-gradient(var(--skin-metal-a)_0deg_1.5deg,var(--skin-metal-b)_1.5deg_3deg)]",
          "shadow-[inset_0_0_0_1px_var(--skin-metal-edge),inset_0_2px_3px_var(--skin-metal-glint)]",
        )}
        aria-hidden="true"
      />

      {/* Progress on the rim: a faint track with the elapsed share lit over it. */}
      <div
        className="absolute inset-[4px] rounded-full"
        style={{
          background: `conic-gradient(${isError ? "var(--skin-danger)" : "var(--skin-progress)"} ${progress * 100}%, var(--skin-track) 0)`,
          maskImage: "radial-gradient(closest-side, transparent calc(100% - 4px), #000 calc(100% - 4px))",
          WebkitMaskImage: "radial-gradient(closest-side, transparent calc(100% - 4px), #000 calc(100% - 4px))",
        }}
        aria-hidden="true"
      />
      <span
        className="pointer-events-none absolute inset-[2px] rounded-full opacity-0 ring-2 ring-ring peer-focus-visible:opacity-100"
        aria-hidden="true"
      />

      <div
        ref={recordRef}
        className="absolute inset-[12px] rounded-full bg-[repeating-radial-gradient(circle,var(--skin-record)_0_1.5px,var(--skin-groove)_1.5px_2.5px)]"
        aria-hidden="true"
      >
        <TrackArtwork
          artworkUrl={artworkUrl ?? undefined}
          className={cn(
            "absolute left-1/2 top-1/2 size-[60px] -translate-x-1/2 -translate-y-1/2 rounded-full bg-(--skin-groove) ring-2 ring-(--skin-label-rim)",
            isLoading && "motion-safe:animate-pulse",
          )}
          iconSize={20}
          loading="eager"
          size={60}
        />
        <span className="absolute left-1/2 top-1/2 size-2 -translate-x-1/2 -translate-y-1/2 rounded-full bg-[radial-gradient(circle_at_35%_30%,var(--skin-arm-hi),var(--skin-arm-lo))]" />
      </div>

      {/* Two fixed sheens and a lit rim: light stays put while the record turns under it. */}
      <div
        className="pointer-events-none absolute inset-[12px] rounded-full bg-[conic-gradient(from_25deg,transparent_0%,var(--skin-sheen)_9%,transparent_20%,transparent_50%,var(--skin-sheen)_59%,transparent_70%)] shadow-[inset_0_0_0_1px_var(--skin-sheen)]"
        aria-hidden="true"
      />

      <svg viewBox="0 0 176 176" className="pointer-events-none absolute inset-[12px]" aria-hidden="true">
        <defs>
          {/* The title arc stops short of the upper right, where the tonearm sweeps; the
              artist arc runs under the label left to right, so both read upright. */}
          <path id={`${arcId}-top`} d="M 24,88 A 64,64 0 0 1 120,32.6" />
          <path id={`${arcId}-bottom`} d="M 16,88 A 72,72 0 0 0 160,88" />
        </defs>
        <text className={cn("text-[11px] font-semibold", isError ? "fill-(--skin-danger)" : "fill-(--skin-ink)")}>
          <textPath href={`#${arcId}-top`} startOffset="50%" textAnchor="middle">
            {isError ? "Couldn't play this song" : fitArc(title ?? "Nothing playing", 20)}
          </textPath>
        </text>
        {artist && (
          <text className="fill-(--skin-ink-soft) text-[9px] tracking-[0.04em]">
            <textPath href={`#${arcId}-bottom`} startOffset="50%" textAnchor="middle">
              {fitArc(artist, 30)}
            </textPath>
          </text>
        )}
      </svg>

      <span
        className={cn(
          "pointer-events-none absolute left-1/2 top-[calc(50%+40px)] -translate-x-1/2 rounded-full bg-(--skin-hud) px-2 py-0.5 font-mono text-[9px] font-semibold tabular-nums text-(--skin-ink) transition-opacity duration-200",
          hud && (volumeVisible || dragging) ? "opacity-100" : "opacity-0 group-hover/vinyl:opacity-100",
          !hud && "hidden",
        )}
        role="status"
        aria-live="polite"
      >
        {hud}
      </span>

      {/* Tonearm, pivoting from the corner outside the record. */}
      <div className="pointer-events-none absolute left-[184px] top-[16px]" aria-hidden="true">
        <div
          className="absolute left-0 top-0 origin-top transition-[transform,filter] duration-500 ease-out"
          style={{
            transform: `rotate(${armAngle}deg) translateY(${lifted ? -2 : 0}px)`,
            filter: `drop-shadow(${lifted ? "3px 5px 3px" : "1px 2px 1px"} var(--skin-shadow))`,
          }}
        >
          <span className="absolute -left-[5px] -top-[20px] h-[15px] w-[10px] rounded-[3px] bg-[linear-gradient(90deg,var(--skin-arm-lo),var(--skin-arm-hi)_45%,var(--skin-arm-lo))]" />
          <span className="absolute -left-[2px] top-0 h-[100px] w-1 rounded-full bg-[linear-gradient(90deg,var(--skin-arm-lo),var(--skin-arm-hi)_40%,var(--skin-arm))]" />
          <span className="absolute -left-[5px] top-[94px] h-4 w-2.5 origin-top rotate-[16deg] rounded-[3px] bg-[linear-gradient(180deg,var(--skin-arm-hi),var(--skin-arm-lo))]">
            <span className="absolute bottom-0.5 left-1/2 size-[3px] -translate-x-1/2 rounded-full bg-(--skin-stylus)" />
          </span>
        </div>
        <span className="absolute -left-[11px] -top-[11px] grid size-[22px] place-items-center rounded-full bg-[radial-gradient(circle_at_35%_30%,var(--skin-arm-hi),var(--skin-arm-lo))] shadow-[0_2px_4px_var(--skin-shadow)]">
          <span className="size-2 rounded-full bg-(--skin-arm-lo)" />
        </span>
      </div>

      {/* Transport over the label: out of the way until you reach for it. */}
      <div className="absolute left-1/2 top-1/2 flex -translate-x-1/2 -translate-y-1/2 items-center gap-2.5 opacity-0 transition-opacity duration-200 focus-within:opacity-100 group-hover/vinyl:opacity-100">
        <VinylButton label="Previous" onClick={onPrevious}>
          <SkipPreviousIcon size={12} aria-hidden="true" />
        </VinylButton>
        <VinylButton label={isLoading ? "Loading song" : isPlaying ? "Pause" : "Play"} onClick={onTogglePlay} large>
          {isLoading ? (
            <SpinnerSteps size={14} color="currentColor" />
          ) : isPlaying ? (
            <PauseActiveIcon size={16} aria-hidden="true" />
          ) : (
            <PlayActiveIcon size={16} aria-hidden="true" />
          )}
        </VinylButton>
        <VinylButton label="Next" onClick={onNext}>
          <SkipNextIcon size={12} aria-hidden="true" />
        </VinylButton>
      </div>

      <div className="absolute left-0 top-0 opacity-0 transition-opacity focus-within:opacity-100 group-hover/vinyl:opacity-100">
        <CornerButton label="Restore main window" onClick={onRestore}>
          <ArrowUpIcon size={11} aria-hidden="true" />
        </CornerButton>
      </div>
      <div className="absolute bottom-0 right-0 opacity-0 transition-opacity focus-within:opacity-100 group-hover/vinyl:opacity-100">
        <CornerButton label="Close mini player" onClick={onClose}>
          <CloseIcon size={11} aria-hidden="true" />
        </CornerButton>
      </div>
    </div>
  );
}

function VinylButton({
  label,
  onClick,
  large = false,
  children,
}: {
  label: string;
  onClick: () => void;
  large?: boolean;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      aria-label={label}
      title={label}
      className={cn(
        "grid shrink-0 place-items-center rounded-full bg-(--skin-hud) text-(--skin-ink) shadow-[0_2px_6px_var(--skin-shadow),inset_0_1px_0_var(--skin-sheen)]",
        "transition-transform duration-150 hover:scale-110 active:scale-95",
        large ? "size-10" : "size-7",
        SKIN_FOCUS,
      )}
    >
      {children}
    </button>
  );
}

function CornerButton({
  label,
  onClick,
  children,
}: {
  label: string;
  onClick: () => void;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      aria-label={label}
      title={label}
      className={cn(
        "grid size-6 place-items-center rounded-full text-(--skin-record) shadow-[0_2px_4px_var(--skin-shadow)]",
        "bg-[radial-gradient(circle_at_35%_30%,var(--skin-arm-hi),var(--skin-arm))] transition-transform hover:scale-110 active:scale-95",
        SKIN_FOCUS,
      )}
    >
      {children}
    </button>
  );
}
