//! Real-YouTube background worker for the vizia UI.
//!
//! One std::thread owns a `YtClient` and a single-threaded tokio runtime
//! (vizia has no async executor; this keeps all network + QuickJS decipher
//! off the UI thread). The UI sends [`YtReq`], the worker replies [`YtResp`];
//! the UI drains the channel on its 60 Hz tick. Library assembly
//! (`YtLibrary::from_*`) happens here so the tick handler just swaps.

use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{Receiver, Sender};
use zuno_core::library::Library;
use zuno_yt::{
    Rating, Session, StreamInfo, YtClient, YtClientExt, YtLibrary, YtMutations,
};

/// Title → browse/channel id maps. Core's `Library` drops YouTube ids, so the
/// worker ships the lookup alongside every library it builds. First title
/// wins on collision.
#[derive(Clone, Default, Debug)]
pub struct IdMaps {
    pub albums: HashMap<String, String>,
    pub artists: HashMap<String, String>,
    pub playlists: HashMap<String, String>,
}

#[derive(Debug)]
pub enum YtReq {
    Home,
    Search { rev: u64, query: String },
    Album { browse_id: String },
    Artist { channel_id: String },
    Playlist { id: String, title: String },
    Stream { video_id: String },
    SignIn { header: String },
    Like { video_id: String, liked: bool },
}

/// Covers are `(url, raw bytes)`; the UI thread decodes them into its own
/// `ArtworkCache` via `insert_bytes` (a few per tick to avoid hitches).
pub type Covers = Vec<(String, Vec<u8>)>;

/// `Library`/`StreamInfo` don't all implement `Debug`, so no derive here.
pub enum YtResp {
    /// Sent once at worker startup: whether a saved session was restored.
    Session(SignState),
    Home(Result<(Library, IdMaps, SignState, Covers), String>),
    Search {
        rev: u64,
        query: String,
        result: Result<(Library, IdMaps, Covers), String>,
    },
    Album(Result<(Library, IdMaps, Covers), String>),
    Artist(Result<(Library, IdMaps, Covers), String>),
    Playlist(Result<(Library, IdMaps, Covers), String>),
    Stream(Result<(String, StreamInfo, Option<String>), String>),
    SignedIn(Result<(Library, IdMaps, SignState, Covers), String>),
    /// Late-arriving prefetched covers for the CURRENT library. Sent after
    /// the main response so data/stream requests never queue behind artwork.
    Covers(Covers),
}

#[derive(Clone, Debug)]
pub struct SignState {
    pub signed_in: bool,
    pub identity: Option<String>,
}

pub struct YtHandle {
    pub tx: Sender<YtReq>,
    pub rx: Receiver<YtResp>,
}

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

/// Unique artwork urls in first-seen order, capped.
fn collect_urls<'a>(
    tracks: impl Iterator<Item = &'a zuno_yt::YtTrack>,
    extra: impl Iterator<Item = String>,
    cap: usize,
) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for t in tracks {
        if let Some(u) = t.artwork_url() {
            if seen.insert(u.to_string()) {
                out.push(u.to_string());
            }
        }
    }
    for u in extra {
        if out.len() >= cap {
            break;
        }
        if seen.insert(u.clone()) {
            out.push(u);
        }
    }
    out.truncate(cap);
    out
}

fn prefetch(urls: Vec<String>) -> Covers {
    // Bounded-parallel fetch: 8 threads at a time. Serial prefetch on a
    // slow link blocks the worker's single-threaded runtime for a minute+.
    let mut out = Vec::new();
    for chunk in urls.chunks(8) {
        std::thread::scope(|s| {
            let handles: Vec<_> = chunk
                .iter()
                .map(|u| {
                    let u = u.clone();
                    s.spawn(move || zuno_yt::fetch_artwork(&u).ok().map(|b| (u, b)))
                })
                .collect();
            for h in handles {
                if let Ok(Some(c)) = h.join() {
                    out.push(c);
                }
            }
        });
    }
    out
}

fn maps_for_tracks(tracks: &[zuno_yt::YtTrack]) -> IdMaps {
    let mut m = IdMaps::default();
    for t in tracks {
        if let Some(a) = &t.album {
            m.albums.entry(a.name.clone()).or_insert_with(|| a.browse_id.clone());
        }
        for ar in &t.artists {
            m.artists.entry(ar.name.clone()).or_insert_with(|| ar.channel_id.clone());
        }
        if let Some(pid) = &t.playlist_id {
            m.playlists.entry(t.title.clone()).or_insert_with(|| pid.clone());
        }
    }
    m
}

struct Worker {
    client: YtClient,
    sign: SignState,
    tx: Sender<YtResp>,
}

