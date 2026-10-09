//! Minimal robust Innertube parsing: recursive collectors over raw JSON.
//!
//! YouTube renames containers freely (`musicShelfRenderer` vs bare
//! `itemSectionRenderer` wrappers, `twoColumnBrowseResultsRenderer` vs
//! `singleColumn…`), but the leaf renderers are stable:
//! `musicResponsiveListItemRenderer` (songs), `musicTwoRowItemRenderer`
//! (albums/artists/playlists), `playlistPanelVideoRenderer` (up-next).
//! Collecting leaves recursively survives container drift — the same reason
//! the TS layer funnels every surface through one item collector.

use crate::model::*;
use serde_json::Value;

/// `title.runs[].text` joined (skips ` • ` separators? No — keep them for
/// subtitle parsing by callers; this joins everything).
pub fn runs_text(v: &Value) -> String {
    v.get("runs")
        .and_then(|r| r.as_array())
        .map(|runs| {
            runs.iter().filter_map(|r| r.get("text")?.as_str()).collect::<Vec<_>>().join("")
        })
        .unwrap_or_default()
}

/// Non-separator run texts (` • ` and similar dropped).
pub fn run_parts(v: &Value) -> Vec<String> {
    v.get("runs")
        .and_then(|r| r.as_array())
        .map(|runs| {
            runs.iter()
                .filter_map(|r| r.get("text")?.as_str())
                .map(str::trim)
                .filter(|t| !t.is_empty() && *t != "•" && *t != "·")
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// Recursive search for the first `watchEndpoint.videoId` anywhere below `v`.
pub fn find_video_id(v: &Value) -> Option<String> {
    match v {
        Value::Object(map) => {
            if let Some(ep) = map.get("watchEndpoint") {
                if let Some(id) = ep.get("videoId").and_then(|x| x.as_str()) {
                    return Some(id.to_string());
                }
            }
            // playlistPanelVideoRenderer carries a top-level videoId too.
            if map.contains_key("videoId") {
                if let Some(id) = map.get("videoId").and_then(|x| x.as_str()) {
                    if id.len() == 11 {
                        return Some(id.to_string());
                    }
                }
            }
            map.values().find_map(find_video_id)
        }
        Value::Array(items) => items.iter().find_map(find_video_id),
        _ => None,
    }
}

/// The playlist *row* id: `playlistItemData.playlistSetVideoId` — the id of
/// the entry, not the song (a duplicate song has its own row).
pub fn find_set_video_id(v: &Value) -> Option<String> {
    v.get("playlistItemData")
        .and_then(|d| d.get("playlistSetVideoId"))
        .and_then(|x| x.as_str())
        .map(str::to_string)
}

/// The item's own `navigationEndpoint.browseEndpoint.browseId` — NOT a deep
/// search. Deep ids belong to runs (artist/album links inside a row) and
/// must not stand in for the row's own target.
pub fn nav_browse_id(item: &Value) -> Option<&str> {
    item
        .get("navigationEndpoint")
        .and_then(|e| e.get("browseEndpoint"))
        .and_then(|e| e.get("browseId"))
        .and_then(|b| b.as_str())
}

/// First 4-digit year in any run text below `v`.
pub fn find_year(v: &Value) -> Option<u16> {
    match v {
        Value::Object(map) => {
            if let Some(text) = map.get("text").and_then(|t| t.as_str()) {
                let t = text.trim();
                if t.len() == 4 {
                    if let Ok(y) = t.parse::<u16>() {
                        if (1000..=2100).contains(&y) {
                            return Some(y);
                        }
                    }
                }
            }
            map.values().find_map(find_year)
        }
        Value::Array(items) => items.iter().find_map(find_year),
        _ => None,
    }
}
/// Subscription state from `subscribeButtonRenderer.subscribed`
/// (`findArtistSubscriptionToggle` in the TS datasource).
pub fn find_subscribed(v: &Value) -> Option<bool> {
    match v {
        Value::Object(map) => {
            if let Some(btn) = map.get("subscribeButtonRenderer") {
                return btn.get("subscribed").and_then(|s| s.as_bool());
            }
            map.values().find_map(find_subscribed)
        }
        Value::Array(items) => items.iter().find_map(find_subscribed),
        _ => None,
    }
}

/// Recursive search for the first `browseEndpoint.browseId` below `v`.
/// When `prefix` is set, only ids starting with it match (e.g. `"MPRE"`).
pub fn find_browse_id(v: &Value, prefix: Option<&str>) -> Option<String> {
    match v {
        Value::Object(map) => {
            if let Some(ep) = map.get("browseEndpoint") {
                if let Some(id) = ep.get("browseId").and_then(|x| x.as_str()) {
                    if prefix.is_none_or(|p| id.starts_with(p)) {
                        return Some(id.to_string());
                    }
                }
            }
            map.values().find_map(|child| find_browse_id(child, prefix))
        }
        Value::Array(items) => items.iter().find_map(|c| find_browse_id(c, prefix)),
        _ => None,
    }
}

pub fn find_playlist_id(v: &Value) -> Option<String> {
    match v {
        Value::Object(map) => {
            if let Some(ep) = map.get("watchEndpoint") {
                if let Some(id) = ep.get("playlistId").and_then(|x| x.as_str()) {
                    return Some(id.to_string());
                }
            }
            if let Some(ep) = map.get("watchPlaylistEndpoint") {
                if let Some(id) = ep.get("playlistId").and_then(|x| x.as_str()) {
                    return Some(id.to_string());
                }
            }
            map.values().find_map(find_playlist_id)
        }
        Value::Array(items) => items.iter().find_map(find_playlist_id),
        _ => None,
    }
}

/// All `musicThumbnailRenderer.thumbnail.thumbnails[]` (and bare
/// `thumbnails[]` on panel videos) below `v`, deduped by URL.
pub fn collect_thumbs(v: &Value) -> Vec<YtThumb> {
    let mut out = Vec::new();
    collect_thumbs_into(v, &mut out);
    let mut seen = std::collections::HashSet::new();
    out.into_iter().filter(|t: &YtThumb| seen.insert(t.url.clone())).collect()
}

fn collect_thumbs_into(v: &Value, out: &mut Vec<YtThumb>) {
    match v {
        Value::Object(map) => {
            if let Some(t) = map.get("thumbnails").and_then(|t| t.as_array()) {
                for c in t {
                    if let Some(url) = c.get("url").and_then(|u| u.as_str()) {
                        if url.is_empty() {
                            continue;
                        }
                        out.push(YtThumb {
                            url: url.to_string(),
                            width: c.get("width").and_then(|w| w.as_u64()).unwrap_or(0) as u32,
                            height: c.get("height").and_then(|h| h.as_u64()).unwrap_or(0) as u32,
                        });
                    }
                }
            }
            for child in map.values() {
                collect_thumbs_into(child, out);
            }
        }
        Value::Array(items) => {
            for child in items {
                collect_thumbs_into(child, out);
            }
        }
        _ => {}
    }
}

/// Collect every `musicResponsiveListItemRenderer` below `v`.
pub fn collect_list_items<'a>(v: &'a Value, out: &mut Vec<&'a Value>) {
    match v {
        Value::Object(map) => {
            if let Some(item) = map.get("musicResponsiveListItemRenderer") {
                out.push(item);
            } else {
                for child in map.values() {
                    collect_list_items(child, out);
                }
            }
        }
        Value::Array(items) => {
            for child in items {
                collect_list_items(child, out);
            }
        }
        _ => {}
    }
}

/// Collect every `musicTwoRowItemRenderer` below `v`.
pub fn collect_tworows<'a>(v: &'a Value, out: &mut Vec<&'a Value>) {
    match v {
        Value::Object(map) => {
            if let Some(item) = map.get("musicTwoRowItemRenderer") {
                out.push(item);
            } else {
                for child in map.values() {
                    collect_tworows(child, out);
                }
            }
        }
        Value::Array(items) => {
            for child in items {
                collect_tworows(child, out);
            }
        }
        _ => {}
    }
}

/// Collect every `playlistPanelVideoRenderer` below `v`.
pub fn collect_panel_videos<'a>(v: &'a Value, out: &mut Vec<&'a Value>) {
    match v {
        Value::Object(map) => {
            if let Some(item) = map.get("playlistPanelVideoRenderer") {
                out.push(item);
            } else {
                for child in map.values() {
                    collect_panel_videos(child, out);
                }
            }
        }
        Value::Array(items) => {
            for child in items {
                collect_panel_videos(child, out);
            }
        }
        _ => {}
    }
}

