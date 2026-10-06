//! The synthetic benchmark library: 5,000 tracks, hundreds of albums and
//! artists, dozens of playlists, all deterministic from one fixed seed so
//! every framework scrolls the exact same data.
//!
//! Names are generated from curated word banks — fictional but realistic, so
//! the UI reads like a real consumer app instead of "Track 0427". No real
//! artists: this is benchmark data, not scraped metadata.

use crate::model::*;
use crate::rng::Rng;

/// Number of tracks the benchmark generates (the spec's minimum).
pub const BENCH_TRACKS: usize = 5_000;

pub struct Library {
    pub tracks: Vec<Track>,
    pub albums: Vec<Album>,
    pub artists: Vec<Artist>,
    /// User playlists, Liked Songs first. The generated "mix" playlists live
    /// in `mixes` so the sidebar and the home shelves can treat them apart.
    pub playlists: Vec<Playlist>,
    pub mixes: Vec<Playlist>,
    /// Deterministic "what you played last" — the Home page's first shelf.
    pub recently_played: Vec<TrackId>,
}

impl Library {
    pub fn generate_bench() -> Self {
        Self::generate(BENCH_TRACKS, 0x5EED_0001)
    }

    /// Deterministic generator. `seed` 0x5EED_0001 is the benchmark's identity;
    /// other seeds exist for tests.
    pub fn generate(target_tracks: usize, seed: u64) -> Self {
        let mut rng = Rng::new(seed);
        let mut tracks = Vec::with_capacity(target_tracks + 256);
        let mut albums = Vec::new();
        let mut artists = Vec::new();

        // Artists until the track target is reached. 2-4 albums each, 6-14
        // tracks per album, plus the occasional single.
        while tracks.len() < target_tracks {
            let artist_id = artists.len() as ArtistId;
            let name = artist_name(&mut rng);
            let album_count = rng.range(2, 5) as usize;
            let mut album_ids = Vec::with_capacity(album_count);
            for _ in 0..album_count {
                if tracks.len() >= target_tracks + 12 {
                    break;
                }
                let album_id = albums.len() as AlbumId;
                let kind = match rng.range(0, 10) {
                    0 => AlbumKind::Single,
                    1 | 2 => AlbumKind::Ep,
                    _ => AlbumKind::Album,
                };
                let n = match kind {
                    AlbumKind::Single => 1,
                    AlbumKind::Ep => rng.range(4, 7),
                    AlbumKind::Album => rng.range(7, 15),
                } as usize;
                let mut track_ids = Vec::with_capacity(n);
                for no in 1..=n {
                    let id = tracks.len() as TrackId;
                    tracks.push(Track {
                        id,
                        title: track_title(&mut rng),
                        artist_id,
                        album_id,
                        track_no: no as u32,
                        duration_sec: track_duration(&mut rng),
                        explicit: rng.range(0, 100) < 8,
                        liked: false,
                    });
                    track_ids.push(id);
                }
                album_ids.push(album_id);
                albums.push(Album {
                    id: album_id,
                    title: album_title(&mut rng),
                    artist_id,
                    year: rng.range(1974, 2026) as u16,
                    track_ids,
                    kind,
                });
            }
            artists.push(Artist {
                id: artist_id,
                name,
                monthly_listeners: rng.range(4_000, 38_000_000),
                album_ids,
            });
        }

        // Likes: ~14% of tracks, biased toward earlier (alphabetically the
        // "older") albums — a real liked list is not uniform noise.
        for (i, t) in tracks.iter_mut().enumerate() {
            let bias = if i % 3 == 0 { 22 } else { 9 };
            t.liked = rng.range(0, 100) < bias;
        }

        // — Playlists ————————————————————————————————————————
        let mut playlists = Vec::new();
        playlists.push(Playlist {
            id: 0,
            title: "Liked Songs".into(),
            description: Some("Every song you've liked".into()),
            track_ids: tracks.iter().filter(|t| t.liked).map(|t| t.id).collect(),
            system: true,
        });
        let pl_words = [
            ("Late Night Drive", "Neon streets, empty highway"),
            ("Focus Flow", "Deep work, no vocals"),
            ("Rainy Sunday", "Coffee and windows"),
            ("Gym Energy", "Nothing under 140 bpm"),
            ("Chill Acoustic", "Wooden strings, slow mornings"),
            ("Synthwave Nights", "1985, but on purpose"),
            ("Indie Discoveries", "Updated every Friday"),
            ("Road Trip", "Six hours, zero skips"),
            ("Sunday Reset", "Put things back in place"),
            ("Study Session", "Lyric-free concentration"),
            ("Dinner Party", "Impressive, not exhausting"),
            ("Morning Momentum", "Start loud"),
            ("Throwback 2010s", "The golden era"),
            ("Bedtime", "Everything slows down"),
            ("Jazz Corner", "Brushes and uprights"),
            ("Bass Heavy", "Subwoofers required"),
            ("Songwriting Sparks", "Demos worth finishing"),
            ("Long Walks", "Podcast replacement"),
            ("Winter Warmer", "December, lamplight"),
            ("Summer Rooftop", "Golden hour"),
            ("Mellow Mornings", "Ease into it"),
            ("Office Ambience", "Polite, rhythmic"),
            ("Lo-Fi Circuit", "Dusty loops"),
            ("Desert Highways", "Wide open spaces"),
            ("Coastal Drive", "Windows down"),
            ("Vinyl Crackle", "Warm and slightly worn"),
            ("Festival Flashbacks", "Mud and confetti"),
            ("Quiet Storm", "Smooth, late"),
            ("Neon Arcade", "Coins and CRTs"),
            ("Paper Lanterns", "Soft light, slow tempo"),
            ("Underground Frequencies", "Below the charts"),
            ("Recovery", "A slow return"),
            ("Golden Hour Sessions", "Recorded live at dusk"),
            ("Midnight Oil", "For the last stretch"),
            ("First Coffee", "Before the news"),
            ("Home Stretch", "Almost there"),
            ("Analog Heart", "Tape hiss romance"),
            ("Blue Hour", "Between day and night"),
            ("Campfire Songs", "Acoustic circle"),
            ("Peak Performance", "Personal bests"),
            ("Slow Motion", "Half speed"),
            ("City Limits", "Where the buildings end"),
        ];
        for (i, (title, desc)) in pl_words.iter().enumerate() {
            let id = (i + 1) as PlaylistId;
            let n = rng.range(30, 121) as usize;
            let mut ids = Vec::with_capacity(n);
            let start = rng.range(0, (tracks.len().saturating_sub(n)) as u32) as usize;
            for t in &tracks[start..start + n] {
                ids.push(t.id);
            }
            rng.shuffle(&mut ids);
            playlists.push(Playlist {
                id,
                title: (*title).into(),
                description: Some((*desc).into()),
                track_ids: ids,
                system: false,
            });
        }

        // — Generated "made for you" mixes (home shelf) ——————————————
        let mix_names = [
            ("Discover Mix", "Fresh finds for you"),
            ("New Releases Mix", "The newest from artists you follow"),
            ("Your Time Capsule", "We made you a playlist of memories"),
            ("Daily Mix 1", "More of what you like"),
            ("Daily Mix 2", "Made for you"),
            ("Daily Mix 3", "Mix it up"),
        ];
        let mut mixes = Vec::new();
        for (i, (title, desc)) in mix_names.iter().enumerate() {
            let n = 50usize;
            let start = rng.range(0, (tracks.len().saturating_sub(n * (i + 1))) as u32) as usize;
            let ids = tracks[start..start + n].iter().map(|t| t.id).collect();
            mixes.push(Playlist {
                id: 1000 + i as PlaylistId,
                title: (*title).into(),
                description: Some((*desc).into()),
                track_ids: ids,
                system: true,
            });
        }

        // — Recently played ———————————————————————————————————————
        let mut recent = Vec::with_capacity(24);
        for _ in 0..24 {
            recent.push(rng.range(0, tracks.len() as u32) as TrackId);
        }

        Library {
            tracks,
            albums,
            artists,
            playlists,
            mixes,
            recently_played: recent,
        }
    }

