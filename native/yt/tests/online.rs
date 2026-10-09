//! The ONE online test. Ignored by default; run explicitly:
//! `cargo test -p zuno-yt --test online -- --ignored --nocapture`
//!
//! Proves end-to-end against the live API: unsigned search returns real
//! results, and stream resolution yields a DIRECT audio URL.

use zuno_yt::{YtClient, YtClientExt, resolve_stream};

#[tokio::test]
#[ignore]
async fn live_search_and_stream() {
    let mut client = YtClient::unsigned().expect("client builds");
    let results = client.search("radiohead creep").await.expect("live search works");
    assert!(!results.tracks.is_empty(), "live search must return tracks");
    let first = &results.tracks[0];
    println!("top hit: {:?} ({})", first.title, first.video_id);
    assert_eq!(first.video_id.len(), 11);

    let stream = resolve_stream(&mut client, &first.video_id)
        .await
        .expect("live stream resolution works");
    println!("stream: client={} mime={} bitrate={}", stream.client, stream.mime, stream.bitrate);
    println!("url head: {}", stream.url.chars().take(120).collect::<String>());
    assert!(stream.url.starts_with("https://"), "DIRECT audio URL, no decipher");
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
}
