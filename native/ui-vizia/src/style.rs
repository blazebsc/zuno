//! The Zuno design language as one vizia stylesheet. Loaded AFTER the
//! framework default theme, so equal-specificity rules here win, and the
//! token values come from `zuno_core::theme` (kept in sync by hand — the
//! values are frozen design tokens, not derived state).

pub const CSS: &str = r#"
/* ————— Zuno dark tokens ————— */
:root {
    background-color: #0a0a0a;
    color: #fafafa;
    fill: #fafafa;
    font-family: "Inter", "Segoe UI", sans-serif;
    font-size: 14px;
    font-variation-settings: "wght" 400;
    selection-color: #2a2a2a;
    caret-color: #fafafa;
}

window {
    background-color: #0a0a0a;
}

/* ————— shell ————— */

.app-root {
    width: 1s;
    height: 1s;
}

.content-col {
    width: 1s;
    height: 1s;
    background-color: #0a0a0a;
    overflow: hidden;
}

/* ————— sidebar ————— */

.sidebar {
    width: 220px;
    height: 1s;
    background-color: #0a0a0a;
    padding-top: 18px;
    padding-left: 12px;
    padding-right: 12px;
    padding-bottom: 10px;
}

.logo {
    font-size: 21px;
    font-variation-settings: "wght" 700;
    color: #fafafa;
    padding-left: 12px;
    padding-bottom: 20px;
}

.nav-item {
    width: 1s;
    height: 38px;
    padding-left: 12px;
    corner-radius: 8px;
    background-color: transparent;
    color: #a1a1a1;
    fill: #a1a1a1;
    horizontal-gap: 12px;
    transition: background-color 120ms;
    cursor: pointer;
}

.nav-item:hover {
    background-color: #2a2a2a;
}

.nav-item.active {
    background-color: #2a2a2a;
    color: #fafafa;
    fill: #fafafa;
}

.section-label {
    font-size: 11px;
    font-variation-settings: "wght" 600;
    color: #a1a1a1;
    padding-left: 12px;
    padding-top: 16px;
    padding-bottom: 6px;
}

.playlist-scroll {
    width: 1s;
    height: 1s;
}

.playlist-row {
    width: 1s;
    min-height: 48px;
    padding-top: 4px;
    padding-bottom: 4px;
    padding-left: 8px;
    padding-right: 8px;
    corner-radius: 8px;
    background-color: transparent;
    horizontal-gap: 10px;
    alignment: left;
    transition: background-color 120ms;
    cursor: pointer;
}

.playlist-row:hover {
    background-color: #2a2a2a;
}

.playlist-row.active {
    background-color: #2a2a2a;
}

/* ————— typography ————— */

.h1 {
    font-size: 32px;
    font-variation-settings: "wght" 700;
    color: #fafafa;
}

.h2 {
    font-size: 24px;
    font-variation-settings: "wght" 700;
    color: #fafafa;
}

.h3 {
    font-size: 18px;
    font-variation-settings: "wght" 600;
    color: #fafafa;
}

.subtle {
    font-size: 12px;
    color: #a1a1a1;
}

.muted {
    color: #a1a1a1;
}

/* ————— pages ————— */

.page {
    width: 1s;
    height: 1s;
    overflow: hidden;
}

.page-scroll {
    width: 1s;
    height: 1s;
}

.page-body {
    width: 1s;
    padding-top: 24px;
    padding-left: 20px;
    padding-right: 20px;
    padding-bottom: 20px;
    vertical-gap: 24px;
}

.page-header {
    height: 44px;
    horizontal-gap: 12px;
    alignment: left;
}

.icon-btn {
    width: 34px;
    height: 34px;
    corner-radius: 999px;
    background-color: transparent;
    fill: #a1a1a1;
    alignment: center;
    transition: background-color 120ms;
    cursor: pointer;
}

.icon-btn:hover {
    background-color: #2a2a2a;
    fill: #fafafa;
}

.icon-btn.on {
    fill: #ff0033;
}

