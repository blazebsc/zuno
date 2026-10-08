//! Hand-drawn SVG icons, sized on a 24px grid, matching the Solar
//! Linear (resting) / Bold (active) convention of `src/ui/icons.tsx`.
//! Identical bytes to the iced branch so all candidates render the
//! same icon set. Vizia tints them wholesale via the CSS `fill`
//! property, so the embedded `currentColor` never matters.

pub const HOME: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M3 10.5 12 3l9 7.5"/><path d="M5 9.5V21h14V9.5"/><path d="M9.5 21v-6h5v6"/></svg>"#;

pub const SEARCH: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"><circle cx="11" cy="11" r="7"/><path d="m20 20-3.5-3.5"/></svg>"#;
pub const LIBRARY: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"><path d="M4 4v16"/><path d="M9 4v16"/><path d="m14 5 5 15"/></svg>"#;
pub const SETTINGS: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="3"/><path d="M12 2.8v3M12 18.2v3M4.2 4.2l2.1 2.1M17.7 17.7l2.1 2.1M2.8 12h3M18.2 12h3M4.2 19.8l2.1-2.1M17.7 6.3l2.1-2.1"/></svg>"#;

pub const PLAY: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="currentColor"><path d="M7 5.4c0-1.1 1.2-1.8 2.2-1.2l11 7.6c.9.6.9 1.9 0 2.5l-11 7.6C8.2 22.4 7 21.7 7 20.6z"/></svg>"#;
pub const PAUSE: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="currentColor"><rect x="6" y="4" width="4" height="16" rx="1.3"/><rect x="14" y="4" width="4" height="16" rx="1.3"/></svg>"#;
pub const NEXT: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="currentColor"><path d="M5 6.1c0-1 1.1-1.6 2-1.1l9 6a1.3 1.3 0 0 1 0 2.2l-9 6c-.9.5-2-.1-2-1.1z"/><rect x="16.5" y="5" width="2.5" height="14" rx="1.2"/></svg>"#;
pub const PREV: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="currentColor"><path d="M19 6.1c0-1-1.1-1.6-2-1.1l-9 6a1.3 1.3 0 0 0 0 2.2l9 6c.9.5 2-.1 2-1.1z"/><rect x="5" y="5" width="2.5" height="14" rx="1.2"/></svg>"#;
pub const SHUFFLE: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M16 3h5v5"/><path d="M4 20 21 3"/><path d="M21 16v5h-5"/><path d="m15 15 6 6"/><path d="M4 4l5 5"/></svg>"#;
pub const REPEAT: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="m17 2 4 4-4 4"/><path d="M3 11v-1a4 4 0 0 1 4-4h14"/><path d="m7 22-4-4 4-4"/><path d="M21 13v1a4 4 0 0 1-4 4H3"/></svg>"#;
pub const REPEAT_ONE: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="m17 2 4 4-4 4"/><path d="M3 11v-1a4 4 0 0 1 4-4h14"/><path d="m7 22-4-4 4-4"/><path d="M21 13v1a4 4 0 0 1-4 4H3"/><path d="M11 11.5 12.5 10.5V15" stroke-width="2"/></svg>"#;

pub const HEART: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M12 20.3 4.6 12.9a5 5 0 0 1 0-7l.2-.2a4.8 4.8 0 0 1 6.9 0l.3.3.3-.3a4.8 4.8 0 0 1 6.9 0l.2.2a5 5 0 0 1 0 7z"/></svg>"#;
pub const HEART_ACTIVE: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="currentColor"><path d="M12 20.7a1 1 0 0 1-.7-.3l-7.4-7.4a5.7 5.7 0 0 1 0-8l.2-.2a5.5 5.5 0 0 1 7.9 0 5.5 5.5 0 0 1 7.9 0l.2.2a5.7 5.7 0 0 1 0 8l-7.4 7.4a1 1 0 0 1-.7.3"/></svg>"#;
pub const VOLUME: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M11 5 6.5 9H3v6h3.5L11 19z" fill="currentColor" stroke="none"/><path d="M15.5 8.5a5 5 0 0 1 0 7"/><path d="M18.5 5.5a9 9 0 0 1 0 13"/></svg>"#;
pub const VOLUME_MUTE: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M11 5 6.5 9H3v6h3.5L11 19z" fill="currentColor" stroke="none"/><path d="m15.5 9.5 5 5M20.5 9.5l-5 5"/></svg>"#;
pub const QUEUE: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"><path d="M3 6h18"/><path d="M3 12h12"/><path d="M3 18h12"/><circle cx="19.5" cy="15.5" r="2.5"/><path d="M22 15.5V7l-2.5.8"/></svg>"#;
pub const BACK: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M19 12H5"/><path d="m11 18-6-6 6-6"/></svg>"#;
pub const CLOSE: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"><path d="M6 6l12 12M18 6 6 18"/></svg>"#;
