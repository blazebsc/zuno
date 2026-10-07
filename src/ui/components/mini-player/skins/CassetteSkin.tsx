import { type CSSProperties, type ReactNode, type Ref } from "react";
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
  SkinTitle,
  useSeekPreview,
  useSpin,
  type MiniSkinProps,
} from "./shared";

/*
 * Tape geometry. Each pack is one circle centred on its hub, so the same circle fills the hub
 * hole and shows as a curved edge in the centre window — the way a real tape looks through its
 * label. The hub centres sit HUB_OFFSET outside the window's edges.
 */
const HOLE = 34;
const WINDOW_WIDTH = 72;
const WINDOW_HEIGHT = 24;
const WINDOW_GAP = 6;
const HUB_OFFSET = WINDOW_GAP + HOLE / 2;
const PACK_MIN = 24;
const PACK_MAX = 84;
const VOLUME_SEGMENTS = 10;

function packSize(share: number): number {
  return PACK_MIN + (PACK_MAX - PACK_MIN) * share;
}

/**
 * A compact cassette. The tape winds from the left spool to the right as the song plays — in
 * the hub holes and across the window — and the head plate along the bottom holds the deck.
 */
export function CassetteSkin({
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
  const seek = useSeekPreview(currentTime, onSeek);
  const shownTime = seek.preview ?? currentTime;
  const progress = clampProgress(shownTime, duration);
  const hasTrack = title !== null;
  const canSeek = duration > 0;
  // Hubs turn while the tape moves, and keep turning while a song loads.
  const spinning = isPlaying || isLoading;
  const leftHub = useSpin<HTMLSpanElement>(spinning, reduceMotion, 2.4);
  const rightHub = useSpin<HTMLSpanElement>(spinning, reduceMotion, 2.4);
  const leftPack = packSize(1 - progress);
  const rightPack = packSize(progress);
  const status = isError
    ? "Couldn't play this song"
    : isLoading
      ? "Loading…"
      : artist ?? (hasTrack ? "" : "Side A · insert a song");
  const led = isError
    ? "bg-(--skin-led-error) shadow-[0_0_6px_var(--skin-led-error)]"
    : isLoading
      ? "bg-(--skin-led-busy) shadow-[0_0_6px_var(--skin-led-busy)] motion-safe:animate-pulse"
      : isPlaying
        ? "bg-(--skin-led-on) shadow-[0_0_6px_var(--skin-led-on)]"
        : "bg-(--skin-led-off)";

  return (
    <div
      data-mini-skin="cassette"
      className={cn(
        "relative h-[164px] w-[256px] rounded-[16px]",
        "bg-[linear-gradient(180deg,var(--skin-shell-hi),var(--skin-shell)_42%,var(--skin-shell-lo))]",
        "shadow-[inset_0_1px_0_var(--skin-bevel-hi),inset_0_-2px_0_var(--skin-bevel-lo)]",
      )}
    >
      <Screw className="left-1 top-1" />
      <Screw className="right-1 top-1" />
      <Screw className="bottom-1 left-1" />
      <Screw className="bottom-1 right-1" />

      {/* The label: a sticker inset clear of the screws, with the window cut through it. */}
      <div
        className={cn(
          "group/label absolute left-[14px] top-3 flex h-[104px] w-[228px] flex-col overflow-hidden rounded-[9px] text-(--skin-label-ink)",
          "bg-(--skin-label) bg-[repeating-linear-gradient(180deg,transparent_0_12px,var(--skin-label-rule)_12px_13px)]",
        )}
      >
        <span
          className="h-1 shrink-0 bg-[linear-gradient(90deg,var(--skin-stripe-a)_0_33%,var(--skin-stripe-b)_33%_66%,var(--skin-stripe-c)_66%)]"
          aria-hidden="true"
        />

        <div className="flex items-center gap-2 px-2 pt-1.5">
          <TrackArtwork
            artworkUrl={artworkUrl ?? undefined}
            className="size-6 shrink-0 rounded-[5px] bg-(--skin-label-rule) ring-1 ring-(--skin-label-rule)"
            iconSize={11}
            loading="eager"
            size={24}
          />
          <div className="min-w-0 flex-1 leading-tight">
            <SkinTitle
              text={title ?? "Nothing playing"}
              className="text-[12px] font-bold tracking-[-0.01em]"
            />
            <p
              className={cn(
                "truncate text-[10px]",
                isError ? "font-semibold text-(--skin-led-error)" : "opacity-60",
              )}
            >
              {status}
            </p>
          </div>
          <LabelButton label="Restore main window" onClick={onRestore}>
            <ArrowUpIcon size={11} aria-hidden="true" />
          </LabelButton>
          <LabelButton label="Close mini player" onClick={onClose}>
            <CloseIcon size={11} aria-hidden="true" />
          </LabelButton>
        </div>

        <div className="relative mt-1.5 flex h-10 items-center justify-between px-2">
          <span className="w-4 text-[18px] font-black leading-none text-(--skin-stripe-a)" aria-hidden="true">
            A
          </span>

          <div className="flex items-center" style={{ gap: WINDOW_GAP }} aria-hidden="true">
            <HubHole pack={leftPack} spinRef={leftHub} />
            <div
              className="relative overflow-hidden rounded-[6px] bg-(--skin-hole) shadow-[inset_0_2px_4px_var(--skin-bevel-lo)]"
              style={{ width: WINDOW_WIDTH, height: WINDOW_HEIGHT }}
            >
              <Pack size={leftPack} centerX={-HUB_OFFSET} />
              <Pack size={rightPack} centerX={WINDOW_WIDTH + HUB_OFFSET} />
              <span className="absolute inset-x-0 bottom-[3px] h-px bg-(--skin-tape-hi)" />
              <span className="absolute inset-0 bg-[linear-gradient(110deg,transparent_25%,var(--skin-glass)_42%,transparent_58%)]" />
            </div>
            <HubHole pack={rightPack} spinRef={rightHub} />
          </div>

          <span className="w-8 text-right font-mono text-[10px] font-semibold tabular-nums">
            {hasTrack && canSeek ? formatSkinTime(shownTime) : "--:--"}
          </span>

          <div
            className={cn(
              "pointer-events-none absolute inset-0 grid place-items-center transition-opacity duration-200",
              volumeVisible ? "opacity-100" : "opacity-0",
            )}
            role="status"
            aria-live="polite"
          >
            <span className="flex items-center gap-2 rounded-full bg-(--skin-hole) px-3 py-1 text-[9px] font-bold tracking-[0.14em] text-(--skin-hub)">
              VOL
              <span className="flex items-end gap-[2px]" aria-hidden="true">
                {Array.from({ length: VOLUME_SEGMENTS }, (_, index) => (
                  <span
                    key={index}
                    className={cn(
                      "w-[3px] rounded-[1px]",
                      index < Math.round(volume * VOLUME_SEGMENTS) ? "bg-(--skin-led-on)" : "bg-(--skin-led-off)",
                    )}
                    style={{ height: 4 + index * 0.8 }}
                  />
                ))}
              </span>
              <span className="w-7 text-right font-mono tabular-nums">{Math.round(volume * 100)}%</span>
            </span>
          </div>
        </div>

        {/* Seek rail along the foot of the label; its thumb shows on hover, drag or focus. */}
        <input
          type="range"
          min={0}
          max={duration || 1}
          step="any"
          value={shownTime}
          disabled={!canSeek}
          onChange={(event) => seek.setPreview(Number(event.currentTarget.value))}
          onPointerUp={(event) => seek.commit(Number(event.currentTarget.value))}
          onKeyUp={(event) => seek.commit(Number(event.currentTarget.value))}
          aria-label="Song position"
          aria-valuetext={`${formatSkinTime(shownTime)} of ${formatSkinTime(duration)}`}
          title={canSeek ? `-${formatSkinTime(duration - shownTime)}` : undefined}
          className={cn(
            "mx-2 mt-auto mb-1.5 h-2.5 cursor-pointer appearance-none rounded-full bg-transparent disabled:cursor-default",
            "[&::-webkit-slider-runnable-track]:h-[3px] [&::-webkit-slider-runnable-track]:rounded-full",
            "[&::-webkit-slider-runnable-track]:bg-[linear-gradient(to_right,var(--skin-stripe-a)_var(--progress),var(--skin-label-rule)_var(--progress))]",
            "[&::-webkit-slider-thumb]:-mt-[3px] [&::-webkit-slider-thumb]:size-[9px] [&::-webkit-slider-thumb]:appearance-none [&::-webkit-slider-thumb]:rounded-full",
            "[&::-webkit-slider-thumb]:bg-(--skin-label-ink) [&::-webkit-slider-thumb]:opacity-0 [&::-webkit-slider-thumb]:transition-opacity",
            "group-hover/label:[&::-webkit-slider-thumb]:opacity-100 focus-visible:[&::-webkit-slider-thumb]:opacity-100 active:[&::-webkit-slider-thumb]:opacity-100",
            SKIN_FOCUS,
          )}
          style={{ "--progress": `${progress * 100}%` } as CSSProperties}
        />
      </div>

      {/* The head plate: the tapered foot of a real cassette, carrying the transport. */}
      <div
        className="absolute bottom-0 left-[28px] grid h-10 w-[200px] grid-cols-[1fr_auto_1fr] items-center bg-[linear-gradient(180deg,var(--skin-bevel-hi)_0_1px,var(--skin-plate)_1px)] px-5"
        style={{ clipPath: "polygon(7% 0, 93% 0, 100% 100%, 0 100%)" }}
      >
        <span className="flex items-center gap-2" aria-hidden="true">
          <CapstanHole />
          <span className={cn("size-1.5 rounded-full transition-colors", led)} />
        </span>
        <span className="flex items-center gap-1.5">
          <DeckKey label="Previous" onClick={onPrevious}>
            <SkipPreviousIcon size={12} aria-hidden="true" />
          </DeckKey>
          <DeckKey label={isLoading ? "Loading song" : isPlaying ? "Pause" : "Play"} onClick={onTogglePlay} wide>
            {isLoading ? (
              <SpinnerSteps size={12} color="currentColor" />
            ) : isPlaying ? (
              <PauseActiveIcon size={13} aria-hidden="true" />
            ) : (
              <PlayActiveIcon size={13} aria-hidden="true" />
            )}
          </DeckKey>
          <DeckKey label="Next" onClick={onNext}>
            <SkipNextIcon size={12} aria-hidden="true" />
          </DeckKey>
        </span>
        <span className="flex justify-end" aria-hidden="true">
          <CapstanHole />
        </span>
      </div>
    </div>
  );
}

/** A hub hole in the label: the tape pack around a toothed, turning hub. */
function HubHole({ pack, spinRef }: { pack: number; spinRef: Ref<HTMLSpanElement> }) {
  return (
    <span
      className="relative grid shrink-0 place-items-center overflow-hidden rounded-full bg-(--skin-hole) shadow-[inset_0_2px_4px_var(--skin-bevel-lo)] ring-1 ring-(--skin-label-rule)"
      style={{ width: HOLE, height: HOLE }}
    >
      <span
        className="absolute rounded-full bg-[repeating-radial-gradient(circle,var(--skin-tape)_0_1px,var(--skin-tape-hi)_1px_2px)] transition-[width,height] duration-700 ease-out"
        style={{ width: pack, height: pack }}
      />
      <span
        ref={spinRef}
        className="relative grid size-[22px] place-items-center rounded-full bg-[repeating-conic-gradient(var(--skin-hub)_0deg_40deg,transparent_40deg_60deg)]"
      >
        <span className="grid size-4 place-items-center rounded-full bg-(--skin-hub)">
          <span className="size-2 rounded-full bg-(--skin-hole)" />
        </span>
      </span>
    </span>
  );
}

/** The same pack seen through the centre window: only its curved edge reaches in. */
function Pack({ size, centerX }: { size: number; centerX: number }) {
  return (
    <span
      className="absolute rounded-full bg-[repeating-radial-gradient(circle,var(--skin-tape)_0_1px,var(--skin-tape-hi)_1px_2px)] transition-[width,height,left,top] duration-700 ease-out"
      style={{
        width: size,
        height: size,
        left: centerX - size / 2,
        top: WINDOW_HEIGHT / 2 - size / 2,
      }}
    />
  );
}

function CapstanHole() {
  return <span className="size-2 rounded-full bg-(--skin-hole) shadow-[inset_0_1px_2px_var(--skin-bevel-lo)]" />;
}

function Screw({ className }: { className: string }) {
  return (
    <span
      className={cn(
        "absolute grid size-[7px] place-items-center rounded-full bg-[radial-gradient(circle_at_35%_30%,var(--skin-screw-hi),var(--skin-screw)_70%)]",
        className,
      )}
      aria-hidden="true"
    >
      <span className="h-px w-[5px] rotate-45 bg-(--skin-shell-lo)" />
    </span>
  );
}

/** A deck key: it sinks when pressed. */
function DeckKey({
  label,
  onClick,
  wide = false,
  children,
}: {
  label: string;
  onClick: () => void;
  wide?: boolean;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      aria-label={label}
      title={label}
      className={cn(
        "grid h-6 shrink-0 place-items-center rounded-md text-(--skin-key-ink)",
        "bg-[linear-gradient(180deg,var(--skin-key-hi),var(--skin-key))] shadow-[inset_0_-2px_0_var(--skin-key-shade)]",
        "transition-[transform,box-shadow,filter] duration-75 hover:brightness-105",
        "active:translate-y-px active:shadow-[inset_0_-1px_0_var(--skin-key-shade)]",
        wide ? "w-9" : "w-7",
        SKIN_FOCUS,
      )}
    >
      {children}
    </button>
  );
}

function LabelButton({
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
        "grid size-4 shrink-0 place-items-center rounded-full opacity-55 transition-[opacity,background-color] hover:bg-(--skin-label-rule) hover:opacity-100",
        SKIN_FOCUS,
      )}
    >
      {children}
    </button>
  );
}
