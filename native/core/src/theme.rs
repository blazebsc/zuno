//! The React app's design tokens, extracted from `src/ui/styles/global.css`
//! and the component classes, converted from oklch to sRGB. Every native GUI
//! renders from these constants so the eight candidates can be judged on the
//! same visual language — the goal is "how good can Zuno look", not "eight
//! different-looking demos".
//!
//! Source of truth per token is noted; don't tune these per framework.

/// One sRGB colour, components 0-255. Cheap to copy, convertible to every
/// framework's colour type in one line.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Rgb(pub [u8; 3]);

impl Rgb {
    pub const fn hex(v: [u8; 3]) -> Self {
        Rgb(v)
    }
    /// Blend toward white by `amount` (0..1). Zuno's light tints derive this way.
    pub fn lighten(&self, amount: f32) -> Rgb {
        let m = |c: u8| (c as f32 + (255.0 - c as f32) * amount) as u8;
        Rgb([m(self.0[0]), m(self.0[1]), m(self.0[2])])
    }
    /// Blend toward black by `amount` (0..1).
    pub fn darken(&self, amount: f32) -> Rgb {
        let m = |c: u8| (c as f32 * (1.0 - amount)) as u8;
        Rgb([m(self.0[0]), m(self.0[1]), m(self.0[2])])
    }
    /// The colour at `alpha` over `base`, as premultiplied-agnostic RGBA.
    pub fn over(&self, base: Rgb, alpha: f32) -> Rgb {
        let m = |a: u8, b: u8| (a as f32 * alpha + b as f32 * (1.0 - alpha)) as u8;
        Rgb([
            m(self.0[0], base.0[0]),
            m(self.0[1], base.0[1]),
            m(self.0[2], base.0[2]),
        ])
    }
}

/// The dark palette (`@theme` defaults in global.css) and the light palette
/// (`html[data-theme="light"]`). `primary` stays `#ff0033` in both — it is the
/// brand accent, never a surface.
pub struct Palette {
    /// `--color-background` — oklch(0.145 0 0)
    pub background: Rgb,
    /// `--color-foreground` — oklch(0.985 0 0)
    pub foreground: Rgb,
    /// `--color-card` — oklch(0.285 0 0). Hover surfaces and active pills.
    pub card: Rgb,
    /// `--color-popover` — oklch(0.205 0 0). Menus, overlays.
    pub popover: Rgb,
    /// `--color-muted` — oklch(0.269 0 0)
    pub muted: Rgb,
    /// `--color-muted-foreground` — oklch(0.708 0 0). Secondary text.
    pub muted_foreground: Rgb,
    /// `--color-primary` — #ff0033, the brand accent.
    pub primary: Rgb,
}

pub const DARK: Palette = Palette {
    background: Rgb::hex([10, 10, 10]),
    foreground: Rgb::hex([250, 250, 250]),
    card: Rgb::hex([42, 42, 42]),
    popover: Rgb::hex([23, 23, 23]),
    muted: Rgb::hex([38, 38, 38]),
    muted_foreground: Rgb::hex([161, 161, 161]),
    primary: Rgb::hex([255, 0, 51]),
};

pub const LIGHT: Palette = Palette {
    background: Rgb::hex([240, 240, 240]),
    foreground: Rgb::hex([23, 23, 23]),
    card: Rgb::hex([255, 255, 255]),
    popover: Rgb::hex([255, 255, 255]),
    muted: Rgb::hex([227, 227, 227]),
    muted_foreground: Rgb::hex([85, 85, 85]),
    primary: Rgb::hex([255, 0, 51]),
};

pub const fn dark() -> Palette {
    DARK
}

/// Row/state colours derived exactly as the Tailwind utilities compose them.
pub mod state {
    use super::{Rgb, DARK};

    /// `hover:bg-card` — plain card surface.
    pub const ROW_HOVER: Rgb = DARK.card;
    /// `isCurrent && "bg-primary/5"` — the playing row.
    pub fn row_playing() -> Rgb {
        DARK.primary.over(DARK.background, 0.05)
    }
    /// `isSelected && "bg-primary/10"` — selected rows.
    pub fn row_selected() -> Rgb {
        DARK.primary.over(DARK.background, 0.10)
    }
    /// `hover:bg-card` on the sidebar over the background-tinted rail.
    pub const SIDEBAR_HOVER: Rgb = DARK.card;
    /// Sidebar active pill.
    pub const SIDEBAR_ACTIVE: Rgb = DARK.card;
    /// AlbumCard hover overlay — `bg-background/50`.
    pub fn card_scrim() -> Rgb {
        DARK.background
    }
    pub const CARD_SCRIM_ALPHA: f32 = 0.5;
}

