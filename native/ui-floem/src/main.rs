//! Zuno — floem implementation of the native-GUI benchmark (gui/floem branch).
//!
//! Renders the shared `zuno_core::AppState` per docs/gui-benchmarks/SPEC.md.
//! State lives in one `Rc<RefCell<AppState>>`; floem signals carry only tiny
//! revision counters (`rev` for content, `pos` for the ~10 Hz playhead) so
//! the 5,000-row Songs list rebuilds only when its content actually changes.
//! The Songs list uses floem's `virtual_list` (only visible rows exist).

use floem::{
    event::{Event, EventListener, EventPropagation},
    ext_event::create_signal_from_channel,
    keyboard::{Key, NamedKey},
    peniko::{kurbo::Point, Color},
    reactive::{create_effect, create_rw_signal, RwSignal, SignalGet, SignalUpdate},
    style::CursorStyle,
    text::Weight,
    unit::Pct,
    views::{
        container, dyn_container, empty, h_stack, h_stack_from_iter, img, label, scroll,
        slider::slider, static_label, svg, text_input, v_stack, v_stack_from_iter,
        virtual_list, Decorators, VirtualDirection, VirtualItemSize,
    },
    window::WindowConfig,
    Application, AnyView, IntoView, View, ViewId,
};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;
use zuno_core::bench::{BenchAction, BenchDriver};
use zuno_core::format::{count_songs, counts_line, listeners, mmss};
use zuno_core::model::*;
use zuno_core::queue::Region;
use zuno_core::search;
use zuno_core::theme::{self, Rgb};
use zuno_core::app::{LibraryTab, RepeatMode};
use zuno_core::{AppState, View as ZView};

// — Icons (Solar-style 24px paths, same set as the iced branch) ———————————
// `currentColor` is baked to the wanted hex at build time.

const HOME: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M3 10.5 12 3l9 7.5"/><path d="M5 9.5V21h14V9.5"/><path d="M9.5 21v-6h5v6"/></svg>"#;
const SEARCH_I: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"><circle cx="11" cy="11" r="7"/><path d="m20 20-3.5-3.5"/></svg>"#;
const LIBRARY: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"><path d="M4 4v16"/><path d="M9 4v16"/><path d="m14 5 5 15"/></svg>"#;
const SETTINGS: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="3"/><path d="M12 2.8v3M12 18.2v3M4.2 4.2l2.1 2.1M17.7 17.7l2.1 2.1M2.8 12h3M18.2 12h3M4.2 19.8l2.1-2.1M17.7 6.3l2.1-2.1"/></svg>"#;
const PLAY: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="currentColor"><path d="M7 5.4c0-1.1 1.2-1.8 2.2-1.2l11 7.6c.9.6.9 1.9 0 2.5l-11 7.6C8.2 22.4 7 21.7 7 20.6z"/></svg>"#;
const PAUSE: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="currentColor"><rect x="6" y="4" width="4" height="16" rx="1.3"/><rect x="14" y="4" width="4" height="16" rx="1.3"/></svg>"#;
const NEXT: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="currentColor"><path d="M5 6.1c0-1 1.1-1.6 2-1.1l9 6a1.3 1.3 0 0 1 0 2.2l-9 6c-.9.5-2-.1-2-1.1z"/><rect x="16.5" y="5" width="2.5" height="14" rx="1.2"/></svg>"#;
const PREV: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="currentColor"><path d="M19 6.1c0-1-1.1-1.6-2-1.1l-9 6a1.3 1.3 0 0 0 0 2.2l9 6c.9.5 2-.1 2-1.1z"/><rect x="5" y="5" width="2.5" height="14" rx="1.2"/></svg>"#;
const SHUFFLE: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M16 3h5v5"/><path d="M4 20 21 3"/><path d="M21 16v5h-5"/><path d="m15 15 6 6"/><path d="M4 4l5 5"/></svg>"#;
const REPEAT: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="m17 2 4 4-4 4"/><path d="M3 11v-1a4 4 0 0 1 4-4h14"/><path d="m7 22-4-4 4-4"/><path d="M21 13v1a4 4 0 0 1-4 4H3"/></svg>"#;
const REPEAT_ONE: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="m17 2 4 4-4 4"/><path d="M3 11v-1a4 4 0 0 1 4-4h14"/><path d="m7 22-4-4 4-4"/><path d="M21 13v1a4 4 0 0 1-4 4H3"/><path d="M11 11.5 12.5 10.5V15" stroke-width="2"/></svg>"#;
const HEART: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M12 20.3 4.6 12.9a5 5 0 0 1 0-7l.2-.2a4.8 4.8 0 0 1 6.9 0l.3.3.3-.3a4.8 4.8 0 0 1 6.9 0l.2.2a5 5 0 0 1 0 7z"/></svg>"#;
const HEART_ACTIVE: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="currentColor"><path d="M12 20.7a1 1 0 0 1-.7-.3l-7.4-7.4a5.7 5.7 0 0 1 0-8l.2-.2a5.5 5.5 0 0 1 7.9 0 5.5 5.5 0 0 1 7.9 0l.2.2a5.7 5.7 0 0 1 0 8l-7.4 7.4a1 1 0 0 1-.7.3"/></svg>"#;
const VOLUME: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M11 5 6.5 9H3v6h3.5L11 19z" fill="currentColor" stroke="none"/><path d="M15.5 8.5a5 5 0 0 1 0 7"/><path d="M18.5 5.5a9 9 0 0 1 0 13"/></svg>"#;
const VOLUME_MUTE: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M11 5 6.5 9H3v6h3.5L11 19z" fill="currentColor" stroke="none"/><path d="m15.5 9.5 5 5M20.5 9.5l-5 5"/></svg>"#;
const QUEUE_I: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"><path d="M3 6h18"/><path d="M3 12h12"/><path d="M3 18h12"/><circle cx="19.5" cy="15.5" r="2.5"/><path d="M22 15.5V7l-2.5.8"/></svg>"#;
const BACK: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M19 12H5"/><path d="m11 18-6-6 6-6"/></svg>"#;
const CLOSE: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"><path d="M6 6l12 12M18 6 6 18"/></svg>"#;

/// Bench scroll geometry: 5,000 rows × 52px, ~560px viewport.
const SONGS_TOTAL_H: f64 = 5000.0 * 52.0;
const SONGS_VIEW_H: f64 = 560.0;

// — Small helpers ————————————————————————————————————————————————

fn rgb(c: Rgb) -> Color {
    Color::rgb8(c.0[0], c.0[1], c.0[2])
}
fn bg() -> Color {
    rgb(theme::DARK.background)
}
fn fg() -> Color {
    rgb(theme::DARK.foreground)
}
fn card() -> Color {
    rgb(theme::DARK.card)
}
fn muted() -> Color {
    rgb(theme::DARK.muted)
}
fn muted_fg() -> Color {
    rgb(theme::DARK.muted_foreground)
}
fn primary() -> Color {
    rgb(theme::DARK.primary)
}
fn row_playing() -> Color {
    rgb(theme::state::row_playing())
}
fn row_selected() -> Color {
    rgb(theme::state::row_selected())
}
fn hex(c: Rgb) -> String {
    format!("#{:02x}{:02x}{:02x}", c.0[0], c.0[1], c.0[2])
}

/// Char-boundary-safe truncation with an ellipsis.
fn trunc(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        return s.to_string();
    }
    let mut out: String = s.chars().take(n.saturating_sub(1)).collect();
    out.push('…');
    out
}

fn h1(s: String) -> impl IntoView {
    static_label(s).style(|s| s.font_size(32.0).font_weight(Weight::BOLD).color(fg()))
}
fn h3(s: String) -> impl IntoView {
    static_label(s).style(|s| {
        s.font_size(18.0)
            .font_weight(Weight::SEMIBOLD)
            .color(fg())
    })
}

/// Baked-color svg icon.
fn ic(icon_src: &str, color: Rgb, size: f64) -> impl IntoView {
    let col = rgb(color);
    svg(icon_src.replace("currentColor", &hex(color)))
        .style(move |s| s.width(size).height(size).color(col))
}

// — Shared model —————————————————————————————————————————————————

#[derive(Clone)]
struct Model {
    app: Rc<RefCell<AppState>>,
    png: Rc<RefCell<HashMap<(u64, u32), Vec<u8>>>>,
    /// Bumped on any content change (nav, selection, playing, likes, queue…).
    rev: RwSignal<u64>,
    /// Bumped at ~10 Hz for the playhead.
    pos: RwSignal<u64>,
    /// Bumped when the search query changes.
    results: RwSignal<u64>,
    queue_open: RwSignal<bool>,
    query: RwSignal<String>,
    search_focused: RwSignal<bool>,
    hover_card: RwSignal<Option<u64>>,
    /// Programmatic scroll target for the Songs virtual list (bench + PgUp/Dn).
    scroll_songs: RwSignal<Option<Point>>,
    /// Programmatic scroll target for the current page list (keyboard follow).
    scroll_page: RwSignal<Option<Point>>,
    search_id: Rc<Cell<Option<ViewId>>>,
    root_id: Rc<Cell<Option<ViewId>>>,
    songs_off: Rc<Cell<f64>>,
    songs_vh: Rc<Cell<f64>>,
    page_off: Rc<Cell<f64>>,
    page_vh: Rc<Cell<f64>>,
    bench: Rc<RefCell<Option<BenchDriver>>>,
    bench_mode: bool,
    focused_once: Rc<Cell<bool>>,
    tick_n: Rc<Cell<u64>>,
}

