//! Zuno — Xilem (Masonry backend) implementation of the native-GUI benchmark
//! (gui/xilem branch). Renders the shared `zuno_core::AppState` per
//! docs/gui-benchmarks/SPEC.md.
//!
//! Xilem 0.4 / Masonry 0.4 has no production virtualized list: the stock
//! `portal` view renders all children (so the Songs tab keeps all 5,000 rows
//! resident, like iced), and while Masonry does ship an experimental
//! `VirtualScroll` widget, it has no scrollbar, no state-driven scroll API in
//! the view layer, and documented MVP caveats (see the branch report). The
//! bench scroll is driven through a small custom view (`widgets::scrolled`)
//! that maps a state fraction onto `Portal::set_viewport_pos` — xilem's stock
//! portal view has no state→widget scroll path.
//!
//! Custom widgets (see `widgets.rs`) fill xilem 0.4 gaps: a root `KeySink`
//! (global keyboard shortcuts + resize reporting, via the window's focus
//! fallback), a hover-aware `HotButton` (hover backgrounds, card play scrim,
//! animated playing bars painted in `post_paint`), window chrome (`DragStrip`,
//! `WindowControl`), vector `Glyph`s, and `Scrolled`.

mod widgets;
mod yt;

use std::collections::HashMap;
use std::time::Duration;

use masonry::core::ArcStr;
use masonry::properties::types::{CrossAxisAlignment, Length, MainAxisAlignment};
use masonry::properties::{BarColor, Background as BgProp, LineBreaking, Padding};
use masonry::vello::peniko::{Blob, ImageAlphaType, ImageBrush, ImageData, ImageFormat};
use masonry::widgets::FlexParams;
use masonry::vello::peniko::Color;
use xilem::winit::dpi::LogicalSize;
use xilem::winit::error::EventLoopError;
use xilem::core::one_of::Either;
use xilem::core::{View, fork};
use xilem::style::Style as _;
use xilem::tokio::time;
use xilem::view::{
    FlexExt as _, FlexSpacer, checkbox, flex_col, flex_row, image, label, portal, slider,
    task_raw, text_input, ObjectFit,
};
use xilem::{EventLoop, FontWeight, ViewCtx, WidgetView, WindowId, Xilem};

use zuno_core::bench::{BenchAction, BenchDriver};
use zuno_core::format::{counts_line, listeners, mmss};
use zuno_core::model::*;
use zuno_core::queue::Region;
use zuno_core::search;
use zuno_core::theme as t;
use zuno_core::app::{LibraryTab, RepeatMode};
use zuno_core::library::Library;
use zuno_core::{AppState, View as AppView};
use yt::{YtReq, YtResp};


use crate::widgets::{
    Icon, KeyAction, Overlay, WindowControlKind, drag_strip, glyph, hot, key_sink, scrolled,
    window_control,
};

// — Design tokens (core::theme) as peniko colors ———————————————————————————

fn rgb(c: t::Rgb) -> Color {
    Color::from_rgb8(c.0[0], c.0[1], c.0[2])
}
const fn rgbc(c: t::Rgb) -> Color {
    Color::from_rgb8(c.0[0], c.0[1], c.0[2])
}
const BG: Color = rgbc(t::DARK.background);
const FG: Color = rgbc(t::DARK.foreground);
const CARD: Color = rgbc(t::DARK.card);
const POPOVER: Color = rgbc(t::DARK.popover);
const MUTED_FG: Color = rgbc(t::DARK.muted_foreground);
const PRIMARY: Color = rgbc(t::DARK.primary);
const PRIMARY_HOVER: Color = Color::from_rgb8(0xff, 0x2e, 0x4d);
const CLEAR: Color = Color::TRANSPARENT;

/// The Zuno shell radius for the frameless window surface.
const SHELL_RADIUS: f64 = 14.0;

const WINDOW_SIZE: LogicalSize<f64> = LogicalSize::new(1280.0, 800.0);
const WINDOW_MIN: LogicalSize<f64> = LogicalSize::new(900.0, 600.0);

/// Anything view type-erased through the sequence machinery.
type AnyV = Box<xilem::AnyWidgetView<Zuno, ()>>;

fn px(v: impl Into<f64>) -> Length {
    Length::px(v.into())
}

/// FlexParams that expand on the main axis and fill on the cross axis.
fn fill() -> FlexParams {
    FlexParams::new(Some(1.0), Some(CrossAxisAlignment::Fill))
}

// — App state ——————————————————————————————————————————————————————————————

struct Zuno {
    app: AppState,
    /// (seed, size, round) → brush. The decoded-bitmap budget stays in core's
    /// shared 96-cover LRU; this maps what the visible views actually need.
    art: HashMap<(u64, u32, bool), ImageBrush>,
    queue_open: bool,
    frame: u64,
    win_w: f64,
    win_h: f64,
    /// Bench-driven scroll fraction for the Library▸Songs list.
    songs_scroll: Option<f64>,
    bench: Option<BenchDriver>,
    bench_pending: bool,
    ticks: u64,
    window_id: WindowId,
    /// Settings-page demo toggles (rendered, not wired).
    settings_autostart: bool,
    settings_explicit: bool,
    // — Real-YouTube wiring (all inert in --bench: no worker, no channels) —
    yt_tx: Option<crossbeam_channel::Sender<YtReq>>,
    yt_rx: Option<crossbeam_channel::Receiver<YtResp>>,
    yt_resp_tx: Option<crossbeam_channel::Sender<YtResp>>,
    home_lib: Option<Library>,
    lib_is_search: bool,
    covers_ready: std::collections::HashSet<String>,
    covers_flying: std::collections::HashSet<String>,
    cookie: String,
    yt_status: String,
    search_seq: u64,
    resolving: Option<TrackId>,
    stream_for: Option<TrackId>,
}

impl Zuno {
    fn art(&mut self, seed: u64, size: u32, round: bool) -> ImageBrush {
        if let Some(b) = self.art.get(&(seed, size, round)) {
            return b.clone();
        }
        let rgba = self.app.artwork.get(seed, size);
        let data: Vec<u8> = if round { mask_circle(&rgba, size) } else { rgba.to_vec() };
        let brush = ImageBrush::new(ImageData {
            data: Blob::from(data),
            format: ImageFormat::Rgba8,
            alpha_type: ImageAlphaType::Alpha,
            width: size,
            height: size,
        });
        self.art.insert((seed, size, round), brush.clone());
        brush
    }

    /// Real cover when fetched, procedural until then (the fetch is kicked
    /// here). Bench tracks carry no `artwork_url` — bench never leaves the
    /// procedural path. URL keys flip the top bit — no collision with seeds.
    fn cover(&mut self, url: Option<&str>, seed: u64, size: u32, round: bool) -> ImageBrush {
        if let Some(u) = url {
            if self.covers_ready.contains(u) {
                let key = yt::cover_key(u);
                if let Some(b) = self.art.get(&(key, size, round)) {
                    return b.clone();
                }
                let rgba = self.app.artwork.get_url(u, size);
                return self.brush_from(rgba, key, size, round);
            }
            if self.covers_flying.insert(u.to_string()) {
                if let Some(tx) = &self.yt_resp_tx {
                    let tx = tx.clone();
                    let u2 = u.to_string();
                    std::thread::spawn(move || {
                        let bytes = zuno_yt::fetch_artwork(&u2).unwrap_or_default();
                        let _ = tx.send(YtResp::Cover { url: u2, bytes });
                    });
                }
            }
        }
        self.art(seed, size, round)
    }

    fn brush_from(&mut self, rgba: std::sync::Arc<Vec<u8>>, key: u64, size: u32, round: bool) -> ImageBrush {
        let data: Vec<u8> = if round { mask_circle(&rgba, size) } else { rgba.to_vec() };
        let brush = ImageBrush::new(ImageData {
            data: Blob::from(data),
            format: ImageFormat::Rgba8,
            alpha_type: ImageAlphaType::Alpha,
            width: size,
            height: size,
        });
        self.art.insert((key, size, round), brush.clone());
        brush
    }
}

/// Circle alpha-mask an RGBA buffer (artist artwork is round in Zuno).
fn mask_circle(rgba: &[u8], size: u32) -> Vec<u8> {
    let s = size as f64;
    let r = s / 2.0 - 0.5;
    let mut out = rgba.to_vec();
    for y in 0..size as usize {
        for x in 0..size as usize {
            let (fx, fy) = (x as f64 + 0.5 - s / 2.0, y as f64 + 0.5 - s / 2.0);
            let d = (fx * fx + fy * fy).sqrt();
            let a = if d > r + 0.5 {
                0
            } else if d > r - 0.5 {
                (((r + 0.5 - d) * 255.0).round() as u8).min(255)
            } else {
                255
            };
            out[(y * size as usize + x) * 4 + 3] = a;
        }
    }
    out
}

