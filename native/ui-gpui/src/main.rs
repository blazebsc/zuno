//! Zuno — GPUI implementation of the native-GUI benchmark (gui/gpui branch).
//!
//! Renders the shared `zuno_core::AppState` per docs/gui-benchmarks/SPEC.md.
//! The 5,000-track Library ▸ Songs list uses gpui's `uniform_list`, which is
//! virtualized (only the visible range is laid out / painted) — recorded as
//! such in docs/gui-benchmarks/gpui.md.
//!
//! gpui 0.2 ships no text-input widget and no slider, so `SearchInput` is a
//! port of the framework's own examples/input.rs (entity + EntityInputHandler
//! + custom text element), and `Slider` below is a small custom element.

use gpui::{
    prelude::*, App, Application, AssetSource, Bounds, Context, CursorStyle, DispatchPhase,
    Element, ElementId, ElementInputHandler, Entity, EntityInputHandler, EventEmitter, Focusable,
    FocusHandle, Font, FontFeatures, FontStyle, FontWeight, GlobalElementId, Hsla,
    InspectorElementId, KeyBinding, LayoutId, MouseButton, MouseDownEvent, MouseMoveEvent,
    Pixels, Point, RenderImage, SharedString, ShapedLine, Style, TextRun, UTF16Selection,
    UnderlineStyle, Window, WindowBackgroundAppearance, WindowBounds, WindowOptions, actions,
    div, fill, point, px, quad, relative, rgb, size, svg, uniform_list,
    UniformListScrollHandle,
};
use image::{Frame, RgbaImage};
use std::{
    borrow::Cow,
    cell::RefCell,
    collections::HashMap,
    ops::Range,
    rc::Rc,
    sync::{Arc, LazyLock},
};
use unicode_segmentation::UnicodeSegmentation;
use zuno_core::app::{LibraryTab, RepeatMode};
use zuno_core::bench::{BenchAction, BenchDriver};
use zuno_core::format::{count_songs, counts_line, listeners, mmss};
use zuno_core::model::*;
use zuno_core::queue::Region;
use zuno_core::search;
use zuno_core::theme as t;
use zuno_core::{AppState, View};

// — Design tokens (zuno_core::theme, dark palette) ————————————————

static BG: LazyLock<Hsla> = LazyLock::new(|| rgb(0x0a0a0a).into()); // background
static FG: LazyLock<Hsla> = LazyLock::new(|| rgb(0xfafafa).into()); // foreground
static CARD: LazyLock<Hsla> = LazyLock::new(|| rgb(0x2a2a2a).into()); // hover card
static POPOVER: LazyLock<Hsla> = LazyLock::new(|| rgb(0x171717).into()); // queue rail
static MUTED: LazyLock<Hsla> = LazyLock::new(|| rgb(0x262626).into()); // slider tracks
static MUTED_FG: LazyLock<Hsla> = LazyLock::new(|| rgb(0xa1a1a1).into()); // secondary text
static PRIMARY: LazyLock<Hsla> = LazyLock::new(|| rgb(0xff0033).into()); // accent

/// `ElementId` has no `From<String>`; route format! ids through Name(SharedString).
macro_rules! eid {
    ($($arg:tt)*) => {
        gpui::ElementId::Name(format!($($arg)*).into())
    };
}

fn inter_font() -> Font {
    Font {
        family: "Inter".into(),
        features: FontFeatures(Arc::new(vec![("tnum".into(), 1)])), // tabular numerals
        weight: FontWeight::default(),
        style: FontStyle::default(),
        fallbacks: None,
    }
}

// — Actions (keyboard) ————————————————————————————————————————————

actions!(
    zuno,
    [
        TogglePlay,
        NextTrack,
        PrevTrack,
        SeekBack,
        SeekFwd,
        ToggleMute,
        ToggleQueue,
        ToggleShuffle,
        CycleRepeat,
        FocusSearch,
        EscapeKey,
    ]
);

actions!(
    search_input,
    [Backspace, Delete, Left, Right, Home, End, SearchDismiss]
);

// — SVG icon assets (lucide-style paths; gpui renders them as alpha masks) —

struct ZunoAssets;

fn stroke_icon(body: &str) -> String {
    format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="white" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">{body}</svg>"#
    )
}
fn fill_icon(body: &str) -> String {
    format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="white" stroke="none">{body}</svg>"#
    )
}

impl AssetSource for ZunoAssets {
    fn load(&self, path: &str) -> gpui::Result<Option<Cow<'static, [u8]>>> {
        let src = match path {
            "home" => stroke_icon(
                r#"<path d="m3 9 9-7 9 7v11a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/><polyline points="9 22 9 12 15 12 15 22"/>"#,
            ),
            "search" => {
                stroke_icon(r#"<circle cx="11" cy="11" r="8"/><path d="m21 21-4.3-4.3"/>"#)
            }
            "library" => stroke_icon(
                r#"<line x1="4" y1="4" x2="4" y2="20"/><line x1="9" y1="4" x2="9" y2="20"/><path d="M14 5v14l6-2V7z"/>"#,
            ),
            "settings" => stroke_icon(
                r#"<line x1="4" y1="21" x2="4" y2="14"/><line x1="4" y1="10" x2="4" y2="3"/><line x1="12" y1="21" x2="12" y2="12"/><line x1="12" y1="8" x2="12" y2="3"/><line x1="20" y1="21" x2="20" y2="16"/><line x1="20" y1="12" x2="20" y2="3"/><line x1="1" y1="14" x2="7" y2="14"/><line x1="9" y1="8" x2="15" y2="8"/><line x1="17" y1="16" x2="23" y2="16"/>"#,
            ),
            "play" => fill_icon(r#"<polygon points="6 3 20 12 6 21 6 3"/>"#),
            "pause" => fill_icon(
                r#"<rect x="14" y="4" width="4" height="16" rx="1"/><rect x="6" y="4" width="4" height="16" rx="1"/>"#,
            ),
            "prev" => fill_icon(
                r#"<polygon points="19 20 9 12 19 4 19 20"/><rect x="4" y="4" width="2" height="16" rx="1"/>"#,
            ),
            "next" => fill_icon(
                r#"<polygon points="5 4 15 12 5 20 5 4"/><rect x="18" y="4" width="2" height="16" rx="1"/>"#,
            ),
            "shuffle" => stroke_icon(
                r#"<path d="M2 18h1.4c1.3 0 2.5-.6 3.3-1.7l6.6-8.6c.8-1.1 2-1.7 3.3-1.7H22"/><path d="m18 2 4 4-4 4"/><path d="M2 6h1.9c1.5 0 2.9.9 3.6 2.2"/><path d="M22 18h-5.9c-1.3 0-2.6-.7-3.3-1.8l-.5-.8"/><path d="m18 14 4 4-4 4"/>"#,
            ),
            "repeat" => stroke_icon(
                r#"<path d="m17 2 4 4-4 4"/><path d="M3 11v-1a4 4 0 0 1 4-4h14"/><path d="m7 22-4-4 4-4"/><path d="M21 13v1a4 4 0 0 1-4 4H3"/>"#,
            ),
            "repeat-one" => stroke_icon(
                r#"<path d="m17 2 4 4-4 4"/><path d="M3 11v-1a4 4 0 0 1 4-4h14"/><path d="m7 22-4-4 4-4"/><path d="M21 13v1a4 4 0 0 1-4 4H3"/><path d="M11 10h1v4"/>"#,
            ),
            "heart" => stroke_icon(
                r#"<path d="M19 14c1.49-1.46 3-3.21 3-5.5A5.5 5.5 0 0 0 16.5 3c-1.76 0-3 .5-4.5 2-1.5-1.5-2.74-2-4.5-2A5.5 5.5 0 0 0 2 8.5c0 2.3 1.5 4.05 3 5.5l7 7Z"/>"#,
            ),
            "heart-filled" => fill_icon(
                r#"<path d="M19 14c1.49-1.46 3-3.21 3-5.5A5.5 5.5 0 0 0 16.5 3c-1.76 0-3 .5-4.5 2-1.5-1.5-2.74-2-4.5-2A5.5 5.5 0 0 0 2 8.5c0 2.3 1.5 4.05 3 5.5l7 7Z"/>"#,
            ),
            "volume" => stroke_icon(
                r#"<path d="M11 4.702a.705.705 0 0 0-1.203-.498L6.41 8.768a.53.53 0 0 1-.39.202H3a1 1 0 0 0-1 1v4.055a1 1 0 0 0 1 1h3.02a.53.53 0 0 1 .39.202l3.387 4.564A.705.705 0 0 0 11 19.298z"/><path d="M16 9a5 5 0 0 1 0 6"/><path d="M19.364 18.364a9 9 0 0 0 0-12.728"/>"#,
            ),
            "volume-mute" => stroke_icon(
                r#"<path d="M11 4.702a.705.705 0 0 0-1.203-.498L6.41 8.768a.53.53 0 0 1-.39.202H3a1 1 0 0 0-1 1v4.055a1 1 0 0 0 1 1h3.02a.53.53 0 0 1 .39.202l3.387 4.564A.705.705 0 0 0 11 19.298z"/><line x1="22" y1="9" x2="16" y2="15"/><line x1="16" y1="9" x2="22" y2="15"/>"#,
            ),
            "queue" => stroke_icon(
                r#"<path d="M21 15V6"/><path d="M18.5 18a2.5 2.5 0 1 0 0-5 2.5 2.5 0 0 0 0 5Z"/><line x1="12" y1="12" x2="3" y2="12"/><line x1="12" y1="18" x2="3" y2="18"/><line x1="12" y1="6" x2="3" y2="6"/>"#,
            ),
            "close" => stroke_icon(r#"<path d="M18 6 6 18"/><path d="m6 6 12 12"/>"#),
            "back" => stroke_icon(r#"<path d="m12 19-7-7 7-7"/><path d="M19 12H5"/>"#),
            _ => return Ok(None),
        };
        Ok(Some(src.into_bytes().into()))
    }

    fn list(&self, _path: &str) -> gpui::Result<Vec<SharedString>> {
        Ok(vec![])
    }
}

fn icon(name: &'static str, px_size: f32, color: impl Into<Hsla>) -> gpui::Svg {
    svg()
        .path(name)
        .size(px(px_size))
        .flex_shrink_0()
        .text_color(color.into())
}

// — SearchInput: port of gpui's examples/input.rs, trimmed to one line ————

pub struct SearchChanged(pub SharedString);

pub struct SearchInput {
    focus_handle: FocusHandle,
    blur_target: FocusHandle,
    content: SharedString,
    placeholder: SharedString,
    selected_range: Range<usize>,
    selection_reversed: bool,
    marked_range: Option<Range<usize>>,
    last_layout: Option<ShapedLine>,
    last_bounds: Option<Bounds<Pixels>>,
    is_selecting: bool,
}

impl EventEmitter<SearchChanged> for SearchInput {}

impl SearchInput {
    fn new(cx: &mut Context<Self>, blur_target: FocusHandle) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            blur_target,
            content: "".into(),
            placeholder: "Search songs, albums, artists…".into(),
            selected_range: 0..0,
            selection_reversed: false,
            marked_range: None,
            last_layout: None,
            last_bounds: None,
            is_selecting: false,
        }
    }

    fn set_content(&mut self, text: &str, cx: &mut Context<Self>) {
        self.content = text.to_owned().into();
        self.selected_range = self.content.len()..self.content.len();
        self.selection_reversed = false;
        cx.notify();
    }

    fn move_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        self.selected_range = offset..offset;
        cx.notify()
    }

