//! Authenticated mutations: ratings, playlist edits, subscriptions.
//!
//! Thin wrappers over the same signed POST path, with request bodies ported
//! verbatim from the TS datasource (`executeTrackRatingCommand`,
//! `executePlaylistLibraryLikeCommand`, `removeTrackFromPlaylist`,
//! `setArtistSubscribed`) and the youtubei.js endpoints they delegate to
//! (`playlist/create`, `browse/edit_playlist`, `playlist/delete`,
//! `like/like|dislike|removelike`, `subscription/subscribe|unsubscribe`).
//!
//! All of these need a signed-in session — `YtError::NotSignedIn` otherwise.

use crate::client::*;
use crate::error::{Result, YtError};
use crate::YtClient;
use serde_json::{json, Value};

const MUSIC_LIKE_URL: &str = "https://music.youtube.com/youtubei/v1/like";
const MUSIC_NEXT_URL: &str = "https://music.youtube.com/youtubei/v1/next";
const MUSIC_PLAYLIST_CREATE_URL: &str = "https://music.youtube.com/youtubei/v1/playlist/create";
const MUSIC_PLAYLIST_DELETE_URL: &str = "https://music.youtube.com/youtubei/v1/playlist/delete";
const MUSIC_EDIT_PLAYLIST_URL: &str = "https://music.youtube.com/youtubei/v1/browse/edit_playlist";
const MUSIC_SUBSCRIPTION_URL: &str = "https://music.youtube.com/youtubei/v1/subscription";

/// The three ratings YouTube has for a track (the app's `TrackRating`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Rating {
    Like,
    Dislike,
    /// "Set rating to indifferent" — clears a like *and* a dislike.
    None,
}

impl Rating {
    /// The `likeEndpoint.status` the command is looked up by.
    fn status(self) -> &'static str {
        match self {
            Rating::Like => "LIKE",
            Rating::Dislike => "DISLIKE",
            Rating::None => "INDIFFERENT",
        }
    }

    fn api_path(self) -> &'static str {
        match self {
            Rating::Like => "like/like",
            Rating::Dislike => "like/dislike",
            Rating::None => "like/removelike",
        }
    }
}

/// `likeParams` / `dislikeParams` / `removeLikeParams` per rating — the
/// `/next` response carries one endpoint per status, each with its own params.
fn params_key(rating: Rating) -> &'static str {
    match rating {
        Rating::Like => "likeParams",
        Rating::Dislike => "dislikeParams",
        Rating::None => "removeLikeParams",
    }
}

// --- request shapes, pure and fixture-testable offline ---

/// The `/like/{path}` body from a resolved likeEndpoint.
pub fn track_rating_body(endpoint: &Value, rating: Rating) -> Value {
    let target = endpoint
        .get("target")
        .cloned()
        .expect("likeEndpoint.target");
    let mut body = json!({ "target": normalize_like_target(&target) });
    if let Some(params) = endpoint
        .get(params_key(rating))
        .or_else(|| endpoint.get("params"))
        .and_then(|p| p.as_str())
    {
        body["params"] = json!(params);
    }
    body
}

/// The `browse/edit_playlist` body for one playlist mutation.
pub fn playlist_edit_body(playlist_id: &str, action: Value) -> Value {
    json!({
        "playlistId": editable_playlist_id(playlist_id),
        "actions": [ action ],
    })
}

/// The `/subscription/{endpoint}` body.
pub fn subscription_body(channel_id: &str, subscribed: bool) -> Value {
    json!({
        "channelIds": [ channel_id ],
        "params": if subscribed { "EgIIAhgA" } else { "CgIIAhgA" },
    })
}

/// The `/like/{like,removelike}` body for saving a playlist to the library
/// — the body is the same for both; the path carries the direction.
pub fn playlist_saved_body(playlist_id: &str) -> Value {
    json!({ "target": { "playlistId": editable_playlist_id(playlist_id) } })
}

