//! Zuno — iced implementation of the native-GUI benchmark (gui/iced branch).
//!
//! Renders the shared `zuno_core::AppState` per docs/gui-benchmarks/SPEC.md.
//! iced has no list virtualization; rows use `lazy` for view caching — the
//! same trade the React app makes with `content-visibility`. That is an
//! honest part of the benchmark, recorded in the branch report.
//!
//! The window is frameless (`decorations(false)` + transparent surface):
//! a custom 44px titlebar (wordmark, drag region, window controls), a 14px
//! rounded shell painted by the app (square when maximized, like the React
//! app's `html[data-window-maximized]`), and edge strips wired to winit's
//! interactive resize via `window::drag_resize`.
//!
//! Every widget style comes from a named function in `style` — inline
//! `move |_t| …` closures inside `column![]` macros defeat type inference.

mod icons;
mod style;

use iced::keyboard::key::Named;
use iced::widget::{
    button, canvas, column, container, image, lazy, mouse_area, row, scrollable, slider,
    stack, svg, text, text_input, Space,
};
use iced::widget::operation::AbsoluteOffset;
use iced::{
    alignment, Color, Element, Length, Padding, Rectangle, Subscription, Task,
};
use std::cell::RefCell;
use std::collections::HashMap;
use std::time::{Duration, Instant};
use zuno_core::bench::{BenchAction, BenchDriver};
use zuno_core::format::{count_songs, counts_line, listeners, mmss};
use zuno_core::model::*;
use zuno_core::queue::Region;
use zuno_core::search;
use zuno_core::theme as t;
use zuno_core::{AppState, LibraryTab, RepeatMode, View};

const SONGS_SCROLL: &str = "songs-scroll";
const PAGE_SCROLL: &str = "page-scroll";
const SEARCH_INPUT: &str = "search-input";

/// Window/titlebar icon — `app-icon.png` pre-converted to 64×64 raw RGBA
/// (committed at `assets/icon.rgba`, no build-time dependency needed).
const ICON_W: u32 = 64;
const ICON_RGBA: &[u8] = include_bytes!("../assets/icon.rgba");

/// Two titlebar presses closer than this are a double-click (maximize), not a
/// drag. iced's own click classification is roughly the same window.
const DOUBLE_CLICK_MS: u64 = 400;

fn main() -> iced::Result {
    let icon = iced::window::icon::from_rgba(ICON_RGBA.to_vec(), ICON_W, ICON_W).ok();
    iced::application(Zuno::new, Zuno::update, Zuno::view)
        .title("Zuno")
        .theme(|_state: &Zuno| style::iced_theme())
        .font(include_bytes!("../assets/Inter-Variable.ttf"))
        .default_font(style::INTER)
        .window(iced::window::Settings {
            size: iced::Size::new(1280.0, 800.0),
            min_size: Some(iced::Size::new(900.0, 600.0)),
            decorations: false,
            transparent: true,
            icon,
            ..iced::window::Settings::default()
        })
        // The window surface itself stays clear; the app paints its rounded
        // shell (`style::shell`) so the corners can be truly transparent.
        .style(|_state: &Zuno, _theme| iced::theme::Style {
            background_color: Color::TRANSPARENT,
            text_color: style::fg(),
        })
        .subscription(Zuno::subscription)
        .run()
}

#[derive(Clone, Debug)]
pub enum Message {
    Tick,
    Resized(f32, f32),
    SetWindowId(iced::window::Id),
    WindowMaximized(bool),
    TitlebarPress,
    TitlebarDoubleClick,
    Minimize,
    ToggleMaximize,
    CloseWindow,
    ResizeEdge(iced::window::Direction),
    Nav(View),
    LibraryTab(LibraryTab),
    SearchInput(String),
    FocusSearch,
    SelectionStep(i32),
    SelectionPlay,
    PageScroll(f32),
    RowHover(Option<usize>),
    OpenAlbum(AlbumId),
    OpenArtist(ArtistId),
    OpenPlaylist(PlaylistId),
    PlayFrom(Vec<TrackId>, usize),
    PlayShuffled(Vec<TrackId>),
    PlayTrack(TrackId),
    TogglePlay,
    Next,
    Previous,
    Seek(f32),
    SeekDelta(f32),
    SetVolume(f32),
    ToggleMute,
    ToggleShuffle,
    CycleRepeat,
    ToggleLike(TrackId),
    QueueToggle,
    QueueJump(usize),
    QueueRemove(usize),
    Back,
    CardHover(Option<u64>),
    Escape,
}

pub struct Zuno {
    pub app: AppState,
    pub handles: RefCell<HashMap<(u64, u32), image::Handle>>,
    /// Titlebar logo, built once (re-rendering the bar every frame must not
    /// re-decode the icon).
    pub logo: image::Handle,
    pub hover_card: Option<u64>,
    pub hover_row: Option<usize>,
    pub queue_open: bool,
    pub frame: u64,
    pub window_width: f32,
    pub window_height: f32,
    /// The single window's id, fetched with `window::latest()` at boot.
    pub window_id: Option<iced::window::Id>,
    pub maximized: bool,
    /// Whether the search input holds keyboard focus, tracked by hand:
    /// iced 0.14 has `operation::is_focused` (async) but no focus-change
    /// event, so global shortcuts gate on this flag.
    pub search_focused: bool,
    /// Row count of the search results, maintained on keystrokes only (the
    /// keyboard context needs the length without re-running the search every
    /// subscription rebuild).
    pub search_len: usize,
    pub last_title_press: Option<Instant>,
    pub bench: Option<BenchDriver>,
}

