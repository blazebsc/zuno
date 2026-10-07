//! Domain types. Mirrors `src/datasource/types.ts` closely enough that the
//! eventual production port keeps its shapes, minus the `source` discriminant
//! (the benchmark has no local-file path).

pub type TrackId = u32;
pub type AlbumId = u32;
pub type ArtistId = u32;
pub type PlaylistId = u32;

#[derive(Clone, Debug)]
pub struct Track {
    pub id: TrackId,
    pub title: String,
    pub artist_id: ArtistId,
    pub album_id: AlbumId,
    /// Position within its album, 1-based.
    pub track_no: u32,
    pub duration_sec: u32,
    pub explicit: bool,
    pub liked: bool,
}

#[derive(Clone, Debug)]
pub struct Album {
    pub id: AlbumId,
    pub title: String,
    pub artist_id: ArtistId,
    pub year: u16,
    pub track_ids: Vec<TrackId>,
    /// "Album" (10+ tracks) or "EP"/"Single" — shown in headers.
    pub kind: AlbumKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlbumKind {
    Album,
    Ep,
    Single,
}

impl AlbumKind {
    pub fn label(self) -> &'static str {
        match self {
            AlbumKind::Album => "Album",
            AlbumKind::Ep => "EP",
            AlbumKind::Single => "Single",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Artist {
    pub id: ArtistId,
    pub name: String,
    pub monthly_listeners: u32,
    pub album_ids: Vec<AlbumId>,
}

#[derive(Clone, Debug)]
pub struct Playlist {
    pub id: PlaylistId,
    pub title: String,
    pub description: Option<String>,
    pub track_ids: Vec<TrackId>,
    /// "Liked Songs" and the generated mixes behave slightly differently in
    /// the UI (fixed artwork, no delete).
    pub system: bool,
}

/// A resolved row — everything a track list row paints, so UIs never join
/// tables themselves. Cheap to copy per row; titles are borrowed by index into
/// the library to keep 5,000 rows light.
#[derive(Clone, Copy, Debug)]
pub struct TrackView<'a> {
    pub id: TrackId,
    pub title: &'a str,
    pub artist: &'a str,
    pub artist_id: ArtistId,
    pub album: &'a str,
    pub album_id: AlbumId,
    pub duration_sec: u32,
    pub explicit: bool,
    pub liked: bool,
}