/// Recursive search for the first `likeEndpoint` whose `status` matches —
/// `findLikeEndpoint` in the TS datasource.
fn find_like_endpoint(v: &Value, status: &str) -> Option<Value> {
    match v {
        Value::Object(map) => {
            if let Some(ep) = map.get("likeEndpoint") {
                if ep.get("status").and_then(|s| s.as_str()) == Some(status) {
                    return Some(ep.clone());
                }
            }
            map.values().find_map(|c| find_like_endpoint(c, status))
        }
        Value::Array(items) => items.iter().find_map(|c| find_like_endpoint(c, status)),
        _ => None,
    }
}

/// `normalizeLikeTarget`: a string target is a playlist id when it starts
/// `PL`/`OLAK5uy_`, a video id otherwise.
fn normalize_like_target(target: &Value) -> Value {
    match target.as_str() {
        Some(s) if s.starts_with("PL") || s.starts_with("OLAK5uy_") => json!({ "playlistId": s }),
        Some(s) => json!({ "videoId": s }),
        None => target.clone(),
    }
}

/// `editablePlaylistId`: strip the `VL` browse prefix.
fn editable_playlist_id(id: &str) -> &str {
    id.strip_prefix("VL").unwrap_or(id)
}

/// Mutations, implemented on [`YtClient`].
#[allow(async_fn_in_trait)]
pub trait YtMutations {
    /// Like, dislike or clear the rating for one track.
    async fn set_track_rating(&mut self, video_id: &str, rating: Rating) -> Result<()>;
    /// Create a playlist (optionally seeding it with tracks); returns its id.
    async fn create_playlist(&mut self, title: &str, video_ids: &[String]) -> Result<String>;
    /// Rename a playlist.
    async fn rename_playlist(&mut self, playlist_id: &str, title: &str) -> Result<()>;
    /// Delete a playlist.
    async fn delete_playlist(&mut self, playlist_id: &str) -> Result<()>;
    /// Add a track to a playlist.
    async fn add_track_to_playlist(&mut self, playlist_id: &str, video_id: &str) -> Result<()>;
    /// Remove a playlist row by *set video id* — the row id, not the song id,
    /// because the same song can appear twice.
    async fn remove_track_from_playlist(&mut self, playlist_id: &str, set_video_id: &str) -> Result<()>;
    /// Move a playlist row to sit directly after `predecessor_set_video_id`,
    /// or to the front when it is empty.
    async fn reorder_playlist_tracks(
        &mut self,
        playlist_id: &str,
        moved_set_video_id: &str,
        predecessor_set_video_id: Option<&str>,
    ) -> Result<()>;
    /// Save a playlist to (or remove it from) the library — the `like`
    /// endpoint with a playlist target.
    async fn set_playlist_saved(&mut self, playlist_id: &str, saved: bool) -> Result<()>;
    /// Subscribe to (or unsubscribe from) an artist.
    async fn set_artist_subscribed(&mut self, channel_id: &str, subscribed: bool) -> Result<()>;
}

impl YtMutations for YtClient {
    /// One of YouTube's three ratings for a track. The status doubles as the
    /// lookup key: `/next` carries a `likeEndpoint` per rating and the one
    /// whose `status` matches is the command to run.
    async fn set_track_rating(&mut self, video_id: &str, rating: Rating) -> Result<()> {
        let body = post_body(web_remix_context(), json!({ "videoId": video_id }));
        let next = self
            .post_signed(
                &url(MUSIC_NEXT_URL),
                CLIENT_WEB_REMIX,
                "1.20250506.00.00",
                UA_WEB,
                body,
            )
            .await?;
        let endpoint = find_like_endpoint(&next, rating.status())
            .ok_or_else(|| YtError::Parse(format!("no {} command for {video_id}", rating.status())))?;
        let body = track_rating_body(&endpoint, rating);
        let path = match rating.api_path() {
            "like/like" => format!("{MUSIC_LIKE_URL}/like"),
            "like/dislike" => format!("{MUSIC_LIKE_URL}/dislike"),
            _ => format!("{MUSIC_LIKE_URL}/removelike"),
        };
        let resp = self
            .post_signed(
                &url(&path),
                CLIENT_WEB_REMIX,
                "1.20250506.00.00",
                UA_WEB,
                body,
            )
            .await?;
        check_success(&resp)
    }

