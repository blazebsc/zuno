//! Signature decipher: the ONE JavaScript step in the native app.
//!
//! YouTube's web clients hand out stream URLs only in ciphered form
//! (`signatureCipher`: a scrambled `s`, the param name `sp`, and the real
//! `url`), plus a throttling `n` that must be transformed on every URL.
//! Undoing both is pure string arithmetic inside YouTube's own player
//! script — no DOM, no network, no BotGuard — so we locate the decipher
//! function in the player JS the way youtubei.js v17 does
//! ([`extract`], a textual port of its AST extractor) and evaluate it with
//! QuickJS (`rquickjs`). This is JS *evaluation*, not a browser engine.
//!
//! Flow (mirrors `Player` + `Player.decipher` in youtubei.js, and the app's
//! `format.decipher` usage in `YouTubeMusicDataSource.ts`):
//!
//! 1. `GET /iframe_api` → `player_id` (between `player\/` and `\/`).
//! 2. `GET /s/player/{id}/player_es6.vflset/en_US/base.js`.
//! 3. Extract the decipher function + dependency closure + signatureTimestamp.
//! 4. Evaluate with a mock URL (youtubei.js's `getNsigProcessorFn` recipe),
//!    read the transformed `sig` and `n` back, stamp them onto the URL, and
//!    rewrite `cver` to the client version the session actually used
//!    (`withSessionClientVersion` in the TS datasource).
//!
//! The compiled player is cached per player-id; a decipher failure refetches
//! the player once and retries (YouTube rotates the script every few weeks).

pub mod extract;
pub mod lex;

use crate::client::UA_WEB;
use crate::error::{Result, YtError};
use rquickjs::{Context, Runtime};
use std::sync::Mutex;

const IFRAME_API_URL: &str = "https://www.youtube.com/iframe_api";
const PLAYER_JS_URL: &str = "https://www.youtube.com/s/player/{id}/player_es6.vflset/en_US/base.js";

/// youtubei.js's runtime preamble, verbatim semantics: the closure may
/// reference browser globals at load time; bind them from the host when it
/// has them and to inert stand-ins when it does not (QuickJS never does).
const PREAMBLE: &str = r#"
const __jsExtractorGlobal = globalThis;
const window = typeof __jsExtractorGlobal.window !== "undefined" ? __jsExtractorGlobal.window : {location:{hostname:"www.youtube.com",href:"https://www.youtube.com/",protocol:"https:",origin:"https://www.youtube.com",search:"",hash:""},navigator:{userAgent:"Mozilla/5.0",language:"en-US",languages:["en-US"]},performance:{now:()=>Date.now()}};
const document = typeof __jsExtractorGlobal.document !== "undefined" ? __jsExtractorGlobal.document : {createElement:()=>({style:{},setAttribute(){},appendChild(){},getElementsByTagName:()=>[]}),getElementsByTagName:()=>[],addEventListener(){},head:{appendChild(){}},documentElement:{style:{}}};
const self = typeof __jsExtractorGlobal.self !== "undefined" ? __jsExtractorGlobal.self : window;
const navigator = typeof __jsExtractorGlobal.navigator !== "undefined" ? __jsExtractorGlobal.navigator : window.navigator;
const location = typeof __jsExtractorGlobal.location !== "undefined" ? __jsExtractorGlobal.location : window.location;
const XMLHttpRequest = typeof __jsExtractorGlobal.XMLHttpRequest !== "undefined" ? __jsExtractorGlobal.XMLHttpRequest : class { open(){} send(){} setRequestHeader(){} abort(){} addEventListener(){} getAllResponseHeaders(){return ""} };
"#;

/// One extracted player script. The QuickJS context that loaded it is kept
/// alive so per-URL decipher calls only evaluate the small recipe.
struct LoadedPlayer {
    player_id: String,
    signature_timestamp: u64,
    ctx: Context,
    failed: bool,
}

/// Cached decipher engine. One QuickJS runtime for the process; one context
/// per player script. `Send + Sync` via rquickjs's `parallel` feature.
pub struct Decipherer {
    runtime: Runtime,
    loaded: Mutex<Option<LoadedPlayer>>,
}

impl Decipherer {
    pub fn new() -> Result<Self> {
        let runtime = Runtime::new().map_err(|e| YtError::Parse(format!("quickjs runtime: {e}")))?;
        Ok(Self { runtime, loaded: Mutex::new(None) })
    }

    /// Build a decipherer from an already-fetched player script (the test
    /// path, and for UIs that cache the player JS themselves). Same
    /// extraction + eval as the networked path.
    pub fn from_source(player_id: &str, source: &str) -> Result<Self> {
        let this = Self::new()?;
        this.load_player(player_id, source)?;
        Ok(this)
    }

