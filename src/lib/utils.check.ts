/** Self-check for `formatMinutesSeconds`, which every playback time in the app goes through. */
export {};

import { formatMinutesSeconds } from "./utils";

function check(actual: string, expected: string, message: string): void {
  if (actual !== expected) throw new Error(`FAILED: ${message} (got ${actual}, want ${expected})`);
}

check(formatMinutesSeconds(225), "3:45", "under an hour stays m:ss, no leading zero");
check(formatMinutesSeconds(59.9), "0:59", "fractions floor");
check(formatMinutesSeconds(3725), "1:02:05", "an hour and up adds hours, padding minutes");
check(formatMinutesSeconds(-1), "0:00", "a countdown that overshoots clamps to zero");
check(formatMinutesSeconds(Number.NaN), "0:00", "an unknown duration reads as zero");

console.log("formatMinutesSeconds: ok");