impl Model {
    fn bump(&self) {
        self.rev.update(|v| *v += 1);
    }
    fn go(&self, v: ZView) {
        self.app.borrow_mut().go(v);
        self.scroll_page.set(None);
        self.bump();
    }
    fn back(&self) {
        self.app.borrow_mut().go_back();
        self.scroll_page.set(None);
        self.bump();
    }
    fn focus_search(&self) {
        self.app.borrow_mut().go(ZView::Search);
        self.scroll_page.set(None);
        self.bump();
        if let Some(id) = self.search_id.get() {
            id.request_focus();
        }
    }
    /// (seed,size) → PNG bytes for floem's `img`, cached so page rebuilds
    /// and virtual-list recycling never re-encode.
    fn png(&self, seed: u64, size: u32) -> Vec<u8> {
        if let Some(hit) = self.png.borrow().get(&(seed, size)) {
            return hit.clone();
        }
        let rgba = self.app.borrow().artwork.get(seed, size);
        let bytes = encode_png(&rgba, size);
        self.png.borrow_mut().insert((seed, size), bytes.clone());
        bytes
    }
    fn play_ids(&self, ids: &Rc<Vec<TrackId>>, i: usize) {
        if i < ids.len() {
            let mut a = self.app.borrow_mut();
            a.selection = Some(i);
            a.play_from(ids, i);
        }
        self.bump();
    }
    /// Move row selection by `delta`, following with the right scroll target.
    fn step_selection(&self, delta: i32) {
        let len = self.app.borrow().list_ids().len();
        if len == 0 {
            return;
        }
        let next = match self.app.borrow().selection {
            None if delta < 0 => len - 1,
            None => 0,
            Some(s) => (s as i32 + delta).clamp(0, len as i32 - 1) as usize,
        };
        self.app.borrow_mut().selection = Some(next);
        self.bump();
        let top = next as f64 * 52.0;
        let on_songs = {
            let a = self.app.borrow();
            a.view == ZView::Library && a.library_tab == LibraryTab::Songs
        };
        if on_songs {
            let y0 = self.songs_off.get();
            let vh = if self.songs_vh.get() > 0.0 {
                self.songs_vh.get()
            } else {
                SONGS_VIEW_H
            };
            if top < y0 || top + 52.0 > y0 + vh {
                self.scroll_songs
                    .set(Some(Point::new(0.0, (top - vh / 2.0).max(0.0))));
            }
        } else {
            let y0 = self.page_off.get();
            let vh = if self.page_vh.get() > 0.0 {
                self.page_vh.get()
            } else {
                560.0
            };
            if top < y0 || top + 52.0 > y0 + vh {
                self.scroll_page
                    .set(Some(Point::new(0.0, (top - vh / 2.0).max(0.0))));
            }
        }
    }
}

fn encode_png(rgba: &[u8], size: u32) -> Vec<u8> {
    use image::codecs::png::PngEncoder;
    use image::ImageEncoder;
    let mut out = Vec::new();
    let enc = PngEncoder::new(&mut out);
    let _ = enc.write_image(rgba, size, size, image::ExtendedColorType::Rgba8);
    out
}

fn art_img(m: &Model, seed: u64, size: u32, radius: f64) -> impl IntoView {
    let bytes = m.png(seed, size);
    img(move || bytes.clone()).style(move |s| {
        s.width(size as f64)
            .height(size as f64)
            .border_radius(radius)
    })
}

// — Track rows ———————————————————————————————————————————————————

/// The static 3-bar "now playing" glyph (spec asks animated; static bars).
fn bars() -> impl IntoView {
    let red = primary();
    h_stack((
        container(empty()).style(move |s| {
            s.width(3.0).height(14.0).background(red).border_radius(1.5)
        }),
        container(empty()).style(move |s| {
            s.width(3.0).height(8.0).background(red).border_radius(1.5)
        }),
        container(empty()).style(move |s| {
            s.width(3.0).height(11.0).background(red).border_radius(1.5)
        }),
    ))
    .style(|s| s.gap(2.0).items_center().justify_center())
}

fn track_row(m: &Model, idx: usize, id: TrackId, ids: Rc<Vec<TrackId>>) -> impl IntoView {
    let (title, artist, dur, explicit, liked, seed, playing, selected) = {
        let a = m.app.borrow();
        let v = a.library.track_view(id);
        let t = a.library.track(id);
        (
            v.title.to_string(),
            v.artist.to_string(),
            v.duration_sec,
            v.explicit,
            v.liked,
            AppState::album_seed(t.album_id),
            a.current == Some(id),
            a.selection == Some(idx),
        )
    };
    let m2 = m.clone();
    let m4 = m.clone();
    let ids2 = ids.clone();
    let bgc = if selected {
        row_selected()
    } else if playing {
        row_playing()
    } else {
        Color::TRANSPARENT
    };
    let index_cell = container(if playing {
        bars().into_any()
    } else {
        static_label(format!("{}", idx + 1))
            .style(|s| s.font_size(12.0).color(muted_fg()))
            .into_any()
    })
    .style(|s| s.width(24.0).items_center().justify_center());
    let art = art_img(m, seed, 40, 4.0);
    let title_row = h_stack((
        static_label(trunc(&title, 44)).style(move |s| {
            s.font_size(14.0)
                .font_weight(Weight::MEDIUM)
                .color(if playing { primary() } else { fg() })
        }),
        if explicit {
            container(static_label("E").style(|s| s.font_size(10.0).color(muted_fg())))
                .style(|s| {
                    s.background(muted())
                        .border_radius(4.0)
                        .padding_horiz(5.0)
                        .margin_left(6.0)
                })
                .into_any()
        } else {
            empty().into_any()
        },
    ))
    .style(|s| s.items_center());
    let names = v_stack((
        title_row,
        static_label(trunc(&artist, 48)).style(|s| s.font_size(12.0).color(muted_fg())),
    ))
    .style(|s| s.flex_grow(1.0).justify_center());
    let right = if liked {
        container(ic(HEART_ACTIVE, theme::DARK.primary, 16.0))
            .style(|s| s.padding(6.0).cursor(CursorStyle::Pointer))
            .on_click_stop(move |_| {
                m4.app.borrow_mut().toggle_like(id);
                m4.bump();
            })
            .into_any()
    } else {
        static_label(mmss(dur))
            .style(|s| s.font_size(12.0).color(muted_fg()))
            .into_any()
    };
    container(
        h_stack((index_cell, art, names, right)).style(|s| {
            s.items_center()
                .gap(12.0)
                .padding_horiz(12.0)
                .width_full()
                .height_full()
        }),
    )
    .style(move |s| {
        s.height(52.0)
            .width_full()
            .border_radius(8.0)
            .background(bgc)
            .hover(|s| s.background(card()))
            .cursor(CursorStyle::Pointer)
    })
    .on_click_stop(move |_| {
        m2.play_ids(&ids2, idx);
    })
}

/// Build all rows of a static (non-virtualized) track list.
fn track_list(m: &Model, ids: &Rc<Vec<TrackId>>) -> impl IntoView {
    let rows: Vec<_> = ids
        .iter()
        .enumerate()
        .map(|(i, id)| track_row(m, i, *id, ids.clone()).into_any())
        .collect();
    v_stack_from_iter(rows).style(|s| s.width_full().gap(2.0))
}

// — Album / artist cards —————————————————————————————————————————

