//! Zuno's design tokens (zuno_core::theme) mapped onto iced styling.
//! Borderless surfaces separated by contrast — no outlines anywhere.

use iced::border::Radius;
use iced::{Background, Border, Color, Padding};
use zuno_core::theme;

pub fn background() -> Color {
    Color::from_rgb8(10, 10, 10)
}
pub fn surface() -> Color {
    Color::from_rgb8(10, 10, 10)
}
pub fn card() -> Color {
    Color::from_rgb8(42, 42, 42)
}
pub fn popover() -> Color {
    Color::from_rgb8(23, 23, 23)
}
pub fn muted_fg() -> Color {
    Color::from_rgb8(161, 161, 161)
}
pub fn fg() -> Color {
    Color::from_rgb8(250, 250, 250)
}
pub fn primary() -> Color {
    Color::from_rgb8(255, 0, 51)
}
/// `bg-primary/5` — the playing row.
pub fn row_playing() -> Color {
    Color::from_rgba(255.0 / 255.0, 0.0, 51.0 / 255.0, 0.05)
}
/// `bg-primary/10` — selected rows.
pub fn row_selected() -> Color {
    Color::from_rgba(1.0, 0.0, 0.2, 0.10)
}

pub const INTER: iced::Font = iced::Font::with_name("Inter");
pub fn font(weight: u16) -> iced::Font {
    let weight = match weight {
        w if w >= 800 => iced::font::Weight::ExtraBold,
        w if w >= 700 => iced::font::Weight::Bold,
        w if w >= 600 => iced::font::Weight::Semibold,
        w if w >= 500 => iced::font::Weight::Medium,
        _ => iced::font::Weight::Normal,
    };
    iced::Font {
        weight,
        ..INTER
    }
}

pub const R_ROW: Radius = Radius {
    top_left: 8.0,
    top_right: 8.0,
    bottom_right: 8.0,
    bottom_left: 8.0,
};
pub const R_ART: Radius = Radius {
    top_left: 4.0,
    top_right: 4.0,
    bottom_right: 4.0,
    bottom_left: 4.0,
};
pub const R_PILL: Radius = Radius {
    top_left: 999.0,
    top_right: 999.0,
    bottom_right: 999.0,
    bottom_left: 999.0,
};

pub fn pad(v: f32) -> Padding {
    Padding::new(v)
}

// — Button styles ————————————————————————————————————————————

/// Flat row: transparent, `card` on hover, tinted when playing/selected.
pub fn row_button(playing: bool, selected: bool) -> impl Fn(&iced::Theme, iced::widget::button::Status) -> iced::widget::button::Style {
    move |_t: &iced::Theme, status: iced::widget::button::Status| {
        let base = if selected {
            row_selected()
        } else if playing {
            row_playing()
        } else {
            Color::TRANSPARENT
        };
        match status {
            iced::widget::button::Status::Hovered | iced::widget::button::Status::Pressed => {
                iced::widget::button::Style {
                    background: Some(Background::Color(card())),
                    text_color: fg(),
                    border: Border {
                        radius: R_ROW,
                        ..Default::default()
                    },
                    ..Default::default()
                }
            }
            _ => iced::widget::button::Style {
                background: Some(Background::Color(base)),
                text_color: fg(),
                border: Border {
                    radius: R_ROW,
                    ..Default::default()
                },
                ..Default::default()
            },
        }
    }
}

/// Sidebar nav item: transparent, `card` on hover, `card` + primary text when active.
pub fn nav_button(active: bool, playing_here: bool) -> impl Fn(&iced::Theme, iced::widget::button::Status) -> iced::widget::button::Style {
    move |_t: &iced::Theme, status: iced::widget::button::Status| {
        let (bg, text) = match status {
            iced::widget::button::Status::Hovered | iced::widget::button::Status::Pressed => (card(), fg()),
            _ if active => (card(), fg()),
            _ => (Color::TRANSPARENT, if playing_here { primary() } else { muted_fg() }),
        };
        iced::widget::button::Style {
            background: Some(Background::Color(bg)),
            text_color: text,
            border: Border {
                radius: R_ROW,
                ..Default::default()
            },
            ..Default::default()
        }
    }
}