impl xilem::AppState for Zuno {
    fn keep_running(&self) -> bool {
        true
    }
}

impl Zuno {
    /// Swap the whole library (home / live search / sign-in), resetting
    /// per-list state — same contract as the other wired branches.
    fn swap_library(&mut self, lib: Library, is_search: bool) {
        self.app.player.stop();
        self.app.library = lib;
        self.app.current = None;
        self.app.playing = false;
        self.app.selection = None;
        self.lib_is_search = is_search;
        self.resolving = None;
        self.stream_for = None;
    }

    /// Drain worker/cover responses on the UI thread, once per tick.
    fn drain_yt(&mut self) {
        let Some(rx) = self.yt_rx.take() else { return };
        while let Ok(resp) = rx.try_recv() {
            match resp {
                YtResp::Home(result) => match result {
                    Ok(lib) => {
                        eprintln!("[yt] home painted: {} tracks", lib.tracks.len());
                        self.home_lib = Some(lib.clone());
                        if !self.lib_is_search {
                            self.swap_library(lib, false);
                        }
                        self.yt_status = "Live · YouTube Music".to_string();
                    }
                    Err(e) => self.yt_status = format!("YouTube offline — synthetic ({e})"),
                },
                YtResp::Search { seq, result } => {
                    if seq != self.search_seq {
                        continue; // stale keystroke
                    }
                    match result {
                        Ok(lib) => self.swap_library(lib, true),
                        Err(e) => self.yt_status = format!("Search failed: {e}"),
                    }
                }
                YtResp::Stream(result) => match result {
                    Ok((track, _vid, info, cookie)) => {
                        if self.app.current == Some(track) {
                            self.app.player.play_stream(&info.url, &info.mime, cookie.as_deref());
                            self.app.playing = true;
                            self.yt_status = "Live · YouTube Music".to_string();
                        }
                        self.stream_for = Some(track);
                        self.resolving = None;
                    }
                    Err(_e) => {
                        if let Some(cur) = self.app.current {
                            self.stream_for = Some(cur); // keep synth; don't retry-loop
                        }
                        self.resolving = None;
                    }
                },
                YtResp::SignedIn(result) => match result {
                    Ok((lib, who)) => {
                        self.home_lib = Some(lib.clone());
                        self.swap_library(lib, false);
                        self.cookie.clear();
                        self.yt_status = format!("Signed in as {who}");
                    }
                    Err(msg) => self.yt_status = msg,
                },
                YtResp::Cover { url, bytes } => {
                    self.covers_flying.remove(&url);
                    if !bytes.is_empty() {
                        // Every size the render path uses (see cover() calls).
                        let mut ok = false;
                        for size in [40u32, 120, 176, 232] {
                            if self.app.artwork.insert_bytes(&url, size, &bytes).is_some() {
                                ok = true;
                            }
                        }
                        if ok {
                            self.covers_ready.insert(url);
                        }
                    }
                }
            }
        }
        self.yt_rx = Some(rx);
    }

    /// After `tick`: a YT track left on synth gets exactly one resolve.
    fn maybe_request_stream(&mut self) {
        if self.yt_tx.is_none() {
            return;
        }
        let Some(id) = self.app.current else { return };
        if self.stream_for == Some(id) || self.resolving == Some(id) {
            return;
        }
        let vid = self.app.library.track(id).video_id.clone();
        if vid.is_empty() {
            return;
        }
        self.resolving = Some(id);
        let _ = self
            .yt_tx
            .as_ref()
            .unwrap()
            .send(YtReq::Resolve { track: id, video_id: vid });
    }
}

// — main / window —————————————————————————————————————————————————————————

fn main() -> Result<(), EventLoopError> {
    let bench = std::env::args().any(|a| a == "--bench");
    let mut zuno = Zuno {
        app: AppState::new(),
        art: HashMap::new(),
        queue_open: false,
        frame: 0,
        win_w: WINDOW_SIZE.width,
        win_h: WINDOW_SIZE.height,
        songs_scroll: None,
        bench: None,
        bench_pending: bench,
        ticks: 0,
        window_id: WindowId::next(),
        settings_autostart: false,
        settings_explicit: true,
        yt_tx: None,
        yt_rx: None,
        yt_resp_tx: None,
        home_lib: None,
        lib_is_search: false,
        covers_ready: std::collections::HashSet::new(),
        covers_flying: std::collections::HashSet::new(),
        cookie: String::new(),
        yt_status: String::new(),
        search_seq: 0,
        resolving: None,
        stream_for: None,
    };
    // Wire the real-YouTube worker (interactive only; bench stays synthetic).
    if !bench {
        let (req_tx, req_rx) = crossbeam_channel::unbounded::<YtReq>();
        let (resp_tx, resp_rx) = crossbeam_channel::unbounded::<YtResp>();
        yt::spawn(req_rx, resp_tx.clone());
        zuno.yt_tx = Some(req_tx);
        zuno.yt_rx = Some(resp_rx);
        zuno.yt_resp_tx = Some(resp_tx);
        let _ = zuno.yt_tx.as_ref().unwrap().send(YtReq::Home);
    }
    Xilem::new(zuno, app_logic)
        .with_font(Blob::from(include_bytes!("../assets/Inter-Variable.ttf").to_vec()))
        .run_in(EventLoop::with_user_event())?;
    Ok(())
}

/// One window whose root is the `KeySink` — it receives keyboard events
/// whenever no text input has focus (the focus-fallback), and reports resizes.
fn app_logic(z: &mut Zuno) -> impl Iterator<Item = xilem::WindowView<Zuno>> + use<> {
    let id = z.window_id;
    std::iter::once(
        xilem::window(
            id,
            "Zuno",
            key_sink(handle_key_action, fork(shell(z), tick_task())),
        )
        // The shell paints its own rounded surface; the window clears to
        // transparent so the corners can be truly see-through.
        .with_base_color(CLEAR)
        .with_options(|o| {
            o.with_decorations(false)
                .with_transparent(true)
                .with_min_inner_size(WINDOW_MIN)
                .with_initial_inner_size(WINDOW_SIZE)
        }),
    )
}

// — Keyboard ——————————————————————————————————————————————————————————————

fn handle_key_action(z: &mut Zuno, action: KeyAction) {
    match action {
        KeyAction::TogglePlay => z.app.toggle_play(),
        KeyAction::SeekDelta(d) => {
            let pos = z.app.player.position_sec();
            z.app.seek((pos + d).max(0.0));
        }
        KeyAction::Escape => {
            if z.app.view == AppView::Search && !z.app.search_query.is_empty() {
                z.app.set_search("");
            } else {
                z.app.go_back();
            }
        }
        KeyAction::Mute => z.app.toggle_mute(),
        KeyAction::ToggleQueue => z.queue_open = !z.queue_open,
        KeyAction::Shuffle => z.app.toggle_shuffle(),
        KeyAction::Repeat => z.app.cycle_repeat(),
        KeyAction::FocusSearch => {
            let q = z.app.search_query.clone();
            z.app.set_search(&q);
        }
        KeyAction::Resized(w, h) => {
            if std::env::var("ZUNO_DEBUG").is_ok() {
                eprintln!("resized {w}x{h}");
            }
            z.win_w = w;
            z.win_h = h;
        }
    }
}

/// The 16 ms heartbeat: advances playback and drives the bench schedule.
fn tick_task() -> impl View<Zuno, (), ViewCtx, Element = xilem::core::NoElement> + use<> {
    task_raw(
        |proxy| async move {
            let mut interval = time::interval(Duration::from_millis(16));
            interval.set_missed_tick_behavior(time::MissedTickBehavior::Skip);
            loop {
                interval.tick().await;
                let Ok(()) = proxy.message(()) else { break };
            }
        },
        |z: &mut Zuno, ()| {
            z.frame += 1;
            z.ticks += 1;
            z.app.tick();
            z.drain_yt();
            z.maybe_request_stream();
            // Construct the bench driver ~0.5 s after the first frame so
            // vello's first-frame shader warmup isn't billed to the schedule.
            if z.bench_pending && z.ticks >= 30 {
                z.bench_pending = false;
                z.bench = Some(BenchDriver::new());
                // Park at Library▸Songs on the first ScrollTo (like iced),
                // so startup/idle samples are the home view.
            }
            let action = z.bench.as_mut().map(|b| b.tick());
            match action {
                Some(BenchAction::ScrollTo(f)) => {
                    if z.app.view != AppView::Library {
                        z.app.go(AppView::Library);
                    }
                    if z.app.library_tab != LibraryTab::Songs {
                        z.app.set_library_tab(LibraryTab::Songs);
                    }
                    z.songs_scroll = Some(f as f64);
                }
                Some(BenchAction::SelectRow(i)) => {
                    z.app.selection =
                        Some(i.min(z.app.library.tracks.len().saturating_sub(1)));
                }
                Some(BenchAction::StartPlayback) => {
                    let ids = z.app.list_ids();
                    if !ids.is_empty() {
                        let idx = ids.len() / 2;
                        z.app.play_from(&ids, idx);
                    }
                }
                Some(BenchAction::Finish) => {
                    if let Some(b) = z.bench.take() {
                        let report = b.report;
                        println!("{report}");
                    }
                    std::process::exit(0);
                }
                _ => {}
            }
        },
    )
}