impl Zuno {
    fn new() -> (Self, Task<Message>) {
        let bench = if std::env::args().any(|a| a == "--bench") {
            Some(BenchDriver::new())
        } else {
            None
        };
        let mut app = AppState::new();
        // Screenshot helper: park on the Library ▸ Songs view (the 5,000-row
        // surface) so an external `grim` can capture it without driving the UI.
        if std::env::args().any(|a| a == "--screenshot-songs") {
            app.view = View::Library;
            app.library_tab = LibraryTab::Songs;
        }
        // Grab the window id at boot; every window action (drag, controls,
        // resize edges) needs it. `latest()` is a oneshot resolved by the
        // window manager once the window exists.
        let boot = iced::window::latest().then(|id| match id {
            Some(id) => Task::done(Message::SetWindowId(id)),
            None => Task::none(),
        });
        (
            Zuno {
                app,
                handles: RefCell::new(HashMap::new()),
                logo: image::Handle::from_rgba(ICON_W, ICON_W, ICON_RGBA.to_vec()),
                hover_card: None,
                hover_row: None,
                queue_open: false,
                frame: 0,
                window_width: 1280.0,
                window_height: 800.0,
                window_id: None,
                maximized: false,
                search_focused: false,
                search_len: 0,
                last_title_press: None,
                bench,
            },
            boot,
        )
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        self.app.tick();
        match message {
            Message::Tick => {
                self.frame = self.frame.wrapping_add(1);
                if let Some(b) = &mut self.bench {
                    match b.tick() {
                        BenchAction::RecordStartup => {}
                        BenchAction::ScrollTo(f) => {
                            self.app.view = View::Library;
                            self.app.library_tab = LibraryTab::Songs;
                            return scroll_to_frac(SONGS_SCROLL, f);
                        }
                        BenchAction::SelectRow(i) => self.app.selection = Some(i),
                        BenchAction::StartPlayback => {
                            let ids = self.app.list_ids();
                            self.app.play_from(&ids, ids.len() / 2);
                        }
                        BenchAction::Finish => {
                            println!("{}", b.report);
                            std::process::exit(0);
                        }
                        BenchAction::Nothing => {}
                    }
                }
            }
            Message::Resized(w, h) => {
                self.window_width = w;
                self.window_height = h;
                if let Some(id) = self.window_id {
                    return iced::window::is_maximized(id)
                        .then(|m| Task::done(Message::WindowMaximized(m)));
                }
            }
            Message::SetWindowId(id) => {
                self.window_id = Some(id);
                return iced::window::is_maximized(id)
                    .then(|m| Task::done(Message::WindowMaximized(m)));
            }
            Message::WindowMaximized(maximized) => self.maximized = maximized,
            Message::TitlebarPress => {
                // Start an interactive drag — unless this press is the second
                // half of a double-click, in which case the maximize toggle
                // (TitlebarDoubleClick) handles it and dragging would fight it.
                let now = Instant::now();
                let is_double = self
                    .last_title_press
                    .map(|t| now.duration_since(t) < Duration::from_millis(DOUBLE_CLICK_MS))
                    .unwrap_or(false);
                self.last_title_press = Some(now);
                if !is_double {
                    if let Some(id) = self.window_id {
                        return iced::window::drag(id);
                    }
                }
            }
            Message::TitlebarDoubleClick => {
                if let Some(id) = self.window_id {
                    return iced::window::toggle_maximize(id);
                }
            }
            Message::Minimize => {
                if let Some(id) = self.window_id {
                    return iced::window::minimize(id, true);
                }
            }
            Message::ToggleMaximize => {
                if let Some(id) = self.window_id {
                    return iced::window::toggle_maximize(id);
                }
            }
            Message::CloseWindow => {
                if let Some(id) = self.window_id {
                    return iced::window::close(id);
                }
            }
            Message::ResizeEdge(direction) => {
                if let Some(id) = self.window_id {
                    return iced::window::drag_resize(id, direction);
                }
            }
            Message::Nav(view) => {
                self.hover_row = None;
                self.search_focused = false;
                self.app.go(view);
            }
            Message::LibraryTab(tab) => {
                self.hover_row = None;
                self.app.set_library_tab(tab);
            }
            Message::SearchInput(q) => {
                self.search_focused = true;
                self.hover_row = None;
                self.app.set_search(&q);
                self.search_len =
                    search::search(&self.app.library, &self.app.search_query).tracks.len();
            }
            Message::FocusSearch => {
                self.search_focused = true;
                self.app.go(View::Search);
                return iced::widget::operation::focus(SEARCH_INPUT);
            }
            Message::SelectionStep(delta) => {
                let len = self.app.list_ids().len();
                if len > 0 {
                    let next = match self.app.selection {
                        None if delta < 0 => len - 1,
                        None => 0,
                        Some(s) => (s as i32 + delta).clamp(0, len as i32 - 1) as usize,
                    };
                    self.app.selection = Some(next);
                    return follow_selection(self.active_scroll_id(), next, self.window_height);
                }
            }
            Message::SelectionPlay => {
                if let Some(sel) = self.app.selection {
                    let ids = self.app.list_ids();
                    if sel < ids.len() {
                        self.app.play_from(&ids, sel);
                    }
                }
            }
            Message::PageScroll(direction) => {
                let viewport = list_viewport(self.window_height);
                return iced::widget::operation::scroll_by(
                    self.active_scroll_id(),
                    AbsoluteOffset { x: 0.0, y: direction * viewport * 0.8 },
                );
            }
            Message::RowHover(index) => self.hover_row = index,
            Message::OpenAlbum(id) => {
                self.hover_row = None;
                self.search_focused = false;
                self.app.open_album(id);
            }
            Message::OpenArtist(id) => {
                self.hover_row = None;
                self.search_focused = false;
                self.app.open_artist(id);
            }
            Message::OpenPlaylist(id) => {
                self.hover_row = None;
                self.search_focused = false;
                self.app.open_playlist(id);
            }
            Message::PlayFrom(list, i) => {
                // Click-to-select: the clicked row is both selected (10% tint)
                // and played, in the same handler.
                if i < list.len() {
                    self.app.selection = Some(i);
                }
                self.app.play_from(&list, i);
            }
            Message::PlayShuffled(list) => {
                self.app.play_from(&list, 0);
                if !self.app.shuffle {
                    self.app.toggle_shuffle();
                }
            }
            Message::PlayTrack(id) => self.app.play_track(id),
            Message::TogglePlay => self.app.toggle_play(),
            Message::Next => self.app.next(),
            Message::Previous => self.app.previous(),
            Message::Seek(pos) => self.app.seek(pos as f64),
            Message::SeekDelta(d) => self.app.seek(self.app.player.position_sec() + d as f64),
            Message::SetVolume(v) => self.app.set_volume(v / 100.0),
            Message::ToggleMute => self.app.toggle_mute(),
            Message::ToggleShuffle => self.app.toggle_shuffle(),
            Message::CycleRepeat => self.app.cycle_repeat(),
            Message::ToggleLike(id) => self.app.toggle_like(id),
            Message::QueueToggle => self.queue_open = !self.queue_open,
            Message::QueueJump(i) => {
                if let Some(id) = self.app.queue.jump_to(i) {
                    self.app.play_track(id);
                }
            }
            Message::QueueRemove(i) => {
                self.app.queue.remove_at(i);
            }
            Message::Back => {
                self.hover_row = None;
                self.search_focused = false;
                self.app.go_back();
            }
            Message::CardHover(seed) => self.hover_card = seed,
            Message::Escape => {
                if !self.app.search_query.is_empty() {
                    // First Esc clears the query (the input keeps focus).
                    self.app.search_query.clear();
                    self.search_len = 0;
                } else {
                    // Second Esc leaves the page; the input unmounts with it,
                    // so the focus flag can honestly go false.
                    self.search_focused = false;
                    self.hover_row = None;
                    self.app.go_back();
                }
            }
        }
        Task::none()
    }

    fn subscription(&self) -> Subscription<Message> {
        let kb = KeyContext {
            selection: self.app.selection,
            list_len: self.list_len(),
            search_focused: self.search_focused,
        };
        Subscription::batch([
            iced::time::every(Duration::from_millis(16)).map(|_| Message::Tick),
            iced::window::resize_events()
                .map(|(_id, size)| Message::Resized(size.width, size.height)),
            iced::keyboard::listen()
                .with(kb)
                .filter_map(|(kb, event)| keyboard_event(event, &kb)),
        ])
    }

    pub fn art(&self, seed: u64, size: u32) -> image::Handle {
        let mut handles = self.handles.borrow_mut();
        handles
            .entry((seed, size))
            .or_insert_with(|| {
                let buf = self.app.artwork.get(seed, size);
                image::Handle::from_rgba(size, size, buf.to_vec())
            })
            .clone()
    }