/// Primary action — the red play pill and Play All buttons.
pub fn primary_pill() -> impl Fn(&iced::Theme, iced::widget::button::Status) -> iced::widget::button::Style {
    move |_t: &iced::Theme, status: iced::widget::button::Status| {
        let bg = match status {
            iced::widget::button::Status::Hovered => Color::from_rgb8(255, 26, 64),
            iced::widget::button::Status::Pressed => Color::from_rgb8(214, 0, 43),
            _ => primary(),
        };
        iced::widget::button::Style {
            background: Some(Background::Color(bg)),
            text_color: Color::WHITE,
            border: Border {
                radius: R_PILL,
                ..Default::default()
            },
            ..Default::default()
        }
    }
}

/// Card-coloured circular/round chip (play-in-scrim, transport buttons).
pub fn chip(active: bool) -> impl Fn(&iced::Theme, iced::widget::button::Status) -> iced::widget::button::Style {
    move |_t: &iced::Theme, status: iced::widget::button::Status| {
        let (bg, text) = match status {
            iced::widget::button::Status::Hovered | iced::widget::button::Status::Pressed => {
                (card(), fg())
            }
            _ if active => (card(), primary()),
            _ => (Color::TRANSPARENT, muted_fg()),
        };
        iced::widget::button::Style {
            background: Some(Background::Color(bg)),
            text_color: text,
            border: Border {
                radius: R_PILL,
                ..Default::default()
            },
            ..Default::default()
        }
    }
}

/// Library tab pill.
pub fn tab_pill(active: bool) -> impl Fn(&iced::Theme, iced::widget::button::Status) -> iced::widget::button::Style {
    move |_t: &iced::Theme, status: iced::widget::button::Status| {
        let (bg, text) = match status {
            iced::widget::button::Status::Hovered | iced::widget::button::Status::Pressed if !active => {
                (Color::from_rgba(1.0, 1.0, 1.0, 0.04), fg())
            }
            _ if active => (card(), fg()),
            _ => (Color::TRANSPARENT, muted_fg()),
        };
        iced::widget::button::Style {
            background: Some(Background::Color(bg)),
            text_color: text,
            border: Border {
                radius: R_PILL,
                ..Default::default()
            },
            ..Default::default()
        }
    }
}

// — Scrollbar ———————————————————————————————————————————————

/// Thin, unobtrusive — the React app hides native scrollbars entirely and
/// draws a transient one; this is iced's closest respectable equivalent.
pub fn scrollable() -> impl Fn(&iced::Theme, iced::widget::scrollable::Status) -> iced::widget::scrollable::Style {
    move |_t: &iced::Theme, status: iced::widget::scrollable::Status| {
        let (scroller_bg, auto_scroll_bg) = match status {
            iced::widget::scrollable::Status::Hovered { .. } => (
                Color::from_rgba(1.0, 1.0, 1.0, 0.18),
                Color::from_rgba(1.0, 1.0, 1.0, 0.18),
            ),
            _ => (
                Color::from_rgba(1.0, 1.0, 1.0, 0.10),
                Color::from_rgba(1.0, 1.0, 1.0, 0.10),
            ),
        };

        let scroller = iced::widget::scrollable::Scroller {
            background: Background::Color(scroller_bg),
            border: Border {
                radius: R_PILL.into(),
                width: 0.0,
                color: Color::TRANSPARENT,
            },
        };
        let rail = iced::widget::scrollable::Rail {
            background: Some(Background::Color(Color::TRANSPARENT)),
            border: Border {
                radius: R_PILL.into(),
                width: 0.0,
                color: Color::TRANSPARENT,
            },
            scroller,
        };
        let auto_scroll = iced::widget::scrollable::AutoScroll {
            background: Background::Color(auto_scroll_bg),
            border: Border {
                radius: R_PILL.into(),
                width: 0.0,
                color: Color::TRANSPARENT,
            },
            shadow: iced::Shadow {
                color: Color::from_rgba(0.0, 0.0, 0.0, 0.3),
                offset: iced::Vector::new(0.0, 4.0),
                blur_radius: 8.0,
            },
            icon: Color::WHITE,
        };

        iced::widget::scrollable::Style {
            container: iced::widget::container::Style::default(),
            vertical_rail: rail,
            horizontal_rail: rail,
            gap: None,
            auto_scroll,
        }
    }
}

