use gpui_kit::*;

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
        // The brand follows the system light/dark preference.
        let dark = matches!(
            cx.window_appearance(),
            WindowAppearance::Dark | WindowAppearance::VibrantDark
        );
        tehom_ui::theme::apply(dark, cx);
        tehom_ui::bind_keys(cx);
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
            window
                .observe_window_appearance(|window, cx| {
                    let dark = matches!(
                        window.appearance(),
                        WindowAppearance::Dark | WindowAppearance::VibrantDark
                    );
                    tehom_ui::theme::apply(dark, cx);
                    window.refresh();
                })
                .detach();
            let view = cx.new(|cx| tehom_ui::Tehom::new(window, cx));
            view.update(cx, |view, cx| view.start_automation(window, cx));
            let handle = view.read(cx).focus_handle();
            window.focus(&handle, cx);
            if let Some(path) = initial_path {
                view.update(cx, |view, cx| view.load(path, window, cx));
            }
            view
        })
        .expect("The Tehom window could not open");
        cx.activate(true);
    });
}
