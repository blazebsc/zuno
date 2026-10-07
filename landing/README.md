# Zuno — marketing site

The public landing page. A separate Vite project that shares Zuno's look but none of its build.

```bash
cd landing
npm install
npm run dev      # http://localhost:5173
npm run check    # the *.check.ts self-checks
npm run build    # -> landing/dist
```

It is its own project so the website can ship on its own schedule, and so no stray import can
pull `@tauri-apps/*` into a bundle with no Tauri runtime under it.

## The demo player

The page plays one track (`public/low-tide.mp3`), and only when someone presses play.
`src/audio/demoPlayer.ts` owns it: one `<audio>` element, a Web Audio graph built on the first
press, and a store the components read with `useSyncExternalStore`.

- **EQ** — ten peaking filters with the app's bands, Q and presets (`src/audio/eq.ts` mirrors
  `src/ui/settings/equalizerCurve.ts`). If the app's EQ changes, change both.
- **Visuals** — an analyser feeds the spectrum ring and writes `--bass`, `--level` and
  `--progress` on `<html>` every frame; CSS does the rest. Nothing animates under reduced motion.
- **Mini player** — appears once the hero scrolls away (`data-hero-out`), hides over the footer.

## Shared with the app

- **Colour tokens** — `src/styles.css` copies the app's `@theme` block byte for byte, plus a
  few site-only brand tokens (the mascot's colours, the record). If the brand shifts, change both.
- **Fonts** — Inter (as in the app) and Geist Mono, self-hosted through Fontsource.
- **Icons** — OS and service marks are extracted from `@iconify-json/logos` into
  `src/components/brandIcons.tsx`, so nothing is fetched at runtime.
- **Images** — `ghost.webp` and `theme-{dark,light}.webp` are resized from `logo.png` and the
  1.2 screenshots; regenerate them if those change.

## Downloads

`src/releases.ts` reads the newest release from the GitHub API and points each tile at the
matching installer. Every button falls back to the releases page if the request fails or an
asset is missing. Repo stats in `src/github.ts` work the same way: no data, no numbers.

## Deploying

`base` is `"./"`, so the build works from a subpath — including GitHub Pages project sites.
Publish `landing/dist` as-is.