.back-btn {
    width: 34px;
    height: 34px;
    corner-radius: 999px;
    background-color: transparent;
    fill: #a1a1a1;
    alignment: center;
    cursor: pointer;
}

.back-btn:hover {
    background-color: #2a2a2a;
    fill: #fafafa;
}

/* ————— shelves + cards ————— */

.shelf {
    width: 1s;
    vertical-gap: 14px;
}

.shelf-row {
    width: 1s;
    height: 232px;
    horizontal-gap: 16px;
}

.card {
    width: 184px;
    height: 232px;
    padding: 4px;
    corner-radius: 8px;
    vertical-gap: 8px;
    background-color: transparent;
    transition: background-color 120ms;
    cursor: pointer;
}

.card:hover {
    background-color: #1c1c1c;
}

.card-art {
    width: 176px;
    height: 176px;
}

.card-title {
    font-size: 14px;
    font-variation-settings: "wght" 500;
    color: #fafafa;
    width: 176px;
    line-clamp: 1;
    text-wrap: false;
}

.card-sub {
    font-size: 12px;
    color: #a1a1a1;
    width: 176px;
    line-clamp: 1;
    text-wrap: false;
}

.scrim {
    position-type: absolute;
    top: 0px;
    left: 0px;
    width: 176px;
    height: 176px;
    corner-radius: 4px;
    background-color: rgba(10, 10, 10, 0.55);
    opacity: 0;
    alignment: center;
    transition: opacity 150ms;
}

.card:hover .scrim {
    opacity: 1;
}

.play-pill {
    width: 44px;
    height: 44px;
    corner-radius: 999px;
    background-color: #ff0033;
    fill: #ffffff;
    color: #ffffff;
    alignment: center;
    shadow: 0px 6px 16px rgba(0, 0, 0, 0.55);
    cursor: pointer;
}

.round-art {
    corner-radius: 999px;
}

/* ————— library tabs ————— */

.tab-bar {
    height: 36px;
    horizontal-gap: 8px;
}

.tab-pill {
    height: 32px;
    padding-left: 16px;
    padding-right: 16px;
    corner-radius: 999px;
    background-color: #2a2a2a;
    color: #fafafa;
    font-size: 13px;
    font-variation-settings: "wght" 500;
    alignment: center;
    transition: background-color 120ms;
    cursor: pointer;
}

.tab-pill:hover {
    background-color: #3a3a3a;
}

.tab-pill.active {
    background-color: #fafafa;
    color: #0a0a0a;
}

/* ————— track rows ————— */

.track-list {
    width: 1s;
    vertical-gap: 2px;
}

.track-row {
    width: 1s;
    height: 52px;
    corner-radius: 8px;
    background-color: transparent;
    horizontal-gap: 12px;
    padding-left: 6px;
    padding-right: 10px;
    alignment: left;
    transition: background-color 120ms;
    cursor: pointer;
}

.track-row:hover {
    background-color: #2a2a2a;
}

.track-row.playing {
    background-color: #160a0c;
}

.track-row.selected {
    background-color: #23090e;
}

.row-index {
    width: 26px;
    height: 1s;
    font-size: 12px;
    color: #a1a1a1;
    text-align: right;
    alignment: right;
}

.row-art {
    width: 40px;
    height: 40px;
    corner-radius: 4px;
}

.row-text {
    width: 1s;
    vertical-gap: 2px;
}

.row-title {
    font-size: 14px;
    font-variation-settings: "wght" 500;
    color: #fafafa;
    line-clamp: 1;
    text-wrap: false;
    text-overflow: ellipsis;
}

.row-title-row {
    horizontal-gap: 6px;
    alignment: left;
}

.row-artist {
    font-size: 12px;
    color: #a1a1a1;
    line-clamp: 1;
    text-wrap: false;
}

.badge-e {
    width: 14px;
    height: 14px;
    corner-radius: 3px;
    background-color: #4a4a4a;
    color: #d4d4d4;
    font-size: 9px;
    font-variation-settings: "wght" 700;
    alignment: center;
}