    async fn create_playlist(&mut self, title: &str, video_ids: &[String]) -> Result<String> {
        let mut body = json!({ "title": title });
        if !video_ids.is_empty() {
            body["videoIds"] = json!(video_ids);
        }
        let resp = self
            .post_signed(
                &url(MUSIC_PLAYLIST_CREATE_URL),
                CLIENT_WEB_REMIX,
                "1.20250506.00.00",
                UA_WEB,
                body,
            )
            .await?;
        check_success(&resp)?;
        resp.get("playlistId")
            .and_then(|p| p.as_str())
            .map(str::to_string)
            .ok_or_else(|| YtError::Parse("playlist/create returned no playlistId".into()))
    }

    async fn rename_playlist(&mut self, playlist_id: &str, title: &str) -> Result<()> {
        let body = playlist_edit_body(playlist_id, json!({ "action": "ACTION_SET_PLAYLIST_NAME", "playlistName": title }));
        let resp = self
            .post_signed(
                &url(MUSIC_EDIT_PLAYLIST_URL),
                CLIENT_WEB_REMIX,
                "1.20250506.00.00",
                UA_WEB,
                body,
            )
            .await?;
        check_success(&resp)
    }

    async fn delete_playlist(&mut self, playlist_id: &str) -> Result<()> {
        // The app's note: youtubei.js 17's playlist.delete() loses the API
        // path, so the same mutation is executed directly.
        let body = json!({ "playlistId": editable_playlist_id(playlist_id) });
        let resp = self
            .post_signed(
                &url(MUSIC_PLAYLIST_DELETE_URL),
                CLIENT_WEB_REMIX,
                "1.20250506.00.00",
                UA_WEB,
                body,
            )
            .await?;
        check_success(&resp)
    }

    async fn add_track_to_playlist(&mut self, playlist_id: &str, video_id: &str) -> Result<()> {
        let body = playlist_edit_body(playlist_id, json!({ "action": "ACTION_ADD_VIDEO", "addedVideoId": video_id }));
        let resp = self
            .post_signed(
                &url(MUSIC_EDIT_PLAYLIST_URL),
                CLIENT_WEB_REMIX,
                "1.20250506.00.00",
                UA_WEB,
                body,
            )
            .await?;
        check_success(&resp)
    }

    async fn remove_track_from_playlist(&mut self, playlist_id: &str, set_video_id: &str) -> Result<()> {
        let body = playlist_edit_body(playlist_id, json!({ "action": "ACTION_REMOVE_VIDEO", "setVideoId": set_video_id }));
        let resp = self
            .post_signed(
                &url(MUSIC_EDIT_PLAYLIST_URL),
                CLIENT_WEB_REMIX,
                "1.20250506.00.00",
                UA_WEB,
                body,
            )
            .await?;
        check_success(&resp)
    }

    async fn reorder_playlist_tracks(
        &mut self,
        playlist_id: &str,
        moved_set_video_id: &str,
        predecessor_set_video_id: Option<&str>,
    ) -> Result<()> {
        let body = playlist_edit_body(playlist_id, json!({
            "action": "ACTION_MOVE_VIDEO_AFTER",
            "setVideoId": moved_set_video_id,
            "movedSetVideoIdPredecessor": predecessor_set_video_id.unwrap_or(""),
        }));
        let resp = self
            .post_signed(
                &url(MUSIC_EDIT_PLAYLIST_URL),
                CLIENT_WEB_REMIX,
                "1.20250506.00.00",
                UA_WEB,
                body,
            )
            .await?;
        check_success(&resp)
    }