    pub fn track(&self, id: TrackId) -> &Track {
        &self.tracks[id as usize]
    }
    pub fn album(&self, id: AlbumId) -> &Album {
        &self.albums[id as usize]
    }
    pub fn artist(&self, id: ArtistId) -> &Artist {
        &self.artists[id as usize]
    }
    pub fn playlist(&self, id: PlaylistId) -> Option<&Playlist> {
        self.playlists
            .iter()
            .find(|p| p.id == id)
            .or_else(|| self.mixes.iter().find(|p| p.id == id))
    }

    /// The resolved row data for a track — the one call track lists make.
    pub fn track_view(&self, id: TrackId) -> TrackView<'_> {
        let t = self.track(id);
        TrackView {
            id: t.id,
            title: &t.title,
            artist: &self.artist(t.artist_id).name,
            album: &self.album(t.album_id).title,
            duration_sec: t.duration_sec,
            explicit: t.explicit,
            liked: t.liked,
        }
    }

    pub fn liked_track_ids(&self) -> Vec<TrackId> {
        self.tracks.iter().filter(|t| t.liked).map(|t| t.id).collect()
    }

    /// Home page shelves, mirroring the React HomePage section order.
    pub fn home_recent_albums(&self, n: usize) -> Vec<AlbumId> {
        // Distinct albums from the recent list, in play order, deduped.
        let mut seen = std::collections::HashSet::new();
        let mut out = Vec::new();
        for tid in &self.recently_played {
            let aid = self.track(*tid).album_id;
            if seen.insert(aid) {
                out.push(aid);
                if out.len() == n {
                    break;
                }
            }
        }
        out
    }
}

