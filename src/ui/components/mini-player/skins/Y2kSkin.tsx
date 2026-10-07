import { useEffect, useState, type ReactNode } from "react";
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
import {
  clampProgress,
  formatSkinTime,
  SKIN_FOCUS,
  SkinTitle,
  useSeekPreview,
  type MiniSkinProps,
} from "./shared";

const METER_COLUMNS = 14;
const METER_ROWS = 5;
const PROGRESS_SEGMENTS = 26;
const VOLUME_SEGMENTS = 16;
const TICK_MS = 120;

/** An organic pebble outline — no two corners alike, the way skins of that era were cut. */
const BODY_RADIUS = "64px 44px 58px 50px / 60px 54px 64px 58px";

/** Advances while `active`; drives the LCD meter and nothing else. */
function useTick(active: boolean): number {
  const [tick, setTick] = useState(0);
  useEffect(() => {
    if (!active) return;
    const id = window.setInterval(() => setTick((value) => value + 1), TICK_MS);
    return () => window.clearInterval(id);
  }, [active]);
  return tick;
}

/*
 * Decorative LCD levels, heavier in the low columns. The mini window never receives the audio
 * signal, so the meter is alive while music plays rather than a measurement of it.
 */
function meterLevel(index: number, tick: number): number {
  const wave = Math.sin(index * 0.85 + tick * 0.6) * 0.3
    + Math.sin(index * 2.1 - tick * 1.2) * 0.22
    + Math.sin(tick * 0.35 + index * 0.3) * 0.18;
  return Math.min(1, Math.max(0.1, (wave + 0.5) * (1 - (index / METER_COLUMNS) * 0.3)));
}

/**
 * A candy-gel pebble from the turn of the millennium: opaque aqua with a hard specular band,
 * a recessed backlit LCD, silver gel keys and a chrome-ringed play orb bulging off one end.
 */