    /// `executePlaylistLibraryLikeCommand`: the `like` endpoint with a
    /// playlist target is the save-to-library command.
    async fn set_playlist_saved(&mut self, playlist_id: &str, saved: bool) -> Result<()> {
        let path = if saved { "like" } else { "removelike" };
        let body = playlist_saved_body(playlist_id);
        let resp = self
            .post_signed(
                &url(&format!("{MUSIC_LIKE_URL}/{path}")),
                CLIENT_WEB_REMIX,
                "1.20250506.00.00",
                UA_WEB,
                body,
            )
            .await?;
        check_success(&resp)
    }

    /// `setArtistSubscribed` verbatim: `/subscription/{subscribe|unsubscribe}`
    /// with the fixed params and the channel id.
    async fn set_artist_subscribed(&mut self, channel_id: &str, subscribed: bool) -> Result<()> {
        let endpoint = if subscribed { "subscribe" } else { "unsubscribe" };
        let body = subscription_body(channel_id, subscribed);
        let resp = self
            .post_signed(
                &url(&format!("{MUSIC_SUBSCRIPTION_URL}/{endpoint}")),
                CLIENT_WEB_REMIX,
                "1.20250506.00.00",
                UA_WEB,
                body,
            )
            .await?;
        check_success(&resp)
    }
}

fn url(base: &str) -> String {
    format!("{base}?key={}&prettyPrint=false", crate::client::MUSIC_KEY)
}

fn post_body(context: Value, extra: Value) -> Value {
    let mut body = json!({ "context": context });
    if let Some(obj) = extra.as_object() {
        for (k, v) in obj {
            body[k] = v.clone();
        }
    }
    body
}

/// youtubei.js's `actions.execute` response shape: `{success, status_code}`
/// arrives alongside the payload; a 401/403 means the session lapsed, not
/// that the action was refused.
fn check_success(resp: &Value) -> Result<()> {
    let success = resp.get("success").and_then(|s| s.as_bool()).unwrap_or(true);
    if success {
        return Ok(());
    }
    Err(YtError::Parse(format!(
        "mutation failed: HTTP {}",
        resp.get("status_code").and_then(|c| c.as_u64()).unwrap_or(0)
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn like_endpoint_lookup() {
        let next = json!({
            "contents": {
                "like": [
                    { "likeEndpoint": { "status": "INDIFFERENT", "target": "vid", "removeLikeParams": "p3" } },
                    { "likeEndpoint": { "status": "LIKE", "target": "vid", "likeParams": "p1" } },
                    { "likeEndpoint": { "status": "DISLIKE", "target": "vid", "dislikeParams": "p2" } },
                ]
            }
        });
        for (rating, want_params) in [(Rating::Like, "p1"), (Rating::Dislike, "p2"), (Rating::None, "p3")] {
            let ep = find_like_endpoint(&next, rating.status()).expect("endpoint found");
            assert_eq!(ep.get(params_key(rating)).and_then(|p| p.as_str()), Some(want_params));
        }
        assert!(find_like_endpoint(&next, "UNHEARD_OF").is_none());
    }

    #[test]
    fn targets_normalize_by_prefix() {
        assert_eq!(normalize_like_target(&json!("XFkzRNyygfk")), json!({ "videoId": "XFkzRNyygfk" }));
        assert_eq!(normalize_like_target(&json!("PLabc123")), json!({ "playlistId": "PLabc123" }));
        assert_eq!(normalize_like_target(&json!("OLAK5uy_x")), json!({ "playlistId": "OLAK5uy_x" }));
        assert_eq!(normalize_like_target(&json!({ "videoId": "v" })), json!({ "videoId": "v" }));
    }

    #[test]
    fn editable_ids_strip_vl() {
        assert_eq!(editable_playlist_id("VLabc"), "abc");
        assert_eq!(editable_playlist_id("abc"), "abc");
    }

    #[test]
    fn success_shape_checked() {
        assert!(check_success(&json!({ "success": true })).is_ok());
        assert!(check_success(&json!({})).is_ok(), "missing success is treated as success");
        assert!(check_success(&json!({ "success": false, "status_code": 403 })).is_err());
    }
}