    fn cursor_offset(&self) -> usize {
        if self.selection_reversed {
            self.selected_range.start
        } else {
            self.selected_range.end
        }
    }

    fn index_for_mouse_position(&self, position: Point<Pixels>) -> usize {
        if self.content.is_empty() {
            return 0;
        }
        let (Some(bounds), Some(line)) = (self.last_bounds.as_ref(), self.last_layout.as_ref())
        else {
            return 0;
        };
        if position.y < bounds.top() {
            return 0;
        }
        if position.y > bounds.bottom() {
            return self.content.len();
        }
        line.closest_index_for_x(position.x - bounds.left())
    }

    fn select_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        if self.selection_reversed {
            self.selected_range.start = offset
        } else {
            self.selected_range.end = offset
        };
        if self.selected_range.end < self.selected_range.start {
            self.selection_reversed = !self.selection_reversed;
            self.selected_range = self.selected_range.end..self.selected_range.start;
        }
        cx.notify()
    }

    fn offset_from_utf16(&self, offset: usize) -> usize {
        let mut utf8_offset = 0;
        let mut utf16_count = 0;
        for ch in self.content.chars() {
            if utf16_count >= offset {
                break;
            }
            utf16_count += ch.len_utf16();
            utf8_offset += ch.len_utf8();
        }
        utf8_offset
    }

    fn offset_to_utf16(&self, offset: usize) -> usize {
        let mut utf16_offset = 0;
        let mut utf8_count = 0;
        for ch in self.content.chars() {
            if utf8_count >= offset {
                break;
            }
            utf8_count += ch.len_utf8();
            utf16_offset += ch.len_utf16();
        }
        utf16_offset
    }

    fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_to_utf16(range.start)..self.offset_to_utf16(range.end)
    }

    fn range_from_utf16(&self, range_utf16: &Range<usize>) -> Range<usize> {
        self.offset_from_utf16(range_utf16.start)..self.offset_from_utf16(range_utf16.end)
    }

    fn previous_boundary(&self, offset: usize) -> usize {
        self.content
            .grapheme_indices(true)
            .rev()
            .find_map(|(idx, _)| (idx < offset).then_some(idx))
            .unwrap_or(0)
    }

    fn next_boundary(&self, offset: usize) -> usize {
        self.content
            .grapheme_indices(true)
            .find_map(|(idx, _)| (idx > offset).then_some(idx))
            .unwrap_or(self.content.len())
    }

    fn backspace(&mut self, _: &Backspace, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.select_to(self.previous_boundary(self.cursor_offset()), cx);
        }
        self.replace_text_in_range(None, "", window, cx)
    }

    fn delete(&mut self, _: &Delete, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.select_to(self.next_boundary(self.selected_range.end), cx)
        }
        self.replace_text_in_range(None, "", window, cx)
    }

    fn left(&mut self, _: &Left, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.move_to(self.previous_boundary(self.cursor_offset()), cx);
        } else {
            self.move_to(self.selected_range.start, cx)
        }
    }

    fn right(&mut self, _: &Right, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.move_to(self.next_boundary(self.selected_range.end), cx);
        } else {
            self.move_to(self.selected_range.end, cx)
        }
    }

    fn home(&mut self, _: &Home, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, cx);
    }

    fn end(&mut self, _: &End, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.content.len(), cx)
    }

    fn dismiss(&mut self, _: &SearchDismiss, window: &mut Window, cx: &mut Context<Self>) {
        if !self.content.is_empty() {
            self.content = "".into();
            self.selected_range = 0..0;
            self.selection_reversed = false;
            self.marked_range = None;
            cx.emit(SearchChanged(self.content.clone()));
        }
        window.focus(&self.blur_target);
        cx.notify();
    }

    fn on_mouse_down(&mut self, event: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.is_selecting = true;
        self.move_to(self.index_for_mouse_position(event.position), cx)
    }

    fn on_mouse_up(&mut self, _: &gpui::MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        self.is_selecting = false;
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.is_selecting {
            self.select_to(self.index_for_mouse_position(event.position), cx);
        }
    }
}

impl EntityInputHandler for SearchInput {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.range_from_utf16(&range_utf16);
        actual_range.replace(self.range_to_utf16(&range));
        Some(self.content[range].to_string())
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.range_to_utf16(&self.selected_range),
            reversed: self.selection_reversed,
        })
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        self.marked_range
            .as_ref()
            .map(|range| self.range_to_utf16(range))
    }

    fn unmark_text(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {
        self.marked_range = None;
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|range_utf16| self.range_from_utf16(range_utf16))
            .or(self.marked_range.clone())
            .unwrap_or(self.selected_range.clone());
        self.content =
            (self.content[0..range.start].to_owned() + new_text + &self.content[range.end..])
                .into();
        self.selected_range = range.start + new_text.len()..range.start + new_text.len();
        self.marked_range.take();
        cx.emit(SearchChanged(self.content.clone()));
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|range_utf16| self.range_from_utf16(range_utf16))
            .or(self.marked_range.clone())
            .unwrap_or(self.selected_range.clone());
        self.content =
            (self.content[0..range.start].to_owned() + new_text + &self.content[range.end..])
                .into();
        if !new_text.is_empty() {
            self.marked_range = Some(range.start..range.start + new_text.len());
        } else {
            self.marked_range = None;
        }
        self.selected_range = new_selected_range_utf16
            .as_ref()
            .map(|range_utf16| self.range_from_utf16(range_utf16))
            .map(|new_range| new_range.start + range.start..new_range.end + range.end)
            .unwrap_or_else(|| range.start + new_text.len()..range.start + new_text.len());
        cx.emit(SearchChanged(self.content.clone()));
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let last_layout = self.last_layout.as_ref()?;
        let range = self.range_from_utf16(&range_utf16);
        Some(Bounds::from_corners(
            point(
                bounds.left() + last_layout.x_for_index(range.start),
                bounds.top(),
            ),
            point(
                bounds.left() + last_layout.x_for_index(range.end),
                bounds.bottom(),
            ),
        ))
    }

    fn character_index_for_point(
        &mut self,
        point: gpui::Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        let line_point = self.last_bounds?.localize(&point)?;
        let last_layout = self.last_layout.as_ref()?;
        let utf8_index = last_layout.index_for_x(point.x - line_point.x)?;
        Some(self.offset_to_utf16(utf8_index))
    }
}

struct SearchTextElement {
    input: Entity<SearchInput>,
}

struct SearchPrepaintState {
    line: Option<ShapedLine>,
    cursor: Option<gpui::PaintQuad>,
    selection: Option<gpui::PaintQuad>,
}

impl Element for SearchTextElement {
    type RequestLayoutState = ();
    type PrepaintState = SearchPrepaintState;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = window.line_height().into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> SearchPrepaintState {
        let input = self.input.read(cx);
        let content = input.content.clone();
        let selected_range = input.selected_range.clone();
        let cursor = input.cursor_offset();
        let style = window.text_style();

        let (display_text, text_color) = if content.is_empty() {
            (input.placeholder.clone(), *MUTED_FG)
        } else {
            (content, style.color)
        };

        let run = TextRun {
            len: display_text.len(),
            font: style.font(),
            color: text_color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let runs = if let Some(marked_range) = input.marked_range.as_ref() {
            vec![
                TextRun {
                    len: marked_range.start,
                    ..run.clone()
                },
                TextRun {
                    len: marked_range.end - marked_range.start,
                    underline: Some(UnderlineStyle {
                        color: Some(run.color),
                        thickness: px(1.0),
                        wavy: false,
                    }),
                    ..run.clone()
                },
                TextRun {
                    len: display_text.len() - marked_range.end,
                    ..run
                },
            ]
            .into_iter()
            .filter(|run| run.len > 0)
            .collect()
        } else {
            vec![run]
        };

        let font_size = style.font_size.to_pixels(window.rem_size());
        let line = window
            .text_system()
            .shape_line(display_text, font_size, &runs, None);

        let cursor_pos = line.x_for_index(cursor);
        let (selection, cursor) = if selected_range.is_empty() {
            (
                None,
                Some(fill(
                    Bounds::new(
                        point(bounds.left() + cursor_pos, bounds.top()),
                        size(px(2.), bounds.bottom() - bounds.top()),
                    ),
                    *PRIMARY,
                )),
            )
        } else {
            (
                Some(fill(
                    Bounds::from_corners(
                        point(
                            bounds.left() + line.x_for_index(selected_range.start),
                            bounds.top(),
                        ),
                        point(
                            bounds.left() + line.x_for_index(selected_range.end),
                            bounds.bottom(),
                        ),
                    ),
                    CARD.opacity(0.8),
                )),
                None,
            )
        };
        SearchPrepaintState {
            line: Some(line),
            cursor,
            selection,
        }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut (),
        prepaint: &mut SearchPrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus_handle = self.input.read(cx).focus_handle.clone();
        window.handle_input(
            &focus_handle,
            ElementInputHandler::new(bounds, self.input.clone()),
            cx,
        );
        if let Some(selection) = prepaint.selection.take() {
            window.paint_quad(selection)
        }
        let line = prepaint.line.take().unwrap();
        line.paint(bounds.origin, window.line_height(), window, cx)
            .unwrap();

        if focus_handle.is_focused(window) {
            if let Some(cursor) = prepaint.cursor.take() {
                window.paint_quad(cursor);
            }
        }

        self.input.update(cx, |input, _cx| {
            input.last_layout = Some(line);
            input.last_bounds = Some(bounds);
        });
    }
}

impl IntoElement for SearchTextElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Render for SearchInput {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .key_context("SearchInput")
            .track_focus(&self.focus_handle(cx))
            .cursor(CursorStyle::IBeam)
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::left))
            .on_action(cx.listener(Self::right))
            .on_action(cx.listener(Self::home))
            .on_action(cx.listener(Self::end))
            .on_action(cx.listener(Self::dismiss))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .w_full()
            .px(px(14.))
            .py(px(9.))
            .text_size(px(t::BODY.size))
            .child(SearchTextElement { input: cx.entity() })
    }
}