export function Y2kSkin({
  title,
  artist,
  isPlaying,
  isLoading,
  isError,
  currentTime,
  duration,
  volume,
  reduceMotion,
  onTogglePlay,
  onNext,
  onPrevious,
  onSeek,
  onVolume,
  onRestore,
  onClose,
}: MiniSkinProps) {
  const seek = useSeekPreview(currentTime, onSeek);
  const shownTime = seek.preview ?? currentTime;
  const progress = clampProgress(shownTime, duration);
  const hasTrack = title !== null;
  const canSeek = duration > 0;
  const meterLive = isPlaying && !reduceMotion;
  const tick = useTick(meterLive);
  const state = isError
    ? "ERROR"
    : isLoading
      ? "LOADING"
      : isPlaying
        ? "PLAYING"
        : hasTrack
          ? "PAUSED"
          : "READY";

  return (
    <div data-mini-skin="y2k" className="relative h-[140px] w-[300px] select-none">
      {/* ── The gel body ───────────────────────────────────────────── */}
      <div
        className={cn(
          "absolute left-0 top-1.5 h-32 w-[268px] overflow-hidden",
          "bg-[linear-gradient(180deg,var(--skin-gel-top),var(--skin-gel)_52%,var(--skin-gel-deep))]",
          "shadow-[inset_0_0_0_1.5px_var(--skin-gel-edge),inset_0_-6px_14px_var(--skin-gel-deep)]",
        )}
        style={{ borderRadius: BODY_RADIUS }}
        aria-hidden="true"
      >
        {/* The hard specular band that made every surface of the era look wet. */}
        <span className="absolute left-[6%] right-[8%] top-[4%] h-[44%] rounded-[50%/60%] bg-[linear-gradient(180deg,var(--skin-specular),var(--skin-specular-fade))]" />
        {/* Light coming up through the gel from below. */}
        <span className="absolute inset-x-[12%] bottom-0 h-[42%] bg-[radial-gradient(ellipse_at_50%_100%,var(--skin-gel-glow),transparent_70%)]" />
      </div>

      {/* ── Backlit LCD ─────────────────────────────────────────────── */}
      <div
        className={cn(
          "absolute left-4 top-5 flex h-[100px] w-[168px] flex-col gap-[3px] overflow-hidden rounded-xl px-2 py-1.5 text-(--skin-lcd-ink)",
          "bg-[linear-gradient(180deg,var(--skin-lcd-hi),var(--skin-lcd))]",
          "shadow-[inset_0_2px_5px_var(--skin-gel-edge)] ring-[3px] ring-(--skin-lcd-bezel)",
        )}
      >
        <div className="relative flex h-2.5 items-center justify-between text-[7.5px] font-bold tracking-[0.18em]">
          <span className={cn(isError && "text-(--skin-danger)")}>{state}</span>
          <span className="flex items-center gap-0.5">
            <LcdButton label="Restore main window" onClick={onRestore}>
              <ArrowUpIcon size={9} aria-hidden="true" />
            </LcdButton>
            <LcdButton label="Close mini player" onClick={onClose}>
              <CloseIcon size={9} aria-hidden="true" />
            </LcdButton>
          </span>
        </div>

        <SkinTitle
          text={title ?? "Nothing playing"}
          className="relative h-4 text-[12px] font-bold leading-4 tracking-[-0.01em]"
        />
        <p className={cn("relative h-3 truncate text-[10px] leading-3", isError ? "font-semibold text-(--skin-danger)" : "opacity-75")}>
          {isError ? "Couldn't play this song" : isLoading ? "Loading…" : artist ?? (hasTrack ? "" : "Insert a song")}
        </p>

        <div className="relative flex h-3.5 items-end justify-between">
          <span className="flex h-full items-end gap-px" aria-hidden="true">
            {Array.from({ length: METER_COLUMNS }, (_, column) => {
              const lit = meterLive ? Math.round(meterLevel(column, tick) * METER_ROWS) : 1;
              return (
                <span key={column} className="flex flex-col-reverse gap-px">
                  {Array.from({ length: METER_ROWS }, (_, row) => (
                    <span
                      key={row}
                      className={cn("h-0.5 w-[5px]", row < lit ? "bg-(--skin-lcd-ink)" : "bg-(--skin-lcd-off)")}
                    />
                  ))}
                </span>
              );
            })}
          </span>
          <span className="text-[11px] font-bold leading-none tabular-nums">
            {hasTrack && canSeek ? formatSkinTime(shownTime) : "-:--"}
          </span>
        </div>

        <SegmentSlider
          label="Song position"
          valueText={`${formatSkinTime(shownTime)} of ${formatSkinTime(duration)}`}
          value={shownTime}
          max={duration || 1}
          share={progress}
          segments={PROGRESS_SEGMENTS}
          disabled={!canSeek}
          onInput={seek.setPreview}
          onCommit={seek.commit}
        />
        <div className="relative flex items-center gap-1.5">
          <span className="text-[7px] font-bold tracking-[0.14em]" aria-hidden="true">VOL</span>
          <div className="flex-1">
            <SegmentSlider
              label="Volume"
              valueText={`${Math.round(volume * 100)}%`}
              value={volume}
              max={1}
              share={volume}
              segments={VOLUME_SEGMENTS}
              onInput={onVolume}
            />
          </div>
        </div>

        {/* Pixel grid and a sliver of glare over the whole screen. */}
        <span
          className="pointer-events-none absolute inset-0 bg-[repeating-linear-gradient(0deg,var(--skin-lcd-off)_0_1px,transparent_1px_3px),repeating-linear-gradient(90deg,var(--skin-lcd-off)_0_1px,transparent_1px_3px)] opacity-50"
          aria-hidden="true"
        />
        <span
          className="pointer-events-none absolute inset-0 bg-[linear-gradient(155deg,var(--skin-lcd-glare)_0%,transparent_34%)]"
          aria-hidden="true"
        />
      </div>

      {/* ── Transport ───────────────────────────────────────────────── */}
      <GelButton label="Previous" onClick={onPrevious} className="left-[194px] top-5">
        <SkipPreviousIcon size={13} aria-hidden="true" />
      </GelButton>
      <GelButton label="Next" onClick={onNext} className="left-[194px] top-[92px]">
        <SkipNextIcon size={13} aria-hidden="true" />
      </GelButton>

      <button
        type="button"
        onClick={onTogglePlay}
        aria-label={isLoading ? "Loading song" : isPlaying ? "Pause" : "Play"}
        title={isLoading ? "Loading song" : isPlaying ? "Pause" : "Play"}
        className={cn(
          "group/orb absolute left-[227px] top-[35px] grid size-[70px] place-items-center rounded-full p-1",
          "bg-[conic-gradient(from_200deg,var(--skin-chrome-lo),var(--skin-chrome-hi),var(--skin-chrome),var(--skin-chrome-lo),var(--skin-chrome-hi),var(--skin-chrome),var(--skin-chrome-lo))]",
          "shadow-[inset_0_0_0_1px_var(--skin-chrome-lo)] transition-transform duration-150 hover:scale-[1.03] active:scale-95",
          SKIN_FOCUS,
        )}
      >
        {/* A glow that breathes while music plays. */}
        <span
          className={cn(
            "pointer-events-none absolute inset-0 rounded-full shadow-[0_0_18px_4px_var(--skin-orb-glow)] transition-opacity duration-500",
            isPlaying ? "opacity-100" : "opacity-0",
            isPlaying && !reduceMotion && "animate-pulse",
          )}
          aria-hidden="true"
        />
        <span
          className={cn(
            "relative grid size-full place-items-center overflow-hidden rounded-full text-(--skin-chrome-hi)",
            "bg-[radial-gradient(circle_at_50%_88%,var(--skin-orb-hi)_0%,var(--skin-orb)_38%,var(--skin-orb-deep)_80%)]",
            "shadow-[inset_0_-4px_10px_var(--skin-orb-glow),inset_0_0_0_1px_var(--skin-orb-deep)] transition-[filter] group-hover/orb:brightness-110",
          )}
        >
          <span
            className="absolute inset-x-[14%] top-[5%] h-[46%] rounded-[50%] bg-[linear-gradient(180deg,var(--skin-specular),var(--skin-specular-fade))]"
            aria-hidden="true"
          />
          <span className="relative">
            {isLoading ? (
              <SpinnerSteps size={22} color="currentColor" />
            ) : isPlaying ? (
              <PauseActiveIcon size={26} aria-hidden="true" />
            ) : (
              <PlayActiveIcon size={26} className="translate-x-0.5" aria-hidden="true" />
            )}
          </span>
        </span>
      </button>
    </div>
  );
}

