//! Zuno — freya implementation of the native-GUI benchmark (gui/freya branch).
//!
//! Freya 0.5.0-rc.9. Renders the shared `zuno_core::AppState`
//! per docs/gui-benchmarks/SPEC.md. The 5,000-track Library ▸ Songs list uses
//! freya's lazy `VirtualScrollView`; everything else is plain elements.
//!
//! Real YouTube Music loads through `mod yt` (freya-native async tasks, no
//! worker thread); `--bench` never initializes it — the synthetic benchmark
//! path is untouched.

mod yt;

use freya::elements::image::ImageHandle;
use freya::prelude::*;
use freya_engine::prelude::AlphaType;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use zuno_core::bench::{BenchAction, BenchDriver};
use zuno_core::format::{counts_line, count_songs, listeners, mmss};
use zuno_core::queue::Region;
use zuno_core::search;
use zuno_core::theme::{
    self, BODY, BODY_MED, CARD_H, CARD_W, DARK, H1, H2, H3, PLAYER_BAR_H, QUEUE_W, Rgb, SIDEBAR_W,
    SMALL, TITLEBAR_H, XS,
};
use zuno_core::app::{LibraryTab, RepeatMode};
use zuno_core::{AppState, TrackId, View};

// ---------------------------------------------------------------------------
// Small helpers
// ---------------------------------------------------------------------------

fn c(v: Rgb) -> (u8, u8, u8) {
    (v.0[0], v.0[1], v.0[2])
}

fn bg() -> (u8, u8, u8) {
    c(DARK.background)
}
fn fg() -> (u8, u8, u8) {
    c(DARK.foreground)
}
fn muted_fg() -> (u8, u8, u8) {
    c(DARK.muted_foreground)
}
fn card() -> (u8, u8, u8) {
    c(DARK.card)
}
fn primary() -> (u8, u8, u8) {
    c(DARK.primary)
}

/// Skia image-handle cache keyed by (artwork seed, size). The decoded RGBA
/// bytes stay budgeted in core's shared 96-cover LRU; this only caches the
/// uploaded `SkImage` wrappers so a re-render doesn't re-upload.
static IMGS: OnceLock<Mutex<HashMap<(u64, u32), ImageHandle>>> = OnceLock::new();

