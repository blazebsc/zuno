//! The ONE online test. Ignored by default; run explicitly:
//! `cargo test -p zuno-yt --test online -- --ignored --nocapture`
//!
//! Proves end-to-end against the live API: unsigned search returns real
//! results, and the client walk resolves a DIRECT audio URL — the direct
//! mobile/TV clients first, then the deciphered WEB_REMIX/WEB formats when
//! those fail (the decipher path fetches the live player JS).

use zuno_yt::stream::resolve_stream_url;
use zuno_yt::{YtClient, YtClientExt};

#[tokio::test]
#[ignore]
async fn live_search_and_stream() {
    let mut client = YtClient::unsigned().expect("client builds");
    let results = client.search("radiohead creep").await.expect("live search works");
    assert!(!results.tracks.is_empty(), "live search must return tracks");
    let first = &results.tracks[0];
    println!("top hit: {:?} ({})", first.title, first.video_id);
    assert_eq!(first.video_id.len(), 11);

    let stream = resolve_stream_url(&mut client, &first.video_id)
        .await
        .expect("live stream resolution works");
    println!("stream: client={} mime={} bitrate={}", stream.client, stream.mime, stream.bitrate);
    println!("url head: {}", stream.url.chars().take(120).collect::<String>());
    assert!(stream.url.starts_with("https://"), "DIRECT audio URL");
    assert!(stream.mime.starts_with("audio/"));

    // The URL actually serves bytes.
    let head = reqwest::Client::new()
        .get(&stream.url)
        .header("Range", "bytes=0-1023")
        .send()
        .await
        .expect("stream URL fetches");
    assert!(head.status().is_success(), "stream URL serves bytes: {}", head.status());
    let bytes = head.bytes().await.expect("body reads");
    assert_eq!(bytes.len(), 1024, "ranged fetch returns exactly the head");
    println!("ranged head fetch: {} bytes OK", bytes.len());

    // Category search + suggestions + one playlist page of the same session.
    let songs = client
        .search_category("radiohead", zuno_yt::api::SearchCategory::Song)
        .await
        .expect("live category search works");
    assert!(!songs.tracks.is_empty(), "song category returns tracks");
    println!("song category: {} tracks", songs.tracks.len());

    let suggestions = client
        .search_suggestions("radiohead cr")
        .await
        .expect("live suggestions work");
    println!("suggestions: {suggestions:?}");
    assert!(!suggestions.is_empty(), "suggestions must not be empty");

    // A long public playlist exercises pagination against the live API.
    let (page, session) = client
        .get_playlist_track_page("PLUBFDMbS-c2lOqb9hWhhcwOpVfj7GIQGF", None)
        .await
        .expect("playlist first page");
    println!("playlist page 1: {} tracks, has_more={}", page.tracks.len(), page.has_more);
    assert!(page.tracks.len() >= 100, "first page of a 111-track playlist");
    if page.has_more {
        let (page2, _) = client
            .get_playlist_track_page("PLUBFDMbS-c2lOqb9hWhhcwOpVfj7GIQGF", session)
            .await
            .expect("playlist second page");
        println!("playlist page 2: +{} tracks", page2.tracks.len());
        assert!(!page2.tracks.is_empty(), "the second page must carry fresh rows");
    }

    // The related tab (browse the MUSIC_PAGE_TYPE_TRACK_RELATED endpoint).
    let related = client.get_related(&first.video_id).await;
    match related {
        Ok(shelves) => println!("related: {} shelves", shelves.len()),
        Err(e) => println!("related unavailable for this track (can happen unsigned): {e}"),
    }
}
