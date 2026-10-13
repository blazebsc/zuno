//! Real-YouTube worker bridge for the gpui UI — the proven worker-thread
//! pattern (see floem/vizia): one std::thread owns a `YtClient` and a
//! single-threaded tokio runtime (gpui's executor is smol-based, so the
//! Innertube POSTs get their own reactor). The UI sends [`YtReq`], the
//! worker replies [`YtResp`], and `Zuno::on_frame` drains the channel at
//! the display frame rate. `--bench` spawns nothing.

use std::sync::mpsc::{Receiver, Sender};
use zuno_core::library::Library;
use zuno_yt::{Session, StreamInfo, YtClient, YtClientExt, YtLibrary};

/// Cache key for a cover URL — keeps gpui's (u64,u32) image map flat.
/// Top bit flipped so real-cover keys can never collide with the
/// procedural seeds (`crate::rng::hash` outputs).
pub fn cover_key(url: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in url.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    h | 0x8000_0000_0000_0000
}

#[derive(Debug)]
pub enum YtReq {
    Home,
    /// Debounce happens worker-side: the UI ships every keystroke; the
    /// worker sleeps 350 ms and keeps only the newest request.
    Search { seq: u64, query: String },
    Resolve { track: zuno_core::TrackId, video_id: String },
    SignIn { header: String },
}

pub enum YtResp {
    Home(Result<Library, String>),
    Search { seq: u64, result: Result<Library, String> },
    Stream(Result<(zuno_core::TrackId, String, StreamInfo, Option<String>), String>),
    SignedIn(Result<(Library, String), String>),
    /// Late-arriving cover bytes for the CURRENT library, fetched on
    /// throwaway threads. `url` + raw JPEG/PNG/WEBP bytes — the UI decodes
    /// through `ArtworkCache::insert_bytes` at every render size.
    Cover { url: String, bytes: Vec<u8> },
}

/// Spawn the worker. The returned sender feeds `req_rx` inside the thread;
/// the UI keeps `resp_tx`'s receiver. Both ends stay `Option` in the app:
/// `None` in bench mode.
pub fn spawn(req_rx: Receiver<YtReq>, resp_tx: Sender<YtResp>) {
    std::thread::spawn(move || {
        let rt = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
            Ok(rt) => rt,
            Err(e) => {
                eprintln!("[yt] no async runtime: {e}");
                return;
            }
        };
        let mut client = match YtClient::unsigned() {
            Ok(c) => c,
            Err(e) => {
                eprintln!("[yt] client init failed: {e}");
                return;
            }
        };
        if let Some(sess) = Session::load() {
            if !sess.is_empty() {
                if let Some(who) = sess.account_identity() {
                    eprintln!("[yt] restored saved session: {who}");
                }
                client.set_session(Some(sess));
            }
        }
        let mut pending: Vec<YtReq> = Vec::new();
        loop {
            let req = if pending.is_empty() {
                match req_rx.recv() {
                    Ok(r) => r,
                    Err(_) => break, // UI gone
                }
            } else {
                pending.remove(0)
            };
            match req {
                YtReq::Home => {
                    let lib = rt.block_on(async {
                        let mut tracks = Vec::new();
                        match client.get_home().await {
                            Ok(home) => {
                                for s in &home.shelves {
                                    tracks.extend(s.tracks.iter().cloned());
                                }
                            }
                            Err(e) => eprintln!("[yt] get_home failed: {e}"),
                        }
                        // Unsigned shelves are playlist/artist-heavy — a seed
                        // search guarantees a playable home library.
                        if tracks.len() < 20 {
                            match client.search("top hits").await {
                                Ok(res) => tracks.extend(res.tracks.iter().cloned()),
                                Err(e) => eprintln!("[yt] home seed search failed: {e}"),
                            }
                        }
                        YtLibrary::from_tracks("Home", &tracks)
                    });
                    eprintln!("[yt] home ready: {} tracks", lib.tracks.len());
                    let _ = resp_tx.send(YtResp::Home(Ok(lib)));
                }
                YtReq::Search { seq, query } => {
                    // Debounce: wait out the keystroke burst, keep the newest.
                    std::thread::sleep(std::time::Duration::from_millis(350));
                    let (mut seq, mut query) = (seq, query);
                    while let Ok(r) = req_rx.try_recv() {
                        match r {
                            YtReq::Search { seq: s2, query: q2 } => {
                                seq = s2;
                                query = q2;
                            }
                            other => pending.push(other),
                        }
                    }
                    if query.trim().len() < 2 {
                        continue;
                    }
                    match rt.block_on(client.search(&query)) {
                        Ok(res) => {
                            let lib = YtLibrary::from_search(&res);
                            eprintln!("[yt] search ready: {seq} {} tracks", lib.tracks.len());
                            let _ = resp_tx.send(YtResp::Search { seq, result: Ok(lib) });
                        }
                        Err(e) => {
                            eprintln!("[yt] search failed: {e}");
                            let _ = resp_tx.send(YtResp::Search { seq, result: Err(e.to_string()) });
                        }
                    }
                }
                YtReq::Resolve { track, video_id } => {
                    let cookie = client.session().map(|s| s.cookie_header.clone());
                    match rt.block_on(zuno_yt::resolve_stream_url(&mut client, &video_id)) {
                        Ok(info) => {
                            eprintln!(
                                "[yt] stream: {video_id} client={} mime={}",
                                info.client, info.mime
                            );
                            let _ = resp_tx.send(YtResp::Stream(Ok((track, video_id, info, cookie))));
                        }
                        Err(e) => {
                            eprintln!("[yt] resolve failed: {e}");
                            let _ = resp_tx.send(YtResp::Stream(Err(e.to_string())));
                        }
                    }
                }
                YtReq::SignIn { header } => {
                    let sess = Session::import(&header);
                    if sess.is_empty() {
                        let _ = resp_tx.send(YtResp::SignedIn(Err(
                            "No cookies parsed — paste the full Cookie: header value.".into(),
                        )));
                        continue;
                    }
                    client.set_session(Some(sess));
                    match rt.block_on(client.get_library()) {
                        Ok(home) => {
                            let lib = YtLibrary::from_home(&home);
                            let who = client
                                .session()
                                .and_then(|s| s.account_identity())
                                .unwrap_or_else(|| "signed in".to_string());
                            if let Some(s) = client.session() {
                                let _ = s.save();
                            }
                            eprintln!("[yt] signed in as {who}: {} tracks", lib.tracks.len());
                            let _ = resp_tx.send(YtResp::SignedIn(Ok((lib, who))));
                        }
                        Err(e) => {
                            client.set_session(None);
                            let _ = resp_tx.send(YtResp::SignedIn(Err(format!("Sign-in failed: {e}"))));
                        }
                    }
                }
            }
        }
    });
}
