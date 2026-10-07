// Spectrum maths for the visualisers: log-spaced bands over an AnalyserNode's FFT bins.

/** `count + 1` edges from `minHz` to `maxHz`, evenly spaced on a log axis. */
export function logBandEdges(count: number, minHz: number, maxHz: number): number[] {
  const ratio = (maxHz / minHz) ** (1 / count);
  return Array.from({ length: count + 1 }, (_, index) => minHz * ratio ** index);
}

/** The FFT bin holding `hz`, clamped to the bins that exist. */
export function hzToBin(hz: number, sampleRate: number, binCount: number): number {
  const bin = Math.round((hz / (sampleRate / 2)) * binCount);
  return Math.min(binCount - 1, Math.max(0, bin));
}

/** Each band's loudest bin, 0–1. A band narrower than one bin still reads the bin it sits in. */
export function bandLevels(
  bins: Uint8Array,
  edges: readonly number[],
  sampleRate: number,
  out: Float32Array,
): Float32Array {
  for (let band = 0; band < out.length; band += 1) {
    const from = hzToBin(edges[band], sampleRate, bins.length);
    const to = Math.max(from, hzToBin(edges[band + 1], sampleRate, bins.length) - 1);
    let loudest = 0;
    for (let bin = from; bin <= to; bin += 1) loudest = Math.max(loudest, bins[bin]);
    out[band] = loudest / 255;
  }
  return out;
}

/** Envelope follower: moves `attack` of the way up, `release` of the way down, per frame. */
export function follow(current: number, target: number, attack: number, release: number): number {
  return current + (target - current) * (target > current ? attack : release);
}

export function meanLevel(levels: ArrayLike<number>, from: number, to: number): number {
  let sum = 0;
  for (let index = from; index < to; index += 1) sum += levels[index];
  return to > from ? sum / (to - from) : 0;
}

/** How far a fast envelope sits above a slow one: near 0 on a held note, near 1 on a hit. */
export function onset(fast: number, slow: number): number {
  if (slow < 0.02) return 0;
  return Math.min(1, Math.max(0, ((fast - slow) / slow) * 5));
}