/// `"3:57"` / `"1:02:03"` → seconds. Anything else (play counts, years) → None.
pub fn parse_duration(s: &str) -> Option<u32> {
    let s = s.trim();
    if s.is_empty() || !s.chars().any(|c| c == ':') {
        return None;
    }
    let mut total = 0u32;
    for part in s.split(':') {
        total = total.checked_mul(60)?.checked_add(part.trim().parse::<u32>().ok()?)?;
    }
    (total > 0 && total < 24 * 3600).then_some(total)
}

fn has_explicit_badge(v: &Value) -> bool {
    let s = serde_json::to_string(v).unwrap_or_default();
    s.contains("MUSIC_EXPLICIT_BADGE")
}

/// Artist runs: `{text, navigationEndpoint.browseEndpoint.browseId}` — names
/// plus UC channel ids. Separator/duration/views runs are skipped by shape:
/// keep runs that have a browseEndpoint OR (no endpoint AND not duration-like
/// AND not views/plays-like).
pub fn parse_artist_runs(runs_holder: &Value) -> Vec<YtArtistRef> {
    let Some(runs) = runs_holder.get("runs").and_then(|r| r.as_array()) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for run in runs {
        let Some(text) = run.get("text").and_then(|t| t.as_str()).map(str::trim) else {
            continue;
        };
        if text.is_empty() || text == "•" || text == "·" || text == "," || text == "&" {
            continue;
        }
        let browse = run
            .get("navigationEndpoint")
            .and_then(|e| e.get("browseEndpoint"))
            .and_then(|e| e.get("browseId"))
            .and_then(|b| b.as_str());
        if let Some(id) = browse {
            if id.starts_with("UC") {
                out.push(YtArtistRef { name: text.to_string(), channel_id: id.to_string() });
                continue;
            }
            // MPRE/VL runs are album/playlist links, not artists.
            if id.starts_with("MPRE") || id.starts_with("VL") {
                continue;
            }
        }
        // Bare runs: skip counts/durations/years/kinds.
        let lower = text.to_ascii_lowercase();
        if parse_duration(text).is_some()
            || lower.contains("view")
            || lower.contains("play")
            || lower.contains("subscriber")
            || lower.ends_with("followers")
            || text.parse::<u32>().is_ok_and(|y| (1000..=2100).contains(&y))
            || ["song", "video", "album", "single", "ep", "artist", "playlist", "explicit"]
                .contains(&lower.as_str())
        {
            continue;
        }
        out.push(YtArtistRef { name: text.to_string(), channel_id: String::new() });
    }
    out
}