// — Sliders ————————————————————————————————————————————————

pub fn slider() -> impl Fn(&iced::Theme, iced::widget::slider::Status) -> iced::widget::slider::Style {
    move |_t: &iced::Theme, status: iced::widget::slider::Status| {
        let handle_radius = match status {
            iced::widget::slider::Status::Hovered | iced::widget::slider::Status::Dragged => 7.0,
            _ => 5.0,
        };
        iced::widget::slider::Style {
            rail: iced::widget::slider::Rail {
                backgrounds: (
                    Background::Color(primary()),
                    Background::Color(Color::from_rgba(1.0, 1.0, 1.0, 0.14)),
                ),
                width: 4.0,
                border: Border {
                    radius: R_PILL.into(),
                    width: 0.0,
                    color: Color::TRANSPARENT,
                },
            },
            handle: iced::widget::slider::Handle {
                shape: iced::widget::slider::HandleShape::Circle { radius: handle_radius },
                background: Background::Color(Color::WHITE),
                border_width: 0.0,
                border_color: Color::TRANSPARENT,
            },
        }
    }
}

// — Named style functions ————————————————————————————————
// Every one of these replaces an inline `move |_t| ...` closure: named
// functions give the compiler a concrete signature, which is what fixes
// the type-inference failures inline closures caused inside column!/row!.

/// Muted-foreground text (the old `muted()` helper).
pub fn text_muted() -> impl Fn(&iced::Theme) -> iced::widget::text::Style {
    |_t: &iced::Theme| iced::widget::text::Style {
        color: Some(muted_fg()),
    }
}

/// Plain foreground text.
pub fn text_plain() -> impl Fn(&iced::Theme) -> iced::widget::text::Style {
    |_t: &iced::Theme| iced::widget::text::Style { color: Some(fg()) }
}

/// Text that is `fg` when active, muted when not — one concrete type for
/// both branches of an `if` (two separate `impl Fn` never unify).
pub fn text_active(active: bool) -> impl Fn(&iced::Theme) -> iced::widget::text::Style {
    move |_t: &iced::Theme| iced::widget::text::Style {
        color: Some(if active { fg() } else { muted_fg() }),
    }
}

/// Card container (`bg-card` surface).
pub fn container_card() -> impl Fn(&iced::Theme) -> iced::widget::container::Style {
    |_t: &iced::Theme| iced::widget::container::Style {
        background: Some(Background::Color(card())),
        text_color: Some(fg()),
        ..Default::default()
    }
}

/// Transparent container.
pub fn container_plain() -> impl Fn(&iced::Theme) -> iced::widget::container::Style {
    |_t: &iced::Theme| iced::widget::container::Style::default()
}

/// App-background container.
pub fn container_bg() -> impl Fn(&iced::Theme) -> iced::widget::container::Style {
    |_t: &iced::Theme| iced::widget::container::Style {
        background: Some(Background::Color(background())),
        text_color: Some(fg()),
        ..Default::default()
    }
}

/// Queue rail panel — popover-dark, full-height.
pub fn container_rail() -> impl Fn(&iced::Theme) -> iced::widget::container::Style {
    |_t: &iced::Theme| iced::widget::container::Style {
        background: Some(Background::Color(Color::from_rgba(0.067, 0.067, 0.067, 0.985))),
        text_color: Some(fg()),
        ..Default::default()
    }
}

/// The hover scrim over album-card artwork (`bg-background/50`).
pub fn container_scrim() -> impl Fn(&iced::Theme) -> iced::widget::container::Style {
    |_t: &iced::Theme| iced::widget::container::Style {
        background: Some(Background::Color(Color::from_rgba(0.039, 0.039, 0.039, 0.5))),
        ..Default::default()
    }
}

/// Rounded-4 artwork tile wrapper.
pub fn container_art() -> impl Fn(&iced::Theme) -> iced::widget::container::Style {
    |_t: &iced::Theme| iced::widget::container::Style {
        border: Border { radius: 4.0.into(), ..Default::default() },
        ..Default::default()
    }
}

