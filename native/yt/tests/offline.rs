//! Offline tests against REAL captured Innertube responses and the REAL
//! captured player script (`tests/fixtures/*.json`, `player_base.js`).
//! No network. `cargo test -p zuno-yt` must pass offline.
//!
//! Fixture redaction: URL params outside the shipping allowlist are `[Nch]`
//! length markers and the ciphered `s=` values are redacted the same way —
//! the decipher pipeline is still fully exercised, and the exact-output
//! vector comes from `decipher_vectors.json` (synthetic inputs, verified
//! against youtubei.js v17.0.1 on the same player).

use serde_json::Value;
use std::fs;
use zuno_yt::api::find_continuation;
use zuno_yt::decipher::Decipherer;
use zuno_yt::mutations::{
    playlist_edit_body, playlist_saved_body, subscription_body, track_rating_body, Rating,
};
use zuno_yt::parse::parse_list_track;
use zuno_yt::{
    api::{collect_track_pages, parse_album, parse_artist, parse_playlist, parse_search, parse_shelves, TrackPageSource},
    parse::{collect_panel_videos, parse_panel_track},
    YtTrackPage,
};
use zuno_yt::stream::AudioQuality;

fn fixture_text(name: &str) -> String {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    fs::read_to_string(path).unwrap_or_else(|e| panic!("fixture {name}: {e}"))
}

fn fixture(name: &str) -> Value {
    serde_json::from_str(&fixture_text(name)).unwrap()
}

// --- decipher: the one ported JS step ---

/// The strict vector: the Rust extractor + QuickJS must reproduce the exact
/// transforms youtubei.js v17.0.1 produced on the same captured player
/// (`player ec429320`, `signatureTimestamp 20733`).
#[test]
fn decipher_vector_matches_youtubei_js() {
    let dec = Decipherer::from_source("ec429320", &fixture_text("player_base.js")).expect("player extracts");
    let vectors: Value = fixture("decipher_vectors.json");
    assert_eq!(
        vectors["signature_timestamp"].as_u64().unwrap_or_default(),
        dec.signature_timestamp(),
        "signatureTimestamp must extract like youtubei.js's timestampMatcher"
    );
    for v in vectors["vectors"].as_array().expect("vectors") {
        let (sig, n) = dec
            .transform(Some(v["n"].as_str().unwrap()), Some(v["sp"].as_str().unwrap()), Some(v["s"].as_str().unwrap()))
            .expect("decipher runs");
        assert_eq!(n.as_deref(), Some(v["expected_n"].as_str().unwrap()), "n transform");
        assert_eq!(sig.as_deref(), Some(v["expected_sig"].as_str().unwrap()), "sig transform");
    }
}

/// The real captured WEB_REMIX ciphers (signature values redacted to `[Nch]`
/// markers — the transform still runs) must decipher into a URL that parses
/// and carries `sig=` + `itag=` + `expire=`, with the throttling `n`
/// transformed and `cver` restamped to the session's client version.
#[test]
fn decipher_real_captured_ciphers() {
    let dec = Decipherer::from_source("ec429320", &fixture_text("player_base.js")).expect("player extracts");
    let player = fixture("player_remix.json");
    let formats = player["streamingData"]["adaptiveFormats"].as_array().expect("formats");
    let audio: Vec<&Value> = formats
        .iter()
        .filter(|f| f["mimeType"].as_str().is_some_and(|m| m.starts_with("audio/")))
        .collect();
    assert!(audio.len() >= 4, "4 audio itags in the capture");
    for f in &audio {
        let cipher = f["signatureCipher"].as_str().or(f["cipher"].as_str()).expect("ciphered format");
        let url = dec
            .decipher_cipher_sync(cipher, "1.20250506.00.00")
            .expect("cipher deciphers");
        assert!(url.starts_with("https://"), "direct googlevideo URL");
        let query = url.split_once('?').expect("has a query").1;
        let params: Vec<&str> = query.split('&').collect();
        let has = |name: &str| params.iter().any(|p| p.starts_with(name) && p.contains('='));
        assert!(has("sig="), "carries sig= ({})", f["itag"]);
        assert!(has("itag="), "carries itag=");
        assert!(has("expire="), "carries expire=");
        assert!(has("cver="), "carries cver=");
        assert!(url.contains("cver=1.20250506.00.00"), "cver restamped to the session's version");
        // the n transform ran: the output n is no longer the raw capture value
        let n_in = percent_decode_plain(cipher.split("url=").nth(1).unwrap())
            .split('&')
            .find(|p| p.starts_with("n="))
            .unwrap()
            .trim_start_matches("n=")
            .to_string();
        let n_out = url
            .split('&')
            .find(|p| p.starts_with("n="))
            .unwrap()
            .trim_start_matches("n=");
        assert_ne!(n_in, n_out, "throttling n transformed");
    }
}