    /// Extract + evaluate one player script into a fresh context.
    fn load_player(&self, player_id: &str, source: &str) -> Result<()> {
        let extracted = extract::build_script(source)
            .map_err(|e| YtError::Parse(format!("player extraction: {e}")))?;
        let ctx = Context::full(&self.runtime)
            .map_err(|e| YtError::Parse(format!("quickjs context: {e}")))?;
        // Evaluate the closure once; the export lands on the global object.
        let full = format!(
            "{PREAMBLE}\nconst exportedVars = (function(g){{\n{}\n; return {{ nsigFunction: {} }};\n}})({{}});",
            extracted.script, extracted.nsig_name
        );
        ctx.with(|ctx| {
            let result: std::result::Result<(), rquickjs::CaughtError> =
                rquickjs::CatchResultExt::catch(ctx.eval::<(), _>(full.as_bytes()), &ctx);
            result.map_err(|e| YtError::Parse(format!("player eval: {e}")))
        })?;
        let loaded = LoadedPlayer {
            player_id: player_id.to_string(),
            signature_timestamp: extracted.signature_timestamp,
            ctx,
            failed: false,
        };
        *self
            .loaded
            .lock()
            .map_err(|_| YtError::Parse("decipherer lock poisoned".into()))? = Some(loaded);
        Ok(())
    }

    /// The current player's `signatureTimestamp` (0 when nothing loaded yet).
    pub fn signature_timestamp(&self) -> u64 {
        self.loaded
            .lock()
            .ok()
            .and_then(|g| g.as_ref().map(|p| p.signature_timestamp))
            .unwrap_or(0)
    }

    /// Ensure a player script is loaded for `player_id` (or the current one
    /// from the iframe API when `None`), refetching after a failed player.
    async fn ensure_player(
        &self,
        http: &reqwest::Client,
        player_id: Option<&str>,
    ) -> Result<()> {
        {
            let guard = self
                .loaded
                .lock()
                .map_err(|_| YtError::Parse("decipherer lock poisoned".into()))?;
            if let Some(p) = guard.as_ref() {
                let wanted = player_id.unwrap_or(p.player_id.as_str());
                if p.player_id == wanted && !p.failed {
                    return Ok(());
                }
            }
        }
        let id = match player_id {
            Some(id) => id.to_string(),
            None => fetch_player_id(http).await?,
        };
        let source = fetch_player_js(http, &id).await?;
        self.load_player(&id, &source)?;
        Ok(())
    }

