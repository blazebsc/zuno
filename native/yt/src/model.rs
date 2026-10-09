//! Plain data returned by the typed API. No serde derives needed by UIs —
//! parsing lives in [`crate::parse`] over `serde_json::Value`, so response
//! shape drift only touches one module.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct YtThumb {
    pub url: String,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct YtArtistRef {
    pub name: String,
    pub channel_id: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct YtAlbumRef {
    pub name: String,
    pub browse_id: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct YtTrack {
    pub video_id: String,
    pub title: String,
    pub artists: Vec<YtArtistRef>,
    pub album: Option<YtAlbumRef>,
    pub duration_sec: u32,
    pub thumbnails: Vec<YtThumb>,
    pub explicit: bool,
    pub playlist_id: Option<String>,
    /// The playlist *row* id (`playlistSetVideoId`), not the song id — the
    /// same song can appear twice in a list, so remove/reorder address rows.
    /// Present on playlist pages only.
    pub set_video_id: Option<String>,
}

impl YtTrack {
    pub fn artist_names(&self) -> String {
        self.artists.iter().map(|a| a.name.as_str()).collect::<Vec<_>>().join(", ")
    }

    /// Largest thumbnail URL, if any.
    pub fn artwork_url(&self) -> Option<&str> {
        crate::artwork::pick_thumb(&self.thumbnails)
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct YtAlbum {
    pub browse_id: String,
    pub title: String,
    pub artist: String,
    pub artist_channel_id: String,
    pub year: Option<u16>,
    pub kind: String,
    pub thumbnails: Vec<YtThumb>,
    pub tracks: Vec<YtTrack>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct YtArtist {
    pub channel_id: String,
    pub name: String,
    pub subscriber_count: Option<String>,
    /// Subscription state when the signed-in account is subscribed
    /// (`subscribeButtonRenderer.subscribed`); `None` when the response
    /// carries no subscribe button (unsigned browse).
    pub subscribed: Option<bool>,
    pub thumbnails: Vec<YtThumb>,
    pub top_tracks: Vec<YtTrack>,
    pub albums: Vec<YtAlbumRefFull>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct YtAlbumRefFull {
    pub browse_id: String,
    pub title: String,
    pub year: Option<u16>,
    pub kind: String,
    pub thumbnails: Vec<YtThumb>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct YtPlaylist {
    pub id: String,
    pub title: String,
    pub description: Option<String>,
    pub thumbnails: Vec<YtThumb>,
    pub tracks: Vec<YtTrack>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct YtSearchResults {
    pub query: String,
    pub tracks: Vec<YtTrack>,
    pub albums: Vec<YtAlbumRefFull>,
    pub artists: Vec<YtArtistHeader>,
    pub playlists: Vec<YtPlaylistHeader>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct YtArtistHeader {
    pub channel_id: String,
    pub name: String,
    pub thumbnails: Vec<YtThumb>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct YtPlaylistHeader {
    pub id: String,
    pub title: String,
    pub thumbnails: Vec<YtThumb>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct YtShelf {
    pub title: String,
    pub tracks: Vec<YtTrack>,
    pub albums: Vec<YtAlbumRefFull>,
    pub playlists: Vec<YtPlaylistHeader>,
    pub artists: Vec<YtArtistHeader>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct YtHome {
    pub shelves: Vec<YtShelf>,
}

/// One page of a paginated track list — the app's `TrackPage` shape
/// (`getPlaylistTrackPage`), fed to [`crate::api::collect_track_pages`].
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct YtTrackPage {
    pub tracks: Vec<YtTrack>,
    pub has_more: bool,
    pub next_page_key: Option<String>,
}
