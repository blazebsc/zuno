//! One Innertube POST path for every call. Contexts, user agents and the
//! API keys are verbatim from `src-tauri/src/lib.rs` (`create_*_context`,
//! `try_youtube_api`); the SAPISIDHASH scheme is a line-for-line port of
//! `applyCookieAuth` in `tauriFetch.ts`. Cookie rotation folding follows
//! the jar rules of lib.rs (see [`crate::session`]).

use crate::error::{Result, YtError};
use crate::session::Session;
use sha1::{Digest, Sha1};

pub const MUSIC_KEY: &str = "AIzaSyC9XL3ZjWddXya6X74dJoCTL-WEYFDNX30";
pub const WEB_KEY: &str = "AIzaSyAO_FJ2SlqU8Q4STEHLGCilw_Y9_11qcW8";

pub const MUSIC_SEARCH_URL: &str = "https://music.youtube.com/youtubei/v1/search";
pub const MUSIC_BROWSE_URL: &str = "https://music.youtube.com/youtubei/v1/browse";
pub const MUSIC_NEXT_URL: &str = "https://music.youtube.com/youtubei/v1/next";
pub const PLAYER_URL: &str = "https://www.youtube.com/youtubei/v1/player";
pub const MUSIC_PLAYER_URL: &str = "https://music.youtube.com/youtubei/v1/player";

pub const UA_WEB: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/135.0.0.0 Safari/537.36";
pub const UA_IOS: &str =
    "com.google.ios.youtube/20.11.6 (iPhone10,4; U; CPU iOS 16_7_7 like Mac OS X)";
pub const UA_ANDROID: &str =
    "com.google.android.youtube/21.03.36(Linux; U; Android 16; en_US; SM-S908E Build/TP1A.220624.014) gzip";
pub const UA_TV: &str = "Mozilla/5.0 (ChromiumStylePlatform) Cobalt/Version";

/// `X-YouTube-Client-Name` per client (from `try_youtube_api`).
pub const CLIENT_WEB_REMIX: &str = "67";
pub const CLIENT_WEB: &str = "1";
pub const CLIENT_IOS: &str = "5";
pub const CLIENT_ANDROID: &str = "3";
pub const CLIENT_TV: &str = "7";

pub fn web_remix_context() -> serde_json::Value {
    serde_json::json!({
        "client": {
            "clientName": "WEB_REMIX",
            "clientVersion": "1.20250506.00.00",
            "hl": "en",
            "gl": "US",
            "platform": "DESKTOP",
            "osName": "Windows",
            "osVersion": "10.0",
            "browserName": "Chrome",
            "browserVersion": "135.0.0.0",
            "userAgent": UA_WEB,
        }
    })
}

pub fn web_context() -> serde_json::Value {
    serde_json::json!({
        "client": {
            "clientName": "WEB",
            "clientVersion": "2.20260206.01.00",
            "hl": "en",
            "gl": "US",
            "platform": "DESKTOP",
            "osName": "Windows",
            "osVersion": "10.0",
            "browserName": "Chrome",
            "browserVersion": "135.0.0.0",
            "userAgent": UA_WEB,
        }
    })
}

pub fn ios_context() -> serde_json::Value {
    serde_json::json!({
        "client": {
            "clientName": "IOS",
            "clientVersion": "20.11.6",
            "hl": "en",
            "gl": "US",
            "deviceModel": "iPhone10,4",
            "osName": "iPhone",
            "osVersion": "16.7.7.20H330",
            "userAgent": UA_IOS,
        }
    })
}

pub fn android_context() -> serde_json::Value {
    serde_json::json!({
        "client": {
            "clientName": "ANDROID",
            "clientVersion": "21.03.36",
            "hl": "en",
            "gl": "US",
            "platform": "MOBILE",
            "osName": "Android",
            "osVersion": "16",
            "androidSdkVersion": 36,
            "userAgent": UA_ANDROID,
        }
    })
}

pub fn tv_context() -> serde_json::Value {
    serde_json::json!({
        "client": {
            "clientName": "TVHTML5",
            "clientVersion": "7.20260311.12.00",
            "hl": "en",
            "gl": "US",
            "platform": "TV",
            "osName": "Linux",
            "userAgent": UA_TV,
        }
    })
}

/// `SAPISIDHASH <ts>_<sha1(ts + " " + sapisid + " " + origin)>` — the exact
/// scheme from `applyCookieAuth`. Origin is bound to the client: WEB_REMIX
/// (name 67) signs for music.youtube.com, everything else for www.youtube.com.
pub fn sapisid_hash(sapisid: &str, client_name: &str, timestamp: u64) -> String {
    let origin = if client_name == CLIENT_WEB_REMIX {
        "https://music.youtube.com"
    } else {
        "https://www.youtube.com"
    };
    let mut h = Sha1::new();
    h.update(format!("{timestamp} {sapisid} {origin}"));
    format!("SAPISIDHASH {timestamp}_{:x}", h.finalize())
}

pub struct YtClient {
    http: reqwest::Client,
    session: Option<Session>,
    decipherer: crate::decipher::Decipherer,
}

impl YtClient {
    /// No credentials. Search, browse, charts, stream resolution all work.
    pub fn unsigned() -> Result<Self> {
        Ok(YtClient {
            http: default_http()?,
            session: None,
            decipherer: crate::decipher::Decipherer::new()?,
        })
    }