// — Name generation ——————————————————————————————————————————

/// Tiny splitmix64; separate module so tests can reach it.
fn track_duration(rng: &mut Rng) -> u32 {
    // Triangular around 3:40 — most tracks 2:30-4:30, tails on both sides.
    let r = rng.next_f32();
    let s = 95.0 + r * r * 240.0 + rng.next_f32() * 30.0;
    s as u32
}

const FIRST: &[&str] = &[
    "Aria", "Beau", "Cecilia", "Dorian", "Elena", "Finn", "Greta", "Hugo", "Iris", "Jonah",
    "Katya", "Luca", "Mira", "Noel", "Odessa", "Pablo", "Quinn", "Rosa", "Silas", "Talia",
    "Ulrik", "Vera", "Wren", "Xena", "Yusuf", "Zola", "Anouk", "Bram", "Cleo", "Dario",
    "Esme", "Felix", "Gaia", "Ines", "Jasper", "Kira", "Leon", "Mabel", "Nadia", "Otto",
];

const LAST: &[&str] = &[
    "Ashgrove", "Bellamy", "Calloway", "Dunmore", "Eastwood", "Fairbanks", "Grimaldi", "Hallow",
    "Ironwood", "Juniper", "Kestrel", "Larkspur", "Merrow", "Nightingale", "Oakhart", "Prescott",
    "Quimby", "Redgrave", "Silverman", "Thorne", "Umber", "Vance", "Whitlock", "Yarrow",
    "Zephyr", "Ambrose", "Birch", "Cobalt", "Draven", "Ellsworth", "Fenn", "Grayson",
];

const BAND_NOUNS: &[&str] = &[
    "Wolves", "Ghosts", "Lanterns", "Pilots", "Foxes", "Sparrows", "Machines", "Mirrors",
    "Harbours", "Ladders", "Compasses", "Cadets", "Satellites", "Anchors", "Vultures", "Kites",
    "Daggers", "Engines", "Lanterns", "Sailors", "Tigers", "Arrows", "Bells", "Canoes",
];

