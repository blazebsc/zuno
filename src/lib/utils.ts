import { clsx, type ClassValue } from "clsx"
import { twMerge } from "tailwind-merge"

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs))
}

/**
 * `m:ss` for a whole-second duration, `h:mm:ss` from an hour up. Callers round `seconds`
 * themselves (floor for elapsed position, ceil for a countdown) — this only formats what it's given.
 */
export function formatMinutesSeconds(seconds: number): string {
  if (!Number.isFinite(seconds)) return "0:00";
  const whole = Math.max(0, Math.floor(seconds));
  const hours = Math.floor(whole / 3600);
  const mins = Math.floor((whole % 3600) / 60);
  const secs = (whole % 60).toString().padStart(2, "0");
  return hours > 0
    ? `${hours}:${mins.toString().padStart(2, "0")}:${secs}`
    : `${mins}:${secs}`;
}
