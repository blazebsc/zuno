# Shared real-YouTube foundation (`native/yt` + core extensions)

Branch `gui/yt`. All seven UI-wiring tasks build on this — nothing here knows
a GUI exists.

## What was built

**`native/yt` (`zuno-yt`)** — framework-free Innertube client. JSON over
HTTPS plus byte downloads. No WebView, no browser/JS engine, no yt-dlp.

| module | what |
|---|---|
| `client.rs` | `YtClient { http, session }`, one POST path. `unsigned()` needs zero credentials. Attaches `User-Agent`, `Origin`/`Referer`, `X-YouTube-Client-Name/Version`, `Content-Type`, and — when signed in — `Cookie` + `Authorization: SAPISIDHASH <ts>_<sha1(ts + " " + sapisid + " " + origin)>` (port of `applyCookieAuth`, origin bound per client). Folds response `Set-Cookie` back into the session. |
| `api.rs` | `YtClientExt`: `search`, `get_album(MPRE…)`, `get_artist(UC…)`, `get_playlist` (VL-prefix added), `get_home` (charts + explore feeds), `get_library` (signed-in only → `NotSignedIn`), `get_related` (`next` endpoint queue panel). Returns plain structs (`YtTrack`, `YtAlbum`, `YtArtist`, `YtPlaylist`, `YtShelf`/`YtHome`, `YtSearchResults`). |
| `parse.rs` | Recursive leaf collectors (`musicResponsiveListItemRenderer`, `musicTwoRowItemRenderer`, `playlistPanelVideoRenderer`) — container drift (bare shelves vs `itemSectionRenderer` wrappers, one- vs two-column browse) passes through untouched. VideoId-less list rows parse too (unsigned search renders >½ of rows that way): MPRE → album, VL/playlistId → playlist, UC → artist. Menu subtrees are excluded from id search ("Start mix" endpoints would reclassify artist rows as playlists). |
| `stream.rs` | `resolve_stream(video_id) -> StreamInfo { url, mime, bitrate, client }`. Client order IOS → ANDROID → TV → WEB_REMIX, contexts/UAs verbatim from `try_youtube_api`. Ranking mirrors the TS resolver: highest-bitrate `audio/mp4`, fall back to any audio; `signatureCipher`-only formats skipped (no decipher). |
| `session.rs` | `Session::import` (pasted `Cookie:` header → pairs, attributes dropped), `account_identity` (SAPISID family), rotation merge (replace/tombstone, YouTube hosts only), plain-JSON disk persist. Tauri's keyring/AES-GCM store is stronger — production should match it. |
| `artwork.rs` | `fetch(url)` (browser UA, 10 s, 8 MiB cap) + `pick` (largest area, same rule as TS `artwork.ts`). |
| `library.rs` | `YtLibrary::from_search/home/album/playlist/shelf/tracks` → core `Library` (real titles/artists/durations, `video_id` + `artwork_url` set, `liked:false`, listeners `0`, unknown year `0`). `AppState` needs NO changes: build it, then replace `app.library`. |

**`native/core`, additive only** (26/26 pre-existing tests pass unmodified;
`--bench` path byte-identical):

- `model.rs`: `Track += video_id: String` (empty = synthetic) `+ artwork_url: Option<String>` (`None` = procedural); `Album`/`Artist`/`Playlist += artwork_url`.
- `artwork.rs`: `get_url(&self, url, size) -> Arc<Vec<u8>>` — same 96-cover LRU (keyed by URL hash), fetch + `image`-crate decode to `size×size` RGBA; any failure falls back to the procedural cover for the URL hash. `insert_bytes` is the network-free half (tests use it). `get(seed, size)` untouched.
- `player.rs`: `play_stream(url, mime, cookie)` — ranged download (`Range: bytes=0-`) into memory, then the ported decode stack: in-memory `VecSource` (`Read+Seek+MediaSource`), container sniff routes by magic bytes (declared mime is not trusted), Opus → symphonia demux + libopus (`OpusSource`, incl. pre-skip + seek), everything else → `rodio::Decoder`. Same sink, same volume/mute. Position is wall-clock while streaming (synth stays sample-counter). Download/decode failure ends the track so the queue advances. `seek_stream(sec)` re-opens the buffered bytes at the offset. Synth path untouched.

## Exact Innertube surface used