/// Assemble a home-shaped response (library + id maps + cover URLS) from a
/// `YtHome`. Shared by the unsigned `Home` and signed-in `SignIn` paths
/// (`get_library` returns the same shape). URLs come back unfetched — the
/// caller sends the main response FIRST and prefetches after, so artwork
/// never blocks data on the worker thread.
fn assemble_home(
    home: &zuno_yt::YtHome,
    sign: SignState,
) -> Result<(Library, IdMaps, SignState, Vec<String>), String> {
    let lib = YtLibrary::from_home(home);
    if lib.tracks.is_empty() {
        return Err("home came back with no tracks".into());
    }
    let tracks: Vec<zuno_yt::YtTrack> =
        home.shelves.iter().flat_map(|s| s.tracks.clone()).collect();
    let mut maps = maps_for_tracks(&tracks);
    let mut extra = Vec::new();
    for s in &home.shelves {
        for a in &s.albums {
            maps.albums.entry(a.title.clone()).or_insert_with(|| a.browse_id.clone());
            if let Some(u) = zuno_yt::pick_artwork(&a.thumbnails) {
                extra.push(u.to_string());
            }
        }
        for a in &s.artists {
            maps.artists.entry(a.name.clone()).or_insert_with(|| a.channel_id.clone());
        }
        for p in &s.playlists {
            maps.playlists.entry(p.title.clone()).or_insert_with(|| p.id.clone());
        }
    }
    let urls = collect_urls(tracks.iter(), extra.into_iter(), 64);
    Ok((lib, maps, sign, urls))
}

impl Worker {
    fn cookie(&self) -> Option<String> {
        self.client.session().map(|s| s.cookie_header.clone())
    }

    fn send(&self, r: YtResp) {
        let _ = self.tx.send(r);
    }

    /// Fetch cover bytes AFTER the main response went out, then ship them
    /// as a trailing `Covers` message.
    fn send_covers(&self, urls: Vec<String>) {
        if urls.is_empty() {
            return;
        }
        let covers = prefetch(urls);
        if !covers.is_empty() {
            self.send(YtResp::Covers(covers));
        }
    }

    async fn handle(&mut self, req: YtReq) {
        match req {
            YtReq::Home => {
                let sign = self.sign.clone();
                let r = self
                    .client
                    .get_home()
                    .await
                    .map_err(err)
                    .and_then(|home| assemble_home(&home, sign));
                match r {
                    Ok((lib, maps, sign, urls)) => {
                        self.send(YtResp::Home(Ok((lib, maps, sign, Vec::new()))));
                        self.send_covers(urls);
                    }
                    Err(e) => self.send(YtResp::Home(Err(e))),
                }
            }
            YtReq::Search { rev, query } => {
                let r = self
                    .client
                    .search(&query)
                    .await
                    .map_err(err)
                    .and_then(|res| {
                        let lib = YtLibrary::from_search(&res);
                        if lib.tracks.is_empty() {
                            return Err(format!("no results for “{query}”"));
                        }
                        let mut maps = maps_for_tracks(&res.tracks);
                        let mut extra = Vec::new();
                        for a in &res.albums {
                            maps.albums
                                .entry(a.title.clone())
                                .or_insert_with(|| a.browse_id.clone());
                            if let Some(u) = zuno_yt::pick_artwork(&a.thumbnails) {
                                extra.push(u.to_string());
                            }
                        }
                        for a in &res.artists {
                            maps.artists
                                .entry(a.name.clone())
                                .or_insert_with(|| a.channel_id.clone());
                        }
                        for p in &res.playlists {
                            maps.playlists
                                .entry(p.title.clone())
                                .or_insert_with(|| p.id.clone());
                        }
                        let urls = collect_urls(res.tracks.iter(), extra.into_iter(), 32);
                        Ok((lib, maps, urls))
                    });
                match r {
                    Ok((lib, maps, urls)) => {
                        self.send(YtResp::Search { rev, query, result: Ok((lib, maps, Vec::new())) });
                        self.send_covers(urls);
                    }
                    Err(e) => self.send(YtResp::Search { rev, query, result: Err(e) }),
                }
            }
            YtReq::Album { browse_id } => {
                let r = self
                    .client
                    .get_album(&browse_id)
                    .await
                    .map_err(err)
                    .map(|album| {
                        let lib = YtLibrary::from_album(&album);
                        let maps = maps_for_tracks(&album.tracks);
                        let urls = collect_urls(album.tracks.iter(), std::iter::empty(), 24);
                        (lib, maps, urls)
                    });
                match r {
                    Ok((lib, maps, urls)) => {
                        self.send(YtResp::Album(Ok((lib, maps, Vec::new()))));
                        self.send_covers(urls);
                    }
                    Err(e) => self.send(YtResp::Album(Err(e))),
                }
            }
            YtReq::Artist { channel_id } => {
                let r = self
                    .client
                    .get_artist(&channel_id)
                    .await
                    .map_err(err)
                    .map(|artist| {
                        let lib = YtLibrary::from_tracks(&artist.name, &artist.top_tracks);
                        let maps = maps_for_tracks(&artist.top_tracks);
                        let urls = collect_urls(artist.top_tracks.iter(), std::iter::empty(), 24);
                        (lib, maps, urls)
                    });
                match r {
                    Ok((lib, maps, urls)) => {
                        self.send(YtResp::Artist(Ok((lib, maps, Vec::new()))));
                        self.send_covers(urls);
                    }
                    Err(e) => self.send(YtResp::Artist(Err(e))),
                }
            }
            YtReq::Playlist { id, title } => {
                let meta = self.client.get_playlist(&id).await.map_err(err);
                let tracks = self.client.get_playlist_tracks(&id).await.map_err(err);
                let r = match (meta, tracks) {
                    (Ok(mut pl), Ok(tracks)) => {
                        pl.tracks = tracks;
                        let lib = YtLibrary::from_playlist(&pl);
                        let maps = maps_for_tracks(&pl.tracks);
                        let urls = collect_urls(pl.tracks.iter(), std::iter::empty(), 24);
                        Ok((lib, maps, urls))
                    }
                    (Err(e), _) => Err(e),
                    (_, Err(e)) => Err(format!("{title}: {e}")),
                };
                match r {
                    Ok((lib, maps, urls)) => {
                        self.send(YtResp::Playlist(Ok((lib, maps, Vec::new()))));
                        self.send_covers(urls);
                    }
                    Err(e) => self.send(YtResp::Playlist(Err(e))),
                }
            }
            YtReq::Stream { video_id } => {
                let cookie = self.cookie();
                let r = zuno_yt::resolve_stream_url(&mut self.client, &video_id)
                    .await
                    .map_err(err)
                    .map(|info| (video_id.clone(), info, cookie));
                self.send(YtResp::Stream(r));
            }
            YtReq::SignIn { header } => {
                let session = Session::import(&header);
                if session.is_empty() {
                    self.send(YtResp::SignedIn(Err("no usable cookies in that paste".into())));
                    return;
                }
                let _ = session.save();
                match YtClient::with_session(session) {
                    Ok(c) => {
                        self.client = c;
                        self.sign = SignState {
                            signed_in: true,
                            identity: self.client.session().and_then(|s| s.account_identity()),
                        };
                        let sign = self.sign.clone();
                        let r = self
                            .client
                            .get_library()
                            .await
                            .map_err(err)
                            .and_then(|home| assemble_home(&home, sign));
                        match r {
                            Ok((lib, maps, sign, urls)) => {
                                self.send(YtResp::SignedIn(Ok((lib, maps, sign, Vec::new()))));
                                self.send_covers(urls);
                            }
                            Err(e) => self.send(YtResp::SignedIn(Err(e))),
                        }
                    }
                    Err(e) => self.send(YtResp::SignedIn(Err(err(e)))),
                }
            }
            YtReq::Like { video_id, liked } => {
                if self.sign.signed_in {
                    let rating = if liked { Rating::Like } else { Rating::None };
                    let _ = self.client.set_track_rating(&video_id, rating).await;
                }
            }
        }
    }
}

