//! Framework-free application core for the Zuno native-GUI benchmark.
//!
//! Every `gui/<framework>` branch renders this state; nothing here knows a
//! windowing or GUI system exists. The split mirrors the target architecture in
//! `docs/gui-benchmarks/SPEC.md`:
//!
//! ```text
//! core::model      — domain types (Track / Album / Artist / Playlist)
//! core::library    — the synthetic 5,000-track library + home sections
//! core::artwork    — deterministic procedural album art + a shared LRU cache
//! core::queue      — the three-region queue (port of src/player/Queue.ts)
//! core::player     — rodio/cpal playback of in-process synthesized audio
//! core::app        — AppState: the shared application logic every UI drives
//! core::search     — match scoring + library filtering
//! core::theme      — the React app's design tokens, as constants
//! core::format     — durations and counts, Zuno's exact wording
//! core::bench      — the measurement harness every branch runs identically
//! ```
//!
//! What is deliberately NOT here: anything WebView. The Tauri app's IFrame
//! playback path, its media server for `<audio>`, and its localhost frontend
//! hosting are all WebView-era machinery and stay behind in `src-tauri`. The
//! audio output stack (cpal/rodio) is the one that the shipping `rust` engine
//! already uses natively; this crate reuses it.

pub mod app;
pub mod artwork;
pub mod bench;
pub mod format;
pub mod library;
pub mod model;
pub mod player;
pub mod queue;
pub mod rng;
pub mod search;
pub mod theme;

pub use app::{AppState, View, LibraryTab, RepeatMode};
pub use queue::{Queue, Region};
pub use model::{Album, AlbumId, Artist, ArtistId, Playlist, PlaylistId, Track, TrackId};
pub use player::PlayerHandle;