impl gpui::Focusable for SearchInput {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

// — Slider: custom element (gpui ships no slider) ————————————————————

struct Slider {
    id: ElementId,
    frac: f32,
    width: Option<Pixels>,
    on_change: Rc<dyn Fn(f32, &mut Window, &mut App)>,
}

impl Element for Slider {
    type RequestLayoutState = ();
    type PrepaintState = Bounds<Pixels>;

    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let mut style = Style::default();
        style.size.width = match self.width {
            Some(w) => w.into(),
            None => relative(1.).into(),
        };
        style.size.height = px(18.).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut (),
        _window: &mut Window,
        _cx: &mut App,
    ) -> Bounds<Pixels> {
        bounds
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _request_layout: &mut (),
        prepaint: &mut Bounds<Pixels>,
        window: &mut Window,
        _cx: &mut App,
    ) {
        let bounds = *prepaint;
        let track_h = px(4.);
        let track_y = bounds.origin.y + (bounds.size.height - track_h) / 2.;
        window.paint_quad(fill(
            Bounds::new(
                point(bounds.origin.x, track_y),
                size(bounds.size.width, track_h),
            ),
            *MUTED,
        ));
        let frac = self.frac.clamp(0.0, 1.0);
        let fill_w = (bounds.size.width * frac).min(bounds.size.width);
        window.paint_quad(fill(
            Bounds::new(point(bounds.origin.x, track_y), size(fill_w, track_h)),
            *PRIMARY,
        ));
        let knob = px(12.);
        let knob_x = bounds.origin.x + fill_w - knob / 2.;
        let knob_y = bounds.origin.y + (bounds.size.height - knob) / 2.;
        window.paint_quad(quad(
            Bounds::new(point(knob_x, knob_y), size(knob, knob)),
            knob / 2.,
            *FG,
            px(0.),
            gpui::transparent_black(),
            gpui::BorderStyle::default(),
        ));

        let on_change = self.on_change.clone();
        window.on_mouse_event(move |ev: &MouseDownEvent, phase, window, cx| {
            if phase == DispatchPhase::Bubble
                && ev.button == MouseButton::Left
                && bounds.contains(&ev.position)
            {
                let f = ((ev.position.x - bounds.origin.x) / bounds.size.width).clamp(0.0, 1.0);
                on_change(f, window, cx);
            }
        });
        let on_change = self.on_change.clone();
        window.on_mouse_event(move |ev: &MouseMoveEvent, phase, window, cx| {
            if phase == DispatchPhase::Bubble
                && ev.pressed_button == Some(MouseButton::Left)
                && bounds.contains(&ev.position)
            {
                let f = ((ev.position.x - bounds.origin.x) / bounds.size.width).clamp(0.0, 1.0);
                on_change(f, window, cx);
            }
        });
    }
}

impl IntoElement for Slider {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

// — The app ———————————————————————————————————————————————————————

pub struct Zuno {
    pub app: AppState,
    images: RefCell<HashMap<(u64, u32), Arc<RenderImage>>>,
    queue_open: bool,
    frame: u64,
    focus_handle: FocusHandle,
    search_input: Entity<SearchInput>,
    songs_scroll: UniformListScrollHandle,
    bench: Option<BenchDriver>,
    bench_pending: bool,
    _search_sub: gpui::Subscription,
}

impl Render for Zuno {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // The frame ticker: re-registered every frame; ticks AppState + the
        // bench driver at the display's frame rate (≥10 Hz per protocol).
        cx.on_next_frame(window, |this, window, cx| this.on_frame(window, cx));

        let vw = f32::from(window.viewport_size().width);

        div()
            .id("root")
            .key_context("Zuno")
            .track_focus(&self.focus_handle)
            .font(inter_font())
            .bg(*BG)
            .text_color(*FG)
            .text_size(px(t::BODY.size))
            .size_full()
            .flex()
            .flex_col()
            .overflow_hidden()
            .on_action(cx.listener(Self::on_toggle_play))
            .on_action(cx.listener(Self::on_next))
            .on_action(cx.listener(Self::on_prev))
            .on_action(cx.listener(Self::on_seek_back))
            .on_action(cx.listener(Self::on_seek_fwd))
            .on_action(cx.listener(Self::on_mute))
            .on_action(cx.listener(Self::on_queue))
            .on_action(cx.listener(Self::on_shuffle))
            .on_action(cx.listener(Self::on_repeat))
            .on_action(cx.listener(Self::on_focus_search))
            .on_action(cx.listener(Self::on_escape))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .child(self.view_sidebar(cx, vw))
                    .child(self.view_content(cx, vw)),
            )
            .child(self.view_player_bar(cx))
    }
}

impl Zuno {
    fn new(cx: &mut Context<Self>) -> Self {
        let focus_handle = cx.focus_handle();
        let search_input = cx.new(|cx| SearchInput::new(cx, focus_handle.clone()));
        let search_sub = cx.subscribe(&search_input, |this, _input, ev: &SearchChanged, cx| {
            this.app.set_search(&ev.0);
            cx.notify();
        });
        Zuno {
            app: AppState::new(),
            images: RefCell::new(HashMap::new()),
            queue_open: false,
            frame: 0,
            focus_handle,
            search_input,
            songs_scroll: UniformListScrollHandle::new(),
            bench: None,
            bench_pending: std::env::args().any(|a| a == "--bench"),
            _search_sub: search_sub,
        }
    }

    /// Per-frame tick: AppState::tick + the bench schedule.
    fn on_frame(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.frame = self.frame.wrapping_add(1);
        self.app.tick();
        if self.bench_pending {
            self.bench_pending = false;
            self.bench = Some(BenchDriver::new()); // after first frame, per protocol
        }
        if let Some(b) = &mut self.bench {
            match b.tick() {
                BenchAction::RecordStartup => {}
                BenchAction::ScrollTo(f) => {
                    self.app.view = View::Library;
                    self.app.library_tab = LibraryTab::Songs;
                    self.scroll_songs_to(f);
                }
                BenchAction::SelectRow(i) => self.app.selection = Some(i),
                BenchAction::StartPlayback => {
                    let ids = self.app.list_ids();
                    self.app.play_from(&ids, ids.len() / 2);
                }
                BenchAction::Finish => {
                    println!("{}", b.report);
                    std::process::exit(0);
                }
                BenchAction::Nothing => {}
            }
        }
        cx.notify();
    }

    /// Hard-jump the Library▸Songs uniform_list to fraction `f` (0..1) of its
    /// scrollable range — no easing, the bench protocol's worst case.
    fn scroll_songs_to(&self, f: f32) {
        let state = self.songs_scroll.0.borrow();
        if let Some(item_size) = &state.last_item_size {
            let max = f32::from(item_size.contents.height - item_size.item.height).max(0.0);
            state.base_handle.set_offset(point(px(0.), px(-(f * max))));
        }
    }

    /// (seed,size) → GPU image. Decoded RGBA comes from the shared core
    /// `ArtworkCache` (96-cover LRU); only the BGRA frames are cached here.
    fn art(&self, seed: u64, size: u32) -> Arc<RenderImage> {
        let mut images = self.images.borrow_mut();
        images
            .entry((seed, size))
            .or_insert_with(|| {
                let rgba = self.app.artwork.get(seed, size);
                let mut bgra = (*rgba).clone();
                for px4 in bgra.chunks_exact_mut(4) {
                    px4.swap(0, 2);
                }
                let buf = RgbaImage::from_raw(size, size, bgra).expect("artwork buffer");
                Arc::new(RenderImage::new(vec![Frame::new(buf)]))
            })
            .clone()
    }

    fn art_img(&self, seed: u64, size: u32, px_size: f32, round: bool) -> gpui::Img {
        let img = gpui::img(self.art(seed, size))
            .size(px(px_size))
            .flex_shrink_0();
        if round {
            img.rounded_full()
        } else {
            img.rounded_sm()
        }
    }

    // — Action handlers (keyboard) ————————————————————————————