// — Text helpers (Inter everywhere; sizes/weights from core::theme) —————————

fn weight(w: u16) -> FontWeight {
    match w {
        700 => FontWeight::BOLD,
        600 => FontWeight::SEMI_BOLD,
        500 => FontWeight::MEDIUM,
        _ => FontWeight::NORMAL,
    }
}

fn txt(s: impl Into<ArcStr>, size: f32, w: u16, color: Color) -> xilem::view::Prop<masonry::properties::ContentColor, xilem::view::Label, Zuno, ()> {
    label(s)
        .text_size(size)
        .weight(weight(w))
        .font(t::FONT_FAMILY)
        .color(color)
}

fn h1(s: impl Into<ArcStr>) -> xilem::view::Prop<masonry::properties::ContentColor, xilem::view::Label, Zuno, ()> {
    txt(s, t::H1.size, t::H1.weight, FG)
}
fn h2(s: impl Into<ArcStr>) -> xilem::view::Prop<masonry::properties::ContentColor, xilem::view::Label, Zuno, ()> {
    txt(s, t::H2.size, t::H2.weight, FG)
}
fn h3(s: impl Into<ArcStr>) -> xilem::view::Prop<masonry::properties::ContentColor, xilem::view::Label, Zuno, ()> {
    txt(s, t::H3.size, t::H3.weight, FG)
}
fn body(s: impl Into<ArcStr>) -> xilem::view::Prop<masonry::properties::ContentColor, xilem::view::Label, Zuno, ()> {
    txt(s, t::BODY.size, t::BODY.weight, FG)
}
fn body_med(s: impl Into<ArcStr>) -> xilem::view::Prop<masonry::properties::ContentColor, xilem::view::Label, Zuno, ()> {
    txt(s, t::BODY_MED.size, t::BODY_MED.weight, FG)
}
fn small(s: impl Into<ArcStr>) -> xilem::view::Prop<masonry::properties::ContentColor, xilem::view::Label, Zuno, ()> {
    txt(s, t::SMALL.size, t::SMALL.weight, MUTED_FG)
}
fn small_med(s: impl Into<ArcStr>) -> xilem::view::Prop<masonry::properties::ContentColor, xilem::view::Label, Zuno, ()> {
    txt(s, t::SMALL_MED.size, t::SMALL_MED.weight, MUTED_FG)
}

fn row_hover() -> Color {
    rgb(t::state::ROW_HOVER)
}
fn row_playing() -> Color {
    rgb(t::state::row_playing())
}
fn row_selected() -> Color {
    rgb(t::state::row_selected())
}

// — Shell ——————————————————————————————————————————————————————————————————

fn shell(z: &mut Zuno) -> impl WidgetView<Zuno> + use<> {
    flex_col((
        xilem::view::sized_box(titlebar(z))
            .height(px(t::TITLEBAR_H))
            .into_any_flex(),
        main_row(z).flex(fill()).into_any_flex(),
        xilem::view::sized_box(player_bar(z))
            .height(px(t::PLAYER_BAR_H))
            .into_any_flex(),
    ))
    .gap(Length::ZERO)
    .must_fill_major_axis(true)
    .background_color(BG)
    .corner_radius(SHELL_RADIUS)
}

/// 44px custom titlebar: wordmark in a drag region + window controls.
fn titlebar(_z: &mut Zuno) -> impl WidgetView<Zuno> + use<> {
    let wordmark = txt("ZUNO", 15.0, 700, FG);
    flex_row((
        drag_strip(xilem::view::sized_box(wordmark).width(px(t::SIDEBAR_W)))
            .flex(1.0)
            .into_any_flex(),
        window_btn(Icon::Minus, WindowControlKind::Minimize, CLEAR).into_any_flex(),
        window_btn(Icon::MaxSquare, WindowControlKind::Maximize, CLEAR).into_any_flex(),
        window_btn(Icon::XClose, WindowControlKind::Close, Color::from_rgb8(0x99, 0x11, 0x1f))
            .into_any_flex(),
    ))
    .gap(Length::ZERO)
    .cross_axis_alignment(CrossAxisAlignment::Center)
    .must_fill_major_axis(true)
    .boxed()
    // height via sized_box in the shell's flex params
}

fn window_btn(icon: Icon, kind: WindowControlKind, hover: Color) -> impl WidgetView<Zuno> + use<> {
    window_control(
        kind,
        xilem::view::sized_box(glyph(icon, 14.0, MUTED_FG))
            .width(px(46.0))
            .height(px(30.0)),
    )
    .prop(widgets::HoveredBackground(BgProp::Color(hover)))
    .corner_radius(6.0)
}

/// Sidebar + content + (toggleable) queue panel.
fn main_row(z: &mut Zuno) -> impl WidgetView<Zuno> + use<> {
    if z.queue_open {
        Either::A(flex_row((
            xilem::view::sized_box(sidebar_view(z))
                .width(px(t::SIDEBAR_W))
                .boxed()
                .into_any_flex(),
            content_view(z).flex(fill()).into_any_flex(),
            queue_panel(z).into_any_flex(),
        ))
        .gap(Length::ZERO)
        .cross_axis_alignment(CrossAxisAlignment::Fill)
        .must_fill_major_axis(true))
    } else {
        Either::B(flex_row((
            xilem::view::sized_box(sidebar_view(z))
                .width(px(t::SIDEBAR_W))
                .boxed()
                .into_any_flex(),
            content_view(z).flex(fill()).into_any_flex(),
        ))
        .gap(Length::ZERO)
        .cross_axis_alignment(CrossAxisAlignment::Fill)
        .must_fill_major_axis(true))
    }
}

/// Search header + scrolling page content.
fn content_view(z: &mut Zuno) -> impl WidgetView<Zuno> + use<> {
    flex_col((
        content_header(z).into_any_flex(),
        page(z).flex(fill()).into_any_flex(),
    ))
    .gap(Length::ZERO)
    .must_fill_major_axis(true)
    .background_color(BG)
}

/// Back button (when there's history) + the always-present search field.
fn content_header(z: &mut Zuno) -> impl WidgetView<Zuno> + use<> {
    let can_back = z.app.can_go_back();
    let back: AnyV = if can_back {
        hot(
            xilem::view::sized_box(glyph(Icon::ChevronLeft, 18.0, MUTED_FG))
                .width(px(28.0))
                .height(px(28.0)),
            |z: &mut Zuno| {
                z.app.go_back();
            },
        )
        .prop(widgets::HoveredBackground(BgProp::Color(row_hover())))
        .corner_radius(t::RADIUS_ROW as f64)
        .boxed()
    } else {
        xilem::view::sized_box(label("")).width(px(28.0)).height(px(28.0)).boxed()
    };
    let query = z.app.search_query.clone();
    let search_field = text_input(query, |z: &mut Zuno, q: String| {
        z.app.set_search(&q);
        if let Some(tx) = &z.yt_tx {
            z.search_seq += 1;
            let _ = tx.send(YtReq::Search { seq: z.search_seq, query: q.clone() });
            if q.trim().is_empty() && z.lib_is_search {
                if let Some(home) = z.home_lib.clone() {
                    z.swap_library(home, false);
                }
            }
        }
    })
    .placeholder("What do you want to listen to?")
    .text_color(FG)
    .prop(masonry::properties::PlaceholderColor::new(MUTED_FG))
    .prop(masonry::properties::SelectionColor { color: PRIMARY })
    .prop(masonry::properties::CaretColor { color: FG })
    .boxed();
    flex_row((back.into_any_flex(), search_field.flex(fill()).into_any_flex()))
        .gap(px(8.0))
        .cross_axis_alignment(CrossAxisAlignment::Center)
        .padding(Padding::from_vh(10.0, t::PAGE_PAD as f64))
}

// — Sidebar ———————————————————————————————————————————————————————————————

fn nav_item(name: &'static str, target: AppView, active: bool) -> AnyV {
    let color = if active { FG } else { MUTED_FG };
    hot(
        body_med(name).color(color),
        move |z: &mut Zuno| {
            z.app.go(target.clone());
        },
    )
    .prop(widgets::HoveredBackground(BgProp::Color(row_hover())))
    .prop(if active { BgProp::Color(CARD) } else { BgProp::Color(CLEAR) })
    .corner_radius(t::RADIUS_ROW as f64)
    .padding(Padding::from_vh(10.0, 12.0))
    .boxed()
}

