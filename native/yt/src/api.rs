//! Typed Innertube surface. Every method posts through the one client path
//! and returns plain data — no youtubei.js node types cross this boundary.

use crate::client::*;
use crate::error::{Result, YtError};
use crate::model::*;
use crate::parse::*;
use crate::YtClient;
use serde_json::{json, Value};

fn url(base: &str, key: &str) -> String {
    format!("{base}?key={key}")
}

fn post_body(context: Value, extra: Value) -> Value {
    let mut body = json!({ "context": context });
    if let Some(obj) = extra.as_object() {
        for (k, v) in obj {
            body[k] = v.clone();
        }
    }
    body
}

/// Implemented on [`YtClient`] from here so `client.rs` stays auth-only.
#[allow(async_fn_in_trait)]
pub trait YtClientExt {
    async fn search(&mut self, query: &str) -> Result<YtSearchResults>;
    async fn get_album(&mut self, browse_id: &str) -> Result<YtAlbum>;
    async fn get_artist(&mut self, channel_id: &str) -> Result<YtArtist>;
    async fn get_playlist(&mut self, id: &str) -> Result<YtPlaylist>;
    async fn get_home(&mut self) -> Result<YtHome>;
    async fn get_library(&mut self) -> Result<YtHome>;
    async fn get_related(&mut self, video_id: &str) -> Result<Vec<YtTrack>>;
}

impl YtClientExt for YtClient {
    /// Unsigned `search` on WEB_REMIX — works with zero credentials.
    async fn search(&mut self, query: &str) -> Result<YtSearchResults> {
        let body = post_body(web_remix_context(), json!({ "query": query }));
        let resp = self
            .post_unsigned(
                &url(MUSIC_SEARCH_URL, MUSIC_KEY),
                CLIENT_WEB_REMIX,
                "1.20250506.00.00",
                UA_WEB,
                body,
            )
            .await?;
        Ok(parse_search(&resp, query))
    }

    /// Album page: `browse` with the `MPRE…` id.
    async fn get_album(&mut self, browse_id: &str) -> Result<YtAlbum> {
        let body = post_body(web_remix_context(), json!({ "browseId": browse_id }));
        let resp = self
            .post_unsigned(
                &url(MUSIC_BROWSE_URL, MUSIC_KEY),
                CLIENT_WEB_REMIX,
                "1.20250506.00.00",
                UA_WEB,
                body,
            )
            .await?;
        parse_album(&resp, browse_id)
    }

    /// Artist page: `browse` with the `UC…` channel id.
    async fn get_artist(&mut self, channel_id: &str) -> Result<YtArtist> {
        let body = post_body(web_remix_context(), json!({ "browseId": channel_id }));
        let resp = self
            .post_unsigned(
                &url(MUSIC_BROWSE_URL, MUSIC_KEY),
                CLIENT_WEB_REMIX,
                "1.20250506.00.00",
                UA_WEB,
                body,
            )
            .await?;
        parse_artist(&resp, channel_id)
    }

    /// Playlist page: `browse` with the `VL…` id (a bare playlist id gets
    /// the `VL` prefix, like the TS `browseId` helper).
    async fn get_playlist(&mut self, id: &str) -> Result<YtPlaylist> {
        let browse_id =
            if id.starts_with("VL") { id.to_string() } else { format!("VL{id}") };
        let body = post_body(web_remix_context(), json!({ "browseId": browse_id }));
        let resp = self
            .post_unsigned(
                &url(MUSIC_BROWSE_URL, MUSIC_KEY),
                CLIENT_WEB_REMIX,
                "1.20250506.00.00",
                UA_WEB,
                body,
            )
            .await?;
        parse_playlist(&resp, id)
    }

