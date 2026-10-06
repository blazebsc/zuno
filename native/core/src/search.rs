//! Search — a port of `SearchOverlay`'s `searchMatchScore` and the page-level
//! filtering the app does locally, plus a small casefold-and-rank pass over the
//! whole 5,000-track library (the benchmark's "searching/filtering" workload).

use crate::model::*;
use crate::library::Library;

/// Zuno's scores: exact 4 → prefix 3 → contains 2 → reverse-contains 1.
/// Higher is better; `None` means no match.
pub fn match_score(query: &str, text: &str) -> Option<u8> {
    let q = query.trim();
    if q.is_empty() {
        return None;
    }
    let hay = text.trim();
    let (ql, hl) = (q.to_lowercase(), hay.to_lowercase());
    if ql == hl {
        Some(4)
    } else if hl.starts_with(&ql) {
        Some(3)
    } else if hl.contains(&ql) {
        Some(2)
    } else if ql.contains(&hl) {
        Some(1)
    } else {
        None
    }
}

#[derive(Default)]
pub struct SearchResults {
    /// Best-first, capped for the UI's "Top result" + track list.
    pub tracks: Vec<TrackId>,
    pub albums: Vec<AlbumId>,
    pub artists: Vec<ArtistId>,
    pub playlists: Vec<PlaylistId>,
}

pub const MAX_TRACK_RESULTS: usize = 300;

/// Full-library search. Scores tracks/albums/artists/playlists and returns
/// each bucket best-first. Cheap enough to run per keystroke against 5k rows
/// (the React app filters client-side the same way).
pub fn search(lib: &Library, query: &str) -> SearchResults {
    let mut out = SearchResults::default();
    if query.trim().is_empty() {
        return out;
    }
    // ponytail: one (title, artist) scan per query — no index. A trigram index
    // is the upgrade path if per-keystroke time ever shows up in a profile.
    let mut scored: Vec<(u8, TrackId)> = Vec::new();
    for t in &lib.tracks {
        let best = match_score(query, &t.title).max(match_score(query, &lib.artist(t.artist_id).name));
        if let Some(s) = best {
            scored.push((s, t.id));
        }
    }
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    out.tracks = scored.into_iter().take(MAX_TRACK_RESULTS).map(|(_, id)| id).collect();

    let mut ascored: Vec<(u8, AlbumId)> = lib
        .albums
        .iter()
        .filter_map(|a| match_score(query, &a.title).map(|s| (s, a.id)))
        .collect();
    ascored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    out.albums = ascored.into_iter().take(60).map(|(_, id)| id).collect();

    let mut rscored: Vec<(u8, ArtistId)> = lib
        .artists
        .iter()
        .filter_map(|r| match_score(query, &r.name).map(|s| (s, r.id)))
        .collect();
    rscored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    out.artists = rscored.into_iter().take(30).map(|(_, id)| id).collect();

    let mut pscored: Vec<(u8, PlaylistId)> = lib
        .playlists
        .iter()
        .chain(lib.mixes.iter())
        .filter_map(|p| match_score(query, &p.title).map(|s| (s, p.id)))
        .collect();
    pscored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    out.playlists = pscored.into_iter().take(30).map(|(_, id)| id).collect();
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::Library;

    #[test]
    fn score_order_matches_the_react_app() {
        assert_eq!(match_score("Dopamine", "Dopamine"), Some(4));
        assert_eq!(match_score("dop", "Dopamine"), Some(3));
        assert_eq!(match_score("pami", "Dopamine"), Some(2));
        assert_eq!(match_score("Dopamine Machine", "Dopamine"), Some(1));
        assert_eq!(match_score("xyz", "Dopamine"), None);
        assert_eq!(match_score("", "Dopamine"), None);
    }

    #[test]
    fn search_finds_and_ranks() {
        let lib = Library::generate_bench();
        let r = search(&lib, &lib.tracks[10].title);
        assert_eq!(r.tracks.first(), Some(&lib.tracks[10].id));
        assert!(!r.tracks.is_empty());
    }
}