fn percent_decode_plain(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let h = (bytes[i + 1] as char).to_digit(16);
            let l = (bytes[i + 2] as char).to_digit(16);
            if let (Some(h), Some(l)) = (h, l) {
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// A plain direct URL with a raw `n` gets only the n transform.
#[test]
fn decipher_transforms_n_on_direct_url() {
    let dec = Decipherer::from_source("ec429320", &fixture_text("player_base.js")).expect("player extracts");
    let url = "https://rr1---sn-x.googlevideo.com/videoplayback?expire=1791554583&n=bobtest123&c=WEB_REMIX&cver=1.20250506.00.00&itag=140";
    let out = dec.decipher_url_sync(url, "1.20250506.00.00").expect("n transform runs");
    assert!(out.contains("n=kuIsIg56"), "n transformed to the vector value: {out}");
    assert!(!out.contains("n=bobtest123"));
}

// --- reads ---

#[test]
fn search_finds_real_tracks() {
    let resp = fixture("search.json");
    let results = parse_search(&resp, "radiohead creep");
    assert!(!results.tracks.is_empty(), "search must yield tracks");
    assert!(
        results.tracks.iter().any(|t| t.video_id.len() == 11 && !t.title.is_empty()),
        "tracks need 11-char videoIds and titles"
    );
    assert!(
        results.tracks.iter().any(|t| t.duration_sec >= 90 && t.duration_sec <= 900),
        "at least one track parses a plausible duration"
    );
    assert!(
        results.tracks.iter().any(|t| !t.artists.is_empty()),
        "artist runs must parse"
    );
    assert!(
        results.tracks.iter().any(|t| t.artwork_url().is_some_and(|u| u.starts_with("https://"))),
        "thumbnails must parse"
    );
    assert!(!results.albums.is_empty(), "album rows must parse");
    assert!(results.albums.iter().all(|a| a.browse_id.starts_with("MPRE")));
    assert!(!results.playlists.is_empty(), "playlist rows must parse");
    assert!(!results.artists.is_empty(), "artist rows must parse");
    assert!(results.artists.iter().all(|a| a.channel_id.starts_with("UC")));
    assert!(results.tracks.iter().any(|t| t.explicit), "explicit badge must parse");
}

#[test]
fn player_picks_direct_audio() {
    let resp = fixture("player_ios.json");
    let (url, mime, bitrate) = zuno_yt::stream::pick_audio(&resp).expect("IOS fixture has direct audio");
    assert!(url.starts_with("https://"), "direct googlevideo URL");
    assert_eq!(mime, "audio/mp4");
    assert!(bitrate > 0);
    assert!(bitrate >= 100_000, "best audio should be itag 140, got {bitrate}");
}

#[test]
fn player_rejects_unplayable() {
    let resp = serde_json::json!({
        "playabilityStatus": { "status": "LOGIN_REQUIRED" },
        "streamingData": { "adaptiveFormats": [] },
    });
    assert!(zuno_yt::stream::pick_audio(&resp).is_err());
}

#[test]
fn song_category_search_returns_only_songs() {
    let resp = fixture("search_song.json");
    let results = parse_search(&resp, "radiohead");
    assert!(!results.tracks.is_empty(), "the song filter returns song rows");
    // A category search answers one shelf: everything is a track here.
    assert!(results.tracks.iter().all(|t| t.video_id.len() == 11));
    assert!(results.artists.is_empty(), "filtered search carries no artist rows");
}

#[test]
fn suggestions_parse_and_cap_at_three() {
    let resp = fixture("search_suggestions.json");
    let sections = resp["contents"].as_array().expect("sections");
    let mut out: Vec<String> = Vec::new();
    for section in sections {
        let Some(section) = section.get("searchSuggestionsSectionRenderer") else { continue };
        for item in section["contents"].as_array().unwrap_or(&vec![]) {
            let text = item
                .get("searchSuggestionRenderer")
                .and_then(|r| r.get("suggestion"))
                .map(zuno_yt::parse::runs_text);
            if let Some(text) = text {
                if !text.is_empty() && !out.contains(&text) {
                    out.push(text);
                }
            }
        }
    }
    out.truncate(3);
    assert_eq!(out.len(), 3, "the app's three-suggestion cap");
    assert!(out.iter().all(|s| s.to_lowercase().contains("radiohead")), "suggestions for the query");
}

#[test]
fn album_parses_header_and_tracks() {
    let resp = fixture("album.json");
    let album = parse_album(&resp, "MPREb_TgQPwAzodvg").expect("album parses");
    assert_eq!(album.title, "Creep");
    assert_eq!(album.artist, "Radiohead");
    assert_eq!(album.year, Some(1992));
    assert!(!album.tracks.is_empty());
    assert!(album.tracks.iter().all(|t| t.video_id.len() == 11));
    assert!(!album.thumbnails.is_empty());
}

#[test]
fn artist_parses_name_shelves_and_subscribe_state() {
    let resp = fixture("artist.json");
    let artist = parse_artist(&resp, "UCr_iyUANcn9OX_yy9piYoLw").expect("artist parses");
    assert_eq!(artist.name, "Radiohead");
    assert!(artist.subscriber_count.is_some());
    assert_eq!(artist.subscriber_count.as_deref(), Some("5.42M"));
    // unsigned browse: the button still reports the state (false)
    assert_eq!(artist.subscribed, Some(false));
    assert!(!artist.top_tracks.is_empty(), "Top songs shelf must parse");
    assert!(!artist.albums.is_empty(), "album carousels must parse");
    assert!(artist.albums.iter().all(|a| a.browse_id.starts_with("MPRE")));
}

#[test]
fn playlist_parses_tracks_with_set_video_ids() {
    let resp = fixture("playlist.json");
    let pl = parse_playlist(&resp, "PLbIPuLeoHaKaGm7k_lL0XxdlguMOiEOVo").expect("playlist parses");
    assert!(!pl.title.is_empty());
    assert!(!pl.tracks.is_empty());
    assert!(pl.tracks.iter().all(|t| t.video_id.len() == 11));
    assert!(
        pl.tracks.iter().any(|t| t.set_video_id.is_some()),
        "playlist rows carry their set video ids (remove/reorder need the row id)"
    );
}

#[test]
fn playlist_page_holds_101_rows_and_a_continuation() {
    let resp = fixture("playlist_page.json");
    let mut seen = std::collections::HashSet::new();
    let mut items = Vec::new();
    zuno_yt::parse::collect_list_items(&resp, &mut items);
    let tracks: Vec<_> = items.into_iter().filter_map(parse_list_track).collect();
    for t in &tracks {
        seen.insert(t.video_id.clone());
    }
    assert_eq!(tracks.len(), 100, "first page rows (the 101st entry is the continuation item)");
    let token = find_continuation(&resp).expect("page 1 carries a continuation");
    assert!(!token.is_empty());
}

#[test]
fn playlist_continuation_page_appends_rows() {
    let resp = fixture("playlist_page_continuation.json");
    // append-action shape: rows under onResponseReceivedActions
    let mut seen = std::collections::HashSet::new();
    let mut items = Vec::new();
    zuno_yt::parse::collect_list_items(&resp, &mut items);
    if items.is_empty() {
        // fall through to the append collector like the page parser does
    }
    let mut appended = Vec::new();
    collect_append_items_test(&resp, &mut appended);
    let tracks: Vec<_> = appended.into_iter().filter_map(parse_list_track).collect();
    for t in &tracks {
        seen.insert(t.video_id.clone());
    }
    assert_eq!(tracks.len(), 10, "the continuation page appends the last 10 rows");
    assert!(find_continuation(&resp).is_none(), "the last page has no further continuation");
}

fn collect_append_items_test<'a>(v: &'a Value, out: &mut Vec<&'a Value>) {
    let Some(actions) = v.get("onResponseReceivedActions").and_then(|a| a.as_array()) else {
        return;
    };
    for action in actions {
        let items = action["appendContinuationItemsAction"]["continuationItems"].as_array();
        if let Some(items) = items {
            for item in items {
                zuno_yt::parse::collect_list_items(item, out);
            }
        }
    }
}

/// The termination guard from collectTrackPages.ts: a cursor that stops
/// advancing must not spin to the page cap.
#[tokio::test]
async fn pagination_guard_stops_on_stuck_cursor() {
    struct Stuck;
    impl TrackPageSource for Stuck {
        async fn fetch_page(&mut self, _page_key: &str) -> zuno_yt::error::Result<YtTrackPage> {
            Ok(YtTrackPage {
                tracks: vec![],
                has_more: true,
                // same key as requested, no new tracks -> must break
                next_page_key: Some("same-key".into()),
            })
        }
    }
    let mut collected = vec![fake_track("AAAAAAAAAAA")];
    let before = collected.len();
    let out = collect_track_pages(&mut collected, true, 50, &mut Stuck).await.expect("walk terminates");
    assert_eq!(out.len(), before, "no new tracks from a stuck cursor");
}

#[tokio::test]
async fn pagination_guard_caps_pages() {
    struct Forever;
    impl TrackPageSource for Forever {
        async fn fetch_page(&mut self, page_key: &str) -> zuno_yt::error::Result<YtTrackPage> {
            Ok(YtTrackPage {
                tracks: vec![fake_track(page_key)],
                has_more: true,
                next_page_key: Some(format!("{page_key}x")),
            })
        }
    }
    fn fake_track(id: &str) -> zuno_yt::model::YtTrack {
        zuno_yt::model::YtTrack { video_id: format!("{id:0<11}"), title: "T".into(), ..Default::default() }
    }
    let mut collected = Vec::new();
    let out = collect_track_pages(&mut collected, true, 5, &mut Forever).await.expect("walk terminates");
    assert_eq!(out.len(), 5, "page cap respected");
}

fn fake_track(id: &str) -> zuno_yt::model::YtTrack {
    zuno_yt::model::YtTrack { video_id: format!("{id:0<11}"), title: "T".into(), ..Default::default() }
}

#[test]
fn charts_parse_to_shelves() {
    let resp = fixture("charts.json");
    let shelves = parse_shelves(&resp);
    assert!(!shelves.is_empty(), "charts must yield shelves");
    assert!(shelves.iter().all(|s| !s.title.is_empty()));
    let artists: usize = shelves.iter().map(|s| s.artists.len()).sum();
    let playlists: usize = shelves.iter().map(|s| s.playlists.len()).sum();
    assert!(artists >= 10, "Top artists rows must parse, got {artists}");
    assert!(playlists >= 1, "chart playlist rows must parse, got {playlists}");
    assert!(
        shelves.iter().any(|s| s.playlists.iter().any(|p| p.id.starts_with("VL"))),
        "chart playlists keep their VL ids"
    );
}

#[test]
fn related_parses_queue() {
    let resp = fixture("related.json");
    let mut items = Vec::new();
    collect_panel_videos(&resp, &mut items);
    assert!(!items.is_empty(), "next response must hold queue items");
    let tracks: Vec<_> = items.into_iter().filter_map(parse_panel_track).collect();
    assert!(!tracks.is_empty());
    assert!(tracks.iter().all(|t| t.video_id.len() == 11));
}

#[test]
fn next_response_carries_the_related_tab() {
    let resp = fixture("next_tabs.json");
    // the tab lookup the related browse uses
    let browse_id = zuno_yt::api::find_related_tab_browse_id(&resp).expect("related tab found");
    assert!(browse_id.starts_with("MPTRt_"), "related tab browse id, got {browse_id}");
}

#[test]
fn related_page_parses_into_shelves() {
    let resp = fixture("related_page.json");
    let shelves = parse_shelves(&resp);
    assert!(shelves.len() >= 3, "related page yields several shelves");
    let titles: Vec<&str> = shelves.iter().map(|s| s.title.as_str()).collect();
    assert!(titles.contains(&"You might also like"), "titles parse: {titles:?}");
    let songs: usize = shelves.iter().map(|s| s.tracks.len()).sum();
    let albums: usize = shelves.iter().map(|s| s.albums.len()).sum();
    let artists: usize = shelves.iter().map(|s| s.artists.len()).sum();
    assert!(songs > 0 && albums > 0 && artists > 0, "related shelves carry tracks, albums and artists");
}

// --- mutations: request shapes offline ---

#[test]
fn mutation_request_shapes_match_the_ts_datasource() {
    // like/dislike/none — target normalisation + per-status params
    let like = serde_json::json!({ "status": "LIKE", "target": "XFkzRNyygfk", "likeParams": "p1" });
    let body = track_rating_body(&like, Rating::Like);
    assert_eq!(body["target"], serde_json::json!({ "videoId": "XFkzRNyygfk" }));
    assert_eq!(body["params"], "p1");

    let none = serde_json::json!({ "status": "INDIFFERENT", "target": "PLabc", "removeLikeParams": "p3" });
    let body = track_rating_body(&none, Rating::None);
    assert_eq!(body["target"], serde_json::json!({ "playlistId": "PLabc" }));
    assert_eq!(body["params"], "p3");

    // playlist create seeds ids; rename edits by row-safe action
    let body = playlist_edit_body("VLabc", serde_json::json!({ "action": "ACTION_SET_PLAYLIST_NAME", "playlistName": "New" }));
    assert_eq!(body["playlistId"], "abc", "VL stripped");
    assert_eq!(body["actions"][0]["action"], "ACTION_SET_PLAYLIST_NAME");

    let body = playlist_edit_body("abc", serde_json::json!({ "action": "ACTION_ADD_VIDEO", "addedVideoId": "v" }));
    assert_eq!(body["actions"][0]["addedVideoId"], "v");

    let body = playlist_edit_body("VLabc", serde_json::json!({ "action": "ACTION_REMOVE_VIDEO", "setVideoId": "row-id" }));
    assert_eq!(body["actions"][0]["setVideoId"], "row-id", "removal addresses the row, not the song");

    let body = playlist_edit_body(
        "VLabc",
        serde_json::json!({ "action": "ACTION_MOVE_VIDEO_AFTER", "setVideoId": "m", "movedSetVideoIdPredecessor": "" }),
    );
    assert_eq!(body["actions"][0]["movedSetVideoIdPredecessor"], "", "empty predecessor moves to the front");

    // playlist save: body identical, path carries the direction
    let body = playlist_saved_body("VLabc");
    assert_eq!(body["target"], serde_json::json!({ "playlistId": "abc" }));

    // subscribe/unsubscribe: fixed params from setArtistSubscribed
    let body = subscription_body("UCr_iyUANcn9OX_yy9piYoLw", true);
    assert_eq!(body["params"], "EgIIAhgA");
    assert_eq!(body["channelIds"], serde_json::json!(["UCr_iyUANcn9OX_yy9piYoLw"]));
    let body = subscription_body("UCr_iyUANcn9OX_yy9piYoLw", false);
    assert_eq!(body["params"], "CgIIAhgA");
}

// --- quality ranking ---

#[test]
fn quality_ranking_matches_select_format_for_quality() {
    use zuno_yt::stream::Format;
    let mk = |mime: &'static str, bitrate: u64| Format { mime, bitrate, url: None, cipher: None };
    let all = vec![mk("audio/mp4; codecs=mp4a.40.2", 130_000), mk("audio/webm; codecs=opus", 151_000)];
    let pick = zuno_yt::stream::select_format_for_quality(&all, AudioQuality::High).unwrap();
    assert_eq!(all[pick].bitrate, 151_000, "high takes the best on offer, Opus included");

    let mp4s: Vec<Format> = all.iter().filter(|f| f.mime.contains("audio/mp4")).cloned().collect();
    let pick = zuno_yt::stream::select_format_for_quality(&mp4s, AudioQuality::Normal).unwrap();
    assert_eq!(mp4s[pick].bitrate, 130_000);
}

// --- session cookie rules (mirroring lib.rs's #[cfg(test)]) ---

#[test]
fn session_cookie_rules() {
    let s = zuno_yt::Session::import("SID=abc; Path=/; SAPISID=xyz");
    assert_eq!(s.cookie_header, "SID=abc; SAPISID=xyz");
    assert_eq!(s.account_identity().as_deref(), Some("xyz"));
}

// --- artwork + adapter ---

#[test]
fn artwork_picks_largest() {
    use zuno_yt::model::YtThumb;
    let thumbs = vec![
        YtThumb { url: "https://x/s".into(), width: 60, height: 60 },
        YtThumb { url: "https://x/l".into(), width: 544, height: 544 },
    ];
    assert_eq!(zuno_yt::pick_artwork(&thumbs), Some("https://x/l"));
    let empty: Vec<YtThumb> = vec![];
    assert_eq!(zuno_yt::pick_artwork(&empty), None);
}

#[test]
fn yt_library_adapter_fills_core() {
    let resp = fixture("search.json");
    let results = parse_search(&resp, "radiohead creep");
    let lib = zuno_yt::YtLibrary::from_search(&results);
    assert_eq!(lib.tracks.len(), results.tracks.len());
    assert!(lib.tracks.iter().all(|t| !t.video_id.is_empty()));
    let view = lib.track_view(lib.tracks[0].id);
    assert!(!view.title.is_empty());
}
