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
- **Likes/ratings, playlist edits, downloads, lyrics** — authenticated mutations and side systems; production ports them as further typed `api.rs` methods over the same `post_signed`. *(Update, phase 1b: the mutations are now ported — see "Mutations" below. Downloads remain a UI concern over `resolve_download_url`; lyrics remain excluded.)*

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

---

# Phase 1b: the full Tauri-app flow (decipher, client walk, endpoints, mutations)

Phase 1's sections above still describe the transport/auth/base parsing; this
phase turns that into a faithful port of the app's YouTube flow. Nothing above
changed shape — additions only, plus one rename (phase 1's queue-panel
`get_related` is now `get_recommendations`, matching the app's
`getRecommendations`; `get_related` is the app's `getRelated`, the related
tab).

## Decipher design (the one ported JS step)

`format.decipher` needs YouTube's player-JS code: pure string arithmetic, **no
DOM, no network from JS, no BotGuard, no attestation**. This is JavaScript
*evaluation* via the `rquickjs` crate (embedded QuickJS, ~1 MB), not a browser
engine — the only JS in the native app.

**A necessary correction to the plan.** The task expected "the same regexes
youtubei.js uses — read the bundled `decipher.ts`". The bundled youtubei.js is
**v17.0.1**, which has no `decipher.ts` and no regexes: it replaced them with a
full ESTree analyzer (`JsAnalyzer`/`JsExtractor` on the `meriyah` parser), a
`nsigMatcher` that finds the decipher function structurally (a ≥3-param
function whose body reassigns its first arg from `new X.Y(…)` and calls
`.set("alr","yes")` — today's combined n/sig function, e.g.
`nQ=function(d,Q="",t=""){d=new g.vL(d,!0);d.set("alr","yes");…}`; there is no
`a=a.split("")` shape in the 2026 player), and a runtime recipe
(`getNsigProcessorFn`) that feeds the function a mock URL, enumerates the
constructed object's prototype methods and reads back the transformed `sp`
and `n` params.

Ported faithfully, at the same level of mechanism but without shipping a JS
parser (`decipher/`):

- `lex.rs` — a mini-lexer (strings, template literals with `${}`
  interpolation, regex-vs-division, comments, numeric literals) recording
  each token's nesting depth. Not a JS parser; enough to brace-match
  machine-generated code.
- `extract.rs` — finds the IIFE body and its top-level statements, locates
  the decipher function by youtubei.js's `nsigMatcher` criteria, then pulls
  the transitive statement closure: plain names, dotted member keys
  (`g.vL=…`), prototype **aliases** (`g.J=yq.prototype; g.J.toString=…` —
  the player reuses scratch alias names sequentially; youtubei.js calls
  this its "prototype alias binding"), minus each statement's over-approximated
  locals (minified one-letter params/vars must not pull every same-named
  top-level). Emitted in source order with youtubei.js's runtime preamble
  (window/document/self bound from the host, inert stand-ins under QuickJS).
- `mod.rs` — player fetch (`/iframe_api` → `player_id` between `player\/`
  and `\/`, then `/s/player/{id}/player_es6.vflset/en_US/base.js`),
  `signatureTimestamp` extraction (the `timestampMatcher` raw value), the
  `getNsigProcessorFn` recipe verbatim, and URL assembly: stamp the
  transformed `sp`/`signature` and `n`, then restamp `cver`
  (`withSessionClientVersion` + youtubei.js's client switch merged — added
  when the cipher omitted it, rewritten when it disagrees; `cver` is not
  covered by the signature).