fn album_ref_from_runs(runs_holder: &Value) -> Option<YtAlbumRef> {
    let runs = runs_holder.get("runs")?.as_array()?;
    for run in runs {
        let browse = run
            .get("navigationEndpoint")
            .and_then(|e| e.get("browseEndpoint"))
            .and_then(|e| e.get("browseId"))
            .and_then(|b| b.as_str())?;
        if browse.starts_with("MPRE") {
            return Some(YtAlbumRef {
                name: run.get("text")?.as_str()?.to_string(),
                browse_id: browse.to_string(),
            });
        }
    }
    None
}

/// A song row: `musicResponsiveListItemRenderer`.
///
/// Column layout (observed): flex[0] = title, flex[1] = artist • album •
/// views/plays, flex[2] (album pages) = play count, fixed[] = duration.
pub fn parse_list_track(item: &Value) -> Option<YtTrack> {
    let video_id = find_video_id(item)?;
    let flex = item.get("flexColumns")?.as_array()?;
    let title = flex.first().and_then(|c| c.get("musicResponsiveListItemFlexColumnRenderer")).map(|c| {
        c.get("text").map(runs_text).unwrap_or_default()
    }).unwrap_or_default();
    if title.is_empty() {
        return None;
    }
    let second = flex.get(1).and_then(|c| c.get("musicResponsiveListItemFlexColumnRenderer"));
    let artists = second.map(parse_artist_runs_from_col).unwrap_or_default();
    let album = second.and_then(album_ref_from_runs);
    // Duration: first duration-like run in fixedColumns, else flex columns.
    let mut duration_sec = 0;
    if let Some(cols) = item.get("fixedColumns").and_then(|c| c.as_array()) {
        for col in cols {
            let text = col
                .get("musicResponsiveListItemFixedColumnRenderer")
                .and_then(|c| c.get("text"))
                .map(runs_text)
                .unwrap_or_default();
            if let Some(d) = parse_duration(&text) {
                duration_sec = d;
                break;
            }
        }
    }
    if duration_sec == 0 {
        for col in flex.iter().skip(1) {
            let text = col
                .get("musicResponsiveListItemFlexColumnRenderer")
                .and_then(|c| c.get("text"))
                .map(runs_text)
                .unwrap_or_default();
            for part in text.split('•').map(str::trim) {
                if let Some(d) = parse_duration(part) {
                    duration_sec = d;
                    break;
                }
            }
            if duration_sec > 0 {
                break;
            }
        }
    }
    Some(YtTrack {
        video_id,
        title,
        artists,
        album,
        duration_sec,
        thumbnails: collect_thumbs(item),
        explicit: has_explicit_badge(item),
        playlist_id: find_playlist_id(item),
        set_video_id: find_set_video_id(item),
    })
}

