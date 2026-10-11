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
use std::sync::mpsc::{channel, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};
use slint::Model;
use zuno_core::app::{LibraryTab, RepeatMode};
use zuno_core::bench::{BenchAction, BenchDriver};
use zuno_core::format::{count_songs, counts_line, listeners, mmss};
use zuno_core::model::*;
use zuno_core::queue::Region;
use zuno_core::search;
use zuno_core::{AppState, View};
use zuno_yt::{
    Rating, Session, YtAlbum, YtArtist, YtClient, YtClientExt, YtHome, YtLibrary, YtMutations,
    YtPlaylist, YtPlaylistHeader, YtSearchResults, YtTrack,
};

/// A card source before it becomes `CardData` (art filled on demand).
#[derive(Clone)]
struct CardSpec {
    id: u32,
    kind: i32,
    title: String,
    subtitle: String,
    round: bool,
    seed: u64,
    /// Remote cover URL in live-YouTube mode; `None` = procedural cover.
    art_url: Option<String>,
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

// ——— real-YouTube background plumbing ———————————————————————————
// Slint owns the UI thread (`Rc<RefCell<Zuno>>` is !Send), so every network
// call runs on a short-lived worker thread with its own client +
// current-thread tokio runtime, and posts a `YtMsg` back. The 16 ms `tick`
// drains the channel — no locks, no cross-thread UI access. `--bench`
// never spawns and never applies: the bench path is byte-identical.

/// Fetched-then-decode-on-UI artwork: (url, size, bytes). The worker does
/// network only (`fetch_artwork`); the UI thread runs `insert_bytes` (the
/// network-free half of `get_url`) before rebuilding models, so first
/// paint of real data already has covers and the UI thread never blocks
/// on the network for warmed URLs.
type ArtBatch = Vec<(String, u32, Vec<u8>)>;

enum YtMsg {
    Home { home: YtHome, mixes: Vec<YtPlaylistHeader>, art: ArtBatch },
    HomeFailed(String),
    Search { gen: u64, results: YtSearchResults, art: ArtBatch },
    SearchFailed { gen: u64, err: String },
    DetailAlbum { album: YtAlbum, art: ArtBatch },
    DetailArtist { artist: YtArtist, art: ArtBatch },
    DetailPlaylist { id: String, title: String, tracks: Vec<YtTrack>, art: ArtBatch },
    DetailFailed(String),
    Stream { id: TrackId, url: String, mime: String },
    StreamFailed { id: TrackId },
    SignedIn { session: Session, home: YtHome, mixes: Vec<YtPlaylistHeader>, art: ArtBatch },
    SignInFailed(String),
}

fn yt_client(cookie: Option<&str>) -> Result<YtClient, String> {
    match cookie {
        Some(c) if !c.trim().is_empty() => {
            let mut s = Session::default();
            s.cookie_header = c.to_string();
            YtClient::with_session(s).map_err(|e| e.to_string())
        }
        _ => YtClient::unsigned().map_err(|e| e.to_string()),
    }
}

fn yt_rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("yt runtime")
}

/// Distinct track artwork urls, first-seen order.
fn track_art_urls(tracks: &[YtTrack]) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for t in tracks {
        if let Some(u) = t.artwork_url() {
            if seen.insert(u) {
                out.push(u.to_string());
            }
        }
    }
    out
}

/// Blocking thumb fetch on a worker thread, fanned out so one slow thumb
/// doesn't stall the batch. Failures are dropped: `get_url`'s procedural
/// fallback covers them at paint time.
fn yt_fetch_art(plans: &[(String, u32)]) -> ArtBatch {
    if plans.is_empty() {
        return Vec::new();
    }
    let out = std::sync::Mutex::new(Vec::new());
    let n = plans.len().div_ceil(6).max(1);
    std::thread::scope(|s| {
        for chunk in plans.chunks(n) {
            let out = &out;
            s.spawn(move || {
                let mut got = Vec::new();
                for (url, size) in chunk {
                    if let Ok(bytes) = zuno_yt::fetch_artwork(url) {
                        got.push((url.clone(), *size, bytes));
                    }
                }
                out.lock().unwrap().extend(got);
            });
        }
    });
    out.into_inner().unwrap()
}

/// Warm plan for a home-sized track set: cards at 176, rows at 40.
fn home_art_plans(tracks: &[YtTrack], mixes: &[YtPlaylistHeader]) -> Vec<(String, u32)> {
    let urls = track_art_urls(tracks);
    let mut plans = Vec::new();
    for u in urls.iter().take(12) {
        plans.push((u.clone(), 176));
    }
    for u in urls.iter().take(24) {
        plans.push((u.clone(), 40));
    }
    for m in mixes.iter().take(12) {
        if let Some(u) = zuno_yt::pick_artwork(&m.thumbnails) {
            plans.push((u.to_string(), 176));
        }
    }
    plans
}

fn mixes_of(home: &YtHome) -> Vec<YtPlaylistHeader> {
    home.shelves
        .iter()
        .flat_map(|s| s.playlists.iter().cloned())
        .take(12)
        .collect()
}

fn home_tracks_of(home: &YtHome) -> Vec<YtTrack> {
    // Same expression `YtLibrary::from_home` builds from — ids stay parallel.
    home.shelves.iter().flat_map(|s| s.tracks.clone()).collect()
}

fn spawn_home_load(tx: Sender<YtMsg>, cookie: Option<String>) {
    thread::spawn(move || {
        let msg = match yt_client(cookie.as_deref()) {
            Err(e) => YtMsg::HomeFailed(e),
            Ok(mut yt) => match yt_rt().block_on(yt.get_home()) {
                Err(e) => YtMsg::HomeFailed(e.to_string()),
                Ok(home) => {
                    let tracks = home_tracks_of(&home);
                    let mixes = mixes_of(&home);
                    let art = yt_fetch_art(&home_art_plans(&tracks, &mixes));
                    eprintln!("[yt] home: {} shelves, {} tracks", home.shelves.len(), tracks.len());
                    YtMsg::Home { home, mixes, art }
                }
            },
        };
        let _ = tx.send(msg);
    });
}