fn imgs() -> &'static Mutex<HashMap<(u64, u32), ImageHandle>> {
    IMGS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Artwork source: `Some(url)` = remote YouTube cover (procedural while the
/// fetch is in flight), `None` = the benchmark's procedural seed. The
/// `--bench` library carries no URLs, so its path is byte-identical.
fn art(app: &AppState, url: Option<&str>, seed: u64, size: u32) -> Option<ImageHandle> {
    let key = match url {
        Some(u) if !u.is_empty() => (yt::url_hash(u), size),
        _ => (seed, size),
    };
    if let Some(h) = imgs().lock().unwrap().get(&key) {
        return Some(h.clone());
    }
    let bytes = match url {
        Some(u) if !u.is_empty() && yt::cover_ready(u) => app.artwork.get_url(u, size),
        _ => app.artwork.get(seed, size),
    };
    let h = ImageHandle::from_rgba(size, size, Bytes::copy_from_slice(&bytes), AlphaType::Opaque)?;
    imgs().lock().unwrap().insert(key, h.clone());
    Some(h)
}

/// Drop the placeholder handles for a URL whose real bytes just landed, so
/// the next frame uploads the fetched cover instead of the procedural one.
pub(crate) fn imgs_purge_url(url: &str) {
    let key = yt::url_hash(url);
    let mut m = imgs().lock().unwrap();
    for size in yt::COVER_SIZES {
        m.remove(&(key, size));
    }
}

/// Artwork tile with graceful fallback to a flat card-color tile.
fn tile(app: State<AppState>, url: Option<&str>, seed: u64, size: f32, radius: f32) -> Element {
    let pa = app.peek();
    let px = size as u32;
    match art(&pa, url, seed, px) {
        Some(h) => image(h)
            .width(Size::px(size))
            .height(Size::px(size))
            .corner_radius(radius)
            .into(),
        None => rect()
            .width(Size::px(size))
            .height(Size::px(size))
            .background(card())
            .corner_radius(radius)
            .into(),
    }
}

fn h1(s: String) -> Element {
    label()
        .text(s)
        .font_size(H1.size)
        .font_weight(H1.weight as i32)
        .color(fg())
        .into()
}

fn h2(s: String) -> Element {
    label()
        .text(s)
        .font_size(H2.size)
        .font_weight(H2.weight as i32)
        .color(fg())
        .into()
}

fn h3(s: String) -> Element {
    label()
        .text(s)
        .font_size(H3.size)
        .font_weight(H3.weight as i32)
        .color(fg())
        .into()
}

fn body(s: String) -> Element {
    label()
        .text(s)
        .font_size(BODY.size)
        .font_weight(BODY.weight as i32)
        .color(fg())
        .max_lines(1)
        .into()
}

fn small_muted(s: String) -> Element {
    label()
        .text(s)
        .font_size(SMALL.size)
        .font_weight(SMALL.weight as i32)
        .color(muted_fg())
        .max_lines(1)
        .into()
}

fn set_hover(ui: State<Ui>, id: &str) {
    if ui.peek().hover.as_deref() != Some(id) {
        ui.write_unchecked().hover = Some(id.to_string());
    }
}

fn clear_hover(ui: State<Ui>, id: &str) {
    if ui.peek().hover.as_deref() == Some(id) {
        ui.write_unchecked().hover = None;
    }
}

// ---------------------------------------------------------------------------
// UI-only state (everything else lives in zuno_core::AppState)
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct Ui {
    queue_open: bool,
    hover: Option<String>,
    frame: u64,
    sett_hq: bool,
    sett_norm: bool,
    // — Real-YouTube wiring (all default/empty in --bench) —
    yt_status: String,
    /// Track a stream resolve is in flight for.
    resolving: Option<TrackId>,
    /// Track currently served by `play_stream` (or synth-fallback) — stops
    /// the per-frame resolve loop from refiring.
    stream_for: Option<TrackId>,
    /// Whether the live library is search results (vs home).
    lib_is_search: bool,
    /// Sequence number for stale search arrivals.
    search_rev: u64,
    /// Last query fanned out to the debounced search.
    search_sent: String,
}

impl Default for Ui {
    fn default() -> Self {
        Self {
            queue_open: false,
            hover: None,
            frame: 0,
            sett_hq: true,
            sett_norm: false,
            yt_status: String::new(),
            resolving: None,
            stream_for: None,
            lib_is_search: false,
            search_rev: 0,
            search_sent: String::new(),
        }
    }
}

/// Owned snapshot identifying the Songs VSV content. `frame` forces visible
/// rows to rebuild every frame (playhead bars animate, selection is fresh).
#[derive(Clone, PartialEq)]
struct SongsData {
    ids: Vec<TrackId>,
    sel: Option<usize>,
    cur: Option<TrackId>,
    hov: Option<String>,
    frame: u64,
}

// ---------------------------------------------------------------------------
// App
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct Zuno {
    app: State<AppState>,
    ui: State<Ui>,
    search: State<String>,
    cookie: State<String>,
}

fn main() {
    let bench_on = std::env::args().any(|a| a == "--bench");
    let songs_view = std::env::args().any(|a| a == "--screenshot-songs");
    let mut state = AppState::new();
    if songs_view || bench_on {
        // The bench drives Library ▸ Songs from 8 s in; parking there from
        // the start means the 5,000-row surface is laid out before RecordStartup.
        state.view = View::Library;
        state.library_tab = LibraryTab::Songs;
    }
    let app = State::create_global(state);
    let ui = State::create_global(Ui::default());
    let search = State::create_global(String::new());
    let cookie = State::create_global(String::new());

    // Real-YouTube mode (skipped entirely under --bench: no client, no
    // threads, no runtime — the synthetic path stays as-is). The multi-thread
    // runtime exists only to drive async reqwest sockets; freya tasks await
    // the POSTs on the UI thread while its driver threads do the wakeups.
    let mut yt_rt = None;
    if !bench_on && yt::init() {
        match tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
        {
            Ok(rt) => yt_rt = Some(rt),
            Err(e) => eprintln!("[yt] no async runtime: {e}"),
        }
    }
    let _yt_enter = yt_rt.as_ref().map(|rt| rt.enter());

    launch(
        LaunchConfig::new()
            .with_font("Inter", Bytes::from_static(include_bytes!("../assets/Inter-Variable.ttf")))
            .with_default_font("Inter")
            .with_window(
                WindowConfig::new_app(Zuno { app, ui, search, cookie })
                    .with_title("Zuno")
                    .with_size(1280.0, 800.0)
                    .with_min_size(900.0, 600.0)
                    .with_decorations(false)
                    .with_transparency(true),
            ),
    );
}

impl App for Zuno {
    fn render(&self) -> impl IntoElement {
        // Subscribe: any AppState/Ui write re-renders the tree.
        let _sub_a = self.app.read();
        let _sub_u = self.ui.read();
        // Scroll controller for the virtualized Songs list. Created here
        // (inside Freya context) — `ScrollController::new` panics in `main`.
        let songs = use_scroll_controller(ScrollConfig::default);

        // Per-frame driver: audio auto-advance, search sync, bench schedule,
        // plus the YouTube bridge tick (search fan-out, stream resolve).
        use_hook(|| {
            let app = self.app;
            let ui = self.ui;
            let search = self.search;
            spawn_forever(async move {
                let ticker = RenderingTicker::get();
                let platform = Platform::get();
                let mut bench: Option<BenchDriver> = None;
                let bench_on = std::env::args().any(|a| a == "--bench");
                let mut frames: u64 = 0;
                loop {
                    frames += 1;
                    {
                        let mut a = app.write_unchecked();
                        a.tick();
                        if a.view == View::Search {
                            let q = search.peek().clone();
                            if q != a.search_query {
                                a.set_search(&q);
                            }
                        }
                        if bench_on {
                            if bench.is_none() && frames > 2 {
                                bench = Some(BenchDriver::new());
                            }
                            if let Some(b) = bench.as_mut() {
                                match b.tick() {
                                    BenchAction::RecordStartup => {}
                                    BenchAction::ScrollTo(f) => {
                                        a.view = View::Library;
                                        a.library_tab = LibraryTab::Songs;
                                        let mut s = songs;
                                        s.scroll_to_y(-((f * 259_000.0) as i32));
                                    }
                                    BenchAction::SelectRow(i) => {
                                        a.selection = Some(i);
                                    }
                                    BenchAction::StartPlayback => {
                                        let ids = a.list_ids();
                                        if !ids.is_empty() {
                                            a.play_from(&ids, ids.len() / 2);
                                        }
                                    }
                                    BenchAction::Finish => {
                                        println!("{}", b.report);
                                        std::process::exit(0);
                                    }
                                    BenchAction::Nothing => {}
                                }
                            }
                        }
                    }
                    yt::frame_tick(&app, &ui, &search);
                    ui.write_unchecked().frame = frames;
                    platform.send(UserEvent::RequestRedraw);
                    ticker.tick().await;
                }
            });
            // First paint is the instant synthetic library; this swaps in the
            // real catalog over the network a few seconds later.
            if yt::enabled() {
                let app = self.app;
                let ui = self.ui;
                spawn(yt::load_home(app.clone(), ui.clone()));
            }
        });

        let app = self.app;
        let ui = self.ui;
        let queue_open = self.ui.peek().queue_open;
        let mut mid = rect()
            .width(Size::fill())
            .height(Size::flex(1.0))
            .direction(Direction::Horizontal)
            .child(self.sidebar())
            .child(
                rect()
                    .width(Size::flex(1.0))
                    .height(Size::fill())
                    .direction(Direction::Vertical)
                    .child(self.content(songs)),
            );
        if queue_open {
            mid = mid.child(self.queue_panel());
        }

        rect()
            .width(Size::fill())
            .height(Size::fill())
            .background(bg())
            .direction(Direction::Vertical)
            .on_global_key_down(move |e: Event<KeyboardEventData>| {
                on_key(app, ui, e);
            })
            .child(self.titlebar())
            .child(mid)
            .child(self.player_bar())
    }
}

// ---------------------------------------------------------------------------
// Keyboard
// ---------------------------------------------------------------------------

fn on_key(app: State<AppState>, ui: State<Ui>, e: Event<KeyboardEventData>) {
    let in_search = app.peek().view == View::Search;
    let ctrl = e.modifiers.ctrl();
    match &e.key {
        Key::Named(NamedKey::Escape) => {
            app.write_unchecked().go_back();
        }
        Key::Named(NamedKey::ArrowLeft) if !in_search => {
            let p = app.peek().player.position_sec();
            yt::seek_to(&app, p - 10.0);
        }
        Key::Named(NamedKey::ArrowRight) if !in_search => {
            let p = app.peek().player.position_sec();
            yt::seek_to(&app, p + 10.0);
        }
        Key::Named(NamedKey::ArrowUp) if !in_search => {
            step_selection(app, -1);
        }
        Key::Named(NamedKey::ArrowDown) if !in_search => {
            step_selection(app, 1);
        }
        Key::Named(NamedKey::Enter) if !in_search => {
            let sel = app.peek().selection;
            if let Some(s) = sel {
                let ids = app.peek().list_ids();
                if s < ids.len() {
                    app.write_unchecked().play_from(&ids, s);
                }
            }
        }
        Key::Named(NamedKey::PageUp) => {
            app.write_unchecked().selection = None;
        }
        Key::Named(NamedKey::PageDown) => {
            app.write_unchecked().selection = None;
        }
        Key::Character(s) if ctrl && (s == "k" || s == "K") => {
            ui.write_unchecked().hover = None;
            app.write_unchecked().go(View::Search);
        }
        Key::Character(s) if !in_search && (s == "m" || s == "M") => {
            app.write_unchecked().toggle_mute();
        }
        Key::Character(s) if !in_search && (s == "q" || s == "Q") => {
            let v = !ui.peek().queue_open;
            ui.write_unchecked().queue_open = v;
        }
        Key::Character(s) if !in_search && (s == "s" || s == "S") => {
            app.write_unchecked().toggle_shuffle();
        }
        Key::Character(s) if !in_search && (s == "r" || s == "R") => {
            app.write_unchecked().cycle_repeat();
        }
        Key::Character(s) if s == "/" && !in_search => {
            app.write_unchecked().go(View::Search);
        }
        Key::Character(s) if s == " " && !in_search => {
            app.write_unchecked().toggle_play();
        }
        _ => {}
    }
}

fn step_selection(app: State<AppState>, delta: i32) {
    let len = app.peek().list_ids().len();
    if len == 0 {
        return;
    }
    let next = match app.peek().selection {
        None if delta < 0 => len - 1,
        None => 0,
        Some(s) => (s as i32 + delta).clamp(0, len as i32 - 1) as usize,
    };
    app.write_unchecked().selection = Some(next);
}

// ---------------------------------------------------------------------------
// Titlebar (44px custom chrome, frameless window)
// ---------------------------------------------------------------------------

impl Zuno {
    fn titlebar(&self) -> Element {
        rect()
            .width(Size::fill())
            .height(Size::px(TITLEBAR_H))
            .background(bg())
            .direction(Direction::Horizontal)
            .cross_align(Alignment::Center)
            .padding(Gaps::new_symmetric(0.0, 12.0))
            .spacing(10.0)
            .window_drag()
            .child(
                rect()
                    .width(Size::px(22.0))
                    .height(Size::px(22.0))
                    .background(primary())
                    .corner_radius(6.0)
                    .main_align(Alignment::Center)
                    .cross_align(Alignment::Center)
                    .child(label().text("Z").font_size(13.0).font_weight(700).color((255, 255, 255))),
            )
            .child(label().text("ZUNO").font_size(14.0).font_weight(700).color(fg()))
            .child(rect().width(Size::flex(1.0)).height(Size::px(1.0)))
            .child(TitlebarButton::new(TitlebarAction::Minimize))
            .child(TitlebarButton::new(TitlebarAction::Maximize))
            .child(TitlebarButton::new(TitlebarAction::Close))
            .into()
    }

    // -----------------------------------------------------------------------
    // Sidebar (220px)
    // -----------------------------------------------------------------------

    fn nav_item(&self, id: &str, glyph: &str, title: &str, active: bool) -> Element {
        let app = self.app;
        let ui = self.ui;
        let hov = ui.peek().hover.as_deref() == Some(id);
        let view = match id {
            "nav-home" => View::Home,
            "nav-search" => View::Search,
            "nav-library" => View::Library,
            _ => View::Settings,
        };
        let key = id.to_string();
        let key2 = id.to_string();
        rect()
            .width(Size::fill())
            .height(Size::px(36.0))
            .background(if active { card() } else if hov { card() } else { bg() })
            .corner_radius(8.0)
            .direction(Direction::Horizontal)
            .cross_align(Alignment::Center)
            .padding(Gaps::new_symmetric(0.0, 12.0))
            .spacing(10.0)
            .on_pointer_enter(move |_| set_hover(ui, &key))
            .on_pointer_leave(move |_| clear_hover(ui, &key2))
            .on_press(move |_| {
                if view == View::Search {
                    ui.write_unchecked().hover = None;
                    app.write_unchecked().go(View::Search);
                } else {
                    ui.write_unchecked().hover = None;
                    app.write_unchecked().go(view.clone());
                }
            })
            .child(
                label()
                    .text(glyph.to_string())
                    .font_size(15.0)
                    .color(if active { primary() } else { muted_fg() })
                    .width(Size::px(20.0)),
            )
            .child(
                label()
                    .text(title.to_string())
                    .font_size(14.0)
                    .font_weight(if active { 600 } else { 400 })
                    .color(if active { fg() } else { muted_fg() }),
            )
            .into()
    }

    fn sidebar(&self) -> Element {
        let g = self.app.peek();
        let view = g.view.clone();
        let active = |v: View| view == v;
        let mut list = rect()
            .width(Size::fill())
            .direction(Direction::Vertical)
            .spacing(2.0)
            .child(self.nav_item("nav-home", "⌂", "Home", active(View::Home)))
            .child(self.nav_item("nav-search", "⌕", "Search", active(View::Search)))
            .child(self.nav_item("nav-library", "▤", "Library", active(View::Library)))
            .child(self.nav_item("nav-settings", "⚙", "Settings", active(View::Settings)))
            .child(
                rect()
                    .width(Size::fill())
                    .height(Size::px(28.0))
                    .padding(Gaps::new(16.0, 0.0, 0.0, 12.0))
                    .main_align(Alignment::End)
                    .child(
                        label()
                            .text("PLAYLISTS")
                            .font_size(XS.size)
                            .font_weight(XS.weight as i32)
                            .color(muted_fg()),
                    ),
            );
        for p in g.library.playlists.iter() {
            let id = p.id;
            let title = p.title.clone();
            let n = p.track_ids.len();
            let seed = AppState::playlist_seed(id);
            let art_url = p.artwork_url.clone();
            let app = self.app;
            let ui = self.ui;
            let hid = format!("pl-{id}");
            let hid2 = hid.clone();
            let hov = ui.peek().hover.as_deref() == Some(hid.as_str());
            let current = matches!(&view, View::Playlist(pid) if *pid == id);
            list = list.child(
                rect()
                    .width(Size::fill())
                    .height(Size::px(52.0))
                    .background(if current || hov { card() } else { bg() })
                    .corner_radius(8.0)
                    .direction(Direction::Horizontal)
                    .cross_align(Alignment::Center)
                    .padding(Gaps::new_symmetric(0.0, 8.0))
                    .spacing(10.0)
                    .on_pointer_enter(move |_| set_hover(ui, &hid))
                    .on_pointer_leave(move |_| clear_hover(ui, &hid2))
                    .on_press(move |_| {
                        app.write_unchecked().open_playlist(id);
                    })
                    .child(tile(app, art_url.as_deref(), seed, 40.0, 4.0))
                    .child(
                        rect()
                            .width(Size::flex(1.0))
                            .direction(Direction::Vertical)
                            .child(
                                label()
                                    .text(title)
                                    .font_size(14.0)
                                    .font_weight(500)
                                    .color(fg())
                                    .max_lines(1),
                            )
                            .child(small_muted(count_songs(n))),
                    ),
            );
        }
        drop(g);
        // Live-YouTube status (empty in --bench — nothing shows).
        let status = self.ui.peek().yt_status.clone();
        if !status.is_empty() {
            list = list.child(
                rect()
                    .width(Size::fill())
                    .padding(Gaps::new(16.0, 0.0, 0.0, 8.0))
                    .child(small_muted(status)),
            );
        }
        rect()
            .width(Size::px(SIDEBAR_W))
            .height(Size::fill())
            .background(bg())
            .padding(Gaps::new_all(12.0))
            .child(
                ScrollView::new()
                    .show_scrollbar(false)
                    .child(list),
            )
            .into()
    }

    // -----------------------------------------------------------------------
    // Content
    // -----------------------------------------------------------------------

    fn content(&self, songs: ScrollController) -> Element {
        let view = self.app.peek().view.clone();
        match view {
            View::Home => self.home(),
            View::Library => self.library(songs),
            View::Album(id) => self.album_page(id),
            View::Playlist(id) => self.playlist_page(id),
            View::Artist(id) => self.artist_page(id),
            View::Search => self.search_page(),
            View::Settings => self.settings_page(),
        }
    }

    fn page_scroll(&self, inner: Element) -> Element {
        rect()
            .width(Size::fill())
            .height(Size::fill())
            .padding(Gaps::new_all(20.0))
            .child(ScrollView::new().child(inner))
            .into()
    }

    fn home(&self) -> Element {
        let g = self.app.peek();
        let recent = g.library.home_recent_albums(12);
        let mixes: Vec<(u32, String)> =
            g.library.mixes.iter().map(|m| (m.id, m.title.clone())).collect();
        let mut new_albums: Vec<(u32, String, String, u16, Option<String>)> = g
            .library
            .albums
            .iter()
            .map(|a| {
                let an = g.library.artist(a.artist_id).name.clone();
                (a.id, a.title.clone(), an, a.year, a.artwork_url.clone())
            })
            .collect();
        new_albums.sort_by(|a, b| b.3.cmp(&a.3));
        new_albums.truncate(24);
        let mut artists: Vec<(u32, String, u32, Option<String>)> = g
            .library
            .artists
            .iter()
            .map(|r| (r.id, r.name.clone(), r.monthly_listeners, r.artwork_url.clone()))
            .collect();
        artists.sort_by(|a, b| b.2.cmp(&a.2));
        artists.truncate(12);
        drop(g);

        let mut col = rect()
            .width(Size::fill())
            .direction(Direction::Vertical)
            .spacing(8.0)
            .child(h1("Home".to_string()))
            .child(rect().width(Size::fill()).height(Size::px(8.0)))
            .child(h2("Recently played".to_string()));
        col = col.child(self.album_shelf(recent));
        col = col.child(rect().width(Size::fill()).height(Size::px(8.0)));
        col = col.child(h2("Made for you".to_string()));
        {
            let mut shelf = rect()
                .direction(Direction::Horizontal)
                .spacing(16.0)
                .padding(Gaps::new_symmetric(4.0, 0.0));
            for (id, title) in mixes {
                shelf = shelf.child(self.mix_card(id, title));
            }
            col = col.child(
                rect()
                    .width(Size::fill())
                    .height(Size::px(CARD_H + 52.0))
                    .child(ScrollView::new().direction(Direction::Horizontal).child(shelf)),
            );
        }
        col = col.child(rect().width(Size::fill()).height(Size::px(8.0)));
        col = col.child(h2("New albums".to_string()));
        for chunk in new_albums.chunks(4) {
            let mut row = rect().width(Size::fill()).direction(Direction::Horizontal).spacing(16.0);
            for (id, title, artist, year, url) in chunk {
                row = row.child(self.album_card(*id, title.clone(), artist.clone(), *year, url.clone()));
            }
            col = col.child(row);
        }
        col = col.child(rect().width(Size::fill()).height(Size::px(8.0)));
        col = col.child(h2("Popular artists".to_string()));
        {
            let mut shelf = rect()
                .direction(Direction::Horizontal)
                .spacing(16.0)
                .padding(Gaps::new_symmetric(4.0, 0.0));
            for (id, name, _, url) in artists {
                shelf = shelf.child(self.artist_card(id, name, url));
            }
            col = col.child(
                rect()
                    .width(Size::fill())
                    .height(Size::px(CARD_H + 52.0))
                    .child(ScrollView::new().direction(Direction::Horizontal).child(shelf)),
            );
        }
        self.page_scroll(col.into())
    }

    fn album_shelf(&self, ids: Vec<u32>) -> Element {
        let g = self.app.peek();
        let mut shelf = rect()
            .direction(Direction::Horizontal)
            .spacing(16.0)
            .padding(Gaps::new_symmetric(4.0, 0.0));
        for id in ids {
            if let Some(a) = g.library.albums.iter().find(|a| a.id == id) {
                let an = g.library.artist(a.artist_id).name.clone();
                shelf =
                    shelf.child(self.album_card(a.id, a.title.clone(), an, a.year, a.artwork_url.clone()));
            }
        }
        drop(g);
        rect()
            .width(Size::fill())
            .height(Size::px(CARD_H + 52.0))
            .child(ScrollView::new().direction(Direction::Horizontal).child(shelf))
            .into()
    }

    fn album_card(&self, id: u32, title: String, artist: String, year: u16, url: Option<String>) -> Element {
        self.card_base(
            format!("album-{id}"),
            url,
            AppState::album_seed(id),
            title,
            format!("{artist} · {year}"),
            {
                let app = self.app;
                move |_| app.write_unchecked().open_album(id)
            },
        )
    }

    fn mix_card(&self, id: u32, title: String) -> Element {
        let (n, url) = self
            .app
            .peek()
            .library
            .playlist(id)
            .map(|p| (p.track_ids.len(), p.artwork_url.clone()))
            .unwrap_or((0, None));
        self.card_base(
            format!("mix-{id}"),
            url,
            AppState::playlist_seed(id),
            title,
            count_songs(n),
            {
                let app = self.app;
                move |_| app.write_unchecked().open_playlist(id)
            },
        )
    }

    fn artist_card(&self, id: u32, name: String, url: Option<String>) -> Element {
        let app = self.app;
        let ui = self.ui;
        let g = app.peek();
        let seed = AppState::artist_seed(id);
        drop(g);
        let hid = format!("artist-{id}");
        let hid2 = hid.clone();
        let hov = ui.peek().hover.as_deref() == Some(hid.as_str());
        rect()
            .width(Size::px(CARD_W))
            .direction(Direction::Vertical)
            .spacing(8.0)
            .padding(Gaps::new_all(8.0))
            .background(if hov { card() } else { bg() })
            .corner_radius(8.0)
            .on_pointer_enter(move |_| set_hover(ui, &hid))
            .on_pointer_leave(move |_| clear_hover(ui, &hid2))
            .on_press(move |_| {
                app.write_unchecked().open_artist(id);
            })
            .child(tile(app, url.as_deref(), seed, CARD_W - 16.0, (CARD_W - 16.0) / 2.0))
            .child(
                label()
                    .text(name)
                    .font_size(14.0)
                    .font_weight(500)
                    .color(fg())
                    .max_lines(1),
            )
            .into()
    }

    /// 176px card: square artwork, 2-line title, 1-line subtitle, hover shows a
    /// red circular play pill in a reserved footer row (no layout shift).
    fn card_base(
        &self,
        hid: String,
        url: Option<String>,
        seed: u64,
        title: String,
        subtitle: String,
        open: impl FnMut(Event<PressEventData>) + 'static,
    ) -> Element {
        let app = self.app;
        let ui = self.ui;
        let hid2 = hid.clone();
        let hov = ui.peek().hover.as_deref() == Some(hid.as_str());
        let play_pill: Element = if hov {
            rect()
                .width(Size::px(28.0))
                .height(Size::px(28.0))
                .background(primary())
                .corner_radius(14.0)
                .main_align(Alignment::Center)
                .cross_align(Alignment::Center)
                .child(label().text("▶").font_size(12.0).color((255, 255, 255)))
                .into()
        } else {
            rect().width(Size::px(28.0)).height(Size::px(28.0)).into()
        };
        rect()
            .width(Size::px(CARD_W))
            .direction(Direction::Vertical)
            .spacing(8.0)
            .padding(Gaps::new_all(8.0))
            .background(if hov { card() } else { bg() })
            .corner_radius(8.0)
            .on_pointer_enter(move |_| set_hover(ui, &hid))
            .on_pointer_leave(move |_| clear_hover(ui, &hid2))
            .on_press(open)
            .child(tile(app, url.as_deref(), seed, CARD_W - 16.0, 4.0))
            .child(
                label().text(title).font_size(14.0).font_weight(500).color(fg()).max_lines(2),
            )
            .child(small_muted(subtitle))
            .child(
                rect()
                    .width(Size::fill())
                    .height(Size::px(28.0))
                    .main_align(Alignment::End)
                    .cross_align(Alignment::Center)
                    .child(play_pill),
            )
            .into()
    }

    // -----------------------------------------------------------------------
    // Library
    // -----------------------------------------------------------------------

    fn library(&self, songs: ScrollController) -> Element {
        let tab = self.app.peek().library_tab;
        let tabs = rect()
            .width(Size::fill())
            .direction(Direction::Horizontal)
            .spacing(8.0)
            .child(self.tab_pill("tab-pl", "Playlists", tab == LibraryTab::Playlists, LibraryTab::Playlists))
            .child(self.tab_pill("tab-al", "Albums", tab == LibraryTab::Albums, LibraryTab::Albums))
            .child(self.tab_pill("tab-ar", "Artists", tab == LibraryTab::Artists, LibraryTab::Artists))
            .child(self.tab_pill("tab-so", "Songs", tab == LibraryTab::Songs, LibraryTab::Songs));
        let header = rect()
            .width(Size::fill())
            .direction(Direction::Vertical)
            .spacing(12.0)
            .padding(Gaps::new(20.0, 20.0, 0.0, 20.0))
            .child(h1("Your Library".to_string()))
            .child(tabs);
        match tab {
            LibraryTab::Songs => {
                let g = self.app.peek();
                let ids: Vec<TrackId> = (0..g.library.tracks.len() as u32).collect();
                let data = SongsData {
                    ids,
                    sel: g.selection,
                    cur: g.current,
                    hov: self.ui.peek().hover.clone(),
                    frame: self.ui.peek().frame,
                };
                let n = data.ids.len();
                drop(g);
                let app = self.app;
                let ui = self.ui;
                rect()
                    .width(Size::fill())
                    .height(Size::fill())
                    .direction(Direction::Vertical)
                    .child(header)
                    .child(
                        VirtualScrollView::new_with_data_controlled(
                            data,
                            move |item: VirtualItem, d: &SongsData| {
                                if item.index < d.ids.len() {
                                    let g = app.peek();
                                    let el = track_row(app, ui, &g, d.ids[item.index], item.index);
                                    drop(g);
                                    el
                                } else {
                                    rect().width(Size::fill()).height(Size::px(52.0)).into()
                                }
                            },
                            songs,
                        )
                        .length(n)
                        .item_size(52.0),
                    )
                    .into()
            }
            LibraryTab::Albums => {
                let g = self.app.peek();
                let albums: Vec<(u32, String, String, u16, Option<String>)> = g
                    .library
                    .albums
                    .iter()
                    .map(|a| {
                        let an = g.library.artist(a.artist_id).name.clone();
                        (a.id, a.title.clone(), an, a.year, a.artwork_url.clone())
                    })
                    .collect();
                drop(g);
                let mut col = rect().width(Size::fill()).direction(Direction::Vertical).spacing(16.0);
                for chunk in albums.chunks(4) {
                    let mut row = rect().width(Size::fill()).direction(Direction::Horizontal).spacing(16.0);
                    for (id, title, artist, year, url) in chunk {
                        row = row.child(self.album_card(*id, title.clone(), artist.clone(), *year, url.clone()));
                    }
                    col = col.child(row);
                }
                rect()
                    .width(Size::fill())
                    .height(Size::fill())
                    .direction(Direction::Vertical)
                    .child(header)
                    .child(self.page_scroll(col.into()))
                    .into()
            }
            LibraryTab::Artists => {
                let g = self.app.peek();
                let artists: Vec<(u32, String, u32, usize, Option<String>)> = g
                    .library
                    .artists
                    .iter()
                    .map(|r| (r.id, r.name.clone(), r.monthly_listeners, r.album_ids.len(), r.artwork_url.clone()))
                    .collect();
                drop(g);
                let mut col = rect().width(Size::fill()).direction(Direction::Vertical).spacing(2.0);
                for (id, name, ml, nal, url) in artists {
                    col = col.child(self.artist_row(id, name, ml, nal, url));
                }
                rect()
                    .width(Size::fill())
                    .height(Size::fill())
                    .direction(Direction::Vertical)
                    .child(header)
                    .child(self.page_scroll(col.into()))
                    .into()
            }
            LibraryTab::Playlists => {
                let g = self.app.peek();
                let pls: Vec<(u32, String, usize, Option<String>)> = g
                    .library
                    .playlists
                    .iter()
                    .map(|p| (p.id, p.title.clone(), p.track_ids.len(), p.artwork_url.clone()))
                    .collect();
                drop(g);
                let mut col = rect().width(Size::fill()).direction(Direction::Vertical).spacing(2.0);
                for (id, title, n, url) in pls {
                    col = col.child(self.playlist_row(id, title, n, url));
                }
                rect()
                    .width(Size::fill())
                    .height(Size::fill())
                    .direction(Direction::Vertical)
                    .child(header)
                    .child(self.page_scroll(col.into()))
                    .into()
            }
        }
    }

    fn tab_pill(&self, id: &str, title: &str, active: bool, tab: LibraryTab) -> Element {
        let app = self.app;
        let ui = self.ui;
        let key = id.to_string();
        let key2 = key.clone();
        rect()
            .padding(Gaps::new_symmetric(8.0, 16.0))
            .background(if active { primary() } else { card() })
            .corner_radius(999.0)
            .on_pointer_enter(move |_| set_hover(ui, &key))
            .on_pointer_leave(move |_| clear_hover(ui, &key2))
            .on_press(move |_| {
                app.write_unchecked().set_library_tab(tab);
            })
            .child(
                label()
                    .text(title.to_string())
                    .font_size(13.0)
                    .font_weight(600)
                    .color(if active { (255, 255, 255) } else { muted_fg() }),
            )
            .into()
    }

    fn artist_row(&self, id: u32, name: String, ml: u32, nal: usize, url: Option<String>) -> Element {
        let app = self.app;
        let ui = self.ui;
        let hid = format!("lartist-{id}");
        let hid2 = hid.clone();
        let hov = ui.peek().hover.as_deref() == Some(hid.as_str());
        rect()
            .width(Size::fill())
            .height(Size::px(52.0))
            .background(if hov { card() } else { bg() })
            .corner_radius(8.0)
            .direction(Direction::Horizontal)
            .cross_align(Alignment::Center)
            .padding(Gaps::new_symmetric(0.0, 12.0))
            .spacing(12.0)
            .on_pointer_enter(move |_| set_hover(ui, &hid))
            .on_pointer_leave(move |_| clear_hover(ui, &hid2))
            .on_press(move |_| {
                app.write_unchecked().open_artist(id);
            })
            .child(tile(app, url.as_deref(), AppState::artist_seed(id), 40.0, 20.0))
            .child(
                rect()
                    .width(Size::flex(1.0))
                    .direction(Direction::Vertical)
                    .child(label().text(name).font_size(14.0).font_weight(500).color(fg()).max_lines(1))
                    .child(small_muted(format!("{} · {} albums", listeners(ml), nal))),
            )
            .into()
    }

    fn playlist_row(&self, id: u32, title: String, n: usize, url: Option<String>) -> Element {
        let app = self.app;
        let ui = self.ui;
        let hid = format!("lpl-{id}");
        let hid2 = hid.clone();
        let hov = ui.peek().hover.as_deref() == Some(hid.as_str());
        rect()
            .width(Size::fill())
            .height(Size::px(52.0))
            .background(if hov { card() } else { bg() })
            .corner_radius(8.0)
            .direction(Direction::Horizontal)
            .cross_align(Alignment::Center)
            .padding(Gaps::new_symmetric(0.0, 12.0))
            .spacing(12.0)
            .on_pointer_enter(move |_| set_hover(ui, &hid))
            .on_pointer_leave(move |_| clear_hover(ui, &hid2))
            .on_press(move |_| {
                app.write_unchecked().open_playlist(id);
            })
            .child(tile(app, url.as_deref(), AppState::playlist_seed(id), 40.0, 4.0))
            .child(
                rect()
                    .width(Size::flex(1.0))
                    .direction(Direction::Vertical)
                    .child(label().text(title).font_size(14.0).font_weight(500).color(fg()).max_lines(1))
                    .child(small_muted(count_songs(n))),
            )
            .into()
    }

    // -----------------------------------------------------------------------
    // Album / Playlist / Artist pages
    // -----------------------------------------------------------------------

    fn play_shuffle_buttons(&self) -> Element {
        let app = self.app;
        rect()
            .direction(Direction::Horizontal)
            .spacing(12.0)
            .child(
                rect()
                    .padding(Gaps::new_symmetric(10.0, 24.0))
                    .background(primary())
                    .corner_radius(999.0)
                    .on_press(move |_| {
                        let ids = app.peek().list_ids();
                        if !ids.is_empty() {
                            app.write_unchecked().play_from(&ids, 0);
                        }
                    })
                    .child(label().text("Play").font_size(14.0).font_weight(600).color((255, 255, 255))),
            )
            .child({
                let app2 = self.app;
                rect()
                    .padding(Gaps::new_symmetric(10.0, 24.0))
                    .background(card())
                    .corner_radius(999.0)
                    .on_press(move |_| {
                        if !app2.peek().shuffle {
                            app2.write_unchecked().toggle_shuffle();
                        }
                        let ids = app2.peek().list_ids();
                        if !ids.is_empty() {
                            app2.write_unchecked().play_from(&ids, 0);
                        }
                    })
                    .child(label().text("Shuffle").font_size(14.0).font_weight(600).color(fg()))
            })
            .into()
    }

    fn detail_header(
        &self,
        url: Option<String>,
        seed: u64,
        round: bool,
        title: String,
        meta: String,
        sub: String,
    ) -> Element {
        rect()
            .width(Size::fill())
            .direction(Direction::Horizontal)
            .cross_align(Alignment::Center)
            .spacing(24.0)
            .child(tile(self.app, url.as_deref(), seed, 232.0, if round { 116.0 } else { 4.0 }))
            .child(
                rect()
                    .width(Size::flex(1.0))
                    .direction(Direction::Vertical)
                    .spacing(8.0)
                    .child(h1(title))
                    .child(small_muted(meta))
                    .child(small_muted(sub))
                    .child(rect().width(Size::fill()).height(Size::px(8.0)))
                    .child(self.play_shuffle_buttons()),
            )
            .into()
    }

    fn track_list_plain(&self, ids: &[TrackId]) -> Element {
        let g = self.app.peek();
        let mut col = rect().width(Size::fill()).direction(Direction::Vertical).spacing(2.0);
        for (i, id) in ids.iter().enumerate() {
            col = col.child(track_row(self.app, self.ui, &g, *id, i));
        }
        drop(g);
        col.into()
    }

    fn album_page(&self, id: u32) -> Element {
        let g = self.app.peek();
        let (title, artist, year, url, ids) = match g.library.albums.iter().find(|a| a.id == id) {
            Some(a) => {
                let an = g.library.artist(a.artist_id).name.clone();
                (a.title.clone(), an, a.year, a.artwork_url.clone(), a.track_ids.clone())
            }
            None => ("Unknown album".to_string(), String::new(), 0, None, Vec::new()),
        };
        let durs: Vec<u32> = ids.iter().map(|t| g.library.track(*t).duration_sec).collect();
        let sub = counts_line(durs.iter().copied());
        drop(g);
        let header = self.detail_header(
            url,
            AppState::album_seed(id),
            false,
            title,
            format!("Album · {artist} · {year}"),
            sub,
        );
        let list = self.track_list_plain(&ids);
        self.page_scroll(rect().width(Size::fill()).direction(Direction::Vertical).spacing(16.0).child(header).child(list).into())
    }

    fn playlist_page(&self, id: u32) -> Element {
        let g = self.app.peek();
        let (title, desc, url, ids) = match g.library.playlist(id) {
            Some(p) => (
                p.title.clone(),
                p.description.clone().unwrap_or_default(),
                p.artwork_url.clone(),
                p.track_ids.clone(),
            ),
            None => ("Unknown playlist".to_string(), String::new(), None, Vec::new()),
        };
        let durs: Vec<u32> = ids.iter().map(|t| g.library.track(*t).duration_sec).collect();
        let sub = counts_line(durs.iter().copied());
        drop(g);
        let header = self.detail_header(
            url,
            AppState::playlist_seed(id),
            false,
            title,
            format!("Playlist · {}", count_songs(ids.len())),
            if desc.is_empty() { sub } else { desc },
        );
        let list = self.track_list_plain(&ids);
        self.page_scroll(rect().width(Size::fill()).direction(Direction::Vertical).spacing(16.0).child(header).child(list).into())
    }

    fn artist_page(&self, id: u32) -> Element {
        let g = self.app.peek();
        let (name, ml, url, top, album_ids) = match g.library.artists.iter().find(|r| r.id == id) {
            Some(r) => {
                let mut tids: Vec<TrackId> = Vec::new();
                for aid in r.album_ids.iter().take(6) {
                    if let Some(a) = g.library.albums.iter().find(|a| a.id == *aid) {
                        tids.extend(a.track_ids.iter().take(10 - tids.len().min(10)));
                    }
                    if tids.len() >= 10 {
                        break;
                    }
                }
                (r.name.clone(), r.monthly_listeners, r.artwork_url.clone(), tids, r.album_ids.clone())
            }
            None => ("Unknown artist".to_string(), 0, None, Vec::new(), Vec::new()),
        };
        drop(g);
        let app = self.app;
        let header = rect()
            .width(Size::fill())
            .direction(Direction::Horizontal)
            .cross_align(Alignment::Center)
            .spacing(24.0)
            .child(tile(app, url.as_deref(), AppState::artist_seed(id), 232.0, 116.0))
            .child(
                rect()
                    .width(Size::flex(1.0))
                    .direction(Direction::Vertical)
                    .spacing(8.0)
                    .child(h1(name))
                    .child(small_muted(format!("{} monthly listeners", listeners(ml))))
                    .child(rect().width(Size::fill()).height(Size::px(8.0)))
                    .child(self.play_shuffle_buttons()),
            );
        let mut col = rect()
            .width(Size::fill())
            .direction(Direction::Vertical)
            .spacing(16.0)
            .child(header)
            .child(h3("Top tracks".to_string()))
            .child(self.track_list_plain(&top))
            .child(h3("Albums".to_string()));
        {
            let g2 = app.peek();
            let mut row = rect().width(Size::fill()).direction(Direction::Horizontal).spacing(16.0);
            let mut n = 0;
            for aid in album_ids.iter() {
                if let Some(a) = g2.library.albums.iter().find(|a| a.id == *aid) {
                    let an = g2.library.artist(a.artist_id).name.clone();
                    row = row.child(self.album_card(a.id, a.title.clone(), an, a.year, a.artwork_url.clone()));
                    n += 1;
                    if n % 4 == 0 {
                        col = col.child(row);
                        row = rect().width(Size::fill()).direction(Direction::Horizontal).spacing(16.0);
                    }
                }
            }
            if n % 4 != 0 {
                col = col.child(row);
            }
            drop(g2);
        }
        self.page_scroll(col.into())
    }

    // -----------------------------------------------------------------------
    // Search
    // -----------------------------------------------------------------------

    fn search_page(&self) -> Element {
        let field = rect()
            .width(Size::fill())
            .padding(Gaps::new(20.0, 20.0, 0.0, 20.0))
            .child(
                rect()
                    .width(Size::fill())
                    .background(card())
                    .corner_radius(999.0)
                    .padding(Gaps::new_symmetric(10.0, 20.0))
                    .child(Input::new(self.search.clone()).placeholder("Search songs, albums, artists…")),
            );
        let g = self.app.peek();
        let res = search::search(&g.library, &g.search_query);
        let mut col = rect().width(Size::fill()).direction(Direction::Vertical).spacing(16.0);
        if res.tracks.is_empty() && res.albums.is_empty() && res.artists.is_empty() {
            col = col.child(small_muted(
                if g.search_query.is_empty() {
                    "Type to search the library.".to_string()
                } else {
                    format!("No results for '{}'", g.search_query)
                },
            ));
        } else {
            if let Some(first) = res.tracks.first() {
                let fview = g.library.track_view(*first);
                let ft = g.library.track(*first);
                let fseed = AppState::album_seed(ft.album_id);
                let furl = ft.artwork_url.clone();
                let top = rect()
                    .width(Size::fill())
                    .background(card())
                    .corner_radius(8.0)
                    .padding(Gaps::new_all(16.0))
                    .direction(Direction::Horizontal)
                    .cross_align(Alignment::Center)
                    .spacing(16.0)
                    .child(tile(self.app, furl.as_deref(), fseed, 96.0, 4.0))
                    .child(
                        rect()
                            .direction(Direction::Vertical)
                            .spacing(4.0)
                            .child(small_muted("TOP RESULT".to_string()))
                            .child(h2(fview.title.to_string()))
                            .child(small_muted(format!("{} · {}", fview.artist, fview.album))),
                    );
                col = col.child(top);
            }
            if !res.tracks.is_empty() {
                col = col.child(h3("Songs".to_string()));
                let mut songs = rect().width(Size::fill()).direction(Direction::Vertical).spacing(2.0);
                for (i, id) in res.tracks.iter().enumerate() {
                    songs = songs.child(track_row(self.app, self.ui, &g, *id, i));
                }
                col = col.child(songs);
            }
            if !res.albums.is_empty() {
                col = col.child(h3("Albums".to_string()));
                let mut shelf = rect().direction(Direction::Horizontal).spacing(16.0);
                for aid in res.albums.iter().take(12) {
                    if let Some(a) = g.library.albums.iter().find(|a| a.id == *aid) {
                        let an = g.library.artist(a.artist_id).name.clone();
                        shelf = shelf.child(self.album_card(a.id, a.title.clone(), an, a.year, a.artwork_url.clone()));
                    }
                }
                col = col.child(
                    rect()
                        .width(Size::fill())
                        .height(Size::px(CARD_H + 52.0))
                        .child(ScrollView::new().direction(Direction::Horizontal).child(shelf)),
                );
            }
            if !res.artists.is_empty() {
                col = col.child(h3("Artists".to_string()));
                let mut shelf = rect().direction(Direction::Horizontal).spacing(16.0);
                for rid in res.artists.iter().take(12) {
                    if let Some(r) = g.library.artists.iter().find(|r| r.id == *rid) {
                        shelf = shelf.child(self.artist_card(r.id, r.name.clone(), r.artwork_url.clone()));
                    }
                }
                col = col.child(
                    rect()
                        .width(Size::fill())
                        .height(Size::px(CARD_H + 52.0))
                        .child(ScrollView::new().direction(Direction::Horizontal).child(shelf)),
                );
            }
        }
        drop(g);
        rect()
            .width(Size::fill())
            .height(Size::fill())
            .direction(Direction::Vertical)
            .child(field)
            .child(self.page_scroll(col.into()))
            .into()
    }

    // -----------------------------------------------------------------------
    // Settings
    // -----------------------------------------------------------------------

    fn settings_page(&self) -> Element {
        let ui = self.ui;
        let hq = ui.peek().sett_hq;
        let norm = ui.peek().sett_norm;
        let toggle = move |id: &str, on: bool, flip: fn(&mut Ui)| -> Element {
            let key = id.to_string();
            let key2 = key.clone();
            rect()
                .direction(Direction::Horizontal)
                .cross_align(Alignment::Center)
                .spacing(12.0)
                .child(
                    rect()
                        .width(Size::px(44.0))
                        .height(Size::px(24.0))
                        .background(if on { primary() } else { card() })
                        .corner_radius(12.0)
                        .cross_align(Alignment::Center)
                        .main_align(if on { Alignment::End } else { Alignment::Start })
                        .padding(Gaps::new_all(3.0))
                        .on_press(move |_| {
                            flip(&mut ui.write_unchecked());
                            let _ = &key;
                        })
                        .child(
                            rect()
                                .width(Size::px(18.0))
                                .height(Size::px(18.0))
                                .background((255, 255, 255))
                                .corner_radius(9.0),
                        ),
                )
                .child(body(key2))
                .into()
        };
        self.page_scroll(
            rect()
                .width(Size::fill())
                .direction(Direction::Vertical)
                .spacing(16.0)
                .child(h1("Settings".to_string()))
                .child(h3("Appearance".to_string()))
                .child(
                    rect()
                        .width(Size::fill())
                        .background(card())
                        .corner_radius(8.0)
                        .padding(Gaps::new_all(16.0))
                        .direction(Direction::Vertical)
                        .spacing(4.0)
                        .child(body("Theme".to_string()))
                        .child(small_muted("Dark (pinned for the benchmark)".to_string())),
                )
                .child(h3("Audio".to_string()))
                .child(
                    rect()
                        .width(Size::fill())
                        .background(card())
                        .corner_radius(8.0)
                        .padding(Gaps::new_all(16.0))
                        .direction(Direction::Vertical)
                        .spacing(4.0)
                        .child(body("Quality".to_string()))
                        .child(small_muted("High · 320 kbps Opus (synth stand-in)".to_string())),
                )
                .child(h3("YouTube".to_string()))
                .child({
                    let app = self.app;
                    let ui = self.ui;
                    let cookie = self.cookie;
                    let status = ui.peek().yt_status.clone();
                    let enabled = yt::enabled();
                    rect()
                        .width(Size::fill())
                        .background(card())
                        .corner_radius(8.0)
                        .padding(Gaps::new_all(16.0))
                        .direction(Direction::Vertical)
                        .spacing(8.0)
                        .child(body("Sign in with cookie".to_string()))
                        .child(small_muted(
                            "Browser → music.youtube.com → devtools → Network → any /youtubei/ request → copy the Cookie: request header → paste → Sign in. Unsigned mode keeps all public content working."
                                .to_string(),
                        ))
                        .child(
                            rect()
                                .width(Size::fill())
                                .background(c(DARK.muted))
                                .corner_radius(8.0)
                                .padding(Gaps::new_symmetric(10.0, 12.0))
                                .child(Input::new(cookie).placeholder("Paste the full Cookie: request header…")),
                        )
                        .child(
                            rect()
                                .direction(Direction::Horizontal)
                                .cross_align(Alignment::Center)
                                .spacing(12.0)
                                .child(
                                    rect()
                                        .padding(Gaps::new_symmetric(10.0, 24.0))
                                        .background(primary())
                                        .corner_radius(999.0)
                                        .on_press(move |_| {
                                            if !yt::enabled() {
                                                return;
                                            }
                                            let header = cookie.peek().trim().to_string();
                                            if header.is_empty() {
                                                return;
                                            }
                                            ui.write_unchecked().yt_status = "Signing in…".to_string();
                                            spawn(yt::sign_in(app.clone(), ui.clone(), header));
                                        })
                                        .child(label().text(if enabled { "Sign in" } else { "Sign-in unavailable (bench mode)" }).font_size(14.0).font_weight(600).color((255, 255, 255))),
                                )
                                .child(small_muted(if status.is_empty() { "Unsigned · public content".to_string() } else { status })),
                        )
                })
                .child(toggle("High quality audio", hq, |u| u.sett_hq = !u.sett_hq))
                .child(toggle("Normalize volume", norm, |u| u.sett_norm = !u.sett_norm))
                .into(),
        )
    }

    // -----------------------------------------------------------------------
    // Queue panel (300px)
    // -----------------------------------------------------------------------

    fn queue_panel(&self) -> Element {
        let app = self.app;
        let ui = self.ui;
        let g = app.peek();
        let rows = g.queue_rows();
        let mut col = rect()
            .width(Size::fill())
            .direction(Direction::Vertical)
            .spacing(2.0)
            .child(
                rect()
                    .width(Size::fill())
                    .direction(Direction::Horizontal)
                    .cross_align(Alignment::Center)
                    .child(
                        rect()
                            .width(Size::flex(1.0))
                            .child(h3("Queue".to_string())),
                    )
                    .child(
                        rect()
                            .padding(Gaps::new_all(6.0))
                            .corner_radius(8.0)
                            .on_press(move |_| {
                                ui.write_unchecked().queue_open = false;
                            })
                            .child(label().text("✕").font_size(14.0).color(muted_fg())),
                    ),
            );
        let mut last: Option<Region> = None;
        for (i, (id, region)) in rows.iter().enumerate() {
            if Some(*region) != last {
                if let Some(l) = match region {
                    Region::Played => None,
                    Region::Current => Some("NOW PLAYING"),
                    Region::Manual => Some("NEXT IN QUEUE"),
                    Region::Automatic => Some("NEXT UP"),
                } {
                    col = col.child(
                        rect()
                            .width(Size::fill())
                            .padding(Gaps::new(10.0, 0.0, 2.0, 0.0))
                            .child(
                                label()
                                    .text(l)
                                    .font_size(XS.size)
                                    .font_weight(XS.weight as i32)
                                    .color(muted_fg()),
                            ),
                    );
                }
                last = Some(*region);
            }
            let tv = g.library.track_view(*id);
            let is_cur = *region == Region::Current;
            let playing = is_cur && g.playing;
            let hid = format!("q-{i}");
            let hid2 = hid.clone();
            let hov = ui.peek().hover.as_deref() == Some(hid.as_str());
            let title = tv.title.to_string();
            let artist = tv.artist.to_string();
            let dur = tv.duration_sec;
            let tq = g.library.track(*id);
            let seed = AppState::album_seed(tq.album_id);
            let art_url = tq.artwork_url.clone();
            col = col.child(
                rect()
                    .width(Size::fill())
                    .height(Size::px(48.0))
                    .background(if hov { card() } else { bg() })
                    .corner_radius(8.0)
                    .direction(Direction::Horizontal)
                    .cross_align(Alignment::Center)
                    .padding(Gaps::new_symmetric(0.0, 8.0))
                    .spacing(8.0)
                    .on_pointer_enter(move |_| set_hover(ui, &hid))
                    .on_pointer_leave(move |_| clear_hover(ui, &hid2))
                    .on_press(move |_| {
                        if let Some(pid) = app.write_unchecked().queue.jump_to(i) {
                            app.write_unchecked().play_track(pid);
                        }
                    })
                    .child(tile(app, art_url.as_deref(), seed, 32.0, 4.0))
                    .child(
                        rect()
                            .width(Size::flex(1.0))
                            .direction(Direction::Vertical)
                            .child(
                                label()
                                    .text(title)
                                    .font_size(13.0)
                                    .font_weight(500)
                                    .color(if is_cur { primary() } else { fg() })
                                    .max_lines(1),
                            )
                            .child(small_muted(artist)),
                    )
                    .child(if playing {
                        bars(0).into()
                    } else {
                        small_muted(mmss(dur))
                    })
                    .child({
                        let app2 = app;
                        rect()
                            .padding(Gaps::new_all(4.0))
                            .on_press(move |_| {
                                app2.write_unchecked().queue.remove_at(i);
                            })
                            .child(label().text("✕").font_size(11.0).color(muted_fg()))
                    }),
            );
        }
        drop(g);
        rect()
            .width(Size::px(QUEUE_W))
            .height(Size::fill())
            .background(bg())
            .padding(Gaps::new_all(12.0))
            .child(ScrollView::new().show_scrollbar(false).child(col))
            .into()
    }

    // -----------------------------------------------------------------------
    // Player bar (76px)
    // -----------------------------------------------------------------------

    fn player_bar(&self) -> Element {
        let app = self.app;
        let ui = self.ui;
        let g = app.peek();
        let (title, artist, art_url, seed, liked, tid) = match g.current_track() {
            Some(t) => (
                t.title.clone(),
                g.library.artist(t.artist_id).name.clone(),
                t.artwork_url.clone(),
                AppState::album_seed(t.album_id),
                t.liked,
                Some(t.id),
            ),
            None => ("Nothing playing".to_string(), "Zuno".to_string(), None, 0, false, None),
        };
        let pos = g.player.position_sec();
        let dur = g.player.duration_sec();
        let playing = g.playing;
        let shuffle = g.shuffle;
        let repeat = g.repeat;
        let vol = g.volume;
        let muted = g.muted;
        let qopen = ui.peek().queue_open;
        let cur_album = g.current_track().map(|t| t.album_id);
        let cur_artist = g.current_track().map(|t| t.artist_id);
        drop(g);

        let seek_val = if dur > 0.0 { (pos / dur * 100.0).clamp(0.0, 100.0) } else { 0.0 };
        rect()
            .width(Size::fill())
            .height(Size::px(PLAYER_BAR_H))
            .background(c(DARK.popover))
            .direction(Direction::Horizontal)
            .cross_align(Alignment::Center)
            .padding(Gaps::new_symmetric(0.0, 16.0))
            .spacing(12.0)
            .child(if tid.is_some() { tile(app, art_url.as_deref(), seed, 40.0, 4.0) } else {
                rect().width(Size::px(40.0)).height(Size::px(40.0)).background(card()).corner_radius(4.0).into()
            })
            .child(
                rect()
                    .width(Size::px(180.0))
                    .direction(Direction::Vertical)
                    .child(
                        rect()
                            .on_press(move |_| {
                                if let Some(aid) = cur_album {
                                    app.write_unchecked().open_album(aid);
                                }
                            })
                            .child(label().text(title).font_size(14.0).font_weight(500).color(fg()).max_lines(1)),
                    )
                    .child(
                        rect()
                            .on_press(move |_| {
                                if let Some(rid) = cur_artist {
                                    app.write_unchecked().open_artist(rid);
                                }
                            })
                            .child(small_muted(artist)),
                    ),
            )
            .child(ctrl_btn("shuf", "⇄", shuffle, self.app, self.ui, move |a: &mut AppState| a.toggle_shuffle()))
            .child(ctrl_btn("prev", "|◀", false, self.app, self.ui, move |a: &mut AppState| a.previous()))
            .child({
                let glyph = if playing { "❚❚" } else { "▶" };
                rect()
                    .width(Size::px(40.0))
                    .height(Size::px(40.0))
                    .background(primary())
                    .corner_radius(20.0)
                    .main_align(Alignment::Center)
                    .cross_align(Alignment::Center)
                    .on_press(move |_| {
                        app.write_unchecked().toggle_play();
                    })
                    .child(label().text(glyph).font_size(15.0).font_weight(700).color((255, 255, 255)))
            })
            .child(ctrl_btn("next", "▶|", false, self.app, self.ui, move |a: &mut AppState| a.next()))
            .child(ctrl_btn(
                "rep",
                match repeat {
                    RepeatMode::One => "↻¹",
                    _ => "↻",
                },
                repeat != RepeatMode::Off,
                self.app,
                self.ui,
                move |a: &mut AppState| a.cycle_repeat(),
            ))
            .child(small_muted(mmss(pos as u32)))
            .child({
                let app2 = app;
                rect()
                    .width(Size::flex(1.0))
                    .child(Slider::new(move |v: f64| {
                        let d = app2.peek().player.duration_sec();
                        if d > 0.0 {
                            yt::seek_to(&app2, v / 100.0 * d);
                        }
                    }).value(seek_val))
            })
            .child(small_muted(mmss(dur as u32)))
            .child({
                let glyph = if liked { "♥" } else { "♡" };
                let col = if liked { primary() } else { muted_fg() };
                rect()
                    .padding(Gaps::new_all(6.0))
                    .on_press(move |_| {
                        if let Some(id) = tid {
                            app.write_unchecked().toggle_like(id);
                        }
                    })
                    .child(label().text(glyph).font_size(16.0).color(col))
            })
            .child(
                rect()
                    .padding(Gaps::new_all(6.0))
                    .background(if qopen { card() } else { c(DARK.popover) })
                    .corner_radius(8.0)
                    .on_press(move |_| {
                        let v = !ui.peek().queue_open;
                        ui.write_unchecked().queue_open = v;
                    })
                    .child(label().text("☰").font_size(16.0).color(if qopen { primary() } else { muted_fg() }))
            )
            .child(
                rect()
                    .padding(Gaps::new_all(6.0))
                    .on_press(move |_| {
                        app.write_unchecked().toggle_mute();
                    })
                    .child(label().text(if muted { "♪̶" } else { "♪" }).font_size(16.0).color(muted_fg()))
            )
            .child({
                let app3 = app;
                rect()
                    .width(Size::px(100.0))
                    .child(Slider::new(move |v: f64| {
                        app3.write_unchecked().set_volume((v / 100.0) as f32);
                    }).value(if muted { 0.0 } else { (vol * 100.0) as f64 }))
            })
            .into()
    }
}

fn ctrl_btn(
    id: &str,
    glyph: &str,
    active: bool,
    app: State<AppState>,
    ui: State<Ui>,
    f: impl FnMut(&mut AppState) + 'static,
) -> Element {
    let hid = format!("pb-{id}");
    let hid2 = hid.clone();
    let hov = ui.peek().hover.as_deref() == Some(hid.as_str());
    let mut f = f;
    rect()
        .padding(Gaps::new_all(8.0))
        .background(if hov { card() } else { bg() })
        .corner_radius(8.0)
        .on_pointer_enter(move |_| set_hover(ui, &hid))
        .on_pointer_leave(move |_| clear_hover(ui, &hid2))
        .on_press(move |_| {
            f(&mut app.write_unchecked());
        })
        .child(label().text(glyph.to_string()).font_size(15.0).color(if active { primary() } else { muted_fg() }))
        .into()
}

/// Animated 3-bar playing indicator, driven by the per-frame counter.
fn bars(frame: u64) -> Element {
    let h = |i: u64| 4.0 + ((frame as f32 * 0.25 + i as f32 * 1.4).sin() * 0.5 + 0.5) * 10.0;
    rect()
        .direction(Direction::Horizontal)
        .cross_align(Alignment::End)
        .spacing(2.0)
        .child(rect().width(Size::px(3.0)).height(Size::px(h(0))).background(primary()))
        .child(rect().width(Size::px(3.0)).height(Size::px(h(1))).background(primary()))
        .child(rect().width(Size::px(3.0)).height(Size::px(h(2))).background(primary()))
        .into()
}

// ---------------------------------------------------------------------------
// Track row (52px) — shared by the virtualized Songs list and plain lists
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn track_row(app: State<AppState>, ui: State<Ui>, g: &AppState, id: TrackId, index: usize) -> Element {
    let tv = g.library.track_view(id);
    let title = tv.title.to_string();
    let artist = tv.artist.to_string();
    let dur = tv.duration_sec;
    let explicit = tv.explicit;
    let liked = tv.liked;
    let t = g.library.track(id);
    let seed = AppState::album_seed(t.album_id);
    let art_url = t.artwork_url.clone();
    let is_current = g.current == Some(id);
    let playing = is_current && g.playing;
    let selected = g.selection == Some(index);
    let frame = ui.peek().frame;
    let hid = format!("tr-{id}-{index}");
    let hid2 = hid.clone();
    let hov = ui.peek().hover.as_deref() == Some(hid.as_str());
    let bgc = if playing {
        c(theme::state::row_playing())
    } else if selected {
        c(theme::state::row_selected())
    } else if hov {
        c(theme::state::ROW_HOVER)
    } else {
        bg()
    };
    rect()
        .width(Size::fill())
        .height(Size::px(52.0))
        .background(bgc)
        .corner_radius(8.0)
        .direction(Direction::Horizontal)
        .cross_align(Alignment::Center)
        .padding(Gaps::new_symmetric(0.0, 12.0))
        .spacing(12.0)
        .on_pointer_enter(move |_| set_hover(ui, &hid))
        .on_pointer_leave(move |_| clear_hover(ui, &hid2))
        .on_press(move |_| {
            let ids = app.peek().list_ids();
            if index < ids.len() {
                app.write_unchecked().selection = Some(index);
                app.write_unchecked().play_from(&ids, index);
            }
        })
        .child(if playing {
            bars(frame)
        } else {
            label()
                .text(format!("{}", index + 1))
                .font_size(12.0)
                .color(muted_fg())
                .width(Size::px(24.0))
                .into()
        })
        .child(tile(app, art_url.as_deref(), seed, 40.0, 4.0))
        .child(
            rect()
                .width(Size::flex(1.0))
                .direction(Direction::Horizontal)
                .cross_align(Alignment::Center)
                .spacing(8.0)
                .child(label().text(title).font_size(14.0).font_weight(500).color(fg()).max_lines(1))
                .child(if explicit {
                    rect()
                        .padding(Gaps::new_symmetric(1.0, 5.0))
                        .background(card())
                        .corner_radius(4.0)
                        .child(label().text("E").font_size(10.0).color(muted_fg()))
                } else {
                    rect().width(Size::px(1.0)).height(Size::px(1.0))
                })
                .child(
                    rect()
                        .width(Size::flex(1.0))
                        .child(small_muted(artist)),
                ),
        )
        .child(if liked {
            label().text("♥").font_size(14.0).color(primary())
        } else {
            label().text(mmss(dur)).font_size(12.0).color(muted_fg())
        })
        .into()
}

// Silence unused-import warnings if theme constants shift.
#[allow(dead_code)]
fn _use_theme_consts() {
    let _ = (BODY_MED, CARD_W, CARD_H, PLAYER_BAR_H, QUEUE_W, SIDEBAR_W, TITLEBAR_H, SMALL);
}
