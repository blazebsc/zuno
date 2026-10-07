export {};

import { bandLevels, follow, hzToBin, logBandEdges, meanLevel, onset } from "./levels";

function check(condition: boolean, message: string): void {
  if (!condition) throw new Error(`FAILED: ${message}`);
}

const edges = logBandEdges(4, 40, 640);
check(edges.length === 5 && Math.abs(edges[2] - 160) < 1e-9 && Math.abs(edges[4] - 640) < 1e-9, "edges double per band");

check(hzToBin(0, 48000, 1024) === 0 && hzToBin(30000, 48000, 1024) === 1023, "bins clamp to the range");
check(hzToBin(12000, 48000, 1024) === 512, "half of Nyquist is the middle bin");

const bins = new Uint8Array(1024);
const out = new Float32Array(4);
check(bandLevels(bins, edges, 48000, out).every((level) => level === 0), "silence reads zero");

bins.fill(255);
check(bandLevels(bins, edges, 48000, out).every((level) => level === 1), "full scale reads one");

bins.fill(0);
bins[hzToBin(200, 48000, 1024)] = 255;
bandLevels(bins, edges, 48000, out);
check(out[2] === 1 && out[0] === 0 && out[1] === 0 && out[3] === 0, "a 200 Hz tone lands in 160–320 Hz only");

check(follow(0, 1, 0.5, 0.1) === 0.5 && Math.abs(follow(1, 0, 0.5, 0.1) - 0.9) < 1e-9, "rises fast, falls slow");
check(meanLevel([0, 1, 1, 0], 1, 3) === 1 && meanLevel([1], 0, 0) === 0, "mean of a range");
check(onset(0.5, 0.5) === 0 && onset(0.4, 0.5) === 0, "a held or falling note is not a hit");
check(Math.abs(onset(0.55, 0.5) - 0.5) < 1e-9 && onset(1, 0.5) === 1, "a jump above the average reads as a hit");
check(onset(0.5, 0.01) === 0, "silence never hits");

console.log("levels: ok");
