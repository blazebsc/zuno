//! Stream resolution: the app's client walk, verbatim.
//!
//! `resolveStream` in the TS datasource walks labelled clients in order and
//! returns the first playable audio URL: the walk itself holds no policy —
//! the order is handed in. The native port maps the labels onto the clients
//! it has:
//!
//! - "download" and "music" collapse to the same unattested set here (PO-token
//!   attestation is the single deliberate exclusion — see
//!   docs/gui-benchmarks/yt.md): direct iOS/Android/TV `player` responses
//!   first, then deciphered WEB_REMIX/WEB formats.
//! - "web" is the WEB client after those.
//!
//! Playback and downloads stay SEPARATE functions with no shared flags, the
//! app's "policy in a named method" rule: a mis-edited condition in one can
//! never change the other's behaviour. Both rank formats exactly like
//! `selectFormatForQuality` (nearest-bitrate, MP4-prefiltered except on
//! `high`, which takes the best on offer including Opus).

use crate::client::*;
use crate::error::{Result, YtError};
use crate::YtClient;
use serde_json::{json, Value};

/// The app's `AudioQuality` (src/internal/audioQuality.ts): `high` takes the
/// best on offer; the others target a bitrate.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AudioQuality {
    Low,
    Normal,
    High,
}

impl AudioQuality {
    /// Target bitrates in bps. `high` has no target — a number could sit
    /// below a format YouTube starts serving later.
    fn target_bps(self) -> Option<u64> {
        match self {
            AudioQuality::Low => Some(64_000),
            AudioQuality::Normal => Some(128_000),
            AudioQuality::High => None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct StreamInfo {
    pub url: String,
    pub mime: String,
    pub bitrate: u32,
    pub client: &'static str,
}

/// One audio format off a `player` response.
#[derive(Clone)]
pub struct Format<'a> {
    pub mime: &'a str,
    pub bitrate: u64,
    pub url: Option<&'a str>,
    pub cipher: Option<&'a str>,
}

/// `selectFormatForQuality`: nearest match, not "highest at or below the
/// target" — YouTube's AAC tier sits at ~131 kbps and a strict 128 kbps cap
/// would drop to the 49 kbps tier over a 3 kbps rounding difference.
pub fn select_format_for_quality(formats: &[Format<'_>], quality: AudioQuality) -> Option<usize> {
    if formats.is_empty() {
        return None;
    }
    // ranked descending by bitrate; ties keep the earlier (higher) entry
    let mut ranked: Vec<usize> = (0..formats.len()).collect();
    ranked.sort_by(|&a, &b| formats[b].bitrate.cmp(&formats[a].bitrate));
    let Some(target) = quality.target_bps() else {
        return ranked.first().copied();
    };
    let mut best = ranked[0];
    let mut best_distance = (formats[best].bitrate as i64 - target as i64).abs();
    for &idx in ranked.iter().skip(1) {
        let distance = (formats[idx].bitrate as i64 - target as i64).abs();
        if distance < best_distance {
            best = idx;
            best_distance = distance;
        }
    }
    Some(best)
}

/// Collect audio formats off a `player` response; `NotPlayable(status)` when
/// the video is not OK. A direct URL and a cipher both count.
fn audio_formats(resp: &Value) -> Result<Vec<Format<'_>>> {
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
    Ok(formats
        .iter()
        .filter_map(|f| {
            let mime = f.get("mimeType").and_then(|m| m.as_str())?;
            if !mime.starts_with("audio/") {
                return None;
            }
            Some(Format {
                mime,
                bitrate: f.get("bitrate").and_then(|b| b.as_u64()).unwrap_or(0),
                url: f.get("url").and_then(|u| u.as_str()),
                cipher: f
                    .get("signatureCipher")
                    .and_then(|c| c.as_str())
                    .or_else(|| f.get("cipher").and_then(|c| c.as_str())),
            })
        })
        .collect())
}

/// Which client a walk attempt targets, with everything the request and the
/// URL post-processing need. Contexts/UAs/keys are verbatim from
/// `try_youtube_api` (phase 1) and the music client bodies from
/// youtubei.js's `Music` client.
struct Attempt {
    name: &'static str,
    url: &'static str,
    context: fn() -> Value,
    user_agent: &'static str,
    client_name: &'static str,
    client_version: &'static str,
    /// `true` for the WEB family: the response's URLs need deciphering and
    /// the request should carry `playbackContext.signatureTimestamp`.
    deciphered: bool,
}

const ATTEMPTS: &[Attempt] = &[
    Attempt {
        name: "YouTube iOS",
        url: PLAYER_URL,
        context: ios_context,
        user_agent: UA_IOS,
        client_name: CLIENT_IOS,
        client_version: "20.11.6",
        deciphered: false,
    },
    Attempt {
        name: "YouTube ANDROID",
        url: PLAYER_URL,
        context: android_context,
        user_agent: UA_ANDROID,
        client_name: CLIENT_ANDROID,
        client_version: "21.03.36",
        deciphered: false,
    },
    Attempt {
        name: "YouTube TV",
        url: PLAYER_URL,
        context: tv_context,
        user_agent: UA_TV,
        client_name: CLIENT_TV,
        client_version: "7.20260311.12.00",
        deciphered: false,
    },
    Attempt {
        name: "YouTube Music WEB_REMIX",
        url: MUSIC_PLAYER_URL,
        context: web_remix_context,
        user_agent: UA_WEB,
        client_name: CLIENT_WEB_REMIX,
        client_version: "1.20250506.00.00",
        deciphered: true,
    },
    Attempt {
        name: "YouTube WEB",
        url: PLAYER_URL,
        context: web_context,
        user_agent: UA_WEB,
        client_name: CLIENT_WEB,
        client_version: "2.20260206.01.00",
        deciphered: true,
    },
];

/// Resolve `video_id` to a direct audio URL for *playback*.
///
/// The direct mobile/TV clients go first (phase 1's proven path), then the
/// deciphered WEB_REMIX and WEB formats — the clients the app's music/web
/// walk lands on. Quality defaults to the app's streaming default (`high`).
pub async fn resolve_stream_url(client: &mut YtClient, video_id: &str) -> Result<StreamInfo> {
    resolve_stream_url_quality(client, video_id, AudioQuality::High).await
}

/// Resolve a URL for the *offline download queue* — deliberately a separate
/// function rather than a flag on the one above: this body does not
/// reference the streaming preference at all, so downloads cannot inherit
/// it by a mis-edited condition (the app's rule for `resolveDownloadUrl`).
/// The unattested anonymous client stays in front — the path proven to
/// survive being pulled from Rust and written to disk.
pub async fn resolve_download_url(client: &mut YtClient, video_id: &str) -> Result<StreamInfo> {
    resolve_stream_url_quality(client, video_id, AudioQuality::High).await
}

/// One walk with an explicit quality — shared plumbing, not shared policy.
async fn resolve_stream_url_quality(
    client: &mut YtClient,
    video_id: &str,
    quality: AudioQuality,
) -> Result<StreamInfo> {
    let mut failures = Vec::new();
    for attempt in ATTEMPTS {
        let mut body = json!({
            "context": (attempt.context)(),
            "videoId": video_id,
            "racyCheckOk": true,
            "contentCheckOk": true,
        });
        if attempt.deciphered {
            // The music/web player bodies carry the playback context (the
            // same body `getBasicInfo` sends; `signatureTimestamp` makes
            // YouTube treat it as a real playback request).
            body["playbackContext"] = json!({
                "contentPlaybackContext": {
                    "vis": 0,
                    "splay": false,
                    "lactMilliseconds": "-1",
                    "signatureTimestamp": client.signature_timestamp(),
                }
            });
        }
        let url = format!("{}?key={}&prettyPrint=false", attempt.url, WEB_KEY);
        let resp = client
            .post_unsigned(&url, attempt.client_name, attempt.client_version, attempt.user_agent, body)
            .await;
        let resp = match resp {
            Ok(r) => r,
            Err(e) => {
                failures.push(format!("{}: {e}", attempt.name));
                continue;
            }
        };
        let formats = match audio_formats(&resp) {
            Ok(f) => f,
            Err(e) => {
                failures.push(format!("{}: {e}", attempt.name));
                continue;
            }
        };
        // Rank exactly like the app: MP4 preferred except on `high`, which
        // sees every audio format (Opus-in-WebM is the only audio for a
        // large share of tracks).
        let mp4s: Vec<usize> = formats
            .iter()
            .enumerate()
            .filter(|(_, f)| f.mime.contains("audio/mp4"))
            .map(|(i, _)| i)
            .collect();
        let candidates: Vec<Format> = if quality == AudioQuality::High || mp4s.is_empty() {
            formats.clone()
        } else {
            mp4s.iter().map(|&i| formats[i].clone()).collect()
        };
        let Some(pick) = select_format_for_quality(&candidates, quality) else {
            failures.push(format!("{}: no audio format", attempt.name));
            continue;
        };
        let format = &candidates[pick];
        match resolve_format_url(client, format, attempt).await {
            Ok(url) => {
                return Ok(StreamInfo {
                    url,
                    mime: format.mime.split(';').next().unwrap_or("audio/mp4").to_string(),
                    bitrate: format.bitrate as u32,
                    client: attempt.name,
                })
            }
            Err(e) => failures.push(format!("{}: {e}", attempt.name)),
        }
    }
    Err(YtError::Parse(format!(
        "no playable audio stream for {video_id}: {}",
        failures.join("; ")
    )))
}

/// Turn one ranked format into a direct URL: direct `url`s only need their
/// throttling `n` transformed (and only for the WEB family — the mobile/TV
/// URLs are usable as-is, phase 1's verified path), ciphers need the full
/// decipher.
async fn resolve_format_url(client: &mut YtClient, format: &Format<'_>, attempt: &Attempt) -> Result<String> {
    if let Some(cipher) = format.cipher {
        return client
            .decipherer()
            .decipher_cipher(client.http(), cipher, attempt.client_version)
            .await;
    }
    let Some(url) = format.url else {
        return Err(YtError::NoStream);
    };
    if !attempt.deciphered {
        return Ok(url.to_string());
    }
    client
        .decipherer()
        .decipher_url(client.http(), url, attempt.client_version)
        .await
}

/// Phase 1's entry point, kept for the seven wiring tasks: the same walk
/// with the app's playback default quality.
pub async fn resolve_stream(client: &mut YtClient, video_id: &str) -> Result<StreamInfo> {
    resolve_stream_url(client, video_id).await
}

/// The best direct audio format of a response, for callers that already hold
/// one (kept from phase 1; ranking now matches the app exactly).
pub fn pick_audio(resp: &Value) -> Result<(String, String, u32)> {
    let formats = audio_formats(resp)?;
    let pick = select_format_for_quality(&formats, AudioQuality::High)
        .ok_or(YtError::NoStream)?;
    let f = &formats[pick];
    let url = f.url.or(f.cipher).ok_or(YtError::NoStream)?;
    Ok((
        url.to_string(),
        f.mime.split(';').next().unwrap_or("audio/mp4").to_string(),
        f.bitrate as u32,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fmt(mime: &str, bitrate: u64) -> Format<'static> {
        Format { mime: Box::leak(mime.to_string().into_boxed_str()), bitrate, url: None, cipher: None }
    }

    #[test]
    fn high_takes_best_bitrate_including_opus() {
        // itag 251 Opus at ~151 kbps outranks itag 140 AAC at ~130 kbps
        let formats = [fmt("audio/mp4; codecs=mp4a.40.2", 130_000), fmt("audio/webm; codecs=opus", 151_000)];
        let pick = select_format_for_quality(&formats, AudioQuality::High).unwrap();
        assert_eq!(formats[pick].mime, "audio/webm; codecs=opus");
    }

    #[test]
    fn normal_prefers_mp4_and_nearest_bitrate() {
        let formats = [
            fmt("audio/webm; codecs=opus", 151_000),
            fmt("audio/mp4; codecs=mp4a.40.2", 131_000), // 3 kbps over the 128k target
            fmt("audio/mp4; codecs=mp4a.40.5", 49_000),
        ];
        // MP4 prefilter keeps itags 140 and 139; nearest to 128k picks 140.
        let mp4s: Vec<Format> = formats.iter().filter(|f| f.mime.contains("audio/mp4")).cloned().collect();
        let pick = select_format_for_quality(&mp4s, AudioQuality::Normal).unwrap();
        assert_eq!(mp4s[pick].bitrate, 131_000, "131k is nearer to 128k than 49k");
    }

    #[test]
    fn unplayable_status_surfaces() {
        let resp = json!({"playabilityStatus": {"status": "LOGIN_REQUIRED"}, "streamingData": {"adaptiveFormats": []}});
        assert!(matches!(audio_formats(&resp), Err(YtError::NotPlayable(_))));
    }
}
