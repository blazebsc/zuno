# Native GUI bake-off — shared spec

Zuno's WebKitGTK frontend costs roughly **0.8–1 GB of RAM** in the web-view
processes. This experiment replaces the entire WebView with a native Rust GUI,
and answers one question for eight candidate frameworks:

> **How good can Zuno look in this framework, and how little RAM does it take?**

One shared application core (`native/core`), eight UIs (`native/ui-<fw>`, one
per `gui/<fw>` branch), one identical benchmark app rendered by each. Nothing
here decides the winner — every branch ships with real measurements and the
user picks.

---

## 1. Ground rules

1. **No browser engine. No WebView. No HTML/CSS renderer. No iframe.** The UI
   must be drawn by the framework's own native renderer. A branch that hides a
   WebView is disqualified, not shipped. Verification in §6.
2. **Real measurements only.** Every number in a report comes from an
   `--bench` run that actually happened on the benchmark machine. No invented
   numbers, no estimates dressed as data.
3. **The same app eight times.** All implementations render the shared
   `AppState` and the shared design tokens (`core::theme`). Visual divergence
   is a bug in the implementation, not an authorial choice.
4. **One branch per framework, no cross-contamination.** Each `gui/<fw>` branch
   contains `native/core` (identical, from `gui/base`) plus exactly one UI
   crate. GUI branches are never merged into each other.
5. **Keep what works.** The existing Tauri app (`src-tauri/`, `src/`) stays
   untouched on every branch — it is the reference implementation and the
   design source of truth, and it keeps building.

## 2. Architecture

```text
zuno/
├── src-tauri/                  the shipping Tauri app (untouched, reference)
├── src/                        the shipping React frontend (untouched, design reference)
└── native/
    ├── core/                   zuno-core — framework-free application core
    │   ├── model               Track / Album / Artist / Playlist / TrackView
    │   ├── library             synthetic 5,000-track library (deterministic)
    │   ├── artwork             procedural covers + shared LRU cache (96 covers)
    │   ├── queue               3-region queue — port of src/player/Queue.ts
    │   ├── player              rodio/cpal engine + in-process synth
    │   ├── app                 AppState — the app logic every UI drives
    │   ├── search              match scoring + library filtering
    │   ├── theme               the React app's design tokens as constants
    │   ├── format              "3:07", "24 songs · 1 hr 32 min"
    │   └── bench               the identical measurement harness
    └── ui-<fw>/                 one crate per gui/<fw> branch
```

### What is kept, replaced, isolated

| Existing piece | Fate in the native rewrite | In the benchmark |
|---|---|---|
| `audio.rs` / `opus_source.rs` / `equalizer.rs` (symphonia+libopus+rodio+cpal, the `rust` engine) | **Kept** — already WebView-free, already the default engine | Same rodio/cpal output stack, in-process synth instead of downloaded Opus (§5) |
| `lib.rs` cache, settings, keyring, cookie jar, HTTP proxy, offline store, folder watcher | **Kept** — framework-independent services; move beside `core` as crates | Out of benchmark scope |
| YouTube IFrame decks, media server, localhost frontend hosting, sign-in window | **Gone** with the WebView (they exist only to serve the renderer) | Gone; no branch may link them |
| Innertube/PO-token stream resolution (TypeScript `YouTubeMusicDataSource`) | **Isolated** — must be ported to Rust (or kept as a sidecar service) for production; documented, not silently resurrected | Out of scope; the synth stands in (§5) |
| React frontend (`src/`) | Replaced by the winning UI | Design reference only |

**The playback path is `GUI → AppState → PlayerHandle → rodio → cpal`.**
There is no `GUI → iframe → youtube.com` anywhere in a native branch, and no
branch may add one.

## 3. The benchmark application

Every branch renders this app. "Must" items are pass/fail for the branch;
"should" items degrade gracefully but must be noted in the report.

### 3.1 Window

- 1280×800 default, 900×600 minimum, resizable, dark theme.
- Custom chrome is a "should" (frameworks with clean frameless support render
  Zuno's 14px-radius shell; others use native decorations with the same
  content inside).

### 3.2 Layout

```text
┌───────────────────────────────────────────────────────────────┐
│ sidebar (220px) │  content column                             │
│                │  ┌────────────────────────────────────────┐  │
│  Zuno logo     │  │ page content (scrolls)                │  │
│  ─ Home        │  │                                      │  │
│  ─ Search      │  │                                      │  │
│  ─ Library     │  └────────────────────────────────────────┘  │
│  ─ Settings    │  ┌────────────────┐ ┌─────────────────────┐  │
│  PLAYLISTS     │  │ queue panel     │ │  (toggleable, 300px)│  │
│  ♪ Liked Songs │  └────────────────┘ └─────────────────────┘  │
│  … playlist rows│                                              │
├──────────────────────────────────────────────────────────────┤
│ player bar (76px): art | title/artist | ◀ ▶ ▶▶ | seekbar | vol │
└───────────────────────────────────────────────────────────────┘
```