    /// The scrollable that holds the current view's rows (cheap, no search
    /// re-run) — used by keyboard selection-follow and PgUp/PgDn.
    fn active_scroll_id(&self) -> &'static str {
        if self.app.view == View::Library && self.app.library_tab == LibraryTab::Songs {
            SONGS_SCROLL
        } else {
            PAGE_SCROLL
        }
    }

    /// Row count of the current view's main list without materialising it.
    fn list_len(&self) -> usize {
        match &self.app.view {
            View::Library if self.app.library_tab == LibraryTab::Songs => {
                self.app.library.tracks.len()
            }
            View::Library => 0,
            View::Album(id) => self.app.library.album(*id).track_ids.len(),
            View::Playlist(id) => self
                .app
                .library
                .playlist(*id)
                .map(|p| p.track_ids.len())
                .unwrap_or(0),
            View::Search => self.search_len,
            View::Home | View::Artist(_) | View::Settings => 0,
        }
    }

    // — Shell ———————————————————————————————————————————————————————

    fn view(&self) -> Element<'_, Message> {
        let sidebar = self.view_sidebar();
        let content = container(self.view_page())
            .width(Length::Fill)
            .height(Length::Fill)
            .style(style::container_bg());
        let middle = row![sidebar, content].height(Length::Fill);
        let base = column![self.view_titlebar(), middle, self.view_player_bar()]
            .spacing(0)
            .height(Length::Fill);

        // Stack order = z-order: queue rail above the page, resize strips
        // above everything (they are 6/10px slivers at the window edge, where
        // nothing else lives — page padding keeps interactive content clear).
        let mut layers = stack![].width(Length::Fill).height(Length::Fill);
        layers = layers.push(base);
        if self.queue_open {
            layers = layers.push(
                container(self.view_queue())
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .align_x(alignment::Horizontal::Right)
                    .style(style::container_plain()),
            );
        }
        for strip in resize_strips() {
            layers = layers.push(strip);
        }
        // The rounded, clipped shell — the app paints the window's surface.
        container(layers)
            .width(Length::Fill)
            .height(Length::Fill)
            .clip(true)
            .style(style::shell(self.maximized))
            .into()
    }

    // — Titlebar (frameless window chrome) ————————————————————

    fn view_titlebar(&self) -> Element<'_, Message> {
        let brand = row![
            image(self.logo.clone())
                .width(Length::Fixed(22.0))
                .height(Length::Fixed(22.0)),
            text("zuno_")
                .font(style::font(700))
                .size(15.0)
                .style(style::text_plain()),
        ]
        .spacing(9)
        .align_y(alignment::Vertical::Center);

        // The empty stretch of the bar: press = interactive drag, double
        // press = maximize (the React TitleBar's drag region contract).
        let drag = mouse_area(
            container(Space::new().width(Length::Fill).height(Length::Fill))
                .width(Length::Fill)
                .height(Length::Fill),
        )
        .on_press(Message::TitlebarPress)
        .on_double_click(Message::TitlebarDoubleClick)
        .interaction(iced::mouse::Interaction::Grab);

        let max_icon = if self.maximized { icons::RESTORE } else { icons::MAXIMIZE };
        let controls = row![
            window_button(icons::MINIMIZE, 15.0, Message::Minimize, false),
            window_button(max_icon, 13.0, Message::ToggleMaximize, false),
            window_button(icons::CLOSE, 14.0, Message::CloseWindow, true),
        ];

        let bar = row![
            container(brand).padding(Padding { left: 16.0, ..Padding::ZERO }),
            drag,
            container(controls).padding(Padding { right: 8.0, ..Padding::ZERO }),
        ]
        .align_y(alignment::Vertical::Center)
        .height(Length::Fixed(t::TITLEBAR_H));

        container(bar)
            .width(Length::Fill)
            .height(Length::Fixed(t::TITLEBAR_H))
            .into()
    }

    // — Sidebar ———————————————————————————————————————————————————————

    fn view_sidebar(&self) -> Element<'_, Message> {
        let is = |target: &View| &self.app.view == target;
        let mut nav_col = column![
            nav_item("Home", icons::HOME, is(&View::Home), Message::Nav(View::Home)),
            nav_item("Search", icons::SEARCH, is(&View::Search), Message::Nav(View::Search)),
            nav_item("Library", icons::LIBRARY, is(&View::Library), Message::Nav(View::Library)),
            nav_item("Settings", icons::SETTINGS, is(&View::Settings), Message::Nav(View::Settings)),
        ]
        .spacing(2);
        nav_col = nav_col.push(Space::new().width(Length::Fill).height(Length::Fixed(18.0)));
        nav_col = nav_col.push(
            container(
                text("PLAYLISTS").font(style::font(600)).size(11.0).style(style::text_muted()),
            )
            .padding(Padding { left: 12.0, ..Padding::ZERO }),
        );
        nav_col = nav_col.push(Space::new().width(Length::Fill).height(Length::Fixed(6.0)));

        let mut rows = column![self.playlist_row(0)].spacing(1);
        for p in &self.app.library.playlists[1..] {
            rows = rows.push(self.playlist_row(p.id));
        }
        let playlists_scroll = scrollable(rows.spacing(1))
            .direction(scrollable::Direction::Vertical(
                scrollable::Scrollbar::new().width(4).scroller_width(4),
            ))
            .style(style::scrollable())
            .height(Length::Fill);
        let col = column![nav_col, playlists_scroll].spacing(8).height(Length::Fill);
        container(col)
            .width(Length::Fixed(t::SIDEBAR_W))
            .height(Length::Fill)
            .padding(Padding { top: 16.0, left: 12.0, right: 12.0, ..Padding::ZERO })
            .style(style::container_bg())
            .into()
    }

    fn playlist_row(&self, id: PlaylistId) -> Element<'_, Message> {
        let Some(p) = self.app.library.playlist(id) else {
            return row![].into();
        };
        let seed = AppState::playlist_seed(p.id);
        let active = matches!(self.app.view, View::Playlist(v) if v == p.id);
        let count = p.track_ids.len();
        let body = row![
            container(
                image(self.art(seed, 40))
                    .width(Length::Fixed(40.0))
                    .height(Length::Fixed(40.0))
                    .content_fit(iced::ContentFit::Cover)
            )
            .clip(true)
            .style(style::container_art_shape(false)),
            column![
                text(p.title.as_str()).font(style::font(500)).size(t::BODY.size),
                mono(format!("{count} songs")),
            ]
            .spacing(2),
        ]
        .spacing(10)
        .align_y(alignment::Vertical::Center);
        button(body)
            .width(Length::Fill)
            .height(Length::Fixed(48.0))
            .padding(Padding { left: 8.0, right: 8.0, top: 4.0, bottom: 4.0 })
            .style(style::nav_button(active, false))
            .on_press(Message::OpenPlaylist(p.id))
            .into()
    }

    // — Page router —————————————————————————————————————————————

    fn view_page(&self) -> Element<'_, Message> {
        match &self.app.view {
            View::Home => self.view_home(),
            View::Library => self.view_library(),
            View::Album(id) => self.view_collection(
                AppState::album_seed(*id),
                self.app.library.album(*id).title.clone(),
                format!(
                    "{} · {}",
                    self.app.library.album(*id).kind.label(),
                    self.app.library.artist(self.app.library.album(*id).artist_id).name
                ),
                self.app.library.album(*id).track_ids.clone(),
            ),
            View::Playlist(id) => {
                let p = self.app.library.playlist(*id).expect("playlist exists");
                self.view_collection(
                    AppState::playlist_seed(*id),
                    p.title.clone(),
                    format!("Playlist · {}", p.description.as_deref().unwrap_or("Curated by you")),
                    p.track_ids.clone(),
                )
            }
            View::Artist(id) => self.view_artist(*id),
            View::Search => self.view_search(),
            View::Settings => self.view_settings(),
        }
    }

    // — Home ————————————————————————————————————————————————————

    fn view_home(&self) -> Element<'_, Message> {
        let mut col = column![
            column![
                style::h1("Home"),
                text("Pick up where you left off")
                    .font(style::font(400))
                    .size(t::SMALL.size)
                    .style(style::text_muted()),
            ]
            .spacing(4)
            .padding(Padding { top: 24.0, ..Padding::ZERO }),
        ]
        .spacing(t::SECTION_GAP);

        col = col.push(shelf(
            "Recently played",
            self.app.library.home_recent_albums(12).into_iter().map(|aid| self.album_card(aid)).collect(),
        ));
        col = col.push(shelf(
            "Made for you",
            self.app
                .library
                .mixes
                .iter()
                .map(|m| {
                    self.generic_card(
                        AppState::playlist_seed(m.id),
                        m.title.clone(),
                        m.description.clone().unwrap_or_default(),
                        Message::OpenPlaylist(m.id),
                        false,
                    )
                })
                .collect(),
        ));

        let mut album_ids: Vec<AlbumId> = (0..self.app.library.albums.len() as u32).collect();
        album_ids.sort_by_key(|&a| std::cmp::Reverse(self.app.library.album(a).year));
        album_ids.truncate(24);
        let mut grid = iced::widget::grid::Grid::new().columns(self.grid_columns()).spacing(16);
        for aid in album_ids {
            grid = grid.push(self.album_card(aid));
        }
        col = col.push(column![style::h2("New albums"), container(grid).width(Length::Fill)].spacing(12));

        let mut artist_ids: Vec<ArtistId> = (0..self.app.library.artists.len() as u32).collect();
        artist_ids.sort_by_key(|&r| std::cmp::Reverse(self.app.library.artist(r).monthly_listeners));
        artist_ids.truncate(12);
        col = col.push(shelf(
            "Popular artists",
            artist_ids
                .into_iter()
                .map(|rid| {
                    let a = self.app.library.artist(rid);
                    self.generic_card(
                        AppState::artist_seed(rid),
                        a.name.clone(),
                        listeners(a.monthly_listeners),
                        Message::OpenArtist(rid),
                        true,
                    )
                })
                .collect(),
        ));

        page_scroll(
            container(col)
                .width(Length::Fill)
                .padding(Padding { left: t::PAGE_PAD, right: t::PAGE_PAD, bottom: t::PAGE_PAD, ..Padding::ZERO })
                .into(),
        )
    }

    fn grid_columns(&self) -> usize {
        (((self.window_width - t::SIDEBAR_W - 2.0 * t::PAGE_PAD) / (t::CARD_W + 16.0)).floor() as usize)
            .clamp(2, 8)
    }

    fn album_card(&self, aid: AlbumId) -> Element<'_, Message> {
        let album = self.app.library.album(aid);
        let artist = self.app.library.artist(album.artist_id);
        self.generic_card(
            AppState::album_seed(aid),
            album.title.clone(),
            format!("{} · {}", artist.name, album.year),
            Message::OpenAlbum(aid),
            false,
        )
    }

    /// AlbumCard from the spec: square art (round for artists), hover card
    /// surface + scrim with a red play pill.
    fn generic_card(
        &self,
        seed: u64,
        title: String,
        subtitle: String,
        on_open: Message,
        art_round: bool,
    ) -> Element<'_, Message> {
        let hovered = self.hover_card == Some(seed);
        let art = image(self.art(seed, 176))
            .width(Length::Fixed(t::CARD_W))
            .height(Length::Fixed(t::CARD_W))
            .content_fit(iced::ContentFit::Cover);
        let mut layers =
            stack![container(art).clip(true).style(style::container_art_shape(art_round))];
        if hovered {
            layers = layers.push(
                container(
                    button(
                        container(icon(icons::PLAY, 22.0, Color::WHITE))
                            .width(Length::Fixed(44.0))
                            .height(Length::Fixed(44.0))
                            .align_x(alignment::Horizontal::Center)
                            .align_y(alignment::Vertical::Center),
                    )
                    .style(style::primary_pill())
                    .on_press(on_open.clone()),
                )
                .width(Length::Fixed(t::CARD_W))
                .height(Length::Fixed(t::CARD_W))
                .align_x(alignment::Horizontal::Center)
                .align_y(alignment::Vertical::Center)
                .style(style::container_scrim()),
            );
        }
        let card = column![
            layers,
            Space::new().width(Length::Fill).height(Length::Fixed(8.0)),
            text(title).font(style::font(500)).size(t::BODY.size).width(Length::Fixed(t::CARD_W)),
            text(subtitle)
                .font(style::font(400))
                .size(t::SMALL.size)
                .style(style::text_muted())
                .width(Length::Fixed(t::CARD_W)),
        ]
        .spacing(2);
        // Hover surface on the whole card (React: `hover:bg-card`), hover
        // tracking on the same area, click opens.
        mouse_area(
            container(card)
                .width(Length::Fixed(t::CARD_W + 8.0))
                .padding(Padding::new(0.0))
                .style(style::card_surface(hovered)),
        )
        .on_enter(Message::CardHover(Some(seed)))
        .on_exit(Message::CardHover(None))
        .on_press(on_open)
        .into()
    }

    // — Library (the 5,000-row large-list surface) ————————————

    fn view_library(&self) -> Element<'_, Message> {
        let mut col = column![
            container(page_header("Your Library", self.app.can_go_back()))
                .padding(Padding { top: 18.0, ..Padding::ZERO })
        ]
        .spacing(16.0);

        col = col.push(
            row![
                tab_button("Playlists", LibraryTab::Playlists, self.app.library_tab == LibraryTab::Playlists),
                tab_button("Albums", LibraryTab::Albums, self.app.library_tab == LibraryTab::Albums),
                tab_button("Artists", LibraryTab::Artists, self.app.library_tab == LibraryTab::Artists),
                tab_button("Songs", LibraryTab::Songs, self.app.library_tab == LibraryTab::Songs),
            ]
            .spacing(8),
        );

        let body: Element<'_, Message> = match self.app.library_tab {
            LibraryTab::Songs => {
                let ids = self.app.list_ids();
                let mut rows = column![].spacing(1);
                for (i, &id) in ids.iter().enumerate() {
                    rows = rows.push(self.track_row(i, id, &ids));
                }
                scrollable(rows)
                    .id(SONGS_SCROLL)
                    .direction(scrollable::Direction::Vertical(
                        scrollable::Scrollbar::new().width(4).scroller_width(4),
                    ))
                    .style(style::scrollable())
                    .height(Length::Fill)
                    .width(Length::Fill)
                    .into()
            }
            LibraryTab::Albums => {
                let mut grid = iced::widget::grid::Grid::new().columns(self.grid_columns()).spacing(16);
                for aid in 0..self.app.library.albums.len() as u32 {
                    grid = grid.push(self.album_card(aid));
                }
                page_scroll(
                    container(grid).width(Length::Fill).padding(Padding { top: 8.0, ..Padding::ZERO }).into(),
                )
            }
            LibraryTab::Artists => {
                let mut rows = column![].spacing(2);
                for rid in 0..self.app.library.artists.len() as u32 {
                    let a = self.app.library.artist(rid);
                    rows = rows.push(
                        button(
                            row![
                                container(
                                    image(self.art(AppState::artist_seed(rid), 40))
                                        .width(Length::Fixed(40.0))
                                        .height(Length::Fixed(40.0))
                                        .content_fit(iced::ContentFit::Cover)
                                )
                                .clip(true)
                                .style(style::container_art_shape(true)),
                                column![
                                    text(a.name.clone()).font(style::font(500)).size(t::BODY.size),
                                    mono(listeners(a.monthly_listeners)),
                                ]
                                .spacing(2),
                                Space::new().width(Length::Fill).height(Length::Shrink),
                            ]
                            .spacing(12)
                            .align_y(alignment::Vertical::Center),
                        )
                        .width(Length::Fill)
                        .padding(6)
                        .style(style::row_button(false, false))
                        .on_press(Message::OpenArtist(rid)),
                    );
                }
                page_scroll(rows.into())
            }
            LibraryTab::Playlists => {
                let mut rows = column![].spacing(2);
                for p in &self.app.library.playlists {
                    rows = rows.push(
                        button(
                            row![
                                container(
                                    image(self.art(AppState::playlist_seed(p.id), 40))
                                        .width(Length::Fixed(40.0))
                                        .height(Length::Fixed(40.0))
                                        .content_fit(iced::ContentFit::Cover)
                                )
                                .clip(true)
                                .style(style::container_art_shape(false)),
                                column![
                                    text(p.title.clone()).font(style::font(500)).size(t::BODY.size),
                                    mono(format!("{} · playlist", count_songs(p.track_ids.len()))),
                                ]
                                .spacing(2),
                                Space::new().width(Length::Fill).height(Length::Shrink),
                            ]
                            .spacing(12)
                            .align_y(alignment::Vertical::Center),
                        )
                        .width(Length::Fill)
                        .padding(6)
                        .style(style::row_button(false, false))
                        .on_press(Message::OpenPlaylist(p.id)),
                    );
                }
                page_scroll(rows.into())
            }
        };
        col = col.push(body);
        container(col)
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(Padding { left: t::PAGE_PAD, right: t::PAGE_PAD, bottom: 8.0, ..Padding::ZERO })
            .into()
    }

    // — Album / Playlist (shared collection page) ————————————

    fn view_collection(
        &self,
        seed: u64,
        title: String,
        subtitle: String,
        ids: Vec<TrackId>,
    ) -> Element<'_, Message> {
        let counts = counts_line(ids.iter().map(|&tid| self.app.library.track(tid).duration_sec));
        let hero = row![
            container(
                image(self.art(seed, 232))
                    .width(Length::Fixed(232.0))
                    .height(Length::Fixed(232.0))
                    .content_fit(iced::ContentFit::Cover),
            )
            .clip(true)
            .style(style::container_card()),
            column![
                text(subtitle).font(style::font(400)).size(t::SMALL.size).style(style::text_muted()),
                style::h1(&title),
                mono(counts),
                Space::new().width(Length::Fill).height(Length::Fill),
                row![
                    primary_pill("Play", icons::PLAY, Message::PlayFrom(ids.clone(), 0)),
                    ghost_pill("Shuffle", icons::SHUFFLE, Message::PlayShuffled(ids.clone())),
                ]
                .spacing(10),
            ]
            .spacing(10)
            .width(Length::Fill)
            .height(Length::Fixed(232.0)),
        ]
        .spacing(24)
        .padding(Padding { top: 20.0, ..Padding::ZERO });

        let mut rows = column![].spacing(1);
        for (i, &tid) in ids.iter().enumerate() {
            rows = rows.push(self.track_row(i, tid, &ids));
        }
        let body = column![hero, Space::new().width(Length::Fill).height(Length::Fixed(16.0)), rows]
            .width(Length::Fill);
        page_scroll(
            container(body)
                .padding(Padding { left: t::PAGE_PAD, right: t::PAGE_PAD, bottom: t::PAGE_PAD, ..Padding::ZERO })
                .into(),
        )
    }

    // — Artist ————————————————————————————————————————————————————

    fn view_artist(&self, id: ArtistId) -> Element<'_, Message> {
        let a = self.app.library.artist(id);
        let seed = AppState::artist_seed(id);
        let top: Vec<TrackId> = self
            .app
            .library
            .albums
            .iter()
            .find(|al| al.artist_id == id)
            .map(|al| al.track_ids.iter().rev().take(10).copied().collect())
            .unwrap_or_default();
        let counts = counts_line(top.iter().map(|&tid| self.app.library.track(tid).duration_sec));
        let hero = row![
            container(
                image(self.art(seed, 232))
                    .width(Length::Fixed(232.0))
                    .height(Length::Fixed(232.0))
                    .content_fit(iced::ContentFit::Cover),
            )
            .clip(true)
            .style(style::container_art_shape(true)),
            column![
                text("Artist").font(style::font(400)).size(t::SMALL.size).style(style::text_muted()),
                style::h1(&a.name),
                mono(listeners(a.monthly_listeners)),
                mono(counts),
                Space::new().width(Length::Fill).height(Length::Fill),
                row![
                    primary_pill("Play", icons::PLAY, Message::PlayFrom(top.clone(), 0)),
                    ghost_pill("Shuffle", icons::SHUFFLE, Message::PlayShuffled(top.clone())),
                ]
                .spacing(10),
            ]
            .spacing(10)
            .width(Length::Fill)
            .height(Length::Fixed(232.0)),
        ]
        .spacing(24)
        .padding(Padding { top: 20.0, ..Padding::ZERO });

        let mut top_rows = column![].spacing(1);
        for (i, &tid) in top.iter().enumerate() {
            top_rows = top_rows.push(self.track_row(i, tid, &top));
        }
        // The explicit lifetime keeps inference from pinning `col` to
        // 'static when every initial element happens to be 'static — the
        // track rows and album cards below borrow `self`.
        let mut col: iced::widget::Column<'_, Message> =
            column![hero, Space::new().width(Length::Fill).height(Length::Fixed(20.0))].spacing(10);
        if !top.is_empty() {
            col = col.push(style::h2("Top tracks"));
            col = col.push(top_rows);
            col = col.push(Space::new().width(Length::Fill).height(Length::Fixed(24.0)));
        }
        col = col.push(style::h2("Albums"));
        let mut grid = iced::widget::grid::Grid::new().columns(self.grid_columns()).spacing(16);
        for &aid in &a.album_ids {
            grid = grid.push(self.album_card(aid));
        }
        col = col.push(grid);
        page_scroll(
            container(col)
                .width(Length::Fill)
                .padding(Padding { left: t::PAGE_PAD, right: t::PAGE_PAD, bottom: t::PAGE_PAD, ..Padding::ZERO })
                .into(),
        )
    }

    // — Search ————————————————————————————————————————————————————

    fn view_search(&self) -> Element<'_, Message> {
        let field = text_input("Search songs, albums, artists…", &self.app.search_query)
            .on_input(Message::SearchInput)
            .id(SEARCH_INPUT)
            .size(t::BODY.size)
            .padding(Padding { left: 14.0, right: 14.0, top: 9.0, bottom: 9.0 })
            .style(style::input_search());
        let mut col = column![container(field).width(Length::Fixed(420.0))].spacing(16);

        let q = self.app.search_query.trim().to_string();
        if !q.is_empty() {
            let results = search::search(&self.app.library, &q);
            if results.tracks.is_empty() {
                let msg = format!("No results for “{q}”");
                col = col.push(
                    container(text(msg).font(style::font(400)).size(t::BODY.size).style(style::text_muted()))
                        .width(Length::Fill)
                        .align_x(alignment::Horizontal::Center),
                );
            } else {
                let top_id = results.tracks[0];
                let tv = self.app.library.track_view(top_id);
                col = col.push(shelf(
                    "Top result",
                    vec![self.generic_card(
                        AppState::album_seed(tv.album_id),
                        tv.title.to_string(),
                        format!("Song · {}", tv.artist),
                        Message::PlayTrack(top_id),
                        false,
                    )],
                ));
                let ids = results.tracks.clone();
                let mut rows = column![].spacing(1);
                for (i, &tid) in ids.iter().enumerate() {
                    rows = rows.push(self.track_row(i, tid, &ids));
                }
                col = col.push(column![style::h2("Songs"), rows].spacing(12));
                if !results.albums.is_empty() {
                    col = col.push(shelf(
                        "Albums",
                        results.albums.iter().map(|&aid| self.album_card(aid)).collect(),
                    ));
                }
                if !results.artists.is_empty() {
                    col = col.push(shelf(
                        "Artists",
                        results.artists
                            .iter()
                            .map(|&rid| {
                                let r = self.app.library.artist(rid);
                                self.generic_card(
                                    AppState::artist_seed(rid),
                                    r.name.clone(),
                                    listeners(r.monthly_listeners),
                                    Message::OpenArtist(rid),
                                    true,
                                )
                            })
                            .collect(),
                    ));
                }
            }
        }
        page_scroll(
            container(col)
                .width(Length::Fill)
                .padding(Padding { left: t::PAGE_PAD, right: t::PAGE_PAD, top: 18.0, bottom: t::PAGE_PAD, ..Padding::ZERO })
                .into(),
        )
    }

    // — Settings ————————————————————————————————————————————————

    fn view_settings(&self) -> Element<'_, Message> {
        let col = column![
            container(page_header("Settings", self.app.can_go_back()))
                .padding(Padding { top: 18.0, ..Padding::ZERO }),
            settings_section("Appearance", vec![
                setting_row("Theme", "Dark (pinned)"),
                setting_row("Accent", "#FF0033"),
            ]),
            settings_section("Playback", vec![
                setting_row("Streaming quality", "High (256 kbps)"),
                setting_row("Download quality", "High (256 kbps)"),
                setting_row("Gapless playback", "On"),
                setting_row("Crossfade", "Off"),
                setting_row("Audio engine", "Rust (cpal) — no WebView"),
            ]),
            settings_section("Library", vec![
                setting_row("Cache", "4 GB"),
                setting_row("Downloads ceiling", "8 GB"),
            ]),
        ]
        .spacing(24);
        page_scroll(
            container(col)
                .width(Length::Fill)
                .padding(Padding { left: t::PAGE_PAD, right: t::PAGE_PAD, bottom: t::PAGE_PAD, ..Padding::ZERO })
                .into(),
        )
    }

    // — Queue rail ————————————————————————————————————————————

    fn view_queue(&self) -> Element<'_, Message> {
        let rows = self.app.queue_rows();
        let mut col = column![
            row![
                text("Queue").font(style::font(700)).size(t::H3.size),
                Space::new().width(Length::Fill).height(Length::Shrink),
                button(icon(icons::CLOSE, 16.0, style::muted_fg()))
                    .padding(6)
                    .style(style::chip(false))
                    .on_press(Message::QueueToggle),
            ]
            .align_y(alignment::Vertical::Center),
        ]
        .spacing(4);
        col = col.push(Space::new().width(Length::Fill).height(Length::Fixed(8.0)));

        let mut queue_col = column![].spacing(1);
        let mut last_region: Option<Region> = None;
        for (i, (id, region)) in rows.iter().enumerate() {
            if region != &last_region.unwrap_or(Region::Played) {
                if let Some(l) = match region {
                    Region::Played => None,
                    Region::Current => Some("NOW PLAYING"),
                    Region::Manual => Some("NEXT IN QUEUE"),
                    Region::Automatic => Some("NEXT UP"),
                } {
                    queue_col = queue_col.push(
                        container(text(l).font(style::font(400)).size(11.0).style(style::text_muted()))
                            .padding(Padding { top: 10.0, ..Padding::ZERO }),
                    );
                }
                last_region = Some(*region);
            }
            let tv = self.app.library.track_view(*id);
            let is_current = *region == Region::Current;
            let playing = is_current && self.app.playing;
            let right: Element<'_, Message> = if playing {
                // The now-playing row trades its duration for the animated
                // bars indicator (same glyph language as the track row).
                canvas::Canvas::new(BarsProgram { frame: self.frame })
                    .width(Length::Fixed(16.0))
                    .height(Length::Fixed(14.0))
                    .into()
            } else {
                container(mono(mmss(tv.duration_sec)))
                    .width(Length::Fixed(36.0))
                    .align_x(alignment::Horizontal::Right)
                    .into()
            };
            let mut r = row![
                container(
                    image(self.art(AppState::album_seed(tv.album_id), 32))
                        .width(Length::Fixed(32.0))
                        .height(Length::Fixed(32.0))
                        .content_fit(iced::ContentFit::Cover)
                )
                .clip(true),
                column![
                    text(tv.title.to_string())
                        .font(style::font(500))
                        .size(t::SMALL.size + 1.0)
                        .style(style::text_current(is_current)),
                    text(tv.artist.to_string()).font(style::font(400)).size(t::SMALL.size).style(style::text_muted()),
                ]
                .spacing(1),
                Space::new().width(Length::Fill).height(Length::Shrink),
                right,
            ]
            .spacing(10)
            .align_y(alignment::Vertical::Center);
            if !is_current {
                r = r.push(
                    button(icon(icons::CLOSE, 13.0, style::muted_fg()))
                        .padding(4)
                        .style(style::chip(false))
                        .on_press(Message::QueueRemove(i)),
                );
            }
            queue_col = queue_col.push(
                button(r)
                    .width(Length::Fill)
                    .padding(6)
                    .style(style::row_button(is_current, false))
                    .on_press(Message::QueueJump(i)),
            );
        }
        col = col.push(
            scrollable(queue_col)
                .direction(scrollable::Direction::Vertical(
                    scrollable::Scrollbar::new().width(4).scroller_width(4),
                ))
                .style(style::scrollable())
                .height(Length::Fill),
        );
        container(col)
            .width(Length::Fixed(t::QUEUE_W))
            .height(Length::Fill)
            .padding(Padding { top: 16.0, left: 16.0, right: 12.0, bottom: 12.0 })
            .style(style::container_rail())
            .into()
    }

    // — Player bar ——————————————————————————————————————————————

    fn view_player_bar(&self) -> Element<'_, Message> {
        let current = self.app.current_track();
        let (title, artist, seed, album_id, artist_id, track_id, liked) = match current {
            Some(track) => (
                track.title.clone(),
                self.app.library.artist(track.artist_id).name.clone(),
                AppState::album_seed(track.album_id),
                track.album_id,
                track.artist_id,
                track.id,
                track.liked,
            ),
            None => ("Nothing playing".to_string(), "—".to_string(), 0, 0, 0, 0, false),
        };
        let has_track = current.is_some();
        let position = self.app.player.position_sec();
        let duration = self.app.player.duration_sec().max(1.0);
        let playing = self.app.playing;

        let info = row![
            container(
                image(self.art(seed, 40))
                    .width(Length::Fixed(40.0))
                    .height(Length::Fixed(40.0))
                    .content_fit(iced::ContentFit::Cover)
            )
            .clip(true)
            .style(style::container_art_shape(false)),
            column![
                button(text(title).font(style::font(500)).size(t::BODY.size))
                    .style(style::chip(false))
                    .on_press(Message::OpenAlbum(album_id)),
                button(text(artist).font(style::font(400)).size(t::SMALL.size).style(style::text_muted()))
                    .style(style::chip(false))
                    .on_press(Message::OpenArtist(artist_id)),
            ]
            .spacing(1),
        ]
        .spacing(12)
        .align_y(alignment::Vertical::Center);

        let transport = row![
            button(icon(icons::SHUFFLE, 18.0, if self.app.shuffle { style::primary() } else { style::muted_fg() }))
                .padding(6)
                .style(style::chip(self.app.shuffle))
                .on_press(Message::ToggleShuffle),
            button(icon(icons::PREV, 20.0, style::fg()))
                .padding(6)
                .style(style::chip(false))
                .on_press(Message::Previous),
            button(
                container(icon(if playing { icons::PAUSE } else { icons::PLAY }, 22.0, Color::WHITE))
                    .width(Length::Fixed(40.0))
                    .height(Length::Fixed(40.0))
                    .align_x(alignment::Horizontal::Center)
                    .align_y(alignment::Vertical::Center),
            )
            .style(style::primary_pill())
            .on_press(Message::TogglePlay),
            button(icon(icons::NEXT, 20.0, style::fg()))
                .padding(6)
                .style(style::chip(false))
                .on_press(Message::Next),
            button(icon(
                match self.app.repeat { RepeatMode::One => icons::REPEAT_ONE, _ => icons::REPEAT },
                18.0,
                if self.app.repeat != RepeatMode::Off { style::primary() } else { style::muted_fg() },
            ))
            .padding(6)
            .style(style::chip(self.app.repeat != RepeatMode::Off))
            .on_press(Message::CycleRepeat),
        ]
        .spacing(6)
        .align_y(alignment::Vertical::Center);

        let seek = row![
            mono(mmss(position as u32))
                .width(Length::Fixed(36.0))
                .align_x(alignment::Horizontal::Right),
            container(
                slider(0.0..=duration as f32, position.min(duration) as f32, Message::Seek)
                    .style(style::slider()),
            )
            .width(Length::Fill)
            .padding(Padding { top: 8.0, bottom: 8.0, ..Padding::ZERO }),
            mono(mmss(duration as u32)).width(Length::Fixed(36.0)),
        ]
        .spacing(10)
        .align_y(alignment::Vertical::Center);

        let volume_icon = if self.app.muted || self.app.volume < 0.01 { icons::VOLUME_MUTE } else { icons::VOLUME };
        let right = row![
            button(icon(if liked { icons::HEART_ACTIVE } else { icons::HEART }, 18.0, if liked { style::primary() } else { style::muted_fg() }))
                .padding(6)
                .style(style::chip(liked))
                .on_press(if has_track { Message::ToggleLike(track_id) } else { Message::Tick }),
            button(icon(icons::QUEUE, 18.0, if self.queue_open { style::primary() } else { style::muted_fg() }))
                .padding(6)
                .style(style::chip(self.queue_open))
                .on_press(Message::QueueToggle),
            button(icon(volume_icon, 18.0, style::muted_fg()))
                .padding(6)
                .style(style::chip(false))
                .on_press(Message::ToggleMute),
            container(
                slider(0.0..=100.0, if self.app.muted { 0.0 } else { self.app.volume * 100.0 }, Message::SetVolume)
                    .style(style::slider()),
            )
            .width(Length::Fixed(96.0))
            .padding(Padding { top: 8.0, bottom: 8.0, ..Padding::ZERO }),
        ]
        .spacing(4)
        .align_y(alignment::Vertical::Center);

        let bar = row![
            container(info).width(Length::FillPortion(10)).align_x(alignment::Horizontal::Left),
            container(
                column![transport, container(seek).width(Length::Fixed(340.0))]
                    .spacing(4)
                    .align_x(alignment::Horizontal::Center),
            )
            .align_x(alignment::Horizontal::Center),
            container(right).width(Length::FillPortion(10)).align_x(alignment::Horizontal::Right),
        ]
        .align_y(alignment::Vertical::Center)
        .padding(Padding { left: 16.0, right: 16.0, top: 10.0, bottom: 12.0 });
        container(bar).width(Length::Fill).style(style::container_bg()).into()
    }

    // — Track row (lazy-cached; used by every list) ——————————

    pub fn track_row(&self, index: usize, id: TrackId, list: &[TrackId]) -> Element<'_, Message> {
        let playing_here = self.app.current == Some(id) && self.app.playing;
        let is_current = self.app.current == Some(id);
        let selected = self.app.selection == Some(index);
        // Row hover is mouse-driven only; the bench drives selection without
        // a cursor, so keep its lazy-cache keys perfectly stable.
        let hovered = self.hover_row == Some(index) && self.bench.is_none();
        let bench_on = self.bench.is_some();
        let liked = self.app.library.track(id).liked;
        let explicit = self.app.library.track(id).explicit;
        let frame_key = if playing_here { self.frame } else { 0 };
        let list = list.to_vec();

        lazy(
            (id, index as u32, playing_here, is_current, selected, hovered, liked, explicit, frame_key),
            move |_| {
                let v = self.app.library.track_view(id);
                let art_seed = AppState::album_seed(v.album_id);

                // The index slot: number → play glyph on hover → bars when
                // this row is the one playing. One slot, no reflow.
                let index_cell: Element<'_, Message> = if is_current {
                    if playing_here {
                        canvas::Canvas::new(BarsProgram { frame: self.frame })
                            .width(Length::Fixed(18.0))
                            .height(Length::Fixed(14.0))
                            .into()
                    } else {
                        container(icon(icons::PLAY, 14.0, style::primary())).width(Length::Fixed(18.0)).into()
                    }
                } else if hovered {
                    container(icon(icons::PLAY, 14.0, style::fg()))
                        .width(Length::Fixed(18.0))
                        .into()
                } else {
                    container(mono(format!("{:3}", index + 1)))
                        .width(Length::Fixed(24.0))
                        .align_x(alignment::Horizontal::Right)
                        .into()
                };

                let mut title_row = row![
                    text(v.title.to_string())
                        .font(style::font(500))
                        .size(t::BODY.size)
                        .style(style::text_current(is_current))
                ]
                .spacing(6)
                .align_y(alignment::Vertical::Center);
                if explicit {
                    title_row = title_row.push(
                        container(
                            text("E").font(style::font(700)).size(10.0),
                        )
                        .width(Length::Fixed(15.0))
                        .height(Length::Fixed(15.0))
                        .align_x(alignment::Horizontal::Center)
                        .align_y(alignment::Vertical::Center)
                        .style(style::container_badge()),
                    );
                }

                let info = column![title_row, text(v.artist.to_string()).font(style::font(400)).size(t::SMALL.size).style(style::text_muted())]
                    .spacing(2)
                    .width(Length::Fill);

                let right: Element<'_, Message> = if liked {
                    container(icon(icons::HEART_ACTIVE, 14.0, style::primary())).width(Length::Fixed(18.0)).into()
                } else {
                    container(mono(mmss(v.duration_sec)))
                        .width(Length::Fixed(36.0))
                        .align_x(alignment::Horizontal::Right)
                        .into()
                };

                let content = row![
                    index_cell,
                    container(
                        image(self.art(art_seed, 40))
                            .width(Length::Fixed(40.0))
                            .height(Length::Fixed(40.0))
                            .content_fit(iced::ContentFit::Cover),
                    )
                    .clip(true)
                    .style(style::container_art_shape(false)),
                    info,
                    right,
                ]
                .spacing(12)
                .align_y(alignment::Vertical::Center)
                .height(Length::Fixed(t::ROW_H - 6.0));

                let row_button: iced::widget::Button<'_, Message> = button(content)
                    .width(Length::Fill)
                    .padding(Padding::new(3.0))
                    .style(style::row_button(is_current, selected))
                    .on_press(Message::PlayFrom(list.clone(), index));
                // The annotation is load-bearing: it gives the if/else one
                // concrete `Element<'_>` type (two `Into` targets never unify).
                let el: Element<'_, Message> = if bench_on {
                    row_button.into()
                } else {
                    mouse_area(row_button)
                        .on_enter(Message::RowHover(Some(index)))
                        .on_exit(Message::RowHover(None))
                        .into()
                };
                el
            },
        )
        .into()
    }
}