fn album_card(m: &Model, album_id: AlbumId) -> impl IntoView {
    let (title, artist_name, year, seed) = {
        let a = m.app.borrow();
        let al = a.library.album(album_id);
        (
            al.title.clone(),
            a.library.artist(al.artist_id).name.clone(),
            al.year,
            AppState::album_seed(album_id),
        )
    };
    let key = (album_id as u64).wrapping_mul(0x9e3779b1);
    let m2 = m.clone();
    let m3 = m.clone();
    let m4 = m.clone();
    let m5 = m.clone();
    let art = container((
        art_img(&m2, seed, 176, 4.0),
        dyn_container(
            move || m3.hover_card.get(),
            move |hov| {
                if hov == Some(key) {
                    container(
                        container(ic(PLAY, theme::DARK.foreground, 20.0)).style(|s| {
                            s.width(48.0)
                                .height(48.0)
                                .border_radius(24.0)
                                .background(primary())
                                .items_center()
                                .justify_center()
                        }),
                    )
                    .style(|s| {
                        s.width(176.0)
                            .height(176.0)
                            .absolute()
                            .items_center()
                            .justify_center()
                            .background(Color::rgba8(0, 0, 0, 128))
                            .border_radius(4.0)
                    })
                    .into_any()
                } else {
                    empty().into_any()
                }
            },
        ),
    ))
    .style(|s| s.width(176.0).height(176.0))
    .on_event(EventListener::PointerEnter, move |_| {
        m4.hover_card.set(Some(key));
        EventPropagation::Continue
    })
    .on_event(EventListener::PointerLeave, move |_| {
        if m4.hover_card.get_untracked() == Some(key) {
            m4.hover_card.set(None);
        }
        EventPropagation::Continue
    });
    container(
        v_stack((
            art,
            static_label(trunc(&title, 30)).style(|s| {
                s.font_size(14.0)
                    .font_weight(Weight::MEDIUM)
                    .color(fg())
                    .margin_top(8.0)
            }),
            static_label(trunc(&format!("{} · {}", artist_name, year), 34))
                .style(|s| s.font_size(12.0).color(muted_fg())),
        ))
        .style(|s| s.width(176.0)),
    )
    .style(|s| {
        s.border_radius(8.0)
            .cursor(CursorStyle::Pointer)
            .hover(|s| s.background(card()))
    })
    .on_click_stop(move |_| {
        m5.app.borrow_mut().open_album(album_id);
        m5.bump();
    })
}

fn artist_card(m: &Model, artist_id: ArtistId) -> impl IntoView {
    let (name, seed) = {
        let a = m.app.borrow();
        (
            a.library.artist(artist_id).name.clone(),
            AppState::artist_seed(artist_id),
        )
    };
    let m2 = m.clone();
    container(
        v_stack((
            art_img(m, seed, 176, 88.0),
            static_label(trunc(&name, 24)).style(|s| {
                s.font_size(14.0)
                    .font_weight(Weight::MEDIUM)
                    .color(fg())
                    .margin_top(8.0)
            }),
        ))
        .style(|s| s.width(176.0).items_center()),
    )
    .style(|s| s.cursor(CursorStyle::Pointer))
    .on_click_stop(move |_| {
        m2.app.borrow_mut().open_artist(artist_id);
        m2.bump();
    })
}

fn mix_card(m: &Model, mix_idx: usize) -> impl IntoView {
    let (title, desc, seed, ids) = {
        let a = m.app.borrow();
        let mx = &a.library.mixes[mix_idx];
        (
            mx.title.clone(),
            mx.description.clone().unwrap_or_default(),
            AppState::playlist_seed(mx.id),
            Rc::new(mx.track_ids.clone()),
        )
    };
    let m2 = m.clone();
    container(
        v_stack((
            art_img(m, seed, 176, 4.0),
            static_label(trunc(&title, 30)).style(|s| {
                s.font_size(14.0)
                    .font_weight(Weight::MEDIUM)
                    .color(fg())
                    .margin_top(8.0)
            }),
            static_label(trunc(&desc, 38)).style(|s| s.font_size(12.0).color(muted_fg())),
        ))
        .style(|s| s.width(176.0)),
    )
    .style(|s| {
        s.border_radius(8.0)
            .cursor(CursorStyle::Pointer)
            .hover(|s| s.background(card()))
    })
    .on_click_stop(move |_| {
        m2.play_ids(&ids, 0);
    })
}

// — Sidebar ——————————————————————————————————————————————————————

fn nav_btn(m: &Model, icon: &'static str, name: &str, target: ZView, active: bool) -> impl IntoView {
    let m2 = m.clone();
    let col = if active { fg() } else { muted_fg() };
    let bgc = if active { card() } else { Color::TRANSPARENT };
    let wt = if active { Weight::SEMIBOLD } else { Weight::NORMAL };
    let icon_col = if active {
        theme::DARK.foreground
    } else {
        theme::DARK.muted_foreground
    };
    container(
        h_stack((
            ic(icon, icon_col, 18.0),
            static_label(name.to_string())
                .style(move |s| s.font_size(14.0).font_weight(wt).color(col)),
        ))
        .style(|s| {
            s.items_center()
                .gap(12.0)
                .padding_horiz(12.0)
                .width_full()
                .height_full()
        }),
    )
    .style(move |s| {
        s.height(36.0)
            .width_full()
            .border_radius(8.0)
            .background(bgc)
            .hover(|s| s.background(card()))
            .cursor(CursorStyle::Pointer)
    })
    .on_click_stop(move |_| {
        m2.go(target.clone());
    })
}

fn playlist_row(m: &Model, pid: PlaylistId) -> impl IntoView {
    let (title, n, seed, active) = {
        let a = m.app.borrow();
        let p = a.library.playlist(pid).unwrap();
        (
            p.title.clone(),
            p.track_ids.len(),
            AppState::playlist_seed(pid),
            a.view == ZView::Playlist(pid),
        )
    };
    let m2 = m.clone();
    let bgc = if active { card() } else { Color::TRANSPARENT };
    container(
        h_stack((
            art_img(m, seed, 40, 4.0),
            v_stack((
                static_label(trunc(&title, 26)).style(|s| {
                    s.font_size(13.0)
                        .font_weight(Weight::MEDIUM)
                        .color(fg())
                }),
                static_label(count_songs(n)).style(|s| s.font_size(11.0).color(muted_fg())),
            ))
            .style(|s| s.justify_center()),
        ))
        .style(|s| {
            s.items_center()
                .gap(10.0)
                .padding(6.0)
                .width_full()
                .height_full()
        }),
    )
    .style(move |s| {
        s.height(52.0)
            .width_full()
            .border_radius(8.0)
            .background(bgc)
            .hover(|s| s.background(card()))
            .cursor(CursorStyle::Pointer)
    })
    .on_click_stop(move |_| {
        m2.app.borrow_mut().open_playlist(pid);
        m2.bump();
    })
}

fn sidebar(m: &Model) -> impl IntoView {
    let m2 = m.clone();
    dyn_container(
        move || m2.rev.get(),
        move |_| {
            let view = m2.app.borrow().view.clone();
            let rows: Vec<_> = m2
                .app
                .borrow()
                .library
                .playlists
                .iter()
                .map(|p| playlist_row(&m2, p.id).into_any())
                .collect();
            v_stack((
                h_stack((
                    container(static_label("z").style(|s| {
                        s.font_size(16.0)
                            .font_weight(Weight::BOLD)
                            .color(Color::WHITE)
                    }))
                    .style(|s| {
                        s.width(28.0)
                            .height(28.0)
                            .border_radius(8.0)
                            .background(primary())
                            .items_center()
                            .justify_center()
                    }),
                    static_label("zuno").style(|s| {
                        s.font_size(18.0)
                            .font_weight(Weight::BOLD)
                            .color(fg())
                    }),
                ))
                .style(|s| {
                    s.items_center()
                        .gap(10.0)
                        .padding_horiz(12.0)
                        .margin_top(16.0)
                        .margin_bottom(12.0)
                }),
                nav_btn(&m2, HOME, "Home", ZView::Home, view == ZView::Home),
                nav_btn(&m2, SEARCH_I, "Search", ZView::Search, view == ZView::Search),
                nav_btn(
                    &m2,
                    LIBRARY,
                    "Library",
                    ZView::Library,
                    view == ZView::Library,
                ),
                nav_btn(
                    &m2,
                    SETTINGS,
                    "Settings",
                    ZView::Settings,
                    view == ZView::Settings,
                ),
                static_label("PLAYLISTS").style(|s| {
                    s.font_size(11.0)
                        .font_weight(Weight::SEMIBOLD)
                        .color(muted_fg())
                        .margin_top(16.0)
                        .margin_bottom(4.0)
                        .padding_horiz(12.0)
                }),
                scroll(v_stack_from_iter(rows).style(|s| s.width_full().gap(2.0)))
                    .style(|s| s.width_full().height_full()),
            ))
            .style(|s| {
                s.width(220.0)
                    .height_full()
                    .padding_horiz(8.0)
                    .background(bg())
            })
            .into_any()
        },
    )
}

// — Header ———————————————————————————————————————————————————————