.row-right {
    width: 36px;
    height: 1s;
    font-size: 12px;
    color: #a1a1a1;
    alignment: right;
    text-align: right;
}

.row-heart {
    width: 18px;
    height: 18px;
    fill: #ff0033;
}

.row-play-icon {
    width: 16px;
    height: 16px;
    fill: #ff0033;
}

/* ————— artist / playlist list rows ————— */

.list-row {
    width: 1s;
    min-height: 56px;
    corner-radius: 8px;
    background-color: transparent;
    horizontal-gap: 12px;
    padding-top: 6px;
    padding-bottom: 6px;
    padding-left: 8px;
    padding-right: 8px;
    alignment: left;
    transition: background-color 120ms;
    cursor: pointer;
}

.list-row:hover {
    background-color: #2a2a2a;
}

.list-row-art {
    width: 44px;
    height: 44px;
    corner-radius: 4px;
}

/* ————— hero (album/playlist/artist headers) ————— */

.hero {
    height: 252px;
    horizontal-gap: 24px;
    padding-top: 20px;
}

.hero-art {
    width: 232px;
    height: 232px;
    corner-radius: 4px;
}

.hero-col {
    width: 1s;
    height: 232px;
    vertical-gap: 10px;
    alignment: bottom-left;
}

.hero-meta {
    font-size: 12px;
    color: #a1a1a1;
}

.pill-bar {
    horizontal-gap: 10px;
}

.pill-primary {
    height: 40px;
    padding-left: 22px;
    padding-right: 22px;
    corner-radius: 999px;
    background-color: #ff0033;
    color: #ffffff;
    fill: #ffffff;
    font-size: 14px;
    font-variation-settings: "wght" 600;
    horizontal-gap: 8px;
    alignment: center;
    cursor: pointer;
    transition: transform 120ms;
}

.pill-primary:hover {
    transform: scale(1.04);
}

.pill-ghost {
    height: 40px;
    padding-left: 22px;
    padding-right: 22px;
    corner-radius: 999px;
    background-color: transparent;
    color: #fafafa;
    fill: #fafafa;
    font-size: 14px;
    font-variation-settings: "wght" 500;
    horizontal-gap: 8px;
    alignment: center;
    border-width: 1px;
    border-color: #3a3a3a;
    cursor: pointer;
}

.pill-ghost:hover {
    border-color: #fafafa;
}

/* ————— search ————— */

.search-box {
    width: 420px;
    height: 42px;
    padding-left: 14px;
    padding-right: 14px;
    corner-radius: 999px;
    background-color: #171717;
    border-width: 1px;
    border-color: #2a2a2a;
    color: #fafafa;
    font-size: 14px;
}

.search-box:focus {
    border-color: #a1a1a1;
    background-color: #202020;
}

/* ————— queue panel ————— */

.queue-panel {
    width: 300px;
    height: 1s;
    background-color: #171717;
    padding-top: 16px;
    padding-left: 16px;
    padding-right: 12px;
    padding-bottom: 12px;
    position-type: absolute;
    top: 0px;
    right: 0px;
}

.queue-head {
    height: 34px;
    horizontal-gap: 8px;
    alignment: left;
}

.queue-scroll {
    width: 1s;
    height: 1s;
}

.queue-list {
    width: 1s;
    vertical-gap: 2px;
}

.queue-label {
    font-size: 11px;
    color: #a1a1a1;
    padding-top: 10px;
    padding-bottom: 4px;
}

.queue-row {
    width: 1s;
    min-height: 48px;
    corner-radius: 8px;
    background-color: transparent;
    horizontal-gap: 10px;
    padding-top: 4px;
    padding-bottom: 4px;
    padding-left: 6px;
    padding-right: 6px;
    alignment: left;
    transition: background-color 120ms;
    cursor: pointer;
}

.queue-row:hover {
    background-color: #2a2a2a;
}

.queue-row.current {
    background-color: #241417;
}

.queue-art {
    width: 32px;
    height: 32px;
    corner-radius: 3px;
}