fn sidebar_view(z: &mut Zuno) -> impl WidgetView<Zuno> + use<> {
    let view = z.app.view.clone();
    let nav: Vec<AnyV> = vec![
        nav_item("Home", AppView::Home, view == AppView::Home),
        nav_item("Search", AppView::Search, view == AppView::Search),
        nav_item("Library", AppView::Library, view == AppView::Library),
        nav_item("Settings", AppView::Settings, view == AppView::Settings),
    ];
    let playlist_ids: Vec<PlaylistId> = z.app.library.playlists.iter().map(|p| p.id).collect();
    let playlists: Vec<AnyV> = playlist_ids.iter().map(|&id| playlist_row(z, id)).collect();
    let list = portal(
        flex_col(playlists)
            .gap(px(2.0))
            .cross_axis_alignment(CrossAxisAlignment::Fill),
    );
    flex_col((
        FlexSpacer::Fixed(px(10.0)),
        nav,
        FlexSpacer::Fixed(px(16.0)),
        small("PLAYLISTS").into_any_flex(),
        FlexSpacer::Fixed(px(4.0)),
        list.flex(fill()).into_any_flex(),
    ))
    .gap(px(6.0))
    .cross_axis_alignment(CrossAxisAlignment::Fill)
    .must_fill_major_axis(true)
    .background_color(POPOVER)
}

fn playlist_row(z: &mut Zuno, id: PlaylistId) -> AnyV {
    let p = z.app.library.playlist(id).unwrap();
    let title = p.title.clone();
    let count = p.track_ids.len();
    let seed = AppState::playlist_seed(id);
    let active = matches!(&z.app.view, AppView::Playlist(v) if *v == id);
    let art_url = p.artwork_url.clone();
    let art = z.cover(art_url.as_deref(), seed, t::ROW_ART as u32, false);
    hot(
        flex_row((
            xilem::view::sized_box(image(art).fit(ObjectFit::Fill))
                .width(px(t::ROW_ART))
                .height(px(t::ROW_ART))
                .corner_radius(t::RADIUS_ART as f64)
                .into_any_flex(),
            flex_col((
                body_med(title).line_break_mode(LineBreaking::Clip).into_any_flex(),
                small(format!("{count} songs")).into_any_flex(),
            ))
            .gap(Length::ZERO)
            .cross_axis_alignment(CrossAxisAlignment::Fill)
            .flex(1.0)
            .into_any_flex(),
        ))
        .gap(px(10.0))
        .cross_axis_alignment(CrossAxisAlignment::Center),
        move |z: &mut Zuno| {
            z.app.open_playlist(id);
        },
    )
    .prop(widgets::HoveredBackground(BgProp::Color(row_hover())))
    .prop(if active { BgProp::Color(CARD) } else { BgProp::Color(CLEAR) })
    .corner_radius(t::RADIUS_ROW as f64)
    .padding(Padding::from_vh(6.0, 10.0))
    .boxed()
}

// — Pages ———————————————————————————————————————————————————————————————————

fn page(z: &mut Zuno) -> impl WidgetView<Zuno> + use<> {
    let content: AnyV = match z.app.view.clone() {
        AppView::Home => home_page(z),
        AppView::Library => library_page(z),
        AppView::Album(id) => album_page(z, id),
        AppView::Playlist(id) => playlist_page(z, id),
        AppView::Artist(id) => artist_page(z, id),
        AppView::Search => search_page(z),
        AppView::Settings => settings_page(z),
    };
    portal(content)
}

/// Home: recently played shelf, made-for-you mixes, new albums grid,
/// popular artists shelf.
fn home_page(z: &mut Zuno) -> AnyV {
    let recent: Vec<AlbumId> = z.app.library.home_recent_albums(12);
    let mixes: Vec<PlaylistId> = z.app.library.mixes.iter().map(|m| m.id).collect();
    let mut albums: Vec<AlbumId> = z.app.library.albums.iter().map(|a| a.id).collect();
    albums.sort_by_key(|&id| std::cmp::Reverse(z.app.library.album(id).year));
    albums.truncate(24);
    let artists: Vec<ArtistId> = (0..12.min(z.app.library.artists.len()) as u32).collect();

    flex_col((
        h1("Home").into_any_flex(),
        section("Recently played", album_shelf(z, &recent)).into_any_flex(),
        section("Made for you", mix_shelf(z, &mixes)).into_any_flex(),
        section("New albums", albums_grid(z, &albums)).into_any_flex(),
        section("Popular artists", artist_shelf(z, &artists)).into_any_flex(),
    ))
    .gap(px(t::SECTION_GAP))
    .cross_axis_alignment(CrossAxisAlignment::Fill)
    .padding(Padding::from_vh(0.0, t::PAGE_PAD as f64))
    .boxed()
}

/// h2 title + shelf content.
fn section(shelf_title: &str, shelf: impl WidgetView<Zuno>) -> impl WidgetView<Zuno> {
    flex_col((h2(shelf_title).into_any_flex(), shelf.into_any_flex()))
        .gap(px(12.0))
        .cross_axis_alignment(CrossAxisAlignment::Fill)
}

fn album_shelf(z: &mut Zuno, ids: &[AlbumId]) -> impl WidgetView<Zuno> + use<> {
    let cards: Vec<AnyV> = ids.iter().map(|&id| album_card(z, id)).collect();
    portal(flex_row(cards).gap(px(16.0)).cross_axis_alignment(CrossAxisAlignment::Start))
}

fn mix_shelf(z: &mut Zuno, ids: &[PlaylistId]) -> impl WidgetView<Zuno> + use<> {
    let cards: Vec<AnyV> = ids.iter().map(|&id| mix_card(z, id)).collect();
    portal(flex_row(cards).gap(px(16.0)).cross_axis_alignment(CrossAxisAlignment::Start))
}

fn artist_shelf(z: &mut Zuno, ids: &[ArtistId]) -> impl WidgetView<Zuno> + use<> {
    let cards: Vec<AnyV> = ids.iter().map(|&id| artist_card(z, id)).collect();
    portal(flex_row(cards).gap(px(16.0)).cross_axis_alignment(CrossAxisAlignment::Start))
}

/// A responsive album grid: cards chunk into rows that reflow with width.
fn albums_grid(z: &mut Zuno, ids: &[AlbumId]) -> impl WidgetView<Zuno> + use<> {
    let per_row = (((z.win_w - t::SIDEBAR_W as f64 - 2.0 * t::PAGE_PAD as f64)
        / (t::CARD_W as f64 + 16.0))
        .floor() as usize)
        .max(2);
    let rows: Vec<AnyV> = ids
        .chunks(per_row)
        .map(|chunk| {
            let cards: Vec<AnyV> = chunk.iter().map(|&id| album_card(z, id)).collect();
            flex_row(cards)
                .gap(px(16.0))
                .cross_axis_alignment(CrossAxisAlignment::Start)
                .boxed()
        })
        .collect();
    flex_col(rows)
        .gap(px(16.0))
        .cross_axis_alignment(CrossAxisAlignment::Fill)
}

/// 176px album card: square artwork (hover: scrim + red play pill),
/// title, "Artist · Year" subtitle.
fn album_card(z: &mut Zuno, id: AlbumId) -> AnyV {
    let a = z.app.library.album(id);
    let title = a.title.clone();
    let artist = z.app.library.artist(a.artist_id).name.clone();
    let year = a.year;
    let art_url = a.artwork_url.clone();
    let art = z.cover(art_url.as_deref(), AppState::album_seed(id), t::CARD_W as u32, false);
    xilem::view::sized_box(flex_col((
        hot(
            xilem::view::sized_box(image(art).fit(ObjectFit::Fill))
                .width(px(t::CARD_W))
                .height(px(t::CARD_W)),
            move |z: &mut Zuno| {
                z.app.open_album(id);
            },
        )
        .overlay(Overlay::CardPlay)
        .corner_radius(t::RADIUS_ART as f64)
        .into_any_flex(),
        body_med(title).line_break_mode(LineBreaking::Clip).into_any_flex(),
        small(format!("{artist} · {year}")).line_break_mode(LineBreaking::Clip).into_any_flex(),
    ))
    .gap(px(6.0))
    .cross_axis_alignment(CrossAxisAlignment::Fill))
    .width(px(t::CARD_W))
    .boxed()
}

fn mix_card(z: &mut Zuno, id: PlaylistId) -> AnyV {
    let p = z.app.library.playlist(id).unwrap();
    let title = p.title.clone();
    let art_url = p.artwork_url.clone();
    let art = z.cover(art_url.as_deref(), AppState::playlist_seed(id), t::CARD_W as u32, false);
    xilem::view::sized_box(flex_col((
        hot(
            xilem::view::sized_box(image(art).fit(ObjectFit::Fill))
                .width(px(t::CARD_W))
                .height(px(t::CARD_W)),
            move |z: &mut Zuno| {
                z.app.open_playlist(id);
            },
        )
        .overlay(Overlay::CardPlay)
        .corner_radius(t::RADIUS_ART as f64)
        .into_any_flex(),
        body_med(title).line_break_mode(LineBreaking::Clip).into_any_flex(),
        small("Made for you").into_any_flex(),
    ))
    .gap(px(6.0))
    .cross_axis_alignment(CrossAxisAlignment::Fill))
    .width(px(t::CARD_W))
    .boxed()
}

