//! Real-YouTube bridge for the freya UI — freya-native async, no worker
//! thread, no channels.
//!
//! Freya tasks are futures polled on the UI thread with no `Send` bound
//! (`spawn`/`spawn_forever`), plus two runtime helpers this module leans on:
//! `timer()` (async-io) for the search debounce and `thread()` (the `blocking`
//! crate pool) so `fetch_artwork` never freezes a frame. The one thing the
//! Innertube POSTs still need is a live tokio reactor (async reqwest), so
//! `main` builds a 2-worker multi-thread runtime and enters it for the whole
//! run — driver threads do the socket wakeups, freya resumes the awaiting
//! task on the UI thread. All of this is created only when `yt::init()` ran,
//! which `main` skips entirely under `--bench`.
//!
//! The client itself sits in a `tokio::sync::Mutex` (async-aware: a second
//! request suspends instead of deadlocking) — operations serialize exactly
//! like the worker-thread branches (floem/vizia), just on freya's executor.

use freya::prelude::*;
use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use zuno_core::library::Library;
use zuno_core::{AppState, TrackId, View};
use zuno_yt::{Session, YtClient, YtClientExt, YtLibrary};

use crate::Ui;

static YT: OnceLock<tokio::sync::Mutex<Option<YtClient>>> = OnceLock::new();
static HOME_LIB: OnceLock<Mutex<Option<Library>>> = OnceLock::new();
static COVERS_READY: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
static COVERS_FLYING: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();

/// Every pixel size a cover renders at in this UI (rows, queue, cards, top
/// result, detail headers). A fetched cover is pre-decoded at all of them so
/// the draw path never blocks on network.
pub const COVER_SIZES: [u32; 5] = [32, 40, 96, 160, 232];

/// ponytail: 24-url prefetch cap × 5 sizes ≈ core's 96-cover LRU. Beyond it,
/// covers stay procedural — cheaper than guaranteed LRU churn re-fetching
/// (blocking) from the render path.
const COVER_PREFETCH_CAP: usize = 24;

pub fn enabled() -> bool {
    YT.get().is_some()
}

/// Build the client (and restore a saved session). Called from `main` only
/// when not benching; on failure the app stays fully synthetic.
pub fn init() -> bool {
    let Ok(mut client) = YtClient::unsigned() else {
        eprintln!("[yt] client init failed — staying synthetic");
        return false;
    };
    if let Some(sess) = Session::load() {
        if !sess.is_empty() {
            if let Some(who) = sess.account_identity() {
                eprintln!("[yt] restored saved session: {who}");
            }
            client.set_session(Some(sess));
        }
    }
    // All the OnceLocks the bridge reads later — set here so every
    // `.get().unwrap()` downstream is safe (they hold empty defaults until
    // real data flows in).
    let _ = HOME_LIB.set(Mutex::new(None));
    let _ = COVERS_READY.set(Mutex::new(HashSet::new()));
    let _ = COVERS_FLYING.set(Mutex::new(HashSet::new()));
    YT.set(tokio::sync::Mutex::new(Some(client))).is_ok()
}

/// Cache key for a cover URL — keeps the (u64, u32) ImageHandle map flat.
pub fn url_hash(u: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in u.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    h
}

pub fn cover_ready(url: &str) -> bool {
    COVERS_READY
        .get()
        .map(|m| m.lock().unwrap().contains(url))
        .unwrap_or(false)
}

/// Per-frame YT work, called from the frame loop (UI thread): debounced
/// search fan-out, empty-query home restore, and stream resolve for the
/// current track (synth paused meanwhile — the resolve task either swaps in
/// `play_stream` or resumes the synth as fallback).
pub fn frame_tick(app: &State<AppState>, ui: &State<Ui>, search: &State<String>) {
    if !enabled() {
        return;
    }
    let q = search.peek().clone();
    if app.peek().view == View::Search {
        if q.trim().len() >= 2 && q != ui.peek().search_sent {
            ui.write_unchecked().search_sent = q.clone();
            ui.write_unchecked().search_rev += 1;
            let rev = ui.peek().search_rev;
            spawn(run_search(app.clone(), ui.clone(), rev, q));
        } else if q.trim().is_empty() && ui.peek().lib_is_search {
            restore_home(app, ui);
        }
    }
    let cur = app.peek().current;
    if let Some(id) = cur {
        let vid = app.peek().library.track(id).video_id.clone();
        let needs = !vid.is_empty()
            && !app.peek().player.is_streaming()
            && ui.peek().resolving != Some(id)
            && ui.peek().stream_for != Some(id);
        if needs {
            {
                let mut a = app.write_unchecked();
                a.player.pause();
                a.playing = false;
            }
            ui.write_unchecked().resolving = Some(id);
            ui.write_unchecked().yt_status = "Resolving stream…".to_string();
            spawn(resolve_stream(app.clone(), ui.clone(), id, vid));
        }
    }
}

