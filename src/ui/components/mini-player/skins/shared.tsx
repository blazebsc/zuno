import { useEffect, useRef, useState } from "react";
import { cn } from "@/lib/utils";
import { Marquee } from "@/components/motion/marquee";

/**
 * What every mini player skin is handed. Skins are purely presentational: the host owns the
 * window, the drag, the keyboard and the playback bridge, which is also what lets Settings
 * preview them.
 */
export interface MiniSkinProps {
  title: string | null;
  artist: string | null;
  artworkUrl: string | null;
  isPlaying: boolean;
  isLoading: boolean;
  isError: boolean;
  currentTime: number;
  duration: number;
  /** 0–1, muted reads as 0. */
  volume: number;
  /** True for a moment after the volume changes, so a skin can flash a readout. */
  volumeVisible: boolean;
  reduceMotion: boolean;
  onTogglePlay: () => void;
  onNext: () => void;
  onPrevious: () => void;
  onSeek: (time: number) => void;
  /** 0–1. For skins with their own volume control; the wheel and arrow keys are the host's. */
  onVolume: (volume: number) => void;
  onRestore: () => void;
  onClose: () => void;
}

/** One focus ring for every skin control, per the app's UI rules. */
export const SKIN_FOCUS =
  "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring";

export function formatSkinTime(seconds: number): string {
  const whole = Math.max(0, Math.floor(Number.isFinite(seconds) ? seconds : 0));
  return `${Math.floor(whole / 60)}:${String(whole % 60).padStart(2, "0")}`;
}

export function clampProgress(time: number, duration: number): number {
  return duration > 0 ? Math.min(1, Math.max(0, time / duration)) : 0;
}

const SPIN_RAMP_MS = 900;

/**
 * Spins an element that winds up to speed and coasts to a stop, like a real motor, instead of
 * starting and freezing dead. The Web Animations API lets the rate be eased while the angle
 * carries on from wherever it is.
 */
export function useSpin<T extends HTMLElement>(
  spinning: boolean,
  reduceMotion: boolean,
  secondsPerTurn: number,
) {
  const ref = useRef<T | null>(null);
  const animationRef = useRef<Animation | null>(null);

  useEffect(() => {
    const element = ref.current;
    if (!element || reduceMotion || typeof element.animate !== "function") return;
    const animation = element.animate(
      [{ transform: "rotate(0deg)" }, { transform: "rotate(360deg)" }],
      { duration: secondsPerTurn * 1000, iterations: Infinity },
    );
    animation.playbackRate = 0;
    animationRef.current = animation;
    return () => {
      animation.cancel();
      animationRef.current = null;
    };
  }, [reduceMotion, secondsPerTurn]);

  useEffect(() => {
    const animation = animationRef.current;
    if (!animation) return;
    const from = animation.playbackRate;
    const to = spinning ? 1 : 0;
    const startedAt = performance.now();
    let frame = 0;
    const step = (now: number) => {
      const t = Math.min(1, (now - startedAt) / SPIN_RAMP_MS);
      animation.playbackRate = from + (to - from) * (1 - (1 - t) ** 3);
      if (t < 1) frame = requestAnimationFrame(step);
    };
    frame = requestAnimationFrame(step);
    return () => cancelAnimationFrame(frame);
  }, [spinning, reduceMotion, secondsPerTurn]);

  return ref;
}

/**
 * The position a seek is heading for, held until playback reports it — clearing on a timer
 * alone made the display jump back to the old position whenever the sync ran late.
 */
export function useSeekPreview(currentTime: number, onSeek: (time: number) => void) {
  const [preview, setPreview] = useState<number | null>(null);
  const pendingRef = useRef<number | null>(null);
  const timerRef = useRef<number | undefined>(undefined);

  useEffect(() => {
    if (pendingRef.current === null || Math.abs(currentTime - pendingRef.current) > 0.75) return;
    pendingRef.current = null;
    window.clearTimeout(timerRef.current);
    setPreview(null);
  }, [currentTime]);

  useEffect(() => () => window.clearTimeout(timerRef.current), []);

  const commit = (time: number) => {
    onSeek(time);
    pendingRef.current = time;
    setPreview(time);
    window.clearTimeout(timerRef.current);
    // A seek the player never confirms (a track change mid-drag) still lets go eventually.
    timerRef.current = window.setTimeout(() => {
      pendingRef.current = null;
      setPreview(null);
    }, 2500);
  };

  return { preview, setPreview, commit };
}

/** A title that scrolls only when it does not fit — a permanent marquee is just noise. */
export function SkinTitle({ text, className }: { text: string; className?: string }) {
  const viewportRef = useRef<HTMLSpanElement | null>(null);
  const textRef = useRef<HTMLSpanElement | null>(null);
  const [overflowing, setOverflowing] = useState(false);

  useEffect(() => {
    const viewport = viewportRef.current;
    const measured = textRef.current;
    if (!viewport || !measured) return;
    const update = () => setOverflowing(measured.scrollWidth - viewport.clientWidth > 1);
    update();
    const observer = new ResizeObserver(update);
    observer.observe(viewport);
    observer.observe(measured);
    return () => observer.disconnect();
  }, [text]);

  return (
    <span ref={viewportRef} className={cn("relative block min-w-0 overflow-hidden", className)} title={text}>
      <span
        ref={textRef}
        className={cn("block whitespace-nowrap", overflowing && "invisible absolute")}
        aria-hidden={overflowing}
      >
        {text}
      </span>
      {overflowing && (
        <Marquee speed={16} gap="2.5rem">
          <span className="whitespace-nowrap">{text}</span>
        </Marquee>
      )}
    </span>
  );
}
