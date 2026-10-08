//! Zuno — vizia implementation of the native-GUI benchmark (gui/vizia branch).
//!
//! Renders the shared `zuno_core::AppState` per docs/gui-benchmarks/SPEC.md.
//! The 5,000-row Songs list uses vizia's `VirtualList` (recycled rows); the
//! bench's 0..1 scroll fraction maps 1:1 onto the list's normalized `scroll_y`,
//! so the triangle-wave scroll phase is a hard jump with no easing — the
//! protocol's worst case, exactly as in the iced reference implementation.
//!
//! One self-contained source file (plus `icons` and the CSS in `style`),
//! per the hard-won lesson from the iced branch: single-file views, owned
//! strings at row-build time, signal-driven rebuilds everywhere else.

mod icons;
mod style;

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;
use vizia::prelude::*;
use vizia::vg;
use vizia::view::View as ViziaView;
use zuno_core::app::{LibraryTab, RepeatMode, View};
use zuno_core::bench::{BenchAction, BenchDriver};
use zuno_core::format::{count_songs, counts_line, listeners, mmss};
use zuno_core::model::*;
use zuno_core::queue::Region;
use zuno_core::search;
use zuno_core::theme as t;
use zuno_core::AppState;

thread_local! {
    /// The search textbox entity, so keymap handlers can focus it and detect
    /// when keystrokes should go to the field instead of the shortcuts.
    static SEARCH_ENTITY: Cell<Entity> = Cell::new(Entity::null());
    /// Ctrl-K was pressed while the field didn't exist yet; focus on rebuild.
    static FOCUS_PENDING: Cell<bool> = Cell::new(false);
    /// (seed,size) artwork covers already PNG-encoded and handed to vizia.
    static ART_LOADED: RefCell<HashSet<(u64, u32)>> = RefCell::new(HashSet::new());
}

// — UI-facing reactive state ————————————————————————————————————

/// Player-bar + row state, snapshot-cloned out of `AppState` whenever it
/// changes (never per frame — `position`/`duration` are separate signals).
#[derive(Clone, PartialEq)]
struct Transport {
    has_track: bool,
    title: String,
    artist: String,
    track_id: TrackId,
    album_id: AlbumId,
    artist_id: ArtistId,
    liked: bool,
    playing: bool,
    shuffle: bool,
    repeat: RepeatMode,
    muted: bool,
}

/// Every signal the views bind to. `Copy` so row/card builder closures can
/// capture the whole bundle cheaply.
#[derive(Clone, Copy)]
struct Sigs {
    view: Signal<View>,
    library_tab: Signal<LibraryTab>,
    search_query: Signal<String>,
    /// Track ids for the current view's main list (Songs = all 5,000,
    /// Search = filtered). The VirtualList binds this directly.
    list: Signal<Vec<TrackId>>,
    selection: Signal<Option<usize>>,
    /// Bench-driven 0..1 scroll fraction for the Songs VirtualList.
    scroll_frac: Signal<f32>,
    transport: Signal<Transport>,
    position: Signal<f32>,
    duration: Signal<f32>,
    vol: Signal<f32>,
    queue_open: Signal<bool>,
    queue_rev: Signal<u64>,
    /// Monotonic frame counter; only bumped while playing (drives the bars).
    frame: Signal<u64>,
    can_back: Signal<bool>,
    columns: Signal<usize>,
}

impl Sigs {
    fn new(app: &AppState) -> Self {
        Sigs {
            view: Signal::new(app.view.clone()),
            library_tab: Signal::new(app.library_tab),
            search_query: Signal::new(app.search_query.clone()),
            list: Signal::new(Vec::new()),
            selection: Signal::new(None),
            scroll_frac: Signal::new(0.0),
            transport: Signal::new(transport_of(app)),
            position: Signal::new(0.0),
            duration: Signal::new(0.0),
            vol: Signal::new(app.volume),
            queue_open: Signal::new(false),
            queue_rev: Signal::new(0),
            frame: Signal::new(0),
            can_back: Signal::new(false),
            columns: Signal::new(5),
        }
    }
}

// — Events ————————————————————————————————————————————————————————

#[derive(Clone, Debug)]
enum ZunoEvent {
    Tick,
    Nav(View),
    OpenAlbum(AlbumId),
    OpenArtist(ArtistId),
    OpenPlaylist(PlaylistId),
    SetLibraryTab(LibraryTab),
    SearchInput(String),
    Escape,
    /// Play `index` of the current view's list (resolves via `list_ids`).
    PlayFromCurrentList(usize),
    PlayFromList(Arc<Vec<TrackId>>, usize),
    PlayShuffledCurrentList,
    PlayShuffledList(Arc<Vec<TrackId>>),
    PlayTrack(TrackId),
    TogglePlay,
    Next,
    Previous,
    Seek(f64),
    SeekDelta(f64),
    SetVolume(f32),
    ToggleMute,
    ToggleShuffle,
    CycleRepeat,
    ToggleLike(TrackId),
    QueueToggle,
    QueueJump(usize),
    QueueRemove(usize),
    Back,
    WidthChanged(f32),
}

fn transport_of(app: &AppState) -> Transport {
    match app.current_track() {
        Some(tr) => Transport {
            has_track: true,
            title: tr.title.clone(),
            artist: app.library.artist(tr.artist_id).name.clone(),
            track_id: tr.id,
            album_id: tr.album_id,
            artist_id: tr.artist_id,
            liked: tr.liked,
            playing: app.playing,
            shuffle: app.shuffle,
            repeat: app.repeat,
            muted: app.muted,
        },
        None => Transport {
            has_track: false,
            title: "Nothing playing".to_string(),
            artist: "—".to_string(),
            track_id: 0,
            album_id: 0,
            artist_id: 0,
            liked: false,
            playing: false,
            shuffle: app.shuffle,
            repeat: app.repeat,
            muted: app.muted,
        },
    }
}

/// Push the app's list contents for the current view into the list signal.
/// `set_if_changed` keeps unrelated views from rebuilding.
fn sync_nav(app: &AppState, sig: &Sigs) {
    sig.view.set_if_changed(app.view.clone());
    sig.library_tab.set_if_changed(app.library_tab);
    sig.can_back.set_if_changed(app.can_go_back());
    sig.search_query.set_if_changed(app.search_query.clone());
    sig.selection.set_if_changed(app.selection);
    sig.list.set_if_changed(app.list_ids());
}

fn sync_transport(app: &AppState, sig: &Sigs) {
    sig.transport.set_if_changed(transport_of(app));
    sig.vol.set_if_changed(app.volume);
}

fn sync_queue(sig: &Sigs) {
    sig.queue_rev.update(|r| *r = r.wrapping_add(1));
}

fn sync_clock(app: &AppState, sig: &Sigs) {
    sig.position.set_if_changed(app.player.position_sec() as f32);
    sig.duration.set_if_changed(app.player.duration_sec() as f32);
}

// — Model ——————————————————————————————————————————————————————————

struct UiState {
    app: AppState,
    bench: Option<BenchDriver>,
    sig: Sigs,
    last_current: Option<TrackId>,
    last_playing: bool,
}