fn header(m: &Model) -> impl IntoView {
    let m2 = m.clone();
    let m3 = m.clone();
    let can_back = m.app.borrow().can_go_back();
    let back_col = if can_back {
        theme::DARK.foreground
    } else {
        theme::DARK.muted
    };
    let inp = text_input(m.query.clone()).placeholder("Search songs, albums, artists…  (Ctrl+K)");
    let input_id = inp.id();
    m.search_id.set(Some(input_id));
    h_stack((
        container(ic(BACK, back_col, 18.0))
            .style(|s| {
                s.width(36.0)
                    .height(36.0)
                    .border_radius(18.0)
                    .items_center()
                    .justify_center()
                    .hover(|s| s.background(card()))
                    .cursor(CursorStyle::Pointer)
            })
            .on_click_stop(move |_| {
                m2.back();
            }),
        inp.style(|s| {
                s.flex_grow(1.0)
                    .height(38.0)
                    .border_radius(19.0)
                    .background(muted())
                    .padding_horiz(18.0)
                    .font_size(14.0)
                    .color(fg())
            })
            .on_event(EventListener::FocusGained, move |_| {
                m3.search_focused.set(true);
                EventPropagation::Continue
            })
            .on_event(EventListener::FocusLost, move |_| {
                m3.search_focused.set(false);
                EventPropagation::Continue
            }),
    ))
    .style(|s| {
        s.items_center()
            .gap(12.0)
            .width_full()
            .padding_horiz(20.0)
            .padding_vert(12.0)
    })
}

// — Pages ————————————————————————————————————————————————————————

fn page_scroll(m: &Model, child: impl IntoView + 'static) -> impl IntoView {
    let m2 = m.clone();
    let m3 = m.clone();
    scroll(child)
        .style(|s| s.width_full().height_full())
        .scroll_to(move || m2.scroll_page.get())
        .on_scroll(move |r| {
            m3.page_off.set(r.y0);
            m3.page_vh.set(r.height());
        })
}

fn home_page(m: &Model) -> impl IntoView {
    let (recent, new_albums, artists, nmixes) = {
        let a = m.app.borrow();
        let mut als: Vec<AlbumId> = a.library.albums.iter().map(|x| x.id).collect();
        als.sort_by_key(|id| std::cmp::Reverse(a.library.album(*id).year));
        als.truncate(24);
        (
            a.library.home_recent_albums(12),
            als,
            a.library.artists.iter().take(12).map(|x| x.id).collect::<Vec<_>>(),
            a.library.mixes.len(),
        )
    };
    let m2 = m.clone();
    let m3 = m.clone();
    let m4 = m.clone();
    let m5 = m.clone();
    let recent_cards: Vec<_> = recent
        .into_iter()
        .map(|id| album_card(&m2, id).into_any())
        .collect();
    let new_cards: Vec<_> = new_albums
        .chunks(6)
        .map(|ch| {
            h_stack_from_iter(ch.iter().map(|id| album_card(&m3, *id).into_any()))
                .style(|s| s.gap(16.0))
                .into_any()
        })
        .collect();
    let artist_cards: Vec<_> = artists
        .into_iter()
        .map(|id| artist_card(&m4, id).into_any())
        .collect();
    let mix_cards: Vec<_> = (0..nmixes).map(|i| mix_card(&m5, i).into_any()).collect();
    page_scroll(
        m,
        v_stack((
            h1("Home".to_string()),
            h3("Recently played".to_string()),
            scroll(
                h_stack_from_iter(recent_cards)
                    .style(|s| s.gap(16.0).padding_vert(4.0)),
            )
            .style(|s| s.width_full().height(264.0)),
            h3("Made for you".to_string()),
            scroll(
                h_stack_from_iter(mix_cards).style(|s| s.gap(16.0).padding_vert(4.0)),
            )
            .style(|s| s.width_full().height(284.0)),
            h3("New albums".to_string()),
            v_stack_from_iter(new_cards).style(|s| s.gap(20.0).width_full()),
            h3("Popular artists".to_string()),
            scroll(
                h_stack_from_iter(artist_cards).style(|s| s.gap(16.0).padding_vert(4.0)),
            )
            .style(|s| s.width_full().height(244.0)),
        ))
        .style(|s| s.gap(16.0).width_full().padding(20.0)),
    )
}

fn pill_tab(m: &Model, name: &str, tab: LibraryTab, active: bool) -> impl IntoView {
    let m2 = m.clone();
    container(static_label(name.to_string()).style(move |s| {
        s.font_size(13.0)
            .font_weight(Weight::SEMIBOLD)
            .color(if active { fg() } else { muted_fg() })
    }))
    .style(move |s| {
        s.padding_horiz(18.0)
            .padding_vert(8.0)
            .border_radius(17.0)
            .background(if active { card() } else { Color::TRANSPARENT })
            .hover(|s| s.background(card()))
            .cursor(CursorStyle::Pointer)
    })
    .on_click_stop(move |_| {
        m2.app.borrow_mut().set_library_tab(tab);
        m2.scroll_page.set(None);
        m2.bump();
    })
}

/// THE 5,000-track virtualized list (the bench scroll target).
fn songs_list(m: &Model) -> impl IntoView {
    let m2 = m.clone();
    let m3 = m.clone();
    let m4 = m.clone();
    let m5 = m.clone();
    scroll(
        virtual_list(
            VirtualDirection::Vertical,
            VirtualItemSize::Fixed(Box::new(|| 52.0)),
            move || {
                // Tracked: rebuilds the item vector when content changes.
                m2.rev.get();
                m2.results.get();
                let ids = m2.app.borrow().list_ids();
                ids.into_iter()
                    .enumerate()
                    .collect::<im::Vector<(usize, TrackId)>>()
            },
            move |(_, t)| *t,
            move |(idx, id)| {
                let ids = Rc::new(m3.app.borrow().list_ids());
                track_row(&m3, idx, id, ids)
            },
        )
        .style(|s| s.flex_col().width_full()),
    )
    .style(|s| s.width_full().height_full())
    .scroll_to(move || m4.scroll_songs.get())
    .on_scroll(move |r| {
        m5.songs_off.set(r.y0);
        m5.songs_vh.set(r.height());
    })
}

fn library_page(m: &Model) -> impl IntoView {
    let tab = m.app.borrow().library_tab;
    let tabs = h_stack((
        pill_tab(m, "Playlists", LibraryTab::Playlists, tab == LibraryTab::Playlists),
        pill_tab(m, "Albums", LibraryTab::Albums, tab == LibraryTab::Albums),
        pill_tab(m, "Artists", LibraryTab::Artists, tab == LibraryTab::Artists),
        pill_tab(m, "Songs", LibraryTab::Songs, tab == LibraryTab::Songs),
    ))
    .style(|s| s.gap(8.0).margin_bottom(8.0));
    match tab {
        LibraryTab::Songs => v_stack((h1("Your Library".to_string()), tabs, songs_list(m)))
            .style(|s| {
                s.gap(8.0)
                    .width_full()
                    .height_full()
                    .padding(20.0)
            })
            .into_any(),
        _ => {
            let content = match tab {
                LibraryTab::Playlists => {
                    let rows: Vec<_> = m
                        .app
                        .borrow()
                        .library
                        .playlists
                        .iter()
                        .map(|p| playlist_row(m, p.id).into_any())
                        .collect();
                    v_stack_from_iter(rows)
                        .style(|s| s.width_full().gap(2.0))
                        .into_any()
                }
                LibraryTab::Albums => {
                    let aids: Vec<AlbumId> =
                        m.app.borrow().library.albums.iter().map(|x| x.id).collect();
                    let grid: Vec<_> = aids
                        .chunks(5)
                        .map(|ch| {
                            h_stack_from_iter(
                                ch.iter().map(|id| album_card(m, *id).into_any()),
                            )
                            .style(|s| s.gap(16.0))
                            .into_any()
                        })
                        .collect();
                    v_stack_from_iter(grid)
                        .style(|s| s.gap(20.0).width_full())
                        .into_any()
                }
                LibraryTab::Artists => {
                    let aids: Vec<ArtistId> =
                        m.app.borrow().library.artists.iter().map(|x| x.id).collect();
                    let rows: Vec<_> =
                        aids.into_iter().map(|id| artist_row(m, id).into_any()).collect();
                    v_stack_from_iter(rows)
                        .style(|s| s.width_full().gap(2.0))
                        .into_any()
                }
                LibraryTab::Songs => empty().into_any(),
            };
            v_stack((h1("Your Library".to_string()), tabs, page_scroll(m, content)))
                .style(|s| {
                    s.gap(8.0)
                        .width_full()
                        .height_full()
                        .padding(20.0)
                })
                .into_any()
        }
    }
}

fn artist_row(m: &Model, artist_id: ArtistId) -> impl IntoView {
    let (name, n_albums, n_tracks, seed) = {
        let a = m.app.borrow();
        let ar = a.library.artist(artist_id);
        let n_tracks: usize = ar
            .album_ids
            .iter()
            .map(|id| a.library.album(*id).track_ids.len())
            .sum();
        (
            ar.name.clone(),
            ar.album_ids.len(),
            n_tracks,
            AppState::artist_seed(artist_id),
        )
    };
    let m2 = m.clone();
    container(
        h_stack((
            art_img(m, seed, 56, 28.0),
            v_stack((
                static_label(trunc(&name, 40)).style(|s| {
                    s.font_size(14.0)
                        .font_weight(Weight::MEDIUM)
                        .color(fg())
                }),
                static_label(format!("{} albums · {} songs", n_albums, n_tracks))
                    .style(|s| s.font_size(12.0).color(muted_fg())),
            ))
            .style(|s| s.justify_center()),
        ))
        .style(|s| {
            s.items_center()
                .gap(12.0)
                .padding_horiz(12.0)
                .width_full()
                .height_full()
        }),
    )
    .style(|s| {
        s.height(68.0)
            .width_full()
            .border_radius(8.0)
            .hover(|s| s.background(card()))
            .cursor(CursorStyle::Pointer)
    })
    .on_click_stop(move |_| {
        m2.app.borrow_mut().open_artist(artist_id);
        m2.bump();
    })
}