/// Seek honoring the active engine: streams re-open the buffered bytes,
/// synth recreates the source.
pub fn seek_to(app: &State<AppState>, sec: f64) {
    let streaming = {
        let a = app.peek();
        a.current.is_some_and(|id| !a.library.track(id).video_id.is_empty())
            && a.player.is_streaming()
    };
    if streaming {
        app.peek().player.seek_stream(sec.max(0.0));
    } else {
        app.write_unchecked().seek(sec);
    }
}

/// Initial load: home shelves, plus one seed search when unsigned home is
/// thin on tracks (charts feeds are mostly playlists/artists).
pub async fn load_home(app: State<AppState>, ui: State<Ui>) {
    ui.write_unchecked().yt_status = "Loading YouTube Music…".to_string();
    let lib = {
        let mut g = YT.get().unwrap().lock().await;
        let Some(c) = g.as_mut() else { return };
        let mut tracks = Vec::new();
        match c.get_home().await {
            Ok(home) => {
                eprintln!("[yt] home: {} shelves", home.shelves.len());
                for s in &home.shelves {
                    tracks.extend(s.tracks.iter().cloned());
                }
            }
            Err(e) => {
                eprintln!("[yt] get_home failed: {e}");
            }
        }
        if tracks.len() < 20 {
            match c.search("top hits").await {
                Ok(res) => tracks.extend(res.tracks.iter().cloned()),
                Err(e) => eprintln!("[yt] home seed search failed: {e}"),
            }
        }
        YtLibrary::from_tracks("Home", &tracks)
    };
    eprintln!("[yt] home ready: {} tracks", lib.tracks.len());
    ui.write_unchecked().yt_status = "Live · YouTube Music".to_string();
    HOME_LIB.get().unwrap().lock().unwrap().replace(lib.clone());
    swap_library(&app, &ui, lib, false);
}

async fn run_search(app: State<AppState>, ui: State<Ui>, rev: u64, query: String) {
    timer(Duration::from_millis(350)).await;
    if ui.peek().search_rev != rev {
        return; // a newer keystroke superseded this one
    }
    let res = {
        let mut g = YT.get().unwrap().lock().await;
        match g.as_mut() {
            Some(c) => c.search(&query).await,
            None => return,
        }
    };
    if ui.peek().search_rev != rev {
        return; // stale arrival
    }
    match res {
        Ok(r) => {
            let lib = YtLibrary::from_search(&r);
            eprintln!("[yt] search ready: {} tracks", lib.tracks.len());
            swap_library(&app, &ui, lib, true);
        }
        Err(e) => {
            eprintln!("[yt] search failed: {e}");
            ui.write_unchecked().yt_status = format!("Search failed: {e}");
        }
    }
}

async fn resolve_stream(app: State<AppState>, ui: State<Ui>, track: TrackId, video_id: String) {
    let res = {
        let mut g = YT.get().unwrap().lock().await;
        let Some(c) = g.as_mut() else { return };
        let cookie = c.session().map(|s| s.cookie_header.clone());
        zuno_yt::resolve_stream_url(c, &video_id).await.map(|i| (i, cookie))
    };
    if ui.peek().resolving != Some(track) {
        return; // superseded by a newer resolve
    }
    if app.peek().current != Some(track) {
        ui.write_unchecked().resolving = None;
        return; // user skipped while resolving
    }
    match res {
        Ok((info, cookie)) => {
            eprintln!(
                "[yt] stream {video_id}: client={} mime={} bitrate={}",
                info.client, info.mime, info.bitrate
            );
            app.peek().player.play_stream(&info.url, &info.mime, cookie.as_deref());
            let mut a = app.write_unchecked();
            a.playing = true;
            drop(a);
            ui.write_unchecked().stream_for = Some(track);
            ui.write_unchecked().resolving = None;
            ui.write_unchecked().yt_status = "Live · YouTube Music".to_string();
        }
        Err(e) => {
            eprintln!("[yt] resolve failed: {e}");
            // Synth fallback; stream_for stops the per-frame loop refiring.
            ui.write_unchecked().stream_for = Some(track);
            ui.write_unchecked().resolving = None;
            if app.peek().current == Some(track) {
                let mut a = app.write_unchecked();
                a.player.resume();
                a.playing = true;
            }
            ui.write_unchecked().yt_status = "Stream unavailable — synth fallback".to_string();
        }
    }
}