fn artist_card(z: &mut Zuno, id: ArtistId) -> AnyV {
    let name = z.app.library.artist(id).name.clone();
    let art_url = z.app.library.artist(id).artwork_url.clone();
    let art = z.cover(art_url.as_deref(), AppState::artist_seed(id), t::CARD_W as u32, true);
    xilem::view::sized_box(flex_col((
        hot(
            xilem::view::sized_box(image(art).fit(ObjectFit::Fill))
                .width(px(t::CARD_W))
                .height(px(t::CARD_W)),
            move |z: &mut Zuno| {
                z.app.open_artist(id);
            },
        )
        .overlay(Overlay::CardPlay)
        .into_any_flex(),
        body_med(name).line_break_mode(LineBreaking::Clip).into_any_flex(),
        small("Artist").into_any_flex(),
    ))
    .gap(px(6.0))
    .cross_axis_alignment(CrossAxisAlignment::Fill))
    .width(px(t::CARD_W))
    .boxed()
}

// — Library ——————————————————————————————————————————————————————————————

fn tab_pill(name: &'static str, target: LibraryTab, active: bool) -> AnyV {
    hot(
        small_med(name).color(if active { FG } else { MUTED_FG }),
        move |z: &mut Zuno| {
            z.app.set_library_tab(target);
        },
    )
    .prop(widgets::HoveredBackground(BgProp::Color(row_hover())))
    .prop(if active { BgProp::Color(CARD) } else { BgProp::Color(CLEAR) })
    .corner_radius(999.0)
    .padding(Padding::from_vh(8.0, 16.0))
    .boxed()
}

fn library_page(z: &mut Zuno) -> AnyV {
    let tab = z.app.library_tab;
    let tabs: Vec<AnyV> = vec![
        tab_pill("Playlists", LibraryTab::Playlists, tab == LibraryTab::Playlists),
        tab_pill("Albums", LibraryTab::Albums, tab == LibraryTab::Albums),
        tab_pill("Artists", LibraryTab::Artists, tab == LibraryTab::Artists),
        tab_pill("Songs", LibraryTab::Songs, tab == LibraryTab::Songs),
    ];
    let body: AnyV = match tab {
        LibraryTab::Songs => songs_list(z),
        LibraryTab::Albums => {
            let ids: Vec<AlbumId> = z.app.library.albums.iter().map(|a| a.id).collect();
            albums_grid(z, &ids).boxed()
        }
        LibraryTab::Artists => {
            let rows: Vec<AnyV> = (0..z.app.library.artists.len() as u32)
                .map(|id| artist_row(z, id))
                .collect();
            flex_col(rows)
                .gap(px(2.0))
                .cross_axis_alignment(CrossAxisAlignment::Fill)
                .boxed()
        }
        LibraryTab::Playlists => {
            let playlist_ids: Vec<PlaylistId> =
                z.app.library.playlists.iter().map(|p| p.id).collect();
            let rows: Vec<AnyV> = playlist_ids.iter().map(|&id| playlist_row(z, id)).collect();
            flex_col(rows)
                .gap(px(2.0))
                .cross_axis_alignment(CrossAxisAlignment::Fill)
                .boxed()
        }
    };
    flex_col((
        h1("Your Library").into_any_flex(),
        flex_row(tabs)
            .gap(px(8.0))
            .cross_axis_alignment(CrossAxisAlignment::Center)
            .into_any_flex(),
        body,
    ))
    .gap(px(t::PAGE_PAD))
    .cross_axis_alignment(CrossAxisAlignment::Fill)
    .padding(Padding::from_vh(0.0, t::PAGE_PAD as f64))
    .boxed()
}

/// THE 5,000-track list — the bench scroll target. All rows are resident:
/// xilem 0.4 has no production virtualized list with scroll control; this is
/// recorded honestly in the report (Masonry's experimental `VirtualScroll`
/// widget exists but isn't drivable from the view layer and has no scrollbar).
fn songs_list(z: &mut Zuno) -> AnyV {
    let n = z.app.library.tracks.len();
    let rows: Vec<_> = (0..n as u32)
        .map(|id| {
            xilem::view::flex_item(track_row(z, id as usize, id, play_from_current(id)), FlexParams::default())
        })
        .collect();
    scrolled(
        z.songs_scroll,
        flex_col(rows)
            .gap(Length::ZERO)
            .cross_axis_alignment(CrossAxisAlignment::Fill)
            .must_fill_major_axis(true),
    )
    .boxed()
}

/// Click handler for a row in the current view's list.
fn play_from_current(id: TrackId) -> impl Fn(&mut Zuno) + Send + Sync + use<> {
    move |z: &mut Zuno| {
        let ids = z.app.list_ids();
        if let Some(pos) = ids.iter().position(|&t| t == id) {
            z.app.play_from(&ids, pos);
        } else {
            z.app.play_track(id);
        }
    }
}

/// A generic 52px track row: index, 40px artwork, title (+ "E" badge),
/// artist, duration or red heart, like toggle.
fn track_row(z: &mut Zuno, index: usize, id: TrackId, on_play: impl Fn(&mut Zuno) + Send + Sync + 'static) -> AnyV {
    let tv = z.app.library.track_view(id);
    let title: ArcStr = tv.title.into();
    let artist: ArcStr = tv.artist.into();
    let dur = mmss(tv.duration_sec);
    let liked = tv.liked;
    let explicit = tv.explicit;
    let playing = z.app.current == Some(id);
    let selected = z.app.selection == Some(index);
    let album_seed = AppState::album_seed(z.app.library.track(id).album_id);
    let art_url = z.app.library.track(id).artwork_url.clone();
    let art = z.cover(art_url.as_deref(), album_seed, t::ROW_ART as u32, false);

    let bg = if playing {
        row_playing()
    } else if selected {
        row_selected()
    } else {
        CLEAR
    };

    let index_area: AnyV = if playing {
        xilem::view::sized_box(label("")).width(px(32.0)).boxed()
    } else {
        xilem::view::sized_box(small(format!("{}", index + 1)).color(MUTED_FG))
            .width(px(32.0))
            .boxed()
    };
    let overlay = if playing {
        Overlay::Bars(((z.frame / 3) % 4) as u8)
    } else {
        Overlay::None
    };

    let title_area: AnyV = if explicit {
        flex_row((
            body_med(title)
                .color(FG)
                .line_break_mode(LineBreaking::Clip)
                .into_any_flex(),
            xilem::view::sized_box(txt("E", 11.0, 500, MUTED_FG))
                .width(px(16.0))
                .height(px(14.0))
                .background_color(CARD)
                .corner_radius(2.0)
                .into_any_flex(),
        ))
        .gap(px(6.0))
        .cross_axis_alignment(CrossAxisAlignment::Center)
        .boxed()
    } else {
        body_med(title)
            .color(FG)
            .line_break_mode(LineBreaking::Clip)
            .boxed()
    };

    let right: AnyV = if liked {
        xilem::view::sized_box(glyph(Icon::HeartFilled, 14.0, PRIMARY))
            .width(px(28.0))
            .height(px(20.0))
            .boxed()
    } else {
        xilem::view::sized_box(small(dur)).width(px(48.0)).height(px(20.0)).boxed()
    };

    let like_glyph = if liked {
        glyph(Icon::HeartFilled, 14.0, PRIMARY)
    } else {
        glyph(Icon::Heart, 14.0, MUTED_FG)
    };

    let row = flex_row((
        index_area.into_any_flex(),
        xilem::view::sized_box(image(art).fit(ObjectFit::Fill))
            .width(px(t::ROW_ART))
            .height(px(t::ROW_ART))
            .corner_radius(t::RADIUS_ART as f64)
            .into_any_flex(),
        flex_col((
            title_area.into_any_flex(),
            small(artist).line_break_mode(LineBreaking::Clip).into_any_flex(),
        ))
        .gap(Length::ZERO)
        .cross_axis_alignment(CrossAxisAlignment::Fill)
        .flex(1.0)
        .into_any_flex(),
        right.into_any_flex(),
        hot(
            xilem::view::sized_box(like_glyph).width(px(22.0)).height(px(22.0)),
            move |z: &mut Zuno| {
                z.app.toggle_like(id);
            },
        )
        .prop(widgets::HoveredBackground(BgProp::Color(CLEAR)))
        .corner_radius(t::RADIUS_ROW as f64)
        .into_any_flex(),
    ))
    .gap(px(10.0))
    .cross_axis_alignment(CrossAxisAlignment::Center);

    hot(row, on_play)
        .overlay(overlay)
        .prop(widgets::HoveredBackground(BgProp::Color(row_hover())))
        .prop(BgProp::Color(bg))
        .corner_radius(t::RADIUS_ROW as f64)
        .padding(Padding::from_vh(6.0, 8.0))
        .boxed()
}