// — Page header / pill / nav helpers (free fns — closures with elided
//    reference params fail lifetime checks inside the view methods) ———

pub fn page_header<'a>(title: &str, can_back: bool) -> Element<'a, Message> {
    let mut h = row![].spacing(12).align_y(alignment::Vertical::Center);
    if can_back {
        h = h.push(
            button(icon(icons::BACK, 20.0, style::muted_fg()))
                .padding(8)
                .style(style::chip(false))
                .on_press(Message::Back),
        );
    }
    h = h.push(style::h1(title));
    h.into()
}

/// Sidebar nav item — active state is signalled by the card surface plus a
/// weight bump (500 → 600), exactly one state signal per the spec.
pub fn nav_item<'a>(label: &str, icon_bytes: &'static [u8], active: bool, msg: Message) -> Element<'a, Message> {
    button(
        row![
            icon(icon_bytes, 19.0, if active { style::fg() } else { style::muted_fg() }),
            text(label.to_string())
                .font(style::font(if active { 600 } else { 500 }))
                .size(t::BODY.size)
                .style(style::text_active(active)),
        ]
        .spacing(12)
        .align_y(alignment::Vertical::Center),
    )
    .width(Length::Fill)
    .height(Length::Fixed(36.0))
    .padding(Padding { left: 12.0, ..Padding::ZERO })
    .style(style::nav_button(active, false))
    .on_press(msg)
    .into()
}