**Validation, not hope**: the strict test vector (synthetic `s`/`n` inputs)
was computed with the real youtubei.js v17.0.1 against the captured player and
the Rust port reproduces both outputs **exactly**. The over-pulled closure is
~790 KB (youtubei.js's AST-precise one is ~180 KB); release extraction takes
~0.5 s, cached per player id.

**Rotation**: the compiled player is cached per player-id in one QuickJS
context (created lazily inside `YtClient`). Any decipher failure marks the
player failed, refetches the current player once, and retries; a persistent
failure surfaces to the walk, which still has the direct clients ahead of the
deciphered ones. Nothing retried beyond that single refetch.

## The client walk (verbatim order and semantics)

`resolve_stream_url` (playback) and `resolve_download_url` (offline) are
SEPARATE functions with no shared flags, mirroring the app's
`resolveStreamUrl`/`resolveDownloadUrl` "policy in a named method" rule. Both
walk:

1. **Direct attempts first** (phase 1's verified path): `player` with IOS →
   ANDROID → TV contexts (their URLs are usable as-is).
2. **Deciphered WEB family**: WEB_REMIX (music) → WEB. The request body
   carries `playbackContext.contentPlaybackContext` with
   `signatureTimestamp` (the same body `getBasicInfo` sends); every format is
   deciphered — `signatureCipher`/`cipher` through the full transform, plain
   `url`s through the `n`-only transform.

Ranking is `selectFormatForQuality` exactly: `high` ranks every audio format
by bitrate (Opus-in-WebM at ~151 kbps beats AAC at ~130 — the comment block at
TS:5686-5700 is why), `low`/`normal` prefilter to `audio/mp4` and then pick
the *nearest* bitrate, not the highest at-or-below (a 128 kbps cap would drop
YouTube's ~131 kbps AAC tier to the 49 kbps one over 3 kbps).

**The single deliberate exclusion, restated**: PO-token/BotGuard attestation
is NOT ported — it needs a real browser challenge. The app's "download" client
is the attested one; here it runs unattested, which collapses the app's
`download`/`music` labels onto the same client set (documented on
`resolve_stream_url_quality`). Direct URLs serve full tracks for native
playback (verified live); the WebView's 1 MiB gate applies only to the
app's unattested *download* path.

## Endpoint coverage (reads)

| method | endpoint | state |
|---|---|---|
| `search` | `search` (mixed) | live-tested (phase 1, re-verified) |
| `search_category` | `search` + filter `params` (`SearchFilter` protobuf, precomputed) | live-tested (20 song rows) |
| `search_suggestions` | `music/get_search_suggestions` | live-tested (deduped, 3-cap) |
| `get_album` | `browse` MPRE | live-tested (phase 1) |
| `get_artist` | `browse` UC — now with `subscribed` state (`subscribeButtonRenderer`) | fixture-tested; live phase 1 |
| `get_playlist` / `get_playlist_track_page` / `get_playlist_tracks` | `browse` + continuation | **live-tested pagination** (100-row first page, +10-row second page on a 111-track playlist) |
| `get_home` | `browse` FEmusic_charts/explore | live-tested (phase 1) |
| `get_library` | signed-in browse | live-tested `NotSignedIn` (phase 1); signed-in shape unchanged from phase 1 |
| `get_recommendations` | `next` queue panel | live-tested (phase 1) |
| `get_related` | `next` tab → browse `MPTRt_…` | live-tested (related shelves) |
| lyrics-source table | — | EXCLUDED by design (separate providers, plain HTTP, later task) |

Pagination ported the app's two layers: `getPlaylistTrackPage`'s page sessions
(continuation token + seen-id set, later pages yield only fresh rows) and the
`collectTrackPages.ts` termination guard as a generic `collect_track_pages`
(termination when nothing is new AND the cursor did not move; page cap; the
fixture-backed tests drive both, including the stuck-cursor case). The
continuation *token lookup* follows `getMusicContinuation`'s order — shelf
tokens and the shelf's trailing `continuationItemRenderer` win over the
`secondaryContents` sidebar token, which on playlist pages is the *related
carousel's* cursor (this cost a live-test iteration to notice).

## Mutations

All ported as thin signed POSTs, bodies verbatim from the TS datasource and
the youtubei.js endpoints it delegates to (`mutations.rs`, trait `YtMutations`):

- `set_track_rating` — `/next` → find the `likeEndpoint` whose status matches →
  `like/like|dislike|removelike` with the per-status params and the normalized
  target (`PL`/`OLAK5uy_` → playlist, else video). `removelike` clears a
  dislike as well as a like.
- `create_playlist` (`playlist/create`), `rename_playlist` /
  `add_track_to_playlist` / `remove_track_from_playlist` /
  `reorder_playlist_tracks` (`browse/edit_playlist` with the four action
  shapes), `delete_playlist` (`playlist/delete` — the app's direct call around
  the youtubei.js 17 bug).
- `set_playlist_saved` (`like` endpoint with a playlist target).
- `set_artist_subscribed` (`subscription/subscribe|unsubscribe`, fixed params
  `EgIIAhgA`/`CgIIAhgA`).
- Remove/reorder address **set video ids** (the playlist row, not the song) —
  `YtTrack.set_video_id` now parses from `playlistItemData`.

**Test state: shape-tested, needs a signed-in session.** The request shapes
are asserted offline against the exact JSON (endpoint constants + key fields,
target normalization, per-status params, VL stripping); the like-endpoint
discovery is fixture-backed. No signed-in test session exists on this machine,
so no harmless like→unlike was run live.

## Session = the app's session, minus the window

Ported the lib.rs jar semantics into `Session`/`YtClient`: `apply_set_cookie`
merge/tombstone rules (unchanged from phase 1), the **youtube.com-only host
gate** (`is_youtube_cookie_host` — the CDN hosts never rotate the jar),
**immediate persist for credential cookies** (`__Secure-*PSIDTS` — Google
retires the superseded value) with a **300 s throttle for the noisy ones**
(`SIDCC`, `__Secure-*PSIDCC`), and the lib.rs `cookie_account_identity` order
(`SAPISID` → `__Secure-3PAPISID` → `__Secure-1PAPISID`; phase 1 had 1P before
3P — fixed). Disk format unchanged (plain JSON, `Session::load` compatible).

**Sign-in, 60 seconds**: browser → music.youtube.com → devtools → Network →
any `/youtubei/` request → copy the whole `Cookie:` request header →
`Session::import(paste)` → `YtClient::with_session`. Unsigned mode keeps
working for public content.

## Tests

- Offline (`cargo test -p zuno-yt`, no network): 23 lib + 24 integration,
  including: the strict decipher vector (exact match with youtubei.js), the
  redacted real WEB_REMIX ciphers (URL parses, carries `sig=`/`itag=`/
  `expire=`, `n` transformed, `cver` restamped), every parser fixture
  (including the new category/suggestions/pagination/related captures),
  mutation request shapes, both pagination-guard cases, and the cookie rules
  mirroring lib.rs's `#[cfg(test)]` module.
- `cargo test -p zuno-core` 28/28 — core untouched this phase; `--bench`
  compiles as before (no bench harness exists).
- Online `#[ignore]`, run once (2026-10-09, release build):

```text
top hit: "Radiohead - Creep (Best live performance)" (US0CUegPr3g)
stream: client=YouTube iOS mime=audio/mp4 bitrate=130893
url head: https://rr1---sn-qx8vapo1-53a6.googlevideo.com/videoplayback?expire=…
ranged head fetch: 1024 bytes OK
song category: 20 tracks
suggestions: ["radiohead creep", "creep radiohead clean", "creep radiohead cover"]
playlist page 1: 100 tracks, has_more=true
playlist page 2: +10 tracks
related: 1 shelves
test live_search_and_stream ... ok (3.98 s)
```

Also validated live while building (scratch, same day): the deciphered
WEB_REMIX itag-140 URL serves ranged bytes (HTTP 206,
`bytes 0-1023/3830364`) — the decipher pipeline is proven against googlevideo,
not just against youtubei.js's own output.

## Fixtures added (`native/yt/tests/fixtures/`)

`player_base.js` (the REAL captured player, 2.6 MB — the extractor runs
against the real artifact, not a synthetic), `player_remix.json` (real
WEB_REMIX player response; opaque URL params and the ciphered `s=` values
redacted to `[Nch]` markers — the transform still runs, and the exact-output
vector lives in `decipher_vectors.json` with synthetic inputs),
`search_song.json`, `search_suggestions.json`, `playlist_page.json`
(100 rows + trailing `continuationItemRenderer`),
`playlist_page_continuation.json` (append-action shape, +10 rows),
`next_tabs.json` (tab → `MPTRt_…` lookup), `related_page.json`.

## Crates

- **ADDED: `rquickjs` 0.14** (`parallel` feature — QuickJS is the task's
  prescribed evaluator for the decipher step; `parallel` makes the runtime
  `Send + Sync` so the player cache can live inside the shared client).
  Vendored quickjs-ng builds with the existing C toolchain (same story as
  `opus`'s cmake build).
- **REMOVED: none** — phase 1 used no third-party YouTube crate (plain
  reqwest + serde), so there was nothing to replace; the flow is implemented
  entirely in this crate.

## Phase-1b honest gaps / surprises

- **The task's decipher description was stale**: youtubei.js v17 replaced the
  classic `a=a.split("")` decipher + regexes with an AST extractor and a
  combined n/sig function. Porting the task's text instead of the library
  would have produced code that matches a player shape YouTube no longer
  ships.
- **Over-pull is the cost of textual analysis**: no AST means no precise
  scopes; the closure is ~4× youtubei.js's. It buys robustness (a missing
  helper is a hard eval error, not a wrong transform) at ~0.5 s release
  extraction, cached per player.
- **The sidebar continuation trap**: playlist pages carry a *second*
  continuation for the related sidebar; following it yields the carousel,
  zero tracks. The TS `getMusicContinuation` ordering exists for exactly this.
- **`getRelated` needs two requests** (next → tab → browse); unsigned it
  still returns shelves, but some tracks' related tabs come back sparse —
  same shape as the app's sparse-unsigned observations.
- **42-track playlists paginate to nothing**: their "continuation" returns
  the related carousel with zero rows and no further token — the guard
  treats that as a completed list, which it is.
- **QuickJS quirks**: `ctx.eval` runs strict, so the exported wrapper must be
  a `const` declaration (a bare assignment to an undeclared global throws);
  DOM globals the over-pulled closure touches at load time need inert
  stand-ins in the preamble (youtubei.js binds the same names for the same
  reason).

## Public API for the seven wiring tasks (phase 1b superset)

```rust
let mut yt = zuno_yt::YtClient::unsigned()?;              // or with_session(Session::import(paste))
// reads (trait YtClientExt)
yt.search("query").await?;                                 // mixed search
yt.search_category("query", SearchCategory::Song).await?;  // one deep category
yt.search_suggestions("query").await?;                     // Vec<String>, 3-cap
yt.get_album("MPRE…") / get_artist("UC…") / get_playlist("VL…") / get_home() / get_library()
yt.get_playlist_track_page(id, None).await?;               // (YtTrackPage, Option<PlaylistPageSession>) — pass the session back for page 2+
yt.get_playlist_tracks(id).await?;                         // whole list, guard included
yt.get_recommendations(video_id).await?;                   // up-next panel (phase 1's get_related)
yt.get_related(video_id).await?;                           // the related tab, Vec<YtShelf>
// streams (SEPARATE functions, no shared flags)
zuno_yt::resolve_stream_url(&mut yt, video_id).await?;     // playback; StreamInfo { url, mime, bitrate, client }
zuno_yt::resolve_download_url(&mut yt, video_id).await?;  // offline queue
zuno_yt::stream::resolve_stream(&mut yt, video_id).await?; // phase-1 alias of resolve_stream_url
zuno_yt::stream::pick_audio(&player_response)?;           // offline ranking helper
// mutations (trait YtMutations, signed-in; Rating::{Like, Dislike, None})
yt.set_track_rating(video_id, Rating::Like).await?;
yt.create_playlist("title", &[]).await?;                    // -> playlist id
yt.rename_playlist / delete_playlist / add_track_to_playlist
yt.remove_track_from_playlist(id, &track.set_video_id.unwrap())   // row id, not the song
yt.reorder_playlist_tracks(id, moved_set_video_id, Some(predecessor_set_video_id))
yt.set_playlist_saved(id, saved) / set_artist_subscribed(channel_id, subscribed)
// session + new model fields
zuno_yt::Session::import / save / load / account_identity   // jar rules ported from lib.rs
YtTrack { set_video_id: Option<String>, … }                 // playlist row id
YtArtist { subscribed: Option<bool>, … }                    // subscribe state
YtTrackPage { tracks, has_more, next_page_key }              // pagination page
// decipher (rarely needed directly — resolve_* calls it)
zuno_yt::Decipherer::from_source(player_id, &player_js)?;  // pre-seeded cache
decipherer.decipher_cipher_sync(cipher, client_version)?;  // or transform(n, sp, s)
```
