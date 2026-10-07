// The app's equaliser maths (src/ui/settings/equalizerCurve.ts): same bands, Q and presets, so the curve drawn is the one Zuno plays.

export const EQ_BANDS_HZ = [31, 62, 125, 250, 500, 1000, 2000, 4000, 8000, 16000] as const;
export const EQ_MAX_DB = 12;
export const EQ_STEP_DB = 0.5;
/** `BAND_Q` in equalizer.rs; Web Audio's peaking filter uses the same RBJ definition. */
export const EQ_Q = 1.4142136;

const SAMPLE_RATE = 48_000;
const LOWEST_HZ = 1000 * 2 ** -5.5;
const OCTAVES = 10;
const POINTS_PER_OCTAVE = 12;

export interface EqPreset {
  name: string;
  bands: readonly number[];
}

export const EQ_PRESETS: readonly EqPreset[] = [
  { name: "Flat", bands: [0, 0, 0, 0, 0, 0, 0, 0, 0, 0] },
  { name: "Bass", bands: [6, 5, 4, 2, 0, 0, 0, 0, 0, 0] },
  { name: "Warm", bands: [2, 3, 3, 2, 1, 0, -1, -1, -2, -2] },
  { name: "Vocal", bands: [-2, -2, -1, 1, 3, 4, 3, 1, 0, 0] },
  { name: "Treble", bands: [0, 0, 0, 0, 0, 1, 2, 4, 5, 5] },
  { name: "Loudness", bands: [5, 4, 2, 0, -1, -1, 0, 2, 4, 4] },
];

export function snapGain(db: number): number {
  if (!Number.isFinite(db)) return 0;
  const clamped = Math.min(EQ_MAX_DB, Math.max(-EQ_MAX_DB, db));
  return Math.round(clamped / EQ_STEP_DB) * EQ_STEP_DB;
}

export function presetFor(bands: readonly number[]): string | null {
  const match = EQ_PRESETS.find((preset) => preset.bands.every((gain, band) => gain === bands[band]));
  return match?.name ?? null;
}

export function formatHz(hz: number): string {
  return hz >= 1000 ? `${hz / 1000}k` : String(hz);
}

const CURVE_HZ = Array.from(
  { length: OCTAVES * POINTS_PER_OCTAVE + 1 },
  (_, index) => LOWEST_HZ * 2 ** (index / POINTS_PER_OCTAVE),
);

/** 0 at the graph's left edge, 1 at its right, on a log axis; band i sits at (i + 0.5) / 10. */
function frequencyPosition(hz: number): number {
  return Math.log2(hz / LOWEST_HZ) / OCTAVES;
}

/** One peaking biquad's gain at `hz`, from the RBJ coefficients. */
function peakingDb(centreHz: number, gainDb: number, hz: number): number {
  const amplitude = 10 ** (gainDb / 40);
  const omega = (2 * Math.PI * centreHz) / SAMPLE_RATE;
  const alpha = Math.sin(omega) / (2 * EQ_Q);
  const a0 = 1 + alpha / amplitude;
  const b0 = (1 + alpha * amplitude) / a0;
  const b1 = (-2 * Math.cos(omega)) / a0; // a1 is the same value for a peaking filter
  const b2 = (1 - alpha * amplitude) / a0;
  const a2 = (1 - alpha / amplitude) / a0;

  const w = (2 * Math.PI * hz) / SAMPLE_RATE;
  const [cos1, sin1, cos2, sin2] = [Math.cos(w), Math.sin(w), Math.cos(2 * w), Math.sin(2 * w)];
  const numerator = (b0 + b1 * cos1 + b2 * cos2) ** 2 + (b1 * sin1 + b2 * sin2) ** 2;
  const denominator = (1 + b1 * cos1 + a2 * cos2) ** 2 + (b1 * sin1 + a2 * sin2) ** 2;
  return 10 * Math.log10(numerator / denominator);
}

/** The bands' combined gain in dB along `CURVE_HZ`. Neighbours overlap, so two +6s peak above 6. */
export function responseCurveDb(bands: readonly number[]): number[] {
  return CURVE_HZ.map((hz) =>
    bands.reduce((sum, gain, band) => (gain === 0 ? sum : sum + peakingDb(EQ_BANDS_HZ[band], gain, hz)), 0),
  );
}

/** The preamp that puts the curve's peak at 0 dB, so a boost cannot clip. */
export function autoPreampDb(bands: readonly number[]): number {
  const peak = Math.max(...responseCurveDb(bands));
  return snapGain(Math.floor(-peak / EQ_STEP_DB + 1e-6) * EQ_STEP_DB);
}

/** The response as an SVG path, 0 dB at mid-height and ±EQ_MAX_DB at the edges. */
export function responsePath(bands: readonly number[], width: number, height: number): string {
  return responseCurveDb(bands)
    .map((db, index) => {
      const x = frequencyPosition(CURVE_HZ[index]) * width;
      const clamped = Math.max(-EQ_MAX_DB, Math.min(EQ_MAX_DB, db));
      const y = height / 2 - (clamped / EQ_MAX_DB) * (height / 2);
      return `${index === 0 ? "M" : "L"}${x.toFixed(1)} ${y.toFixed(1)}`;
    })
    .join(" ");
}
