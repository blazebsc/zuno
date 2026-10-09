//! Offline parser tests against REAL captured Innertube responses
//! (`tests/fixtures/*.json`, signatures redacted to `[Nch]` markers).
//! No network. `cargo test -p zuno-yt` must pass offline.

use serde_json::Value;
use std::fs;
use zuno_yt::api::{parse_album, parse_artist, parse_playlist, parse_search, parse_shelves};
use zuno_yt::parse::TwoRowKind;
use zuno_yt::stream::pick_audio;

fn fixture(name: &str) -> Value {
    let path = format!("{}/tests/fixtures/{name}.json", env!("CARGO_MANIFEST_DIR"));
    serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
}

#[test]
fn search_finds_real_tracks() {
    let resp = fixture("search");
    let results = parse_search(&resp, "radiohead creep");
    assert!(!results.tracks.is_empty(), "search must yield tracks");
    // The OMV top hit carries the video id in the title run's endpoint.
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
    // Non-song rows: this unsigned capture renders albums/playlists/artists
    // as videoId-less list items — over half the rows.
    assert!(!results.albums.is_empty(), "album rows must parse");
    assert!(results.albums.iter().all(|a| a.browse_id.starts_with("MPRE")));
    assert!(!results.playlists.is_empty(), "playlist rows must parse");
    assert!(!results.artists.is_empty(), "artist rows must parse");
    assert!(results.artists.iter().all(|a| a.channel_id.starts_with("UC")));
    // Explicit badge present in this capture (2 badged rows).
    assert!(results.tracks.iter().any(|t| t.explicit), "explicit badge must parse");
}

#[test]
fn player_picks_direct_audio() {
    let resp = fixture("player_ios");
    let (url, mime, bitrate) = pick_audio(&resp).expect("IOS fixture has direct audio");
    assert!(url.starts_with("https://"), "direct googlevideo URL");
    assert_eq!(mime, "audio/mp4");
    assert!(bitrate > 0);
    // itag 140 (128k) outranks 139 (48k).
    assert!(bitrate >= 100_000, "best audio should be itag 140, got {bitrate}");
}

#[test]
fn player_rejects_unplayable() {
    let resp = serde_json::json!({
        "playabilityStatus": { "status": "LOGIN_REQUIRED" },
        "streamingData": { "adaptiveFormats": [] },
    });
    assert!(pick_audio(&resp).is_err());
}

#[test]
fn album_parses_header_and_tracks() {
    let resp = fixture("album");
    let album = parse_album(&resp, "MPREb_TgQPwAzodvg").expect("album parses");
    assert_eq!(album.title, "Creep");
    assert_eq!(album.artist, "Radiohead");
    assert_eq!(album.year, Some(1992));
    assert!(!album.tracks.is_empty());
    assert!(album.tracks.iter().all(|t| t.video_id.len() == 11));
    assert!(!album.thumbnails.is_empty());
}

#[test]
fn artist_parses_name_and_shelves() {
    let resp = fixture("artist");
    let artist = parse_artist(&resp, "UCr_iyUANcn9OX_yy9piYoLw").expect("artist parses");
    assert_eq!(artist.name, "Radiohead");
    assert!(artist.subscriber_count.is_some());
    assert!(!artist.top_tracks.is_empty(), "Top songs shelf must parse");
    assert!(!artist.albums.is_empty(), "album carousels must parse");
    assert!(artist.albums.iter().all(|a| a.browse_id.starts_with("MPRE")));
}

#[test]
fn playlist_parses_tracks() {
    let resp = fixture("playlist");
    let pl = parse_playlist(&resp, "PLbIPuLeoHaKaGm7k_lL0XxdlguMOiEOVo").expect("playlist parses");
    assert!(!pl.title.is_empty());
    assert!(!pl.tracks.is_empty());
    assert!(pl.tracks.iter().all(|t| t.video_id.len() == 11));
}

#[test]
fn charts_parse_to_shelves() {
    let resp = fixture("charts");
    let shelves = parse_shelves(&resp);
    assert!(!shelves.is_empty(), "charts must yield shelves");
    assert!(shelves.iter().all(|s| !s.title.is_empty()));
    // The unsigned charts feed holds chart playlists + artist rows, no song
    // rows — assert what it actually carries, not songs.
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
    use zuno_yt::parse::{collect_panel_videos, parse_panel_track};
    let resp = fixture("related");
    let mut items = Vec::new();
    collect_panel_videos(&resp, &mut items);
    assert!(!items.is_empty(), "next response must hold queue items");
    let tracks: Vec<_> = items.into_iter().filter_map(parse_panel_track).collect();
    assert!(!tracks.is_empty());
    assert!(tracks.iter().all(|t| t.video_id.len() == 11));
}

#[test]
fn tworow_classification_covers_kinds() {
    use zuno_yt::parse::{classify_tworow, collect_tworows};
    let resp = fixture("artist");
    let mut rows = Vec::new();
    collect_tworows(&resp, &mut rows);
    assert!(!rows.is_empty());
    let mut saw_album = false;
    for row in rows {
        if matches!(classify_tworow(row).0, TwoRowKind::Album) {
            saw_album = true;
        }
    }
    assert!(saw_album, "artist page must classify album rows");
}

#[test]
fn session_cookie_rules() {
    let s = zuno_yt::Session::import("SID=abc; Path=/; SAPISID=xyz");
    assert_eq!(s.cookie_header, "SID=abc; SAPISID=xyz");
    assert_eq!(s.account_identity().as_deref(), Some("xyz"));
}

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
    let resp = fixture("search");
    let results = parse_search(&resp, "radiohead creep");
    let lib = zuno_yt::YtLibrary::from_search(&results);
    assert_eq!(lib.tracks.len(), results.tracks.len());
    assert!(lib.tracks.iter().all(|t| !t.video_id.is_empty()));
    // track_view still resolves (AppState compatibility).
    let view = lib.track_view(lib.tracks[0].id);
    assert!(!view.title.is_empty());
}
