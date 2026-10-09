//! Cookie session: import, identity, rotation, disk persistence.
//!
//! Mirrors the rules in `src-tauri/src/lib.rs` (`parse_cookie_header`,
//! `apply_set_cookie`, `cookie_account_identity`): split on `;`, keep
//! `NAME=value` pairs, drop attributes; a `Set-Cookie` that echoes a name
//! back empty/`EXPIRED`/`deleted` removes it; only YouTube hosts rotate.
//!
//! Persisted as a plain JSON file under the platform data dir — no keyring
//! dependency. The Tauri app's keyring/AES-GCM storage is stronger; a
//! production port should match it (see docs/gui-benchmarks/yt.md).

use crate::error::{Result, YtError};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Session {
    /// The `Cookie:` header value (`NAME=value; NAME=value; …`).
    pub cookie_header: String,
}

impl Session {
    /// Parse a pasted `Cookie:` request header. Only `NAME=value` pairs
    /// survive; `Set-Cookie` attributes (`Path`, `Domain`, …) are dropped —
    /// a browser never sends those in a `Cookie` header, but pastes
    /// sometimes include them.
    pub fn import(header: &str) -> Self {
        Session { cookie_header: serialize_pairs(&parse_pairs_filtered(header)) }
    }

    pub fn is_empty(&self) -> bool {
        self.cookie_header.trim().is_empty()
    }

    /// Which Google account this cookie belongs to, as far as the app cares:
    /// the SAPISID-family value (mirrors `cookie_account_identity` +
    /// `getSapisidAuthCookie`: `SAPISID`, `__Secure-1PAPISID`, `__Secure-3PAPISID`).
    pub fn account_identity(&self) -> Option<String> {
        let pairs = parse_pairs(&self.cookie_header);
        for name in ["SAPISID", "__Secure-1PAPISID", "__Secure-3PAPISID"] {
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

    /// Fold response `Set-Cookie` values back into the session. Returns true
    /// when the header changed. `url` gates rotation to YouTube hosts.
    pub fn merge_set_cookies(&mut self, url: &str, set_cookies: &[String]) -> bool {
        if !is_youtube_host(url) {
            return false;
        }
        let mut pairs = parse_pairs(&self.cookie_header);
        let mut changed = false;
        for sc in set_cookies {
            changed |= apply_set_cookie(&mut pairs, sc);
        }
        if changed {
            self.cookie_header = serialize_pairs(&pairs);
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
    ["youtube.com", "googlevideo.com", "ytimg.com", "ggpht.com", "googleusercontent.com"]
        .iter()
        .any(|suffix| host == *suffix || host.ends_with(&format!(".{suffix}")))
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
    fn identity_prefers_sapisid() {
        let s = Session::import("A=1; __Secure-3PAPISID=three; SAPISID=one");
        assert_eq!(s.account_identity().as_deref(), Some("one"));
        let s = Session::import("A=1; __Secure-3PAPISID=three");
        assert_eq!(s.account_identity().as_deref(), Some("three"));
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
    fn rotation_rejects_foreign_hosts() {
        let mut s = Session::import("SID=old");
        assert!(!s.merge_set_cookies("https://evil.example/x", &["SID=new".into()]));
        assert_eq!(s.cookie_header, "SID=old");
    }
}