    /// Transform one (n, sp, s) triple through the loaded player, using
    /// youtubei.js's `getNsigProcessorFn` recipe verbatim. Returns
    /// `(sig, n)` — either may be `None` when the input had none. On any
    /// eval failure the loaded player is marked for a rotation refetch.
    pub fn transform(&self, n: Option<&str>, sp: Option<&str>, s: Option<&str>) -> Result<(Option<String>, Option<String>)> {
        // Inputs are base64url-ish alphabet strings — safe to embed as string
        // literals, same as youtubei.js does.
        let n = n.unwrap_or("");
        let sp = sp.unwrap_or("");
        let s = s.unwrap_or("");
        let recipe = format!(
            r#"(function() {{
  function process(n = "", sp = "", s = "") {{
    const mockStreamingURL = "https://ytjs.googlevideo.com/videoplayback?expire=1234567890&"+"n="+encodeURIComponent(n);
    const urlCtorFunction = exportedVars.nsigFunction || (() => {{ throw new Error('No n/sig decipher function extracted') }});
    const urlCtor = urlCtorFunction(mockStreamingURL, sp, s);
    const proto = Object.getPrototypeOf(urlCtor);
    const properties = Object.getOwnPropertyNames(proto);
    const methodBlacklist = ['constructor', 'clone', 'set', 'get'];
    for (const prop of properties) {{
      if (methodBlacklist.includes(prop)) continue;
      if (typeof urlCtor[prop] === 'function') urlCtor[prop]();
    }}
    const sigResult = urlCtor.get(sp);
    const nResult = urlCtor.get('n');
    return JSON.stringify({{
      sig: sigResult ? decodeURIComponent(sigResult) : null,
      n: nResult ? decodeURIComponent(nResult) : null
    }});
  }}
  return process("{n}", "{sp}", "{s}");
}})()"#
        );
        let guard = self
            .loaded
            .lock()
            .map_err(|_| YtError::Parse("decipherer lock poisoned".into()))?;
        let Some(player) = guard.as_ref() else {
            return Err(YtError::Parse("decipherer has no player loaded".into()));
        };
        let result: std::result::Result<String, _> = player
            .ctx
            .with(|ctx| ctx.eval::<String, _>(recipe.as_bytes()));
        let json = match result {
            Ok(json) => json,
            Err(e) => {
                // Mark for rotation so the next ensure_player refetches.
                drop(guard);
                if let Ok(mut g) = self.loaded.lock() {
                    if let Some(p) = g.as_mut() {
                        p.failed = true;
                    }
                }
                return Err(YtError::Parse(format!("decipher eval: {e}")));
            }
        };
        let parsed: serde_json::Value = serde_json::from_str(&json)
            .map_err(|e| YtError::Parse(format!("decipher result: {e}")))?;
        let sig = parsed.get("sig").and_then(|v| v.as_str()).map(str::to_string);
        let n = parsed.get("n").and_then(|v| v.as_str()).map(str::to_string);
        if let Some(n) = &n {
            if n.starts_with("enhanced_except_") {
                return Err(YtError::Parse(format!("decipher returned an error marker: {n}")));
            }
        }
        Ok((sig, n))
    }

    /// Port of `Player.decipher` for a `signatureCipher`/`cipher` string:
    /// `s=…&sp=sig&url=…`. Recovers the direct URL with the transformed
    /// signature, transformed `n` and the caller's client version stamped in.
    /// A decipher failure refetches the player once and retries (rotation).
    pub async fn decipher_cipher(
        &self,
        http: &reqwest::Client,
        cipher: &str,
        client_version: &str,
    ) -> Result<String> {
        self.ensure_player(http, None).await?;
        match self.decipher_cipher_sync(cipher, client_version) {
            Ok(url) => Ok(url),
            Err(first) => {
                self.force_refetch(http).await?;
                self.decipher_cipher_sync(cipher, client_version)
                    .map_err(|e| YtError::Parse(format!("decipher failed after player refetch: {e} | first: {first}")))
            }
        }
    }

    /// Port of `Player.decipher` for a plain direct URL that only needs its
    /// throttling `n` transformed.
    pub async fn decipher_url(
        &self,
        http: &reqwest::Client,
        url: &str,
        client_version: &str,
    ) -> Result<String> {
        self.ensure_player(http, None).await?;
        match self.decipher_url_sync(url, client_version) {
            Ok(u) => Ok(u),
            Err(first) => {
                self.force_refetch(http).await?;
                self.decipher_url_sync(url, client_version)
                    .map_err(|e| YtError::Parse(format!("n-transform failed after refetch: {e} | first: {first}")))
            }
        }
    }

    /// Decipher one `signatureCipher` with the loaded player (sync — the
    /// async wrapper only adds the player fetch + rotation retry).
    pub fn decipher_cipher_sync(&self, cipher: &str, client_version: &str) -> Result<String> {
        let params = parse_query_pairs(cipher);
        let find = |name: &str| params.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone());
        let s = find("s").map(|v| percent_decode(&v));
        let sp = find("sp");
        let url = find("url")
            .map(|v| percent_decode(&v))
            .ok_or_else(|| YtError::Parse("cipher has no url param".into()))?;
        let n = query_get(&url, "n");
        if s.is_none() && n.is_none() {
            return Ok(url); // nothing to transform
        }
        let (sig, n_out) = self.transform(n.as_deref(), sp.as_deref(), s.as_deref())?;
        let mut url = url;
        if let Some(sig) = sig {
            let key = sp.as_deref().unwrap_or("signature");
            url = set_query_param(&url, key, &sig);
        }
        if let Some(n) = n_out {
            url = set_query_param(&url, "n", &n);
        }
        Ok(stamp_client_version(&url, client_version))
    }

    /// Transform the `n` of one direct URL with the loaded player (sync).
    pub fn decipher_url_sync(&self, url: &str, client_version: &str) -> Result<String> {
        let Some(n) = query_get(url, "n") else {
            return Ok(stamp_client_version(url, client_version));
        };
        let (_, n_out) = self.transform(Some(&n), None, None)?;
        let mut url = url.to_string();
        if let Some(n) = n_out {
            url = set_query_param(&url, "n", &n);
        }
        Ok(stamp_client_version(&url, client_version))
    }

    async fn force_refetch(&self, http: &reqwest::Client) -> Result<String> {
        {
            let mut guard = self
                .loaded
                .lock()
                .map_err(|_| YtError::Parse("decipherer lock poisoned".into()))?;
            *guard = None; // drop the failed context
        }
        let id = fetch_player_id(http).await?;
        self.ensure_player(http, Some(&id)).await?;
        Ok(id)
    }
}

async fn fetch_player_id(http: &reqwest::Client) -> Result<String> {
    let text = http
        .get(IFRAME_API_URL)
        .header("User-Agent", UA_WEB)
        .send()
        .await?
        .error_for_status()
        .map_err(YtError::from)?
        .text()
        .await?;
    // getStringBetweenStrings(js, 'player\/', '\/')
    let id = text
        .split("player\\/")
        .nth(1)
        .and_then(|rest| rest.split("\\/").next())
        .unwrap_or("");
    if id.is_empty() || id.len() > 64 {
        return Err(YtError::Parse("iframe_api carried no player id".into()));
    }
    Ok(id.to_string())
}