fn artist_row(z: &mut Zuno, id: ArtistId) -> AnyV {
    let r = z.app.library.artist(id);
    let name = r.name.clone();
    let n_albums = r.album_ids.len();
    let art_url = r.artwork_url.clone();
    let art = z.cover(art_url.as_deref(), AppState::artist_seed(id), t::ROW_ART as u32, true);
    hot(
        flex_row((
            xilem::view::sized_box(image(art).fit(ObjectFit::Fill))
                .width(px(t::ROW_ART))
                .height(px(t::ROW_ART))
                .into_any_flex(),
            flex_col((
                body_med(name).line_break_mode(LineBreaking::Clip).into_any_flex(),
                small(format!("{n_albums} albums")).into_any_flex(),
            ))
            .gap(Length::ZERO)
            .cross_axis_alignment(CrossAxisAlignment::Fill)
            .flex(1.0)
            .into_any_flex(),
        ))
        .gap(px(10.0))
        .cross_axis_alignment(CrossAxisAlignment::Center),
        move |z: &mut Zuno| {
            z.app.open_artist(id);
        },
    )
    .prop(widgets::HoveredBackground(BgProp::Color(row_hover())))
    .corner_radius(t::RADIUS_ROW as f64)
    .padding(Padding::from_vh(6.0, 8.0))
    .boxed()
}

// — Detail pages ——————————————————————————————————————————————————————————

fn album_page(z: &mut Zuno, id: AlbumId) -> AnyV {
    let a = z.app.library.album(id);
    let title = a.title.clone();
    let artist = z.app.library.artist(a.artist_id).name.clone();
    let year = a.year;
    let kind = a.kind.label();
    let meta = counts_line(a.track_ids.iter().map(|&tid| z.app.library.track(tid).duration_sec));
    let ids: Vec<TrackId> = a.track_ids.clone();
    let art_url = a.artwork_url.clone();
    let art = z.cover(art_url.as_deref(), AppState::album_seed(id), 232, false);

    flex_col((
        media_header(z, art, title, format!("{kind} · {artist} · {year}"), meta).into_any_flex(),
        track_list(z, &ids).into_any_flex(),
    ))
    .gap(px(8.0))
    .cross_axis_alignment(CrossAxisAlignment::Fill)
    .padding(Padding::from_vh(0.0, t::PAGE_PAD as f64))
    .boxed()
}

fn playlist_page(z: &mut Zuno, id: PlaylistId) -> AnyV {
    let p = z.app.library.playlist(id).unwrap();
    let title = p.title.clone();
    let desc = p.description.clone().unwrap_or_default();
    let ids: Vec<TrackId> = p.track_ids.clone();
    let meta = counts_line(ids.iter().map(|&tid| z.app.library.track(tid).duration_sec));
    let art_url = p.artwork_url.clone();
    let art = z.cover(art_url.as_deref(), AppState::playlist_seed(id), 232, false);

    flex_col((
        media_header(z, art, title, desc, meta).into_any_flex(),
        track_list(z, &ids).into_any_flex(),
    ))
    .gap(px(8.0))
    .cross_axis_alignment(CrossAxisAlignment::Fill)
    .padding(Padding::from_vh(0.0, t::PAGE_PAD as f64))
    .boxed()
}

fn artist_page(z: &mut Zuno, id: ArtistId) -> AnyV {
    let r = z.app.library.artist(id);
    let name = r.name.clone();
    let listeners_line = listeners(r.monthly_listeners);
    let top: Vec<TrackId> = r
        .album_ids
        .iter()
        .flat_map(|&aid| z.app.library.album(aid).track_ids.iter().copied())
        .take(10)
        .collect();
    let album_ids: Vec<AlbumId> = r.album_ids.clone();
    let art_url = r.artwork_url.clone();
    let art = z.cover(art_url.as_deref(), AppState::artist_seed(id), 232, true);

    flex_col((
        media_header(z, art, name.clone(), listeners_line, format!("About {name}"))
            .into_any_flex(),
        h3("Top tracks").into_any_flex(),
        track_list(z, &top).into_any_flex(),
        h3("Albums").into_any_flex(),
        albums_grid(z, &album_ids).into_any_flex(),
    ))
    .gap(px(t::PAGE_PAD))
    .cross_axis_alignment(CrossAxisAlignment::Fill)
    .padding(Padding::from_vh(0.0, t::PAGE_PAD as f64))
    .boxed()
}

/// 232px artwork + h1 + meta + Play/Shuffle pills.
fn media_header(
    z: &mut Zuno,
    art: ImageBrush,
    title: String,
    meta: String,
    counts: String,
) -> impl WidgetView<Zuno> + use<> {
    let play_ids = z.app.list_ids();
    flex_row((
        xilem::view::sized_box(image(art).fit(ObjectFit::Fill))
            .width(px(232.0))
            .height(px(232.0))
            .corner_radius(t::RADIUS_ART as f64)
            .into_any_flex(),
        flex_col((
            h1(title).into_any_flex(),
            small(meta).into_any_flex(),
            small(counts).into_any_flex(),
            FlexSpacer::Fixed(px(12.0)),
            flex_row((
                hot(
                    flex_row((
                        glyph(Icon::Play, 16.0, FG).into_any_flex(),
                        body_med("Play").color(FG).into_any_flex(),
                    ))
                    .gap(px(8.0))
                    .cross_axis_alignment(CrossAxisAlignment::Center),
                    move |z: &mut Zuno| {
                        let ids = play_ids.clone();
                        if !ids.is_empty() {
                            z.app.play_from(&ids, 0);
                        }
                    },
                )
                .prop(BgProp::Color(PRIMARY))
                .prop(widgets::HoveredBackground(BgProp::Color(PRIMARY_HOVER)))
                .corner_radius(999.0)
                .padding(Padding::from_vh(10.0, 24.0))
                .into_any_flex(),
                hot(
                    body_med("Shuffle").color(FG),
                    |z: &mut Zuno| {
                        z.app.toggle_shuffle();
                        let ids = z.app.list_ids();
                        if !ids.is_empty() {
                            z.app.play_from(&ids, 0);
                        }
                    },
                )
                .prop(BgProp::Color(CLEAR))
                .prop(widgets::HoveredBackground(BgProp::Color(row_hover())))
                .corner_radius(999.0)
                .border(MUTED_FG, 1.0)
                .padding(Padding::from_vh(10.0, 24.0))
                .into_any_flex(),
            ))
            .gap(px(10.0))
            .cross_axis_alignment(CrossAxisAlignment::Center)
            .into_any_flex(),
        ))
        .gap(px(4.0))
        .cross_axis_alignment(CrossAxisAlignment::Fill)
        .flex(1.0)
        .into_any_flex(),
    ))
    .gap(px(24.0))
    .cross_axis_alignment(CrossAxisAlignment::End)
}

/// The track list on an album/playlist/artist/search page.
fn track_list(z: &mut Zuno, ids: &[TrackId]) -> impl WidgetView<Zuno> + use<> {
    let rows: Vec<_> = ids
        .iter()
        .enumerate()
        .map(|(i, &id)| {
            xilem::view::flex_item(track_row(z, i, id, play_from_current(id)), FlexParams::default())
        })
        .collect();
    flex_col(rows)
        .gap(Length::ZERO)
        .cross_axis_alignment(CrossAxisAlignment::Fill)
}

// — Search ———————————————————————————————————————————————————————————————

