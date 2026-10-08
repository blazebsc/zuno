//! Zuno — Slint implementation of the native-GUI benchmark (gui/slint).
//!
//! One self-contained driver: `ui/app.slint` holds the whole retained UI
//! (virtualized `ListView` for the 5,000-row list), this file owns the shared
//! `zuno_core::AppState`, the (seed,size)→`slint::Image` map, the models the
//! markup binds to, and the `--bench` protocol loop.

slint::include_modules!();

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;
use slint::Model;
use zuno_core::app::{LibraryTab, RepeatMode};
use zuno_core::bench::{BenchAction, BenchDriver};
use zuno_core::format::{count_songs, counts_line, listeners, mmss};
use zuno_core::model::*;
use zuno_core::queue::Region;
use zuno_core::search;
use zuno_core::{AppState, View};

/// A card source before it becomes `CardData` (art filled on demand).
#[derive(Clone)]
struct CardSpec {
    id: u32,
    kind: i32,
    title: String,
    subtitle: String,
    round: bool,
    seed: u64,
}

/// Which chunked grid a callback refers to.
#[derive(Clone, Copy)]
enum GridSlot {
    HomeNew,
    LibAlbums,
    ArtistAlbums,
}

/// One chunked grid: source specs + the row model the markup binds to.
struct Grid {
    specs: Vec<CardSpec>,
    rows: Rc<slint::VecModel<GridRow>>,
    row_models: Vec<Rc<slint::VecModel<CardData>>>,
    cols: usize,
}

struct Zuno {
    state: AppState,
    art: HashMap<(u64, u32), slint::Image>,
    ui: ZunoWindow,

    home_recent: Rc<slint::VecModel<CardData>>,
    home_mixes: Rc<slint::VecModel<CardData>>,
    home_artists: Rc<slint::VecModel<CardData>>,
    home_new: Grid,

    sidebar_playlists: Rc<slint::VecModel<PlaylistRowData>>,
    lib_playlists: Rc<slint::VecModel<PlaylistRowData>>,
    lib_artists: Rc<slint::VecModel<ArtistRowData>>,
    lib_albums: Grid,

    songs: Rc<slint::VecModel<TrackRow>>,
    page_tracks: Rc<slint::VecModel<TrackRow>>,
    artist_albums: Grid,

    search_top: Rc<slint::VecModel<CardData>>,
    search_songs: Rc<slint::VecModel<TrackRow>>,
    search_albums: Rc<slint::VecModel<CardData>>,
    search_artists: Rc<slint::VecModel<CardData>>,

    queue: Rc<slint::VecModel<QueueRowData>>,

    /// The list `play-from` targets for the current view ("queue = the list
    /// you clicked").
    page_ids: Vec<TrackId>,
    queue_sig: (Option<TrackId>, usize, usize, usize),
    last_current: Option<TrackId>,
    last_liked: bool,
    window_w: f32,
    queue_open: bool,
    bench: Option<BenchDriver>,
}

impl Zuno {
    fn new(ui: ZunoWindow) -> Self {
        let state = AppState::new();
        let bench = if std::env::args().any(|a| a == "--bench") {
            Some(BenchDriver::new())
        } else {
            None
        };
        let empty_grid = || Grid {
            specs: Vec::new(),
            rows: Rc::new(slint::VecModel::default()),
            row_models: Vec::new(),
            cols: 0,
        };
        let mut z = Zuno {
            state,
            art: HashMap::new(),
            ui,
            home_recent: Rc::new(slint::VecModel::default()),
            home_mixes: Rc::new(slint::VecModel::default()),
            home_artists: Rc::new(slint::VecModel::default()),
            home_new: empty_grid(),
            sidebar_playlists: Rc::new(slint::VecModel::default()),
            lib_playlists: Rc::new(slint::VecModel::default()),
            lib_artists: Rc::new(slint::VecModel::default()),
            lib_albums: empty_grid(),
            songs: Rc::new(slint::VecModel::default()),
            page_tracks: Rc::new(slint::VecModel::default()),
            artist_albums: empty_grid(),
            search_top: Rc::new(slint::VecModel::default()),
            search_songs: Rc::new(slint::VecModel::default()),
            search_albums: Rc::new(slint::VecModel::default()),
            search_artists: Rc::new(slint::VecModel::default()),
            queue: Rc::new(slint::VecModel::default()),
            page_ids: Vec::new(),
            queue_sig: (None, 0, 0, 0),
            last_current: None,
            last_liked: false,
            window_w: 1280.0,
            queue_open: false,
            bench,
        };
        z.build_static_models();
        z.connect_models();
        z
    }