fn spawn_search(tx: Sender<YtMsg>, cookie: Option<String>, gen: u64, q: String) {
    thread::spawn(move || {
        let msg = match yt_client(cookie.as_deref()) {
            Err(e) => YtMsg::SearchFailed { gen, err: e },
            Ok(mut yt) => match yt_rt().block_on(yt.search(&q)) {
                Err(e) => YtMsg::SearchFailed { gen, err: e.to_string() },
                Ok(results) => {
                    let urls = track_art_urls(&results.tracks);
                    let mut plans: Vec<(String, u32)> = Vec::new();
                    for u in urls.iter().take(10) {
                        plans.push((u.clone(), 176));
                    }
                    for u in urls.iter().take(20) {
                        plans.push((u.clone(), 40));
                    }
                    let art = yt_fetch_art(&plans);
                    eprintln!("[yt] search {q:?}: {} tracks", results.tracks.len());
                    YtMsg::Search { gen, results, art }
                }
            },
        };
        let _ = tx.send(msg);
    });
}

fn spawn_album(tx: Sender<YtMsg>, cookie: Option<String>, browse_id: String) {
    thread::spawn(move || {
        let msg = match yt_client(cookie.as_deref()) {
            Err(e) => YtMsg::DetailFailed(e),
            Ok(mut yt) => match yt_rt().block_on(yt.get_album(&browse_id)) {
                Err(e) => YtMsg::DetailFailed(format!("album: {e}")),
                Ok(album) => {
                    let urls = track_art_urls(&album.tracks);
                    let mut plans: Vec<(String, u32)> = urls
                        .iter()
                        .take(24)
                        .map(|u| (u.clone(), 40))
                        .collect();
                    if let Some(u) = zuno_yt::pick_artwork(&album.thumbnails) {
                        plans.push((u.to_string(), 232));
                    }
                    let art = yt_fetch_art(&plans);
                    eprintln!("[yt] album {:?} ({} tracks)", album.title, album.tracks.len());
                    YtMsg::DetailAlbum { album, art }
                }
            },
        };
        let _ = tx.send(msg);
    });
}

fn spawn_artist(tx: Sender<YtMsg>, cookie: Option<String>, channel_id: String) {
    thread::spawn(move || {
        let msg = match yt_client(cookie.as_deref()) {
            Err(e) => YtMsg::DetailFailed(e),
            Ok(mut yt) => match yt_rt().block_on(yt.get_artist(&channel_id)) {
                Err(e) => YtMsg::DetailFailed(format!("artist: {e}")),
                Ok(artist) => {
                    let urls = track_art_urls(&artist.top_tracks);
                    let mut plans: Vec<(String, u32)> = urls
                        .iter()
                        .take(24)
                        .map(|u| (u.clone(), 40))
                        .collect();
                    if let Some(u) = zuno_yt::pick_artwork(&artist.thumbnails) {
                        plans.push((u.to_string(), 232));
                    }
                    for a in artist.albums.iter().take(12) {
                        if let Some(u) = zuno_yt::pick_artwork(&a.thumbnails) {
                            plans.push((u.to_string(), 176));
                        }
                    }
                    let art = yt_fetch_art(&plans);
                    eprintln!("[yt] artist {:?} ({} top tracks)", artist.name, artist.top_tracks.len());
                    YtMsg::DetailArtist { artist, art }
                }
            },
        };
        let _ = tx.send(msg);
    });
}

fn spawn_playlist(tx: Sender<YtMsg>, cookie: Option<String>, id: String, title: String) {
    thread::spawn(move || {
        let msg = match yt_client(cookie.as_deref()) {
            Err(e) => YtMsg::DetailFailed(e),
            Ok(mut yt) => {
                // Whole list — pagination + termination guard inside.
                match yt_rt().block_on(yt.get_playlist_tracks(&id)) {
                    Err(e) => YtMsg::DetailFailed(format!("playlist: {e}")),
                    Ok(tracks) => {
                        let urls = track_art_urls(&tracks);
                        let plans: Vec<(String, u32)> = urls
                            .iter()
                            .take(24)
                            .map(|u| (u.clone(), 40))
                            .collect();
                        let art = yt_fetch_art(&plans);
                        eprintln!("[yt] playlist {title:?} ({} tracks)", tracks.len());
                        YtMsg::DetailPlaylist { id, title, tracks, art }
                    }
                }
            }
        };
        let _ = tx.send(msg);
    });
}

fn spawn_stream(tx: Sender<YtMsg>, cookie: Option<String>, id: TrackId, video_id: String) {
    thread::spawn(move || {
        let r = yt_client(cookie.as_deref()).and_then(|mut yt| {
            yt_rt()
                .block_on(zuno_yt::resolve_stream_url(&mut yt, &video_id))
                .map_err(|e| e.to_string())
        });
        let msg = match r {
            Ok(info) => {
                eprintln!("[yt] stream {video_id}: {} {}kbps", info.mime, info.bitrate / 1000);
                YtMsg::Stream { id, url: info.url, mime: info.mime }
            }
            Err(e) => {
                eprintln!("[yt] resolve failed for {video_id}: {e}");
                YtMsg::StreamFailed { id }
            }
        };
        let _ = tx.send(msg);
    });
}

fn spawn_sign_in(tx: Sender<YtMsg>, paste: String) {
    thread::spawn(move || {
        let session = Session::import(&paste);
        if session.is_empty() {
            let _ = tx.send(YtMsg::SignInFailed("Paste a YouTube Cookie header first.".into()));
            return;
        }
        let msg = match YtClient::with_session(session.clone()) {
            Err(e) => YtMsg::SignInFailed(e.to_string()),
            Ok(mut yt) => match yt_rt().block_on(yt.get_library()) {
                Err(e) => YtMsg::SignInFailed(format!("Library load failed: {e}")),
                Ok(home) => {
                    let tracks = home_tracks_of(&home);
                    let mixes = mixes_of(&home);
                    let art = yt_fetch_art(&home_art_plans(&tracks, &mixes));
                    YtMsg::SignedIn { session, home, mixes, art }
                }
            },
        };
        let _ = tx.send(msg);
    });
}

struct Zuno {
    state: AppState,
    art: HashMap<(u64, u32), slint::Image>,
    url_art: HashMap<(String, u32), slint::Image>,
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

