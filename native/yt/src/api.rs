//! Typed Innertube surface. Every method posts through the one client path
//! and returns plain data — no youtubei.js node types cross this boundary.
//!
//! Reads cover the whole app flow: mixed search, per-category search,
//! suggestions, album, artist (with subscription state), playlist with
//! paginated track pages, library (signed-in), home feeds, up-next and the
//! related tab. Mutations live in [`crate::mutations`].

use crate::client::*;
use crate::error::{Result, YtError};
use crate::model::*;
use crate::parse::*;
use crate::YtClient;
use serde_json::{json, Value};
use std::collections::HashSet;

fn url(base: &str, key: &str) -> String {
    format!("{base}?key={key}&prettyPrint=false")
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

async fn music_post_unsigned(
    client: &mut YtClient,
    endpoint: &str,
    body: Value,
) -> Result<Value> {
    client
        .post_unsigned(
            &url(endpoint, MUSIC_KEY),
            CLIENT_WEB_REMIX,
            "1.20250506.00.00",
            UA_WEB,
            body,
        )
        .await
}

async fn music_post_signed(client: &mut YtClient, endpoint: &str, body: Value) -> Result<Value> {
    client
        .post_signed(
            &url(endpoint, MUSIC_KEY),
            CLIENT_WEB_REMIX,
            "1.20250506.00.00",
            UA_WEB,
            body,
        )
        .await
}

/// The search filter categories (`SearchCategory` in the app). `params` is
/// the base64 of the `SearchFilter` protobuf youtubei.js builds —
/// `musicSearchType: { <kind>: true }` — precomputed here because the
/// encoding is fixed for all time (field numbers never change).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SearchCategory {
    Song,
    Video,
    Album,
    Artist,
    Playlist,
}

impl SearchCategory {
    fn params(self) -> &'static str {
        match self {
            SearchCategory::Song => "EgWKAQIIAQ%3D%3D",
            SearchCategory::Video => "EgWKAQIQAQ%3D%3D",
            SearchCategory::Album => "EgWKAQIYAQ%3D%3D",
            SearchCategory::Artist => "EgWKAQIgAQ%3D%3D",
            SearchCategory::Playlist => "EgWKAQIoAQ%3D%3D",
        }
    }
}

/// Implemented on [`YtClient`] from here so `client.rs` stays auth-only.
#[allow(async_fn_in_trait)]
pub trait YtClientExt {
    /// Mixed search: songs, albums, playlists and artists in one response.
    async fn search(&mut self, query: &str) -> Result<YtSearchResults>;
    /// One filtered search — how the category tabs go deeper than the
    /// handful of rows a mixed search returns per category.
    async fn search_category(&mut self, query: &str, category: SearchCategory) -> Result<YtSearchResults>;
    /// Search box suggestions, deduped, first three — the app's cap.
    async fn search_suggestions(&mut self, query: &str) -> Result<Vec<String>>;
    async fn get_album(&mut self, browse_id: &str) -> Result<YtAlbum>;
    async fn get_artist(&mut self, channel_id: &str) -> Result<YtArtist>;
    /// First page of a playlist (add `VL` if missing, like the TS helper).
    async fn get_playlist(&mut self, id: &str) -> Result<YtPlaylist>;
    /// One page of a playlist's tracks: first call gets page 1 (pass `None`),
    /// later calls continue from the returned session. Dedupes against
    /// everything the session has already seen.
    async fn get_playlist_track_page(
        &mut self,
        playlist_id: &str,
        session: Option<PlaylistPageSession>,
    ) -> Result<(YtTrackPage, Option<PlaylistPageSession>)>;
    /// The whole playlist, every page — the termination guard lives in
    /// [`collect_track_pages`].
    async fn get_playlist_tracks(&mut self, id: &str) -> Result<Vec<YtTrack>>;
    async fn get_home(&mut self) -> Result<YtHome>;
    async fn get_library(&mut self) -> Result<YtHome>;
    /// Up-next for a video: the `next` endpoint's queue panel
    /// (`getRecommendations` in the app).
    async fn get_recommendations(&mut self, video_id: &str) -> Result<Vec<YtTrack>>;
    /// The related tab (`getRelated` in the app): the `next` response's
    /// `MUSIC_PAGE_TYPE_TRACK_RELATED` tab browsed into its shelves.
    async fn get_related(&mut self, video_id: &str) -> Result<Vec<YtShelf>>;
}

