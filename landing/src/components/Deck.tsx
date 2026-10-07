import type { CSSProperties } from "react";
import { Spectrum } from "./Spectrum";
import { PauseIcon, PlayIcon } from "./icons";
import { FOCUS_RING, cn } from "./ui";
import { TRACK, seek, togglePlayback, usePlayer } from "../audio/demoPlayer";
import { formatTime } from "../audio/track";
import { useReducedMotion } from "../useReducedMotion";

/** Pivot at (88, 14) in the deck's 0–100 box; the arm rotates about it in styles.css. */
function Tonearm() {
  return (
    <svg className="pointer-events-none absolute inset-0 size-full overflow-visible" viewBox="0 0 100 100" aria-hidden="true">
      <circle cx="88" cy="14" r="6.5" className="arm-base" />
      <g className="tonearm">
        <line x1="88" y1="14" x2="90.3" y2="6.2" className="arm-weight" />
        <line x1="88" y1="14" x2="70.6" y2="73.5" className="arm-rod" />
        <line x1="70.6" y1="73.5" x2="67.8" y2="81.6" className="arm-head" />
      </g>
      <circle cx="88" cy="14" r="2.6" className="arm-pivot" />
    </svg>
  );
}

function Record() {
  return (
    // A mouse shortcut for the play button below, which is the accessible control.
    <div className="record" onClick={togglePlayback} aria-hidden="true">
      <div className="record-spin">
        <div className="record-grooves" />
        <svg className="record-text" viewBox="0 0 100 100">
          <path id="deck-label-ring" d="M50 50m-23.5 0a23.5 23.5 0 1 1 47 0a23.5 23.5 0 1 1-47 0" fill="none" />
          <text>
            <textPath href="#deck-label-ring">
              LOW TIDE · TOM RHODES · HUSTLE STANDARD · ZUNO_ · LOW TIDE · TOM RHODES · HUSTLE STANDARD ·
            </textPath>
          </text>
        </svg>
        {/* Text above and below the spindle, the way a real label keeps clear of the hole. */}
        <div className="record-label">
          <span className="text-[19cqi] font-bold leading-none tracking-[-0.04em]">zuno_</span>
          <span className="spindle" />
          <span className="font-mono text-[6.5cqi] uppercase tracking-[0.16em] opacity-80">side a · 33 rpm</span>
        </div>
      </div>
    </div>
  );
}

function NowPlaying() {
  const { playing, started, time, duration } = usePlayer();
  const progress = duration > 0 ? (time / duration) * 100 : 0;

  return (
    <div className="relative mx-auto -mt-3 w-full max-w-[22rem]">
      {started ? null : (
        <p className="nudge pointer-events-none absolute -top-8 left-0 right-0 text-center font-mono text-[12px] text-primary">
          press play
        </p>
      )}
      <div className="flex items-center gap-3.5 rounded-[1.4rem] bg-card/50 p-2.5 pr-4 backdrop-blur-xl">
        <button
          type="button"
          onClick={togglePlayback}
          aria-label={playing ? `Pause ${TRACK.title}` : `Play ${TRACK.title} by ${TRACK.artist}`}
          className={cn(
            "grid size-12 shrink-0 place-items-center rounded-full bg-foreground text-background transition-transform hover:scale-105 active:scale-95",
            FOCUS_RING,
          )}
        >
          {playing ? <PauseIcon size={18} /> : <PlayIcon size={18} className="translate-x-px" />}
        </button>
        <div className="min-w-0 flex-1">
          <div className="flex items-baseline justify-between gap-3">
            <p className="truncate text-[15px] font-semibold tracking-[-0.01em]">{TRACK.title}</p>
            <p className="shrink-0 font-mono text-[11px] tabular-nums text-muted-foreground">
              {formatTime(time)} / {formatTime(duration)}
            </p>
          </div>
          <p className="truncate text-[13px] text-muted-foreground">{TRACK.artist}</p>
          <input
            type="range"
            min={0}
            max={duration || 1}
            step="any"
            value={time}
            onChange={(event) => {
              if (!started) togglePlayback();
              seek(Number(event.currentTarget.value));
            }}
            aria-label="Seek"
            aria-valuetext={`${formatTime(time)} of ${formatTime(duration)}`}
            className={cn("seek mt-1 w-full", FOCUS_RING)}
            style={{ "--seek": `${progress}%` } as CSSProperties}
          />
        </div>
      </div>
    </div>
  );
}

export function Deck() {
  const reduced = useReducedMotion();

  return (
    <div className="relative mx-auto w-full max-w-[24rem] lg:mr-6">
      <div className="deck">
        <div className="deck-glow" aria-hidden="true" />
        <Spectrum className="spectrum" />
        <Record />
        <Tonearm />
        <div className="screen-ghost deck-ghost" aria-hidden="true">
          <video
            className="float size-full object-cover"
            src="./zuno-character.mp4"
            autoPlay={!reduced}
            muted
            loop
            playsInline
          />
        </div>
      </div>
      <NowPlaying />
    </div>
  );
}
