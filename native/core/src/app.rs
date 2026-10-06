//! The shared application state — the "core/application logic" of the target
//! architecture. Every framework drives this; none of it knows a window
//! exists. This is what keeps eight implementations behaving as one app.
//!
//! Mapping to the existing codebase:
//! - queue semantics: `src/player/Queue.ts` (ported in [`crate::queue`])
//! - playback: `src-tauri/src/audio.rs`'s engine (same rodio/cpal stack)
//! - library/search/likes: the `datasource` + `LibraryController` surface,
//!   synthetic (see SPEC.md §Audio for what stays WebView-bound and why)
//!
//! What is intentionally NOT modelled: multiple tabs (a benchmark app has
//! one), the mini player, tray, Discord/Last.fm, media keys, downloads. Those
//! are framework-independent services that sit beside this state and would be
//! ported per the same pattern; the bake-off measures rendering + memory, and
//! they are identical Rust code regardless of GUI choice.

use crate::artwork::ArtworkCache;
use crate::library::Library;
use crate::model::*;
use crate::player::PlayerHandle;
use crate::queue::{Queue, Region};
use crate::search;

/// The page a tab is showing (a trimmed `ui/types/tab.ts` `TabView`).
#[derive(Clone, PartialEq, Debug)]
pub enum View {
    Home,
    Library,
    Album(AlbumId),
    Artist(ArtistId),
    Playlist(PlaylistId),
    Search,
    Settings,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LibraryTab {
    Playlists,
    Albums,
    Artists,
    Songs,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RepeatMode {
    Off,
    All,
    One,
}

/// App-level state. UI frameworks own one of these and render it.
pub struct AppState {
    pub library: Library,
    pub artwork: ArtworkCache,
    pub queue: Queue,
    pub player: PlayerHandle,

    pub view: View,
    pub library_tab: LibraryTab,
    back: Vec<View>,
    forward: Vec<View>,

    pub search_query: String,
    /// The id list the current view's main track list shows (and selection
    /// indexes into). Refreshed by [`Self::list_ids`]; kept here so per-frame
    /// rendering stays allocation-free in the common case.
    pub selection: Option<usize>,

    /// The list playback started from — Repeat::All wraps to its start.
    source_list: Vec<TrackId>,

    // Mirrors of player state so retained-mode UIs can diff cheaply.
    pub playing: bool,
    pub current: Option<TrackId>,
    pub shuffle: bool,
    pub repeat: RepeatMode,
    pub volume: f32,
    pub muted: bool,
}

impl AppState {
    pub fn new() -> Self {
        Self::with_tracks(crate::library::BENCH_TRACKS)
    }

    pub fn with_tracks(n: usize) -> Self {
        let lib = Library::generate(n, 0x5EED_0001);
        let player = PlayerHandle::new();
        let volume = player.volume();
        let muted = player.is_muted();
        AppState {
            library: lib,
            artwork: ArtworkCache::new(),
            queue: Queue::new(),
            player,
            view: View::Home,
            library_tab: LibraryTab::Songs,
            back: Vec::new(),
            forward: Vec::new(),
            search_query: String::new(),
            selection: None,
            source_list: Vec::new(),
            playing: false,
            current: None,
            shuffle: false,
            repeat: RepeatMode::Off,
            volume,
            muted,
        }
    }

    /// Deterministic seed for a track's synth + artwork.
    pub fn track_seed(id: TrackId) -> u64 {
        crate::rng::hash(id as u64)
    }
    pub fn album_seed(id: AlbumId) -> u64 {
        crate::rng::hash(0xA1B0_0000_0000 | id as u64)
    }
    pub fn artist_seed(id: ArtistId) -> u64 {
        crate::rng::hash(0x5A70_0000_0000 | id as u64)
    }
    pub fn playlist_seed(id: PlaylistId) -> u64 {
        crate::rng::hash(0x9_0000_0000 | id as u64)
    }

    // — Playback ————————————————————————————————————————————

    fn start(&mut self, id: TrackId) {
        let track = self.library.track(id);
        self.player.play(track, Self::track_seed(id), 0.0);
        self.current = Some(id);
        self.playing = true;
    }

    /// Play `list` starting at `index` — the playlist-mode entry point
    /// (`PlayerController.playTrackById`).
    pub fn play_from(&mut self, list: &[TrackId], index: usize) {
        if list.is_empty() {
            return;
        }
        self.source_list = list.to_vec();
        self.queue.set_list(list, index);
        self.start(list[index]);
    }

    /// Play one track with no queue behind it (single clicks on cards).
    pub fn play_track(&mut self, id: TrackId) {
        self.play_from(&[id], 0);
    }

    pub fn toggle_play(&mut self) {
        match self.current {
            Some(_) if self.playing => {
                self.player.pause();
                self.playing = false;
            }
            Some(id) => {
                let pos = self.player.position_sec();
                if pos >= self.player.duration_sec() - 0.05 {
                    self.start(id); // restart after finishing
                } else {
                    self.player.resume();
                    self.playing = true;
                }
            }
            None => {
                // Nothing loaded: play the first thing in the main list.
                let ids = self.list_ids();
                if !ids.is_empty() {
                    self.play_from(&ids, 0);
                }
            }
        }
    }

    pub fn next(&mut self) {
        if let Some(id) = self.queue.next() {
            self.start(id);
        } else if self.repeat == RepeatMode::All && !self.source_list.is_empty() {
            self.play_from(&self.source_list.clone(), 0);
        } else {
            self.playing = false;
        }
    }

    pub fn previous(&mut self) {
        // Spotify/Zuno behaviour: within the first 3 s, previous restarts the
        // track; after that it goes back in history.
        if self.player.position_sec() > 3.0 {
            if let Some(id) = self.queue.prev() {
                self.start(id);
                return;
            }
        }
        if let Some(id) = self.current {
            self.start(id);
        }
    }

    pub fn seek(&mut self, sec: f64) {
        let Some(id) = self.current else { return };
        let track = self.library.track(id);
        let clamped = sec.clamp(0.0, track.duration_sec as f64 - 0.2);
        self.player.seek(track, Self::track_seed(id), clamped);
        self.playing = true;
    }

    pub fn set_volume(&mut self, v: f32) {
        self.volume = v.clamp(0.0, 1.0);
        self.player.set_volume(self.volume);
    }

    pub fn toggle_mute(&mut self) {
        self.muted = !self.muted;
        self.player.set_muted(self.muted);
    }

    pub fn toggle_shuffle(&mut self) {
        self.shuffle = !self.shuffle;
        self.queue.toggle_shuffle();
    }

    pub fn cycle_repeat(&mut self) {
        self.repeat = match self.repeat {
            RepeatMode::Off => RepeatMode::All,
            RepeatMode::All => RepeatMode::One,
            RepeatMode::One => RepeatMode::Off,
        };
    }

    pub fn toggle_like(&mut self, id: TrackId) {
        let t = &mut self.library.tracks[id as usize];
        t.liked = !t.liked;
        if t.liked {
            self.library.playlists[0].track_ids.insert(0, id);
        } else if let Some(pos) = self.library.playlists[0].track_ids.iter().position(|&x| x == id) {
            self.library.playlists[0].track_ids.remove(pos);
        }
    }

    pub fn add_to_queue(&mut self, id: TrackId) {
        self.queue.add_to_queue(id);
    }
    pub fn play_next_in_queue(&mut self, id: TrackId) {
        self.queue.play_next(id);
    }

    /// Advance playback state: call every frame (or ≥10 Hz). Polls the audio
    /// engine's "track ended" flag and applies repeat policy.
    pub fn tick(&mut self) {
        if self.player.take_ended() {
            if self.repeat == RepeatMode::One {
                if let Some(id) = self.current {
                    self.start(id);
                }
            } else {
                self.next();
            }
        }
    }

    pub fn current_track(&self) -> Option<&Track> {
        self.current.map(|id| self.library.track(id))
    }

    /// The track that would play on `next` — for the player bar's "up next".
    pub fn up_next(&self) -> Option<TrackId> {
        self.queue
            .manual()
            .first()
            .or_else(|| self.queue.automatic().first())
            .copied()
    }

    // — Navigation ———————————————————————————————————————————

    fn push_history(&mut self) {
        if self.back.last() != Some(&self.view) {
            self.back.push(self.view.clone());
            if self.back.len() > 64 {
                self.back.remove(0);
            }
        }
        self.forward.clear();
        self.selection = None;
    }

    pub fn go(&mut self, view: View) {
        if view == self.view {
            return;
        }
        self.push_history();
        self.view = view;
        if self.view == View::Search && self.search_query.is_empty() {
            // Entering search with no query still shows the field focus state.
        }
    }

    pub fn go_back(&mut self) -> bool {
        if let Some(v) = self.back.pop() {
            self.forward.push(std::mem::replace(&mut self.view, v));
            self.selection = None;
            true
        } else {
            false
        }
    }

    pub fn go_forward(&mut self) -> bool {
        if let Some(v) = self.forward.pop() {
            self.back.push(std::mem::replace(&mut self.view, v));
            self.selection = None;
            true
        } else {
            false
        }
    }

    pub fn can_go_back(&self) -> bool {
        !self.back.is_empty()
    }

    pub fn open_album(&mut self, id: AlbumId) {
        self.go(View::Album(id));
    }
    pub fn open_artist(&mut self, id: ArtistId) {
        self.go(View::Artist(id));
    }
    pub fn open_playlist(&mut self, id: PlaylistId) {
        self.go(View::Playlist(id));
    }
    pub fn set_search(&mut self, q: &str) {
        if self.view != View::Search {
            self.push_history();
            self.view = View::Search;
        }
        self.search_query = q.to_string();
        self.selection = None;
    }

    pub fn set_library_tab(&mut self, tab: LibraryTab) {
        self.library_tab = tab;
        self.selection = None;
    }

    // — List contents for the current view ————————————————————

    /// The track-id list the current view's main track list shows. This is
    /// the 5,000-row surface in Library ▸ Songs (the bench's scroll target)
    /// and the filtered list in Search.
    pub fn list_ids(&self) -> Vec<TrackId> {
        match &self.view {
            View::Library => match self.library_tab {
                LibraryTab::Songs => (0..self.library.tracks.len() as u32).collect(),
                LibraryTab::Playlists | LibraryTab::Albums | LibraryTab::Artists => Vec::new(),
            },
            View::Album(id) => self.library.album(*id).track_ids.clone(),
            View::Playlist(id) => self
                .library
                .playlist(*id)
                .map(|p| p.track_ids.clone())
                .unwrap_or_default(),
            View::Search => search::search(&self.library, &self.search_query).tracks,
            View::Home | View::Artist(_) | View::Settings => Vec::new(),
        }
    }

    /// Queue rows for the queue panel, with region tags.
    pub fn queue_rows(&self) -> Vec<(TrackId, Region)> {
        self.queue.rows().into_iter().map(|r| (r.id, r.region)).collect()
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> AppState {
        AppState::with_tracks(400)
    }

    #[test]
    fn play_from_partitions_and_advances() {
        let mut s = state();
        let ids: Vec<TrackId> = (0..10).collect();
        s.play_from(&ids, 4);
        assert_eq!(s.current, Some(4));
        s.next();
        assert_eq!(s.current, Some(5));
        // Within the first 3 s, previous restarts the current track —
        // the Spotify/Zuno semantics AppState::previous implements.
        s.previous();
        assert_eq!(s.current, Some(5));
    }

    #[test]
    fn repeat_all_wraps() {
        let mut s = state();
        let ids: Vec<TrackId> = (0..3).collect();
        s.play_from(&ids, 2);
        s.cycle_repeat(); // Off -> All
        assert_eq!(s.repeat, RepeatMode::All);
        s.next(); // queue exhausted -> wrap
        assert_eq!(s.current, Some(0));
    }

    #[test]
    fn like_updates_liked_playlist() {
        let mut s = state();
        let id = s.library.playlists[0].track_ids[0];
        s.toggle_like(id); // unlike
        assert!(!s.library.track(id).liked);
        assert!(!s.library.playlists[0].track_ids.contains(&id));
        s.toggle_like(id); // like again
        assert_eq!(s.library.playlists[0].track_ids[0], id);
    }

    #[test]
    fn navigation_history_roundtrip() {
        let mut s = state();
        s.open_album(3);
        s.open_artist(5);
        assert!(s.go_back());
        assert_eq!(s.view, View::Album(3));
        assert!(s.go_forward());
        assert_eq!(s.view, View::Artist(5));
    }

    #[test]
    fn search_view_lists_filtered_tracks() {
        let mut s = state();
        let title = s.library.tracks[0].title.clone();
        s.set_search(&title);
        assert_eq!(s.view, View::Search);
        assert!(!s.list_ids().is_empty());
    }
}