fn play_pill(m: &Model, ids: Rc<Vec<TrackId>>) -> impl IntoView {
    let m2 = m.clone();
    container(
        h_stack((
            ic(PLAY, theme::DARK.foreground, 16.0),
            static_label("Play".to_string()).style(|s| {
                s.font_size(14.0)
                    .font_weight(Weight::SEMIBOLD)
                    .color(fg())
            }),
        ))
        .style(|s| s.items_center().gap(8.0)),
    )
    .style(|s| {
        s.padding_horiz(24.0)
            .padding_vert(10.0)
            .border_radius(20.0)
            .background(primary())
            .hover(|s| s.background(rgb(theme::DARK.primary.darken(0.15))))
            .cursor(CursorStyle::Pointer)
    })
    .on_click_stop(move |_| {
        m2.play_ids(&ids, 0);
    })
}

fn shuffle_pill(m: &Model, ids: Rc<Vec<TrackId>>) -> impl IntoView {
    let m2 = m.clone();
    container(
        h_stack((
            ic(SHUFFLE, theme::DARK.foreground, 16.0),
            static_label("Shuffle".to_string()).style(|s| {
                s.font_size(14.0)
                    .font_weight(Weight::SEMIBOLD)
                    .color(fg())
            }),
        ))
        .style(|s| s.items_center().gap(8.0)),
    )
    .style(|s| {
        s.padding_horiz(24.0)
            .padding_vert(10.0)
            .border_radius(20.0)
            .background(card())
            .hover(|s| s.background(muted()))
            .cursor(CursorStyle::Pointer)
    })
    .on_click_stop(move |_| {
        if !ids.is_empty() {
            use rand_helper::pick;
            let i = pick(ids.len());
            m2.play_ids(&ids, i);
            if !m2.app.borrow().shuffle {
                m2.app.borrow_mut().toggle_shuffle();
            }
            m2.bump();
        }
    })
}

mod rand_helper {
    use std::time::{SystemTime, UNIX_EPOCH};
    pub fn pick(n: usize) -> usize {
        let t = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.subsec_nanos() as usize)
            .unwrap_or(0);
        // xorshift
        let mut x = t.wrapping_add(0x9e3779b97f4a7c15).max(1);
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        (x.wrapping_mul(0x2545F4914F6CDD1D) >> 32) as usize % n.max(1)
    }
}

fn collection_page(
    m: &Model,
    art_seed: u64,
    kind: &str,
    title: String,
    meta: String,
    counts: String,
    ids: Rc<Vec<TrackId>>,
) -> impl IntoView {
    let buttons = h_stack((play_pill(m, ids.clone()), shuffle_pill(m, ids.clone())))
        .style(|s| s.gap(12.0).margin_top(16.0));
    page_scroll(
        m,
        v_stack((
            h_stack((
                art_img(m, art_seed, 232, 4.0),
                v_stack((
                    static_label(kind.to_string()).style(|s| {
                        s.font_size(12.0)
                            .font_weight(Weight::SEMIBOLD)
                            .color(muted_fg())
                    }),
                    static_label(trunc(&title, 40)).style(|s| {
                        s.font_size(32.0)
                            .font_weight(Weight::BOLD)
                            .color(fg())
                    }),
                    static_label(meta).style(|s| s.font_size(13.0).color(muted_fg())),
                    static_label(counts).style(|s| s.font_size(13.0).color(muted_fg())),
                    buttons,
                ))
                .style(|s| s.justify_end().padding_bottom(4.0)),
            ))
            .style(|s| s.gap(24.0).margin_bottom(12.0)),
            track_list(m, &ids),
        ))
        .style(|s| s.width_full().padding(20.0).gap(8.0)),
    )
}

fn album_page(m: &Model, album_id: AlbumId) -> impl IntoView {
    let (title, meta, counts, ids) = {
        let a = m.app.borrow();
        let al = a.library.album(album_id);
        let artist = a.library.artist(al.artist_id).name.clone();
        let durs = al.track_ids.iter().map(|id| a.library.track(*id).duration_sec);
        (
            al.title.clone(),
            format!("Album · {} · {}", artist, al.year),
            counts_line(durs),
            Rc::new(al.track_ids.clone()),
        )
    };
    collection_page(
        m,
        AppState::album_seed(album_id),
        "ALBUM",
        title,
        meta,
        counts,
        ids,
    )
}

fn playlist_page(m: &Model, pid: PlaylistId) -> impl IntoView {
    let (title, desc, counts, ids) = {
        let a = m.app.borrow();
        let p = a.library.playlist(pid).unwrap();
        let durs = p.track_ids.iter().map(|id| a.library.track(*id).duration_sec);
        (
            p.title.clone(),
            p.description.clone().unwrap_or_default(),
            counts_line(durs),
            Rc::new(p.track_ids.clone()),
        )
    };
    collection_page(
        m,
        AppState::playlist_seed(pid),
        "PLAYLIST",
        title,
        desc,
        counts,
        ids,
    )
}

fn artist_page(m: &Model, artist_id: ArtistId) -> impl IntoView {
    let (name, fans, top, aids) = {
        let a = m.app.borrow();
        let ar = a.library.artist(artist_id);
        let mut top = Vec::new();
        for aid in &ar.album_ids {
            for tid in &a.library.album(*aid).track_ids {
                if top.len() >= 10 {
                    break;
                }
                top.push(*tid);
            }
        }
        (
            ar.name.clone(),
            listeners(ar.monthly_listeners),
            Rc::new(top),
            ar.album_ids.clone(),
        )
    };
    let m2 = m.clone();
    let grid: Vec<_> = aids
        .chunks(5)
        .map(|ch| {
            h_stack_from_iter(ch.iter().map(|id| album_card(&m2, *id).into_any()))
                .style(|s| s.gap(16.0))
                .into_any()
        })
        .collect();
    page_scroll(
        m,
        v_stack((
            h_stack((
                art_img(m, AppState::artist_seed(artist_id), 232, 116.0),
                v_stack((
                    static_label("ARTIST".to_string()).style(|s| {
                        s.font_size(12.0)
                            .font_weight(Weight::SEMIBOLD)
                            .color(muted_fg())
                    }),
                    static_label(trunc(&name, 36)).style(|s| {
                        s.font_size(32.0)
                            .font_weight(Weight::BOLD)
                            .color(fg())
                    }),
                    static_label(format!("{} monthly listeners", fans))
                        .style(|s| s.font_size(13.0).color(muted_fg())),
                    h_stack((play_pill(m, top.clone()),)).style(|s| s.margin_top(16.0)),
                ))
                .style(|s| s.justify_end().padding_bottom(4.0)),
            ))
            .style(|s| s.gap(24.0).margin_bottom(12.0)),
            h3("Top tracks".to_string()),
            track_list(m, &top),
            h3("Albums".to_string()),
            v_stack_from_iter(grid).style(|s| s.gap(20.0).width_full()),
        ))
        .style(|s| s.width_full().padding(20.0).gap(12.0)),
    )
}