// — Layout constants (global.css `:root` + component classes) ————————
// All in logical px; frameworks with HiDPI handle scaling themselves.

/// `--titlebar-height`.
pub const TITLEBAR_H: f32 = 44.0;
/// `--sidebar-width` (the mid "hover" mode; 62 collapsed / 240 expanded).
pub const SIDEBAR_W: f32 = 220.0;
pub const SIDEBAR_W_COLLAPSED: f32 = 62.0;
pub const SIDEBAR_W_EXPANDED: f32 = 240.0;
/// TrackRow intrinsic height — `contain-intrinsic-size: auto 52px`
/// (40px artwork + `py-1.5` padding).
pub const ROW_H: f32 = 52.0;
/// TrackRow artwork (`size-10`), rounded-lg (4px).
pub const ROW_ART: f32 = 40.0;
/// AlbumCard rendered width (`DEFAULT_CARD_SIZE`).
pub const CARD_W: f32 = 176.0;
/// Card art + the two label lines (`contain-intrinsic-size: auto 232px`).
pub const CARD_H: f32 = 232.0;
/// QueuePanel width, full mode (collapsed rail is the artwork column).
pub const QUEUE_W: f32 = 300.0;
/// PlayerBar height, expanded layout (SeekBar row + controls row + padding).
pub const PLAYER_BAR_H: f32 = 76.0;
/// Content padding on pages (`p-4` .. `p-6`).
pub const PAGE_PAD: f32 = 20.0;
/// Gap between home sections (`flex flex-col gap-6` region).
pub const SECTION_GAP: f32 = 24.0;

// — Radii ————————————————————————————————————————————————
/// Rows, sidebar items — `rounded-lg`.
pub const RADIUS_ROW: f32 = 8.0;
/// Artwork tiles — `rounded` (4px).
pub const RADIUS_ART: f32 = 4.0;
/// Artist photos — `rounded-full`.
pub const RADIUS_FULL: f32 = 512.0;
/// Buttons and inputs — `rounded-full` in Zuno's chrome.
pub const RADIUS_BTN: f32 = 999.0;

// — Typography (global.css @layer base + Tailwind scale) ——————————
/// Inter with `cv11`/`ss01`. Every framework should load its closest Inter
/// (bundled variable font where the framework supports it) and fall back to
/// system sans rather than shipping a different identity face.
pub const FONT_FAMILY: &str = "Inter";

pub struct TextStyle {
    pub size: f32,
    pub weight: u16,
    /// letter-spacing in em.
    pub tracking: f32,
    /// line-height as a multiple.
    pub line_height: f32,
}

/// h1 — `font-size: 2rem; font-weight: 700; letter-spacing: -0.03em; line-height: 1.15`
pub const H1: TextStyle = TextStyle { size: 32.0, weight: 700, tracking: -0.03, line_height: 1.15 };
/// h2 — 1.5rem / 700 / -0.02em / 1.25
pub const H2: TextStyle = TextStyle { size: 24.0, weight: 700, tracking: -0.02, line_height: 1.25 };
/// h3 — 1.125rem / 600 / -0.01em / 1.3
pub const H3: TextStyle = TextStyle { size: 18.0, weight: 600, tracking: -0.01, line_height: 1.3 };
/// `text-sm` body — 14px / 400-500
pub const BODY: TextStyle = TextStyle { size: 14.0, weight: 400, tracking: 0.0, line_height: 1.45 };
/// `text-sm font-medium` — card titles, row titles.
pub const BODY_MED: TextStyle = TextStyle { size: 14.0, weight: 500, tracking: 0.0, line_height: 1.45 };
/// `text-xs` — 12px secondary text, durations, meta.
pub const SMALL: TextStyle = TextStyle { size: 12.0, weight: 400, tracking: 0.0, line_height: 1.35 };
/// `text-xs font-medium` — pills, badges.
pub const SMALL_MED: TextStyle = TextStyle { size: 12.0, weight: 500, tracking: 0.0, line_height: 1.35 };
/// `text-[11px]` — the smallest Zuno goes (badges in rows).
pub const XS: TextStyle = TextStyle { size: 11.0, weight: 500, tracking: 0.01, line_height: 1.3 };

/// `tabular-nums` — every duration, count and clock must use tabular figures.
pub const TABULAR_NUMS: bool = true;

// — Motion (beUI `ease` tokens; hover transitions) —————————————————
/// Zuno's hover colour transitions.
pub const HOVER_MS: f32 = 120.0;
/// Card play-scrrim fade-in.
pub const SCRIM_MS: f32 = 150.0;
/// Page/view switch fade.
pub const PAGE_MS: f32 = 120.0;
/// Standard ease-out curve for the above (cubic).
pub fn ease_out(t: f32) -> f32 {
    1.0 - (1.0 - t).powi(3)
}