    fn on_toggle_play(&mut self, _: &TogglePlay, _: &mut Window, cx: &mut Context<Self>) {
        self.app.toggle_play();
        cx.notify();
    }
    fn on_next(&mut self, _: &NextTrack, _: &mut Window, cx: &mut Context<Self>) {
        self.app.next();
        cx.notify();
    }
    fn on_prev(&mut self, _: &PrevTrack, _: &mut Window, cx: &mut Context<Self>) {
        self.app.previous();
        cx.notify();
    }
    fn on_seek_back(&mut self, _: &SeekBack, _: &mut Window, cx: &mut Context<Self>) {
        let pos = self.app.player.position_sec() - 10.0;
        self.app.seek(pos.max(0.0));
        cx.notify();
    }
    fn on_seek_fwd(&mut self, _: &SeekFwd, _: &mut Window, cx: &mut Context<Self>) {
        let pos = self.app.player.position_sec() + 10.0;
        self.app.seek(pos);
        cx.notify();
    }
    fn on_mute(&mut self, _: &ToggleMute, _: &mut Window, cx: &mut Context<Self>) {
        self.app.toggle_mute();
        cx.notify();
    }
    fn on_queue(&mut self, _: &ToggleQueue, _: &mut Window, cx: &mut Context<Self>) {
        self.queue_open = !self.queue_open;
        cx.notify();
    }
    fn on_shuffle(&mut self, _: &ToggleShuffle, _: &mut Window, cx: &mut Context<Self>) {
        self.app.toggle_shuffle();
        cx.notify();
    }
    fn on_repeat(&mut self, _: &CycleRepeat, _: &mut Window, cx: &mut Context<Self>) {
        self.app.cycle_repeat();
        cx.notify();
    }
    fn on_focus_search(&mut self, _: &FocusSearch, window: &mut Window, cx: &mut Context<Self>) {
        self.app.go(View::Search);
        let handle = self.search_input.read(cx).focus_handle.clone();
        window.focus(&handle);
        cx.notify();
    }
    fn on_escape(&mut self, _: &EscapeKey, _: &mut Window, cx: &mut Context<Self>) {
        if !self.app.search_query.is_empty() {
            self.app.search_query.clear();
            self.search_input.update(cx, |input, cx| {
                input.set_content("", cx);
            });
        } else {
            self.app.go_back();
        }
        cx.notify();
    }

    // — Shell pieces ———————————————————————————————————————————

    fn grid_columns(&self, vw: f32) -> usize {
        (((vw - t::SIDEBAR_W - 2.0 * t::PAGE_PAD) / (t::CARD_W + 16.0)).floor() as usize)
            .clamp(2, 8)
    }

    /// Content column: page + the (overlay) queue panel.
    fn view_content(&self, cx: &Context<Self>, vw: f32) -> gpui::AnyElement {
        let page = div()
            .id("page")
            .relative()
            .flex()
            .flex_1()
            .min_w_0()
            .overflow_hidden()
            .child(self.view_page(cx, vw))
            .when(self.queue_open, |el| el.child(self.view_queue(cx)))
            .into_any_element();
        page
    }

    fn view_page(&self, cx: &Context<Self>, vw: f32) -> gpui::AnyElement {
        match &self.app.view {
            View::Home => self.view_home(cx, vw),
            View::Library => self.view_library(cx, vw),
            View::Album(id) => {
                let album = self.app.library.album(*id);
                let artist = self.app.library.artist(album.artist_id);
                self.view_collection(
                    cx,
                    AppState::album_seed(*id),
                    album.title.clone(),
                    format!("{} · {} · {}", album.kind.label(), artist.name, album.year),
                    album.track_ids.clone(),
                )
            }
            View::Playlist(id) => {
                let p = self.app.library.playlist(*id).expect("playlist exists");
                self.view_collection(
                    cx,
                    AppState::playlist_seed(*id),
                    p.title.clone(),
                    format!(
                        "Playlist · {}",
                        p.description.as_deref().unwrap_or("Curated by you")
                    ),
                    p.track_ids.clone(),
                )
            }
            View::Artist(id) => self.view_artist(cx, *id, vw),
            View::Search => self.view_search(cx),
            View::Settings => self.view_settings(cx),
        }
    }

    // — Sidebar ———————————————————————————————————————————————————————