fn parse_artist_runs_from_col(col: &Value) -> Vec<YtArtistRef> {
    col.get("text").map(parse_artist_runs).unwrap_or_default()
}

pub fn parse_panel_track(item: &Value) -> Option<YtTrack> {
    let video_id = find_video_id(item)?;
    let title = item.get("title").map(runs_text).unwrap_or_default();
    if title.is_empty() {
        return None;
    }
    let artists = item.get("longBylineText").map(parse_artist_runs).unwrap_or_default();
    let duration_sec = item
        .get("lengthText")
        .map(runs_text)
        .and_then(|t| parse_duration(&t))
        .unwrap_or(0);
    Some(YtTrack {
        video_id,
        title,
        artists,
        album: None,
        duration_sec,
        thumbnails: collect_thumbs(item),
        explicit: has_explicit_badge(item),
        playlist_id: find_playlist_id(item),
        set_video_id: find_set_video_id(item),
    })
}

pub enum TwoRowKind {
    Album,
    Playlist,
    Artist,
    Unknown,
}

/// Classify a `musicTwoRowItemRenderer` by its subtitle runs:
/// `Album • 2021`, `Single • 1992`, `Artist • 5.42M subscribers`,
/// `Playlist • 2026`.
pub fn classify_tworow(item: &Value) -> (TwoRowKind, String, Option<u16>) {
    let parts = item.get("subtitle").map(run_parts).unwrap_or_default();
    let mut kind = TwoRowKind::Unknown;
    let mut year = None;
    for part in &parts {
        let lower = part.to_ascii_lowercase();
        if ["album", "single", "ep"].contains(&lower.as_str()) {
            kind = TwoRowKind::Album;
        } else if lower == "playlist" {
            kind = TwoRowKind::Playlist;
        } else if lower == "artist" || lower.contains("subscriber") {
            kind = TwoRowKind::Artist;
        }
        if let Ok(y) = part.parse::<u16>() {
            if (1000..=2100).contains(&y) {
                year = Some(y);
            }
        }
    }
    // Fallback: id prefix decides when the subtitle is bare (e.g. `2021`).
    if matches!(kind, TwoRowKind::Unknown) {
        let id = item
            .get("navigationEndpoint")
            .and_then(|e| e.get("browseEndpoint"))
            .and_then(|e| e.get("browseId"))
            .and_then(|b| b.as_str())
            .unwrap_or("");
        if id.starts_with("MPRE") {
            kind = TwoRowKind::Album;
        } else if id.starts_with("UC") {
            kind = TwoRowKind::Artist;
        } else if id.starts_with("VL") || find_playlist_id(item).is_some() {
            kind = TwoRowKind::Playlist;
        }
    }
    let kind_label = match kind {
        TwoRowKind::Album => {
            let lower = parts.iter().map(|p| p.to_ascii_lowercase()).collect::<Vec<_>>();
            if lower.iter().any(|p| p == "single") {
                "Single".to_string()
            } else if lower.iter().any(|p| p == "ep") {
                "EP".to_string()
            } else {
                "Album".to_string()
            }
        }
        TwoRowKind::Playlist => "Playlist".to_string(),
        TwoRowKind::Artist => "Artist".to_string(),
        TwoRowKind::Unknown => String::new(),
    };
    (kind, kind_label, year)
}

