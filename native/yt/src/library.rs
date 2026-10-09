//! `YtLibrary`: the adapter that fills core's [`zuno_core::library::Library`]
//! from `yt` responses. Core stays network-free; the adapter lives here and
//! depends on core — never the reverse.
//!
//! Usage (no `AppState` changes needed):
//! ```ignore
//! let mut app = AppState::new();
//! app.library = YtLibrary::from_search(&results);
//! ```
//!
//! Real titles/artists/durations come through; `video_id` is set on every
//! track, `artwork_url` wherever YouTube served a thumbnail. Fields with no
//! YouTube answer stay neutral: `liked: false`, `monthly_listeners: 0`,
//! `year: 0` when unknown.

use crate::model::*;
use std::collections::HashMap;
use zuno_core::library::Library;
use zuno_core::model::{Album, AlbumId, AlbumKind, Artist, ArtistId, Playlist, Track, TrackId};

pub struct YtLibrary;

impl YtLibrary {
    pub fn from_tracks(title: &str, tracks: &[YtTrack]) -> Library {
        build(title, tracks)
    }

    pub fn from_search(results: &YtSearchResults) -> Library {
        build(&format!("Results for “{}”", results.query), &results.tracks)
    }

    pub fn from_home(home: &YtHome) -> Library {
        let tracks: Vec<YtTrack> =
            home.shelves.iter().flat_map(|s| s.tracks.clone()).collect();
        build("Home", &tracks)
    }

    pub fn from_album(album: &YtAlbum) -> Library {
        build(&album.title, &album.tracks)
    }

    pub fn from_playlist(playlist: &YtPlaylist) -> Library {
        build(&playlist.title, &playlist.tracks)
    }

    pub fn from_shelf(shelf: &YtShelf) -> Library {
        build(&shelf.title, &shelf.tracks)
    }
}

fn build(list_title: &str, tracks: &[YtTrack]) -> Library {
    let mut artists: Vec<Artist> = Vec::new();
    let mut artist_idx: HashMap<String, ArtistId> = HashMap::new();
    let mut albums: Vec<Album> = Vec::new();
    let mut album_idx: HashMap<String, AlbumId> = HashMap::new();
    let mut out_tracks: Vec<Track> = Vec::with_capacity(tracks.len());

    for (i, yt) in tracks.iter().enumerate() {
        let artist_name = if yt.artist_names().is_empty() {
            "Unknown artist".to_string()
        } else {
            yt.artist_names()
        };
        let artist_id = *artist_idx.entry(artist_name.clone()).or_insert_with(|| {
            let id = artists.len() as ArtistId;
            artists.push(Artist {
                id,
                name: artist_name.clone(),
                monthly_listeners: 0,
                album_ids: Vec::new(),
                artwork_url: None,
            });
            id
        });

        let (album_title, album_key, album_art) = match &yt.album {
            Some(a) => (a.name.clone(), format!("mp:{}\0{a}", a.browse_id, a = a.name), None),
            None => {
                let t = format!("{list_title} — {artist_name}");
                (t.clone(), format!("loose:{t}"), yt.artwork_url().map(str::to_string))
            }
        };
        let album_id = *album_idx.entry(album_key).or_insert_with(|| {
            let id = albums.len() as AlbumId;
            albums.push(Album {
                id,
                title: album_title.clone(),
                artist_id,
                year: 0,
                track_ids: Vec::new(),
                kind: AlbumKind::Album,
                artwork_url: album_art.clone(),
            });
            artists[artist_id as usize].album_ids.push(id);
            id
        });
        // Tracks from the same album share the album's first artwork.
        if albums[album_id as usize].artwork_url.is_none() {
            albums[album_id as usize].artwork_url = yt.artwork_url().map(str::to_string);
        }

        let id = out_tracks.len() as TrackId;
        albums[album_id as usize].track_ids.push(id);
        out_tracks.push(Track {
            id,
            title: yt.title.clone(),
            artist_id,
            album_id,
            track_no: (albums[album_id as usize].track_ids.len()) as u32,
            duration_sec: yt.duration_sec,
            explicit: yt.explicit,
            liked: false,
            video_id: yt.video_id.clone(),
            artwork_url: yt.artwork_url().map(str::to_string),
        });
        let _ = i;
    }

    // Artist portraits: first track artwork seen for that artist.
    for t in &out_tracks {
        let a = &mut artists[t.artist_id as usize];
        if a.artwork_url.is_none() {
            a.artwork_url = t.artwork_url.clone();
        }
    }

    let all_ids: Vec<TrackId> = out_tracks.iter().map(|t| t.id).collect();
    let recent: Vec<TrackId> = all_ids.iter().take(24).copied().collect();
    Library {
        tracks: out_tracks,
        albums,
        artists,
        playlists: vec![Playlist {
            id: 0,
            title: list_title.to_string(),
            description: None,
            track_ids: all_ids,
            system: true,
            artwork_url: None,
        }],
        mixes: Vec::new(),
        recently_played: recent,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(video_id: &str, title: &str, artist: &str) -> YtTrack {
        YtTrack {
            video_id: video_id.into(),
            title: title.into(),
            artists: vec![YtArtistRef { name: artist.into(), channel_id: "UCx".into() }],
            album: Some(YtAlbumRef { name: "Alb".into(), browse_id: "MPRE1".into() }),
            duration_sec: 200,
            thumbnails: vec![YtThumb { url: "https://i.ytimg.com/vi/x/hqdefault.jpg".into(), width: 400, height: 225 }],
            explicit: false,
            playlist_id: None,
        }
    }

    #[test]
    fn adapter_sets_video_id_and_artwork() {
        let lib = YtLibrary::from_tracks("T", &[track("ABCDEFGHIJK", "Creep", "Radiohead")]);
        assert_eq!(lib.tracks.len(), 1);
        assert_eq!(lib.tracks[0].video_id, "ABCDEFGHIJK");
        assert!(lib.tracks[0].artwork_url.is_some());
        assert_eq!(lib.artists.len(), 1);
        assert_eq!(lib.albums.len(), 1);
        // AppState-compatible: library swaps wholesale.
        assert_eq!(lib.playlists.len(), 1);
        assert_eq!(lib.recently_played.len(), 1);
    }
}