### 3.3 Views (each is a distinct screen)

1. **Home** — h1 "Home"; "Recently played" shelf (12 album cards);
   "Made for you" shelf (6 mix cards); "New albums" responsive grid
   (24 cards); "Popular artists" shelf (12 round artist cards).
2. **Library** — h1 "Your Library"; pill tabs Playlists / Albums / Artists /
   Songs. **Songs renders the full 5,000-track list** — the large-list test.
   Albums tab: grid of all ~600 albums. Playlists tab: all 45 rows. Artists
   tab: all ~180 rows.
3. **Album view** — 232px artwork, h1 title, "Album · Artist · 2021",
   `counts_line` meta, Play + Shuffle buttons, the album's track list.
4. **Playlist view** — same shape for playlists (incl. Liked Songs).
5. **Artist view** — round 232px artwork, h1 name, listeners line, Play,
   "Top tracks" (first 10), "Albums" grid.
6. **Search** — search field in the content header; typing filters the 5,000
   tracks live (debounce optional); results: "Top result" card + tracks list
   + album/artist shelves. `Ctrl/⌘+K` and `/` focus it.
7. **Settings** — sidebar-reachable page: theme (dark pinned), audio-quality
   rows, a few toggles. Rendered, not wired.
8. **Queue panel** — right side, 300px, toggleable; "Now playing" + "Next in
   queue" (manual) + "Next up" (automatic) sections, region-aware styling,
   click-to-jump, remove buttons. "Should": drag reorder within a region.

### 3.4 The track row (used by every list)

`52px` tall: `24px` index column (tabular numerals) → `40px` artwork → title
(14px medium, single line) + artist (12px, muted-foreground) → duration
(12px tabular). States: hover `bg-card`; playing `primary/5` + red
playing-bars glyph; selected `primary/10`; explicit tag "E" badge; liked rows
show a red heart in place of the duration. Click plays from the list; the row
that is playing shows three animated bars instead of its number.

### 3.5 The player bar

`76px`, background at the bottom: 40px artwork (click → album) · title +
artist (click → artist) · shuffle / previous / **play-pause** / next / repeat,
play-pause is a 40px primary red pill · seekbar: `m:ss` / track /
`m:ss`, the elapsed side colored `primary`, drag to seek · like button ·
queue toggle · volume icon + slider, mute toggle.

### 3.6 Interaction requirements

- Hover states on every interactive thing (nav items, cards, rows, buttons).
- Selected state on the current sidebar item and the playing row.
- Animations where practical: hover colour fades (120 ms), card play-scrim
  fade-in (150 ms), playing-bars indicator, page transitions (120 ms fade
  where the framework makes it cheap). Do not fake smoothness in the report.
- Responsive: sidebar collapses at < 1000px width; grids reflow; queue panel
  overlays rather than squeezes at narrow widths.
- Keyboard, where practical: `Space` play/pause · `←/→` seek ∓10 s ·
  `↑/↓` move row selection (list views) · `Enter` play selected row ·
  `PgUp/PgDn` scroll · `Ctrl/⌘+K`, `/` focus search · `Esc` back/clear ·
  `M` mute · `Q` toggle queue. Record what shipped in the report.

## 4. Design language (exact values)

Source: `src/ui/styles/global.css`, `AlbumCard`, `TrackRow`, `Sidebar`,
`SeekBar`, `PlayerBar`. All constants live in `core::theme` — use them, don't
re-derive.

- **Palette (dark)**: background `#0a0a0a`, foreground `#fafafa`, card
  `#2a2a2a`, popover `#171717`, muted `#262626`, muted-foreground `#a1a1a1`,
  **primary `#ff0033`** (accent only — never a large surface).
- Borderless: surfaces separate by contrast, not outlines.
- **Typography**: Inter (variable, `cv11`/`ss01` where available). h1 32/700
  -0.03em · h2 24/700 -0.02em · h3 18/600 · body 14 · secondary 12 · tabular
  numerals for every duration/count.
- **Radii**: rows + nav items 8px · artwork tiles 4px · artist art full ·
  buttons/inputs full (pill).
- **Layout**: titlebar 44 · sidebar 220 (62 collapsed / 240 expanded) ·
  row 52 · album card 176 art + 2 label lines · queue 300 · page padding 20 ·
  section gap 24.