impl Model for UiState {
    fn event(&mut self, _cx: &mut EventContext, event: &mut Event) {
        event.map(|msg, _| match msg {
            ZunoEvent::Tick => {
                self.app.tick();
                if let Some(b) = self.bench.as_mut() {
                    match b.tick() {
                        BenchAction::RecordStartup | BenchAction::Nothing => {}
                        BenchAction::ScrollTo(f) => {
                            if self.app.view != View::Library
                                || self.app.library_tab != LibraryTab::Songs
                            {
                                self.app.view = View::Library;
                                self.app.library_tab = LibraryTab::Songs;
                                self.app.selection = None;
                                sync_nav(&self.app, &self.sig);
                            }
                            // Hard jump, no easing — the bench's worst case.
                            self.sig.scroll_frac.set(f);
                        }
                        BenchAction::SelectRow(i) => {
                            self.app.selection = Some(i);
                            self.sig.selection.set_if_changed(Some(i));
                        }
                        BenchAction::StartPlayback => {
                            let ids = self.app.list_ids();
                            self.app.play_from(&ids, ids.len() / 2);
                            sync_transport(&self.app, &self.sig);
                            sync_queue(&self.sig);
                            sync_clock(&self.app, &self.sig);
                        }
                        BenchAction::Finish => {
                            println!("{}", b.report);
                            std::process::exit(0);
                        }
                    }
                }
                if self.app.playing {
                    sync_clock(&self.app, &self.sig);
                    self.sig.frame.update(|f| *f = f.wrapping_add(1));
                }
                // Auto-advance / end-of-queue may have moved the current track.
                if self.app.current != self.last_current || self.app.playing != self.last_playing {
                    self.last_current = self.app.current;
                    self.last_playing = self.app.playing;
                    sync_transport(&self.app, &self.sig);
                    sync_queue(&self.sig);
                }
            }
            ZunoEvent::Nav(v) => {
                self.app.go(v.clone());
                sync_nav(&self.app, &self.sig);
            }
            ZunoEvent::OpenAlbum(id) => {
                self.app.open_album(*id);
                sync_nav(&self.app, &self.sig);
            }
            ZunoEvent::OpenArtist(id) => {
                self.app.open_artist(*id);
                sync_nav(&self.app, &self.sig);
            }
            ZunoEvent::OpenPlaylist(id) => {
                self.app.open_playlist(*id);
                sync_nav(&self.app, &self.sig);
            }
            ZunoEvent::SetLibraryTab(tab) => {
                self.app.set_library_tab(*tab);
                sync_nav(&self.app, &self.sig);
            }
            ZunoEvent::SearchInput(q) => {
                self.app.set_search(q);
                sync_nav(&self.app, &self.sig);
            }
            ZunoEvent::Escape => {
                if !self.app.search_query.is_empty() {
                    self.app.set_search("");
                } else {
                    self.app.go_back();
                }
                sync_nav(&self.app, &self.sig);
            }
            ZunoEvent::PlayFromCurrentList(index) => {
                let ids = self.app.list_ids();
                if !ids.is_empty() {
                    self.app.play_from(&ids, (*index).min(ids.len() - 1));
                    sync_transport(&self.app, &self.sig);
                    sync_queue(&self.sig);
                    sync_clock(&self.app, &self.sig);
                }
            }
            ZunoEvent::PlayFromList(list, index) => {
                if !list.is_empty() {
                    self.app.play_from(&list, (*index).min(list.len() - 1));
                    sync_transport(&self.app, &self.sig);
                    sync_queue(&self.sig);
                    sync_clock(&self.app, &self.sig);
                }
            }
            ZunoEvent::PlayShuffledCurrentList => {
                let ids = self.app.list_ids();
                self.play_shuffled(ids);
            }
            ZunoEvent::PlayShuffledList(list) => {
                self.play_shuffled(list.as_ref().clone());
            }
            ZunoEvent::PlayTrack(id) => {
                self.app.play_track(*id);
                sync_transport(&self.app, &self.sig);
                sync_queue(&self.sig);
                sync_clock(&self.app, &self.sig);
            }
            ZunoEvent::TogglePlay => {
                self.app.toggle_play();
                sync_transport(&self.app, &self.sig);
                sync_clock(&self.app, &self.sig);
            }
            ZunoEvent::Next => {
                self.app.next();
                sync_transport(&self.app, &self.sig);
                sync_queue(&self.sig);
                sync_clock(&self.app, &self.sig);
            }
            ZunoEvent::Previous => {
                self.app.previous();
                sync_transport(&self.app, &self.sig);
                sync_queue(&self.sig);
                sync_clock(&self.app, &self.sig);
            }
            ZunoEvent::Seek(sec) => {
                self.app.seek(*sec);
                sync_transport(&self.app, &self.sig);
                sync_clock(&self.app, &self.sig);
            }
            ZunoEvent::SeekDelta(delta) => {
                self.app.seek(self.app.player.position_sec() + *delta);
                sync_transport(&self.app, &self.sig);
                sync_clock(&self.app, &self.sig);
            }
            ZunoEvent::SetVolume(v) => {
                self.app.set_volume(*v);
                sync_transport(&self.app, &self.sig);
            }
            ZunoEvent::ToggleMute => {
                self.app.toggle_mute();
                sync_transport(&self.app, &self.sig);
            }
            ZunoEvent::ToggleShuffle => {
                self.app.toggle_shuffle();
                sync_transport(&self.app, &self.sig);
                sync_queue(&self.sig);
            }
            ZunoEvent::CycleRepeat => {
                self.app.cycle_repeat();
                sync_transport(&self.app, &self.sig);
            }
            ZunoEvent::ToggleLike(id) => {
                self.app.toggle_like(*id);
                sync_transport(&self.app, &self.sig);
                sync_nav(&self.app, &self.sig);
            }
            ZunoEvent::QueueToggle => {
                self.sig.queue_open.update(|o| *o = !*o);
            }
            ZunoEvent::QueueJump(index) => {
                if let Some(id) = self.app.queue.jump_to(*index) {
                    self.app.play_track(id);
                }
                sync_transport(&self.app, &self.sig);
                sync_queue(&self.sig);
                sync_clock(&self.app, &self.sig);
            }
            ZunoEvent::QueueRemove(index) => {
                self.app.queue.remove_at(*index);
                sync_queue(&self.sig);
            }
            ZunoEvent::Back => {
                self.app.go_back();
                sync_nav(&self.app, &self.sig);
            }
            ZunoEvent::WidthChanged(w) => {
                let n = (((*w - 2.0 * t::PAGE_PAD) / (t::CARD_W + 16.0)).floor() as usize).clamp(2, 8);
                self.sig.columns.set_if_changed(n);
            }
        });
    }
}

impl UiState {
    fn play_shuffled(&mut self, ids: Vec<TrackId>) {
        if ids.is_empty() {
            return;
        }
        self.app.play_from(&ids, 0);
        if !self.app.shuffle {
            self.app.toggle_shuffle();
        }
        sync_transport(&self.app, &self.sig);
        sync_queue(&self.sig);
        sync_clock(&self.app, &self.sig);
    }
}

// — Artwork bridge: core's RGBA LRU → PNG → vizia image store ————————————

fn encode_png(w: u32, h: u32, rgba: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, w, h);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.set_compression(png::Compression::Fast);
        let mut wr = enc.write_header().expect("png header");
        wr.write_image_data(rgba).expect("png data");
    }
    out
}

/// Ensure the (seed,size) cover exists in vizia's image store, return its name.
/// The decoded-bitmap budget stays in core's shared 96-cover LRU; vizia keeps
/// its own decoded copies alive for the app's lifetime (reported honestly).
fn ensure_art(cx: &mut Context, seed: u64, size: u32) -> String {
    let name = format!("art-{seed}-{size}");
    let fresh = ART_LOADED.with(|set| set.borrow_mut().insert((seed, size)));
    if fresh {
        let rgba = {
            let st = cx.data::<UiState>();
            st.app.artwork.get(seed, size)
        };
        let png = encode_png(size, size, &rgba);
        cx.load_image(&name, Box::leak(png.into_boxed_slice()), ImageRetentionPolicy::Forever);
    }
    name
}

fn art_for_album(cx: &mut Context, aid: AlbumId, size: u32) -> String {
    let seed = AppState::album_seed(aid);
    ensure_art(cx, seed, size)
}