fn search_page(m: &Model) -> impl IntoView {
    let q = m.query.get_untracked();
    if q.trim().is_empty() {
        return page_scroll(
            m,
            v_stack((
                h1("Search".to_string()),
                static_label("Type to search 5,000 tracks, albums and artists.".to_string())
                    .style(|s| s.font_size(14.0).color(muted_fg())),
            ))
            .style(|s| s.gap(12.0).width_full().padding(20.0)),
        )
        .into_any();
    }
    let res = search::search(&m.app.borrow().library, &q);
    let top_id = res.tracks.first().copied();
    let (top_title, top_artist, top_seed) = top_id
        .map(|id| {
            let a = m.app.borrow();
            let v = a.library.track_view(id);
            let t = a.library.track(id);
            (
                v.title.to_string(),
                v.artist.to_string(),
                AppState::album_seed(t.album_id),
            )
        })
        .unwrap_or_default();
    let m2 = m.clone();
    let song_ids = Rc::new(res.tracks.iter().take(30).copied().collect::<Vec<_>>());
    let song_ids2 = song_ids.clone();
    let album_cards: Vec<_> = res
        .albums
        .iter()
        .take(12)
        .map(|id| album_card(&m2, *id).into_any())
        .collect();
    let artist_cards: Vec<_> = res
        .artists
        .iter()
        .take(12)
        .map(|id| artist_card(&m2, *id).into_any())
        .collect();
    let top_card = match top_id {
        Some(id) => {
            let m3 = m.clone();
            let ids3 = song_ids2.clone();
            container(
                h_stack((
                    art_img(m, top_seed, 96, 4.0),
                    v_stack((
                        static_label("TOP RESULT".to_string()).style(|s| {
                            s.font_size(11.0)
                                .font_weight(Weight::SEMIBOLD)
                                .color(muted_fg())
                        }),
                        static_label(trunc(&top_title, 36)).style(|s| {
                            s.font_size(20.0)
                                .font_weight(Weight::BOLD)
                                .color(fg())
                        }),
                        static_label(trunc(&top_artist, 44))
                            .style(|s| s.font_size(13.0).color(muted_fg())),
                    ))
                    .style(|s| s.justify_center()),
                ))
                .style(|s| s.gap(16.0)),
            )
            .style(|s| {
                s.background(card())
                    .border_radius(8.0)
                    .padding(16.0)
                    .cursor(CursorStyle::Pointer)
            })
            .on_click_stop(move |_| {
                if let Some(pos) = ids3.iter().position(|t| *t == id) {
                    m3.play_ids(&ids3, pos);
                }
            })
            .into_any()
        }
        None => static_label("No results.".to_string())
            .style(|s| s.font_size(14.0).color(muted_fg()))
            .into_any(),
    };
    page_scroll(
        m,
        v_stack((
            h1(format!("Results for “{}”", trunc(&q, 32))),
            top_card,
            h3("Songs".to_string()),
            track_list(m, &song_ids),
            h3("Albums".to_string()),
            scroll(h_stack_from_iter(album_cards).style(|s| s.gap(16.0)))
                .style(|s| s.width_full().height(264.0)),
            h3("Artists".to_string()),
            scroll(h_stack_from_iter(artist_cards).style(|s| s.gap(16.0)))
                .style(|s| s.width_full().height(244.0)),
        ))
        .style(|s| s.gap(14.0).width_full().padding(20.0)),
    )
    .into_any()
}

fn settings_page(m: &Model) -> impl IntoView {
    let row = |name: &str, desc: &str, on: bool| {
        h_stack((
            v_stack((
                static_label(name.to_string()).style(|s| {
                    s.font_size(14.0)
                        .font_weight(Weight::MEDIUM)
                        .color(fg())
                }),
                static_label(desc.to_string()).style(|s| s.font_size(12.0).color(muted_fg())),
            ))
            .style(|s| s.flex_grow(1.0).justify_center()),
            container(static_label(if on { "On" } else { "Off" }.to_string()).style(
                move |s| {
                    s.font_size(12.0)
                        .font_weight(Weight::SEMIBOLD)
                        .color(if on { fg() } else { muted_fg() })
                },
            ))
            .style(move |s| {
                s.padding_horiz(14.0)
                    .padding_vert(6.0)
                    .border_radius(13.0)
                    .background(if on { primary() } else { card() })
            }),
        ))
        .style(|s| {
            s.items_center()
                .width_full()
                .padding_vert(10.0)
                .padding_horiz(4.0)
        })
    };
    page_scroll(
        m,
        v_stack((
            h1("Settings".to_string()),
            h3("Appearance".to_string()),
            row("Theme", "Dark is pinned for the benchmark", true),
            h3("Playback".to_string()),
            row("Audio quality", "High · 320 kbps Opus (synth in benchmark)", true),
            row("Normalize volume", "Match loudness across tracks", true),
            row("Autoplay", "Keep playing similar music", true),
            row("Explicit content", "Allow tracks tagged explicit", false),
            h3("About".to_string()),
            static_label("Zuno native benchmark · floem 0.2 · settings are rendered, not wired.".to_string())
                .style(|s| s.font_size(12.0).color(muted_fg())),
        ))
        .style(|s| s.gap(10.0).width_full().padding(20.0)),
    )
}

fn page(m: &Model) -> impl IntoView {
    let m2 = m.clone();
    let app = m2.app.clone();
    dyn_container(
        move || {
            m2.rev.get();
            m2.results.get();
            app.borrow().view.clone()
        },
        move |view| match view {
            ZView::Home => home_page(&m2).into_any(),
            ZView::Library => library_page(&m2).into_any(),
            ZView::Album(id) => album_page(&m2, id).into_any(),
            ZView::Artist(id) => artist_page(&m2, id).into_any(),
            ZView::Playlist(id) => playlist_page(&m2, id).into_any(),
            ZView::Search => search_page(&m2).into_any(),
            ZView::Settings => settings_page(&m2).into_any(),
        },
    )
    .style(|s| s.flex_grow(1.0).height_full())
}

// — Queue panel ————————————————————————————————————————————————————

fn queue_item(m: &Model, idx: usize, id: TrackId, removable: bool) -> impl IntoView {
    let (title, artist, seed) = {
        let a = m.app.borrow();
        let v = a.library.track_view(id);
        let t = a.library.track(id);
        (
            v.title.to_string(),
            v.artist.to_string(),
            AppState::album_seed(t.album_id),
        )
    };
    let m2 = m.clone();
    let m3 = m.clone();
    container(
        h_stack((
            art_img(m, seed, 40, 4.0),
            v_stack((
                static_label(trunc(&title, 30)).style(|s| {
                    s.font_size(13.0)
                        .font_weight(Weight::MEDIUM)
                        .color(fg())
                }),
                static_label(trunc(&artist, 34)).style(|s| s.font_size(11.0).color(muted_fg())),
            ))
            .style(|s| s.flex_grow(1.0).justify_center()),
            if removable {
                container(ic(CLOSE, theme::DARK.muted_foreground, 14.0))
                    .style(|s| {
                        s.padding(6.0)
                            .border_radius(12.0)
                            .hover(|s| s.background(card()))
                            .cursor(CursorStyle::Pointer)
                    })
                    .on_click_stop(move |_| {
                        m3.app.borrow_mut().queue.remove_at(idx);
                        m3.bump();
                    })
                    .into_any()
            } else {
                empty().into_any()
            },
        ))
        .style(|s| {
            s.items_center()
                .gap(10.0)
                .padding(6.0)
                .width_full()
                .height_full()
        }),
    )
    .style(|s| {
        s.height(52.0)
            .width_full()
            .border_radius(8.0)
            .hover(|s| s.background(card()))
            .cursor(CursorStyle::Pointer)
    })
    .on_click_stop(move |_| {
        if let Some(id) = m2.app.borrow_mut().queue.jump_to(idx) {
            m2.app.borrow_mut().play_track(id);
        }
        m2.bump();
    })
}

fn queue_panel(m: &Model) -> impl IntoView {
    let m2 = m.clone();
    dyn_container(
        move || {
            m2.rev.get();
            m2.queue_open.get()
        },
        move |_| {
            if !m2.queue_open.get_untracked() {
                return empty().into_any();
            }
            let rows = m2.app.borrow().queue_rows();
            let mut cur = Vec::new();
            let mut manual = Vec::new();
            let mut auto = Vec::new();
            for (i, (id, region)) in rows.iter().enumerate() {
                match region {
                    Region::Current => {
                        cur.push(queue_item(&m2, i, *id, false).into_any())
                    }
                    Region::Manual => {
                        manual.push(queue_item(&m2, i, *id, true).into_any())
                    }
                    Region::Automatic => {
                        auto.push(queue_item(&m2, i, *id, true).into_any())
                    }
                    Region::Played => {}
                }
            }
            let section = |name: &str, items: Vec<AnyView>| {
                v_stack((
                    static_label(name.to_string()).style(|s| {
                        s.font_size(11.0)
                            .font_weight(Weight::SEMIBOLD)
                            .color(muted_fg())
                            .margin_bottom(4.0)
                    }),
                    v_stack_from_iter(items).style(|s| s.width_full().gap(2.0)),
                ))
                .style(|s| s.width_full().margin_bottom(12.0))
            };
            scroll(
                v_stack((
                    static_label("Queue".to_string()).style(|s| {
                        s.font_size(18.0)
                            .font_weight(Weight::BOLD)
                            .color(fg())
                            .margin_bottom(8.0)
                    }),
                    section("NOW PLAYING", cur),
                    section("NEXT IN QUEUE", manual),
                    section("NEXT UP", auto),
                ))
                .style(|s| s.width_full().padding(16.0)),
            )
            .style(|s| s.width_full().height_full())
            .into_any()
        },
    )
    .style(|s| s.width(300.0).height_full().background(rgb(theme::DARK.popover)))
}

// — Player bar —————————————————————————————————————————————————————

fn ctrl_btn(
    m: &Model,
    icon: &'static str,
    color: Rgb,
    active_bg: bool,
    size: f64,
    action: impl Fn(&Model) + 'static,
) -> impl IntoView {
    let m2 = m.clone();
    let bgc = if active_bg { card() } else { Color::TRANSPARENT };
    container(ic(icon, color, 18.0))
        .style(move |s| {
            s.width(size)
                .height(size)
                .border_radius(size / 2.0)
                .background(bgc)
                .items_center()
                .justify_center()
                .hover(|s| s.background(card()))
                .cursor(CursorStyle::Pointer)
        })
        .on_click_stop(move |_| {
            action(&m2);
        })
}