impl YtClientExt for YtClient {
    /// Unsigned `search` on WEB_REMIX — works with zero credentials.
    async fn search(&mut self, query: &str) -> Result<YtSearchResults> {
        let body = post_body(web_remix_context(), json!({ "query": query }));
        let resp = music_post_unsigned(self, MUSIC_SEARCH_URL, body).await?;
        Ok(parse_search(&resp, query))
    }

    async fn search_category(&mut self, query: &str, category: SearchCategory) -> Result<YtSearchResults> {
        let body = post_body(
            web_remix_context(),
            json!({ "query": query, "params": category.params() }),
        );
        let resp = music_post_unsigned(self, MUSIC_SEARCH_URL, body).await?;
        Ok(parse_search(&resp, query))
    }

    async fn search_suggestions(&mut self, query: &str) -> Result<Vec<String>> {
        let body = post_body(web_remix_context(), json!({ "input": query }));
        let resp = music_post_unsigned(
            self,
            "https://music.youtube.com/youtubei/v1/music/get_search_suggestions",
            body,
        )
        .await?;
        // searchSuggestionsSectionRenderer.contents[].searchSuggestionRenderer
        let mut out: Vec<String> = Vec::new();
        if let Some(sections) = resp.get("contents").and_then(|c| c.as_array()) {
            for section in sections {
                let Some(section) = section.get("searchSuggestionsSectionRenderer") else { continue };
                let Some(contents) = section.get("contents").and_then(|c| c.as_array()) else { continue };
                for item in contents {
                    let text = item
                        .get("searchSuggestionRenderer")
                        .and_then(|r| r.get("suggestion"))
                        .map(runs_text);
                    if let Some(text) = text {
                        if !text.is_empty() && !out.contains(&text) {
                            out.push(text);
                        }
                    }
                }
            }
        }
        out.truncate(3); // the app's cap
        Ok(out)
    }

    /// Album page: `browse` with the `MPRE…` id.
    async fn get_album(&mut self, browse_id: &str) -> Result<YtAlbum> {
        let body = post_body(web_remix_context(), json!({ "browseId": browse_id }));
        let resp = music_post_unsigned(self, MUSIC_BROWSE_URL, body).await?;
        parse_album(&resp, browse_id)
    }

    /// Artist page: `browse` with the `UC…` channel id.
    async fn get_artist(&mut self, channel_id: &str) -> Result<YtArtist> {
        let body = post_body(web_remix_context(), json!({ "browseId": channel_id }));
        let resp = music_post_unsigned(self, MUSIC_BROWSE_URL, body).await?;
        parse_artist(&resp, channel_id)
    }

    /// Playlist page: `browse` with the `VL…` id (a bare playlist id gets
    /// the `VL` prefix, like the TS `browseId` helper).
    async fn get_playlist(&mut self, id: &str) -> Result<YtPlaylist> {
        let browse_id = if id.starts_with("VL") { id.to_string() } else { format!("VL{id}") };
        let body = post_body(web_remix_context(), json!({ "browseId": browse_id }));
        let resp = music_post_unsigned(self, MUSIC_BROWSE_URL, body).await?;
        parse_playlist(&resp, id)
    }

    /// One page of a playlist's tracks. Mirrors `getPlaylistTrackPage`:
    /// page sessions keep the continuation token and the set of track ids
    /// already yielded, so later pages only carry fresh rows.
    async fn get_playlist_track_page(
        &mut self,
        playlist_id: &str,
        session: Option<PlaylistPageSession>,
    ) -> Result<(YtTrackPage, Option<PlaylistPageSession>)> {
        match session {
            None => {
                let browse_id =
                    if playlist_id.starts_with("VL") { playlist_id.to_string() } else { format!("VL{playlist_id}") };
                let body = post_body(web_remix_context(), json!({ "browseId": browse_id }));
                let resp = music_post_unsigned(self, MUSIC_BROWSE_URL, body).await?;
                let mut seen = HashSet::new();
                let tracks: Vec<YtTrack> = playlist_page_tracks(&resp, &mut seen);
                let continuation = find_continuation(&resp);
                let page = YtTrackPage {
                    tracks,
                    has_more: continuation.is_some(),
                    next_page_key: continuation.clone(),
                };
                let next = continuation.map(|token| PlaylistPageSession {
                    playlist_id: playlist_id.to_string(),
                    continuation: token,
                    seen,
                });
                Ok((page, next))
            }
            Some(mut session) => {
                if session.playlist_id != playlist_id {
                    return Err(YtError::Parse("page session belongs to another playlist".into()));
                }
                let body = post_body(web_remix_context(), json!({ "continuation": session.continuation }));
                let resp = music_post_unsigned(self, MUSIC_BROWSE_URL, body).await?;
                let fresh: Vec<YtTrack> = playlist_page_tracks(&resp, &mut session.seen);
                let continuation = find_continuation(&resp);
                let page = YtTrackPage {
                    tracks: fresh,
                    has_more: continuation.is_some(),
                    next_page_key: continuation.clone(),
                };
                let next = continuation.map(|token| PlaylistPageSession {
                    playlist_id: session.playlist_id.clone(),
                    continuation: token,
                    seen: session.seen.clone(),
                });
                Ok((page, next))
            }
        }
    }