const ONE_WORD: &[&str] = &[
    "Solstice", "Vespera", "Halcyon", "Monolith", "Paragon", "Wanderlust", "Meridian", "Obsidian",
    "Reverie", "Cascade", "Zenith", "Lumina", "Nocturne", "Ember", "Fathom", "Gossamer",
    "Inertia", "Kindred", "Lyric", "Mosaic", "Nimbus", "Onyx", "Parallax", "Quasar",
    "Riptide", "Sonnet", "Tempest", "Umbra", "Vertigo", "Wavelength",
];

const DJ_WORDS: &[&str] = &[
    "Kilo", "Nova", "Static", "Vapor", "Echo", "Prime", "Neon", "Chrome", "Pulse", "Delta",
    "Circuit", "Bassline", "Freq", "Modular", "Wired",
];

fn artist_name(rng: &mut Rng) -> String {
    match rng.range(0, 10) {
        0..=2 => format!("{} {}", rng.pick(FIRST), rng.pick(LAST)),
        3 | 4 => format!("The {}", rng.pick(BAND_NOUNS)),
        5 => format!("{} & {}", rng.pick(FIRST), rng.pick(LAST)),
        6 => format!("DJ {}", rng.pick(DJ_WORDS)),
        7 => rng.pick(ONE_WORD).to_string(),
        8 => format!("{} {}", rng.pick(ONE_WORD), rng.pick(ONE_WORD)),
        _ => format!("{} {} Trio", rng.pick(FIRST), rng.pick(LAST)),
    }
}

const ALBUM_ADJ: &[&str] = &[
    "Midnight", "Velvet", "Quiet", "Electric", "Golden", "Broken", "Distant", "Paper",
    "Northern", "Southern", "Ancient", "Modern", "Silver", "Hidden", "Endless", "Gentle",
    "Feral", "Holy", "Deep", "Wild",
];
const ALBUM_NOUN: &[&str] = &[
    "Rooms", "Streets", "Weather", "Machinery", "Fever", "Gospel", "Anthem", "Postcards",
    "Ceilings", "Telephone", "Bloom", "Static", "Signals", "Lullabies", "Sirens", "Gravity",
    "Daydreams", "Rust", "Saltwater", "Neon",
];
const ALBUM_OF: &[&str] = &[
    "Songs of", "Tales of", "Letters from", "Notes on", "Map of", "Hours of", "The Book of",
];
const ALBUM_PLACES: &[&str] = &[
    "the Valley", "Nowhere", "the Coast", "the Underworld", "Small Towns", "the North Sea",
    "Everywhere", "the Desert", "the Old City", "the Woods",
];

fn album_title(rng: &mut Rng) -> String {
    match rng.range(0, 10) {
        0 => format!("{} {}", rng.pick(ALBUM_ADJ), rng.pick(ALBUM_NOUN)),
        1 => format!("{} {} {}", rng.pick(ALBUM_ADJ), rng.pick(ALBUM_NOUN), rng.range(2, 5)),
        2 => format!("{} {}", rng.pick(ALBUM_OF), rng.pick(ALBUM_PLACES)),
        3 => format!("{} & {}", rng.pick(ALBUM_NOUN), rng.pick(ALBUM_NOUN)),
        4 => rng.pick(ONE_WORD).to_string(),
        5 => format!("The {} {}", rng.pick(ALBUM_ADJ), rng.pick(ALBUM_NOUN)),
        6 => format!("{} in {}", rng.pick(ALBUM_NOUN), rng.pick(ALBUM_PLACES)),
        7 => format!("{} (Deluxe)", rng.pick(ALBUM_NOUN)),
        8 => format!("No. {}", rng.range(1, 9)),
        _ => format!("{} {}, Vol. {}", rng.pick(ALBUM_ADJ), rng.pick(ALBUM_NOUN), rng.range(2, 4)),
    }
}