fn art_for_playlist(cx: &mut Context, pid: PlaylistId, size: u32) -> String {
    let seed = AppState::playlist_seed(pid);
    ensure_art(cx, seed, size)
}

fn art_for_artist(cx: &mut Context, rid: ArtistId, size: u32) -> String {
    let seed = AppState::artist_seed(rid);
    ensure_art(cx, seed, size)
}

// — The playing-bars indicator (custom drawn view) ——————————————————

pub struct Bars {
    frame: Signal<u64>,
}

impl Bars {
    pub fn new(cx: &mut Context, frame: Signal<u64>) -> Handle<'_, Self> {
        Self { frame }
            .build(cx, |_| {})
            .bind(frame, |mut h| {
                h.needs_redraw();
            })
    }
}

impl ViziaView for Bars {
    fn element(&self) -> Option<&'static str> {
        Some("bars")
    }

    fn draw(&self, cx: &mut DrawContext, canvas: &Canvas) {
        let f = self.frame.get() as f32;
        let b = cx.bounds();
        let col = Color::rgb(255, 0, 51);
        for i in 0..3i32 {
            let phase = ((f * 0.11) + (i as f32 * 1.9)).sin() * 0.5 + 0.5;
            let bh = (b.h * (0.28 + 0.62 * phase)).max(2.0);
            let x = b.x + i as f32 * 5.0;
            let bb = BoundingBox { x, y: b.y + (b.h - bh) * 0.5, w: 3.0, h: bh };
            let rect: vg::Rect = bb.into();
            let path = vg::Path::rect(rect, None);
            let mut paint = vg::Paint::default();
            paint.set_color(col);
            canvas.draw_path(&path, &paint);
        }
    }
}

// — Keyboard ————————————————————————————————————————————————————————

#[derive(Clone, Copy, PartialEq, Debug)]
enum KeyAction {
    TogglePlay,
    SeekBack,
    SeekFwd,
    Escape,
    Mute,
    Queue,
    Shuffle,
    Repeat,
    FocusSearch,
}

/// True when the search textbox has keyboard focus — keystrokes belong to it.
fn typing(cx: &EventContext) -> bool {
    let e = SEARCH_ENTITY.with(|c| c.get());
    !e.is_null() && cx.focused() == e
}

fn ka_toggle_play(cx: &mut EventContext) {
    if !typing(cx) {
        cx.emit(ZunoEvent::TogglePlay);
    }
}
fn ka_seek_back(cx: &mut EventContext) {
    if !typing(cx) {
        cx.emit(ZunoEvent::SeekDelta(-10.0));
    }
}
fn ka_seek_fwd(cx: &mut EventContext) {
    if !typing(cx) {
        cx.emit(ZunoEvent::SeekDelta(10.0));
    }
}
fn ka_escape(cx: &mut EventContext) {
    cx.emit(ZunoEvent::Escape);
}
fn ka_mute(cx: &mut EventContext) {
    if !typing(cx) {
        cx.emit(ZunoEvent::ToggleMute);
    }
}
fn ka_queue(cx: &mut EventContext) {
    if !typing(cx) {
        cx.emit(ZunoEvent::QueueToggle);
    }
}
fn ka_shuffle(cx: &mut EventContext) {
    if !typing(cx) {
        cx.emit(ZunoEvent::ToggleShuffle);
    }
}
fn ka_repeat(cx: &mut EventContext) {
    if !typing(cx) {
        cx.emit(ZunoEvent::CycleRepeat);
    }
}
fn ka_focus_search(cx: &mut EventContext) {
    let on_search = cx.data::<UiState>().app.view == View::Search;
    if on_search {
        let e = SEARCH_ENTITY.with(|c| c.get());
        if !e.is_null() {
            cx.with_current(e, |cx| cx.focus());
        }
    } else {
        FOCUS_PENDING.with(|f| f.set(true));
        cx.emit(ZunoEvent::Nav(View::Search));
    }
}

fn build_keymap(cx: &mut Context) {
    Keymap::from(vec![
        (KeyChord::new(Modifiers::empty(), Code::Space), KeymapEntry::new(KeyAction::TogglePlay, ka_toggle_play)),
        (KeyChord::new(Modifiers::empty(), Code::ArrowLeft), KeymapEntry::new(KeyAction::SeekBack, ka_seek_back)),
        (KeyChord::new(Modifiers::empty(), Code::ArrowRight), KeymapEntry::new(KeyAction::SeekFwd, ka_seek_fwd)),
        (KeyChord::new(Modifiers::empty(), Code::Escape), KeymapEntry::new(KeyAction::Escape, ka_escape)),
        (KeyChord::new(Modifiers::empty(), Code::KeyM), KeymapEntry::new(KeyAction::Mute, ka_mute)),
        (KeyChord::new(Modifiers::empty(), Code::KeyQ), KeymapEntry::new(KeyAction::Queue, ka_queue)),
        (KeyChord::new(Modifiers::empty(), Code::KeyS), KeymapEntry::new(KeyAction::Shuffle, ka_shuffle)),
        (KeyChord::new(Modifiers::empty(), Code::KeyR), KeymapEntry::new(KeyAction::Repeat, ka_repeat)),
        (KeyChord::new(Modifiers::CTRL, Code::KeyK), KeymapEntry::new(KeyAction::FocusSearch, ka_focus_search)),
    ])
    .build(cx);
}

// — Shell ———————————————————————————————————————————————————————————

fn build_shell(cx: &mut Context) {
    let sig = cx.data::<UiState>().sig;
    VStack::new(cx, move |cx| {
        HStack::new(cx, move |cx| {
            sidebar(cx);
            ZStack::new(cx, move |cx| {
                VStack::new(cx, move |cx| {
                    Binding::new(cx, sig.view, move |cx| page(cx));
                })
                .class("content-col")
                .on_geo_changed(|cx, _geo| {
                    let w = cx.cache.get_width(cx.current());
                    cx.emit(ZunoEvent::WidthChanged(w));
                });
                Binding::new(cx, sig.queue_open, move |cx| {
                    let sig = cx.data::<UiState>().sig;
                    if sig.queue_open.get() {
                        queue_panel(cx);
                    }
                });
            })
            .width(Stretch(1.0))
            .height(Stretch(1.0));
        })
        .height(Stretch(1.0));
        player_bar(cx);
    })
    .class("app-root");
}

fn sidebar(cx: &mut Context) {
    let sig = cx.data::<UiState>().sig;
    let playlists: Vec<(PlaylistId, String, usize)> = {
        let st = cx.data::<UiState>();
        st.app
            .library
            .playlists
            .iter()
            .map(|p| (p.id, p.title.clone(), p.track_ids.len()))
            .collect()
    };
    VStack::new(cx, move |cx| {
        Label::new(cx, "Zuno").class("logo");
        nav_item(cx, "Home", icons::HOME, |v| matches!(v, View::Home), ZunoEvent::Nav(View::Home));
        nav_item(cx, "Search", icons::SEARCH, |v| matches!(v, View::Search), ZunoEvent::Nav(View::Search));
        nav_item(cx, "Library", icons::LIBRARY, |v| matches!(v, View::Library), ZunoEvent::Nav(View::Library));
        nav_item(cx, "Settings", icons::SETTINGS, |v| matches!(v, View::Settings), ZunoEvent::Nav(View::Settings));
        Label::new(cx, "PLAYLISTS").class("section-label");
        ScrollView::new(cx, move |cx| {
            VStack::new(cx, move |cx| {
                for (pid, title, count) in playlists {
                    playlist_nav_row(cx, pid, title, count);
                }
            })
            .class("track-list");
        })
        .class("playlist-scroll");
    })
    .class("sidebar");
    let _ = sig;
}

