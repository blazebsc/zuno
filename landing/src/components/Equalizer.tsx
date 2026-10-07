import { useMemo, type CSSProperties } from "react";
import { PlayIcon } from "./icons";
import { FOCUS_RING, cn } from "./ui";
import { applyEqPreset, setEqBand, togglePlayback, useEq, usePlayer } from "../audio/demoPlayer";
import { EQ_BANDS_HZ, EQ_MAX_DB, EQ_PRESETS, EQ_STEP_DB, formatHz, responsePath } from "../audio/eq";

const signed = (db: number) => `${db > 0 ? "+" : ""}${db} dB`;

/** The app's ten-band EQ, wired to Web Audio filters on the demo track: it really changes the sound. */
export function Equalizer() {
  const { bands, preset } = useEq();
  const { playing } = usePlayer();
  const curve = useMemo(() => responsePath(bands, 300, 100), [bands]);

  return (
    <div className="flex flex-col gap-5">
      <svg viewBox="0 0 300 100" preserveAspectRatio="none" className="h-32 w-full overflow-visible" aria-hidden="true">
        <line x1="0" y1="50" x2="300" y2="50" className="eq-zero" />
        <path d={`${curve} L300 50 L0 50 Z`} className="eq-fill" />
        <path d={curve} className="eq-curve" />
      </svg>

      <div className="grid grid-cols-10 justify-items-center gap-1">
        {EQ_BANDS_HZ.map((hz, band) => {
          const position = ((bands[band] + EQ_MAX_DB) / (EQ_MAX_DB * 2)) * 100;
          return (
            <div key={hz} className="flex flex-col items-center gap-2">
              <div className="h-28">
                <input
                  type="range"
                  min={-EQ_MAX_DB}
                  max={EQ_MAX_DB}
                  step={EQ_STEP_DB}
                  value={bands[band]}
                  onChange={(event) => setEqBand(band, Number(event.currentTarget.value))}
                  aria-label={`${formatHz(hz)} Hz`}
                  aria-valuetext={signed(bands[band])}
                  className={cn("eq-slider rounded-full", FOCUS_RING)}
                  style={{ "--lo": `${Math.min(50, position)}%`, "--hi": `${Math.max(50, position)}%` } as CSSProperties}
                />
              </div>
              <span className="font-mono text-[10px] text-muted-foreground">{formatHz(hz)}</span>
            </div>
          );
        })}
      </div>

      <div className="flex flex-wrap items-center gap-1.5" role="group" aria-label="Presets">
        {EQ_PRESETS.map((option) => (
          <button
            key={option.name}
            type="button"
            aria-pressed={preset === option.name}
            onClick={() => applyEqPreset(option.name)}
            className={cn(
              "rounded-full px-3 py-1.5 text-[13px] transition-colors",
              FOCUS_RING,
              preset === option.name ? "bg-primary/15 text-foreground" : "text-muted-foreground hover:bg-card hover:text-foreground",
            )}
          >
            {option.name}
          </button>
        ))}
        {playing ? null : (
          <button
            type="button"
            onClick={togglePlayback}
            className={cn(
              "ml-auto flex items-center gap-1.5 rounded-full bg-foreground px-3 py-1.5 text-[13px] font-medium text-background",
              FOCUS_RING,
            )}
          >
            <PlayIcon size={12} />
            Play to hear it
          </button>
        )}
      </div>
    </div>
  );
}