    /// Unsigned home: charts + explore feeds via `browse` with the stable
    /// feed ids (the same literals the TS `BROWSE_IDS` table holds).
    async fn get_home(&mut self) -> Result<YtHome> {
        let mut shelves = Vec::new();
        for browse_id in ["FEmusic_charts", "FEmusic_explore"] {
            let body = post_body(web_remix_context(), json!({ "browseId": browse_id }));
            match self
                .post_unsigned(
                    &url(MUSIC_BROWSE_URL, MUSIC_KEY),
                    CLIENT_WEB_REMIX,
                    "1.20250506.00.00",
                    UA_WEB,
                    body,
                )
                .await
            {
                Ok(resp) => shelves.extend(parse_shelves(&resp)),
                Err(e) => eprintln!("[zuno-yt] home feed {browse_id} failed: {e}"),
            }
        }
        Ok(YtHome { shelves })
    }

    /// Signed-in library only — unsigned callers get a clean `NotSignedIn`.
    async fn get_library(&mut self) -> Result<YtHome> {
        if !self.signed_in() {
            return Err(YtError::NotSignedIn);
        }
        let mut shelves = Vec::new();
        for browse_id in ["FEmusic_library_landing", "FEmusic_history"] {
            let body = post_body(web_remix_context(), json!({ "browseId": browse_id }));
            let resp = self
                .post_signed(
                    &url(MUSIC_BROWSE_URL, MUSIC_KEY),
                    CLIENT_WEB_REMIX,
                    "1.20250506.00.00",
                    UA_WEB,
                    body,
                )
                .await?;
            shelves.extend(parse_shelves(&resp));
        }
        Ok(YtHome { shelves })
    }

    /// Up-next for a video: the `next` endpoint's queue panel.
    async fn get_related(&mut self, video_id: &str) -> Result<Vec<YtTrack>> {
        let body = post_body(web_remix_context(), json!({ "videoId": video_id }));
        let resp = self
            .post_unsigned(
                &url(MUSIC_NEXT_URL, MUSIC_KEY),
                CLIENT_WEB_REMIX,
                "1.20250506.00.00",
                UA_WEB,
                body,
            )
            .await?;
        let mut items = Vec::new();
        collect_panel_videos(&resp, &mut items);
        Ok(items.into_iter().filter_map(parse_panel_track).collect())
    }
}

fn find_first<'a>(v: &'a Value, key: &str) -> Option<&'a Value> {
    match v {
        Value::Object(map) => {
            if let Some(hit) = map.get(key) {
                return Some(hit);
            }
            map.values().find_map(|c| find_first(c, key))
        }
        Value::Array(items) => items.iter().find_map(|c| find_first(c, key)),
        _ => None,
    }
}

pub fn parse_search(resp: &Value, query: &str) -> YtSearchResults {
    let mut out = YtSearchResults { query: query.to_string(), ..Default::default() };
    let mut items = Vec::new();
    collect_list_items(resp, &mut items);
    for item in items {
        if let Some(track) = parse_list_track(item) {
            out.tracks.push(track);
            continue;
        }
        match parse_list_non_track(item) {
            Some(ListNonTrack::Album(a)) => out.albums.push(a),
            Some(ListNonTrack::Playlist(p)) => out.playlists.push(p),
            Some(ListNonTrack::Artist(a)) => out.artists.push(a),
            None => {}
        }
    }
    let mut rows = Vec::new();
    collect_tworows(resp, &mut rows);
    for row in rows {
        match classify_tworow(row).0 {
            TwoRowKind::Album => {
                if let Some(a) = parse_tworow_album(row) {
                    out.albums.push(a);
                }
            }
            TwoRowKind::Artist => {
                if let Some(a) = parse_tworow_artist(row) {
                    out.artists.push(a);
                }
            }
            TwoRowKind::Playlist => {
                if let Some(p) = parse_tworow_playlist(row) {
                    out.playlists.push(p);
                }
            }
            TwoRowKind::Unknown => {}
        }
    }
    out
}