const TRACK_PATTERNS: &[&str] = &[
    "{a} {n}", "{n} in {p}", "The {n}", "{n}", "{a} {n} - {r}", "{n} ({r})", "{t} {n}",
    "All the {n}", "No {n}", "{n} of {y}", "{n} for {s}", "Something Like {n}",
    "{f}'s {n}", "Dancing With {n}", "{n} at {d}", "{a}, {a} {n}",
];
const TRACK_ADJ: &[&str] = ALBUM_ADJ;
const TRACK_NOUN: &[&str] = &[
    "Hearts", "Windows", "Photographs", "Letters", "Cars", "Lights", "Bridges", "Winters",
    "Summers", "Ghosts", "Kings", "Roses", "Streets", "Chances", "Doors", "Rivers", "Anchors",
    "Questions", "Promises", "Maps", "Clouds", "Trains", "Anthem", "Afternoons", "Fireflies",
];
const TRACK_PLACES: &[&str] = &[
    "Tokyo", "the Rain", "Slow Motion", "Reversed", "the Dark", "Hollywood", "June",
    "the Summertime", "the Fall", "Silence", "the Waiting Room", "Rome", "the Cold",
];
const TRACK_REMASTERS: &[&str] = &[
    "Remastered", "Radio Edit", "Live", "Acoustic", "Demo", "Single Version", "Extended Mix",
    "Instrumental",
];
const TRACK_TITLES: &[&str] = &[
    "Runaway", "Supernova", "Overboard", "Headlights", "Daydreamer", "Wildfire", "Slow Down",
    "Fever Dream", "Vertigo", "Favourite Mistake", "Paper Planes", "Satellite",
];
const TRACK_SEASONS: &[&str] = &["Two", "Nobody", "Everyone", "You", "the Road", "December", "the Brokenhearted"];
const TRACK_DAYS: &[&str] = &["Midnight", "Daybreak", "Closing Time", "5AM", "Last Call", "Sunrise"];

fn track_title(rng: &mut Rng) -> String {
    let pat = rng.pick(TRACK_PATTERNS);
    let mut s = pat.to_string();
    while s.contains('{') {
        s = if let Some(i) = s.find('{') {
            let j = s[i..].find('}').unwrap() + i;
            let key = &s[i + 1..j];
            let rep = match key {
                "a" => rng.pick(TRACK_ADJ).to_string(),
                "n" => rng.pick(TRACK_NOUN).to_string(),
                "p" => rng.pick(TRACK_PLACES).to_string(),
                "r" => rng.pick(TRACK_REMASTERS).to_string(),
                "t" => rng.pick(TRACK_TITLES).to_string(),
                "f" => rng.pick(FIRST).to_string(),
                "y" => rng.pick(TRACK_SEASONS).to_string(),
                "s" => rng.pick(TRACK_SEASONS).to_string(),
                "d" => rng.pick(TRACK_DAYS).to_string(),
                _ => key.to_string(),
            };
            format!("{}{}{}", &s[..i], rep, &s[j + 1..])
        } else {
            break;
        };
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bench_library_shape() {
        let lib = Library::generate_bench();
        assert!(lib.tracks.len() >= BENCH_TRACKS, "at least 5,000 tracks");
        assert!(lib.albums.len() >= 400, "hundreds of albums, got {}", lib.albums.len());
        assert!(lib.artists.len() >= 150, "hundreds of artists, got {}", lib.artists.len());
        assert!(lib.playlists.len() >= 40);
        assert!(!lib.mixes.is_empty());
        // Every album points at valid tracks, in order.
        for a in lib.albums.iter().take(50) {
            assert_eq!(a.track_ids.len() as u32, lib.track(a.track_ids.last().copied().unwrap()).track_no);
        }
        // Liked list non-empty and consistent.
        assert!(lib.liked_track_ids().len() > 200);
        assert_eq!(lib.liked_track_ids().len(), lib.playlists[0].track_ids.len());
    }

    #[test]
    fn deterministic() {
        let a = Library::generate_bench();
        let b = Library::generate_bench();
        assert_eq!(a.tracks[4231].title, b.tracks[4231].title);
        assert_eq!(a.tracks.len(), b.tracks.len());
    }

    #[test]
    fn durations_are_plausible() {
        let lib = Library::generate_bench();
        for t in &lib.tracks {
            assert!(t.duration_sec >= 90 && t.duration_sec <= 400, "{}", t.duration_sec);
        }
    }
}