fn player_bar(m: &Model) -> impl IntoView {
    let m2 = m.clone();
    // Left: artwork + titles (reactive labels, no rebuild).
    let art_dyn = dyn_container(
        move || m2.rev.get(),
        move |_| {
            let (seed, album_id) = {
                let a = m2.app.borrow();
                match a.current_track() {
                    Some(t) => (AppState::album_seed(t.album_id), Some(t.album_id)),
                    None => (0u64, None),
                }
            };
            let m3 = m2.clone();
            container(art_img(&m2, seed, 40, 4.0))
                .style(|s| s.cursor(CursorStyle::Pointer))
                .on_click_stop(move |_| {
                    if let Some(id) = album_id {
                        m3.app.borrow_mut().open_album(id);
                        m3.bump();
                    }
                })
                .into_any()
        },
    );
    let m3 = m.clone();
    let m4 = m.clone();
    let title_lbl = label(move || {
        m3.rev.get();
        m3.app
            .borrow()
            .current_track()
            .map(|t| trunc(&t.title, 32))
            .unwrap_or_else(|| "Nothing playing".to_string())
    })
    .style(|s| s.font_size(13.0).font_weight(Weight::MEDIUM).color(fg()));
    let artist_lbl = label(move || {
        m4.rev.get();
        m4.app
            .borrow()
            .current_track()
            .map(|t| {
                let a = m4.app.borrow();
                trunc(&a.library.artist(t.artist_id).name, 36)
            })
            .unwrap_or_default()
    })
    .style(|s| s.font_size(12.0).color(muted_fg()));
    let left = h_stack((art_dyn, v_stack((title_lbl, artist_lbl)).style(|s| s.justify_center())))
        .style(|s| s.items_center().gap(10.0).width(300.0));

    // Center: controls + seek.
    let m5 = m.clone();
    let play_dyn = dyn_container(
        move || m5.rev.get(),
        move |_| {
            let playing = m5.app.borrow().playing;
            let m6 = m5.clone();
            container(ic(
                if playing { PAUSE } else { PLAY },
                theme::DARK.foreground,
                18.0,
            ))
            .style(|s| {
                s.width(40.0)
                    .height(40.0)
                    .border_radius(20.0)
                    .background(primary())
                    .items_center()
                    .justify_center()
                    .hover(|s| s.background(rgb(theme::DARK.primary.darken(0.15))))
                    .cursor(CursorStyle::Pointer)
            })
            .on_click_stop(move |_| {
                m6.app.borrow_mut().toggle_play();
                m6.bump();
            })
            .into_any()
        },
    );
    let m7 = m.clone();
    let m8 = m.clone();
    let m9 = m.clone();
    let m10 = m.clone();
    let shuffle_dyn = dyn_container(
        move || m7.rev.get(),
        move |_| {
            let on = m7.app.borrow().shuffle;
            ctrl_btn(
                &m7,
                SHUFFLE,
                if on {
                    theme::DARK.primary
                } else {
                    theme::DARK.muted_foreground
                },
                on,
                32.0,
                |m| {
                    m.app.borrow_mut().toggle_shuffle();
                    m.bump();
                },
            )
            .into_any()
        },
    );
    let repeat_dyn = dyn_container(
        move || m8.rev.get(),
        move |_| {
            let rep = m8.app.borrow().repeat;
            ctrl_btn(
                &m8,
                match rep {
                    RepeatMode::One => REPEAT_ONE,
                    _ => REPEAT,
                },
                if rep == RepeatMode::Off {
                    theme::DARK.muted_foreground
                } else {
                    theme::DARK.primary
                },
                rep != RepeatMode::Off,
                32.0,
                |m| {
                    m.app.borrow_mut().cycle_repeat();
                    m.bump();
                },
            )
            .into_any()
        },
    );
    let controls = h_stack((
        shuffle_dyn,
        ctrl_btn(&m9, PREV, theme::DARK.foreground, false, 32.0, |m| {
            m.app.borrow_mut().previous();
            m.bump();
        }),
        play_dyn,
        ctrl_btn(&m9, NEXT, theme::DARK.foreground, false, 32.0, |m| {
            m.app.borrow_mut().next();
            m.bump();
        }),
        repeat_dyn,
    ))
    .style(|s| s.items_center().gap(4.0).justify_center());

    let m11 = m.clone();
    let m12 = m.clone();
    let m13 = m.clone();
    let elapsed = label(move || {
        m11.pos.get();
        mmss(m11.app.borrow().player.position_sec() as u32)
    })
    .style(|s| s.font_size(12.0).color(muted_fg()));
    let total = label(move || {
        m12.pos.get();
        m12.rev.get();
        mmss(m12.app.borrow().player.duration_sec() as u32)
    })
    .style(|s| s.font_size(12.0).color(muted_fg()));
    let seek = slider(move || {
        m13.pos.get();
        m13.rev.get();
        let a = m13.app.borrow();
        let d = a.player.duration_sec();
        Pct(if d > 0.0 {
            (a.player.position_sec() / d * 100.0) as f64
        } else {
            0.0
        })
    })
    .on_change_pct(move |p| {
        m10.app.borrow_mut().seek(
            m10.app.borrow().player.duration_sec() * (p.0.clamp(0.0, 100.0) / 100.0),
        );
        m10.pos.update(|v| *v += 1);
    })
    .slider_style(|s| {
        s.bar_height(4.0)
            .accent_bar_height(4.0)
            .bar_radius(2.0)
            .accent_bar_radius(2.0)
            .bar_color(muted())
            .accent_bar_color(primary())
            .handle_color(Some(Color::WHITE.into()))
            .handle_radius(6.0)
    })
    .style(|s| s.flex_grow(1.0).height(20.0));
    let center = v_stack((
        controls,
        h_stack((elapsed, seek, total)).style(|s| {
            s.items_center().gap(10.0).width_full()
        }),
    ))
    .style(|s| s.flex_grow(1.0).gap(2.0).justify_center());

    // Right: like, queue toggle, volume.
    let m14 = m.clone();
    let m15 = m.clone();
    let m16 = m.clone();
    let m17 = m.clone();
    let like_dyn = dyn_container(
        move || m14.rev.get(),
        move |_| {
            let liked = m14
                .app
                .borrow()
                .current
                .map(|id| m14.app.borrow().library.track(id).liked)
                .unwrap_or(false);
            let cur = m14.app.borrow().current;
            let m18 = m14.clone();
            ctrl_btn(
                &m14,
                if liked { HEART_ACTIVE } else { HEART },
                if liked {
                    theme::DARK.primary
                } else {
                    theme::DARK.muted_foreground
                },
                false,
                32.0,
                move |_| {
                    if let Some(id) = cur {
                        m18.app.borrow_mut().toggle_like(id);
                        m18.bump();
                    }
                },
            )
            .into_any()
        },
    );
    let queue_dyn = dyn_container(
        move || m15.queue_open.get(),
        move |_| {
            let open = m15.queue_open.get_untracked();
            ctrl_btn(
                &m15,
                QUEUE_I,
                if open {
                    theme::DARK.primary
                } else {
                    theme::DARK.muted_foreground
                },
                open,
                32.0,
                |m| {
                    m.queue_open.update(|v| *v = !*v);
                },
            )
            .into_any()
        },
    );
    let vol_dyn = dyn_container(
        move || m16.rev.get(),
        move |_| {
            let muted_now = m16.app.borrow().muted;
            ctrl_btn(
                &m16,
                if muted_now { VOLUME_MUTE } else { VOLUME },
                theme::DARK.muted_foreground,
                false,
                32.0,
                |m| {
                    m.app.borrow_mut().toggle_mute();
                    m.bump();
                },
            )
            .into_any()
        },
    );
    let vol_app = m17.app.clone();
    let vol = slider(move || {
        m17.rev.get();
        Pct((vol_app.borrow().volume * 100.0) as f64)
    })
    .on_change_pct(move |p| {
        m17.app.borrow_mut().set_volume((p.0 as f32 / 100.0).clamp(0.0, 1.0));
        m17.bump();
    })
    .slider_style(|s| {
        s.bar_height(4.0)
            .accent_bar_height(4.0)
            .bar_radius(2.0)
            .accent_bar_radius(2.0)
            .bar_color(muted())
            .accent_bar_color(primary())
            .handle_color(Some(Color::WHITE.into()))
            .handle_radius(6.0)
    })
    .style(|s| s.width(100.0).height(20.0));
    let right = h_stack((like_dyn, queue_dyn, vol_dyn, vol))
        .style(|s| s.items_center().gap(4.0).width(300.0).justify_end());

    h_stack((left, center, right))
        .style(|s| {
            s.height(76.0)
                .width_full()
                .items_center()
                .gap(16.0)
                .padding_horiz(16.0)
                .background(rgb(theme::DARK.popover))
        })
}

