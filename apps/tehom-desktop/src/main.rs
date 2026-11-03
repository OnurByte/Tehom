use gpui_kit::{
    component::{Theme, ThemeMode},
    *,
};

fn main() {
    let initial_path = std::env::args_os().nth(1).map(std::path::PathBuf::from);
    application().with_assets(tehom_ui::Assets).run(move |cx| {
        if let Err(error) = cx.text_system().add_fonts(vec![
            std::borrow::Cow::Borrowed(include_bytes!("../../../assets/fonts/Geist-Regular.ttf")),
            std::borrow::Cow::Borrowed(include_bytes!(
                "../../../assets/fonts/GeistMono-Regular.ttf"
            )),
        ]) {
            eprintln!("Tehom fonts: {error}");
        }
        init(cx);
        Theme::change(ThemeMode::Dark, None, cx);
        let theme = Theme::global_mut(cx);
        theme.font_family = "Geist".into();
        theme.mono_font_family = "Geist Mono".into();
        theme.colors.button_primary = rgb(0xbe95ff).into();
        theme.colors.button_primary_foreground = rgb(0x161616).into();
        theme.colors.background = rgb(0x161616).into();
        theme.colors.foreground = rgb(0xf2f4f8).into();
        theme.colors.primary = rgb(0xbe95ff).into();
        theme.colors.primary_foreground = rgb(0x161616).into();
        theme.colors.ring = rgb(0x78a9ff).into();
        theme.tokens.button_primary = theme.colors.button_primary.into();
        theme.tokens.button_primary_hover = gpui_kit::Hsla::from(rgb(0xd0b0ff)).into();
        theme.tokens.button_primary_active = gpui_kit::Hsla::from(rgb(0xa675ec)).into();
        theme.colors.sidebar = rgb(0x1c1c1c).into();
        theme.colors.sidebar_accent = rgb(0x30283d).into();
        theme.colors.sidebar_accent_foreground = rgb(0xbe95ff).into();
        theme.colors.sidebar_foreground = rgb(0xdde1e6).into();
        theme.colors.sidebar_border = rgb(0x393939).into();
        Theme::sync_base(cx);
        let options = WindowOptions {
            titlebar: Some(TitlebarOptions {
                title: Some("Tehom".into()),
                ..Default::default()
            }),
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                None,
                size(px(1100.), px(780.)),
                cx,
            ))),
            ..Default::default()
        };
        open_window(options, cx, move |window, cx| {
            let view = cx.new(|cx| tehom_ui::Tehom::new(window, cx));
            view.update(cx, |view, cx| view.start_automation(window, cx));
            if let Some(path) = initial_path {
                view.update(cx, |view, cx| view.load(path, window, cx));
            }
            view
        })
        .expect("Tehom penceresi açılamadı");
        cx.activate(true);
    });
}
