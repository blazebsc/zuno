/**
 * Self-check for artwork selection. No test runner in this project, so:
 *
 *   npx esbuild src/datasource/youtube/artwork.check.ts --bundle --platform=node --format=esm
 *     --outfile=check.mjs && node check.mjs
 *
 * Selection is "biggest wins", which only stays right as long as the caller hands it one kind
 * of image at a time — an artist's portrait and their page banner are picked between by field,
 * not by size, precisely because the banner would win here every time.
 */
export {};

import {
  artworkIdentity,
  getArtworkSizeBucket,
  getArtworkUrlCandidates,
  selectArtworkUrl,
} from "./artwork";

function check(condition: boolean, message: string): void {
  if (!condition) throw new Error(`FAILED: ${message}`);
}

const large = { url: "https://example.com/large", width: 1000, height: 1000 };
const small = { url: "https://example.com/small", width: 120, height: 120 };
const sizeless = { url: "https://example.com/sizeless" };

check(selectArtworkUrl([small, large]) === large.url, "the largest image wins");
check(selectArtworkUrl([large, sizeless]) === large.url, "a sized image beats an unsized one");
check(selectArtworkUrl([sizeless]) === sizeless.url, "an unsized image is still an image");
check(selectArtworkUrl([]) === undefined, "nothing in, nothing out");
check(
  selectArtworkUrl(null, undefined, [small]) === small.url,
  "missing groups are skipped rather than throwing",
);
check(
  selectArtworkUrl([{ url: "  " }, small]) === small.url,
  "a blank url is not a candidate",
);
check(
  selectArtworkUrl([{ url: "//example.com/img", width: 10, height: 10 }])
    === "https://example.com/img",
  "protocol-relative urls are normalised",
);

// Size variants: with no size asked for, the original is tried first, so a rewrite can never
// lose the image.
const original = "https://yt3.googleusercontent.com/abc=w2565-h1068-l90-rj";
const variants = getArtworkUrlCandidates(original);
check(variants[0] === original, "without a size, the original url is the first candidate");
check(variants.length > 1, "youtube urls get smaller variants to fall back on");
check(
  getArtworkUrlCandidates("https://example.com/img.jpg").length === 1,
  "urls on other hosts have no size syntax to rewrite",
);
check(getArtworkUrlCandidates("").length === 0, "no url, no candidates");

/*
 * The point of the whole exercise: a small slot must not be handed the full-size image. This is
 * the check that fails if the size ever stops reaching the URL, which is invisible on screen —
 * the picture still appears, it just costs twenty times the memory.
 */
const sized = getArtworkUrlCandidates(original, 120);
check(
  sized[0] === "https://yt3.googleusercontent.com/abc=w120-h120-l90-rj",
  "a requested size is the first candidate, ahead of the original",
);
check(sized.includes(original), "the original stays in the ladder as a fallback");
check(
  getArtworkUrlCandidates("https://example.com/img.jpg", 120)[0] === "https://example.com/img.jpg",
  "a size on a host with no rewrite syntax falls back to the original rather than dropping it",
);

// Buckets, so the same cover in two components shares one cache entry and one download.
(globalThis as Record<string, unknown>).devicePixelRatio = 1;
check(getArtworkSizeBucket(40) === 120, "a 40px slot asks for the smallest bucket");
check(getArtworkSizeBucket(200) === 240, "a bucket must cover the slot, not merely be near it");
check(getArtworkSizeBucket(900) === null, "a slot larger than every bucket keeps the original");

(globalThis as Record<string, unknown>).devicePixelRatio = 2;
check(getArtworkSizeBucket(40) === 120, "a retina 40px slot needs 80px, so still 120");
check(
  getArtworkSizeBucket(260) === 544,
  "density is applied before bucketing, or hi-dpi art renders soft",
);

/*
 * The card sizes, at the density most people have. These are the two that decide how much
 * bitmap a grid holds — before the 400 bucket existed both landed on 544, which is 2.4x the
 * pixels either of them can display.
 */
check(getArtworkSizeBucket(176) === 400, "a 176px card at 2x needs 352px, so 400 not 544");
check(getArtworkSizeBucket(200) === 400, "a 200px card at 2x needs 400px, so 400 exactly");
check(
  getArtworkSizeBucket(201) === 544,
  "one pixel past the 400 bucket steps up rather than rendering soft",
);
check(
  getArtworkSizeBucket(280) === null,
  "a 2x slot needing more than 544 still keeps the original",
);

// A saved download cover has to match the same song at every size it is shown at.
check(
  artworkIdentity("https://lh3.googleusercontent.com/abc=w120-h120-l90-rj")
    === artworkIdentity("//lh3.googleusercontent.com/abc=w544-h544-l90-rj"),
  "size variants of one cover share an identity",
);
check(
  artworkIdentity("https://i.ytimg.com/vi/dQw4w9WgXcQ/hqdefault.jpg")
    === artworkIdentity("https://i9.ytimg.com/vi_webp/dQw4w9WgXcQ/mqdefault.webp"),
  "every thumbnail of one video shares an identity",
);
check(
  artworkIdentity("https://lh3.googleusercontent.com/abc=w120")
    !== artworkIdentity("https://lh3.googleusercontent.com/xyz=w120"),
  "different covers stay different",
);

console.log("artwork.check.ts OK");