pub fn tab_button<'a>(label: &str, tab: LibraryTab, active: bool) -> Element<'a, Message> {
    button(text(label.to_string()).font(style::font(500)).size(t::SMALL.size + 1.0))
        .padding(Padding { left: 14.0, right: 14.0, top: 6.0, bottom: 6.0 })
        .style(style::tab_pill(active))
        .on_press(Message::LibraryTab(tab))
        .into()
}

pub fn setting_row<'a>(label: &str, value: &str) -> Element<'a, Message> {
    container(
        row![
            text(label.to_string()).font(style::font(500)).size(t::BODY.size),
            Space::new().width(Length::Fill).height(Length::Shrink),
            text(value.to_string()).font(style::font(400)).size(t::SMALL.size).style(style::text_muted()),
        ]
        .align_y(alignment::Vertical::Center),
    )
    .width(Length::Fill)
    .padding(Padding { left: 14.0, right: 14.0, top: 11.0, bottom: 11.0 })
    .style(style::container_setting())
    .into()
}

pub fn settings_section<'a>(title: &str, rows: Vec<Element<'a, Message>>) -> Element<'a, Message> {
    column![style::h2(title)]
        .spacing(10)
        .push(column(rows).spacing(6))
        .spacing(14)
        .into()
}

pub fn page_scroll(body: Element<'_, Message>) -> Element<'_, Message> {
    scrollable(body)
        .id(PAGE_SCROLL)
        .direction(scrollable::Direction::Vertical(
            scrollable::Scrollbar::new().width(4).scroller_width(4),
        ))
        .style(style::scrollable())
        .height(Length::Fill)
        .width(Length::Fill)
        .into()
}

pub fn shelf<'a>(title: &str, cards: Vec<Element<'a, Message>>) -> Element<'a, Message> {
    let mut scroller = row![].spacing(16);
    for c in cards {
        scroller = scroller.push(c);
    }
    column![
        style::h2(title),
        // Shelf breathing room under the card row — the hover surface
        // extends past the art and must not sit flush against the next
        // section's heading. (Scrollables take no padding; the container
        // provides it.)
        container(
            scrollable(scroller)
                .direction(scrollable::Direction::Horizontal(
                    scrollable::Scrollbar::new().width(4).scroller_width(4)
                ))
                .style(style::scrollable())
                .width(Length::Fill),
        )
        .padding(Padding { bottom: 12.0, ..Padding::ZERO })
        .width(Length::Fill),
    ]
    .spacing(12)
    .into()
}

