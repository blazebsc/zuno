export {};

import {
  EQ_BANDS_HZ,
  EQ_PRESETS,
  autoPreampDb,
  formatHz,
  presetFor,
  responseCurveDb,
  responsePath,
  snapGain,
} from "./eq";

function check(condition: boolean, message: string): void {
  if (!condition) throw new Error(`FAILED: ${message}`);
}

check(responseCurveDb(EQ_PRESETS[0].bands).every((db) => Math.abs(db) < 1e-9), "flat is flat");

const peak = Math.max(...responseCurveDb([0, 0, 0, 0, 0, 12, 0, 0, 0, 0]));
check(Math.abs(peak - 12) < 0.25, `one +12 dB band peaks near 12 dB (got ${peak})`);

check(autoPreampDb(EQ_PRESETS[0].bands) === 0, "flat needs no preamp");
check(autoPreampDb(EQ_PRESETS[1].bands) < 0, "a bass boost is pulled down so it cannot clip");

check(snapGain(13) === 12 && snapGain(-0.26) === -0.5 && snapGain(Number.NaN) === 0, "gains clamp and snap");
check(EQ_PRESETS.every((preset) => preset.bands.length === EQ_BANDS_HZ.length), "every preset has ten bands");
check(presetFor([6, 5, 4, 2, 0, 0, 0, 0, 0, 0]) === "Bass", "a preset curve is recognised");
check(presetFor([1, 0, 0, 0, 0, 0, 0, 0, 0, 0]) === null, "a custom curve is not a preset");
check(formatHz(31) === "31" && formatHz(1000) === "1k" && formatHz(16000) === "16k", "band labels");

const flat = responsePath(EQ_PRESETS[0].bands, 200, 100);
check(flat.startsWith("M0.0 50.0") && flat.endsWith("L200.0 50.0"), "a flat curve runs along the midline");

console.log("eq: ok");