/// Spawn the worker. Tries a saved session first (signed-in client from the
/// start when the user signed in before); otherwise unsigned.
pub fn spawn() -> YtHandle {
    let (tx_req, rx_req) = std::sync::mpsc::channel::<YtReq>();
    let (tx_resp, rx_resp) = std::sync::mpsc::channel::<YtResp>();
    std::thread::Builder::new()
        .name("zuno-yt".into())
        .spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("yt runtime builds");
            let mut worker = match Session::load() {
                Some(s) if !s.is_empty() => match YtClient::with_session(s) {
                    Ok(c) => {
                        let identity = c.session().and_then(|x| x.account_identity());
                        Worker {
                            client: c,
                            sign: SignState { signed_in: true, identity },
                            tx: tx_resp.clone(),
                        }
                    }
                    Err(_) => Worker {
                        client: YtClient::unsigned().expect("unsigned client builds"),
                        sign: SignState { signed_in: false, identity: None },
                        tx: tx_resp.clone(),
                    },
                },
                _ => Worker {
                    client: YtClient::unsigned().expect("unsigned client builds"),
                    sign: SignState { signed_in: false, identity: None },
                    tx: tx_resp.clone(),
                },
            };
            // Tell the UI about a restored session before any request runs.
            let _ = tx_resp.send(YtResp::Session(worker.sign.clone()));
            for req in rx_req {
                rt.block_on(worker.handle(req));
                if worker.tx.send(YtResp::Covers(Vec::new())).is_err() {
                    break; // UI gone; the empty Covers is just a liveness ping
                }
            }
        })
        .expect("yt worker thread spawns");
    YtHandle { tx: tx_req, rx: rx_resp }
}

/// FNV-1a64 — stable procedural-fallback tile per artwork URL, and the
/// image-store name for a remote cover.
pub fn url_seed(url: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in url.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

pub fn url_art_name(url: &str, size: u32) -> String {
    format!("ytu-{:x}-{size}", url_seed(url))
}