    async fn get_playlist_tracks(&mut self, id: &str) -> Result<Vec<YtTrack>> {
        // First page carries the session's seed tracks.
        let (first, session) = self.get_playlist_track_page(id, None).await?;
        let mut collected = first.tracks;
        let has_more = first.has_more;
        if !has_more {
            return Ok(collected);
        }
        let mut source = PlaylistPageSource { client: self, session };
        let fresh = collect_track_pages(
            &mut collected,
            has_more,
            50, // ponytail: page cap — the app's 200 covers pathological libraries; a music playlist never reaches it
            &mut source,
        )
        .await?;
        Ok(fresh)
    }

    /// Unsigned home: charts + explore feeds via `browse` with the stable
    /// feed ids (the same literals the TS `BROWSE_IDS` table holds).
    async fn get_home(&mut self) -> Result<YtHome> {
        let mut shelves = Vec::new();
        for browse_id in ["FEmusic_charts", "FEmusic_explore"] {
            let body = post_body(web_remix_context(), json!({ "browseId": browse_id }));
            match music_post_unsigned(self, MUSIC_BROWSE_URL, body).await {
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
            let resp = music_post_signed(self, MUSIC_BROWSE_URL, body).await?;
            shelves.extend(parse_shelves(&resp));
        }
        Ok(YtHome { shelves })
    }

    async fn get_recommendations(&mut self, video_id: &str) -> Result<Vec<YtTrack>> {
        let body = post_body(web_remix_context(), json!({ "videoId": video_id }));
        let resp = music_post_unsigned(self, MUSIC_NEXT_URL, body).await?;
        let mut items = Vec::new();
        collect_panel_videos(&resp, &mut items);
        Ok(items.into_iter().filter_map(parse_panel_track).collect())
    }

    async fn get_related(&mut self, video_id: &str) -> Result<Vec<YtShelf>> {
        // find the Related tab's browse endpoint in the next response
        let body = post_body(web_remix_context(), json!({ "videoId": video_id }));
        let next = music_post_unsigned(self, MUSIC_NEXT_URL, body).await?;
        let endpoint = find_related_tab_browse_id(&next)
            .ok_or_else(|| YtError::Parse(format!("no related tab for {video_id}")))?;
        let body = post_body(web_remix_context(), json!({ "browseId": endpoint }));
        let resp = music_post_unsigned(self, MUSIC_BROWSE_URL, body).await?;
        let mut shelves = parse_shelves(&resp);
        // The related page's shelves are carousels; dedupe by title like the app
        let mut seen_titles = HashSet::new();
        shelves.retain(|s| !s.title.trim().is_empty() && seen_titles.insert(s.title.clone()));
        Ok(shelves)
    }
}

/// A live playlist pagination cursor. Opaque to UIs — pass it back to
/// [`YtClientExt::get_playlist_track_page`].
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct PlaylistPageSession {
    pub playlist_id: String,
    pub continuation: String,
    #[serde(skip)]
    pub seen: HashSet<String>,
}

impl PlaylistPageSession {
    pub fn playlist_id(&self) -> &str {
        &self.playlist_id
    }
}

/// `TrackPageSource` for [`collect_track_pages`] over a real client.
struct PlaylistPageSource<'a> {
    client: &'a mut YtClient,
    session: Option<PlaylistPageSession>,
}

#[allow(async_fn_in_trait)]
pub trait TrackPageSource {
    async fn fetch_page(&mut self, page_key: &str) -> Result<YtTrackPage>;
}

