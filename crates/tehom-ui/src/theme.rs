//! Tehom's look: soft neutral surfaces that follow the system light/dark
//! preference, one accent colour the person picks, Helvetica Neue with plain
//! fallbacks, 12 px cards and 8 px controls at least 44 px high.
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
    /// The chosen accent, and the text colour that reads on it.
    pub accent: Rgba,
    pub on_accent: Rgba,
}

pub const DARK: Palette = Palette {
    background: rgba(0x0e0e11),
    text: rgba(0xf4f4f6),
    muted: rgba(0xa8a8b3),
    border: rgba(0x2e2e38),
    surface: rgba(0x1a1a20),
    edge: rgba(0x8a8a94),
    accent: rgba(0xf4f4f6),
    on_accent: rgba(0x0e0e11),
};
pub const LIGHT: Palette = Palette {
    background: rgba(0xfbfbfc),
    text: rgba(0x15151a),
    muted: rgba(0x55555f),
    border: rgba(0xd6d6de),
    surface: rgba(0xf0f0f4),
    edge: rgba(0x767682),
    accent: rgba(0x15151a),
    on_accent: rgba(0xfbfbfc),
};

/// The accent's colour; white text reads on every one of them (4.5:1 or more).
/// `None` for black and white, which uses the text colour instead.
pub fn accent_color(accent: tehom_core::Accent) -> Option<Rgba> {
    use tehom_core::Accent::*;
    Some(rgba(match accent {
        Violet => 0x6d4aff,
        Blue => 0x2563eb,
        Green => 0x15803d,
        Orange => 0xc2410c,
        Pink => 0xc81e78,
        Mono => return None,
    }))
}

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
    let accent = cx.try_global::<Look>().map(|look| look.accent);
    with_accent(Theme::global(cx).mode.is_dark(), accent.unwrap_or_default())
}

fn with_accent(dark: bool, accent: tehom_core::Accent) -> Palette {
    let base = if dark { DARK } else { LIGHT };
    match accent_color(accent) {
        Some(color) => Palette {
            accent: color,
            on_accent: rgba(0xffffff),
            ..base
        },
        None => base,
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
    accent: tehom_core::Accent,
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

/// Applies the user's accent colour at once.
pub fn set_accent(accent: tehom_core::Accent, cx: &mut App) {
    cx.default_global::<Look>().accent = accent;
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
    let p = with_accent(dark, look.accent);
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
    c.ring = p.accent.into();
    c.caret = p.text.into();
    c.selection = Hsla::from(p.text).opacity(0.25);
    c.primary = p.accent.into();
    c.primary_foreground = p.on_accent.into();
    c.primary_hover = Hsla::from(p.accent).opacity(0.85);
    c.primary_active = Hsla::from(p.accent).opacity(0.75);
    c.secondary = p.background.into();
    c.secondary_foreground = p.text.into();
    c.secondary_hover = hover;
    c.secondary_active = hover;
    c.accent = p.surface.into();
    c.accent_foreground = p.text.into();
    c.button_primary = p.accent.into();
    c.button_primary_foreground = p.on_accent.into();
    c.button_primary_hover = Hsla::from(p.accent).opacity(0.85);
    c.button_primary_active = Hsla::from(p.accent).opacity(0.75);
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
    c.sidebar_primary = p.accent.into();
    c.sidebar_primary_foreground = p.on_accent.into();
    c.title_bar = p.background.into();
    c.title_bar_border = p.border.into();
    c.window_border = p.border.into();
    c.scrollbar_thumb = p.border.into();
    c.progress_bar = p.accent.into();
    c.slider_bar = p.accent.into();
    c.slider_thumb = p.accent.into();
    theme.tokens.button_primary = theme.colors.button_primary.into();
    theme.tokens.button_primary_hover = theme.colors.button_primary_hover.into();
    theme.tokens.button_primary_active = theme.colors.button_primary_active.into();
    Theme::sync_base(cx);
}

/// A bordered 12 px card, the container every page section uses.
pub fn card(cx: &App) -> gpui_kit::Div {
    use gpui_kit::Styled as _;
    let p = palette(cx);
    gpui_kit::div()
        .p_5()
        .bg(p.background)
        .border_1()
        .border_color(p.border)
        .rounded(px(12.))
        .flex()
        .flex_col()
        .gap_3()
}
