export {};

import { formatCount } from "./github";
import { timeAgo } from "./releases";

function check(condition: boolean, message: string): void {
  if (!condition) throw new Error(`FAILED: ${message}`);
}

check(formatCount(633) === "633" && formatCount(1000) === "1k", "small counts and round thousands");
check(formatCount(1234) === "1.2k" && formatCount(12_500) === "13k", "one decimal below ten thousand");

const now = Date.parse("2026-10-07T12:00:00Z");
check(timeAgo("2026-10-07T09:00:00Z", now) === "today", "same day");
check(timeAgo("2026-10-04T12:00:00Z", now) === "3 days ago", "days");
check(timeAgo("2026-09-23T12:00:00Z", now) === "2 weeks ago", "weeks");
check(timeAgo(null, now) === "recently" && timeAgo("not a date", now) === "recently", "no date");

console.log("format: ok");