pub fn parse_tworow_album(item: &Value) -> Option<YtAlbumRefFull> {
    let title = item.get("title").map(runs_text).unwrap_or_default();
    if title.is_empty() {
        return None;
    }
    // The row's own target — never a run's artist/album link.
    let browse_id = match nav_browse_id(item) {
        Some(id) if id.starts_with("MPRE") => id.to_string(),
        _ => return None,
    };
    let (_, kind, year) = classify_tworow(item);
    Some(YtAlbumRefFull { browse_id, title, year, kind, thumbnails: collect_thumbs(item) })
}

pub fn parse_tworow_artist(item: &Value) -> Option<YtArtistHeader> {
    let title = item.get("title").map(runs_text).unwrap_or_default();
    if title.is_empty() {
        return None;
    }
    let channel_id = match nav_browse_id(item) {
        Some(id) if id.starts_with("UC") => id.to_string(),
        _ => find_browse_id(item, Some("UC"))?,
    };
    Some(YtArtistHeader { channel_id, name: title, thumbnails: collect_thumbs(item) })
}

pub fn parse_tworow_playlist(item: &Value) -> Option<YtPlaylistHeader> {
    let title = item.get("title").map(runs_text).unwrap_or_default();
    if title.is_empty() {
        return None;
    }
    // Prefer the row's own VL browse id over menu action playlistIds
    // ("Shuffle play" endpoints carry unprefixed ids for the same list).
    let id = find_browse_id(item, Some("VL")).or_else(|| find_playlist_id(item))?;
    Some(YtPlaylistHeader { id, title, thumbnails: collect_thumbs(item) })
}

/// A videoId-less list row: album, playlist or artist/channel. Unsigned
/// search renders most non-song results this way (over half the rows in the
/// captured fixture), so skipping them drops albums, playlists and artists
/// alike. `None` when the row resolves to nothing (not an error — some rows
/// are pure navigation).
pub fn parse_list_non_track(item: &Value) -> Option<ListNonTrack> {
    if find_video_id(item).is_some() {
        return None;
    }
    let title = item
        .get("flexColumns")
        .and_then(|c| c.as_array())
        .and_then(|cols| cols.first())
        .and_then(|c| c.get("musicResponsiveListItemFlexColumnRenderer"))
        .and_then(|c| c.get("text"))
        .map(runs_text)
        .unwrap_or_default();
    if title.is_empty() {
        return None;
    }
    let thumbs = collect_thumbs(item);
    // Album rows link the album (MPRE) from a run alongside the artist.
    if let Some(id) = find_browse_id_no_menu(item, "MPRE") {
        let (_, kind, year) = classify_tworow(item);
        let kind = if kind.is_empty() { "Album".to_string() } else { kind };
        return Some(ListNonTrack::Album(YtAlbumRefFull {
            browse_id: id,
            title,
            year: year.or_else(|| find_year(item)),
            kind,
            thumbnails: thumbs,
        }));
    }
    // Playlist rows navigate to VL… (item-level or run-level).
    if let Some(id) = nav_browse_id(item)
        .filter(|id| id.starts_with("VL"))
        .map(str::to_string)
        .or_else(|| find_playlist_id_no_menu(item))
        .or_else(|| find_browse_id_no_menu(item, "VL"))
    {
        return Some(ListNonTrack::Playlist(YtPlaylistHeader { id, title, thumbnails: thumbs }));
    }
    // Artist/channel rows: UC target, no MPRE/VL anywhere.
    if let Some(id) = nav_browse_id(item)
        .filter(|id| id.starts_with("UC"))
        .map(str::to_string)
        .or_else(|| find_browse_id_no_menu(item, "UC"))
    {
        return Some(ListNonTrack::Artist(YtArtistHeader {
            channel_id: id,
            name: title,
            thumbnails: thumbs,
        }));
    }
    None
}