/**
 * An LCD segment bar over a real, invisible slider: the segments are the look, the input is
 * what makes dragging, arrow keys and screen readers work.
 */
function SegmentSlider({
  label,
  valueText,
  value,
  max,
  share,
  segments,
  disabled = false,
  onInput,
  onCommit,
}: {
  label: string;
  valueText: string;
  value: number;
  max: number;
  share: number;
  segments: number;
  disabled?: boolean;
  onInput: (value: number) => void;
  onCommit?: (value: number) => void;
}) {
  const lit = Math.round(Math.min(1, Math.max(0, share)) * segments);
  return (
    <div className="relative h-2">
      <input
        type="range"
        min={0}
        max={max}
        step="any"
        value={value}
        disabled={disabled}
        onChange={(event) => onInput(Number(event.currentTarget.value))}
        onPointerUp={(event) => onCommit?.(Number(event.currentTarget.value))}
        onKeyUp={(event) => onCommit?.(Number(event.currentTarget.value))}
        aria-label={label}
        aria-valuetext={valueText}
        className="peer absolute inset-0 z-10 h-full w-full cursor-pointer opacity-0 disabled:cursor-default"
      />
      <span
        className="flex h-full gap-px rounded-[2px] peer-focus-visible:ring-2 peer-focus-visible:ring-ring"
        aria-hidden="true"
      >
        {Array.from({ length: segments }, (_, index) => (
          <span
            key={index}
            className={cn("flex-1 rounded-[1px]", index < lit ? "bg-(--skin-lcd-ink)" : "bg-(--skin-lcd-off)")}
          />
        ))}
      </span>
    </div>
  );
}

/** A round silver gel key with its own specular cap; it sinks a little when pressed. */
function GelButton({
  label,
  onClick,
  className,
  children,
}: {
  label: string;
  onClick: () => void;
  className: string;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      aria-label={label}
      title={label}
      className={cn(
        "absolute grid size-7 place-items-center overflow-hidden rounded-full text-(--skin-chrome-ink)",
        "bg-[radial-gradient(circle_at_50%_85%,var(--skin-chrome-hi),var(--skin-chrome)_45%,var(--skin-chrome-lo))]",
        "shadow-[inset_0_0_0_1px_var(--skin-chrome-lo),inset_0_-2px_3px_var(--skin-chrome-lo)]",
        "transition-[transform,filter] duration-100 hover:brightness-110 active:scale-90",
        SKIN_FOCUS,
        className,
      )}
    >
      <span
        className="absolute inset-x-[18%] top-[6%] h-[45%] rounded-full bg-[linear-gradient(180deg,var(--skin-specular),var(--skin-specular-fade))]"
        aria-hidden="true"
      />
      <span className="relative">{children}</span>
    </button>
  );
}

/** A tiny button drawn on the LCD itself; it inverts like a selected segment. */
function LcdButton({
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
        "grid size-3.5 place-items-center rounded-[3px] transition-colors hover:bg-(--skin-lcd-ink) hover:text-(--skin-lcd-hi)",
        SKIN_FOCUS,
      )}
    >
      {children}
    </button>
  );
}
