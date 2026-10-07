/** Last.fm's rule: a track over 30 s scrobbles once half of it, or four minutes, has played. */
export function scrobbleAt(durationSec: number): number | null {
  if (!(durationSec > 30)) return null;
  return Math.min(durationSec / 2, 240);
}

export function formatTime(seconds: number): string {
  if (!Number.isFinite(seconds) || seconds < 0) return "0:00";
  const whole = Math.floor(seconds);
  return `${Math.floor(whole / 60)}:${String(whole % 60).padStart(2, "0")}`;
}