// — Frame tick + bench —————————————————————————————————————————————

fn tick_once(m: &Model) {
    if !m.focused_once.get() {
        m.focused_once.set(true);
        if let Some(id) = m.root_id.get() {
            id.request_focus();
        }
        // Create the driver on the NEXT tick so the first frame is on screen.
        if m.bench_mode {
            return;
        }
    }
    m.app.borrow_mut().tick();
    let n = m.tick_n.get() + 1;
    m.tick_n.set(n);
    if n % 3 == 0 {
        m.pos.update(|v| *v += 1);
    }
    if m.bench_mode {
        let mut bg = m.bench.borrow_mut();
        if bg.is_none() {
            *bg = Some(BenchDriver::new());
            return;
        }
        let b = bg.as_mut().unwrap();
        match b.tick() {
            BenchAction::RecordStartup => {}
            BenchAction::ScrollTo(f) => {
                {
                    let mut a = m.app.borrow_mut();
                    if a.view != ZView::Library || a.library_tab != LibraryTab::Songs {
                        a.view = ZView::Library;
                        a.library_tab = LibraryTab::Songs;
                        drop(a);
                        m.bump();
                    }
                }
                m.scroll_songs.set(Some(Point::new(
                    0.0,
                    f as f64 * (SONGS_TOTAL_H - SONGS_VIEW_H),
                )));
            }
            BenchAction::SelectRow(i) => {
                m.app.borrow_mut().selection = Some(i);
                m.bump();
            }
            BenchAction::StartPlayback => {
                let ids = m.app.borrow().list_ids();
                if !ids.is_empty() {
                    m.app.borrow_mut().play_from(&ids, ids.len() / 2);
                }
                m.bump();
            }
            BenchAction::Finish => {
                println!("{}", b.report);
                std::process::exit(0);
            }
            BenchAction::Nothing => {}
        }
    }
}

// — Root + keyboard ——————————————————————————————————————————————

fn handle_key(m: &Model, e: &Event) -> EventPropagation {
    let Event::KeyDown(ke) = e else {
        return EventPropagation::Continue;
    };
    let ctrl = ke.modifiers.control() || ke.modifiers.meta();
    let in_input = m.search_focused.get_untracked();
    // Named keys.
    if let Key::Named(named) = ke.key.logical_key {
        match named {
            NamedKey::Space if !in_input && !ctrl => {
                m.app.borrow_mut().toggle_play();
                m.bump();
                return EventPropagation::Stop;
            }
            NamedKey::ArrowLeft if !in_input => {
                let p = m.app.borrow().player.position_sec();
                m.app.borrow_mut().seek(p - 10.0);
                m.pos.update(|v| *v += 1);
                return EventPropagation::Stop;
            }
            NamedKey::ArrowRight if !in_input => {
                let p = m.app.borrow().player.position_sec();
                m.app.borrow_mut().seek(p + 10.0);
                m.pos.update(|v| *v += 1);
                return EventPropagation::Stop;
            }
            NamedKey::ArrowUp if !in_input => {
                m.step_selection(-1);
                return EventPropagation::Stop;
            }
            NamedKey::ArrowDown if !in_input => {
                m.step_selection(1);
                return EventPropagation::Stop;
            }
            NamedKey::Enter if !in_input => {
                if let Some(sel) = m.app.borrow().selection {
                    let ids = m.app.borrow().list_ids();
                    if sel < ids.len() {
                        m.play_ids(&Rc::new(ids), sel);
                    }
                }
                return EventPropagation::Stop;
            }
            NamedKey::PageUp => {
                let y0 = if m.app.borrow().view == ZView::Library {
                    m.songs_off.get()
                } else {
                    m.page_off.get()
                };
                let vh = 560.0;
                let p = Point::new(0.0, (y0 - vh * 0.8).max(0.0));
                if m.app.borrow().view == ZView::Library {
                    m.scroll_songs.set(Some(p));
                } else {
                    m.scroll_page.set(Some(p));
                }
                return EventPropagation::Stop;
            }
            NamedKey::PageDown => {
                let on_songs = m.app.borrow().view == ZView::Library;
                let y0 = if on_songs {
                    m.songs_off.get()
                } else {
                    m.page_off.get()
                };
                let vh = 560.0;
                let p = Point::new(0.0, y0 + vh * 0.8);
                if on_songs {
                    m.scroll_songs.set(Some(p));
                } else {
                    m.scroll_page.set(Some(p));
                }
                return EventPropagation::Stop;
            }
            NamedKey::Escape => {
                if !m.query.get_untracked().is_empty() {
                    m.query.set(String::new());
                } else {
                    m.search_focused.set(false);
                    m.back();
                }
                return EventPropagation::Stop;
            }
            _ => {}
        }
    }
    // Character keys.
    if let Key::Character(ref c) = ke.key.logical_key {
        let ch = c.to_lowercase();
        if ctrl && (ch.as_str() == "k") {
            m.focus_search();
            return EventPropagation::Stop;
        }
        if !in_input && !ctrl {
            match ch.as_str() {
                " " => {}
                "m" => {
                    m.app.borrow_mut().toggle_mute();
                    m.bump();
                    return EventPropagation::Stop;
                }
                "q" => {
                    m.queue_open.update(|v| *v = !*v);
                    return EventPropagation::Stop;
                }
                "s" => {
                    m.app.borrow_mut().toggle_shuffle();
                    m.bump();
                    return EventPropagation::Stop;
                }
                "r" => {
                    m.app.borrow_mut().cycle_repeat();
                    m.bump();
                    return EventPropagation::Stop;
                }
                "/" => {
                    m.focus_search();
                    return EventPropagation::Stop;
                }
                _ => {}
            }
        }
    }
    EventPropagation::Continue
}

fn app_view(m: Model) -> impl IntoView {
    // Search query → live filter (the input itself lives outside the page
    // dyn, so keystrokes never destroy focus).
    {
        let m2 = m.clone();
        create_effect(move |_| {
            let q = m2.query.get();
            let is_search = m2.app.borrow().view == ZView::Search;
            if !q.is_empty() || is_search {
                m2.app.borrow_mut().set_search(&q);
                m2.results.update(|v| *v += 1);
                m2.bump();
            }
        });
    }
    // ~30 Hz frame pump: player tick, playhead, bench driver.
    {
        let (tx, rx) = crossbeam_channel::unbounded::<u64>();
        std::thread::spawn(move || {
            let mut n: u64 = 0;
            loop {
                std::thread::sleep(Duration::from_millis(33));
                n += 1;
                if tx.send(n).is_err() {
                    break;
                }
            }
        });
        let frame = create_signal_from_channel(rx);
        let m2 = m.clone();
        create_effect(move |_| {
            let _ = frame.get();
            tick_once(&m2);
        });
    }
    let m3 = m.clone();
    let m4 = m.clone();
    let root = v_stack((
        h_stack((sidebar(&m), {
            let m5 = m.clone();
            v_stack((header(&m), page(&m5))).style(|s| s.flex_grow(1.0).height_full())
        }, queue_panel(&m)))
        .style(|s| s.flex_grow(1.0).width_full()),
        player_bar(&m),
    ))
    .style(|s| {
        s.width_full()
            .height_full()
            .background(bg())
            .font_family("Inter".to_string())
    })
    .keyboard_navigable()
    .on_event(EventListener::KeyDown, move |e| handle_key(&m3, e));
    let id = root.id();
    m4.root_id.set(Some(id));
    root
}

fn main() {
    let bench_mode = std::env::args().any(|a| a == "--bench");
    let state = AppState::new();
    let m = Model {
        app: Rc::new(RefCell::new(state)),
        png: Rc::new(RefCell::new(HashMap::new())),
        rev: create_rw_signal(0u64),
        pos: create_rw_signal(0u64),
        results: create_rw_signal(0u64),
        queue_open: create_rw_signal(false),
        query: create_rw_signal(String::new()),
        search_focused: create_rw_signal(false),
        hover_card: create_rw_signal(None),
        scroll_songs: create_rw_signal(None),
        scroll_page: create_rw_signal(None),
        search_id: Rc::new(Cell::new(None)),
        root_id: Rc::new(Cell::new(None)),
        songs_off: Rc::new(Cell::new(0.0)),
        songs_vh: Rc::new(Cell::new(0.0)),
        page_off: Rc::new(Cell::new(0.0)),
        page_vh: Rc::new(Cell::new(0.0)),
        bench: Rc::new(RefCell::new(None)),
        bench_mode,
        focused_once: Rc::new(Cell::new(false)),
        tick_n: Rc::new(Cell::new(0)),
    };
    Application::new()
        .window(
            move |_| app_view(m),
            Some(
                WindowConfig::default()
                    .size((1280.0, 800.0))
                    .title("Zuno"),
            ),
        )
        .run();
}