impl TrackPageSource for PlaylistPageSource<'_> {
    async fn fetch_page(&mut self, page_key: &str) -> Result<YtTrackPage> {
        let session = self.session.take();
        let (playlist_id, session) = match session {
            Some(s) => (s.playlist_id.clone(), Some(s)),
            None => (page_key.to_string(), None),
        };
        let (page, next) = self
            .client
            .get_playlist_track_page(&playlist_id, session)
            .await
            .map_err(|e| YtError::Parse(format!("playlist page {page_key}: {e}")))?;
        self.session = next;
        Ok(page)
    }
}

/// Pages a track list to the end — the termination guard from
/// `collectTrackPages.ts`, verbatim semantics: a cursor that stops advancing
/// or hands back the same page must not spin until the page cap.
pub async fn collect_track_pages<S: TrackPageSource>(
    collected: &mut Vec<YtTrack>,
    has_more: bool,
    max_pages: usize,
    source: &mut S,
) -> Result<Vec<YtTrack>> {
    let mut more = has_more;
    let mut page_key: Option<String> = Some(String::new());
    let mut pages = 0usize;
    while more && page_key.is_some() && pages < max_pages {
        let key = page_key.clone().unwrap_or_default();
        let result = source.fetch_page(&key).await?;
        pages += 1;
        let fresh: Vec<YtTrack> = result
            .tracks
            .into_iter()
            .filter(|t| !collected.iter().any(|c| c.video_id == t.video_id))
            .collect();
        let next_key = result.next_page_key;
        // Nothing new *and* the cursor did not move means the source is
        // handing back the same page: stop rather than spin to the cap.
        if fresh.is_empty() && next_key.as_deref() == Some(key.as_str()) {
            break;
        }
        if !fresh.is_empty() {
            collected.extend(fresh);
        }
        more = result.has_more;
        page_key = next_key;
    }
    Ok(collected.clone())
}

/// Tracks of one playlist-style browse/continuation response, deduped
/// against `seen` (which absorbs every id yielded).
fn playlist_page_tracks(resp: &Value, seen: &mut HashSet<String>) -> Vec<YtTrack> {
    let mut items = Vec::new();
    collect_list_items(resp, &mut items);
    let mut tracks = Vec::new();
    for item in items {
        let Some(track) = parse_list_track(item) else { continue };
        if seen.insert(track.video_id.clone()) {
            tracks.push(track);
        }
    }
    // Continuation responses put rows under append actions instead of shelves.
    if tracks.is_empty() {
        let mut appended = Vec::new();
        collect_append_items(resp, &mut appended);
        for item in appended {
            let Some(track) = parse_list_track(item) else { continue };
            if seen.insert(track.video_id.clone()) {
                tracks.push(track);
            }
        }
    }
    tracks
}

/// Rows under `onResponseReceivedActions[].appendContinuationItemsAction.
/// continuationItems` — the shape playlist continuation responses use.
fn collect_append_items<'a>(v: &'a Value, out: &mut Vec<&'a Value>) {
    let Some(actions) = v.get("onResponseReceivedActions").and_then(|a| a.as_array()) else {
        return };
    for action in actions {
        let items = action
            .get("appendContinuationItemsAction")
            .and_then(|a| a.get("continuationItems"))
            .and_then(|c| c.as_array());
        if let Some(items) = items {
            for item in items {
                collect_list_items(item, out);
            }
        }
    }
}

/// `getMusicContinuation`'s explicit sites first, then the tree walk: the
/// next-page token, wherever this surface keeps it. Order matters:
/// shelf-level tokens and the shelf's trailing `continuationItemRenderer`
/// (primary contents) must win over the `secondaryContents` section list's
/// sidebar token — the DFS reaches primary contents first, like the TS walk.
pub fn find_continuation(resp: &Value) -> Option<String> {
    if let Some(found) = find_continuation_field(resp) {
        return Some(found);
    }
    if let Some(found) = find_continuation_item_token(resp) {
        return Some(found);
    }
    if let Some(found) = find_section_list_continuation(resp) {
        return Some(found);
    }
    None
}

/// The renderer shapes that own a direct `continuation` field: shelves,
/// grids and their continuation payloads.
const CONTINUATION_PARENTS: &[&str] = &[
    "musicShelfRenderer",
    "musicPlaylistShelfRenderer",
    "gridRenderer",
    "musicPlaylistShelfContinuation",
    "musicShelfContinuation",
    "gridContinuation",
];