fn nav_item(
    cx: &mut Context,
    label: &'static str,
    icon: &'static [u8],
    active: impl Fn(&View) -> bool + Copy + 'static,
    msg: ZunoEvent,
) {
    let sig = cx.data::<UiState>().sig;
    Button::new(cx, move |cx| {
        HStack::new(cx, move |cx| {
            Svg::new(cx, icon).size(Pixels(19.0)).hoverable(false);
            Label::new(cx, label).hoverable(false);
        })
        .horizontal_gap(Pixels(12.0))
        .alignment(Alignment::Left)
        .hoverable(false)
    })
    .class("nav-item")
    .toggle_class("active", sig.view.map(active))
    .on_press(move |cx| cx.emit(msg.clone()));
}

fn playlist_nav_row(cx: &mut Context, pid: PlaylistId, title: String, count: usize) {
    let sig = cx.data::<UiState>().sig;
    let art = art_for_playlist(cx, pid, 40);
    let count_text = format!("{count} songs");
    Button::new(cx, move |cx| {
        HStack::new(cx, move |cx| {
            Image::new(cx, art).class("row-art").hoverable(false);
            VStack::new(cx, move |cx| {
                Label::new(cx, title).class("row-title").hoverable(false);
                Label::new(cx, count_text).class("row-artist").hoverable(false);
            })
            .class("row-text")
            .hoverable(false);
        })
        .horizontal_gap(Pixels(10.0))
        .alignment(Alignment::Left)
        .hoverable(false)
    })
    .class("playlist-row")
    .toggle_class("active", sig.view.map(move |v| matches!(v, View::Playlist(p) if *p == pid)))
    .on_press(move |cx| cx.emit(ZunoEvent::OpenPlaylist(pid)));
}

// — Page router —————————————————————————————————————————————————————

fn page(cx: &mut Context) {
    let sig = cx.data::<UiState>().sig;
    match sig.view.get() {
        View::Home => home_page(cx),
        View::Library => library_page(cx),
        View::Album(_) | View::Playlist(_) => collection_page(cx),
        View::Artist(_) => artist_page(cx),
        View::Search => search_page(cx),
        View::Settings => settings_page(cx),
    }
}

// — Home ————————————————————————————————————————————————————————————

fn home_page(cx: &mut Context) {
    let sig = cx.data::<UiState>().sig;
    let (recent, mixes, new_albums, artists): (Vec<AlbumId>, Vec<(PlaylistId, String, String)>, Vec<AlbumId>, Vec<ArtistId>) = {
        let st = cx.data::<UiState>();
        let lib = &st.app.library;
        let mut new_albums: Vec<AlbumId> = (0..lib.albums.len() as u32).collect();
        new_albums.sort_by_key(|&a| std::cmp::Reverse(lib.album(a).year));
        new_albums.truncate(24);
        let mut artists: Vec<ArtistId> = (0..lib.artists.len() as u32).collect();
        artists.sort_by_key(|&r| std::cmp::Reverse(lib.artist(r).monthly_listeners));
        artists.truncate(12);
        let mixes = lib
            .mixes
            .iter()
            .map(|m| (m.id, m.title.clone(), m.description.clone().unwrap_or_default()))
            .collect();
        (lib.home_recent_albums(12), mixes, new_albums, artists)
    };

    ScrollView::new(cx, move |cx| {
        VStack::new(cx, move |cx| {
            VStack::new(cx, move |cx| {
                Label::new(cx, "Home").class("h1");
                Label::new(cx, "Pick up where you left off").class("subtle");
            })
            .vertical_gap(Pixels(4.0));

            VStack::new(cx, move |cx| {
                Label::new(cx, "Recently played").class("h2");
                HStack::new(cx, move |cx| {
                    for aid in recent {
                        album_card(cx, aid);
                    }
                })
                .class("shelf-row");
            })
            .class("shelf");

            VStack::new(cx, move |cx| {
                Label::new(cx, "Made for you").class("h2");
                HStack::new(cx, move |cx| {
                    for (pid, title, desc) in mixes {
                        let art = art_for_playlist(cx, pid, 176);
                        media_card(cx, art, false, true, title, desc, ZunoEvent::OpenPlaylist(pid));
                    }
                })
                .class("shelf-row");
            })
            .class("shelf");

            VStack::new(cx, move |cx| {
                Label::new(cx, "New albums").class("h2");
                album_grid(cx, sig, new_albums);
            })
            .class("shelf");

            VStack::new(cx, move |cx| {
                Label::new(cx, "Popular artists").class("h2");
                HStack::new(cx, move |cx| {
                    for rid in artists {
                        let (name, listeners) = {
                            let st = cx.data::<UiState>();
                            let a = st.app.library.artist(rid);
                            (a.name.clone(), listeners(a.monthly_listeners))
                        };
                        let art = art_for_artist(cx, rid, 176);
                        media_card(cx, art, true, false, name, listeners, ZunoEvent::OpenArtist(rid));
                    }
                })
                .class("shelf-row");
            })
            .class("shelf");
        })
        .class("page-body");
    })
    .class("page-scroll");
}

/// Responsive album grid bound to the column count.
fn album_grid(cx: &mut Context, sig: Sigs, ids: Vec<AlbumId>) {
    let sig_inner = sig;
    Binding::new(cx, sig.columns, move |cx| {
        let cols = sig_inner.columns.get().max(1);
        let nrows = (ids.len() + cols - 1) / cols;
        Grid::new(
            cx,
            vec![Stretch(1.0); cols],
            vec![Pixels(232.0); nrows.max(1)],
            |cx| {
                for (i, aid) in ids.iter().enumerate() {
                    album_card(cx, *aid).column_start(i % cols).row_start(i / cols);
                }
            },
        )
        .width(Stretch(1.0));
    });
}

fn album_card(cx: &mut Context, aid: AlbumId) -> Handle<'_, Button> {
    let (title, sub) = {
        let st = cx.data::<UiState>();
        let a = st.app.library.album(aid);
        let artist = st.app.library.artist(a.artist_id);
        (a.title.clone(), format!("{} · {}", artist.name, a.year))
    };
    let art = art_for_album(cx, aid, 176);
    media_card(cx, art, false, true, title, sub, ZunoEvent::OpenAlbum(aid))
}

/// The AlbumCard from the spec: square (or round) art, hover scrim with a red
/// play pill, 2-line title, muted subtitle.
#[allow(clippy::too_many_arguments)]
fn media_card(
    cx: &mut Context,
    art: String,
    round: bool,
    scrim: bool,
    title: String,
    sub: String,
    msg: ZunoEvent,
) -> Handle<'_, Button> {
    let msg_inner = msg.clone();
    Button::new(cx, move |cx| {
        VStack::new(cx, move |cx| {
            // Sized via modifiers (entity style), not CSS classes — the default
            // theme's `image { size: auto }` wins over class rules in some
            // containers, but entity-level style always wins.
            let mut img = Image::new(cx, art)
                .class("card-art")
                .width(Pixels(176.0))
                .height(Pixels(176.0));
            if round {
                img = img.corner_radius(Percentage(50.0));
            } else {
                img = img.corner_radius(Pixels(4.0));
            }
            img.hoverable(false);
            if scrim {
                let pill_msg = msg_inner.clone();
                VStack::new(cx, move |cx| {
                    Button::new(cx, move |cx| Svg::new(cx, icons::PLAY).size(Pixels(20.0)))
                        .class("play-pill")
                        .width(Pixels(44.0))
                        .height(Pixels(44.0))
                        .on_press(move |cx| cx.emit(pill_msg.clone()));
                })
                .class("scrim")
                .width(Pixels(176.0))
                .height(Pixels(176.0))
                .hoverable(false);
            }
            Label::new(cx, title).class("card-title").hoverable(false);
            Label::new(cx, sub).class("card-sub").hoverable(false);
        })
        .vertical_gap(Pixels(8.0))
        .height(Pixels(224.0))
        .hoverable(false)
    })
    .class("card")
    .on_press(move |cx| cx.emit(msg.clone()))
}