async fn fetch_player_js(http: &reqwest::Client, player_id: &str) -> Result<String> {
    let url = PLAYER_JS_URL.replace("{id}", player_id);
    let text = http
        .get(url)
        .header("User-Agent", UA_WEB)
        .send()
        .await?
        .error_for_status()
        .map_err(YtError::from)?
        .text()
        .await?;
    Ok(text)
}

// --- tiny query-string helpers: exact string surgery, never re-encoding ---
// (A signed URL does not survive being normalised; the TS datasource does the
// same with `withSessionClientVersion`.)

/// `name=value` pairs joined by `&`, percent-encoded values (URLSearchParams
/// semantics for a cipher string).
fn parse_query_pairs(s: &str) -> Vec<(String, String)> {
    s.split('&')
        .filter_map(|pair| pair.split_once('='))
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(h), Some(l)) = (hex_val(bytes[i + 1]), hex_val(bytes[i + 2])) {
                out.push(h * 16 + l);
                i += 3;
                continue;
            }
        }
        if bytes[i] == b'+' {
            out.push(b' ');
        } else {
            out.push(bytes[i]);
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn percent_encode(s: &str) -> String {
    // The values we write back (sig/n transforms, client versions) are all
    // alphanumeric plus `-._~*`; everything else gets escaped.
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_' | '~' | '*') {
            out.push(c);
        } else {
            out.push_str(&format!("%{:02X}", c as u32 as u8));
        }
    }
    out
}

fn hex_val(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

fn query_get(url: &str, key: &str) -> Option<String> {
    let q = url.split_once('?')?.1;
    for pair in q.split('&') {
        let (k, v) = pair.split_once('=')?;
        if k == key {
            return Some(percent_decode(v));
        }
    }
    None
}

/// Replace (or append) `key=value` in the query, percent-encoding the value.
/// String surgery on the raw URL — re-serialising would re-encode values
/// that are already percent-exact.
fn set_query_param(url: &str, key: &str, value: &str) -> String {
    let encoded = percent_encode(value);
    let (base, query) = match url.split_once('?') {
        Some((b, q)) => (b, q),
        None => return format!("{url}?{key}={encoded}"),
    };
    let mut pairs: Vec<String> = query.split('&').map(str::to_string).collect();
    let target = format!("{key}=");
    let mut replaced = false;
    for pair in pairs.iter_mut() {
        if pair == key || pair.starts_with(&target) {
            *pair = format!("{key}={encoded}");
            replaced = true;
            break;
        }
    }
    if !replaced {
        pairs.push(format!("{key}={encoded}"));
    }
    format!("{base}?{}", pairs.join("&"))
}

/// `withSessionClientVersion` + youtubei.js's client switch, merged: the
/// deciphered URL's `cver` must say the version the session actually used —
/// added when the cipher omitted it (youtubei.js's `c` switch), rewritten
/// when it disagrees (the TS `withSessionClientVersion`). `cver` is not
/// covered by the signature, which is exactly why stamping it after
/// deciphering is safe (see TS:494-502).
pub fn stamp_client_version(url: &str, client_version: &str) -> String {
    set_query_param(url, "cver", client_version)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_param_surgery() {
        let url = "https://x/videoplayback?expire=1&n=abc&cver=old";
        assert_eq!(set_query_param(url, "n", "new"), "https://x/videoplayback?expire=1&n=new&cver=old");
        assert_eq!(set_query_param(url, "sig", "A.B"), "https://x/videoplayback?expire=1&n=abc&cver=old&sig=A.B");
        assert_eq!(stamp_client_version(url, "1.20250506.00.00"), "https://x/videoplayback?expire=1&n=abc&cver=1.20250506.00.00");
    }

    #[test]
    fn percent_round_trip() {
        assert_eq!(percent_decode("https%3A%2F%2Fx%26y"), "https://x&y");
        assert_eq!(percent_decode("a+b"), "a b");
        assert_eq!(percent_encode("1.20250506.00.00"), "1.20250506.00.00");
    }

    #[test]
    fn cipher_params_parse() {
        let pairs = parse_query_pairs("s=abc&sp=sig&url=https%3A%2F%2Fx%3Fid%3D1");
        assert_eq!(pairs.iter().find(|(k, _)| k == "s").unwrap().1, "abc");
        assert_eq!(pairs.iter().find(|(k, _)| k == "url").unwrap().1, "https%3A%2F%2Fx%3Fid%3D1");
        assert_eq!(percent_decode(pairs.iter().find(|(k, _)| k == "url").unwrap().1.as_str()), "https://x?id=1");
    }
}