- `POST music.youtube.com/youtubei/v1/search?key=AIzaSyC9XL3ZjWddXya6X74dJoCTL-WEYFDNX30` — `WEB_REMIX 1.20250506.00.00`, body `{context, query}`.
- `POST …/youtubei/v1/browse` — `{browseId}`: `MPRE…` albums, `UC…` artists, `VL…` playlists, feeds `FEmusic_charts`, `FEmusic_explore`, (`FEmusic_library_landing`, `FEmusic_history` signed-in).
- `POST …/youtubei/v1/next` — `{videoId}` for up-next.
- `POST www.youtube.com/youtubei/v1/player?key=AIzaSyAO_FJ2SlqU8Q4STEHLGCilw_Y9_11qcW8` (music-URL twin for WEB_REMIX) — `{context, videoId, racyCheckOk, contentCheckOk}`. IOS `20.11.6`, ANDROID `21.03.36`, TVHTML5 `7.20260311.12.00`, WEB_REMIX as above.
- New deps: `reqwest` (rustls, no defaults; blocking for artwork/stream), `serde/serde_json`, `tokio` (rt only; rt-multi-thread+macros for tests), `sha1` (SAPISIDHASH is plain SHA-1 — no HMAC involved, so no hmac crate), `thiserror`, `image` (jpeg/png/webp decode only), `symphonia 0.5` (mkv+ogg demux — same version rodio pulls) + `opus 0.3` (cmake build). Nothing else.

## Deliberately NOT ported (WebView-app exclusives)

- **PO-token/BotGuard attestation** — needs a real browser challenge; unattested direct URLs serve full tracks for native playback (verified), only the WebView's *download* path hits the 1 MiB gate. Production native playback needs nothing here.
- **Sign-in window flow** — replaced by pasting a `Cookie:` header into `Session::import`. Production needs the WebView sign-in + keyring persist.
- **Likes/ratings, playlist edits, downloads, lyrics** — authenticated mutations and side systems; production ports them as further typed `api.rs` methods over the same `post_signed`.

## Online test (run once, 2026-10-09)

`cargo test -p zuno-yt --test online -- --ignored --nocapture` — PASSED (2.2 s):

```text
top hit: "Radiohead - Creep (Best live performance)" (US0CUegPr3g)
stream: client=YouTube iOS mime=audio/mp4 bitrate=130893
url head: https://rr1---sn-qx8vapo1-53a6.googlevideo.com/videoplayback?expire=…&ip=…
ranged head fetch: 1024 bytes OK
```

Also exercised live (scratch, same day): album `MPREb_TgQPwAzodvg` → "Creep" single 1992, 5 tracks; artist → Radiohead 5.42M subs, 5 top tracks, 20 albums; playlist → 42 tracks; home → 5 shelves; related → 1 item; unsigned `get_library` → `NotSignedIn`. Offline suite: `cargo test -p zuno-yt` green (7 unit + 12 fixture tests, no network); `cargo test -p zuno-core` 28/28.

## Honest gaps / surprises

- **Unsigned search shape differs from signed**: `itemSectionRenderer` wrappers (not bare shelves), zero `musicTwoRowItemRenderer`s — everything non-song arrives as videoId-less *list* rows. The recursive collectors absorb this; path-based parsing would have broken.
- **Charts feed has no song rows unsigned**: "Video charts" = chart *playlists* (`VL…` twoRows), "Top artists" = artist rows. `get_home` therefore yields playlists/artists, few tracks — signed-in home (library/history) will be richer; UI wiring should not assume home shelves contain tracks.
- **`next` up-next is thin unsigned** (1 item for the probe video) — related shelves will look sparse until a signed-in session or a search-backed fallback; documented, not worked around.
- **405s/403s**: none observed on any of the four player clients today; IOS succeeds first every time, so ANDROID/TV/WEB_REMIX fallbacks are untested against live failures (code path identical to the shipping backend's).
- **Age-restricted / region-locked**: not probed (needs such a videoId + signed-in session); `NotPlayable(status)` surfaces whatever `playabilityStatus` says — UI should show the status string.
- **Fixtures are redacted**: URL params outside the shipping allowlist are `[Nch]` length markers; structure/counts verified identical to raw captures.

## Per-branch integration recipe

```rust
// 1. Async load at startup (UI runtime block_on/spawn):
let mut yt = YtClient::unsigned();
let results = yt.search("…").await?;
app.library = YtLibrary::from_search(&results);   // or from_home/album/playlist
// 2. Covers: artwork.get_url(track.artwork_url, size) instead of .get(seed, size)
//    (None -> keep .get for bench/offline fallback).
// 3. Playback: let s = resolve_stream(&mut yt, &track.video_id).await?;
//    player.play_stream(&s.url, &s.mime, None) instead of player.play(track,…).
// 4. Sign-in affordance: text field -> Session::import(pasted) -> YtClient::with_session
//    -> get_library() fills app.library; save() persists (plain JSON).
```

Public API the seven wiring tasks consume: crate `zuno-yt` — `YtClient::unsigned()/with_session()`, trait `YtClientExt`
(`search/get_album/get_artist/get_playlist/get_home/get_library/get_related`),
`resolve_stream(&mut YtClient, &str) -> StreamInfo { url, mime, bitrate, client }`,
`Session::import/account_identity/save/load/merge_set_cookies`, `YtLibrary::from_*`,
`pick_artwork/fetch_artwork`; core — `Track.video_id/artwork_url`, `ArtworkCache::get_url/insert_bytes`,
`PlayerHandle::play_stream/seek_stream/is_streaming`.
