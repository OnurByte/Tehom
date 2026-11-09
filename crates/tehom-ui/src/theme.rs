//! The onur.af design system applied to Tehom.
//!
//! Monochrome tokens that follow the system light/dark preference, Helvetica
//! Neue with plain fallbacks, 12 px cards, 8 px controls at least 44 px high,
//! filled primary and bordered secondary actions. No accent colour or glow.
use gpui_kit::{
    App, Global, Hsla, Rgba,
    component::{Theme, ThemeMode},
    px,
};

/// Brand tokens for one theme (`--background`, `--text`, `--muted`, `--border`, `--surface`).
#[derive(Clone, Copy)]
pub struct Palette {
    pub background: Rgba,
    pub text: Rgba,
    pub muted: Rgba,
    pub border: Rgba,
    pub surface: Rgba,
    /// The corner light of the shared edge: text mixed 55% into the border.
    pub edge: Rgba,
}

pub const DARK: Palette = Palette {
    background: rgba(0x000000),
    text: rgba(0xffffff),
    muted: rgba(0xffffff),
    border: rgba(0x404040),
    surface: rgba(0x222222),
    edge: rgba(0xa9a9a9),
};
pub const LIGHT: Palette = Palette {
    background: rgba(0xffffff),
    text: rgba(0x0d0d0d),
    muted: rgba(0x333333),
    border: rgba(0xb3b3b3),
    surface: rgba(0xf2f2f2),
    edge: rgba(0x585858),
};

const fn rgba(hex: u32) -> Rgba {
    Rgba {
        r: ((hex >> 16) & 0xff) as f32 / 255.,
        g: ((hex >> 8) & 0xff) as f32 / 255.,
        b: (hex & 0xff) as f32 / 255.,
        a: 1.,
    }
}

/// The palette for the theme currently in effect.
pub fn palette(cx: &App) -> Palette {
    if Theme::global(cx).mode.is_dark() {
        DARK
    } else {
        LIGHT
    }
}

/// The first installed family of the brand stack.
fn brand_font(cx: &App) -> &'static str {
    let installed = cx.text_system().all_font_names();
    ["Helvetica Neue", "Helvetica", "Arial", "Liberation Sans"]
        .into_iter()
        .find(|family| installed.iter().any(|name| name == family))
        .unwrap_or("Geist")
}

/// The user's choice and the system's appearance, so either can change.
#[derive(Default)]
struct Look {
    choice: tehom_core::Appearance,
    system_dark: bool,
}
impl Global for Look {}

/// Installs the brand theme for the given system appearance, unless the user
/// chose Light or Dark in Settings.
pub fn apply(system_dark: bool, cx: &mut App) {
    cx.default_global::<Look>().system_dark = system_dark;
    install(cx);
}

/// Applies the user's appearance choice at once.
pub fn set_appearance(choice: tehom_core::Appearance, cx: &mut App) {
    cx.default_global::<Look>().choice = choice;
    install(cx);
    cx.refresh_windows();
}

fn install(cx: &mut App) {
    let look = cx.default_global::<Look>();
    let dark = match look.choice {
        tehom_core::Appearance::System => look.system_dark,
        tehom_core::Appearance::Light => false,
        tehom_core::Appearance::Dark => true,
    };
    let p = if dark { DARK } else { LIGHT };
    let font = brand_font(cx);
    Theme::change(
        if dark {
            ThemeMode::Dark
        } else {
            ThemeMode::Light
        },
        None,
        cx,
    );
    let theme = Theme::global_mut(cx);
    theme.font_family = font.into();
    theme.mono_font_family = "Geist Mono".into();
    theme.font_size = px(16.);
    theme.radius = px(8.);
    theme.radius_lg = px(12.);
    let c = &mut theme.colors;
    let hover: Hsla = p.surface.into();
    c.background = p.background.into();
    c.foreground = p.text.into();
    c.muted = p.surface.into();
    // Placeholders and secondary hints sit one step below the text.
    c.muted_foreground = p.edge.into();
    c.border = p.border.into();
    c.input = p.border.into();
    c.ring = p.text.into();
    c.caret = p.text.into();
    c.selection = Hsla::from(p.text).opacity(0.25);
    c.primary = p.text.into();
    c.primary_foreground = p.background.into();
    c.primary_hover = p.muted.into();
    c.primary_active = p.muted.into();
    c.secondary = p.background.into();
    c.secondary_foreground = p.text.into();
    c.secondary_hover = hover;
    c.secondary_active = hover;
    c.accent = p.surface.into();
    c.accent_foreground = p.text.into();
    c.button_primary = p.text.into();
    c.button_primary_foreground = p.background.into();
    c.button_primary_hover = Hsla::from(p.text).opacity(0.85);
    c.button_primary_active = Hsla::from(p.text).opacity(0.75);
    c.button = p.background.into();
    c.button_foreground = p.text.into();
    c.button_hover = hover;
    c.button_active = hover;
    c.button_secondary = p.background.into();
    c.button_secondary_foreground = p.text.into();
    c.button_secondary_hover = hover;
    c.button_secondary_active = hover;
    c.list_hover = hover;
    c.list_active = hover;
    c.popover = p.background.into();
    c.popover_foreground = p.text.into();
    c.sidebar = p.background.into();
    c.sidebar_foreground = p.text.into();
    c.sidebar_border = p.border.into();
    c.sidebar_accent = p.surface.into();
    c.sidebar_accent_foreground = p.text.into();
    c.sidebar_primary = p.text.into();
    c.sidebar_primary_foreground = p.background.into();
    c.title_bar = p.background.into();
    c.title_bar_border = p.border.into();
    c.window_border = p.border.into();
    c.scrollbar_thumb = p.border.into();
    c.progress_bar = p.text.into();
    c.slider_bar = p.text.into();
    c.slider_thumb = p.text.into();
    theme.tokens.button_primary = theme.colors.button_primary.into();
    theme.tokens.button_primary_hover = theme.colors.button_primary_hover.into();
    theme.tokens.button_primary_active = theme.colors.button_primary_active.into();
    Theme::sync_base(cx);
}
