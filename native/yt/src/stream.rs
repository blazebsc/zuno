//! Stream resolution: `player` across clients, best direct audio URL out.
//!
//! Client order and every context/UA/header is verbatim from
//! `fetch_youtube_music_audio` + `try_youtube_api` in `src-tauri/src/lib.rs`:
//! IOS → ANDROID → TV → WEB_REMIX. Format ranking mirrors the TS
//! `resolveStream`: prefer highest-bitrate `audio/mp4`, fall back to any
//! audio. Formats carrying only `signatureCipher` are skipped — no decipher
//! step exists (and the spikes prove none is needed).

use crate::client::*;
use crate::error::{Result, YtError};
use crate::YtClient;
use serde_json::{json, Value};

#[derive(Clone, Debug)]
pub struct StreamInfo {
    pub url: String,
    pub mime: String,
    pub bitrate: u32,
    pub client: &'static str,
}

struct Attempt {
    name: &'static str,
    url: &'static str,
    context: fn() -> Value,
    user_agent: &'static str,
    client_name: &'static str,
    client_version: &'static str,
    music: bool,
}

const ATTEMPTS: &[Attempt] = &[
    Attempt {
        name: "YouTube iOS",
        url: PLAYER_URL,
        context: ios_context,
        user_agent: UA_IOS,
        client_name: CLIENT_IOS,
        client_version: "20.11.6",
        music: false,
    },
    Attempt {
        name: "YouTube ANDROID",
        url: PLAYER_URL,
        context: android_context,
        user_agent: UA_ANDROID,
        client_name: CLIENT_ANDROID,
        client_version: "21.03.36",
        music: false,
    },
    Attempt {
        name: "YouTube TV",
        url: PLAYER_URL,
        context: tv_context,
        user_agent: UA_TV,
        client_name: CLIENT_TV,
        client_version: "7.20260311.12.00",
        music: false,
    },
    Attempt {
        name: "YouTube Music WEB_REMIX",
        url: MUSIC_PLAYER_URL,
        context: web_remix_context,
        user_agent: UA_WEB,
        client_name: CLIENT_WEB_REMIX,
        client_version: "1.20250506.00.00",
        music: true,
    },
];

/// Resolve `video_id` to a direct audio URL. Unsigned is enough.
pub async fn resolve_stream(client: &mut YtClient, video_id: &str) -> Result<StreamInfo> {
    let mut failures = Vec::new();
    for attempt in ATTEMPTS {
        let body = json!({
            "context": (attempt.context)(),
            "videoId": video_id,
            "racyCheckOk": true,
            "contentCheckOk": true,
        });
        let url = format!("{}?key={}&prettyPrint=false", attempt.url, WEB_KEY);
        let resp = client
            .post_unsigned(&url, attempt.client_name, attempt.client_version, attempt.user_agent, body)
            .await;
        match resp {
            Ok(json) => match pick_audio(&json) {
                Ok((url, mime, bitrate)) => {
                    return Ok(StreamInfo { url, mime, bitrate, client: attempt.name });
                }
                Err(e) => failures.push(format!("{}: {e}", attempt.name)),
            },
            Err(e) => failures.push(format!("{}: {e}", attempt.name)),
        }
        let _ = attempt.music;
    }
    Err(YtError::Parse(format!("no direct audio stream for {video_id}: {}", failures.join("; "))))
}

/// Pick the best direct audio format: highest-bitrate `audio/mp4` first,
/// then any audio. Ciphered-only formats are skipped with no decipher.
pub fn pick_audio(resp: &Value) -> Result<(String, String, u32)> {
    let status = resp
        .get("playabilityStatus")
        .and_then(|s| s.get("status"))
        .and_then(|s| s.as_str())
        .unwrap_or("unknown");
    if status != "OK" {
        return Err(YtError::NotPlayable(status.to_string()));
    }
    let formats = resp
        .get("streamingData")
        .and_then(|sd| sd.get("adaptiveFormats"))
        .and_then(|af| af.as_array())
        .ok_or(YtError::NoStream)?;
    let mut best: Option<(String, String, u32, bool)> = None;
    let mut ciphered = 0u32;
    for format in formats {
        let Some(format) = format.as_object() else { continue };
        let (Some(mime), Some(bitrate)) = (
            format.get("mimeType").and_then(|m| m.as_str()),
            format.get("bitrate").and_then(|b| b.as_u64()),
        ) else {
            continue;
        };
        if !mime.starts_with("audio/") {
            continue;
        }
        let Some(url) = format.get("url").and_then(|u| u.as_str()) else {
            ciphered += 1;
            continue;
        };
        let is_mp4 = mime.starts_with("audio/mp4");
        let better = match &best {
            None => true,
            Some((_, _, best_bitrate, best_is_mp4)) => {
                (is_mp4 && !best_is_mp4) || (is_mp4 == *best_is_mp4 && bitrate > *best_bitrate as u64)
            }
        };
        if better {
            best = Some((
                url.to_string(),
                mime.split(';').next().unwrap_or("audio/mp4").to_string(),
                bitrate as u32,
                is_mp4,
            ));
        }
    }
    if ciphered > 0 {
        eprintln!("[zuno-yt] skipped {ciphered} ciphered-only audio formats (no decipher)");
    }
    best.map(|(url, mime, bitrate, _)| (url, mime, bitrate)).ok_or(YtError::NoStream)
}
