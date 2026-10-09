//! Cookie session: import, identity, rotation, disk persistence.
//!
//! Mirrors the rules in `src-tauri/src/lib.rs` (`parse_cookie_header`,
//! `apply_set_cookie`, `cookie_account_identity`, the `CookieJarState`
//! persistence throttle): split on `;`, keep `NAME=value` pairs, drop
//! attributes; a `Set-Cookie` that echoes a name back empty/`EXPIRED`/
//! `deleted` removes it; only `youtube.com` (and subdomains) rotate the jar;
//! a rotated *credential* cookie (`__Secure-*PSIDTS` above all) is written to
//! disk the moment it changes, while the noisy ones (`SIDCC`,
//! `__Secure-*PSIDCC`) wait for a 300 s interval — Google rotates them on
//! almost every response and nothing authenticates with them.
//!
//! Persisted as a plain JSON file under the platform data dir — no keyring
//! dependency. The Tauri app's keyring/AES-GCM storage is stronger; a
//! production port should match it (see docs/gui-benchmarks/yt.md).

use crate::error::{Result, YtError};
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

/// How often a rotation of [`SLOW_PERSIST_COOKIES`] is written back to disk.
const PERSIST_INTERVAL: Duration = Duration::from_secs(300);
/// Cookies whose rotation may be persisted late, because nothing
/// authenticates with them. Everything else is written the moment it
/// changes — Google retires the superseded value, so quitting inside the
/// throttle window would leave a dead credential on disk.
const SLOW_PERSIST_COOKIES: [&str; 3] = ["SIDCC", "__Secure-1PSIDCC", "__Secure-3PSIDCC"];

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Session {
    /// The `Cookie:` header value (`NAME=value; NAME=value; …`).
    pub cookie_header: String,
    /// Last durable write, for the slow-persist throttle (never persisted
    /// itself — serde skips are not needed because it stays `None` on disk).
    #[serde(skip)]
    pub persisted_at: Option<Instant>,
}

impl Session {
    /// Parse a pasted `Cookie:` request header. Only `NAME=value` pairs
    /// survive; `Set-Cookie` attributes (`Path`, `Domain`, …) are dropped —
    /// a browser never sends those in a `Cookie` header, but pastes
    /// sometimes include them.
    pub fn import(header: &str) -> Self {
        Session { cookie_header: serialize_pairs(&parse_pairs_filtered(header)), ..Default::default() }
    }

    pub fn is_empty(&self) -> bool {
        self.cookie_header.trim().is_empty()
    }

    /// Which Google account this cookie belongs to, as far as the app cares:
    /// the SAPISID-family value (`cookie_account_identity` — same order as
    /// lib.rs: `SAPISID`, then `__Secure-3PAPISID`, then `__Secure-1PAPISID`).
    pub fn account_identity(&self) -> Option<String> {
        let pairs = parse_pairs(&self.cookie_header);
        for name in ["SAPISID", "__Secure-3PAPISID", "__Secure-1PAPISID"] {
            if let Some((_, v)) = pairs.iter().find(|(n, _)| n == name) {
                return Some(v.clone());
            }
        }
        None
    }

    /// The SAPISID-family value used for the SAPISIDHASH signature.
    pub fn sapisid(&self) -> Option<String> {
        self.account_identity()
    }

    /// Fold response `Set-Cookie` values back into the session (`url` gates
    /// rotation to YouTube hosts, like `is_youtube_cookie_host`). Persists to
    /// disk under the lib.rs rule — credentials immediately, noisy cookies
    /// at most once per [`PERSIST_INTERVAL`] — and reports whether anything
    /// changed.
    pub fn merge_set_cookies(&mut self, url: &str, set_cookies: &[String]) -> bool {
        if !is_youtube_host(url) {
            return false;
        }
        let mut pairs = parse_pairs(&self.cookie_header);
        let mut changed = false;
        let mut credential_changed = false;
        for sc in set_cookies {
            if apply_set_cookie(&mut pairs, sc) {
                changed = true;
                credential_changed |= !is_slow_persist_cookie(sc);
            }
        }
        if !changed {
            return false;
        }
        self.cookie_header = serialize_pairs(&pairs);
        let should_persist = credential_changed
            || self
                .persisted_at
                .is_none_or(|at| at.elapsed() >= PERSIST_INTERVAL);
        if should_persist {
            self.persisted_at = Some(Instant::now());
            if let Err(e) = self.save() {
                eprintln!("[zuno-yt] session persist failed: {e}");
            }
        }
        changed
    }

    /// Load a previously saved session, if any.
    pub fn load() -> Option<Self> {
        let path = session_path()?;
        let data = std::fs::read(path).ok()?;
        serde_json::from_slice(&data).ok()
    }

    /// Persist the session (plain JSON — see module docs).
    pub fn save(&self) -> Result<()> {
        let path = session_path().ok_or(YtError::Parse("no data dir".into()))?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| YtError::Parse(e.to_string()))?;
        }
        let data = serde_json::to_vec(self)?;
        std::fs::write(path, data).map_err(|e| YtError::Parse(e.to_string()))
    }

    pub fn delete_saved() {
        if let Some(path) = session_path() {
            let _ = std::fs::remove_file(path);
        }
    }
}

fn parse_pairs(header: &str) -> Vec<(String, String)> {
    header
        .split(';')
        .filter_map(|part| part.trim().split_once('='))
        .map(|(n, v)| (n.trim().to_string(), v.trim().to_string()))
        .filter(|(n, _)| !n.is_empty())
        .collect()
}

