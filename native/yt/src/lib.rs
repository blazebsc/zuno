//! Real-YouTube-Music Innertube client, framework-free.
//!
//! JSON over HTTPS plus byte downloads. The ONE piece of JavaScript in the
//! native app is deliberate: YouTube's own decipher code, extracted from
//! the player script and evaluated with embedded QuickJS — pure string
//! arithmetic, no DOM, no network from JS, no BotGuard (see `decipher` and
//! docs/gui-benchmarks/yt.md). Everything else is plain Rust.
//!
//! Deliberately NOT ported (WebView-app exclusives, see docs/gui-benchmarks/yt.md):
//! PO-token/BotGuard attestation — the `download` client runs unattested and
//! the stream walk falls through to direct mobile/TV URLs and deciphered
//! web formats — and the sign-in window flow (`Session::import` replaces it).

pub mod api;
pub mod artwork;
pub mod client;
pub mod decipher;
pub mod error;
pub mod library;
pub mod model;
pub mod mutations;
pub mod parse;
pub mod session;
pub mod stream;

pub use api::{PlaylistPageSession, SearchCategory, YtClientExt};
pub use artwork::{fetch as fetch_artwork, pick as pick_artwork};
pub use client::YtClient;
pub use decipher::Decipherer;
pub use error::YtError;
pub use library::YtLibrary;
pub use model::*;
pub use mutations::{Rating, YtMutations};
pub use session::Session;
pub use stream::{resolve_download_url, resolve_stream_url, AudioQuality, StreamInfo};