    pub fn with_session(session: Session) -> Result<Self> {
        Ok(YtClient {
            http: default_http()?,
            session: Some(session),
            decipherer: crate::decipher::Decipherer::new()?,
        })
    }

    pub fn set_session(&mut self, session: Option<Session>) {
        self.session = session;
    }

    pub fn session(&self) -> Option<&Session> {
        self.session.as_ref()
    }

    pub fn signed_in(&self) -> bool {
        self.session.as_ref().is_some_and(|s| !s.is_empty())
    }

    pub(crate) fn http(&self) -> &reqwest::Client {
        &self.http
    }

    pub(crate) fn decipherer(&self) -> &crate::decipher::Decipherer {
        &self.decipherer
    }

    /// The loaded player script's `signatureTimestamp` (0 until the first
    /// decipher, or after a rotation reset). Sent in the WEB-family player
    /// bodies, exactly like youtubei.js's music/web clients.
    pub fn signature_timestamp(&self) -> u64 {
        self.decipherer.signature_timestamp()
    }

    fn require_session(&self) -> Result<&Session> {
        self.session.as_ref().filter(|s| !s.is_empty()).ok_or(YtError::NotSignedIn)
    }

    /// Authenticated POST: stamps `Cookie` + `SAPISIDHASH`, folds rotation.
    pub async fn post_signed(
        &mut self,
        url: &str,
        client_name: &str,
        client_version: &str,
        user_agent: &str,
        body: serde_json::Value,
    ) -> Result<serde_json::Value> {
        let cookie = self.require_session()?.cookie_header.clone();
        self.post_inner(url, client_name, client_version, user_agent, body, Some(&cookie)).await
    }

    /// Unsigned POST (no cookie, no signature).
    pub async fn post_unsigned(
        &mut self,
        url: &str,
        client_name: &str,
        client_version: &str,
        user_agent: &str,
        body: serde_json::Value,
    ) -> Result<serde_json::Value> {
        self.post_inner(url, client_name, client_version, user_agent, body, None).await
    }

    /// POST with the session when present, unsigned otherwise.
    async fn post_inner(
        &mut self,
        url: &str,
        client_name: &str,
        client_version: &str,
        user_agent: &str,
        body: serde_json::Value,
        cookie: Option<&str>,
    ) -> Result<serde_json::Value> {
        let ts = now_secs();
        let mut req = self
            .http
            .post(url)
            .header("Content-Type", "application/json")
            .header("User-Agent", user_agent)
            .header("Accept", "application/json")
            .header("Accept-Language", "en-US,en;q=0.9")
            .header("X-YouTube-Client-Name", client_name)
            .header("X-YouTube-Client-Version", client_version);
        if let Some(cookie) = cookie {
            req = req.header("Cookie", cookie);
            if let Some(sapisid) = self.session.as_ref().and_then(|s| s.sapisid()) {
                let origin = if client_name == CLIENT_WEB_REMIX {
                    "https://music.youtube.com"
                } else {
                    "https://www.youtube.com"
                };
                req = req
                    .header("Authorization", sapisid_hash(&sapisid, client_name, ts))
                    .header("X-Goog-Request-Time", ts.to_string())
                    .header("Origin", origin)
                    .header("X-Origin", origin)
                    .header("Referer", format!("{origin}/"));
            } else {
                let (origin, referer) = origin_pair(client_name);
                req = req.header("Origin", origin).header("Referer", referer);
            }
        } else {
            let (origin, referer) = origin_pair(client_name);
            req = req.header("Origin", origin).header("Referer", referer);
        }
        let body_str =
            serde_json::to_string(&body).map_err(|e| YtError::Json(e.to_string()))?;
        let resp = req.body(body_str).send().await?;
        let status = resp.status();
        let set_cookies: Vec<String> = resp
            .headers()
            .get_all(reqwest::header::SET_COOKIE)
            .iter()
            .filter_map(|v| v.to_str().ok().map(str::to_string))
            .collect();
        if !set_cookies.is_empty() {
            if let Some(session) = self.session.as_mut() {
                session.merge_set_cookies(url, &set_cookies);
            }
        }
        let text = resp.text().await?;
        if !status.is_success() {
            let preview: String = text.chars().take(300).collect();
            return Err(YtError::Status(status.as_u16(), preview));
        }
        serde_json::from_str(&text).map_err(YtError::from)
    }
}

fn origin_pair(client_name: &str) -> (&'static str, &'static str) {
    if client_name == CLIENT_WEB_REMIX {
        ("https://music.youtube.com", "https://music.youtube.com/")
    } else {
        ("https://www.youtube.com", "https://www.youtube.com/")
    }
}

fn default_http() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(YtError::from)
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_binds_origin_to_client() {
        let a = sapisid_hash("SAPISID_VALUE", CLIENT_WEB_REMIX, 1_700_000_000);
        let b = sapisid_hash("SAPISID_VALUE", CLIENT_WEB, 1_700_000_000);
        assert!(a.starts_with("SAPISIDHASH 1700000000_"));
        assert_ne!(a, b, "music vs www origin must sign differently");
        assert_eq!(a.len(), "SAPISIDHASH 1700000000_".len() + 40);
    }
}