.queue-x {
    width: 22px;
    height: 22px;
    corner-radius: 999px;
    fill: #a1a1a1;
    background-color: transparent;
    alignment: center;
    cursor: pointer;
}

.queue-x:hover {
    fill: #fafafa;
    background-color: #3a3a3a;
}

/* ————— player bar ————— */

.player-bar {
    width: 1s;
    height: 76px;
    background-color: #0a0a0a;
    padding-top: 10px;
    padding-left: 16px;
    padding-right: 16px;
    padding-bottom: 12px;
    horizontal-gap: 16px;
}

.pb-info {
    width: 260px;
    horizontal-gap: 12px;
    alignment: left;
}

.pb-art {
    width: 40px;
    height: 40px;
    corner-radius: 4px;
    cursor: pointer;
}

.pb-text {
    width: 1s;
    vertical-gap: 2px;
}

.pb-title {
    font-size: 14px;
    font-variation-settings: "wght" 500;
    color: #fafafa;
    background-color: transparent;
    line-clamp: 1;
    text-wrap: false;
    text-align: left;
    padding: 0;
    corner-radius: 4px;
    cursor: pointer;
}

.pb-title:hover {
    color: #a1a1a1;
}

.pb-artist {
    font-size: 12px;
    color: #a1a1a1;
    background-color: transparent;
    line-clamp: 1;
    text-wrap: false;
    text-align: left;
    padding: 0;
    corner-radius: 4px;
    cursor: pointer;
}

.pb-artist:hover {
    color: #fafafa;
}

.pb-center {
    width: 1s;
    vertical-gap: 2px;
    alignment: top-center;
}

.pb-transport {
    height: 44px;
    horizontal-gap: 6px;
    alignment: center;
}

.pb-play {
    width: 40px;
    height: 40px;
    corner-radius: 999px;
    background-color: #ff0033;
    fill: #ffffff;
    alignment: center;
    cursor: pointer;
    transition: transform 120ms;
}

.pb-play:hover {
    transform: scale(1.05);
}

.pb-seek {
    height: 30px;
    horizontal-gap: 10px;
    alignment: center;
}

.pb-time {
    width: 40px;
    font-size: 12px;
    color: #a1a1a1;
    text-align: center;
}

.pb-right {
    width: 260px;
    horizontal-gap: 4px;
    alignment: right;
}

.pb-vol {
    width: 96px;
}

/* ————— sliders (seek + volume) ————— */

slider {
    height: 20px;
    cursor: pointer;
}

slider .track {
    height: 4px;
    corner-radius: 999px;
    background-color: #2a2a2a;
}

slider .range {
    height: 4px;
    corner-radius: 999px;
    background-color: #ff0033;
}

slider .thumb {
    width: 12px;
    height: 12px;
    corner-radius: 999px;
    background-color: #fafafa;
    border-width: 0px;
}

/* ————— scrollbars ————— */

scrollbar .thumb {
    background-color: #3f3f3f;
}

scrollview.h-scroll:hover > scrollbar.horizontal > .thumb,
scrollview.v-scroll:hover > scrollbar.vertical > .thumb {
    background-color: #555555;
}

/* ————— settings ————— */

.settings-card {
    width: 1s;
    corner-radius: 10px;
    background-color: #141414;
    padding: 16px;
    vertical-gap: 4px;
}

.settings-title {
    font-size: 13px;
    font-variation-settings: "wght" 600;
    color: #fafafa;
    padding-bottom: 8px;
}

.settings-row {
    width: 1s;
    min-height: 44px;
    horizontal-gap: 12px;
    alignment: left;
}

.settings-key {
    width: 1s;
    font-size: 14px;
    color: #fafafa;
}

.settings-val {
    font-size: 13px;
    color: #a1a1a1;
}

.empty-hint {
    width: 1s;
    padding-top: 24px;
    alignment: top-center;
    color: #a1a1a1;
    font-size: 14px;
}

.virtual-list {
    width: 1s;
    height: 1s;
}
"#;