    /// Hand the shared VecModels to the window as `ModelRc` properties —
    /// later mutation goes through the VecModel and the UI updates in place
    /// (no model swap → no ListView scroll reset).
    fn connect_models(&self) {
        let ui = &self.ui;
        ui.set_home_recent(slint::ModelRc::new(self.home_recent.clone()));
        ui.set_home_mixes(slint::ModelRc::new(self.home_mixes.clone()));
        ui.set_home_artists(slint::ModelRc::new(self.home_artists.clone()));
        ui.set_home_new_rows(slint::ModelRc::new(self.home_new.rows.clone()));
        ui.set_sidebar_playlists(slint::ModelRc::new(self.sidebar_playlists.clone()));
        ui.set_lib_playlists(slint::ModelRc::new(self.lib_playlists.clone()));
        ui.set_lib_artists(slint::ModelRc::new(self.lib_artists.clone()));
        ui.set_lib_album_rows(slint::ModelRc::new(self.lib_albums.rows.clone()));
        ui.set_songs(slint::ModelRc::new(self.songs.clone()));
        ui.set_page_tracks(slint::ModelRc::new(self.page_tracks.clone()));
        ui.set_artist_album_rows(slint::ModelRc::new(self.artist_albums.rows.clone()));
        ui.set_search_top(slint::ModelRc::new(self.search_top.clone()));
        ui.set_search_songs(slint::ModelRc::new(self.search_songs.clone()));
        ui.set_search_albums(slint::ModelRc::new(self.search_albums.clone()));
        ui.set_search_artists(slint::ModelRc::new(self.search_artists.clone()));
        ui.set_queue_rows(slint::ModelRc::new(self.queue.clone()));
    }

    // ——— artwork ————————————————————————————————————————————————————
    // A free function (not a method) so call sites can borrow `state`/`art`/
    // `ui` as disjoint fields of `self` in the same expression.

    fn art_of(
        state: &AppState,
        map: &mut HashMap<(u64, u32), slint::Image>,
        seed: u64,
        size: u32,
    ) -> slint::Image {
        if let Some(img) = map.get(&(seed, size)) {
            return img.clone();
        }
        let rgba = state.artwork.get(seed, size);
        let buf = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(
            rgba.as_slice(),
            size,
            size,
        );
        let img = slint::Image::from_rgba8(buf);
        map.insert((seed, size), img.clone());
        img
    }

    fn art(&mut self, seed: u64, size: u32) -> slint::Image {
        Self::art_of(&self.state, &mut self.art, seed, size)
    }

    // ——— row builders ————————————————————————————————————————————————

    fn track_row(&mut self, id: TrackId) -> TrackRow {
        let v = self.state.library.track_view(id);
        let seed = AppState::album_seed(self.state.library.track(id).album_id);
        TrackRow {
            id: id as i32,
            title: v.title.into(),
            artist: v.artist.into(),
            duration: mmss(v.duration_sec).into(),
            explicit: v.explicit,
            liked: v.liked,
            art: self.art(seed, 40),
        }
    }

    fn filled_card(&mut self, spec: &CardSpec, size: u32) -> CardData {
        CardData {
            id: spec.id as i32,
            kind: spec.kind,
            title: spec.title.clone().into(),
            subtitle: spec.subtitle.clone().into(),
            round: spec.round,
            art: self.art(spec.seed, size),
            needs_art: false,
        }
    }

    fn album_spec(&self, id: AlbumId) -> CardSpec {
        let album = self.state.library.album(id);
        let artist = self.state.library.artist(album.artist_id);
        CardSpec {
            id,
            kind: 0,
            title: album.title.clone(),
            subtitle: format!("{} · {}", artist.name, album.year),
            round: false,
            seed: AppState::album_seed(id),
        }
    }

    fn artist_spec(&self, id: ArtistId) -> CardSpec {
        let a = self.state.library.artist(id);
        CardSpec {
            id,
            kind: 1,
            title: a.name.clone(),
            subtitle: listeners(a.monthly_listeners),
            round: true,
            seed: AppState::artist_seed(id),
        }
    }

    fn playlist_spec(&self, id: PlaylistId) -> CardSpec {
        let p = self.state.library.playlist(id).expect("playlist exists");
        CardSpec {
            id,
            kind: 2,
            title: p.title.clone(),
            subtitle: p.description.clone().unwrap_or_else(|| "Made for you".into()),
            round: false,
            seed: AppState::playlist_seed(id),
        }
    }

    fn cols(&self) -> usize {
        // Same responsive math the reference implementation uses.
        let content_w =
            self.window_w - zuno_core::theme::SIDEBAR_W - 2.0 * zuno_core::theme::PAGE_PAD;
        ((content_w / (zuno_core::theme::CARD_W + 16.0)).floor() as usize).clamp(2, 8)
    }

    fn grid(&mut self, slot: GridSlot) -> &mut Grid {
        match slot {
            GridSlot::HomeNew => &mut self.home_new,
            GridSlot::LibAlbums => &mut self.lib_albums,
            GridSlot::ArtistAlbums => &mut self.artist_albums,
        }
    }