// — Library (home of the 5,000-row list) ———————————————————————————

fn library_page(cx: &mut Context) {
    let sig = cx.data::<UiState>().sig;
    VStack::new(cx, move |cx| {
        HStack::new(cx, move |cx| {
            Binding::new(cx, sig.can_back, move |cx| {
                let s = cx.data::<UiState>().sig;
                if s.can_back.get() {
                    Button::new(cx, move |cx| Svg::new(cx, icons::BACK).size(Pixels(18.0)))
                        .class("back-btn")
                        .on_press(|cx| cx.emit(ZunoEvent::Back));
                }
            });
            Label::new(cx, "Your Library").class("h1");
        })
        .class("page-header");

        HStack::new(cx, move |cx| {
            tab_button(cx, "Playlists", LibraryTab::Playlists);
            tab_button(cx, "Albums", LibraryTab::Albums);
            tab_button(cx, "Artists", LibraryTab::Artists);
            tab_button(cx, "Songs", LibraryTab::Songs);
        })
        .class("tab-bar");

        Binding::new(cx, sig.library_tab, move |cx| {
            let sig = cx.data::<UiState>().sig;
            match sig.library_tab.get() {
                LibraryTab::Songs => songs_list(cx, sig),
                LibraryTab::Albums => albums_tab(cx, sig),
                LibraryTab::Artists => artists_tab(cx),
                LibraryTab::Playlists => playlists_tab(cx),
            }
        });
    })
    .class("page")
    .vertical_gap(Pixels(16.0));
}

/// THE list: all 5,000 songs in a recycled VirtualList. `scroll_y` takes the
/// bench's 0..1 fraction directly — a hard jump every frame, no easing.
fn songs_list(cx: &mut Context, sig: Sigs) {
    VirtualList::new(
        cx,
        sig.list,
        t::ROW_H,
        move |cx, index, item: Memo<TrackId>| track_row_button(cx, sig, index, item.get()),
    )
    .class("virtual-list")
    .scroll_y(sig.scroll_frac);
}

fn albums_tab(cx: &mut Context, sig: Sigs) {
    let ids: Vec<AlbumId> = {
        let st = cx.data::<UiState>();
        (0..st.app.library.albums.len() as u32).collect()
    };
    ScrollView::new(cx, move |cx| {
        VStack::new(cx, move |cx| {
            album_grid(cx, sig, ids);
        })
        .class("page-body");
    })
    .class("page-scroll");
}

fn artists_tab(cx: &mut Context) {
    let rows: Vec<(ArtistId, String, u32)> = {
        let st = cx.data::<UiState>();
        st.app
            .library
            .artists
            .iter()
            .map(|a| (a.id, a.name.clone(), a.monthly_listeners))
            .collect()
    };
    ScrollView::new(cx, move |cx| {
        VStack::new(cx, move |cx| {
            for (rid, name, monthly) in rows {
                let art = art_for_artist(cx, rid, 44);
                let listeners_text = listeners(monthly);
                Button::new(cx, move |cx| {
                    HStack::new(cx, move |cx| {
                        Image::new(cx, art)
                            .class("list-row-art")
                            .corner_radius(Percentage(50.0))
                            .hoverable(false);
                        VStack::new(cx, move |cx| {
                            Label::new(cx, name).class("row-title").hoverable(false);
                            Label::new(cx, listeners_text).class("row-artist").hoverable(false);
                        })
                        .class("row-text")
                        .hoverable(false);
                        Element::new(cx).width(Stretch(1.0)).hoverable(false);
                    })
                    .horizontal_gap(Pixels(12.0))
                    .alignment(Alignment::Left)
                    .hoverable(false)
                })
                .class("list-row")
                .on_press(move |cx| cx.emit(ZunoEvent::OpenArtist(rid)));
            }
        })
        .class("track-list");
    })
    .class("page-scroll");
}

fn playlists_tab(cx: &mut Context) {
    let rows: Vec<(PlaylistId, String, usize)> = {
        let st = cx.data::<UiState>();
        st.app
            .library
            .playlists
            .iter()
            .map(|p| (p.id, p.title.clone(), p.track_ids.len()))
            .collect()
    };
    ScrollView::new(cx, move |cx| {
        VStack::new(cx, move |cx| {
            for (pid, title, count) in rows {
                let art = art_for_playlist(cx, pid, 44);
                let meta = format!("{} · playlist", count_songs(count));
                Button::new(cx, move |cx| {
                    HStack::new(cx, move |cx| {
                        Image::new(cx, art).class("list-row-art").hoverable(false);
                        VStack::new(cx, move |cx| {
                            Label::new(cx, title).class("row-title").hoverable(false);
                            Label::new(cx, meta).class("row-artist").hoverable(false);
                        })
                        .class("row-text")
                        .hoverable(false);
                        Element::new(cx).width(Stretch(1.0)).hoverable(false);
                    })
                    .horizontal_gap(Pixels(12.0))
                    .alignment(Alignment::Left)
                    .hoverable(false)
                })
                .class("list-row")
                .on_press(move |cx| cx.emit(ZunoEvent::OpenPlaylist(pid)));
            }
        })
        .class("track-list");
    })
    .class("page-scroll");
}

fn tab_button(cx: &mut Context, label: &'static str, tab: LibraryTab) {
    let sig = cx.data::<UiState>().sig;
    Button::new(cx, move |cx| Label::new(cx, label))
        .class("tab-pill")
        .toggle_class("active", sig.library_tab.map(move |t| *t == tab))
        .on_press(move |cx| cx.emit(ZunoEvent::SetLibraryTab(tab)));
}

// — Album / Playlist pages ——————————————————————————————————————————

fn collection_page(cx: &mut Context) {
    let sig = cx.data::<UiState>().sig;
    let view = sig.view.get();
    let (seed, title, sub, ids): (u64, String, String, Vec<TrackId>) = match view {
        View::Album(aid) => {
            let st = cx.data::<UiState>();
            let a = st.app.library.album(aid);
            let artist = st.app.library.artist(a.artist_id).name.clone();
            let sub = format!("{} · {}", a.kind.label(), artist);
            let ids = a.track_ids.clone();
            (AppState::album_seed(aid), a.title.clone(), sub, ids)
        }
        View::Playlist(pid) => {
            let st = cx.data::<UiState>();
            let p = st.app.library.playlist(pid).expect("playlist exists");
            let sub = format!("Playlist · {}", p.description.clone().unwrap_or_else(|| "Curated by you".into()));
            let ids = p.track_ids.clone();
            (AppState::playlist_seed(pid), p.title.clone(), sub, ids)
        }
        _ => return,
    };
    // The hero art is requested at 232px per the spec.
    let hero_art = ensure_art(cx, seed, 232);
    let counts = {
        let st = cx.data::<UiState>();
        counts_line(ids.iter().map(|&tid| st.app.library.track(tid).duration_sec))
    };

    ScrollView::new(cx, move |cx| {
        VStack::new(cx, move |cx| {
            HStack::new(cx, move |cx| {
                Image::new(cx, hero_art)
                    .class("hero-art")
                    .width(Pixels(232.0))
                    .height(Pixels(232.0))
                    .hoverable(false);
                VStack::new(cx, move |cx| {
                    Label::new(cx, sub).class("hero-meta");
                    Label::new(cx, title).class("h1");
                    Label::new(cx, counts).class("hero-meta");
                    Element::new(cx).height(Stretch(1.0)).hoverable(false);
                    HStack::new(cx, move |cx| {
                        pill_button(cx, "Play", icons::PLAY, true, ZunoEvent::PlayFromCurrentList(0));
                        pill_button(
                            cx,
                            "Shuffle",
                            icons::SHUFFLE,
                            false,
                            ZunoEvent::PlayShuffledCurrentList,
                        );
                    })
                    .class("pill-bar");
                })
                .class("hero-col")
                .height(Pixels(232.0));
            })
            .class("hero");

            VStack::new(cx, move |cx| {
                for (i, id) in ids.iter().enumerate() {
                    track_row_button(cx, sig, i, *id);
                }
            })
            .class("track-list");
        })
        .class("page-body")
        .vertical_gap(Pixels(16.0));
    })
    .class("page-scroll");
}

