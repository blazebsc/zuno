//! Real-YouTube-Music Innertube client, framework-free.
//!
//! No WebView, no browser engine, no JS runtime, no yt-dlp: JSON over HTTPS
//! plus byte downloads. Two facts make this possible (both validated live):
//! unsigned `search` on `WEB_REMIX` returns real results, and the `player`
//! endpoint with the `IOS` client returns direct audio URLs (no decipher,
//! no PO token) with `playabilityStatus.status == "OK"`.
//!
//! Deliberately NOT ported (WebView-app exclusives, see docs/gui-benchmarks/yt.md):
//! PO-token/BotGuard attestation, the sign-in window flow, likes/ratings
//! mutations, playlist edits, downloads, lyrics.

pub mod api;
pub mod artwork;
pub mod client;
pub mod error;
pub mod library;
pub mod model;
pub mod parse;
pub mod session;
pub mod stream;

pub use api::YtClientExt;
pub use artwork::{fetch as fetch_artwork, pick as pick_artwork};
pub use client::YtClient;
pub use error::YtError;
pub use library::YtLibrary;
pub use model::*;
pub use session::Session;
pub use stream::{resolve_stream, StreamInfo};