fn search_page(z: &mut Zuno) -> AnyV {
    let q = z.app.search_query.clone();
    if q.trim().is_empty() {
        return flex_col(vec![
            h1("Search").boxed(),
            body("Search across 5,000 tracks, albums and artists.").color(MUTED_FG).boxed(),
        ])
        .gap(px(8.0))
        .cross_axis_alignment(CrossAxisAlignment::Fill)
        .padding(Padding::from_vh(0.0, t::PAGE_PAD as f64))
        .boxed();
    }
    let results = search::search(&z.app.library, &q);
    let top_id = results.tracks.first().copied();
    let tracks: Vec<TrackId> = results.tracks.iter().take(50).copied().collect();
    let albums: Vec<AlbumId> = results.albums.iter().take(12).copied().collect();
    let artists: Vec<ArtistId> = results.artists.iter().take(12).copied().collect();

    let top_result: AnyV = if let Some(id) = top_id {
        let tv = z.app.library.track_view(id);
        let title: ArcStr = tv.title.into();
        let artist: ArcStr = tv.artist.into();
        let art_url = z.app.library.track(id).artwork_url.clone();
        let art = z.cover(art_url.as_deref(), AppState::album_seed(z.app.library.track(id).album_id), 120, false);
        xilem::view::sized_box(flex_col((
            h2("Top result").into_any_flex(),
            xilem::view::sized_box(image(art).fit(ObjectFit::Fill))
                .width(px(120.0))
                .height(px(120.0))
                .corner_radius(t::RADIUS_ART as f64)
                .into_any_flex(),
            body_med(title).into_any_flex(),
            small(format!("Song · {artist}")).into_any_flex(),
        ))
        .gap(px(8.0))
        .cross_axis_alignment(CrossAxisAlignment::Fill))
        .width(px(240.0))
        .boxed()
    } else {
        label("").boxed()
    };

    flex_col((
        h1(format!("Search \"{q}\"")).into_any_flex(),
        flex_row((
            top_result.into_any_flex(),
            section("Songs", track_list(z, &tracks)).flex(fill()).into_any_flex(),
        ))
        .gap(px(24.0))
        .cross_axis_alignment(CrossAxisAlignment::Start)
        .into_any_flex(),
        section("Albums", album_shelf(z, &albums)).into_any_flex(),
        section("Artists", artist_shelf(z, &artists)).into_any_flex(),
    ))
    .gap(px(t::SECTION_GAP))
    .cross_axis_alignment(CrossAxisAlignment::Fill)
    .padding(Padding::from_vh(0.0, t::PAGE_PAD as f64))
    .boxed()
}

// — Settings ——————————————————————————————————————————————————————————————

fn settings_page(z: &mut Zuno) -> AnyV {
    let autostart = z.settings_autostart;
    let explicit = z.settings_explicit;
    flex_col((
        h1("Settings").into_any_flex(),
        setting_row("Theme", "Dark (pinned)").into_any_flex(),
        setting_row("Audio quality", "High (Opus 160k)").into_any_flex(),
        setting_row("Gapless playback", "On").into_any_flex(),
        setting_row("Crossfade", "Off").into_any_flex(),
        setting_row("Normalize volume", "Off").into_any_flex(),
        checkbox("Launch on startup", autostart, move |z: &mut Zuno, v: bool| {
            z.settings_autostart = v;
        })
        .into_any_flex(),
        checkbox("Show explicit content", explicit, move |z: &mut Zuno, v: bool| {
            z.settings_explicit = v;
        })
        .into_any_flex(),
        small("Zuno native benchmark — rendered, not wired.").into_any_flex(),
        h3("YouTube Music").into_any_flex(),
        text_input(z.cookie.clone(), |z: &mut Zuno, q: String| {
            z.cookie = q;
        })
        .placeholder("Paste your music.youtube.com Cookie: header…")
        .text_color(FG)
        .prop(masonry::properties::PlaceholderColor::new(MUTED_FG))
        .into_any_flex(),
        hot(
            body_med("Sign in with cookie").color(FG),
            move |z: &mut Zuno| {
                if let Some(tx) = &z.yt_tx {
                    let header = z.cookie.trim().to_string();
                    if !header.is_empty() {
                        let _ = tx.send(YtReq::SignIn { header });
                        z.yt_status = "Signing in…".to_string();
                    }
                }
            },
        )
        .into_any_flex(),
        small(if z.yt_status.is_empty() { "Unsigned — public content only.".to_string() } else { z.yt_status.clone() }).into_any_flex(),
    ))
    .gap(px(10.0))
    .cross_axis_alignment(CrossAxisAlignment::Fill)
    .padding(Padding::from_vh(0.0, t::PAGE_PAD as f64))
    .boxed()
}

fn setting_row(name: &str, value: &str) -> impl WidgetView<Zuno> + use<> {
    flex_row((body(name).flex(1.0).into_any_flex(), small(value).into_any_flex()))
        .gap(px(16.0))
        .cross_axis_alignment(CrossAxisAlignment::Center)
        .padding(Padding::from_vh(8.0, 12.0))
        .background_color(POPOVER)
        .corner_radius(t::RADIUS_ROW as f64)
}

// — Queue panel ———————————————————————————————————————————————————————————

fn queue_panel(z: &mut Zuno) -> AnyV {
    let rows = z.app.queue_rows();
    let mut items: Vec<AnyV> = Vec::new();
    items.push(xilem::view::sized_box(small_med("NOW PLAYING")).padding(Padding::from_vh(12.0, 12.0)).boxed());
    for (i, row) in rows.iter().enumerate() {
        if row.1 == Region::Played || row.1 == Region::Current {
            items.push(queue_row(z, row.0, row.1, i));
        }
    }
    let manual: Vec<(usize, (TrackId, Region))> = rows
        .iter()
        .enumerate()
        .filter(|(_, r)| r.1 == Region::Manual)
        .map(|(i, r)| (i, *r))
        .collect();
    if !manual.is_empty() {
        items.push(xilem::view::sized_box(small_med("NEXT IN QUEUE")).padding(Padding::from_vh(12.0, 12.0)).boxed());
        for (i, row) in manual {
            items.push(queue_row(z, row.0, row.1, i));
        }
    }
    let auto: Vec<(usize, (TrackId, Region))> = rows
        .iter()
        .enumerate()
        .filter(|(_, r)| r.1 == Region::Automatic)
        .map(|(i, r)| (i, *r))
        .collect();
    if !auto.is_empty() {
        items.push(xilem::view::sized_box(small_med("NEXT UP")).padding(Padding::from_vh(12.0, 12.0)).boxed());
        for (i, row) in auto {
            items.push(queue_row(z, row.0, row.1, i));
        }
    }
    if rows.is_empty() {
        items.push(small("Queue is empty").boxed());
    }
    let list = portal(flex_col(items).gap(px(2.0)).cross_axis_alignment(CrossAxisAlignment::Fill));
    xilem::view::sized_box(list)
        .width(px(t::QUEUE_W))
        .expand_height()
        .background_color(POPOVER)
        .boxed()
}

/// One queue row: artwork, title/artist, ✕ remove; clicking jumps.
fn queue_row(z: &mut Zuno, id: TrackId, region: Region, flat_index: usize) -> AnyV {
    let tv = z.app.library.track_view(id);
    let title: ArcStr = tv.title.into();
    let artist: ArcStr = tv.artist.into();
    let playing = z.app.current == Some(id);
    let is_current = region == Region::Current;
    let album_seed = AppState::album_seed(z.app.library.track(id).album_id);
    let art_url = z.app.library.track(id).artwork_url.clone();
    let art = z.cover(art_url.as_deref(), album_seed, t::ROW_ART as u32, false);
    let idx = flat_index;
    let row = flex_row((
        xilem::view::sized_box(image(art).fit(ObjectFit::Fill))
            .width(px(t::ROW_ART))
            .height(px(t::ROW_ART))
            .corner_radius(t::RADIUS_ART as f64)
            .into_any_flex(),
        flex_col((
            body_med(title)
                .color(if is_current { PRIMARY } else { FG })
                .line_break_mode(LineBreaking::Clip)
                .into_any_flex(),
            small(artist).line_break_mode(LineBreaking::Clip).into_any_flex(),
        ))
        .gap(Length::ZERO)
        .cross_axis_alignment(CrossAxisAlignment::Fill)
        .flex(1.0)
        .into_any_flex(),
        hot(
            xilem::view::sized_box(glyph(Icon::XClose, 12.0, MUTED_FG))
                .width(px(22.0))
                .height(px(22.0)),
            move |z: &mut Zuno| {
                z.app.queue.remove_at(idx);
            },
        )
        .prop(widgets::HoveredBackground(BgProp::Color(CLEAR)))
        .corner_radius(t::RADIUS_ROW as f64)
        .into_any_flex(),
    ))
    .gap(px(10.0))
    .cross_axis_alignment(CrossAxisAlignment::Center);

    let jump_idx = flat_index;
    hot(
        row,
        move |z: &mut Zuno| {
            if let Some(id) = z.app.queue.jump_to(jump_idx) {
                z.app.play_track(id);
            }
        },
    )
    .prop(if playing { BgProp::Color(row_playing()) } else { BgProp::Color(CLEAR) })
    .prop(widgets::HoveredBackground(BgProp::Color(row_hover())))
    .corner_radius(t::RADIUS_ROW as f64)
    .padding(Padding::from_vh(6.0, 8.0))
    .boxed()
}

// — Player bar ———————————————————————————————————————————————————————————