fn pill_button(cx: &mut Context, label: &'static str, icon: &'static [u8], primary: bool, msg: ZunoEvent) {
    Button::new(cx, move |cx| {
        HStack::new(cx, move |cx| {
            Svg::new(cx, icon).size(Pixels(16.0)).hoverable(false);
            Label::new(cx, label).hoverable(false);
        })
        .horizontal_gap(Pixels(8.0))
        .alignment(Alignment::Center)
        .hoverable(false)
    })
    .class(if primary { "pill-primary" } else { "pill-ghost" })
    .on_press(move |cx| cx.emit(msg.clone()));
}

// — Artist page ————————————————————————————————————————————————————

fn artist_page(cx: &mut Context) {
    let sig = cx.data::<UiState>().sig;
    let View::Artist(rid) = sig.view.get() else { return };
    let (name, monthly, album_ids, top): (String, u32, Vec<AlbumId>, Vec<TrackId>) = {
        let st = cx.data::<UiState>();
        let a = st.app.library.artist(rid);
        let top: Vec<TrackId> = st
            .app
            .library
            .albums
            .iter()
            .find(|al| al.artist_id == rid)
            .map(|al| al.track_ids.iter().rev().take(10).copied().collect())
            .unwrap_or_default();
        (a.name.clone(), a.monthly_listeners, a.album_ids.clone(), top)
    };
    let hero_art = art_for_artist(cx, rid, 232);
    let listeners_text = listeners(monthly);
    let top_arc = Arc::new(top.clone());
    let hero_play = ZunoEvent::PlayFromList(top_arc.clone(), 0);
    let hero_shuffle = ZunoEvent::PlayShuffledList(top_arc.clone());
    let top_arc_rows = top_arc.clone();
    let counts = {
        let st = cx.data::<UiState>();
        counts_line(top.iter().map(|&tid| st.app.library.track(tid).duration_sec))
    };

    ScrollView::new(cx, move |cx| {
        VStack::new(cx, move |cx| {
            HStack::new(cx, move |cx| {
                Image::new(cx, hero_art)
                    .class("hero-art")
                    .width(Pixels(232.0))
                    .height(Pixels(232.0))
                    .corner_radius(Percentage(50.0))
                    .hoverable(false);
                VStack::new(cx, move |cx| {
                    Label::new(cx, "Artist").class("hero-meta");
                    Label::new(cx, name).class("h1");
                    Label::new(cx, listeners_text).class("hero-meta");
                    Label::new(cx, counts).class("hero-meta");
                    Element::new(cx).height(Stretch(1.0)).hoverable(false);
                    HStack::new(cx, move |cx| {
                        pill_button(cx, "Play", icons::PLAY, true, hero_play);
                        pill_button(cx, "Shuffle", icons::SHUFFLE, false, hero_shuffle);
                    })
                    .class("pill-bar");
                })
                .class("hero-col")
                .height(Pixels(232.0));
            })
            .class("hero");

            if !top.is_empty() {
                VStack::new(cx, move |cx| {
                    Label::new(cx, "Top tracks").class("h2");
                    VStack::new(cx, move |cx| {
                        for (i, id) in top.iter().enumerate() {
                            track_row_button_msg(
                                cx,
                                sig,
                                i,
                                *id,
                                ZunoEvent::PlayFromList(top_arc_rows.clone(), i),
                            );
                        }
                    })
                    .class("track-list");
                })
                .class("shelf");
            }

            VStack::new(cx, move |cx| {
                Label::new(cx, "Albums").class("h2");
                album_grid(cx, sig, album_ids);
            })
            .class("shelf");
        })
        .class("page-body")
        .vertical_gap(Pixels(16.0));
    })
    .class("page-scroll");
}

// — Search ——————————————————————————————————————————————————————————

fn search_page(cx: &mut Context) {
    let sig = cx.data::<UiState>().sig;
    VStack::new(cx, move |cx| {
        let tb = Textbox::new(cx, sig.search_query)
            .class("search-box")
            .placeholder("Search songs, albums, artists…")
            .on_edit(|cx, text| cx.emit(ZunoEvent::SearchInput(text)));
        let tb_entity = tb.entity();
        SEARCH_ENTITY.with(|s| s.set(tb_entity));
        if FOCUS_PENDING.with(|f| f.get()) {
            FOCUS_PENDING.with(|f| f.set(false));
            EventContext::new(cx).with_current(tb_entity, |cx| cx.focus());
        }

        Binding::new(cx, sig.search_query, move |cx| {
            let sig = cx.data::<UiState>().sig;
            let q = sig.search_query.get();
            if q.trim().is_empty() {
                Label::new(cx, "Search Zuno").class("empty-hint");
                return;
            }
            let results = {
                let st = cx.data::<UiState>();
                search::search(&st.app.library, &q)
            };
            if results.tracks.is_empty() {
                Label::new(cx, format!("No results for “{q}”")).class("empty-hint");
                return;
            }
            VStack::new(cx, move |cx| {
                VStack::new(cx, move |cx| {
                    Label::new(cx, "Top result").class("h2");
                    let top_id = results.tracks[0];
                    let (title, artist, aid) = {
                        let st = cx.data::<UiState>();
                        let v = st.app.library.track_view(top_id);
                        let aid = st.app.library.track(top_id).album_id;
                        (v.title.to_string(), v.artist.to_string(), aid)
                    };
                    let art = art_for_album(cx, aid, 176);
                    media_card(
                        cx,
                        art,
                        false,
                        true,
                        title,
                        format!("Song · {artist}"),
                        ZunoEvent::PlayTrack(top_id),
                    );
                })
                .class("shelf");
                VStack::new(cx, move |cx| {
                    Label::new(cx, "Songs").class("h2");
                    VirtualList::new(
                        cx,
                        sig.list,
                        t::ROW_H,
                        move |cx, index, item: Memo<TrackId>| {
                            track_row_button(cx, sig, index, item.get())
                        },
                    )
                    .class("virtual-list")
                    .height(Pixels(400.0));
                })
                .class("shelf");
                if !results.albums.is_empty() {
                    VStack::new(cx, move |cx| {
                        Label::new(cx, "Albums").class("h2");
                        HStack::new(cx, move |cx| {
                            for aid in results.albums.iter().take(6) {
                                album_card(cx, *aid);
                            }
                        })
                        .class("shelf-row");
                    })
                    .class("shelf");
                }
                if !results.artists.is_empty() {
                    VStack::new(cx, move |cx| {
                        Label::new(cx, "Artists").class("h2");
                        HStack::new(cx, move |cx| {
                            for rid in results.artists.iter().take(6) {
                                let (name, monthly) = {
                                    let st = cx.data::<UiState>();
                                    let a = st.app.library.artist(*rid);
                                    (a.name.clone(), a.monthly_listeners)
                                };
                                let art = art_for_artist(cx, *rid, 176);
                                media_card(
                                    cx,
                                    art,
                                    true,
                                    false,
                                    name,
                                    listeners(monthly),
                                    ZunoEvent::OpenArtist(*rid),
                                );
                            }
                        })
                        .class("shelf-row");
                    })
                    .class("shelf");
                }
            })
            .class("page-body")
            .vertical_gap(Pixels(16.0));
        });
    })
    .class("page")
    .vertical_gap(Pixels(16.0));
}