- Album/playlist/mix cards: **square artwork** (Zuno uses `rounded-none`),
  title 14/medium 2-line clamp, subtitle 12 muted 1-line; hover: card surface
  bg + 50%-opacity background scrim with a 48px round `primary` play pill.
- Artwork comes from `state.artwork.get(seed, size)` — RGBA, premultiplied by
  nothing, rows use 40px, cards use 176px, headers 232px. Do not build a
  per-framework image store larger than the shared cache.

## 5. Audio

**Benchmark**: `PlayerHandle` plays an in-process synthesized track (seeded
chord progression through the real rodio → cpal → ALSA output stack — the same
stack `src-tauri/src/audio.rs` ships). This exercises a live audio thread and a
live device stream without the network.

**Production note (documented, not solved here)**: real YouTube stream
resolution — Innertube negotiation, deciphering, PO tokens — currently lives
in TypeScript (`src/datasource/youtube/`). It is **WebView-independent**
(it already funnels through Rust's `proxy_http_request`), so the native
rewrite ports it to Rust beside `core::player`; nothing about it requires a
browser. The `rust` engine's decode path (`audio.rs`, `opus_source.rs`) is
kept as-is. **No branch may link the IFrame path back in.**

## 6. No-WebView verification (per branch)

Record the actual command output in the report:

```bash
ldd target/release/zuno-<fw> | grep -iE 'webkit|gtk|webkit2gtk|cef|chromium|electron'   # expect: no matches
ps -ef | grep -iE 'webkit|WebKitWebProcess' | grep -v grep                              # while running: none
```

Also state the renderer the framework actually used (wgpu/Vulkan, GL, Skia,
software) and back it with `nvidia-smi` output captured during the run if a
GPU context is expected.

## 7. The `--bench` protocol

Run: `./target/release/zuno-<fw> --bench` on the benchmark machine, real
display (`:0`, NVIDIA RTX 4070), release profile. 48 s, driven by
`core::bench::BenchDriver` — every framework implements the same loop:

```rust
match bench.tick() {
    BenchAction::RecordStartup => {}                       // first frame + 0.5 s
    BenchAction::ScrollTo(f)  => scroll_library_to(f),     // 8–28 s, every frame
    BenchAction::SelectRow(i) => select_visible_row(i),    // 28–33 s, 2 Hz
    BenchAction::StartPlayback => play_first_result(),     // at 33 s
    BenchAction::Finish      => { print(bench.report); exit(0) }
    BenchAction::Nothing     => {}
}
state.tick();                                             // every frame
```

The app must scroll the **Library ▸ Songs 5,000-row list** during the scroll
phase (jump straight to the offset; no easing). Record: the driver's
auto-generated report table (startup/idle/scroll/select/playback RSS
mean/min/max, VmHWM peak, CPU seconds, threads), plus:

- process count (the frameworks should be single-process — verify, note),
- `nvidia-smi` capture (GPU in use? video memory?),
- the §6 ldd output,
- build time and binary size (nice-to-have context),
- notes: what virtualization/recycling the list uses, dropped frames or
  visible stutter while scrolling (say what you observed), what felt slow.

Idle-without-bench numbers (app opened normally, left for 60 s) are also
worth recording via `ps` — but label them separately.

## 8. Report template

Each branch writes `docs/gui-benchmarks/<fw>.md`:

```markdown
# <Framework> — benchmark report
- branch / commit:
- framework + version (crates.io or git rev):
- renderer actually used:
- build: (command, profile, time, warnings of note)

## Result table (auto-generated block from --bench)
(paste verbatim)

## Process / GPU / no-webview verification
(paste actual outputs)

## Implementation notes
- list virtualization approach:
- what was animated vs static:
- keyboard interactions shipped:
- visual divergences from the spec (honesty here is the point):
- anything the framework made hard/easy that the next evaluator should know:

## Honest issues observed
```

## 9. Run matrix

| Branch | Crate | Status |
|---|---|---|
| `gui/gpui` | `native/ui-gpui` | Zed's GPUI |
| `gui/vizia` | `native/ui-vizia` | Vizia |
| `gui/iced` | `native/ui-iced` | Iced |
| `gui/slint` | `native/ui-slint` | Slint |
| `gui/makepad` | `native/ui-makepad` | Makepad |
| `gui/freya` | `native/ui-freya` | Freya (Dioxus + Skia) |
| `gui/xilem` | `native/ui-xilem` | Xilem (Masonry backend) |
| `gui/floem` | `native/ui-floem` | Floem |

The final comparison table lands in `docs/gui-benchmarks/README.md` once all
eight have real runs.