pub fn parse_album(resp: &Value, browse_id: &str) -> Result<YtAlbum> {
    let header = find_first(resp, "musicResponsiveHeaderRenderer")
        .ok_or_else(|| YtError::NotFound(format!("album {browse_id}: no header")))?;
    let title = header.get("title").map(runs_text).unwrap_or_default();
    let subtitle = header.get("subtitle").map(run_parts).unwrap_or_default();
    let (kind, year) = parse_kind_year(&subtitle);
    let mut artist = String::new();
    let mut artist_channel_id = String::new();
    if let Some(strap) = header.get("straplineTextOne") {
        if let Some(runs) = strap.get("runs").and_then(|r| r.as_array()) {
            if let Some(run) = runs.first() {
                artist = run.get("text").and_then(|t| t.as_str()).unwrap_or_default().to_string();
                artist_channel_id = run
                    .get("navigationEndpoint")
                    .and_then(|e| e.get("browseEndpoint"))
                    .and_then(|e| e.get("browseId"))
                    .and_then(|b| b.as_str())
                    .unwrap_or_default()
                    .to_string();
            }
        }
    }
    let mut items = Vec::new();
    collect_list_items(resp, &mut items);
    let tracks: Vec<YtTrack> = items.into_iter().filter_map(parse_list_track).collect();
    if title.is_empty() && tracks.is_empty() {
        return Err(YtError::NotFound(format!("album {browse_id}: empty")));
    }
    Ok(YtAlbum {
        browse_id: browse_id.to_string(),
        title,
        artist,
        artist_channel_id,
        year,
        kind,
        thumbnails: collect_thumbs(header),
        tracks,
    })
}

fn parse_kind_year(parts: &[String]) -> (String, Option<u16>) {
    let mut kind = "Album".to_string();
    let mut year = None;
    for part in parts {
        match part.to_ascii_lowercase().as_str() {
            "single" => kind = "Single".to_string(),
            "ep" => kind = "EP".to_string(),
            "album" => kind = "Album".to_string(),
            _ => {
                if let Ok(y) = part.parse::<u16>() {
                    if (1000..=2100).contains(&y) {
                        year = Some(y);
                    }
                }
            }
        }
    }
    (kind, year)
}

pub fn parse_artist(resp: &Value, channel_id: &str) -> Result<YtArtist> {
    let header = find_first(resp, "musicImmersiveHeaderRenderer")
        .or_else(|| find_first(resp, "musicDetailHeaderRenderer"));
    let (name, subscriber_count, header_thumbs) = match header {
        Some(h) => (
            h.get("title").map(runs_text).unwrap_or_default(),
            h.get("subscriptionButton")
                .and_then(|b| b.get("subscribeButtonRenderer"))
                .and_then(|b| b.get("subscriberCountText"))
                .map(runs_text),
            collect_thumbs(h),
        ),
        None => (String::new(), None, Vec::new()),
    };
    let mut items = Vec::new();
    collect_list_items(resp, &mut items);
    let top_tracks: Vec<YtTrack> = items.into_iter().filter_map(parse_list_track).collect();
    let mut rows = Vec::new();
    collect_tworows(resp, &mut rows);
    // Only album-kind rows: artist pages also list songs/videos as twoRows
    // (their nav target is not an MPRE id — see parse_tworow_album).
    let mut albums: Vec<YtAlbumRefFull> = Vec::new();
    for row in rows {
        if matches!(classify_tworow(row).0, TwoRowKind::Album) {
            if let Some(a) = parse_tworow_album(row) {
                albums.push(a);
            }
        }
    }
    if name.is_empty() && top_tracks.is_empty() && albums.is_empty() {
        return Err(YtError::NotFound(format!("artist {channel_id}: empty")));
    }
    Ok(YtArtist {
        channel_id: channel_id.to_string(),
        name,
        subscriber_count,
        thumbnails: header_thumbs,
        top_tracks,
        albums,
    })
}