// — Settings ——————————————————————————————————————————————————————————

fn settings_page(cx: &mut Context) {
    let sig = cx.data::<UiState>().sig;
    ScrollView::new(cx, move |cx| {
        VStack::new(cx, move |cx| {
            HStack::new(cx, move |cx| {
                Binding::new(cx, sig.can_back, move |cx| {
                    let s = cx.data::<UiState>().sig;
                    if s.can_back.get() {
                        Button::new(cx, move |cx| Svg::new(cx, icons::BACK).size(Pixels(18.0)))
                            .class("back-btn")
                            .on_press(|cx| cx.emit(ZunoEvent::Back));
                    }
                });
                Label::new(cx, "Settings").class("h1");
            })
            .class("page-header");

            settings_section(cx, "Appearance", &[
                ("Theme", "Dark (pinned)"),
                ("Accent", "#FF0033"),
            ]);
            settings_section(cx, "Playback", &[
                ("Streaming quality", "High (256 kbps)"),
                ("Download quality", "High (256 kbps)"),
                ("Gapless playback", "On"),
                ("Crossfade", "Off"),
                ("Audio engine", "Rust (cpal) — no WebView"),
            ]);
            settings_section(cx, "Library", &[
                ("Cache", "4 GB"),
                ("Downloads ceiling", "8 GB"),
            ]);
        })
        .class("page-body");
    })
    .class("page-scroll");
}

fn settings_section(cx: &mut Context, title: &'static str, rows: &[(&'static str, &'static str)]) {
    VStack::new(cx, move |cx| {
        Label::new(cx, title).class("settings-title");
        for (key, val) in rows {
            HStack::new(cx, move |cx| {
                Label::new(cx, *key).class("settings-key");
                Label::new(cx, *val).class("settings-val");
            })
            .class("settings-row");
        }
    })
    .class("settings-card");
}

// — Queue panel ——————————————————————————————————————————————————————

#[derive(Clone)]
struct QueueRowOwned {
    region: Region,
    title: String,
    artist: String,
    dur: u32,
    album_id: AlbumId,
}

fn queue_panel(cx: &mut Context) {
    VStack::new(cx, move |cx| {
        HStack::new(cx, move |cx| {
            Label::new(cx, "Queue").class("h3");
            Element::new(cx).width(Stretch(1.0)).hoverable(false);
            Button::new(cx, move |cx| Svg::new(cx, icons::CLOSE).size(Pixels(14.0)))
                .class("queue-x")
                .on_press(|cx| cx.emit(ZunoEvent::QueueToggle));
        })
        .class("queue-head");

        let queue_rev = cx.data::<UiState>().sig.queue_rev;
        ScrollView::new(cx, move |cx| {
            Binding::new(cx, queue_rev, move |cx| {
                let rows: Vec<QueueRowOwned> = {
                    let st = cx.data::<UiState>();
                    st.app
                        .queue_rows()
                        .into_iter()
                        .map(|(id, region)| {
                            let v = st.app.library.track_view(id);
                            let album_id = st.app.library.track(id).album_id;
                            QueueRowOwned {
                                region,
                                title: v.title.to_string(),
                                artist: v.artist.to_string(),
                                dur: v.duration_sec,
                                album_id,
                            }
                        })
                        .collect()
                };
                VStack::new(cx, move |cx| {
                    let mut last_region = Region::Played;
                    for (i, row) in rows.iter().enumerate() {
                        if row.region != last_region {
                            let label = match row.region {
                                Region::Current => Some("NOW PLAYING"),
                                Region::Manual => Some("NEXT IN QUEUE"),
                                Region::Automatic => Some("NEXT UP"),
                                Region::Played => None,
                            };
                            if let Some(l) = label {
                                Label::new(cx, l).class("queue-label");
                            }
                            last_region = row.region;
                        }
                        queue_row(cx, i, row);
                    }
                })
                .class("queue-list");
            });
        })
        .class("queue-scroll");
    })
    .class("queue-panel");
}

fn queue_row(cx: &mut Context, index: usize, row: &QueueRowOwned) {
    let QueueRowOwned { region, title, artist, dur, album_id, .. } = row.clone();
    let art = art_for_album(cx, album_id, 32);
    let is_current = region == Region::Current;
    let dur_text = mmss(dur);
    Button::new(cx, move |cx| {
        HStack::new(cx, move |cx| {
            Image::new(cx, art).class("queue-art").hoverable(false);
            VStack::new(cx, move |cx| {
                Label::new(cx, title).class("row-title").hoverable(false);
                Label::new(cx, artist).class("row-artist").hoverable(false);
            })
            .class("row-text")
            .hoverable(false);
            Element::new(cx).width(Stretch(1.0)).hoverable(false);
            Label::new(cx, dur_text).class("row-artist").hoverable(false);
            if !is_current {
                Button::new(cx, move |cx| Svg::new(cx, icons::CLOSE).size(Pixels(11.0)))
                    .class("queue-x")
                    .on_press(move |cx| cx.emit(ZunoEvent::QueueRemove(index)));
            }
        })
        .horizontal_gap(Pixels(10.0))
        .alignment(Alignment::Left)
        .hoverable(false)
    })
    .class("queue-row")
    .toggle_class("current", is_current)
    .on_press(move |cx| cx.emit(ZunoEvent::QueueJump(index)));
}

// — Player bar ———————————————————————————————————————————————————————

