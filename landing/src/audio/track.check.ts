export {};

import { formatTime, scrobbleAt } from "./track";

function check(condition: boolean, message: string): void {
  if (!condition) throw new Error(`FAILED: ${message}`);
}

check(scrobbleAt(20) === null && scrobbleAt(Number.NaN) === null, "short or unknown tracks never scrobble");
check(scrobbleAt(238) === 119, "half the track");
check(scrobbleAt(600) === 240, "or four minutes, whichever comes first");
check(formatTime(0) === "0:00" && formatTime(238.9) === "3:58" && formatTime(-1) === "0:00", "m:ss");

console.log("track: ok");
