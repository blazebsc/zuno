//! Artwork bytes + candidate picking. Same rule as TS `artwork.ts`
//! `selectArtworkUrl`: largest area wins.

use crate::error::{Result, YtError};
use crate::model::YtThumb;

const MAX_BYTES: usize = 8 * 1024 * 1024;

/// Largest candidate by pixel area.
pub fn pick(candidates: &[YtThumb]) -> Option<&str> {
    candidates
        .iter()
        .filter(|c| !c.url.trim().is_empty())
        .max_by_key(|c| c.width as u64 * c.height as u64)
        .map(|c| c.url.as_str())
}

pub fn pick_thumb(candidates: &[YtThumb]) -> Option<&str> {
    pick(candidates)
}

/// Plain GET with a browser UA, 10 s timeout, 8 MiB cap. Async variant for
/// runtimes that have one; [`fetch_blocking`] for the sync cache path.
pub async fn fetch_async(client: &reqwest::Client, url: &str) -> Result<Vec<u8>> {
    let bytes = client
        .get(url)
        .header("User-Agent", crate::client::UA_WEB)
        .send()
        .await?
        .error_for_status()
        .map_err(|e| YtError::Http(e.to_string()))?
        .bytes()
        .await?;
    if bytes.len() > MAX_BYTES {
        return Err(YtError::Parse("artwork exceeds 8 MiB cap".into()));
    }
    Ok(bytes.to_vec())
}

/// `fetch(url) -> Vec<u8>`: blocking, for [`zuno_core::ArtworkCache::get_url`].
pub fn fetch_blocking(url: &str) -> Result<Vec<u8>> {
    let bytes = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()?
        .get(url)
        .header("User-Agent", crate::client::UA_WEB)
        .send()?
        .error_for_status()
        .map_err(|e| YtError::Http(e.to_string()))?
        .bytes()?;
    if bytes.len() > MAX_BYTES {
        return Err(YtError::Parse("artwork exceeds 8 MiB cap".into()));
    }
    Ok(bytes.to_vec())
}

/// Blocking fetch with the crate's shared async client shape, kept for the
/// doc recipe (`fetch` in the integration recipe means this).
pub fn fetch(url: &str) -> Result<Vec<u8>> {
    fetch_blocking(url)
}