fn find_continuation_field(v: &Value) -> Option<String> {
    match v {
        Value::Object(map) => {
            for parent in CONTINUATION_PARENTS {
                if let Some(node) = map.get(*parent) {
                    if let Some(token) = node.get("continuation").and_then(|t| t.as_str()) {
                        if !token.is_empty() {
                            return Some(token.to_string());
                        }
                    }
                }
            }
            map.values().find_map(find_continuation_field)
        }
        Value::Array(items) => items.iter().find_map(find_continuation_field),
        _ => None,
    }
}

/// Section lists keep their token in
/// `continuations[0].nextContinuationData.continuation` — checked last,
/// because on playlist pages this is the *sidebar's* token, not the tracks'.
fn find_section_list_continuation(v: &Value) -> Option<String> {
    match v {
        Value::Object(map) => {
            for parent in ["sectionListRenderer", "sectionListContinuation"] {
                if let Some(node) = map.get(parent) {
                    if let Some(conts) = node.get("continuations").and_then(|c| c.as_array()) {
                        for c in conts {
                            let token = c
                                .get("nextContinuationData")
                                .and_then(|n| n.get("continuation"))
                                .and_then(|t| t.as_str());
                            if let Some(token) = token.filter(|t| !t.is_empty()) {
                                return Some(token.to_string());
                            }
                        }
                    }
                    if let Some(token) = node.get("continuation").and_then(|t| t.as_str()) {
                        if !token.is_empty() {
                            return Some(token.to_string());
                        }
                    }
                }
            }
            map.values().find_map(find_section_list_continuation)
        }
        Value::Array(items) => items.iter().find_map(find_section_list_continuation),
        _ => None,
    }
}

fn find_continuation_item_token(v: &Value) -> Option<String> {
    match v {
        Value::Object(map) => {
            if let Some(item) = map.get("continuationItemRenderer") {
                let token = item
                    .get("continuationEndpoint")
                    .and_then(|e| e.get("continuationCommand"))
                    .and_then(|c| c.get("token"))
                    .and_then(|t| t.as_str());
                if let Some(token) = token {
                    return Some(token.to_string());
                }
            }
            map.values().find_map(find_continuation_item_token)
        }
        Value::Array(items) => items.iter().find_map(find_continuation_item_token),
        _ => None,
    }
}

/// The related tab's browse id (`MPTRt_…`): the `next` response's tab whose
/// `pageType` is `MUSIC_PAGE_TYPE_TRACK_RELATED`.
pub fn find_related_tab_browse_id(next: &Value) -> Option<String> {
    let mut found = None;
    find_related_tab(next, &mut found);
    found
}

fn find_related_tab(v: &Value, out: &mut Option<String>) {
    if out.is_some() {
        return;
    }
    match v {
        Value::Object(map) => {
            if let Some(tab) = map.get("tabRenderer") {
                let page_type = tab
                    .get("endpoint")
                    .and_then(|e| e.get("browseEndpoint"))
                    .and_then(|e| e.get("browseEndpointContextSupportedConfigs"))
                    .and_then(|c| c.get("browseEndpointContextMusicConfig"))
                    .and_then(|c| c.get("pageType"))
                    .and_then(|p| p.as_str());
                if page_type == Some("MUSIC_PAGE_TYPE_TRACK_RELATED") {
                    *out = tab
                        .get("endpoint")
                        .and_then(|e| e.get("browseEndpoint"))
                        .and_then(|e| e.get("browseId"))
                        .and_then(|b| b.as_str())
                        .map(str::to_string);
                    return;
                }
            }
            for child in map.values() {
                find_related_tab(child, out);
                if out.is_some() {
                    return;
                }
            }
        }
        Value::Array(items) => {
            for child in items {
                find_related_tab(child, out);
                if out.is_some() {
                    return;
                }
            }
        }
        _ => {}
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
    // Subscription state — signed-in responses carry `subscribed: true`
    // (`findArtistSubscriptionToggle`); unsigned ones carry the button too,
    // with `subscribed` absent, which stays `None`.
    let subscribed = find_subscribed(resp);
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
        subscribed,
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
    let mut seen = HashSet::new();
    let tracks = playlist_page_tracks(resp, &mut seen);
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
        let mut seen = HashSet::new();
        let tracks = playlist_page_tracks(resp, &mut seen);
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