    /// Re-chunk a grid's specs into rows of `cols` cards. Cards are created
    /// art-empty (`needs-art: true`); visible ones ask Rust for art via the
    /// delegate `init` callback — so a 600-card grid never holds 74 MB of
    /// decoded covers.
    fn chunk_grid(&mut self, slot: GridSlot) {
        let cols = self.cols();
        let specs = self.grid(slot).specs.clone();
        let mut rows: Vec<GridRow> = Vec::new();
        let mut row_models: Vec<Rc<slint::VecModel<CardData>>> = Vec::new();
        for chunk in specs.chunks(cols) {
            let cards: Vec<CardData> = chunk
                .iter()
                .map(|s| CardData {
                    id: s.id as i32,
                    kind: s.kind,
                    title: s.title.clone().into(),
                    subtitle: s.subtitle.clone().into(),
                    round: s.round,
                    art: slint::Image::default(),
                    needs_art: true,
                })
                .collect();
            let rm = Rc::new(slint::VecModel::from(cards));
            rows.push(GridRow { cards: slint::ModelRc::new(rm.clone()) });
            row_models.push(rm);
        }
        let grid = self.grid(slot);
        grid.cols = cols;
        grid.rows.set_vec(rows);
        grid.row_models = row_models;
    }

    // ——— static (library-wide) models, built once ————————————————————

    fn build_static_models(&mut self) {
        // Home shelves: specs first (shared borrows), then art fill (&mut).
        let recent_specs: Vec<CardSpec> = self
            .state
            .library
            .home_recent_albums(12)
            .into_iter()
            .map(|id| self.album_spec(id))
            .collect();
        let recent: Vec<CardData> =
            recent_specs.iter().map(|s| self.filled_card(s, 176)).collect();
        self.home_recent.set_vec(recent);

        let mix_specs: Vec<CardSpec> = self
            .state
            .library
            .mixes
            .iter()
            .map(|m| self.playlist_spec(m.id))
            .collect();
        let mixes: Vec<CardData> =
            mix_specs.iter().map(|s| self.filled_card(s, 176)).collect();
        self.home_mixes.set_vec(mixes);

        let mut artist_ids: Vec<ArtistId> = (0..self.state.library.artists.len() as u32).collect();
        artist_ids
            .sort_by_key(|&r| std::cmp::Reverse(self.state.library.artist(r).monthly_listeners));
        artist_ids.truncate(12);
        let popular: Vec<CardData> = artist_ids
            .iter()
            .map(|&r| self.artist_spec(r))
            .collect::<Vec<_>>()
            .iter()
            .map(|s| self.filled_card(s, 176))
            .collect();
        self.home_artists.set_vec(popular);

        let mut new_ids: Vec<AlbumId> = (0..self.state.library.albums.len() as u32).collect();
        new_ids.sort_by_key(|&a| std::cmp::Reverse(self.state.library.album(a).year));
        new_ids.truncate(24);
        self.grid(GridSlot::HomeNew).specs =
            new_ids.iter().map(|&a| self.album_spec(a)).collect();
        self.chunk_grid(GridSlot::HomeNew);

        // Sidebar + Library ▸ Playlists (id, title, count, seed) then rows.
        let pl_info: Vec<(PlaylistId, String, u64)> = self
            .state
            .library
            .playlists
            .iter()
            .map(|p| {
                (
                    p.id,
                    count_songs(p.track_ids.len()),
                    AppState::playlist_seed(p.id),
                )
            })
            .collect();
        let sidebar: Vec<PlaylistRowData> = pl_info
            .iter()
            .map(|&(id, ref sub, seed)| {
                let p = self.state.library.playlist(id).unwrap();
                PlaylistRowData {
                    id: id as i32,
                    title: p.title.clone().into(),
                    sub: sub.clone().into(),
                    art: self.art(seed, 40),
                }
            })
            .collect();
        self.sidebar_playlists.set_vec(sidebar);
        let lib_pl: Vec<PlaylistRowData> = pl_info
            .iter()
            .map(|&(id, _, seed)| {
                let p = self.state.library.playlist(id).unwrap();
                PlaylistRowData {
                    id: id as i32,
                    title: p.title.clone().into(),
                    sub: format!("{} · playlist", count_songs(p.track_ids.len())).into(),
                    art: self.art(seed, 40),
                }
            })
            .collect();
        self.lib_playlists.set_vec(lib_pl);

        // Library ▸ Artists.
        let artist_info: Vec<(ArtistId, String, String, u64)> = (0
            ..self.state.library.artists.len() as u32)
            .map(|id| {
                let a = self.state.library.artist(id);
                (id, a.name.clone(), listeners(a.monthly_listeners), AppState::artist_seed(id))
            })
            .collect();
        let artists: Vec<ArtistRowData> = artist_info
            .iter()
            .map(|&(id, ref name, ref sub, seed)| ArtistRowData {
                id: id as i32,
                name: name.clone().into(),
                sub: sub.clone().into(),
                art: self.art(seed, 40),
            })
            .collect();
        self.lib_artists.set_vec(artists);

        // Library ▸ Albums grid (600 cards, lazy art per visible row).
        self.grid(GridSlot::LibAlbums).specs = (0..self.state.library.albums.len() as u32)
            .map(|id| self.album_spec(id))
            .collect();
        self.chunk_grid(GridSlot::LibAlbums);

        // The 5,000-row songs list. The full model exists from startup —
        // Slint's ListView only instantiates visible delegates.
        let n = self.state.library.tracks.len() as u32;
        let songs: Vec<TrackRow> = (0..n).map(|id| self.track_row(id)).collect();
        self.songs.set_vec(songs);
    }