/// Deep id search that skips `menu` subtrees. Menus carry "Start mix" /
/// "Go to album" endpoints whose ids belong to actions, not to the row —
/// searching them misclassifies artist rows as playlists.
fn find_browse_id_no_menu(v: &Value, prefix: &str) -> Option<String> {
    match v {
        Value::Object(map) => {
            if let Some(ep) = map.get("browseEndpoint") {
                if let Some(id) = ep.get("browseId").and_then(|x| x.as_str()) {
                    if id.starts_with(prefix) {
                        return Some(id.to_string());
                    }
                }
            }
            map.iter()
                .filter(|(k, _)| *k != "menu")
                .find_map(|(_, child)| find_browse_id_no_menu(child, prefix))
        }
        Value::Array(items) => items.iter().find_map(|c| find_browse_id_no_menu(c, prefix)),
        _ => None,
    }
}

fn find_playlist_id_no_menu(v: &Value) -> Option<String> {
    match v {
        Value::Object(map) => {
            if let Some(ep) = map.get("watchEndpoint") {
                if let Some(id) = ep.get("playlistId").and_then(|x| x.as_str()) {
                    return Some(id.to_string());
                }
            }
            if let Some(ep) = map.get("watchPlaylistEndpoint") {
                if let Some(id) = ep.get("playlistId").and_then(|x| x.as_str()) {
                    return Some(id.to_string());
                }
            }
            map.iter()
                .filter(|(k, _)| *k != "menu")
                .find_map(|(_, child)| find_playlist_id_no_menu(child))
        }
        Value::Array(items) => items.iter().find_map(find_playlist_id_no_menu),
        _ => None,
    }
}

pub enum ListNonTrack {
    Album(YtAlbumRefFull),
    Playlist(YtPlaylistHeader),
    Artist(YtArtistHeader),
}

/// An artist/channel list row (charts' "Top artists", search profiles).
/// `None` for song rows — those carry a videoId and belong to
/// [`parse_list_track`].
pub fn parse_list_artist(item: &Value) -> Option<YtArtistHeader> {
    match parse_list_non_track(item)? {
        ListNonTrack::Artist(a) => Some(a),
        _ => None,
    }
}

/// Shelf title for grouping (`musicShelfRenderer.title` / carousel header).
pub fn shelf_title(shelf: &Value) -> String {
    shelf
        .get("title")
        .map(runs_text)
        .filter(|t| !t.is_empty())
        .or_else(|| {
            shelf
                .get("header")
                .and_then(|h| h.get("musicCarouselShelfBasicHeaderRenderer"))
                .and_then(|h| h.get("title"))
                .map(runs_text)
        })
        .unwrap_or_default()
}

pub fn collect_shelves<'a>(v: &'a Value, out: &mut Vec<&'a Value>) {
    match v {
        Value::Object(map) => {
            if map.contains_key("musicShelfRenderer") {
                out.push(&map["musicShelfRenderer"]);
            } else if map.contains_key("musicCarouselShelfRenderer") {
                out.push(&map["musicCarouselShelfRenderer"]);
            } else {
                for child in map.values() {
                    collect_shelves(child, out);
                }
            }
        }
        Value::Array(items) => {
            for child in items {
                collect_shelves(child, out);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations() {
        assert_eq!(parse_duration("3:57"), Some(237));
        assert_eq!(parse_duration("1:02:03"), Some(3723));
        assert_eq!(parse_duration("2.3B plays"), None);
        assert_eq!(parse_duration("1992"), None);
        assert_eq!(parse_duration(""), None);
    }
}
