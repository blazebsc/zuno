# Native GUI bake-off

Replacing Zuno's Tauri/WebView frontend with a native Rust GUI — eight
candidate frameworks, one shared application core, one identical benchmark
app, real measurements only.

- **Why**: WebKitWebProcess alone costs ~0.8–1 GB of RAM on a normal session.
  The goal is a browser-free app that looks at least as good as the React
  frontend.
- **The spec**: [SPEC.md](./SPEC.md) — architecture, the benchmark app, the
  design language, the measurement protocol, and the no-WebView verification
  every branch must show.
- **Baseline branch**: `gui/base` — the framework-free `zuno-core` crate all
  eight branches build on.
- **One branch per framework**: `gui/gpui`, `gui/vizia`, `gui/iced`,
  `gui/slint`, `gui/makepad`, `gui/freya`, `gui/xilem`, `gui/floem`. Each is
  independently buildable: `cargo run -p zuno-ui-<fw> --release` (interactive)
  or `cargo run -p zuno-ui-<fw> --release -- --bench` (measured run).

## Reports

| Framework | Report | Branch |
|---|---|---|
| GPUI | [gpui.md](./gpui.md) | `gui/gpui` |
| Vizia | [vizia.md](./vizia.md) | `gui/vizia` |
| Iced | [iced.md](./iced.md) | `gui/iced` |
| Slint | [slint.md](./slint.md) | `gui/slint` |
| Makepad | [makepad.md](./makepad.md) | `gui/makepad` |
| Freya | [freya.md](./freya.md) | `gui/freya` |
| Xilem | [xilem.md](./xilem.md) | `gui/xilem` |
| Floem | [floem.md](./floem.md) | `gui/floem` |

## Final comparison

Filled in after all eight branches have real `--bench` runs. No invented
numbers — see each report for the actual measurement conditions.

| Framework | Idle RAM | 5k-track RAM | Playback RAM | Peak (VmHWM) | WebView? | Visual quality | Notes |
| --------- | -------: | -----------: | -----------: | -----------: | -------- | -------------- | ----- |
| GPUI      |          |              |              |              |          |                |       |
| Vizia     |          |              |              |              |          |                |       |
| Iced      |          |              |              |              |          |                |       |
| Slint     |          |              |              |              |          |                |       |
| Makepad   |          |              |              |              |          |                |       |
| Freya     |          |              |              |              |          |                |       |
| Xilem     |          |              |              |              |          |                |       |
| Floem     |          |              |              |              |          |                |       |