    // ——— model refresh on navigation ————————————————————————————————

    fn refresh_page(&mut self) {
        // Extract everything from the library first (shared borrows), then
        // touch `self.art` / models (&mut).
        enum Hero {
            None,
            Collection { seed: u64, kind: String, title: String, sub: String, counts: String, round: bool },
            Artist { seed: u64, name: String, listeners: String, counts: String, top: Vec<TrackId>, album_specs: Vec<CardSpec> },
        }
        let hero = match self.state.view.clone() {
            View::Album(id) => {
                let album = self.state.library.album(id);
                let artist = self.state.library.artist(album.artist_id);
                let ids = album.track_ids.clone();
                let counts = counts_line(
                    ids.iter().map(|&t| self.state.library.track(t).duration_sec),
                );
                Hero::Collection {
                    seed: AppState::album_seed(id),
                    kind: album.kind.label().to_string(),
                    title: album.title.clone(),
                    sub: format!("Album · {}", artist.name),
                    counts,
                    round: false,
                }
            }
            View::Playlist(id) => {
                let p = self.state.library.playlist(id).expect("playlist exists");
                let ids = p.track_ids.clone();
                let counts = counts_line(
                    ids.iter().map(|&t| self.state.library.track(t).duration_sec),
                );
                Hero::Collection {
                    seed: AppState::playlist_seed(id),
                    kind: "Playlist".into(),
                    title: p.title.clone(),
                    sub: format!(
                        "Playlist · {}",
                        p.description.as_deref().unwrap_or("Curated by you")
                    ),
                    counts,
                    round: false,
                }
            }
            View::Artist(id) => {
                let a = self.state.library.artist(id);
                // Same "top tracks" definition as the reference implementation:
                // the artist's first album, reversed, first 10.
                let top: Vec<TrackId> = self
                    .state
                    .library
                    .albums
                    .iter()
                    .find(|al| al.artist_id == id)
                    .map(|al| al.track_ids.iter().rev().take(10).copied().collect())
                    .unwrap_or_default();
                let counts = counts_line(
                    top.iter().map(|&t| self.state.library.track(t).duration_sec),
                );
                let album_specs: Vec<CardSpec> =
                    a.album_ids.iter().map(|&aid| self.album_spec(aid)).collect();
                Hero::Artist {
                    seed: AppState::artist_seed(id),
                    name: a.name.clone(),
                    listeners: listeners(a.monthly_listeners),
                    counts,
                    top,
                    album_specs,
                }
            }
            _ => Hero::None,
        };

        // Now apply to the UI. Note: no long-lived `&self.ui` binding —
        // `self.ui.set_*(art_of(&self.state, &mut self.art, …))` borrows
        // three disjoint fields of self in one expression, which is fine.
        match self.state.view.clone() {
            View::Home => self.ui.set_page(PageTag::Home),
            View::Library => self.ui.set_page(PageTag::Library),
            View::Search => self.ui.set_page(PageTag::Search),
            View::Settings => self.ui.set_page(PageTag::Settings),
            View::Album(_) => self.ui.set_page(PageTag::Album),
            View::Playlist(_) => self.ui.set_page(PageTag::Playlist),
            View::Artist(_) => self.ui.set_page(PageTag::Artist),
        }
        match hero {
            Hero::Collection { seed, kind, title, sub, counts, round } => {
                self.ui.set_hero_art(Self::art_of(&self.state, &mut self.art, seed, 232));
                self.ui.set_hero_round(round);
                self.ui.set_hero_kind(kind.into());
                self.ui.set_hero_title(title.into());
                self.ui.set_hero_sub(sub.into());
                self.ui.set_hero_counts(counts.into());
                self.ui.set_hero_is_artist(false);
            }
            Hero::Artist { seed, name, listeners, counts, top, album_specs } => {
                self.ui.set_hero_art(Self::art_of(&self.state, &mut self.art, seed, 232));
                self.ui.set_hero_round(true);
                self.ui.set_hero_kind("Artist".into());
                self.ui.set_hero_title(name.into());
                self.ui.set_hero_sub(listeners.into());
                self.ui.set_hero_counts(counts.into());
                self.ui.set_hero_is_artist(true);
                self.grid(GridSlot::ArtistAlbums).specs = album_specs;
                self.chunk_grid(GridSlot::ArtistAlbums);
                self.page_ids = top;
            }
            Hero::None => {}
        }
        if let View::Album(id) = self.state.view {
            self.page_ids = self.state.library.album(id).track_ids.clone();
        }
        if let View::Playlist(id) = self.state.view {
            self.page_ids = self
                .state
                .library
                .playlist(id)
                .map(|p| p.track_ids.clone())
                .unwrap_or_default();
        }

        self.ui.set_library_tab(match self.state.library_tab {
            LibraryTab::Playlists => 0,
            LibraryTab::Albums => 1,
            LibraryTab::Artists => 2,
            LibraryTab::Songs => 3,
        });
        self.ui.set_active_playlist(match self.state.view {
            View::Playlist(id) => id as i32,
            _ => -1,
        });
        self.ui.set_can_go_back(self.state.can_go_back());
        self.ui.set_selected_index(self.state.selection.map(|i| i as i32).unwrap_or(-1));
        // Track rows for the current (non-library) view.
        if !matches!(self.state.view, View::Library | View::Home | View::Settings) {
            let ids = self.page_ids.clone();
            let rows: Vec<TrackRow> = ids.iter().map(|&id| self.track_row(id)).collect();
            self.page_tracks.set_vec(rows);
        }
    }