    fn view_sidebar(&self, cx: &Context<Self>, vw: f32) -> gpui::AnyElement {
        let collapsed = vw < 1000.0;
        let w = if collapsed {
            t::SIDEBAR_W_COLLAPSED
        } else {
            t::SIDEBAR_W
        };

        let nav_item = |label: &'static str,
                        ic: &'static str,
                        target: View,
                        active: bool,
                        cx: &Context<Self>|
         -> gpui::AnyElement {
            div()
                .id(eid!("nav-{label}"))
                .flex()
                .h(px(36.))
                .items_center()
                .rounded_lg()
                .gap(px(12.))
                .px(px(if collapsed { 18. } else { 12. }))
                .justify_center()
                .map(|el| if collapsed { el } else { el.justify_start() })
                .when(active, |el| el.bg(*CARD))
                .when(!active, |el| el.hover(|s| s.bg(*CARD).cursor_pointer()))
                .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                    this.app.go(target.clone());
                    cx.notify();
                }))
                .child(icon(ic, 19., if active { *FG } else { *MUTED_FG }))
                .when(!collapsed, |el| {
                    el.child(
                        div()
                            .text_size(px(t::BODY.size))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(if active { *FG } else { *MUTED_FG })
                            .child(label),
                    )
                })
                .into_any_element()
        };

        let is = |target: &View| &self.app.view == target;
        let mut sidebar = div()
            .w(px(w))
            .h_full()
            .flex()
            .flex_col()
            .flex_shrink_0()
            .bg(*BG)
            .pt(px(16.))
            .px(px(12.))
            .gap(px(8.))
            .child(if collapsed {
                div().flex().justify_center().child(Self::logo(true)).into_any_element()
            } else {
                Self::logo(false).into_any_element()
            })
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(nav_item("Home", "home", View::Home, is(&View::Home), cx))
                    .child(nav_item("Search", "search", View::Search, is(&View::Search), cx))
                    .child(nav_item("Library", "library", View::Library, is(&View::Library), cx))
                    .child(nav_item(
                        "Settings",
                        "settings",
                        View::Settings,
                        is(&View::Settings),
                        cx,
                    )),
            );

        if !collapsed {
            sidebar = sidebar
                .child(
                    div()
                        .text_size(px(11.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(*MUTED_FG)
                        .pl(px(12.))
                        .pt(px(10.))
                        .child("PLAYLISTS"),
                )
                .child(self.view_sidebar_playlists(cx));
        }

        sidebar.into_any_element()
    }

    fn logo(compact: bool) -> gpui::Div {
        let mark = div()
            .size(px(28.))
            .rounded_full()
            .bg(*PRIMARY)
            .flex()
            .items_center()
            .justify_center()
            .text_size(px(14.))
            .font_weight(FontWeight::BOLD)
            .text_color(rgb(0xffffff))
            .child("Z");
        if compact {
            mark
        } else {
            div()
                .flex()
                .items_center()
                .gap(px(10.))
                .px(px(4.))
                .child(mark)
                .child(
                    div()
                        .text_size(px(t::H3.size))
                        .font_weight(FontWeight::BOLD)
                        .text_color(*FG)
                        .child("Zuno"),
                )
        }
    }

    fn view_sidebar_playlists(&self, cx: &Context<Self>) -> gpui::AnyElement {
        let rows = self
            .app
            .library
            .playlists
            .iter()
            .map(|p| self.playlist_row(cx, p.id))
            .collect::<Vec<_>>();
        div()
            .id("sidebar-playlists")
            .overflow_y_scroll()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .gap(px(1.))
            .children(rows)
            .into_any_element()
    }

    fn playlist_row(&self, cx: &Context<Self>, id: PlaylistId) -> gpui::AnyElement {
        let Some(p) = self.app.library.playlist(id) else {
            return div().into_any_element();
        };
        let active = matches!(self.app.view, View::Playlist(v) if v == id);
        let count = p.track_ids.len();
        let title: SharedString = p.title.clone().into();
        let pid = id;
        div()
            .id(eid!("pl-{id}"))
            .flex()
            .items_center()
            .gap(px(10.))
            .h(px(48.))
            .rounded_lg()
            .px(px(8.))
            .when(active, |el| el.bg(*CARD))
            .when(!active, |el| el.hover(|s| s.bg(*CARD).cursor_pointer()))
            .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                this.app.open_playlist(pid);
                cx.notify();
            }))
            .child(self.art_img(AppState::playlist_seed(id), 40, t::ROW_ART, false))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(1.))
                    .min_w_0()
                    .child(
                        div()
                            .text_size(px(t::SMALL.size + 1.))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(*FG)
                            .line_clamp(1)
                            .child(title),
                    )
                    .child(
                        div()
                            .text_size(px(t::SMALL.size))
                            .text_color(*MUTED_FG)
                            .child(format!("{count} songs")),
                    ),
            )
            .into_any_element()
    }

    // — Home —————————————————————————————————————————————————————————

    fn view_home(&self, cx: &Context<Self>, vw: f32) -> gpui::AnyElement {
        let recent: Vec<AlbumId> = self.app.library.home_recent_albums(12);

        let mut album_ids: Vec<AlbumId> = (0..self.app.library.albums.len() as u32).collect();
        album_ids.sort_by_key(|&a| std::cmp::Reverse(self.app.library.album(a).year));
        album_ids.truncate(24);

        let mut artist_ids: Vec<ArtistId> = (0..self.app.library.artists.len() as u32).collect();
        artist_ids.sort_by_key(|&r| std::cmp::Reverse(self.app.library.artist(r).monthly_listeners));
        artist_ids.truncate(12);

        let cols = self.grid_columns(vw);

        let body = div()
            .flex()
            .flex_col()
            .gap(px(t::SECTION_GAP))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .pt(px(24.))
                    .child(Self::h1("Home"))
                    .child(
                        div()
                            .text_size(px(t::SMALL.size))
                            .text_color(*MUTED_FG)
                            .child("Pick up where you left off"),
                    ),
            )
            .child(self.shelf(cx, "Recently played", recent.iter().map(|&a| self.album_card(cx, a))))
            .child(self.shelf(
                cx,
                "Made for you",
                self.app.library.mixes.iter().map(|m| {
                    let track_ids = m.track_ids.clone();
                    let mid = m.id;
                    self.generic_card(
                        cx,
                        AppState::playlist_seed(mid),
                        m.title.clone().into(),
                        m.description.clone().unwrap_or_default().into(),
                        Box::new(move |z: &mut Zuno| z.app.open_playlist(mid)) as Box<dyn Fn(&mut Zuno)>,
                        Box::new(move |z: &mut Zuno| {
                            let ids = track_ids.clone();
                            z.app.play_from(&ids, 0);
                        }),
                    )
                }),
            ))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(12.))
                    .child(Self::h2("New albums"))
                    .child(
                        div()
                            .id("home-grid")
                            .overflow_y_scroll()
                            .flex()
                            .flex_wrap()
                            .gap(px(16.))
                            .min_h_0()
                            .children(album_ids.iter().map(|&a| self.album_card(cx, a))),
                    ),
            )
            .child(self.shelf(
                cx,
                "Popular artists",
                artist_ids.iter().map(|&rid| self.artist_card(cx, rid)),
            ));

        let _ = cols;
        self.page_scroll(body)
    }

    /// A horizontally scrollable shelf of cards with its section title.
    fn shelf(
        &self,
        _cx: &Context<Self>,
        title: &'static str,
        cards: impl Iterator<Item = gpui::AnyElement>,
    ) -> gpui::AnyElement {
        div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(Self::h2(title))
            .child(
                div()
                    .id(eid!("shelf-{title}"))
                    .overflow_x_scroll()
                    .flex()
                    .gap(px(16.))
                    .children(cards),
            )
            .into_any_element()
    }

    // — Cards —————————————————————————————————————————————————————————

    fn album_card(&self, cx: &Context<Self>, aid: AlbumId) -> gpui::AnyElement {
        let album = self.app.library.album(aid);
        let artist = self.app.library.artist(album.artist_id);
        let ids = album.track_ids.clone();
        self.generic_card(
            cx,
            AppState::album_seed(aid),
            album.title.clone().into(),
            format!("{} · {}", artist.name, album.year).into(),
            Box::new(move |z: &mut Zuno| z.app.open_album(aid)),
            Box::new(move |z: &mut Zuno| {
                let ids = ids.clone();
                z.app.play_from(&ids, 0);
            }),
        )
    }

    fn artist_card(&self, cx: &Context<Self>, rid: ArtistId) -> gpui::AnyElement {
        self.artist_shelf_card(cx, rid)
    }

    /// The album/mix card from the spec: square art, title + subtitle, and a
    /// hover scrim with a round primary play pill (group-hover driven).
    #[allow(clippy::too_many_arguments)]
    fn generic_card(
        &self,
        cx: &Context<Self>,
        seed: u64,
        title: SharedString,
        subtitle: SharedString,
        on_open: Box<dyn Fn(&mut Zuno)>,
        on_play: Box<dyn Fn(&mut Zuno)>,
    ) -> gpui::AnyElement {
        let group = format!("card-{seed}");
        let art = self.art(seed, t::CARD_W as u32);
        div()
            .id(ElementId::Name(group.clone().into()))
            .group(group.clone())
            .w(px(t::CARD_W + 8.))
            .flex()
            .flex_col()
            .gap(px(8.))
            .flex_shrink_0()
            .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                on_open(this);
                cx.notify();
            }))
            .hover(|s| s.cursor_pointer())
            .child(
                div()
                    .relative()
                    .w(px(t::CARD_W))
                    .h(px(t::CARD_W))
                    .child(gpui::img(art).size_full().rounded_sm())
                    .child(
                        div()
                            .id(eid!("scrim-{seed}"))
                            .absolute()
                            .inset_0()
                            .rounded_sm()
                            .group_hover(group.clone(), |s| s.bg(BG.opacity(0.5)))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(
                                div()
                                    .id(eid!("pill-{seed}"))
                                    .size(px(48.))
                                    .rounded_full()
                                    .bg(*PRIMARY)
                                    .opacity(0.)
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .group_hover(group.clone(), |s| s.opacity(1.))
                                    .on_click(cx.listener(move |this, _ev: &gpui::ClickEvent, window, cx| {
                                        cx.stop_propagation();
                                        on_play(this);
                                        cx.notify();
                                        window.prevent_default();
                                    }))
                                    .child(icon("play", 20., rgb(0xffffff))),
                            ),
                    ),
            )
            .child(
                div()
                    .w(px(t::CARD_W))
                    .text_size(px(t::BODY.size))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(*FG)
                    .line_clamp(2)
                    .child(title),
            )
            .child(
                div()
                    .w(px(t::CARD_W))
                    .text_size(px(t::SMALL.size))
                    .text_color(*MUTED_FG)
                    .line_clamp(1)
                    .child(subtitle),
            )
            .into_any_element()
    }

    /// Popular-artists shelf card: round artwork + name + listeners.
    fn artist_shelf_card(&self, cx: &Context<Self>, rid: ArtistId) -> gpui::AnyElement {
        let artist = self.app.library.artist(rid);
        div()
            .id(eid!("artist-{rid}"))
            .group(format!("artist-{rid}"))
            .w(px(t::CARD_W + 8.))
            .flex()
            .flex_col()
            .gap(px(8.))
            .flex_shrink_0()
            .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                this.app.open_artist(rid);
                cx.notify();
            }))
            .hover(|s| s.cursor_pointer())
            .child(div().size(px(176.)).child(self.art_img(AppState::artist_seed(rid), 176, 176., true)))
            .child(
                div()
                    .text_size(px(t::BODY.size))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(*FG)
                    .line_clamp(1)
                    .child(artist.name.clone()),
            )
            .child(
                div()
                    .text_size(px(t::SMALL.size))
                    .text_color(*MUTED_FG)
                    .child(listeners(artist.monthly_listeners)),
            )
            .into_any_element()
    }

    // — Library ——————————————————————————————————————————————————————

    fn view_library(&self, cx: &Context<Self>, _vw: f32) -> gpui::AnyElement {
        let tab = self.app.library_tab;
        let tab_pill = |label: &'static str, target: LibraryTab, cx: &Context<Self>| -> gpui::AnyElement {
            let active = tab == target;
            div()
                .id(eid!("tab-{label}"))
                .rounded_full()
                .px(px(14.))
                .py(px(6.))
                .text_size(px(t::SMALL_MED.size))
                .font_weight(FontWeight::MEDIUM)
                .text_color(if active { *FG } else { *MUTED_FG })
                .when(active, |el| el.bg(*CARD))
                .when(!active, |el| {
                    el.hover(|s| s.bg(*MUTED).cursor_pointer().text_color(*FG))
                })
                .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                    this.app.set_library_tab(target);
                    cx.notify();
                }))
                .child(label)
                .into_any_element()
        };

        let body = match tab {
            LibraryTab::Songs => {
                let ids = Rc::new((0..self.app.library.tracks.len() as u32).collect::<Vec<_>>());
                self.track_list(cx, "songs", ids.clone(), Some(self.songs_scroll.clone()))
            }
            LibraryTab::Albums => div()
                .id("lib-albums")
                .overflow_y_scroll()
                .flex_1()
                .min_h_0()
                .flex()
                .flex_wrap()
                .gap(px(16.))
                .pt(px(8.))
                .children((0..self.app.library.albums.len() as u32).map(|a| self.album_card(cx, a)))
                .into_any_element(),
            LibraryTab::Artists => div()
                .id("lib-artists")
                .overflow_y_scroll()
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .gap(px(2.))
                .pt(px(8.))
                .children((0..self.app.library.artists.len() as u32).map(|rid| {
                    let artist = self.app.library.artist(rid);
                    div()
                        .id(eid!("lartist-{rid}"))
                        .flex()
                        .items_center()
                        .gap(px(12.))
                        .rounded_lg()
                        .p(px(6.))
                        .hover(|s| s.bg(*CARD).cursor_pointer())
                        .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                            this.app.open_artist(rid);
                            cx.notify();
                        }))
                        .child(self.art_img(AppState::artist_seed(rid), 40, t::ROW_ART, true))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(2.))
                                .child(
                                    div()
                                        .text_size(px(t::BODY.size))
                                        .font_weight(FontWeight::MEDIUM)
                                        .child(artist.name.clone()),
                                )
                                .child(
                                    div()
                                        .text_size(px(t::SMALL.size))
                                        .text_color(*MUTED_FG)
                                        .child(listeners(artist.monthly_listeners)),
                                ),
                        )
                        .into_any_element()
                }))
                .into_any_element(),
            LibraryTab::Playlists => div()
                .id("lib-playlists")
                .overflow_y_scroll()
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .gap(px(2.))
                .pt(px(8.))
                .children(self.app.library.playlists.iter().map(|p| {
                    let pid = p.id;
                    let count = p.track_ids.len();
                    div()
                        .id(eid!("lpl-{pid}"))
                        .flex()
                        .items_center()
                        .gap(px(12.))
                        .rounded_lg()
                        .p(px(6.))
                        .hover(|s| s.bg(*CARD).cursor_pointer())
                        .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                            this.app.open_playlist(pid);
                            cx.notify();
                        }))
                        .child(self.art_img(AppState::playlist_seed(pid), 40, t::ROW_ART, false))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(2.))
                                .child(
                                    div()
                                        .text_size(px(t::BODY.size))
                                        .font_weight(FontWeight::MEDIUM)
                                        .child(p.title.clone()),
                                )
                                .child(
                                    div()
                                        .text_size(px(t::SMALL.size))
                                        .text_color(*MUTED_FG)
                                        .child(format!("{} · playlist", count_songs(count))),
                                ),
                        )
                        .into_any_element()
                }))
                .into_any_element(),
        };

        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .h_full()
            .px(px(t::PAGE_PAD))
            .pb(px(8.))
            .gap(px(16.))
            .child(div().pt(px(18.)).child(self.page_header(cx, "Your Library")))
            .child(
                div()
                    .flex()
                    .gap(px(8.))
                    .child(tab_pill("Playlists", LibraryTab::Playlists, cx))
                    .child(tab_pill("Albums", LibraryTab::Albums, cx))
                    .child(tab_pill("Artists", LibraryTab::Artists, cx))
                    .child(tab_pill("Songs", LibraryTab::Songs, cx)),
            )
            .child(body)
            .into_any_element()
    }

    /// The virtualized 5,000-row list (and any collection list >60 rows):
    /// gpui's `uniform_list` lays out and paints only the visible range.
    fn track_list(
        &self,
        cx: &Context<Self>,
        id: &'static str,
        ids: Rc<Vec<TrackId>>,
        scroll: Option<UniformListScrollHandle>,
    ) -> gpui::AnyElement {
        let count = ids.len();
        let render_items = cx.processor(move |this, range: Range<usize>, _window, cx| {
            range
                .filter(|&ix| ix < ids.len())
                .map(|ix| this.track_row(cx, ix, ids[ix], ids.clone()))
                .collect::<Vec<_>>()
        });
        let mut list = uniform_list(id, count, render_items)
            .w_full()
            .flex_1()
            .min_h_0();
        if let Some(handle) = scroll {
            list = list.track_scroll(handle);
        }
        list.into_any_element()
    }

    /// Plain (non-virtualized) rows for short lists.
    fn track_rows(&self, cx: &Context<Self>, ids: &[TrackId]) -> Vec<gpui::AnyElement> {
        let rc = Rc::new(ids.to_vec());
        rc.iter()
            .enumerate()
            .map(|(ix, &id)| self.track_row(cx, ix, id, rc.clone()))
            .collect()
    }

    // — Album / Playlist pages ———————————————————————————————————————

    fn view_collection(
        &self,
        cx: &Context<Self>,
        seed: u64,
        title: String,
        subtitle: String,
        ids: Vec<TrackId>,
    ) -> gpui::AnyElement {
        let counts = counts_line(ids.iter().map(|&tid| self.app.library.track(tid).duration_sec));
        let play_ids = ids.clone();
        let shuffle_ids = ids.clone();

        let hero = div()
            .flex()
            .gap(px(24.))
            .pt(px(20.))
            .child(self.art_img(seed, 232, 232., false))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(10.))
                    .w_full()
                    .h(px(232.))
                    .justify_end()
                    .pb(px(4.))
                    .child(
                        div()
                            .text_size(px(t::SMALL.size))
                            .text_color(*MUTED_FG)
                            .child(subtitle),
                    )
                    .child(Self::h1(&title))
                    .child(
                        div()
                            .text_size(px(t::SMALL.size))
                            .text_color(*MUTED_FG)
                            .child(counts),
                    )
                    .child(
                        div()
                            .flex()
                            .gap(px(10.))
                            .child(
                                div()
                                    .id("coll-play")
                                    .rounded_full()
                                    .bg(*PRIMARY)
                                    .px(px(20.))
                                    .py(px(10.))
                                    .flex()
                                    .items_center()
                                    .gap(px(8.))
                                    .hover(|s| s.opacity(0.9).cursor_pointer())
                                    .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                                        let ids = play_ids.clone();
                                        this.app.play_from(&ids, 0);
                                        cx.notify();
                                    }))
                                    .child(icon("play", 16., rgb(0xffffff)))
                                    .child(
                                        div()
                                            .text_size(px(t::BODY.size))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(rgb(0xffffff))
                                            .child("Play"),
                                    ),
                            )
                            .child(
                                div()
                                    .id("coll-shuffle")
                                    .rounded_full()
                                    .border_1()
                                    .border_color(FG.opacity(0.2))
                                    .px(px(20.))
                                    .py(px(10.))
                                    .flex()
                                    .items_center()
                                    .gap(px(8.))
                                    .hover(|s| s.bg(*CARD).cursor_pointer())
                                    .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                                        let ids = shuffle_ids.clone();
                                        this.app.play_from(&ids, 0);
                                        if !this.app.shuffle {
                                            this.app.toggle_shuffle();
                                        }
                                        cx.notify();
                                    }))
                                    .child(icon("shuffle", 16., *FG))
                                    .child(
                                        div()
                                            .text_size(px(t::BODY.size))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(*FG)
                                            .child("Shuffle"),
                                    ),
                            ),
                    ),
            );

        let body = if ids.len() > 60 {
            div()
                .flex_1()
                .min_h_0()
                .child(self.track_list(cx, "collection-songs", Rc::new(ids.clone()), None))
        } else {
            div().flex().flex_col().children(self.track_rows(cx, &ids))
        };

        self.page_scroll(div().flex().flex_col().gap(px(16.)).child(hero).child(body))
    }

    fn artist_top_tracks(&self, rid: ArtistId) -> Vec<TrackId> {
        self.app
            .library
            .albums
            .iter()
            .find(|al| al.artist_id == rid)
            .map(|al| al.track_ids.iter().rev().take(10).copied().collect())
            .unwrap_or_default()
    }

    // — Artist page ————————————————————————————————————————————————————

    fn view_artist(&self, cx: &Context<Self>, rid: ArtistId, vw: f32) -> gpui::AnyElement {
        let artist = self.app.library.artist(rid);
        let top = self.artist_top_tracks(rid);
        let counts = counts_line(top.iter().map(|&tid| self.app.library.track(tid).duration_sec));
        let play_ids = top.clone();

        let hero = div()
            .flex()
            .gap(px(24.))
            .pt(px(20.))
            .child(self.art_img(AppState::artist_seed(rid), 232, 232., true))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(10.))
                    .w_full()
                    .h(px(232.))
                    .justify_end()
                    .pb(px(4.))
                    .child(
                        div()
                            .text_size(px(t::SMALL.size))
                            .text_color(*MUTED_FG)
                            .child("Artist"),
                    )
                    .child(Self::h1(&artist.name))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(2.))
                            .child(
                                div()
                                    .text_size(px(t::SMALL.size))
                                    .text_color(*MUTED_FG)
                                    .child(listeners(artist.monthly_listeners)),
                            )
                            .child(
                                div()
                                    .text_size(px(t::SMALL.size))
                                    .text_color(*MUTED_FG)
                                    .child(counts),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .gap(px(10.))
                            .child(
                                div()
                                    .id("artist-play")
                                    .rounded_full()
                                    .bg(*PRIMARY)
                                    .px(px(20.))
                                    .py(px(10.))
                                    .flex()
                                    .items_center()
                                    .gap(px(8.))
                                    .hover(|s| s.opacity(0.9).cursor_pointer())
                                    .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                                        let ids = play_ids.clone();
                                        this.app.play_from(&ids, 0);
                                        cx.notify();
                                    }))
                                    .child(icon("play", 16., rgb(0xffffff)))
                                    .child(
                                        div()
                                            .text_size(px(t::BODY.size))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(rgb(0xffffff))
                                            .child("Play"),
                                    ),
                            ),
                    ),
            );

        let mut col = div()
            .flex()
            .flex_col()
            .gap(px(16.))
            .child(hero)
            .when(!top.is_empty(), |el| {
                el.child(Self::h2("Top tracks"))
                    .child(div().flex().flex_col().children(self.track_rows(cx, &top)))
                    .child(div().h(px(8.)))
            })
            .child(Self::h2("Albums"))
            .child(
                div()
                    .id("artist-albums")
                    .overflow_y_scroll()
                    .flex()
                    .flex_wrap()
                    .gap(px(16.))
                    .children(artist.album_ids.iter().map(|&aid| self.album_card(cx, aid))),
            );
        let _ = &mut col;
        let _ = vw;

        self.page_scroll(col)
    }

    // — Search —————————————————————————————————————————————————————————

    fn view_search(&self, cx: &Context<Self>) -> gpui::AnyElement {
        let field = div()
            .id("search-field")
            .w(px(420.))
            .rounded_full()
            .bg(*MUTED)
            .overflow_hidden()
            .hover(|s| s.bg(*CARD))
            .child(self.search_input.clone());

        let q = self.app.search_query.trim().to_string();
        let mut body = div().flex().flex_col().gap(px(16.));

        if q.is_empty() {
            body = body.child(
                div()
                    .pt(px(40.))
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(8.)
                    )
                    .child(
                        div()
                            .text_size(px(t::H3.size))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(*MUTED_FG)
                            .child("Search Zuno"),
                    )
                    .child(
                        div()
                            .text_size(px(t::SMALL.size))
                            .text_color(*MUTED_FG)
                            .child("Find songs, albums and artists · Ctrl-K"),
                    ),
            );
        } else {
            let results = search::search(&self.app.library, &q);
            if results.tracks.is_empty() {
                body = body.child(
                    div()
                        .pt(px(24.))
                        .w_full()
                        .flex()
                        .justify_center()
                        .child(
                            div()
                                .text_size(px(t::BODY.size))
                                .text_color(*MUTED_FG)
                                .child(format!("No results for “{q}”")),
                        ),
                );
            } else {
                let top_id = results.tracks[0];
                let tv = self.app.library.track_view(top_id);
                body = body
                    .child(self.shelf(
                        cx,
                        "Top result",
                        std::iter::once(self.generic_card(
                            cx,
                            AppState::album_seed(self.app.library.track(top_id).album_id),
                            tv.title.to_string().into(),
                            format!("Song · {}", tv.artist).into(),
                            Box::new(move |z: &mut Zuno| z.app.play_track(top_id)),
                            Box::new(move |z: &mut Zuno| z.app.play_track(top_id)),
                        )),
                    ))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(12.))
                            .child(Self::h2("Songs"))
                            .child(div().flex().flex_col().children(self.track_rows(cx, &results.tracks))),
                    );
                if !results.albums.is_empty() {
                    body = body.child(self.shelf(
                        cx,
                        "Albums",
                        results.albums.iter().map(|&aid| self.album_card(cx, aid)),
                    ));
                }
                if !results.artists.is_empty() {
                    body = body.child(self.shelf(
                        cx,
                        "Artists",
                        results.artists.iter().map(|&rid| self.artist_card(cx, rid)),
                    ));
                }
            }
        }

        self.page_scroll(
            div().flex().flex_col().gap(px(16.)).pt(px(18.)).child(field).child(body),
        )
    }

    // — Settings ———————————————————————————————————————————————————————

    fn view_settings(&self, cx: &Context<Self>) -> gpui::AnyElement {
        let setting_row = |label: &'static str, value: &'static str| -> gpui::AnyElement {
            div()
                .id(eid!("setting-{label}"))
                .flex()
                .items_center()
                .justify_between()
                .rounded_lg()
                .px(px(12.))
                .py(px(10.))
                .hover(|s| s.bg(*CARD))
                .child(
                    div()
                        .text_size(px(t::BODY.size))
                        .text_color(*FG)
                        .child(label),
                )
                .child(
                    div()
                        .text_size(px(t::SMALL.size))
                        .text_color(*MUTED_FG)
                        .child(value),
                )
                .into_any_element()
        };

        let _ = cx;
        let col = div()
            .flex()
            .flex_col()
            .gap(px(24.))
            .child(div().pt(px(18.)).child(self.page_header(cx, "Settings")))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(Self::h3("Appearance"))
                    .child(setting_row("Theme", "Dark (pinned)"))
                    .child(setting_row("Accent", "#FF0033")),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(Self::h3("Playback"))
                    .child(setting_row("Streaming quality", "High (256 kbps)"))
                    .child(setting_row("Download quality", "High (256 kbps)"))
                    .child(setting_row("Gapless playback", "On"))
                    .child(setting_row("Crossfade", "Off"))
                    .child(setting_row("Audio engine", "Rust (cpal) — no WebView")),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(Self::h3("Library"))
                    .child(setting_row("Cache", "4 GB"))
                    .child(setting_row("Downloads ceiling", "8 GB")),
            );

        self.page_scroll(col)
    }

    // — Queue panel ————————————————————————————————————————————————————

    fn view_queue(&self, cx: &Context<Self>) -> gpui::AnyElement {
        let rows = self.app.queue_rows();
        let mut body = div().flex().flex_col().gap(px(2.));
        let mut last_region: Option<Region> = None;
        for (i, (id, region)) in rows.iter().enumerate() {
            if region != &last_region.unwrap_or(Region::Played) {
                if let Some(label) = match region {
                    Region::Played => None,
                    Region::Current => Some("NOW PLAYING"),
                    Region::Manual => Some("NEXT IN QUEUE"),
                    Region::Automatic => Some("NEXT UP"),
                } {
                    body = body.child(
                        div()
                            .pt(px(10.))
                            .text_size(px(11.))
                            .text_color(*MUTED_FG)
                            .child(label),
                    );
                }
                last_region = Some(*region);
            }
            let tv = self.app.library.track_view(*id);
            let is_current = *region == Region::Current;
            let mut row = div()
                .id(eid!("qrow-{i}"))
                .flex()
                .items_center()
                .gap(px(10.))
                .rounded_lg()
                .px(px(6.))
                .h(px(56.))
                .when(is_current, |el| el.bg(PRIMARY.opacity(0.08)))
                .when(!is_current, |el| {
                    el.hover(|s| s.bg(*CARD).cursor_pointer())
                })
                .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                    if let Some(id) = this.app.queue.jump_to(i) {
                        this.app.play_track(id);
                        cx.notify();
                    }
                }))
                .child(self.art_img(AppState::album_seed(self.app.library.track(*id).album_id), 32, 32., false))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(1.))
                        .min_w_0()
                        .flex_1()
                        .child(
                            div()
                                .text_size(px(t::SMALL.size + 1.))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(*FG)
                                .line_clamp(1)
                                .child(tv.title.to_string()),
                        )
                        .child(
                            div()
                                .text_size(px(t::SMALL.size))
                                .text_color(*MUTED_FG)
                                .line_clamp(1)
                                .child(tv.artist.to_string()),
                        ),
                )
                .child(
                    div()
                        .text_size(px(t::SMALL.size))
                        .text_color(*MUTED_FG)
                        .child(mmss(tv.duration_sec)),
                );
            if !is_current {
                row = row.child(
                    div()
                        .id(eid!("qdel-{i}"))
                        .p(px(4.))
                        .rounded_full()
                        .flex()
                        .items_center()
                        .hover(|s| s.cursor_pointer())
                        .on_click(cx.listener(move |this, _ev: &gpui::ClickEvent, window, cx| {
                            cx.stop_propagation();
                            this.app.queue.remove_at(i);
                            cx.notify();
                            window.prevent_default();
                        }))
                        .child(icon("close", 13., *MUTED_FG)),
                );
            }
            body = body.child(row);
        }

        div()
            .id("queue-panel")
            .absolute()
            .top_0()
            .right_0()
            .bottom_0()
            .w(px(t::QUEUE_W))
            .bg(*POPOVER)
            .border_l_1()
            .border_color(MUTED_FG.opacity(0.15))
            .shadow_lg()
            .flex()
            .flex_col()
            .pt(px(16.))
            .px(px(12.))
            .gap(px(8.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(t::H3.size))
                            .font_weight(FontWeight::BOLD)
                            .child("Queue"),
                    )
                    .child(
                        div()
                            .id("queue-close")
                            .p(px(6.))
                            .rounded_full()
                            .hover(|s| s.bg(*CARD).cursor_pointer())
                            .on_click(cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
                                this.queue_open = false;
                                cx.notify();
                            }))
                            .child(icon("close", 16., *MUTED_FG)),
                    ),
            )
            .child(
                div()
                    .id("queue-scroll")
                    .overflow_y_scroll()
                    .flex_1()
                    .min_h_0()
                    .child(body),
            )
            .into_any_element()
    }

    // — Player bar —————————————————————————————————————————————————————

    fn view_player_bar(&self, cx: &Context<Self>) -> gpui::AnyElement {
        let current = self.app.current_track();
        let (title, artist_name, seed, album_id, artist_id, track_id, liked) = match current {
            Some(track) => (
                track.title.clone(),
                self.app.library.artist(track.artist_id).name.clone(),
                AppState::album_seed(track.album_id),
                track.album_id,
                track.artist_id,
                track.id,
                track.liked,
            ),
            None => (
                "Nothing playing".to_string(),
                "—".to_string(),
                AppState::album_seed(0),
                0,
                0,
                0,
                false,
            ),
        };
        let has_track = current.is_some();
        let position = self.app.player.position_sec();
        let duration = self.app.player.duration_sec().max(1.0);
        let playing = self.app.playing;

        let entity = cx.entity();
        let seek_slider = Slider {
            id: "seek".into(),
            frac: ((position / duration) as f32).clamp(0.0, 1.0),
            width: None,
            on_change: Rc::new(move |f, _window, cx: &mut App| {
                entity.update(cx, |this, cx| {
                    this.app.seek(f as f64 * this.app.player.duration_sec().max(1.0));
                    cx.notify();
                });
            }),
        };

        let entity = cx.entity();
        let volume_slider = Slider {
            id: "volume".into(),
            frac: if self.app.muted { 0.0 } else { self.app.volume },
            width: Some(px(96.)),
            on_change: Rc::new(move |f, _window, cx: &mut App| {
                entity.update(cx, |this, cx| {
                    this.app.set_volume(f);
                    cx.notify();
                });
            }),
        };

        let info = div()
            .flex()
            .items_center()
            .gap(px(12.))
            .min_w_0()
            .flex_1()
            .child(
                div()
                    .id("pb-art")
                    .rounded_sm()
                    .overflow_hidden()
                    .hover(|s| s.cursor_pointer())
                    .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                        if has_track {
                            this.app.open_album(album_id);
                            cx.notify();
                        }
                    }))
                    .child(self.art_img(seed, 40, t::ROW_ART, false)),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(1.))
                    .min_w_0()
                    .child(
                        div()
                            .id("pb-title")
                            .line_clamp(1)
                            .text_size(px(t::BODY.size))
                            .font_weight(FontWeight::MEDIUM)
                            .hover(|s| s.cursor_pointer())
                            .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                                if has_track {
                                    this.app.open_album(album_id);
                                    cx.notify();
                                }
                            }))
                            .child(title),
                    )
                    .child(
                        div()
                            .id("pb-artist")
                            .line_clamp(1)
                            .text_size(px(t::SMALL.size))
                            .text_color(*MUTED_FG)
                            .hover(|s| s.cursor_pointer())
                            .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                                if has_track {
                                    this.app.open_artist(artist_id);
                                    cx.notify();
                                }
                            }))
                            .child(artist_name),
                    ),
            );

        let icon_btn = |ic: &'static str,
                        size: f32,
                        color: Hsla,
                        handler: Box<dyn Fn(&mut Zuno, &mut Context<Zuno>)>|
         -> gpui::Stateful<gpui::Div> {
            div()
                .id(eid!("pb-{ic}"))
                .p(px(6.))
                .rounded_full()
                .flex()
                .items_center()
                .hover(|s| s.bg(*CARD).cursor_pointer())
                .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                    handler(this, cx);
                }))
                .child(icon(ic, size, color))
        };

        let transport = div()
            .flex()
            .items_center()
            .gap(px(6.))
            .child(
                div()
                    .id("pb-shuffle")
                    .p(px(6.))
                    .rounded_full()
                    .flex()
                    .when(self.app.shuffle, |el| el.bg(*CARD))
                    .hover(|s| s.bg(*CARD).cursor_pointer())
                    .on_click(cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
                        this.app.toggle_shuffle();
                        cx.notify();
                    }))
                    .child(icon(
                        "shuffle",
                        18.,
                        if self.app.shuffle { *PRIMARY } else { *MUTED_FG },
                    )),
            )
            .child(icon_btn(
                "prev",
                20.,
                *FG,
                Box::new(|z, cx| {
                    z.app.previous();
                    cx.notify();
                }),
            ))
            .child(
                div()
                    .id("pb-play")
                    .size(px(40.))
                    .rounded_full()
                    .bg(*PRIMARY)
                    .flex()
                    .items_center()
                    .justify_center()
                    .hover(|s| s.opacity(0.9).cursor_pointer())
                    .on_click(cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
                        this.app.toggle_play();
                        cx.notify();
                    }))
                    .child(icon(
                        if playing { "pause" } else { "play" },
                        20.,
                        rgb(0xffffff),
                    )),
            )
            .child(icon_btn(
                "next",
                20.,
                *FG,
                Box::new(|z, cx| {
                    z.app.next();
                    cx.notify();
                }),
            ))
            .child(
                div()
                    .id("pb-repeat")
                    .p(px(6.))
                    .rounded_full()
                    .flex()
                    .when(self.app.repeat != RepeatMode::Off, |el| el.bg(*CARD))
                    .hover(|s| s.bg(*CARD).cursor_pointer())
                    .on_click(cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
                        this.app.cycle_repeat();
                        cx.notify();
                    }))
                    .child(icon(
                        match self.app.repeat {
                            RepeatMode::One => "repeat-one",
                            _ => "repeat",
                        },
                        18.,
                        if self.app.repeat != RepeatMode::Off { *PRIMARY } else { *MUTED_FG },
                    )),
            );

        let seek_row = div()
            .flex()
            .items_center()
            .gap(px(10.))
            .w(px(340.))
            .child(
                div()
                    .w(px(36.))
                    .flex()
                    .justify_end()
                    .text_size(px(t::SMALL.size))
                    .text_color(*MUTED_FG)
                    .child(mmss(position as u32)),
            )
            .child(div().flex_1().flex().items_center().child(seek_slider))
            .child(
                div()
                    .w(px(36.))
                    .text_size(px(t::SMALL.size))
                    .text_color(*MUTED_FG)
                    .child(mmss(duration as u32)),
            );

        let right = div()
            .flex()
            .items_center()
            .gap(px(4.))
            .flex_1()
            .justify_end()
            .child(
                div()
                    .id("pb-like")
                    .p(px(6.))
                    .rounded_full()
                    .when(liked, |el| el.bg(*CARD))
                    .hover(|s| s.bg(*CARD).cursor_pointer())
                    .on_click(cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                        if has_track {
                            this.app.toggle_like(track_id);
                            cx.notify();
                        }
                    }))
                    .child(icon(
                        if liked { "heart-filled" } else { "heart" },
                        18.,
                        if liked { *PRIMARY } else { *MUTED_FG },
                    )),
            )
            .child(
                div()
                    .id("pb-queue")
                    .p(px(6.))
                    .rounded_full()
                    .when(self.queue_open, |el| el.bg(*CARD))
                    .hover(|s| s.bg(*CARD).cursor_pointer())
                    .on_click(cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
                        this.queue_open = !this.queue_open;
                        cx.notify();
                    }))
                    .child(icon(
                        "queue",
                        18.,
                        if self.queue_open { *PRIMARY } else { *MUTED_FG },
                    )),
            )
            .child(
                div()
                    .id("pb-mute")
                    .p(px(6.))
                    .rounded_full()
                    .hover(|s| s.bg(*CARD).cursor_pointer())
                    .on_click(cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
                        this.app.toggle_mute();
                        cx.notify();
                    }))
                    .child(icon(
                        if self.app.muted || self.app.volume < 0.01 {
                            "volume-mute"
                        } else {
                            "volume"
                        },
                        18.,
                        *MUTED_FG,
                    )),
            )
            .child(div().flex().items_center().child(volume_slider));

        div()
            .id("player-bar")
            .h(px(t::PLAYER_BAR_H))
            .flex()
            .items_center()
            .px(px(16.))
            .bg(*BG)
            .child(info)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(4.))
                    .child(transport)
                    .child(seek_row),
            )
            .child(right)
            .into_any_element()
    }

    // — Track row (used by every list) ————————————————————————————

    fn track_row(
        &self,
        cx: &Context<Zuno>,
        ix: usize,
        id: TrackId,
        ids: Rc<Vec<TrackId>>,
    ) -> gpui::AnyElement {
        let v = self.app.library.track_view(id);
        let current = self.app.current == Some(id);
        let playing_here = current && self.app.playing;
        let selected = self.app.selection == Some(ix);
        let art_seed = AppState::album_seed(self.app.library.track(id).album_id);

        let index_cell: gpui::AnyElement = if current {
            if playing_here {
                Self::playing_bars(self.frame).into_any_element()
            } else {
                div()
                    .w(px(28.))
                    .flex()
                    .justify_center()
                    .child(icon("play", 13., *PRIMARY))
                    .into_any_element()
            }
        } else {
            div()
                .w(px(28.))
                .flex()
                .justify_end()
                .text_size(px(t::SMALL.size))
                .text_color(*MUTED_FG)
                .child(format!("{:>3}", ix + 1))
                .into_any_element()
        };

        let right: gpui::AnyElement = if v.liked {
            div()
                .w(px(36.))
                .flex()
                .justify_end()
                .child(icon("heart-filled", 14., *PRIMARY))
                .into_any_element()
        } else {
            div()
                .w(px(36.))
                .flex()
                .justify_end()
                .text_size(px(t::SMALL.size))
                .text_color(*MUTED_FG)
                .child(mmss(v.duration_sec))
                .into_any_element()
        };

        let mut title_row = div().flex().items_center().gap(px(6.)).min_w_0();
        title_row = title_row.child(
            div()
                .flex_1()
                .min_w_0()
                .line_clamp(1)
                .text_size(px(t::BODY.size))
                .font_weight(FontWeight::MEDIUM)
                .text_color(if current { *PRIMARY } else { *FG })
                .child(v.title.to_string()),
        );
        if v.explicit {
            title_row = title_row.child(
                div()
                    .flex_shrink_0()
                    .size(px(14.))
                    .rounded_sm()
                    .bg(*MUTED_FG)
                    .opacity(0.35)
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(9.))
                    .font_weight(FontWeight::BOLD)
                    .text_color(*FG)
                    .child("E"),
            );
        }

        let row = div()
            .id(ix)
            .w_full()
            .h(px(t::ROW_H))
            .flex()
            .items_center()
            .gap(px(12.))
            .px(px(8.))
            .rounded_lg()
            .when(selected, |el| el.bg(PRIMARY.opacity(0.10)))
            .when(current && !selected, |el| el.bg(PRIMARY.opacity(0.05)))
            .when(!current && !selected, |el| {
                el.hover(|s| s.bg(*CARD).cursor_pointer())
            })
            .when(current && !selected, |el| {
                el.hover(|s| s.cursor_pointer())
            })
            .on_click({
                let ids = ids.clone();
                cx.listener(move |this, _: &gpui::ClickEvent, _, cx| {
                    let ids = ids.clone();
                    this.app.play_from(&ids, ix);
                    cx.notify();
                })
            })
            .child(index_cell)
            .child(self.art_img(art_seed, 40, t::ROW_ART, false))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .min_w_0()
                    .flex_1()
                    .child(title_row)
                    .child(
                        div()
                            .line_clamp(1)
                            .text_size(px(t::SMALL.size))
                            .text_color(*MUTED_FG)
                            .child(v.artist.to_string()),
                    ),
            )
            .child(right);
        row.into_any_element()
    }

    /// The animated 3-bar "now playing" glyph, driven by the frame counter.
    fn playing_bars(frame: u64) -> gpui::Div {
        let phase = |offset: f32| {
            let t = (frame as f32 / 60.0 + offset) % 1.0;
            (t * std::f32::consts::TAU).sin()
        };
        let bar = |h: f32| {
            div()
                .w(px(3.))
                .h(px(h))
                .rounded_full()
                .bg(*PRIMARY)
        };
        let h = |p: f32| 4.0 + (p * 0.5 + 0.5) * 10.0;
        div()
            .w(px(18.))
            .flex()
            .items_center()
            .justify_center()
            .gap(px(2.))
            .child(bar(h(phase(0.0))))
            .child(bar(h(phase(0.33))))
            .child(bar(h(phase(0.66))))
    }

    // — Small text helpers ———————————————————————————————————————————

    fn h1(label: &str) -> gpui::Div {
        div()
            .text_size(px(t::H1.size))
            .font_weight(FontWeight::BOLD)
            .text_color(*FG)
            .child(label.to_string())
    }

    fn h2(label: &str) -> gpui::Div {
        div()
            .text_size(px(t::H2.size))
            .font_weight(FontWeight::BOLD)
            .text_color(*FG)
            .child(label.to_string())
    }

    fn h3(label: &str) -> gpui::Div {
        div()
            .text_size(px(t::H3.size))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(*FG)
            .child(label.to_string())
    }

    /// h1 + an optional back button (shown when history exists).
    fn page_header(&self, cx: &Context<Self>, title: &str) -> gpui::AnyElement {
        div()
            .flex()
            .items_center()
            .gap(px(12.))
            .when(self.app.can_go_back(), |el| {
                el.child(
                    div()
                        .id(eid!("back-{title}"))
                        .p(px(8.))
                        .rounded_full()
                        .hover(|s| s.bg(*CARD).cursor_pointer())
                        .on_click(cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
                            this.app.go_back();
                            cx.notify();
                        }))
                        .child(icon("back", 20., *MUTED_FG)),
                )
            })
            .child(Self::h1(title))
            .into_any_element()
    }

    /// Standard scrollable page body with the spec's padding.
    fn page_scroll(&self, body: impl IntoElement) -> gpui::AnyElement {
        div()
            .id("page-scroll")
            .overflow_y_scroll()
            .h_full()
            .w_full()
            .px(px(t::PAGE_PAD))
            .pb(px(t::PAGE_PAD))
            .child(body)
            .into_any_element()
    }
}

