//! Full-track stream probe: resolve → ranged download → decode → playback.
//!
//! Run explicitly: `cargo run -p zuno-yt --example stream_probe --release`.
//! Asserts the whole pipeline serves audible samples, not just the 1 KB head:
//! the playhead must advance past 5 s within 60 s (proves download completed,
//! symphonia/libopus decoded it, and rodio is clocking samples out).
//!
//! Temporary diagnostic, kept because re-running it is the cheapest way to
//! re-prove the path after touching `download_stream`.

use zuno_core::PlayerHandle;
use zuno_yt::stream::resolve_stream_url;
use zuno_yt::{YtClient, YtClientExt};

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let mut client = YtClient::unsigned().expect("client builds");
    let results = client.search("radiohead creep").await.expect("live search works");
    let first = &results.tracks[0];
    println!("top hit: {:?} ({})", first.title, first.video_id);

    let stream = resolve_stream_url(&mut client, &first.video_id)
        .await
        .expect("live stream resolution works");
    println!("stream: client={} mime={} bitrate={}", stream.client, stream.mime, stream.bitrate);

    let player = PlayerHandle::new();
    player.play_stream(&stream.url, &stream.mime, None);
    println!("playing… waiting for the playhead to pass 5 s");

    let start = std::time::Instant::now();
    loop {
        std::thread::sleep(std::time::Duration::from_millis(500));
        let pos = player.position_sec();
        let playing = player.is_playing();
        if pos >= 5.0 && playing {
            println!("OK: playhead at {pos:.1} s after {:.1} s wall — full pipeline works",
                start.elapsed().as_secs_f32());
            player.stop();
            return;
        }
        if !playing && pos == 0.0 && start.elapsed().as_secs() > 10 {
            eprintln!("FAIL: playback never started (pos={pos}, playing={playing})");
            std::process::exit(1);
        }
        if start.elapsed().as_secs() > 90 {
            eprintln!("FAIL: playhead stuck at {pos:.1} s after 90 s (playing={playing})");
            std::process::exit(1);
        }
    }
}