    fn refresh_search(&mut self) {
        let q = self.state.search_query.trim().to_string();
        self.ui.set_search_query(q.clone().into());
        self.ui.set_search_has_query(!q.is_empty());
        if q.is_empty() {
            self.search_top.set_vec(Vec::new());
            self.search_songs.set_vec(Vec::new());
            self.search_albums.set_vec(Vec::new());
            self.search_artists.set_vec(Vec::new());
            return;
        }
        let results = search::search(&self.state.library, &q);
        let top: Vec<CardData> = results
            .tracks
            .first()
            .map(|&id| {
                let v = self.state.library.track_view(id);
                let spec = CardSpec {
                    id,
                    kind: 3,
                    title: v.title.to_string(),
                    subtitle: format!("Song · {}", v.artist),
                    round: false,
                    seed: AppState::album_seed(self.state.library.track(id).album_id),
                };
                self.filled_card(&spec, 176)
            })
            .into_iter()
            .collect();
        self.search_top.set_vec(top);
        self.page_ids = results.tracks.clone();
        let rows: Vec<TrackRow> =
            results.tracks.iter().map(|&id| self.track_row(id)).collect();
        self.search_songs.set_vec(rows);
        let album_specs: Vec<CardSpec> =
            results.albums.iter().map(|&a| self.album_spec(a)).collect();
        let albums: Vec<CardData> =
            album_specs.iter().map(|s| self.filled_card(s, 176)).collect();
        self.search_albums.set_vec(albums);
        let artist_specs: Vec<CardSpec> =
            results.artists.iter().map(|&a| self.artist_spec(a)).collect();
        let artists: Vec<CardData> =
            artist_specs.iter().map(|s| self.filled_card(s, 176)).collect();
        self.search_artists.set_vec(artists);
    }

    // ——— queue panel ————————————————————————————————————————————————

    fn refresh_queue(&mut self) {
        let rows_src = self.state.queue_rows();
        let mut rows: Vec<QueueRowData> = Vec::with_capacity(rows_src.len());
        let mut last_region: Option<Region> = None;
        for (i, (id, region)) in rows_src.iter().enumerate() {
            let header = if Some(*region) != last_region {
                match region {
                    Region::Played => "",
                    Region::Current => "NOW PLAYING",
                    Region::Manual => "NEXT IN QUEUE",
                    Region::Automatic => "NEXT UP",
                }
                .to_string()
            } else {
                String::new()
            };
            last_region = Some(*region);
            let v = self.state.library.track_view(*id);
            let region_ix = match region {
                Region::Played => 0,
                Region::Current => 1,
                Region::Manual => 2,
                Region::Automatic => 3,
            };
            rows.push(QueueRowData {
                index: i as i32,
                region: region_ix,
                header: header.into(),
                title: v.title.into(),
                artist: v.artist.into(),
                duration: mmss(v.duration_sec).into(),
                art: self.art(
                    AppState::album_seed(self.state.library.track(*id).album_id),
                    32,
                ),
            });
        }
        self.queue.set_vec(rows);
        self.queue_sig = (
            self.state.current,
            self.state.queue.history().len(),
            self.state.queue.manual().len(),
            self.state.queue.automatic().len(),
        );
    }

    // ——— per-frame player sync ————————————————————————————————————————