fn player_bar(z: &mut Zuno) -> impl WidgetView<Zuno> + use<> {
    let current = z.app.current_track().cloned();
    let playing = z.app.playing;
    let shuffle_on = z.app.shuffle;
    let repeat = z.app.repeat;
    let muted = z.app.muted;
    let queue_open = z.queue_open;
    let position = z.app.player.position_sec();
    let duration = z.app.player.duration_sec();

    let (art, title, artist, album_id, track_id, liked) = match &current {
        Some(track) => (
            Some(z.cover(track.artwork_url.clone().as_deref(), AppState::album_seed(track.album_id), t::ROW_ART as u32, false)),
            Some(track.title.clone()),
            Some(z.app.library.artist(track.artist_id).name.clone()),
            Some(track.album_id),
            Some(track.id),
            track.liked,
        ),
        None => (None, None, None, None, None, false),
    };

    // Now-playing area (left)
    let now_playing: AnyV = match (art, title, artist, album_id) {
        (Some(art), Some(title), Some(artist), Some(album_id)) => {
            let t_title: ArcStr = title.into();
            let t_artist: ArcStr = artist.into();
            flex_row((
                hot(
                    xilem::view::sized_box(image(art).fit(ObjectFit::Fill))
                        .width(px(t::ROW_ART))
                        .height(px(t::ROW_ART))
                        .corner_radius(t::RADIUS_ART as f64),
                    move |z: &mut Zuno| {
                        z.app.open_album(album_id);
                    },
                )
                .into_any_flex(),
                xilem::view::sized_box(flex_col((
                    body_med(t_title).line_break_mode(LineBreaking::Clip).into_any_flex(),
                    small(t_artist).line_break_mode(LineBreaking::Clip).into_any_flex(),
                ))
                .gap(Length::ZERO)
                .cross_axis_alignment(CrossAxisAlignment::Fill))
                .width(px(180.0))
                .into_any_flex(),
            ))
            .gap(px(10.0))
            .cross_axis_alignment(CrossAxisAlignment::Center)
            .boxed()
        }
        _ => flex_row((
            xilem::view::sized_box(label(""))
                .width(px(t::ROW_ART))
                .height(px(t::ROW_ART))
                .into_any_flex(),
            xilem::view::sized_box(flex_col((
                body_med("Nothing playing").color(MUTED_FG).line_break_mode(LineBreaking::Clip).into_any_flex(),
                small("—").into_any_flex(),
            ))
            .gap(Length::ZERO)
            .cross_axis_alignment(CrossAxisAlignment::Fill))
            .width(px(180.0))
            .into_any_flex(),
        ))
        .gap(px(10.0))
        .cross_axis_alignment(CrossAxisAlignment::Center)
        .boxed(),
    };

    let play_button = hot(
        xilem::view::sized_box(if playing {
            glyph(Icon::Pause, 20.0, FG)
        } else {
            glyph(Icon::Play, 20.0, FG)
        })
        .width(px(40.0))
        .height(px(40.0)),
        |z: &mut Zuno| {
            z.app.toggle_play();
        },
    )
    .prop(BgProp::Color(PRIMARY))
    .prop(widgets::HoveredBackground(BgProp::Color(PRIMARY_HOVER)))
    .corner_radius(999.0);

    let seek_max = if duration > 0.1 { duration } else { 1.0 };
    let seek_pos = position.clamp(0.0, seek_max);
    let has_track = current.is_some();
    let seek = slider(0.0, seek_max, seek_pos, |z: &mut Zuno, v: f64| {
        z.app.seek(v);
    })
    .disabled(!has_track)
    .prop(BgProp::Color(rgb(t::DARK.muted)))
    .prop(BarColor(PRIMARY))
    .flex(1.0);

    let elapsed = mmss(position.max(0.0) as u32);
    let total = mmss(duration.max(0.0) as u32);

    let like_area: AnyV = match track_id {
        Some(id) => hot(
            xilem::view::sized_box(if liked {
                glyph(Icon::HeartFilled, 16.0, PRIMARY)
            } else {
                glyph(Icon::Heart, 16.0, MUTED_FG)
            })
            .width(px(28.0))
            .height(px(28.0)),
            move |z: &mut Zuno| {
                z.app.toggle_like(id);
            },
        )
        .prop(widgets::HoveredBackground(BgProp::Color(CLEAR)))
        .corner_radius(t::RADIUS_ROW as f64)
        .boxed(),
        None => xilem::view::sized_box(glyph(Icon::Heart, 16.0, Color::from_rgba8(0xa1, 0xa1, 0xa1, 0x40)))
            .width(px(28.0))
            .height(px(28.0))
            .boxed(),
    };

    flex_row((
        now_playing.into_any_flex(),
        // Transport + seek (center)
        flex_col((
            flex_row((
                hot(
                    xilem::view::sized_box(if shuffle_on {
                        glyph(Icon::Shuffle, 16.0, PRIMARY)
                    } else {
                        glyph(Icon::Shuffle, 16.0, MUTED_FG)
                    })
                    .width(px(28.0))
                    .height(px(28.0)),
                    |z: &mut Zuno| {
                        z.app.toggle_shuffle();
                    },
                )
                .prop(widgets::HoveredBackground(BgProp::Color(CLEAR)))
                .corner_radius(t::RADIUS_ROW as f64)
                .into_any_flex(),
                hot(
                    xilem::view::sized_box(glyph(Icon::Prev, 18.0, FG))
                        .width(px(28.0))
                        .height(px(28.0)),
                    |z: &mut Zuno| {
                        z.app.previous();
                    },
                )
                .prop(widgets::HoveredBackground(BgProp::Color(CLEAR)))
                .corner_radius(t::RADIUS_ROW as f64)
                .into_any_flex(),
                play_button.into_any_flex(),
                hot(
                    xilem::view::sized_box(glyph(Icon::Next, 18.0, FG))
                        .width(px(28.0))
                        .height(px(28.0)),
                    |z: &mut Zuno| {
                        z.app.next();
                    },
                )
                .prop(widgets::HoveredBackground(BgProp::Color(CLEAR)))
                .corner_radius(t::RADIUS_ROW as f64)
                .into_any_flex(),
                hot(
                    xilem::view::sized_box(match repeat {
                        RepeatMode::Off => glyph(Icon::Repeat, 16.0, MUTED_FG),
                        RepeatMode::All => glyph(Icon::Repeat, 16.0, PRIMARY),
                        RepeatMode::One => glyph(Icon::RepeatOne, 16.0, PRIMARY),
                    })
                    .width(px(28.0))
                    .height(px(28.0)),
                    |z: &mut Zuno| {
                        z.app.cycle_repeat();
                    },
                )
                .prop(widgets::HoveredBackground(BgProp::Color(CLEAR)))
                .corner_radius(t::RADIUS_ROW as f64)
                .into_any_flex(),
            ))
            .gap(px(6.0))
            .main_axis_alignment(MainAxisAlignment::Center)
            .cross_axis_alignment(CrossAxisAlignment::Center)
            .into_any_flex(),
            flex_row((
                small(elapsed).into_any_flex(),
                seek.into_any_flex(),
                small(total).into_any_flex(),
            ))
            .gap(px(8.0))
            .cross_axis_alignment(CrossAxisAlignment::Center)
            .into_any_flex(),
        ))
        .gap(px(4.0))
        .cross_axis_alignment(CrossAxisAlignment::Fill)
        .flex(1.0)
        .into_any_flex(),
        // Right: like, queue toggle, volume
        like_area.into_any_flex(),
        hot(
            xilem::view::sized_box(if queue_open {
                glyph(Icon::QueueList, 16.0, PRIMARY)
            } else {
                glyph(Icon::QueueList, 16.0, MUTED_FG)
            })
            .width(px(28.0))
            .height(px(28.0)),
            |z: &mut Zuno| {
                z.queue_open = !z.queue_open;
            },
        )
        .prop(widgets::HoveredBackground(BgProp::Color(CLEAR)))
        .corner_radius(t::RADIUS_ROW as f64)
        .into_any_flex(),
        flex_row((
            hot(
                xilem::view::sized_box(if muted {
                    glyph(Icon::VolumeMute, 16.0, MUTED_FG)
                } else {
                    glyph(Icon::VolumeHigh, 16.0, MUTED_FG)
                })
                .width(px(24.0))
                .height(px(24.0)),
                |z: &mut Zuno| {
                    z.app.toggle_mute();
                },
            )
            .prop(widgets::HoveredBackground(BgProp::Color(CLEAR)))
            .corner_radius(t::RADIUS_ROW as f64)
            .into_any_flex(),
            xilem::view::sized_box(slider(0.0, 1.0, z.app.volume as f64, |z: &mut Zuno, v: f64| {
                z.app.set_volume(v as f32);
            }))
            .width(px(80.0))
            .into_any_flex(),
        ))
        .gap(px(2.0))
        .cross_axis_alignment(CrossAxisAlignment::Center)
        .into_any_flex(),
    ))
    .gap(px(16.0))
    .cross_axis_alignment(CrossAxisAlignment::Center)
    .padding(Padding::from_vh(10.0, 16.0))
}