pub fn primary_pill<'a>(label: &str, icon_bytes: &'static [u8], msg: Message) -> Element<'a, Message> {
    button(
        row![
            icon(icon_bytes, 18.0, Color::WHITE),
            text(label.to_string()).font(style::font(500)).size(t::BODY.size),
        ]
        .spacing(8)
        .align_y(alignment::Vertical::Center),
    )
    .padding(Padding { left: 20.0, right: 20.0, top: 9.0, bottom: 9.0 })
    .style(style::primary_pill())
    .on_press(msg)
    .into()
}

pub fn ghost_pill<'a>(label: &str, icon_bytes: &'static [u8], msg: Message) -> Element<'a, Message> {
    button(
        row![
            icon(icon_bytes, 18.0, style::fg()),
            text(label.to_string()).font(style::font(500)).size(t::BODY.size),
        ]
        .spacing(8)
        .align_y(alignment::Vertical::Center),
    )
    .padding(Padding { left: 18.0, right: 18.0, top: 9.0, bottom: 9.0 })
    .style(style::pill_ghost())
    .on_press(msg)
    .into()
}

/// A 12px secondary figure — every duration, count and clock uses Advanced
/// shaping (tabular figures) so timers never jitter the layout. Muted by
/// design: every duration/count in the app is secondary text.
pub fn mono<'a>(value: impl std::fmt::Display) -> iced::widget::Text<'a> {
    text(value.to_string())
        .font(style::font(400))
        .size(t::SMALL.size)
        .style(style::text_muted())
        .shaping(iced::widget::text::Shaping::Advanced)
}