    fn sync_player(&mut self) {
        self.ui.set_playing(self.state.playing);
        self.ui.set_current_id(self.state.current.map(|i| i as i32).unwrap_or(-1));
        self.ui.set_shuffle(self.state.shuffle);
        self.ui.set_repeat(match self.state.repeat {
            RepeatMode::Off => 0,
            RepeatMode::All => 1,
            RepeatMode::One => 2,
        });
        self.ui.set_volume(self.state.volume);
        self.ui.set_muted(self.state.muted);
        self.ui.set_position(self.state.player.position_sec() as f32);
        let dur = self.state.player.duration_sec();
        self.ui.set_track_duration(dur.max(1.0) as f32);
        self.ui.set_position_label(mmss(self.state.player.position_sec() as u32).into());
        self.ui.set_duration_label(mmss(dur.max(0.0) as u32).into());
        if self.state.current != self.last_current {
            self.last_current = self.state.current;
            match self.state.current_track() {
                Some(track) => {
                    let (title, artist_name, album_id, artist_id, liked) = (
                        track.title.clone(),
                        self.state.library.artist(track.artist_id).name.clone(),
                        track.album_id,
                        track.artist_id,
                        track.liked,
                    );
                    self.ui.set_has_track(true);
                    self.ui.set_pb_title(title.into());
                    self.ui.set_pb_artist(artist_name.into());
                    self.ui.set_pb_art(Self::art_of(
                        &self.state,
                        &mut self.art,
                        AppState::album_seed(album_id),
                        40,
                    ));
                    self.ui.set_pb_album_id(album_id as i32);
                    self.ui.set_pb_artist_id(artist_id as i32);
                    self.ui.set_pb_liked(liked);
                    self.last_liked = liked;
                }
                None => {
                    self.ui.set_has_track(false);
                    self.ui.set_pb_title("Nothing playing".into());
                    self.ui.set_pb_artist("—".into());
                    self.ui.set_pb_liked(false);
                }
            }
        } else if let Some(track) = self.state.current_track() {
            if track.liked != self.last_liked {
                self.last_liked = track.liked;
                self.ui.set_pb_liked(track.liked);
            }
        }
    }

    // ——— likes ————————————————————————————————————————————————————

    fn after_like(&mut self, id: TrackId) {
        // Update the row wherever it is visible (library songs list is
        // index == id; the current page's lists are scanned).
        if matches!(self.state.view, View::Library)
            && self.state.library_tab == LibraryTab::Songs
        {
            if let Some(r) = self.songs.row_data(id as usize) {
                self.songs
                    .set_row_data(id as usize, TrackRow { liked: !r.liked, ..r });
            }
        }
        if let Some(pos) = self.page_ids.iter().position(|&x| x == id) {
            if let Some(r) = self.page_tracks.row_data(pos) {
                self.page_tracks
                    .set_row_data(pos, TrackRow { liked: !r.liked, ..r });
            }
            if let Some(r) = self.search_songs.row_data(pos) {
                self.search_songs
                    .set_row_data(pos, TrackRow { liked: !r.liked, ..r });
            }
        }
        // Liked Songs count rows in sidebar + library playlists.
        let liked = self.state.library.playlists[0].track_ids.len();
        let sub: slint::SharedString = count_songs(liked).into();
        if let Some(mut r0) = self.sidebar_playlists.row_data(0) {
            r0.sub = sub.clone();
            self.sidebar_playlists.set_row_data(0, r0);
        }
        if let Some(mut r0) = self.lib_playlists.row_data(0) {
            r0.sub = format!("{} · playlist", count_songs(liked)).into();
            self.lib_playlists.set_row_data(0, r0);
        }
    }

    // ——— bench helpers ——————————————————————————————————————————————

    fn ensure_library_songs(&mut self) {
        let mut dirty = false;
        if !matches!(self.state.view, View::Library) {
            self.state.view = View::Library;
            dirty = true;
        }
        if self.state.library_tab != LibraryTab::Songs {
            self.state.library_tab = LibraryTab::Songs;
            dirty = true;
        }
        if dirty {
            self.refresh_page();
        }
    }
}