    // ——— real-YouTube wiring (all dead in `--bench`) ———
    yt_tx: Sender<YtMsg>,
    yt_rx: Receiver<YtMsg>,
    /// Home data swapped in: search/play/detail go to the network.
    yt_live: bool,
    yt_loading: bool,
    session: Option<Session>,
    /// Raw YT tracks parallel to `state.library.tracks` (same order — the
    /// `YtLibrary` adapters assign sequential ids). Empty in synth mode.
    /// Lets card clicks recover browse/channel ids the adapter drops.
    yt_tracks: Vec<YtTrack>,
    /// Chart-playlist headers behind the home mixes shelf (id order matches
    /// the `home_mixes` cards); the swapped library carries no mixes.
    yt_mixes: Vec<YtPlaylistHeader>,
    /// YT playlist id behind the current single-playlist library, if any.
    yt_pl_id: Option<String>,
    /// Raw home data to restore when leaving search/detail views
    /// (rebuilt deterministically — the art cache already holds covers).
    yt_home: Option<(YtHome, Vec<YtPlaylistHeader>)>,
    search_gen: u64,
    search_at: Option<Instant>,
    search_q: String,
    /// List a stream play started from (Repeat::All wrap target).
    yt_source: Vec<TrackId>,
    yt_status: String,
}

impl Zuno {
    fn new(ui: ZunoWindow) -> Self {
        let state = AppState::new();
        let bench = std::env::args().any(|a| a == "--bench");
        let (yt_tx, yt_rx) = channel::<YtMsg>();
        let empty_grid = || Grid {
            specs: Vec::new(),
            rows: Rc::new(slint::VecModel::default()),
            row_models: Vec::new(),
            cols: 0,
        };
        let mut z = Zuno {
            state,
            art: HashMap::new(),
            url_art: HashMap::new(),
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
            bench: bench.then(BenchDriver::new),
            yt_tx,
            yt_rx,
            yt_live: false,
            yt_loading: false,
            session: None,
            yt_tracks: Vec::new(),
            yt_mixes: Vec::new(),
            yt_pl_id: None,
            yt_home: None,
            search_gen: 0,
            search_at: None,
            search_q: String::new(),
            yt_source: Vec::new(),
            yt_status: String::new(),
        };
        z.build_static_models();
        z.connect_models();
        // Synthetic first paint stays; real home loads in the background.
        // `--bench` never spawns: same library, same synth, same numbers.
        if bench {
            return z;
        }
        let saved = Session::load();
        if let Some(ref s) = saved {
            if !s.is_empty() {
                z.session = saved;
                let who = z
                    .session
                    .as_ref()
                    .and_then(|s| s.account_identity())
                    .map(|id| format!(" ({}…)", &id[..id.len().min(6)]))
                    .unwrap_or_default();
                z.set_yt_status(format!("Signed in{who} — loading home…"));
            }
        }
        if z.yt_status.is_empty() {
            z.set_yt_status("Connecting to YouTube Music…".to_string());
        }
        z.yt_loading = true;
        spawn_home_load(z.yt_tx.clone(), z.cookie());
        z
    }

    fn is_bench(&self) -> bool {
        self.bench.is_some()
    }

    fn cookie(&self) -> Option<String> {
        self.session.as_ref().map(|s| s.cookie_header.clone())
    }

