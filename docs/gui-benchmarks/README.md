# Native GUI bake-off

Replacing Zuno's Tauri/WebView frontend with a native Rust GUI — seven
candidate frameworks (makepad was cut during bring-up: its list draw path
leaked unbounded, 2.9 GB at 3 s → 13.3 GB at 12 s), one shared application
core, one identical benchmark app, real measurements only.

- **Why**: WebKitWebProcess alone costs ~0.8–1 GB of RAM on a normal session.
  The goal is a browser-free app that looks at least as good as the React
  frontend.
- **The spec**: [SPEC.md](./SPEC.md) — architecture, the benchmark app, the
  design language, the measurement protocol, and the no-WebView verification
  every branch must show.
- **Baseline branch**: `gui/base` — the framework-free `zuno-core` crate all
  branches build on, plus the shared `zuno-yt` crate (real YouTube:
  unsigned search/browse/stream resolution via direct iOS/Android/TV URLs +
  QuickJS-deciphered WEB fallbacks, cookie session, mutations).
- **One branch per framework**: `gui/gpui`, `gui/vizia`, `gui/iced`,
  `gui/slint`, `gui/freya`, `gui/xilem`, `gui/floem` — each independently
  buildable (`cargo run -p zuno-ui-<fw> --release`, or `-- --bench` for the
  measured run), each wired to real YouTube Music data with the synthetic
  bench preserved for comparability.

## Reports

| Framework | Report | Branch |
|---|---|---|
| GPUI | [gpui.md](./gpui.md) | `gui/gpui` |
| Vizia | [vizia.md](./vizia.md) | `gui/vizia` |
| Iced | [iced.md](./iced.md) | `gui/iced` |
| Slint | [slint.md](./slint.md) | `gui/slint` |
| Freya | [freya.md](./freya.md) | `gui/freya` |
| Xilem | [xilem.md](./xilem.md) | `gui/xilem` |
| Floem | [floem.md](./floem.md) | `gui/floem` |

The shared YouTube work: [yt.md](./yt.md).

## Final comparison

Pre-wiring synthetic-bench numbers (identical `--bench` protocol on all
branches; each branch's report re-verifies post-wiring comparability —
the linked-`zuno-yt` delta is 1–14 MB of file-backed code, documented
per branch). Every number below was actually observed on the benchmark
machine (Wayland + niri, NVIDIA RTX 4070, release builds):

| Framework | Idle RAM | 5k-track RAM | Playback RAM | Peak (VmHWM) | WebView? | Visual quality | Notes |
| --------- | -------: | -----------: | -----------: | -----------: | -------- | -------------- | ----- |
| GPUI      | 258 MB | 258 MB | 259 MB | 258 MB | None — blade/Vulkan, 134 MiB VRAM | High — Zed-grade polish, frameless, custom titlebar | `uniform_list` virtualizes; scroll costs ~nothing; 42% CPU during scroll sweep; 40 threads |
| Vizia     | 150 MB | 148 MB | 148 MB | 150 MB | None — Skia/OpenGL, 117 MiB VRAM | High — CSS-like styling, full benchmark surface | VirtualList recycles rows: 5k scroll is RSS-flat; 19–20% CPU; 9 threads |
| Iced      | 204 MB | 489 MB | 489 MB | 489–576 MB | None — wgpu/Vulkan, 52 MiB VRAM | High — frameless rewrite, custom titlebar, selection model | No virtualization: +285 MB for the 5k tree; 83–86% CPU scrolling; one-run playback spike (575.7 MB) recorded honestly |
| Slint     | 122–153 MB | 153 MB | 153 MB | 153–158 MB | None — femtovg/OpenGL, 75 MiB VRAM | High — declarative `.slint` UI, full surface | `ListView` virtualizes: +4 MB for 5k; lowest CPU (7–12%); 9 threads |
| Freya     | 163 MB | 202–207 MB | 207 MB | 207 MB | None — Skia GL, 39–57 MiB VRAM | Good — Dioxus RSX; frameless + custom titlebar | Lazy row materialization tracks frame rate; niri occlusion throttling affects idle CPU; 19 threads |
| Xilem     | 450 MB | 1052 MB | 1055 MB | 1119–1123 MB | None — vello/Vulkan, 473 MiB VRAM | Moderate — early-stage widget set; chunky scrollbars, no slider focus polish | No virtualization (2× iced per row); honest upper bound of the Elm-style diffing approach |
| Floem     | 564 MB | 485 MB | 487 MB | 562 MB | None — vger/wgpu, 42 MiB VRAM | Good — reactive signals, native decorations | Virtualizes the 5k list, but idle > scrolled (home-view teardown anomaly, documented); 26–28% CPU; 39 threads |

### Reading the table

- **The virtualization split is the whole memory story.** Slint, Vizia and
  GPUI recycle list rows — the 5,000-track library costs +4 MB, nothing,
  and ~nothing respectively. Iced and Xilem have no virtualization and pay
  +285 MB / +600 MB for resident widget trees. Floem and Freya sit between.
- **Every candidate beats WebKit.** Even the worst case (Xilem, 1.1 GB with
  the full 5k tree) is comparable to the old WebView alone — and the
  virtualized candidates run the whole app at 150–260 MB, ~3–5× less.
- **Zero browser code anywhere**: every branch is a single process,
  GPU-rendered (verified via nvidia-smi + ldd), with the audio engine
  (rodio→cpal→ALSA) shared from the shipping app.
- **Real YouTube**: all seven branches load real shelves/search/artwork and
  resolve real streams through the shared `zuno-yt` crate. Sustained
  full-track playback is gated by googlevideo's per-IP byte budget for
  PO-token-less requests — measured and documented in [yt.md](./yt.md) —
  which the WebView app solved with the browser's BotGuard; the native
  app deliberately does not embed one, and the branches fall back to synth
  past the gate.