fn wire(ui: &ZunoWindow, zuno: &Rc<RefCell<Zuno>>) {
    let weak = zuno.clone();
    ui.on_nav_home(move || {
        let mut z = weak.borrow_mut();
        z.state.go(View::Home);
        z.refresh_page();
    });
    let weak = zuno.clone();
    ui.on_nav_search(move || {
        let mut z = weak.borrow_mut();
        z.state.go(View::Search);
        z.refresh_page();
    });
    let weak = zuno.clone();
    ui.on_nav_library(move || {
        let mut z = weak.borrow_mut();
        z.state.go(View::Library);
        z.refresh_page();
    });
    let weak = zuno.clone();
    ui.on_nav_settings(move || {
        let mut z = weak.borrow_mut();
        z.state.go(View::Settings);
        z.refresh_page();
    });
    let weak = zuno.clone();
    ui.on_open_album(move |id| {
        let mut z = weak.borrow_mut();
        z.state.open_album(id.max(0) as u32);
        z.refresh_page();
    });
    let weak = zuno.clone();
    ui.on_open_artist(move |id| {
        let mut z = weak.borrow_mut();
        z.state.open_artist(id.max(0) as u32);
        z.refresh_page();
    });
    let weak = zuno.clone();
    ui.on_open_playlist(move |id| {
        let mut z = weak.borrow_mut();
        z.state.open_playlist(id.max(0) as u32);
        z.refresh_page();
    });
    let weak = zuno.clone();
    ui.on_card_click(move |kind, id| {
        let mut z = weak.borrow_mut();
        match kind {
            0 => z.state.open_album(id.max(0) as u32),
            1 => z.state.open_artist(id.max(0) as u32),
            2 => z.state.open_playlist(id.max(0) as u32),
            _ => {
                let tid = id.max(0) as u32;
                z.state.play_track(tid);
            }
        }
        z.refresh_page();
    });
    let weak = zuno.clone();
    ui.on_set_tab(move |ix| {
        let mut z = weak.borrow_mut();
        let tab = match ix {
            0 => LibraryTab::Playlists,
            1 => LibraryTab::Albums,
            2 => LibraryTab::Artists,
            _ => LibraryTab::Songs,
        };
        z.state.set_library_tab(tab);
        z.ui.set_library_tab(ix);
    });
    let weak = zuno.clone();
    ui.on_search_edited(move |t| {
        let mut z = weak.borrow_mut();
        z.state.set_search(&t);
        z.refresh_search();
        z.refresh_page();
    });
    let weak = zuno.clone();
    ui.on_play_from(move |ix| {
        let mut z = weak.borrow_mut();
        let ids = z.page_ids.clone();
        if !ids.is_empty() {
            let i = (ix.max(0) as usize).min(ids.len() - 1);
            z.state.play_from(&ids, i);
        }
    });
    let weak = zuno.clone();
    ui.on_shuffle_play(move || {
        let mut z = weak.borrow_mut();
        let ids = z.page_ids.clone();
        if !ids.is_empty() {
            z.state.play_from(&ids, 0);
            if !z.state.shuffle {
                z.state.toggle_shuffle();
            }
        }
    });
    let weak = zuno.clone();
    ui.on_play_all(move || {
        let mut z = weak.borrow_mut();
        let ids = z.page_ids.clone();
        if !ids.is_empty() {
            z.state.play_from(&ids, 0);
        }
    });
    let weak = zuno.clone();
    ui.on_toggle_like(move |id| {
        let mut z = weak.borrow_mut();
        if id < 0 {
            return;
        }
        let tid = id as u32;
        z.state.toggle_like(tid);
        z.after_like(tid);
    });
    let weak = zuno.clone();
    ui.on_toggle_play(move || weak.borrow_mut().state.toggle_play());
    let weak = zuno.clone();
    ui.on_next_track(move || weak.borrow_mut().state.next());
    let weak = zuno.clone();
    ui.on_prev_track(move || weak.borrow_mut().state.previous());
    let weak = zuno.clone();
    ui.on_seek_to(move |f| {
        let mut z = weak.borrow_mut();
        z.state.seek(f.max(0.0) as f64);
    });
    let weak = zuno.clone();
    ui.on_seek_by(move |d| {
        let mut z = weak.borrow_mut();
        let pos = z.state.player.position_sec();
        z.state.seek(pos + d as f64);
    });
    let weak = zuno.clone();
    ui.on_set_volume(move |f| {
        let mut z = weak.borrow_mut();
        z.state.set_volume(f.clamp(0.0, 1.0));
        z.ui.set_volume(z.state.volume);
    });
    let weak = zuno.clone();
    ui.on_toggle_mute(move || weak.borrow_mut().state.toggle_mute());
    let weak = zuno.clone();
    ui.on_toggle_shuffle(move || weak.borrow_mut().state.toggle_shuffle());
    let weak = zuno.clone();
    ui.on_cycle_repeat(move || weak.borrow_mut().state.cycle_repeat());
    let weak = zuno.clone();
    ui.on_toggle_queue(move || {
        let mut z = weak.borrow_mut();
        z.queue_open = !z.queue_open;
        z.ui.set_queue_open(z.queue_open);
        if z.queue_open {
            z.refresh_queue();
        }
    });
    let weak = zuno.clone();
    ui.on_queue_jump(move |ix| {
        let mut z = weak.borrow_mut();
        if let Some(id) = z.state.queue.jump_to(ix.max(0) as usize) {
            z.state.play_track(id);
        }
        z.refresh_queue();
    });
    let weak = zuno.clone();
    ui.on_queue_remove(move |ix| {
        let mut z = weak.borrow_mut();
        z.state.queue.remove_at(ix.max(0) as usize);
        z.refresh_queue();
    });
    let weak = zuno.clone();
    ui.on_go_back(move || {
        let mut z = weak.borrow_mut();
        z.state.go_back();
        z.refresh_page();
    });
    let weak = zuno.clone();
    ui.on_escape(move || {
        let mut z = weak.borrow_mut();
        if !z.state.search_query.is_empty() && matches!(z.state.view, View::Search) {
            z.state.search_query.clear();
            z.refresh_search();
        } else {
            z.state.go_back();
        }
        z.refresh_page();
    });
    let weak = zuno.clone();
    {
        let ui_weak = ui.as_weak();
        ui.on_ctrl_k(move || {
            let mut z = weak.borrow_mut();
            if !matches!(z.state.view, View::Search) {
                z.state.go(View::Search);
                z.refresh_page();
            }
            // Focus after the page becomes visible: one event-loop turn later.
            let w = ui_weak.clone();
            slint::invoke_from_event_loop(move || {
                if let Some(u) = w.upgrade() {
                    u.invoke_focus_search_field();
                }
            })
            .unwrap();
        });
    }

    // Lazy grid-card artwork: fill the (row, col) the delegate asked for.
    ui.on_card_art_home(make_art_cb(zuno, GridSlot::HomeNew));
    ui.on_card_art_library(make_art_cb(zuno, GridSlot::LibAlbums));
    ui.on_card_art_artist(make_art_cb(zuno, GridSlot::ArtistAlbums));
}