fn parse_pairs_filtered(header: &str) -> Vec<(String, String)> {
    parse_pairs(header)
        .into_iter()
        .filter(|(n, _)| {
            !["expires", "max-age", "path", "domain", "samesite"]
                .contains(&n.to_ascii_lowercase().as_str())
        })
        .collect()
}

fn serialize_pairs(pairs: &[(String, String)]) -> String {
    pairs.iter().map(|(n, v)| format!("{n}={v}")).collect::<Vec<_>>().join("; ")
}

fn split_set_cookie(set_cookie: &str) -> Option<(&str, &str)> {
    let (name, value) = set_cookie.split(';').next()?.split_once('=')?;
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    Some((name, value.trim()))
}

/// `apply_set_cookie` from lib.rs, verbatim rules.
fn apply_set_cookie(pairs: &mut Vec<(String, String)>, set_cookie: &str) -> bool {
    let Some((name, value)) = split_set_cookie(set_cookie) else {
        return false;
    };
    // Google clears a cookie by echoing it back empty/tombstoned.
    if value.is_empty() || value == "EXPIRED" || value == "deleted" {
        let before = pairs.len();
        pairs.retain(|(n, _)| n != name);
        return pairs.len() != before;
    }
    match pairs.iter_mut().find(|(n, _)| n == name) {
        Some(e) if e.1 == value => false,
        Some(e) => {
            e.1 = value.to_string();
            true
        }
        None => {
            pairs.push((name.to_string(), value.to_string()));
            true
        }
    }
}

fn is_slow_persist_cookie(set_cookie: &str) -> bool {
    split_set_cookie(set_cookie)
        .is_some_and(|(name, _)| SLOW_PERSIST_COOKIES.contains(&name))
}

/// `is_youtube_cookie_host`: the jar has exactly one destination.
fn is_youtube_host(url: &str) -> bool {
    let host = url
        .split("://")
        .nth(1)
        .unwrap_or(url)
        .split(['/', '?', ':'])
        .next()
        .unwrap_or("")
        .trim_start_matches('.')
        .to_ascii_lowercase();
    host == "youtube.com" || host.ends_with(".youtube.com")
}

fn session_path() -> Option<std::path::PathBuf> {
    let base = dirs_fallback()?;
    Some(base.join("zuno").join("yt-session.json"))
}

/// Home-based data dir without a new dependency.
fn dirs_fallback() -> Option<std::path::PathBuf> {
    if let Ok(dir) = std::env::var("XDG_DATA_HOME") {
        if !dir.is_empty() {
            return Some(dir.into());
        }
    }
    let home = std::env::var("HOME").ok()?;
    Some(std::path::PathBuf::from(home).join(".local").join("share"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn import_drops_attributes() {
        let s = Session::import("SID=abc; Path=/; Domain=.youtube.com; SAPISID=xyz; Secure; HttpOnly");
        assert_eq!(s.cookie_header, "SID=abc; SAPISID=xyz");
    }

    #[test]
    fn identity_order_matches_lib_rs() {
        // lib.rs checks SAPISID, then __Secure-3PAPISID, then __Secure-1PAPISID.
        let s = Session::import("A=1; __Secure-1PAPISID=one; __Secure-3PAPISID=three");
        assert_eq!(s.account_identity().as_deref(), Some("three"));
        let s = Session::import("A=1; __Secure-1PAPISID=one");
        assert_eq!(s.account_identity().as_deref(), Some("one"));
        assert!(Session::import("A=1").account_identity().is_none());
    }

    #[test]
    fn rotation_replaces_and_tombstones() {
        let mut s = Session::import("SID=old; KEEP=k");
        let changed = s.merge_set_cookies(
            "https://music.youtube.com/youtubei/v1/browse",
            &["SID=new; Path=/; Domain=.youtube.com".into(), "GONE=EXPIRED; Path=/".into()],
        );
        assert!(changed);
        assert!(s.cookie_header.contains("SID=new"));
        assert!(s.cookie_header.contains("KEEP=k"));
        assert!(!s.cookie_header.contains("GONE"));
    }

    #[test]
    fn rotation_rejects_foreign_and_googlevideo_hosts() {
        // The jar has exactly one destination: youtube.com (lib.rs), so the
        // CDN hosts that serve stream bytes never rotate it.
        let mut s = Session::import("SID=old");
        assert!(!s.merge_set_cookies("https://evil.example/x", &["SID=new".into()]));
        assert!(!s
            .merge_set_cookies("https://rr1---sn-qx8vapo1-53a6.googlevideo.com/videoplayback", &["SID=new".into()]));
        assert!(s.merge_set_cookies("https://www.youtube.com/x", &["SID=new".into()]));
        assert_eq!(s.cookie_header, "SID=new");
    }

    #[test]
    fn slow_persist_classification() {
        assert!(is_slow_persist_cookie("SIDCC=abc; Path=/"));
        assert!(is_slow_persist_cookie("__Secure-1PSIDCC=abc"));
        assert!(is_slow_persist_cookie("__Secure-3PSIDCC=abc"));
        assert!(!is_slow_persist_cookie("__Secure-1PSIDTS=abc"), "credentials are not slow-persist");
        assert!(!is_slow_persist_cookie("SAPISID=abc"));
    }
}
