/**
 * Self-check for offline queue stepping. Run with the whole suite:
 *
 *   npm run check
 */
export {};

import { findPlayableIndex } from "./playableIndex";

function check(condition: boolean, message: string): void {
  if (!condition) throw new Error(`FAILED: ${message}`);
}

// d = downloaded, s = streaming only
const queue = ["d", "s", "s", "d", "s"];
const downloaded = (item: string) => item === "d";

check(findPlayableIndex(queue, 0, 1, downloaded) === 3, "next skips over streaming-only songs");
check(findPlayableIndex(queue, 3, -1, downloaded) === 0, "previous skips back the same way");
check(findPlayableIndex(queue, 3, 1, downloaded) === -1, "nothing playable ahead is -1, not a wrap");
check(findPlayableIndex(queue, 0, -1, downloaded) === -1, "nothing before the start is -1");
check(findPlayableIndex([], 0, 1, downloaded) === -1, "an empty queue has nothing to play");

console.log("playableIndex.check passed");