fn make_art_cb(zuno: &Rc<RefCell<Zuno>>, slot: GridSlot) -> impl Fn(i32, i32) {
    let zuno = zuno.clone();
    move |row: i32, col: i32| {
        let mut z = zuno.borrow_mut();
        let row = row.max(0) as usize;
        let col = col.max(0) as usize;
        // Find the seed (and the row model) without holding a &mut borrow.
        let seed = {
            let grid = z.grid(slot);
            let cols = grid.cols;
            let Some(rm) = grid.row_models.get(row).cloned() else { return };
            let Some(card) = rm.row_data(col) else { return };
            if !card.needs_art {
                return;
            }
            let Some(spec) = grid.specs.get(row * cols + col) else { return };
            spec.seed
        };
        let art = z.art(seed, 176);
        let grid = z.grid(slot);
        if let Some(rm) = grid.row_models.get(row).cloned() {
            if let Some(card) = rm.row_data(col) {
                rm.set_row_data(col, CardData { art, needs_art: false, ..card });
            }
        }
    }
}

fn tick(zuno: &Rc<RefCell<Zuno>>) {
    let mut z = zuno.borrow_mut();
    z.state.tick();

    // Window width poll → responsive grid re-chunk + sidebar collapse.
    let w = z.ui.window().size().width as f32 / z.ui.window().scale_factor() as f32;
    if (w - z.window_w).abs() > 1.0 {
        z.window_w = w;
        z.ui.set_compact(w < 1000.0);
        z.chunk_grid(GridSlot::HomeNew);
        z.chunk_grid(GridSlot::LibAlbums);
        if matches!(z.state.view, View::Artist(_)) {
            z.chunk_grid(GridSlot::ArtistAlbums);
        }
    }

    if let Some(b) = &mut z.bench {
        match b.tick() {
            BenchAction::RecordStartup => {}
            BenchAction::ScrollTo(f) => {
                z.ensure_library_songs();
                z.ui.invoke_bench_scroll(f);
            }
            BenchAction::SelectRow(i) => {
                z.state.selection = Some(i);
                z.ui.set_selected_index(i as i32);
            }
            BenchAction::StartPlayback => {
                let ids = z.state.list_ids();
                z.state.play_from(&ids, ids.len() / 2);
            }
            BenchAction::Finish => {
                println!("{}", b.report);
                // stdout is block-buffered when piped; flush before exit or
                // the report is lost.
                use std::io::Write;
                std::io::stdout().flush().unwrap();
                std::process::exit(0);
            }
            BenchAction::Nothing => {}
        }
    }

    z.sync_player();

    // Queue model rebuild only when the queue actually changed.
    let sig = (
        z.state.current,
        z.state.queue.history().len(),
        z.state.queue.manual().len(),
        z.state.queue.automatic().len(),
    );
    if sig != z.queue_sig {
        z.refresh_queue();
    }
}

fn main() {
    let ui = ZunoWindow::new().expect("failed to create Zuno window");
    let zuno = Rc::new(RefCell::new(Zuno::new(ui.clone_strong())));
    zuno.borrow_mut().refresh_page();
    wire(&ui, &zuno);
    ui.show().expect("failed to show window");

    let timer = slint::Timer::default();
    {
        let z = zuno.clone();
        timer.start(
            slint::TimerMode::Repeated,
            Duration::from_millis(16),
            move || tick(&z),
        );
    }
    slint::run_event_loop().expect("event loop failed");
}
