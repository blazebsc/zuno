import type { Lyrics, LyricsSourceAttempt } from "../types";

export interface LyricsSource {
  id: string;
  label: string;
  timeoutMs: number;
  /**
   * Which round this source runs in. Everything in a wave is raced in parallel; wave 2 only
   * runs if wave 1 came back empty, because its two sources are expensive YouTube calls and
   * a synced hit makes them pointless.
   */
  wave: 1 | 2;
  /**
   * The source matches on recording length and cannot be asked anything useful without one.
   *
   * Declared here rather than left as an early `return null` inside the fetcher, because a
   * fetcher that quietly returns nothing is indistinguishable from one that searched and
   * found nothing — which is how two of the sources went missing on every track without a
   * duration while the UI cheerfully reported "No match".
   */
  requiresDuration?: boolean;
  /** Why it sits where it does. Shown in the UI, so it has to read as a sentence. */
  note: string;
}

/**
 * Lyric sources in preference order, best first.
 *
 * The ranking is about how far the *timing* can be trusted, not how complete the words are.
 * This screen follows along with the song, so a line-synced sheet matched to this exact
 * recording beats a flawless transcript with no timestamps — the latter cannot drive
 * anything, however correct it is.
 */
export const LYRICS_SOURCES: LyricsSource[] = [
  {
    id: "lrclib-exact",
    label: "LRCLIB",
    timeoutMs: 2_500,
    wave: 1,
    requiresDuration: true,
    note: "Line-synced, matched on title, artist and exact duration — the same recording.",
  },
  {
    id: "betterlyrics",
    label: "BetterLyrics",
    timeoutMs: 3_500,
    wave: 1,
    note: "Line-synced TTML with explicit end times, community curated.",
  },
  {
    id: "lrclib-search",
    label: "LRCLIB search",
    timeoutMs: 4_500,
    wave: 1,
    requiresDuration: true,
    note: "Same corpus, matched by text within two seconds of duration — can land on a different master.",
  },
  {
    id: "simpmusic",
    label: "SimpMusic",
    timeoutMs: 3_500,
    wave: 1,
    note: "Line-synced and keyed to this exact video, but community entries are sometimes translations.",
  },
  {
    id: "youtube-transcript",
    label: "YouTube transcript",
    timeoutMs: 6_000,
    wave: 2,
    note: "Timed, but machine transcribed: right often enough to try, wrong in ways worth ranking below real lyrics.",
  },
  {
    id: "youtube-music",
    label: "YouTube Music",
    timeoutMs: 6_000,
    wave: 2,
    note: "The official words for this exact video, so never the wrong song — but carries no timings at all.",
  },
];

/**
 * Position in the table, with the listener's preferred source promoted ahead of everything.
 *
 * Anything unknown — a source removed since the preference was stored — sorts to the end
 * rather than to the front, so a stale setting degrades to the default order.
 */
export function rankOfSource(id: string | undefined, preferredId?: string): number {
  if (id !== undefined && preferredId !== undefined && id === preferredId) return -1;
  const index = LYRICS_SOURCES.findIndex((source) => source.id === id);
  return index === -1 ? Number.MAX_SAFE_INTEGER : index;
}

/**
 * The best result available, by table rank rather than by who answered first.
 *
 * Racing sources means the fastest wins by default, which is the opposite of what is wanted:
 * a two-second exact match is worth waiting for over a 200ms search hit that guessed at the
 * recording. A result with no lines is not a result.
 */
export function pickBestLyrics<T extends { source: LyricsSource; lyrics: Lyrics | null }>(
  candidates: T[],
  preferredId?: string,
): T | undefined {
  return [...candidates]
    .sort(
      (left, right) =>
        rankOfSource(left.source.id, preferredId) - rankOfSource(right.source.id, preferredId),
    )
    .find((candidate) => (candidate.lyrics?.lines.length ?? 0) > 0);
}

/**
 * The rounds to run, in order.
 *
 * A preferred source joins the first round wherever it normally sits, because the point of
 * choosing it is not to wait behind four other hosts. Everything else keeps its wave, so
 * promoting the cheap YouTube Music lookup does not also drag the expensive transcript call
 * into every request.
 */
export function planLyricsWaves(preferredId?: string): LyricsSource[][] {
  const first = LYRICS_SOURCES.filter(
    (source) => source.wave === 1 || source.id === preferredId,
  );
  const rest = LYRICS_SOURCES.filter((source) => !first.includes(source));
  return rest.length > 0 ? [first, rest] : [first];
}

/** A source that was never reached, so the UI can show the order instead of implying a failure. */
export function skippedAttempt(source: LyricsSource, reason = "Not needed"): LyricsSourceAttempt {
  return {
    id: source.id,
    label: source.label,
    status: "skipped",
    durationMs: 0,
    detail: reason,
  };
}

/**
 * Why this source cannot be asked about this track, or null if it can.
 *
 * Checked before the request rather than inside it, so an unanswerable source reports the
 * actual reason instead of a miss it never went looking for.
 */
export function unmetPrecondition(
  source: LyricsSource,
  track: { durationSec?: number },
): string | null {
  if (source.requiresDuration && !(track.durationSec && track.durationSec > 0)) {
    return "Needs a track duration to match on";
  }
  return null;
}

/** Attempts in table order, so the list always reads as the priority it actually is. */
export function sortAttempts(
  attempts: LyricsSourceAttempt[],
  preferredId?: string,
): LyricsSourceAttempt[] {
  return [...attempts].sort(
    (left, right) => rankOfSource(left.id, preferredId) - rankOfSource(right.id, preferredId),
  );
}