    fn set_yt_status(&mut self, s: String) {
        self.yt_status = s.clone();
        self.ui.set_yt_status(s.into());
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

    /// Remote cover via the shared cache (`get_url` — same 96-cover LRU;
    /// any fetch/decode failure falls back to the procedural tile, so
    /// callers never branch on network state). Cache-hit fast; call sites
    /// warm the cache on a worker thread before first display.
    fn art_url_of(
        state: &AppState,
        map: &mut HashMap<(String, u32), slint::Image>,
        url: &str,
        size: u32,
    ) -> slint::Image {
        let key = (url.to_string(), size);
        if let Some(img) = map.get(&key) {
            return img.clone();
        }
        let rgba = state.artwork.get_url(url, size);
        let buf = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(
            rgba.as_slice(),
            size,
            size,
        );
        let img = slint::Image::from_rgba8(buf);
        map.insert(key, img.clone());
        img
    }

    fn art_url(&mut self, url: &str, size: u32) -> slint::Image {
        Self::art_url_of(&self.state, &mut self.url_art, url, size)
    }

    /// Row/card art: remote URL when the track has one (live mode),
    /// procedural seed otherwise (synth/`--bench`/missing URL).
    fn track_art(&mut self, id: TrackId, size: u32) -> slint::Image {
        let (url, seed) = {
            let t = self.state.library.track(id);
            (
                t.artwork_url.clone(),
                AppState::album_seed(t.album_id),
            )
        };
        match url {
            Some(u) => self.art_url(&u, size),
            None => self.art(seed, size),
        }
    }

    // ——— row builders ————————————————————————————————————————————————

    fn track_row(&mut self, id: TrackId) -> TrackRow {
        let art = self.track_art(id, 40);
        let v = self.state.library.track_view(id);
        TrackRow {
            id: id as i32,
            title: v.title.into(),
            artist: v.artist.into(),
            duration: mmss(v.duration_sec).into(),
            explicit: v.explicit,
            liked: v.liked,
            art,
        }
    }

    fn filled_card(&mut self, spec: &CardSpec, size: u32) -> CardData {
        let art = match &spec.art_url {
            Some(u) => self.art_url(u, size),
            None => self.art(spec.seed, size),
        };
        CardData {
            id: spec.id as i32,
            kind: spec.kind,
            title: spec.title.clone().into(),
            subtitle: spec.subtitle.clone().into(),
            round: spec.round,
            art,
            needs_art: false,
        }
    }

    fn album_spec(&self, id: AlbumId) -> CardSpec {
        let album = self.state.library.album(id);
        let artist = self.state.library.artist(album.artist_id);
        // YouTube leaves year unknown (0) — don't print it.
        let subtitle = if album.year == 0 {
            artist.name.clone()
        } else {
            format!("{} · {}", artist.name, album.year)
        };
        CardSpec {
            id,
            kind: 0,
            title: album.title.clone(),
            subtitle,
            round: false,
            seed: AppState::album_seed(id),
            art_url: album.artwork_url.clone(),
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
            art_url: a.artwork_url.clone(),
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
            art_url: p.artwork_url.clone(),
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

        let mix_specs: Vec<CardSpec> = if self.yt_live {
            // The swapped library carries no mixes; chart playlists come
            // from the raw home shelves (card ids index `yt_mixes` below).
            self.yt_mixes
                .iter()
                .map(|m| CardSpec {
                    id: 0,
                    kind: 2,
                    title: m.title.clone(),
                    subtitle: "Playlist · YouTube Music".to_string(),
                    round: false,
                    seed: 0,
                    art_url: zuno_yt::pick_artwork(&m.thumbnails).map(str::to_string),
                })
                .collect()
        } else {
            self.state
                .library
                .mixes
                .iter()
                .map(|m| self.playlist_spec(m.id))
                .collect()
        };
        // Mix card ids index `yt_mixes` in live mode (see `open_mix_yt`).
        let mixes: Vec<CardData> = mix_specs
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let mut s = s.clone();
                if self.yt_live {
                    s.id = i as u32;
                }
                self.filled_card(&s, 176)
            })
            .collect();
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

        // Sidebar + Library ▸ Playlists (id, title, count, seed, art) then rows.
        let pl_info: Vec<(PlaylistId, String, u64, Option<String>)> = self
            .state
            .library
            .playlists
            .iter()
            .map(|p| {
                (
                    p.id,
                    count_songs(p.track_ids.len()),
                    AppState::playlist_seed(p.id),
                    p.artwork_url.clone(),
                )
            })
            .collect();
        let sidebar: Vec<PlaylistRowData> = pl_info
            .iter()
            .map(|&(id, ref sub, seed, ref art_url)| {
                let title = self.state.library.playlist(id).unwrap().title.clone();
                let art = match art_url {
                    Some(u) => self.art_url(u, 40),
                    None => self.art(seed, 40),
                };
                PlaylistRowData {
                    id: id as i32,
                    title: title.into(),
                    sub: sub.clone().into(),
                    art,
                }
            })
            .collect();
        self.sidebar_playlists.set_vec(sidebar);
        let lib_pl: Vec<PlaylistRowData> = pl_info
            .iter()
            .map(|&(id, _, seed, ref art_url)| {
                let p = self.state.library.playlist(id).unwrap();
                let title = p.title.clone();
                let counts = count_songs(p.track_ids.len());
                let art = match art_url {
                    Some(u) => self.art_url(u, 40),
                    None => self.art(seed, 40),
                };
                PlaylistRowData {
                    id: id as i32,
                    title: title.into(),
                    sub: format!("{} · playlist", counts).into(),
                    art,
                }
            })
            .collect();
        self.lib_playlists.set_vec(lib_pl);

        // Library ▸ Artists.
        let artist_info: Vec<(ArtistId, String, String, u64, Option<String>)> = (0
            ..self.state.library.artists.len() as u32)
            .map(|id| {
                let a = self.state.library.artist(id);
                (
                    id,
                    a.name.clone(),
                    listeners(a.monthly_listeners),
                    AppState::artist_seed(id),
                    a.artwork_url.clone(),
                )
            })
            .collect();
        let artists: Vec<ArtistRowData> = artist_info
            .iter()
            .map(|&(id, ref name, ref sub, seed, ref art_url)| {
                let art = match art_url {
                    Some(u) => self.art_url(u, 40),
                    None => self.art(seed, 40),
                };
                ArtistRowData {
                    id: id as i32,
                    name: name.clone().into(),
                    sub: sub.clone().into(),
                    art,
                }
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
            Collection { seed: u64, art_url: Option<String>, kind: String, title: String, sub: String, counts: String, round: bool },
            Artist { seed: u64, art_url: Option<String>, name: String, listeners: String, counts: String, top: Vec<TrackId>, album_specs: Vec<CardSpec> },
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
                    art_url: album.artwork_url.clone(),
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
                    art_url: p.artwork_url.clone(),
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
                    art_url: a.artwork_url.clone(),
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
            Hero::Collection { seed, art_url, kind, title, sub, counts, round } => {
                let art = match art_url {
                    Some(u) => Self::art_url_of(&self.state, &mut self.url_art, &u, 232),
                    None => Self::art_of(&self.state, &mut self.art, seed, 232),
                };
                self.ui.set_hero_art(art);
                self.ui.set_hero_round(round);
                self.ui.set_hero_kind(kind.into());
                self.ui.set_hero_title(title.into());
                self.ui.set_hero_sub(sub.into());
                self.ui.set_hero_counts(counts.into());
                self.ui.set_hero_is_artist(false);
            }
            Hero::Artist { seed, art_url, name, listeners, counts, top, album_specs } => {
                let art = match art_url {
                    Some(u) => Self::art_url_of(&self.state, &mut self.url_art, &u, 232),
                    None => Self::art_of(&self.state, &mut self.art, seed, 232),
                };
                self.ui.set_hero_art(art);
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
                let t = self.state.library.track(id);
                let spec = CardSpec {
                    id,
                    kind: 3,
                    title: v.title.to_string(),
                    subtitle: format!("Song · {}", v.artist),
                    round: false,
                    seed: AppState::album_seed(t.album_id),
                    art_url: t.artwork_url.clone(),
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
            let art = self.track_art(*id, 32);
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
                art,
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
                    let (title, artist_name, album_id, artist_id, liked, art_url) = (
                        track.title.clone(),
                        self.state.library.artist(track.artist_id).name.clone(),
                        track.album_id,
                        track.artist_id,
                        track.liked,
                        track.artwork_url.clone(),
                    );
                    self.ui.set_has_track(true);
                    self.ui.set_pb_title(title.into());
                    self.ui.set_pb_artist(artist_name.into());
                    let pb_art = match art_url {
                        Some(u) => Self::art_url_of(&self.state, &mut self.url_art, &u, 40),
                        None => Self::art_of(
                            &self.state,
                            &mut self.art,
                            AppState::album_seed(album_id),
                            40,
                        ),
                    };
                    self.ui.set_pb_art(pb_art);
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

    // ——— real-YouTube playback (stream when live, synth otherwise) ———

    fn video_of(&self, id: TrackId) -> String {
        if (id as usize) < self.state.library.tracks.len() {
            self.state.library.track(id).video_id.clone()
        } else {
            String::new()
        }
    }

    /// Synth single-track start that leaves the queue cursor alone
    /// (mirrors the private `AppState::start`).
    fn start_synth(&mut self, id: TrackId) {
        if (id as usize) >= self.state.library.tracks.len() {
            return;
        }
        let track = self.state.library.track(id).clone();
        self.state.player.play(&track, AppState::track_seed(id), 0.0);
        self.state.current = Some(id);
        self.state.playing = true;
    }

    /// User-initiated play of `ids[i]` — the queue becomes that list.
    fn play_ids(&mut self, ids: &[TrackId], i: usize) {
        if ids.is_empty() {
            return;
        }
        let i = i.min(ids.len() - 1);
        if self.is_bench() || !self.yt_live || self.video_of(ids[i]).is_empty() {
            self.state.play_from(ids, i);
            return;
        }
        self.state.queue.set_list(ids, i);
        self.yt_source = ids.to_vec();
        self.start_stream(ids[i]);
    }

    /// Start one track without touching the queue (advance / resume paths).
    fn start_stream(&mut self, id: TrackId) {
        if self.is_bench() || !self.yt_live || self.video_of(id).is_empty() {
            self.start_synth(id);
            return;
        }
        let vid = self.video_of(id);
        self.state.current = Some(id);
        self.state.playing = true;
        spawn_stream(self.yt_tx.clone(), self.cookie(), id, vid);
    }

    /// Queue advance with the streaming start (mirrors `AppState::next`).
    fn next_yt(&mut self) {
        if let Some(id) = self.state.queue.next() {
            self.start_stream(id);
        } else if self.state.repeat == RepeatMode::All && !self.yt_source.is_empty() {
            let src = self.yt_source.clone();
            self.state.queue.set_list(&src, 0);
            self.start_stream(src[0]);
        } else {
            self.state.playing = false;
        }
    }

    fn prev_yt(&mut self) {
        if self.state.player.position_sec() > 3.0 {
            if let Some(id) = self.state.queue.prev() {
                self.start_stream(id);
                return;
            }
        }
        if let Some(id) = self.state.current {
            self.start_stream(id);
        }
    }

    fn toggle_play_yt(&mut self) {
        match self.state.current {
            Some(_) if self.state.playing => {
                self.state.player.pause();
                self.state.playing = false;
            }
            Some(id) => {
                let pos = self.state.player.position_sec();
                if pos >= self.state.player.duration_sec() - 0.05 {
                    self.start_stream(id); // restart after finishing
                } else {
                    self.state.player.resume();
                    self.state.playing = true;
                }
            }
            None => {
                let ids = self.state.list_ids();
                if !ids.is_empty() {
                    self.play_ids(&ids, 0);
                }
            }
        }
    }

    fn seek_yt(&mut self, sec: f64) {
        if self.state.player.is_streaming() {
            let dur = self.state.player.duration_sec();
            self.state.player.seek_stream(sec.clamp(0.0, (dur - 0.2).max(0.0)));
            self.state.playing = true;
        } else {
            self.state.seek(sec);
        }
    }

    // ——— real-YouTube search (debounced, stale generations dropped) ———

    fn schedule_search(&mut self) {
        let q = self.state.search_query.trim().to_string();
        self.ui.set_search_query(q.clone().into());
        self.ui.set_search_has_query(!q.is_empty());
        if q.is_empty() {
            self.search_at = None;
            self.search_gen += 1; // invalidate anything in flight
            self.search_top.set_vec(Vec::new());
            self.search_songs.set_vec(Vec::new());
            self.search_albums.set_vec(Vec::new());
            self.search_artists.set_vec(Vec::new());
            self.page_ids = Vec::new();
            return;
        }
        self.search_q = q;
        self.search_at = Some(Instant::now());
    }

    fn poll_search(&mut self) {
        if self.is_bench() || !self.yt_live {
            return;
        }
        if let Some(at) = self.search_at {
            if at.elapsed() >= Duration::from_millis(300) {
                self.search_at = None;
                self.search_gen += 1;
                let gen = self.search_gen;
                let q = self.search_q.clone();
                spawn_search(self.yt_tx.clone(), self.cookie(), gen, q);
            }
        }
    }

    /// Search models straight from the results (relevance order), not via
    /// the local scorer — the library already holds exactly these tracks.
    fn refresh_search_yt(&mut self) {
        let n = self.state.library.tracks.len() as u32;
        let ids: Vec<TrackId> = (0..n).collect();
        let top: Vec<CardData> = ids
            .first()
            .map(|&id| {
                let v = self.state.library.track_view(id);
                let t = self.state.library.track(id);
                let spec = CardSpec {
                    id,
                    kind: 3,
                    title: v.title.to_string(),
                    subtitle: format!("Song · {}", v.artist),
                    round: false,
                    seed: AppState::album_seed(t.album_id),
                    art_url: t.artwork_url.clone(),
                };
                self.filled_card(&spec, 176)
            })
            .into_iter()
            .collect();
        self.search_top.set_vec(top);
        self.page_ids = ids.clone();
        let rows: Vec<TrackRow> = ids.iter().map(|&id| self.track_row(id)).collect();
        self.search_songs.set_vec(rows);
        let album_specs: Vec<CardSpec> = (0..self.state.library.albums.len() as u32)
            .map(|a| self.album_spec(a))
            .collect();
        let albums: Vec<CardData> =
            album_specs.iter().map(|s| self.filled_card(s, 176)).collect();
        self.search_albums.set_vec(albums);
        let artist_specs: Vec<CardSpec> = (0..self.state.library.artists.len() as u32)
            .map(|a| self.artist_spec(a))
            .collect();
        let artists: Vec<CardData> =
            artist_specs.iter().map(|s| self.filled_card(s, 176)).collect();
        self.search_artists.set_vec(artists);
    }

    // ——— real-YouTube navigation ———

    /// Raw YT track behind a library track (parallel vec; `None` in synth).
    fn yt_track(&self, id: TrackId) -> Option<YtTrack> {
        self.yt_tracks.get(id as usize).cloned()
    }

    fn album_browse_of(&self, album_id: AlbumId) -> Option<String> {
        let first = self.state.library.album(album_id).track_ids.first()?;
        self.yt_track(*first)?.album?.browse_id.into()
    }

    fn artist_channel_of(&self, artist_id: ArtistId) -> Option<String> {
        let name = self.state.library.artist(artist_id).name.clone();
        self.yt_tracks
            .iter()
            .find(|t| t.artist_names() == name)?
            .artists
            .first()
            .map(|a| a.channel_id.clone())
    }

    /// Local sub-library from an album's own tracks — the fallback when
    /// YouTube gave no browse id for the group.
    fn open_album_local(&mut self, album_id: AlbumId) {
        let (title, tids) = {
            let a = self.state.library.album(album_id);
            (a.title.clone(), a.track_ids.clone())
        };
        let tracks: Vec<YtTrack> =
            tids.iter().filter_map(|&t| self.yt_track(t)).collect();
        if tracks.is_empty() {
            self.state.open_album(album_id);
            self.refresh_page();
            return;
        }
        self.state.library = YtLibrary::from_tracks(&title, &tracks);
        self.yt_tracks = tracks;
        self.yt_pl_id = None;
        let id = (0..self.state.library.albums.len() as u32)
            .find(|&a| self.state.library.album(a).title == title)
            .unwrap_or(0);
        self.state.open_album(id);
        self.build_static_models();
        self.refresh_page();
    }

    fn open_album_yt(&mut self, album_id: AlbumId) {
        match self.album_browse_of(album_id) {
            Some(browse) => {
                self.set_yt_status("Loading album…".to_string());
                spawn_album(self.yt_tx.clone(), self.cookie(), browse);
            }
            None => self.open_album_local(album_id),
        }
    }

    fn open_artist_yt(&mut self, artist_id: ArtistId) {
        match self.artist_channel_of(artist_id) {
            Some(channel) => {
                self.set_yt_status("Loading artist…".to_string());
                spawn_artist(self.yt_tx.clone(), self.cookie(), channel);
            }
            None => {
                self.state.open_artist(artist_id);
                self.refresh_page();
            }
        }
    }

    fn open_mix_yt(&mut self, ix: usize) {
        let (id, title) = match (self.yt_mixes.get(ix), self.home_mixes.row_data(ix)) {
            (Some(m), Some(card)) => (m.id.clone(), card.title.to_string()),
            _ => return,
        };
        self.set_yt_status(format!("Loading playlist {title:?}…"));
        spawn_playlist(self.yt_tx.clone(), self.cookie(), id, title);
    }

    /// Home library snapshot to restore when leaving search/detail views.
    fn restore_home_lib(&mut self) {
        if let Some((home, mixes)) = self.yt_home.clone() {
            let tracks = home_tracks_of(&home);
            self.state.library = YtLibrary::from_home(&home);
            self.yt_tracks = tracks;
            self.yt_mixes = mixes;
            self.yt_pl_id = None;
            self.build_static_models();
        }
    }

    // ——— applying worker results (UI thread) ———

    fn apply_art(&mut self, art: &ArtBatch) {
        for (url, size, bytes) in art {
            self.state.artwork.insert_bytes(url, *size, bytes);
        }
    }

    fn apply_home_data(&mut self, home: YtHome, mixes: Vec<YtPlaylistHeader>, art: ArtBatch) {
        self.apply_art(&art);
        let tracks = home_tracks_of(&home);
        let lib = YtLibrary::from_home(&home);
        let n = lib.tracks.len();
        eprintln!("[yt] home live: {n} tracks");
        self.state.library = lib;
        self.yt_tracks = tracks;
        self.yt_mixes = mixes.clone();
        self.yt_pl_id = None;
        self.yt_home = Some((home, mixes));
        self.yt_live = true;
        self.yt_loading = false;
        self.set_yt_status(format!("YouTube live — {n} tracks"));
        self.ui.set_search_live(true);
        self.build_static_models();
        self.refresh_page();
        // A query typed while loading now has a network to hit.
        if matches!(self.state.view, View::Search) && !self.state.search_query.trim().is_empty() {
            self.schedule_search();
            // Run it now instead of waiting out the debounce.
            self.search_at = Some(Instant::now() - Duration::from_millis(300));
        }
    }

    fn apply_search_data(&mut self, gen: u64, results: YtSearchResults, art: ArtBatch) {
        if gen != self.search_gen {
            return; // stale
        }
        self.apply_art(&art);
        let query = results.query.clone();
        let lib = YtLibrary::from_search(&results);
        eprintln!("[yt] search live: {} tracks for {query:?}", lib.tracks.len());
        self.state.library = lib;
        self.yt_tracks = results.tracks.clone();
        self.yt_pl_id = None;
        self.refresh_search_yt();
        self.ui.set_search_query(query.into());
        self.ui.set_search_has_query(true);
    }

    fn apply_album_data(&mut self, mut album: YtAlbum, art: ArtBatch) {
        self.apply_art(&art);
        // The adapter groups by track-level refs (no cover); the raw album
        // thumbs are the better hero.
        let hero = zuno_yt::pick_artwork(&album.thumbnails).map(str::to_string);
        let mut lib = YtLibrary::from_album(&album);
        if let Some(u) = hero {
            for a in &mut lib.albums {
                if a.artwork_url.is_none() {
                    a.artwork_url = Some(u.clone());
                }
            }
        }
        let title = album.title.clone();
        let n = lib.tracks.len();
        self.state.library = lib;
        self.yt_tracks = std::mem::take(&mut album.tracks);
        self.yt_pl_id = None;
        let id = (0..self.state.library.albums.len() as u32)
            .find(|&a| self.state.library.album(a).title == title)
            .unwrap_or(0);
        self.state.open_album(id);
        self.set_yt_status(format!("YouTube live — {title} ({n} tracks)"));
        self.build_static_models();
        self.refresh_page();
    }

    fn apply_artist_data(&mut self, mut artist: YtArtist, art: ArtBatch) {
        self.apply_art(&art);
        let hero = zuno_yt::pick_artwork(&artist.thumbnails).map(str::to_string);
        let mut lib = YtLibrary::from_tracks(&artist.name, &artist.top_tracks);
        if let Some(u) = hero {
            for a in &mut lib.artists {
                if a.artwork_url.is_none() {
                    a.artwork_url = Some(u.clone());
                }
            }
        }
        let name = artist.name.clone();
        self.state.library = lib;
        self.yt_tracks = std::mem::take(&mut artist.top_tracks);
        self.yt_pl_id = None;
        // Note: the albums grid shows the top tracks' groups, not the
        // artist's full discography refs (no second browse needed to play).
        let id = (0..self.state.library.artists.len() as u32)
            .find(|&a| self.state.library.artist(a).name == name)
            .unwrap_or(0);
        self.state.open_artist(id);
        self.set_yt_status(format!("YouTube live — {name}"));
        self.build_static_models();
        self.refresh_page();
    }

    fn apply_playlist_data(&mut self, id: String, title: String, tracks: Vec<YtTrack>, art: ArtBatch) {
        self.apply_art(&art);
        let pl = YtPlaylist {
            id: id.clone(),
            title: title.clone(),
            description: None,
            thumbnails: Vec::new(),
            tracks: tracks.clone(),
        };
        let mut lib = YtLibrary::from_playlist(&pl);
        // Cover from the chart header when opened via the mixes shelf
        // (the track-built library carries no playlist art of its own).
        let cover = self
            .yt_mixes
            .iter()
            .find(|m| m.id == id)
            .and_then(|m| zuno_yt::pick_artwork(&m.thumbnails))
            .map(str::to_string);
        if let Some(u) = cover {
            for p in &mut lib.playlists {
                if p.artwork_url.is_none() {
                    p.artwork_url = Some(u.clone());
                }
            }
        }
        let n = lib.tracks.len();
        self.state.library = lib;
        self.yt_tracks = tracks;
        self.yt_pl_id = Some(id);
        self.state.open_playlist(0);
        self.set_yt_status(format!("YouTube live — {title} ({n} tracks)"));
        self.build_static_models();
        self.refresh_page();
    }

    fn apply_stream(&mut self, id: TrackId, url: String, mime: String) {
        // Stale resolve (user moved on) — never hijack the new track.
        if self.state.current != Some(id) {
            return;
        }
        let cookie = self.cookie();
        self.state.player.play_stream(&url, &mime, cookie.as_deref());
        self.state.playing = true;
    }

    fn apply_stream_failed(&mut self, id: TrackId) {
        if self.state.current != Some(id) {
            return;
        }
        // Audible beats silent: synth fallback keeps the queue moving.
        eprintln!("[yt] stream failed, synth fallback for track {id}");
        self.start_synth(id);
    }

    fn apply_yt(&mut self, msg: YtMsg) {
        if self.is_bench() {
            return;
        }
        match msg {
            YtMsg::Home { home, mixes, art } => self.apply_home_data(home, mixes, art),
            YtMsg::HomeFailed(e) => {
                self.yt_loading = false;
                eprintln!("[yt] home failed: {e} (synthetic library kept)");
                self.set_yt_status(format!("Offline ({e}) — built-in library"));
            }
            YtMsg::Search { gen, results, art } => self.apply_search_data(gen, results, art),
            YtMsg::SearchFailed { gen, err } => {
                if gen == self.search_gen {
                    eprintln!("[yt] search failed: {err}");
                    self.set_yt_status(format!("Search failed: {err}"));
                }
            }
            YtMsg::DetailAlbum { album, art } => self.apply_album_data(album, art),
            YtMsg::DetailArtist { artist, art } => self.apply_artist_data(artist, art),
            YtMsg::DetailPlaylist { id, title, tracks, art } => {
                self.apply_playlist_data(id, title, tracks, art)
            }
            YtMsg::DetailFailed(e) => {
                eprintln!("[yt] detail failed: {e}");
                self.set_yt_status(format!("Load failed: {e}"));
            }
            YtMsg::Stream { id, url, mime } => self.apply_stream(id, url, mime),
            YtMsg::StreamFailed { id } => self.apply_stream_failed(id),
            YtMsg::SignedIn { session, home, mixes, art } => {
                if let Err(e) = session.save() {
                    eprintln!("[yt] session persist failed: {e}");
                }
                self.session = Some(session);
                self.apply_home_data(home, mixes, art);
                let who = self
                    .session
                    .as_ref()
                    .and_then(|s| s.account_identity())
                    .map(|id| format!(" ({}…)", &id[..id.len().min(6)]))
                    .unwrap_or_default();
                let n = self.state.library.tracks.len();
                self.set_yt_status(format!("Signed in{who} — {n} tracks"));
            }
            YtMsg::SignInFailed(e) => {
                eprintln!("[yt] sign-in failed: {e}");
                self.set_yt_status(format!("Sign-in failed: {e}"));
            }
        }
    }

    /// Fire-and-forget like → rating (local state stays the source of
    /// truth; failures only log).
    fn rate_yt(&mut self, id: TrackId, liked: bool) {
        if self.is_bench() || !self.yt_live || self.session.is_none() {
            return;
        }
        let vid = self.video_of(id);
        if vid.is_empty() {
            return;
        }
        let cookie = self.cookie();
        let rating = if liked { Rating::Like } else { Rating::None };
        thread::spawn(move || {
            let r = yt_client(cookie.as_deref()).and_then(|mut yt| {
                yt_rt()
                    .block_on(yt.set_track_rating(&vid, rating))
                    .map_err(|e| e.to_string())
            });
            if let Err(e) = r {
                eprintln!("[yt] rating failed: {e}");
            }
        });
    }
}

fn wire(ui: &ZunoWindow, zuno: &Rc<RefCell<Zuno>>) {
    let weak = zuno.clone();
    ui.on_nav_home(move || {
        let mut z = weak.borrow_mut();
        if z.yt_live && !z.is_bench() {
            z.restore_home_lib();
        }
        z.state.go(View::Home);
        z.refresh_page();
    });
    let weak = zuno.clone();
    ui.on_nav_search(move || {
        let mut z = weak.borrow_mut();
        z.state.go(View::Search);
        z.refresh_page();
        // Re-entering with a query re-runs it live.
        if z.yt_live && !z.is_bench() && !z.state.search_query.trim().is_empty() {
            z.schedule_search();
            z.search_at = Some(Instant::now() - Duration::from_millis(300));
        }
    });
    let weak = zuno.clone();
    ui.on_nav_library(move || {
        let mut z = weak.borrow_mut();
        if z.yt_live && !z.is_bench() {
            z.restore_home_lib();
        }
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
        let aid = id.max(0) as u32;
        if z.yt_live && !z.is_bench() {
            z.open_album_yt(aid);
        } else {
            z.state.open_album(aid);
            z.refresh_page();
        }
    });
    let weak = zuno.clone();
    ui.on_open_artist(move |id| {
        let mut z = weak.borrow_mut();
        let arid = id.max(0) as u32;
        if z.yt_live && !z.is_bench() {
            z.open_artist_yt(arid);
        } else {
            z.state.open_artist(arid);
            z.refresh_page();
        }
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
        // Live mixes-shelf cards: id indexes the chart-playlist headers.
        if z.yt_live && !z.is_bench() && kind == 2 {
            z.open_mix_yt(id.max(0) as usize);
            return;
        }
        match kind {
            0 if z.yt_live && !z.is_bench() => z.open_album_yt(id.max(0) as u32),
            1 if z.yt_live && !z.is_bench() => z.open_artist_yt(id.max(0) as u32),
            0 => z.state.open_album(id.max(0) as u32),
            1 => z.state.open_artist(id.max(0) as u32),
            2 => z.state.open_playlist(id.max(0) as u32),
            _ => {
                let tid = id.max(0) as u32;
                if z.yt_live && !z.is_bench() {
                    z.play_ids(&[tid], 0);
                } else {
                    z.state.play_track(tid);
                }
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
        if z.yt_live && !z.is_bench() {
            z.schedule_search();
        } else {
            z.refresh_search();
        }
        z.refresh_page();
    });
    let weak = zuno.clone();
    ui.on_play_from(move |ix| {
        let mut z = weak.borrow_mut();
        let ids = z.page_ids.clone();
        if !ids.is_empty() {
            let i = (ix.max(0) as usize).min(ids.len() - 1);
            if z.yt_live && !z.is_bench() {
                z.play_ids(&ids, i);
            } else {
                z.state.play_from(&ids, i);
            }
        }
    });
    let weak = zuno.clone();
    ui.on_shuffle_play(move || {
        let mut z = weak.borrow_mut();
        let ids = z.page_ids.clone();
        if !ids.is_empty() {
            if z.yt_live && !z.is_bench() {
                z.play_ids(&ids, 0);
            } else {
                z.state.play_from(&ids, 0);
            }
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
            if z.yt_live && !z.is_bench() {
                z.play_ids(&ids, 0);
            } else {
                z.state.play_from(&ids, 0);
            }
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
        if z.yt_live && !z.is_bench() {
            let liked = (tid as usize) < z.state.library.tracks.len()
                && z.state.library.track(tid).liked;
            z.rate_yt(tid, liked);
        }
    });
    let weak = zuno.clone();
    ui.on_toggle_play(move || {
        let mut z = weak.borrow_mut();
        if z.yt_live && !z.is_bench() {
            z.toggle_play_yt();
        } else {
            z.state.toggle_play();
        }
    });
    let weak = zuno.clone();
    ui.on_next_track(move || {
        let mut z = weak.borrow_mut();
        if z.yt_live && !z.is_bench() {
            z.next_yt();
        } else {
            z.state.next();
        }
    });
    let weak = zuno.clone();
    ui.on_prev_track(move || {
        let mut z = weak.borrow_mut();
        if z.yt_live && !z.is_bench() {
            z.prev_yt();
        } else {
            z.state.previous();
        }
    });
    let weak = zuno.clone();
    ui.on_seek_to(move |f| {
        let mut z = weak.borrow_mut();
        if z.yt_live && !z.is_bench() {
            z.seek_yt(f.max(0.0) as f64);
        } else {
            z.state.seek(f.max(0.0) as f64);
        }
    });
    let weak = zuno.clone();
    ui.on_seek_by(move |d| {
        let mut z = weak.borrow_mut();
        let pos = z.state.player.position_sec();
        if z.yt_live && !z.is_bench() {
            z.seek_yt(pos + d as f64);
        } else {
            z.state.seek(pos + d as f64);
        }
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
            if z.yt_live && !z.is_bench() {
                z.start_stream(id);
            } else {
                z.state.play_track(id);
            }
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
    let weak = zuno.clone();
    ui.on_sign_in(move |paste| {
        let mut z = weak.borrow_mut();
        if z.is_bench() {
            return;
        }
        z.set_yt_status("Signing in…".to_string());
        spawn_sign_in(z.yt_tx.clone(), paste.to_string());
    });
}

fn make_art_cb(zuno: &Rc<RefCell<Zuno>>, slot: GridSlot) -> impl Fn(i32, i32) {
    let zuno = zuno.clone();
    move |row: i32, col: i32| {
        let mut z = zuno.borrow_mut();
        let row = row.max(0) as usize;
        let col = col.max(0) as usize;
        // Find the art source (and the row model) without holding a &mut borrow.
        let (seed, art_url) = {
            let grid = z.grid(slot);
            let cols = grid.cols;
            let Some(rm) = grid.row_models.get(row).cloned() else { return };
            let Some(card) = rm.row_data(col) else { return };
            if !card.needs_art {
                return;
            }
            let Some(spec) = grid.specs.get(row * cols + col) else { return };
            (spec.seed, spec.art_url.clone())
        };
        let art = match art_url {
            Some(u) => z.art_url(&u, 176),
            None => z.art(seed, 176),
        };
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
    // Background YouTube results (nothing ever spawns in `--bench`, so
    // this only drains an empty channel there).
    while let Ok(msg) = z.yt_rx.try_recv() {
        z.apply_yt(msg);
    }
    z.poll_search();

    // Track-end advance: streaming-aware when live (mirrors AppState::tick's
    // repeat policy, but restarts streams instead of the synth).
    let yt_ended = z.yt_live && !z.is_bench() && z.state.player.take_ended();
    if yt_ended {
        if z.state.repeat == RepeatMode::One {
            if let Some(id) = z.state.current {
                z.start_stream(id);
            }
        } else if let Some(id) = z.state.queue.next() {
            z.start_stream(id);
        } else if z.state.repeat == RepeatMode::All && !z.yt_source.is_empty() {
            let src = z.yt_source.clone();
            z.state.queue.set_list(&src, 0);
            z.start_stream(src[0]);
        } else {
            z.state.playing = false;
        }
    } else {
        z.state.tick();
    }

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