fn player_bar(cx: &mut Context) {
    let sig = cx.data::<UiState>().sig;
    HStack::new(cx, move |cx| {
        // Left: artwork + title/artist (rebuilds only when transport changes)
        Binding::new(cx, sig.transport, move |cx| {
            let sig = cx.data::<UiState>().sig;
            let tr = sig.transport.get();
            let art = if tr.has_track { art_for_album(cx, tr.album_id, 40) } else { String::new() };
            let (title, artist) = (tr.title.clone(), tr.artist.clone());
            let (album_id, artist_id) = (tr.album_id, tr.artist_id);
            HStack::new(cx, move |cx| {
                if !art.is_empty() {
                    Image::new(cx, art).class("pb-art").hoverable(false);
                }
                VStack::new(cx, move |cx| {
                    Button::new(cx, move |cx| Label::new(cx, title).class("pb-title"))
                        .on_press(move |cx| cx.emit(ZunoEvent::OpenAlbum(album_id)));
                    Button::new(cx, move |cx| Label::new(cx, artist).class("pb-artist"))
                        .on_press(move |cx| cx.emit(ZunoEvent::OpenArtist(artist_id)));
                })
                .class("pb-text")
                .hoverable(false);
            })
            .class("pb-info")
            .alignment(Alignment::Left);
        });

        // Center: transport + seekbar. The seek slider/time labels bind the
        // per-frame position/duration signals, OUTSIDE the transport binding,
        // so they update without rebuilding the bar.
        VStack::new(cx, move |cx| {
            Binding::new(cx, sig.transport, move |cx| {
                let sig = cx.data::<UiState>().sig;
                let tr = sig.transport.get();
                HStack::new(cx, move |cx| {
                    Button::new(cx, move |cx| Svg::new(cx, icons::SHUFFLE).size(Pixels(17.0)))
                        .class("icon-btn")
                        .toggle_class("on", tr.shuffle)
                        .on_press(|cx| cx.emit(ZunoEvent::ToggleShuffle));
                    Button::new(cx, move |cx| Svg::new(cx, icons::PREV).size(Pixels(20.0)))
                        .class("icon-btn")
                        .on_press(|cx| cx.emit(ZunoEvent::Previous));
                    Button::new(cx, move |cx| {
                        Svg::new(cx, if tr.playing { icons::PAUSE } else { icons::PLAY })
                            .size(Pixels(20.0))
                            .hoverable(false)
                    })
                    .class("pb-play")
                    .on_press(|cx| cx.emit(ZunoEvent::TogglePlay));
                    Button::new(cx, move |cx| Svg::new(cx, icons::NEXT).size(Pixels(20.0)))
                        .class("icon-btn")
                        .on_press(|cx| cx.emit(ZunoEvent::Next));
                    Button::new(cx, move |cx| {
                        Svg::new(cx, if tr.repeat == RepeatMode::One { icons::REPEAT_ONE } else { icons::REPEAT })
                            .size(Pixels(17.0))
                            .hoverable(false)
                    })
                    .class("icon-btn")
                    .toggle_class("on", tr.repeat != RepeatMode::Off)
                    .on_press(|cx| cx.emit(ZunoEvent::CycleRepeat));
                })
                .class("pb-transport");
            });
            HStack::new(cx, move |cx| {
                Label::new(cx, sig.position.map(|p| mmss(*p as u32))).class("pb-time");
                Slider::new(cx, sig.position)
                    .range(sig.duration.map(|d| 0.0..(*d).max(1.0)))
                    .on_change(|cx, v| cx.emit(ZunoEvent::Seek(v as f64)))
                    .width(Pixels(340.0));
                Label::new(cx, sig.duration.map(|d| mmss(*d as u32))).class("pb-time");
            })
            .class("pb-seek");
        })
        .class("pb-center")
        .alignment(Alignment::TopCenter);

        // Right: like + queue + volume. Volume slider outside the binding.
        HStack::new(cx, move |cx| {
            Binding::new(cx, sig.transport, move |cx| {
                let sig = cx.data::<UiState>().sig;
                let tr = sig.transport.get();
                HStack::new(cx, move |cx| {
                    if tr.has_track {
                        Button::new(cx, move |cx| {
                            Svg::new(cx, if tr.liked { icons::HEART_ACTIVE } else { icons::HEART })
                                .size(Pixels(17.0))
                                .hoverable(false)
                        })
                        .class("icon-btn")
                        .toggle_class("on", tr.liked)
                        .on_press(move |cx| cx.emit(ZunoEvent::ToggleLike(tr.track_id)));
                    }
                })
                .horizontal_gap(Pixels(4.0));
            });
            Button::new(cx, move |cx| Svg::new(cx, icons::QUEUE).size(Pixels(17.0)))
                .class("icon-btn")
                .toggle_class("on", sig.queue_open)
                .on_press(|cx| cx.emit(ZunoEvent::QueueToggle));
            Binding::new(cx, sig.transport, move |cx| {
                let sig = cx.data::<UiState>().sig;
                let tr = sig.transport.get();
                Button::new(cx, move |cx| {
                    Svg::new(cx, if tr.muted { icons::VOLUME_MUTE } else { icons::VOLUME })
                        .size(Pixels(17.0))
                        .hoverable(false)
                })
                .class("icon-btn")
                .on_press(|cx| cx.emit(ZunoEvent::ToggleMute));
            });
            Slider::new(cx, sig.vol)
                .range(0.0..1.0)
                .on_change(|cx, v| cx.emit(ZunoEvent::SetVolume(v)))
                .class("pb-vol");
        })
        .class("pb-right");
    })
    .class("player-bar");
}

// — Track row (shared by every list) ——————————————————————————————————

fn track_row_button(cx: &mut Context, sig: Sigs, index: usize, id: TrackId) -> Handle<'_, Button> {
    track_row_button_msg(cx, sig, index, id, ZunoEvent::PlayFromCurrentList(index))
}

fn track_row_button_msg(
    cx: &mut Context,
    sig: Sigs,
    index: usize,
    id: TrackId,
    play: ZunoEvent,
) -> Handle<'_, Button> {
    let (title, artist, dur, explicit, liked, album_id) = {
        let st = cx.data::<UiState>();
        let v = st.app.library.track_view(id);
        let album_id = st.app.library.track(id).album_id;
        (
            v.title.to_string(),
            v.artist.to_string(),
            v.duration_sec,
            v.explicit,
            v.liked,
            album_id,
        )
    };
    let art = art_for_album(cx, album_id, 40);
    let tr_sig = sig.transport;
    let sel_sig = sig.selection;
    let frame_sig = sig.frame;

    Button::new(cx, move |cx| {
        HStack::new(cx, move |cx| {
            // Index column: number, or red play glyph / animated bars when current.
            Binding::new(cx, tr_sig, move |cx| {
                let tr = tr_sig.get();
                let cur = tr.has_track && tr.track_id == id;
                if cur && tr.playing {
                    Bars::new(cx, frame_sig).width(Pixels(26.0)).height(Pixels(14.0));
                } else if cur {
                    Svg::new(cx, icons::PLAY).class("row-play-icon");
                } else {
                    Label::new(cx, format!("{}", index + 1)).class("row-index");
                }
            });
            Image::new(cx, art).class("row-art").hoverable(false);
            VStack::new(cx, move |cx| {
                HStack::new(cx, move |cx| {
                    Label::new(cx, title).class("row-title").hoverable(false);
                    if explicit {
                        Label::new(cx, "E").class("badge-e").hoverable(false);
                    }
                })
                .class("row-title-row");
                Label::new(cx, artist).class("row-artist").hoverable(false);
            })
            .class("row-text")
            .hoverable(false);
            // Right column: heart when liked, duration otherwise.
            Binding::new(cx, tr_sig, move |cx| {
                let tr = tr_sig.get();
                let show_heart = if tr.has_track && tr.track_id == id { tr.liked } else { liked };
                if show_heart {
                    Svg::new(cx, icons::HEART_ACTIVE).class("row-heart");
                } else {
                    Label::new(cx, mmss(dur)).class("row-right");
                }
            });
        })
        .height(Percentage(100.0))
        .alignment(Alignment::Left)
        .hoverable(false)
    })
    .class("track-row")
    .toggle_class("playing", tr_sig.map(move |t| t.has_track && t.track_id == id && t.playing))
    .toggle_class("selected", sel_sig.map(move |s| *s == Some(index)))
    .on_press(move |cx| cx.emit(play.clone()))
}

// — main ————————————————————————————————————————————————————————————

fn main() -> Result<(), ApplicationError> {
    Application::new(|cx| {
        cx.add_font_mem(include_bytes!("../assets/Inter-Variable.ttf"));

        let app = AppState::new();
        let sig = Sigs::new(&app);
        let bench = if std::env::args().any(|a| a == "--bench") {
            Some(BenchDriver::new())
        } else {
            None
        };
        UiState { app, bench, sig, last_current: None, last_playing: false }.build(cx);

        cx.add_stylesheet(style::CSS).expect("failed to add zuno stylesheet");

        // ~60 Hz frame tick: playhead, auto-advance, bench schedule, animation.
        let timer = cx.add_timer(Duration::from_millis(16), None, |cx, action| {
            if matches!(action, TimerAction::Tick(_)) {
                cx.emit(ZunoEvent::Tick);
            }
        });
        // add_timer registers a *stopped* timer; start it on the event context.
        EventContext::new(cx).start_timer(timer);

        build_keymap(cx);
        build_shell(cx);
    })
    .title("Zuno")
    .inner_size((1280, 800))
    .min_inner_size(Some((900, 600)))
    .run()
}
