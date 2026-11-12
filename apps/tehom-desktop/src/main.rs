// A GUI program on Windows: no console window opens beside it.
#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

use gpui_kit::*;

/// Ties windows, taskbar button, Start-menu shortcut (install.ps1) and Linux
/// desktop entry (org.tehom.Tehom.desktop) to one application.
const APP_ID: &str = "org.tehom.Tehom";

/// Packaged Windows builds carry GStreamer beside the executable. Pointing it
/// there keeps plug-ins of another GStreamer on the computer from mixing in.
#[cfg(windows)]
fn use_bundled_gstreamer() {
    let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|exe| Some(exe.parent()?.to_path_buf()))
    else {
        return;
    };
    let scanner = dir.join("gst-plugin-scanner.exe");
    if !scanner.is_file() {
        return; // A development build uses the installed GStreamer.
    }
    // SAFETY: runs first in main, before any other thread exists.
    unsafe {
        std::env::set_var("GST_PLUGIN_SCANNER_1_0", scanner);
        std::env::set_var("GST_PLUGIN_SYSTEM_PATH_1_0", dir.join("gstreamer-plugins"));
        std::env::remove_var("GST_PLUGIN_PATH_1_0");
        std::env::remove_var("GST_PLUGIN_PATH");
    }
}

fn main() {
    #[cfg(windows)]
    use_bundled_gstreamer();
    let mut args = std::env::args_os().skip(1);
    let first = args.next();
    // The release workflow proves with this that a packed build decodes video.
    if first.as_deref() == Some(std::ffi::OsStr::new("--check-preview")) {
        // A GUI-subsystem program has no console; borrow the caller's so the
        // error below is visible when run from cmd or PowerShell.
        #[cfg(windows)]
        {
            unsafe extern "system" {
                fn AttachConsole(process: u32) -> i32;
            }
            // SAFETY: plain Win32 call; ATTACH_PARENT_PROCESS = u32::MAX.
            unsafe { AttachConsole(u32::MAX) };
        }
        let path = args
            .next()
            .map(std::path::PathBuf::from)
            .unwrap_or_default();
        if let Err(error) = tehom_media::preview::Preview::open(&path) {
            eprintln!("Tehom preview check failed: {error}");
            std::process::exit(1);
        }
        return;
    }
    let initial_path = first.map(std::path::PathBuf::from);
    application().with_assets(tehom_ui::Assets).run(move |cx| {
        // Sets the Windows AppUserModelID; must precede the first window.
        cx.set_app_identity(APP_ID, "Tehom");
        if let Err(error) = cx.text_system().add_fonts(vec![
            std::borrow::Cow::Borrowed(include_bytes!("../../../assets/fonts/Geist-Regular.ttf")),
            std::borrow::Cow::Borrowed(include_bytes!(
                "../../../assets/fonts/GeistMono-Regular.ttf"
            )),
            std::borrow::Cow::Borrowed(include_bytes!(
                "../../../assets/fonts/Staatliches-Regular.ttf"
            )),
            std::borrow::Cow::Borrowed(include_bytes!("../../../assets/fonts/Anton-Regular.ttf")),
            std::borrow::Cow::Borrowed(include_bytes!(
                "../../../assets/fonts/Righteous-Regular.ttf"
            )),
            std::borrow::Cow::Borrowed(include_bytes!(
                "../../../assets/fonts/BebasNeue-Regular.ttf"
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
            app_id: Some(APP_ID.into()),
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