// — Titlebar window controls / resize strips ————————————————————

/// A minimal window-control button: muted glyph, card surface on hover,
/// destructive red for the close button (React TitleBar convention).
pub fn window_button<'a>(icon_bytes: &'static [u8], icon_size: f32, msg: Message, close: bool) -> Element<'a, Message> {
    button(
        container(icon(icon_bytes, icon_size, if close { Color::WHITE } else { style::muted_fg() }))
            .width(Length::Fixed(44.0))
            .height(Length::Fixed(30.0))
            .align_x(alignment::Horizontal::Center)
            .align_y(alignment::Vertical::Center),
    )
    .width(Length::Fixed(44.0))
    .height(Length::Fixed(32.0))
    .style(style::window_button(close))
    .on_press(msg)
    .into()
}

/// One invisible resize sliver pinned to a window edge, wired to winit's
/// interactive resize. A frameless window has no server-drawn borders, so
/// the app provides the 6/10px hit zones itself.
fn resize_strip<'a>(
    direction: iced::window::Direction,
    width: Length,
    height: Length,
    horizontal: alignment::Horizontal,
    vertical: alignment::Vertical,
    interaction: iced::mouse::Interaction,
) -> Element<'a, Message> {
    container(
        mouse_area(Space::new().width(width).height(height))
            .on_press(Message::ResizeEdge(direction))
            .interaction(interaction),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .align_x(horizontal)
    .align_y(vertical)
    .into()
}

