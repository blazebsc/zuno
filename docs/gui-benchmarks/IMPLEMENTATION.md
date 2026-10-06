# Implementing a branch — core API cheat-sheet

Everything a `gui/<fw>` branch needs to drive `zuno-core`. Read
[SPEC.md](./SPEC.md) first; this file is the concrete API you call.

## Workspace wiring

Each branch's root `Cargo.toml` gains one member:

```toml
[workspace]
resolver = "2"
members = ["native/core", "native/ui-<fw>"]
exclude = ["src-tauri", "landing", "node_modules"]
```

`native/ui-<fw>/Cargo.toml`:

```toml
[package]
name = "zuno-ui-iced"        # per framework
version = "0.1.0"
edition = "2021"

[dependencies]
zuno-core = { path = "../core" }
<framework> = "…"
```

## `zuno_core::AppState` — the whole app

```rust
use zuno_core::{AppState, View, TrackId, LibraryTab, RepeatMode};

let mut state = AppState::new();   // builds the 5,000-track library (~30 ms)
```

### Navigation
```rust
state.go(View::Home);                       // with history
state.open_album(album_id);                 // = go(View::Album(id))
state.open_artist(id); state.open_playlist(id);
state.set_search(&query);                   // enters Search view, sets query
state.set_library_tab(LibraryTab::Songs);   // Playlists|Albums|Artists|Songs
state.go_back(); state.go_forward(); state.can_go_back();
```

### Playback
```rust
state.play_from(&ids, index);   // playlist-mode: plays ids[index], queues the rest
state.play_track(id);          // single track, no queue
state.toggle_play(); state.next(); state.previous();
state.seek(127.0);              // seconds, clamped internally
state.set_volume(0.7); state.toggle_mute();
state.toggle_shuffle(); state.cycle_repeat();      // Off → All → One → Off
state.toggle_like(track_id);
state.add_to_queue(id); state.play_next_in_queue(id);
state.tick();                   // EVERY FRAME (or ≥10 Hz): auto-advance, repeat policy
```

Reads (mirror fields, cheap — diff them in retained UIs):
```rust
state.playing: bool
state.current: Option<TrackId>
state.shuffle: bool
state.repeat: RepeatMode
state.volume: f32
state.muted: bool
state.player.position_sec() -> f64     // playhead
state.player.duration_sec() -> f64      // loaded track length
state.current_track() -> Option<&Track>
state.up_next() -> Option<TrackId>
```

### Lists
```rust
state.list_ids() -> Vec<TrackId>          // rows for the current view:
                                         //  Library▸Songs = all 5,000
                                         //  Album/Playlist = its tracks
                                         //  Search = filtered, ranked
state.queue_rows() -> Vec<(TrackId, Region)>   // queue panel rows
state.selection: Option<usize>                 // set by UI on selection
```

### Rows (borrowed — clone only what visible rows need)
```rust
let v = state.library.track_view(id);
// v: TrackView { id, title: &str, artist: &str, album: &str,
//                duration_sec: u32, explicit: bool, liked: bool }
let t = state.library.track(id);     // full Track (has album_id, artist_id…)
let a = state.library.album(id);    // Album { title, artist_id, year, kind, track_ids }
let r = state.library.artist(id);   // Artist { name, monthly_listeners, album_ids }
let p = state.library.playlist(id); // Option<&Playlist { title, description, track_ids, system }>
```

Home shelves: `state.library.home_recent_albums(12)`, `state.library.mixes`
(6 mixes), `&state.library.albums` (grid — take 24 by year desc or first 24),
`&state.library.artists` (shelf). Sidebar: `state.library.playlists`
(index 0 = Liked Songs, `system: true`).

### Artwork (identical across branches — do not roll your own cache)
```rust
let rgba: Arc<Vec<u8>> = state.artwork.get(AppState::album_seed(album_id), 176);
// RGBA8, width == height == requested size (40 rows, 176 cards, 232 headers).
// Wrap into your framework's texture/image handle; keep a small
// (seed,size) → handle map so you don't re-upload every frame, but the
// decoded-bitmap budget stays in core's shared 96-cover LRU.
AppState::track_seed(id) / album_seed / artist_seed / playlist_seed  // u64 seeds
```

### Formatting + search + theme
```rust
use zuno_core::format::{mmss, long_duration, count_songs, counts_line, listeners};
use zuno_core::search::search;
use zuno_core::theme::{DARK, LIGHT, H1, H2, H3, BODY, BODY_MED, SMALL, SMALL_MED, XS,
                      TITLEBAR_H, SIDEBAR_W, ROW_H, ROW_ART, CARD_W, CARD_H, QUEUE_W,
                      PLAYER_BAR_H, PAGE_PAD, SECTION_GAP, RADIUS_ROW, RADIUS_ART, RADIUS_FULL, RADIUS_BTN};
let hits = search(&state.library, "neon");   // SearchResults { tracks, albums, artists, playlists }
```

## The `--bench` loop

```rust
use zuno_core::bench::{BenchDriver, BenchAction};

let mut bench = if std::env::args().any(|a| a == "--bench") {
    Some(BenchDriver::new())    // construct AFTER the first frame is on screen
} else { None };

// per frame:
state.tick();
if let Some(b) = &mut bench {
    match b.tick() {
        BenchAction::RecordStartup => {}                      // nothing extra
        BenchAction::ScrollTo(f)  => scroll_songs_list_to(f), // Library▸Songs list,
                                                             // jump, no easing
        BenchAction::SelectRow(i) => select_visible_row(i),   // 0..60 of visible
        BenchAction::StartPlayback => { let ids = state.list_ids();
                                        state.play_from(&ids, ids.len()/2); }
        BenchAction::Finish => { println!("{}", b.report);
                                 std::process::exit(0); }
        BenchAction::Nothing => {}
    }
}
```

Notes:
- `ScrollTo(f)` arrives **every frame** for 20 s — map `f` (0..1) onto
  `f * (total_rows * ROW_H - viewport_h)`. The library list must be the
  Library▸Songs 5,000-row list, and it must actually move.
- `SelectRow(i)` is an index into the **currently visible** rows — clamp.
- Playback must come out of the real speakers path (rodio) — don't stub it.
- Print the report verbatim to stdout; the runner captures it.
- Interactive mode (no `--bench`) is the same app minus the driver.

## Verify (see SPEC §6), then report

Fill `docs/gui-benchmarks/<fw>.md` from the template in SPEC §8 with the real
outputs. Commit on the branch with message
`feat: native <framework> UI benchmark` and push to `origin`.