/// Round (circle) artwork wrapper for artist photos.
pub fn container_art_round() -> impl Fn(&iced::Theme) -> iced::widget::container::Style {
    |_t: &iced::Theme| iced::widget::container::Style {
        border: Border { radius: 116.0.into(), ..Default::default() },
        ..Default::default()
    }
}

/// Settings row — faint surface.
pub fn container_setting() -> impl Fn(&iced::Theme) -> iced::widget::container::Style {
    |_t: &iced::Theme| iced::widget::container::Style {
        background: Some(Background::Color(Color::from_rgba(1.0, 1.0, 1.0, 0.03))),
        border: Border { radius: R_ROW, ..Default::default() },
        ..Default::default()
    }
}

/// The "E" explicit badge.
pub fn container_badge() -> impl Fn(&iced::Theme) -> iced::widget::container::Style {
    |_t: &iced::Theme| iced::widget::container::Style {
        background: Some(Background::Color(Color::from_rgba(1.0, 1.0, 1.0, 0.1))),
        border: Border { radius: 2.0.into(), ..Default::default() },
        ..Default::default()
    }
}

/// Ghost pill (secondary action: Shuffle, Go to artist).
pub fn pill_ghost() -> impl Fn(&iced::Theme, iced::widget::button::Status) -> iced::widget::button::Style {
    |_t: &iced::Theme, status: iced::widget::button::Status| {
        let bg = match status {
            iced::widget::button::Status::Hovered
            | iced::widget::button::Status::Pressed => card(),
            _ => Color::from_rgba(1.0, 1.0, 1.0, 0.06),
        };
        iced::widget::button::Style {
            background: Some(Background::Color(bg)),
            text_color: fg(),
            border: Border { radius: 999.0.into(), ..Default::default() },
            ..Default::default()
        }
    }
}

/// Text input — search field.
pub fn input_search() -> impl Fn(&iced::Theme, iced::widget::text_input::Status) -> iced::widget::text_input::Style {
    |_t: &iced::Theme, status: iced::widget::text_input::Status| {
        iced::widget::text_input::Style {
            background: Background::Color(Color::from_rgba(1.0, 1.0, 1.0, 0.05)),
            border: Border {
                radius: 999.0.into(),
                width: 1.0,
                color: match status {
                    iced::widget::text_input::Status::Active => Color::from_rgba(1.0, 1.0, 1.0, 0.08),
                    _ => primary(),
                },
            },
            icon: muted_fg(),
            placeholder: muted_fg(),
            value: fg(),
            selection: primary(),
        }
    }
}

// — iced theme ——————————————————————————————————————————————

pub fn iced_theme() -> iced::Theme {
    iced::Theme::custom(
        "Zuno",
        iced::theme::Palette {
            background: background(),
            text: fg(),
            primary: primary(),
            success: Color::from_rgb8(80, 200, 120),
            warning: Color::from_rgb8(240, 180, 60),
            danger: Color::from_rgb8(200, 60, 60),
        },
    )
}

/// Section h2 (24/700), page h1 (32/700).
///
/// The `'a` is a free lifetime parameter, NOT tied to the `&str` input —
/// the fragment is owned (`to_string()`), so the returned `Text` is valid
/// for any `'a` the caller's context picks. A fixed `Text<'static>` return
/// breaks inference wherever it is pushed into a column that also holds
/// `&self`-borrowing elements (`Element<'static>` and `Element<'a>` do
/// not unify through `impl Into<Element<'a>>`).
pub fn h1<'a>(text: &str) -> iced::widget::Text<'a> {
    iced::widget::text(text.to_string())
        .font(font(theme::H1.weight))
        .size(theme::H1.size)
        .line_height(iced::widget::text::LineHeight::from(theme::H1.line_height))
        .shaping(iced::widget::text::Shaping::Advanced)
}

pub fn h2<'a>(text: &str) -> iced::widget::Text<'a> {
    iced::widget::text(text.to_string())
        .font(font(theme::H2.weight))
        .size(theme::H2.size)
        .line_height(iced::widget::text::LineHeight::from(theme::H2.line_height))
        .shaping(iced::widget::text::Shaping::Advanced)
}