// — Bootstrap —————————————————————————————————————————————————————————

fn main() {
    Application::new().with_assets(ZunoAssets).run(|cx: &mut App| {
        cx.text_system()
            .add_fonts(vec![include_bytes!("../assets/Inter-Variable.ttf").as_ref().into()])
            .expect("failed to load Inter");

        cx.bind_keys([
            KeyBinding::new("space", TogglePlay, Some("Zuno && !SearchInput")),
            KeyBinding::new("left", SeekBack, Some("Zuno && !SearchInput")),
            KeyBinding::new("right", SeekFwd, Some("Zuno && !SearchInput")),
            KeyBinding::new("m", ToggleMute, Some("Zuno && !SearchInput")),
            KeyBinding::new("q", ToggleQueue, Some("Zuno && !SearchInput")),
            KeyBinding::new("s", ToggleShuffle, Some("Zuno && !SearchInput")),
            KeyBinding::new("r", CycleRepeat, Some("Zuno && !SearchInput")),
            KeyBinding::new("escape", EscapeKey, Some("Zuno && !SearchInput")),
            KeyBinding::new("ctrl-k", FocusSearch, None),
            KeyBinding::new("backspace", Backspace, Some("SearchInput")),
            KeyBinding::new("delete", Delete, Some("SearchInput")),
            KeyBinding::new("left", Left, Some("SearchInput")),
            KeyBinding::new("right", Right, Some("SearchInput")),
            KeyBinding::new("home", Home, Some("SearchInput")),
            KeyBinding::new("end", End, Some("SearchInput")),
            KeyBinding::new("escape", SearchDismiss, Some("SearchInput")),
        ]);

        cx.on_window_closed(|cx| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();

        let bounds = Bounds::centered(None, size(px(1280.), px(800.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_min_size: Some(size(px(900.), px(600.))),
                window_background: WindowBackgroundAppearance::Opaque,
                titlebar: Some(gpui::TitlebarOptions {
                    title: Some("Zuno".into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            |window, cx| {
                let app = cx.new(|cx| Zuno::new(cx));
                let handle = app.read(cx).focus_handle.clone();
                window.focus(&handle);
                app
            },
        )
        .unwrap();
        cx.activate(true);
    });
}