/// Cookie-paste sign-in: browser → music.youtube.com → devtools → Network →
/// any `/youtubei/` request → copy the `Cookie:` request header → paste.
pub async fn sign_in(app: State<AppState>, ui: State<Ui>, header: String) {
    let sess = Session::import(&header);
    if sess.is_empty() {
        ui.write_unchecked().yt_status =
            "No cookies parsed — paste the full Cookie: header value.".to_string();
        return;
    }
    let res = {
        let mut g = YT.get().unwrap().lock().await;
        let Some(c) = g.as_mut() else { return };
        c.set_session(Some(sess));
        let who = c
            .session()
            .and_then(|s| s.account_identity())
            .unwrap_or_else(|| "signed in".to_string());
        let r = c.get_library().await;
        if r.is_ok() {
            if let Some(s) = c.session() {
                let _ = s.save();
            }
        }
        (r, who)
    };
    match res {
        (Ok(home), who) => {
            let lib = YtLibrary::from_home(&home);
            eprintln!("[yt] signed in as {who}: {} tracks", lib.tracks.len());
            ui.write_unchecked().yt_status = format!("Signed in as {who}");
            HOME_LIB.get().unwrap().lock().unwrap().replace(lib.clone());
            swap_library(&app, &ui, lib, false);
        }
        (Err(e), _) => {
            eprintln!("[yt] sign-in failed: {e}");
            let mut g = YT.get().unwrap().lock().await;
            if let Some(c) = g.as_mut() {
                c.set_session(None); // back to unsigned so public content keeps working
            }
            drop(g);
            ui.write_unchecked().yt_status = format!("Sign-in failed: {e}");
        }
    }
}

fn restore_home(app: &State<AppState>, ui: &State<Ui>) {
    let home = HOME_LIB.get().unwrap().lock().unwrap().clone();
    if let Some(lib) = home {
        swap_library(app, ui, lib, false);
    }
}

/// Swap the whole library. Track ids from the old library dangle, so
/// playback/queue/selection state resets with it.
fn swap_library(app: &State<AppState>, ui: &State<Ui>, lib: Library, is_search: bool) {
    prefetch_covers(app, &lib);
    let mut a = app.write_unchecked();
    a.player.stop();
    a.library = lib;
    a.current = None;
    a.playing = false;
    a.selection = None;
    drop(a);
    let mut u = ui.write_unchecked();
    u.lib_is_search = is_search;
    u.resolving = None;
    u.stream_for = None;
}

/// Kick background fetches for the unique artwork URLs of a freshly swapped
/// library (deduped, capped; ready/flying sets make repeat calls no-ops).
fn prefetch_covers(app: &State<AppState>, lib: &Library) {
    let mut urls: Vec<String> = Vec::new();
    let mut seen = HashSet::new();
    for t in &lib.tracks {
        if let Some(u) = &t.artwork_url {
            if seen.insert(u.clone()) {
                urls.push(u.clone());
            }
        }
    }
    for a in &lib.albums {
        if urls.len() >= COVER_PREFETCH_CAP {
            break;
        }
        if let Some(u) = &a.artwork_url {
            if seen.insert(u.clone()) {
                urls.push(u.clone());
            }
        }
    }
    for r in &lib.artists {
        if urls.len() >= COVER_PREFETCH_CAP {
            break;
        }
        if let Some(u) = &r.artwork_url {
            if seen.insert(u.clone()) {
                urls.push(u.clone());
            }
        }
    }
    urls.truncate(COVER_PREFETCH_CAP);
    for u in urls {
        kick_cover(app.clone(), u);
    }
}

fn kick_cover(app: State<AppState>, url: String) {
    {
        let fly = COVERS_FLYING.get_or_init(|| Mutex::new(HashSet::new())).lock().unwrap();
        if fly.contains(&url) {
            return;
        }
    }
    COVERS_FLYING
        .get_or_init(|| Mutex::new(HashSet::new()))
        .lock()
        .unwrap()
        .insert(url.clone());
    spawn(async move {
        let fetch_url = url.clone();
        let bytes = thread(move || zuno_yt::fetch_artwork(&fetch_url)).await;
        COVERS_FLYING.get().unwrap().lock().unwrap().remove(&url);
        if let Ok(b) = bytes {
            if b.is_empty() {
                return;
            }
            let mut ok = false;
            for size in COVER_SIZES {
                if app.peek().artwork.insert_bytes(&url, size, &b).is_some() {
                    ok = true;
                }
            }
            if ok {
                COVERS_READY
                    .get_or_init(|| Mutex::new(HashSet::new()))
                    .lock()
                    .unwrap()
                    .insert(url.clone());
                crate::imgs_purge_url(&url); // drop placeholder handles; next frame uploads real art
            }
        }
    });
}