fn resize_strips() -> Vec<Element<'static, Message>> {
    use iced::window::Direction::*;
    use iced::mouse::Interaction as I;
    vec![
        resize_strip(North, Length::Fill, Length::Fixed(6.0), alignment::Horizontal::Center, alignment::Vertical::Top, I::ResizingVertically),
        resize_strip(South, Length::Fill, Length::Fixed(6.0), alignment::Horizontal::Center, alignment::Vertical::Bottom, I::ResizingVertically),
        resize_strip(West, Length::Fixed(6.0), Length::Fill, alignment::Horizontal::Left, alignment::Vertical::Center, I::ResizingHorizontally),
        resize_strip(East, Length::Fixed(6.0), Length::Fill, alignment::Horizontal::Right, alignment::Vertical::Center, I::ResizingHorizontally),
        resize_strip(NorthWest, Length::Fixed(10.0), Length::Fixed(10.0), alignment::Horizontal::Left, alignment::Vertical::Top, I::ResizingDiagonallyUp),
        resize_strip(NorthEast, Length::Fixed(10.0), Length::Fixed(10.0), alignment::Horizontal::Right, alignment::Vertical::Top, I::ResizingDiagonallyUp),
        resize_strip(SouthWest, Length::Fixed(10.0), Length::Fixed(10.0), alignment::Horizontal::Left, alignment::Vertical::Bottom, I::ResizingDiagonallyDown),
        resize_strip(SouthEast, Length::Fixed(10.0), Length::Fixed(10.0), alignment::Horizontal::Right, alignment::Vertical::Bottom, I::ResizingDiagonallyDown),
    ]
}

// — Keyboard ————————————————————————————————————————————————

/// What a global shortcut needs to know before it fires. Plain typing must
/// reach the search field (and nothing else), so shortcuts gate on
/// `search_focused`. Hashed into the subscription's identity via
/// `Subscription::with` (mapper closures must be non-capturing).
#[derive(Clone, Hash)]
struct KeyContext {
    selection: Option<usize>,
    list_len: usize,
    search_focused: bool,
}

fn keyboard_event(event: iced::keyboard::Event, kb: &KeyContext) -> Option<Message> {
    let iced::keyboard::Event::KeyPressed { key, modifiers, .. } = event else {
        return None;
    };
    use iced::keyboard::Key;
    let ctrl = modifiers.control() || modifiers.logo();
    let typing = kb.search_focused;
    match key {
        Key::Character(ref c) if ctrl && (c == "k" || c == "K") => Some(Message::FocusSearch),
        Key::Named(Named::Escape) => Some(Message::Escape),
        Key::Named(Named::Space) if !ctrl && !typing => Some(Message::TogglePlay),
        Key::Named(Named::ArrowRight) if !ctrl && !typing => Some(Message::SeekDelta(10.0)),
        Key::Named(Named::ArrowLeft) if !ctrl && !typing => Some(Message::SeekDelta(-10.0)),
        Key::Named(Named::ArrowDown) if !ctrl && !typing && kb.list_len > 0 => Some(Message::SelectionStep(1)),
        Key::Named(Named::ArrowUp) if !ctrl && !typing && kb.list_len > 0 => Some(Message::SelectionStep(-1)),
        Key::Named(Named::PageDown) if !typing => Some(Message::PageScroll(1.0)),
        Key::Named(Named::PageUp) if !typing => Some(Message::PageScroll(-1.0)),
        Key::Named(Named::Enter) if !typing && kb.selection.is_some() => Some(Message::SelectionPlay),
        Key::Character(ref c) if !ctrl && !typing && c == "/" => Some(Message::FocusSearch),
        Key::Character(ref c) if !ctrl && !typing && (c == "m" || c == "M") => Some(Message::ToggleMute),
        Key::Character(ref c) if !ctrl && !typing && (c == "q" || c == "Q") => Some(Message::QueueToggle),
        Key::Character(ref c) if !ctrl && !typing && (c == "s" || c == "S") => Some(Message::ToggleShuffle),
        Key::Character(ref c) if !ctrl && !typing && (c == "r" || c == "R") => Some(Message::CycleRepeat),
        _ => None,
    }
}

// — Scroll helpers ——————————————————————————————————————————

fn scroll_to_frac(id: &'static str, frac: f32) -> Task<Message> {
    iced::widget::operation::snap_to(
        id,
        iced::widget::operation::RelativeOffset { x: 0.0, y: frac },
    )
}

/// Estimated viewport height of the current list, for PgUp/PgDn and
/// selection-follow. Frameworks with real anchors would not need this
/// estimate; iced only exposes absolute offsets.
fn list_viewport(window_height: f32) -> f32 {
    (window_height - t::TITLEBAR_H - t::PLAYER_BAR_H - 40.0).max(200.0)
}

/// Keep the keyboard selection roughly centered in the list viewport by
/// scrolling to `selection * ROW_H` (iced clamps past the ends).
fn follow_selection(scroll_id: &'static str, selection: usize, window_height: f32) -> Task<Message> {
    let viewport = list_viewport(window_height) + 80.0;
    let y = (selection as f32 * t::ROW_H - viewport / 2.0).max(0.0);
    iced::widget::operation::scroll_to(
        scroll_id,
        AbsoluteOffset { x: 0.0, y },
    )
}

// — Bars canvas (the now-playing indicator) ———————————————————

pub struct BarsProgram {
    pub frame: u64,
}

impl canvas::Program<Message> for BarsProgram {
    type State = ();
    fn draw(
        &self,
        _state: &Self::State,
        renderer: &iced::Renderer,
        _theme: &iced::Theme,
        bounds: Rectangle,
        _cursor: iced::mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let f = self.frame as f32 * 0.12;
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        for i in 0..3 {
            let phase = f + i as f32 * 0.9;
            let h = (phase.sin() * 0.5 + 0.5) * 0.85 + 0.15;
            let bar_h = bounds.height * h;
            frame.fill_rectangle(
                iced::Point::new(bounds.x + i as f32 * 6.0, bounds.y + (bounds.height - bar_h)),
                iced::Size::new(4.0, bar_h),
                style::primary(),
            );
        }
        vec![frame.into_geometry()]
    }
}

// — Icon helper ————————————————————————————————————————————————

pub fn icon(bytes: &'static [u8], size: f32, color: Color) -> svg::Svg<'static> {
    svg(icons::handle(bytes))
        .width(Length::Fixed(size))
        .height(Length::Fixed(size))
        .style(move |_t: &iced::Theme, _s| svg::Style { color: Some(color) })
}