pub fn parse_playlist(resp: &Value, id: &str) -> Result<YtPlaylist> {
    let header = find_first(resp, "musicResponsiveHeaderRenderer")
        .ok_or_else(|| YtError::NotFound(format!("playlist {id}: no header")))?;
    let title = header.get("title").map(runs_text).unwrap_or_default();
    let description = header
        .get("description")
        .map(runs_text)
        .filter(|t| !t.is_empty());
    let mut items = Vec::new();
    collect_list_items(resp, &mut items);
    let tracks: Vec<YtTrack> = items.into_iter().filter_map(parse_list_track).collect();
    if title.is_empty() && tracks.is_empty() {
        return Err(YtError::NotFound(format!("playlist {id}: empty")));
    }
    Ok(YtPlaylist {
        id: id.to_string(),
        title,
        description,
        thumbnails: collect_thumbs(header),
        tracks,
    })
}

pub fn parse_shelves(resp: &Value) -> Vec<YtShelf> {
    let mut raws = Vec::new();
    collect_shelves(resp, &mut raws);
    let mut shelves = Vec::new();
    for raw in raws {
        let title = shelf_title(raw);
        let mut items = Vec::new();
        collect_list_items(raw, &mut items);
        let mut tracks = Vec::new();
        let mut shelf_artists = Vec::new();
        for item in items {
            if let Some(track) = parse_list_track(item) {
                tracks.push(track);
                continue;
            }
            // Charts' "Top artists" and similar: videoId-less artist rows.
            if let Some(artist) = parse_list_artist(item) {
                shelf_artists.push(artist);
            }
        }
        let mut rows = Vec::new();
        collect_tworows(raw, &mut rows);
        let mut albums = Vec::new();
        let mut playlists = Vec::new();
        let mut artists = Vec::new();
        for row in rows {
            match classify_tworow(row).0 {
                TwoRowKind::Album => {
                    if let Some(a) = parse_tworow_album(row) {
                        albums.push(a);
                    }
                }
                TwoRowKind::Playlist => {
                    if let Some(p) = parse_tworow_playlist(row) {
                        playlists.push(p);
                    }
                }
                TwoRowKind::Artist => {
                    if let Some(a) = parse_tworow_artist(row) {
                        artists.push(a);
                    }
                }
                TwoRowKind::Unknown => {}
            }
        }
        // Panel videos can appear in shelves too (charts' top-songs rows).
        artists.extend(shelf_artists);
        if tracks.is_empty() {
            let mut panels = Vec::new();
            collect_panel_videos(raw, &mut panels);
            let extra: Vec<YtTrack> = panels.into_iter().filter_map(parse_panel_track).collect();
            if !extra.is_empty() || !albums.is_empty() || !playlists.is_empty() || !artists.is_empty() {
                shelves.push(YtShelf { title, tracks: extra, albums, playlists, artists });
            }
            continue;
        }
        shelves.push(YtShelf { title, tracks, albums, playlists, artists });
    }
    // Flat fallback: response holds music but no shelf parsed (shape drift) —
    // one shelf with everything, like the TS flat fallback.
    if shelves.is_empty() {
        let mut items = Vec::new();
        collect_list_items(resp, &mut items);
        let tracks: Vec<YtTrack> = items.into_iter().filter_map(parse_list_track).collect();
        let mut rows = Vec::new();
        collect_tworows(resp, &mut rows);
        let mut albums = Vec::new();
        let mut artists = Vec::new();
        let mut playlists = Vec::new();
        for row in rows {
            match classify_tworow(row).0 {
                TwoRowKind::Album => {
                    if let Some(a) = parse_tworow_album(row) {
                        albums.push(a);
                    }
                }
                TwoRowKind::Artist => {
                    if let Some(a) = parse_tworow_artist(row) {
                        artists.push(a);
                    }
                }
                TwoRowKind::Playlist => {
                    if let Some(p) = parse_tworow_playlist(row) {
                        playlists.push(p);
                    }
                }
                TwoRowKind::Unknown => {}
            }
        }
        if !tracks.is_empty() || !albums.is_empty() || !artists.is_empty() || !playlists.is_empty() {
            shelves.push(YtShelf {
                title: "Results".into(),
                tracks,
                albums,
                playlists,
                artists,
            });
        }
    }
    shelves
}
