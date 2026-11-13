//! "About Tehom": what it is, version, licence, links, the author, credits,
//! updates and folders, plus the quiet update check while the app is open.
use super::*;
use crate::automation_ui::Action;
use tehom_automation::update::{self, UpdateStatus};

/// The author shown under "Made by"; every link here gets a button.
pub(crate) const AUTHOR: &str = "Onur Konuk";
pub(crate) const THANKS_URL: &str = "https://www.youtube.com/@UrasASMR";
pub(crate) const AUTHOR_LINKS: &[(&str, &str)] = &[
    ("GitHub", "https://github.com/OnurByte"),
    ("Portfolio", "https://onur.af"),
    ("X @onurstirner", "https://x.com/onurstirner"),
    ("X @onurbinary", "https://x.com/onurbinary"),
];

/// How often an open app looks for a new release.
const RECHECK: Duration = Duration::from_secs(6 * 60 * 60);

/// Where the update check stands for this session.
#[derive(Clone)]
pub(crate) enum UpdateState {
    Idle,
    Checking,
    Ready(UpdateStatus),
    Installing(String),
    /// The release and the folder it was installed into.
    Installed(String, PathBuf),
    Failed(String),
}

impl Tehom {
    /// Checks at start and then every six hours, if the user allows it.
    pub(crate) fn maybe_check_updates(&mut self, window: &Window, cx: &mut Context<Self>) {
        let Some(snapshot) = &self.engine_snapshot else {
            return;
        };
        if !snapshot.check_updates
            || self.backend.is_none()
            || self
                .update_checked_at
                .is_some_and(|at| at.elapsed() < RECHECK)
            || matches!(
                self.update,
                UpdateState::Checking | UpdateState::Installing(_) | UpdateState::Installed(..)
            )
        {
            return;
        }
        self.update_checked_at = Some(std::time::Instant::now());
        self.run_update_check(true, window, cx);
    }

    pub(crate) fn check_for_updates(&mut self, window: &Window, cx: &mut Context<Self>) {
        self.run_update_check(false, window, cx);
    }

    /// A `quiet` (automatic) check that fails keeps what was shown before and
    /// tries again at the next interval.
    fn run_update_check(&mut self, quiet: bool, window: &Window, cx: &mut Context<Self>) {
        let Some(backend) = self.backend.clone() else {
            return;
        };
        if matches!(
            self.update,
            UpdateState::Checking | UpdateState::Installing(_) | UpdateState::Installed(..)
        ) {
            return;
        }
        let before = std::mem::replace(&mut self.update, UpdateState::Checking);
        cx.notify();
        let task = cx.background_spawn(async move {
            backend
                .runtime
                .block_on(update::check())
                .map_err(|error| error.to_string())
        });
        cx.spawn_in(window, async move |view, cx| {
            let result = task.await;
            let _ = view.update(cx, |view, cx| {
                view.update = match result {
                    Ok(status) => UpdateState::Ready(status),
                    Err(error) if quiet => {
                        eprintln!("Update check: {error}");
                        before
                    }
                    Err(error) => UpdateState::Failed(error),
                };
                cx.notify();
            });
        })
        .detach();
    }

    pub(crate) fn install_update(&mut self, tag: String, window: &Window, cx: &mut Context<Self>) {
        let Some(backend) = self.backend.clone() else {
            return;
        };
        self.update = UpdateState::Installing(tag.clone());
        cx.notify();
        let target = tag.clone();
        let task = cx.background_spawn(async move {
            // Read before installing: the running program's path changes after.
            let folder = update::binary_dir().map_err(|error| error.to_string())?;
            backend
                .runtime
                .block_on(update::install(&target, &folder))
                .map_err(|error| format!("{error:#}"))?;
            Ok::<_, String>(folder)
        });
        cx.spawn_in(window, async move |view, cx| {
            let result = task.await;
            let _ = view.update(cx, |view, cx| {
                view.update = match result {
                    Ok(folder) => UpdateState::Installed(tag, folder),
                    Err(error) => UpdateState::Failed(error),
                };
                cx.notify();
            });
        })
        .detach();
    }

    /// A newer release, if the check found one.
    pub(crate) fn available_update(&self) -> Option<&UpdateStatus> {
        match &self.update {
            UpdateState::Ready(status) if status.available => Some(status),
            _ => None,
        }
    }

    /// The update button for the speech-tool fallback, when one is waiting.
    pub(crate) fn update_button(&self, cx: &mut Context<Self>) -> Option<Button> {
        let tag = self.available_update()?.latest.tag.clone();
        Some(
            Button::new("speech-update")
                .h(px(44.))
                .primary()
                .label(format!("Update to {tag}"))
                .disabled(self.backend.is_none())
                .on_click(cx.listener(move |view, _, window, cx| {
                    view.navigate(Page::About, cx);
                    view.install_update(tag.clone(), window, cx)
                })),
        )
    }

    pub(crate) fn about_card(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = crate::theme::palette(cx);
        let unavailable = self.backend.is_none();
        let automatic = self
            .engine_snapshot
            .as_ref()
            .is_none_or(|snapshot| snapshot.check_updates);
        let (message, action): (String, Option<(&'static str, Option<String>)>) = match &self.update
        {
            UpdateState::Idle => (
                "Updates have not been checked yet.".into(),
                Some(("Check for updates", None)),
            ),
            UpdateState::Checking => ("Checking for updates…".into(), None),
            UpdateState::Ready(status) if status.available => (
                format!(
                    "Tehom {} is available. You have {}.",
                    status.latest.tag, status.current
                ),
                Some(("Install update", Some(status.latest.tag.clone()))),
            ),
            UpdateState::Ready(status) => (
                format!("Tehom {} is up to date.", status.current),
                Some(("Check again", None)),
            ),
            UpdateState::Installing(tag) => (
                format!("Installing {tag}… Tehom keeps working meanwhile."),
                None,
            ),
            UpdateState::Installed(tag, _) => (
                format!(
                    "{tag} is installed. Restart Tehom to finish. If it does not open again by itself, start it as usual."
                ),
                None,
            ),
            UpdateState::Failed(error) => (
                format!(
                    "The update did not finish, and nothing was changed: {}",
                    error.chars().take(160).collect::<String>()
                ),
                Some(("Try again", None)),
            ),
        };
        let restart = match &self.update {
            UpdateState::Installed(_, folder) => {
                Some(folder.join(format!("tehom-desktop{}", std::env::consts::EXE_SUFFIX)))
            }
            _ => None,
        };
        let data = directories::ProjectDirs::from("org", "Tehom", "Tehom")
            .map(|dirs| dirs.data_local_dir().to_path_buf());
        let clips = self
            .engine_snapshot
            .as_ref()
            .map(|snapshot| snapshot.export_directory.clone());
        let release_url = match &self.update {
            UpdateState::Ready(status) if !status.latest.url.is_empty() => {
                status.latest.url.clone()
            }
            _ => format!("https://github.com/{}/releases", update::REPOSITORY),
        };
        let link = |id: (&'static str, usize), label: String, url: String| {
            Button::new(id)
                .h(px(44.))
                .label(label)
                .on_click(move |_, _, cx: &mut gpui_kit::App| cx.open_url(&url))
        };
        let folder = |id: &'static str, label: &'static str, path: Option<PathBuf>| {
            Button::new(id)
                .h(px(44.))
                .label(label)
                .disabled(path.is_none())
                .on_click(move |_, _, cx: &mut gpui_kit::App| {
                    if let Some(path) = &path {
                        cx.open_with_system(path);
                    }
                })
        };
        let heading = |text: &'static str| div().pt_2().text_lg().child(text);
        let row = || div().flex().flex_wrap().gap_2();
        div()
            .p(px(24.))
            .bg(p.background)
            .border_1()
            .border_color(p.border)
            .rounded(px(12.))
            .flex()
            .flex_col()
            .gap_4()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(
                        svg()
                            .path("brand/icon.svg")
                            .size(px(40.))
                            .text_color(p.text),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .child(div().text_lg().child("Tehom"))
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(p.muted)
                                    .child(format!("Version {}", update::CURRENT_VERSION)),
                            ),
                    ),
            )
            .child(div().child(
                "Tehom turns long videos into captioned vertical clips on your own computer: paste a link, get clips.",
            ))
            .child(heading("Updates"))
            .child(div().text_sm().child(message))
            .child(
                row()
                    .when_some(action, |row, (label, tag)| {
                        row.child(
                            Button::new("update-action")
                                .h(px(44.))
                                .primary()
                                .label(label)
                                .disabled(unavailable)
                                .on_click(cx.listener(move |view, _, window, cx| match &tag {
                                    Some(tag) => view.install_update(tag.clone(), window, cx),
                                    None => view.check_for_updates(window, cx),
                                })),
                        )
                    })
                    .when_some(restart, |row, path| {
                        row.child(
                            Button::new("restart-tehom")
                                .h(px(44.))
                                .primary()
                                .label("Restart Tehom")
                                .on_click(move |_, _, cx: &mut gpui_kit::App| {
                                    cx.set_restart_path(path.clone());
                                    cx.restart();
                                }),
                        )
                    })
                    .child(
                        Button::new("update-automatic")
                            .h(px(44.))
                            .label(if automatic {
                                "Check automatically: on"
                            } else {
                                "Check automatically: off"
                            })
                            .disabled(unavailable)
                            .on_click(cx.listener(move |view, _, window, cx| {
                                view.act(Action::CheckUpdates(!automatic), window, cx)
                            })),
                    )
                    .child(link(("release-notes", 0), "What's new".into(), release_url)),
            )
            .child(heading("Folders"))
            .child(
                row()
                    .child(folder("open-clips", "Open clip folder", clips))
                    .child(folder("open-data", "Open data folder", data)),
            )
            .child(heading("Links"))
            .child(
                row()
                    .child(link(
                        ("project-page", 0),
                        "Source code".into(),
                        format!("https://github.com/{}", update::REPOSITORY),
                    ))
                    .child(link(
                        ("report-problem", 0),
                        "Report a problem".into(),
                        format!("https://github.com/{}/issues/new", update::REPOSITORY),
                    ))
                    .child(link(
                        ("all-releases", 0),
                        "All releases".into(),
                        format!("https://github.com/{}/releases", update::REPOSITORY),
                    )),
            )
            .child(heading("Made by"))
            .child(div().id("author").test_support().aria_label(AUTHOR).child(AUTHOR))
            .child(row().children(AUTHOR_LINKS.iter().enumerate().map(
                |(index, (label, url))| link(("author-link", index), (*label).into(), (*url).into()),
            )))
            .child(heading("Special thanks"))
            .child(row().child(link(
                ("thanks-link", 0),
                "Uras ASMR on YouTube".into(),
                THANKS_URL.into(),
            )))
            .child(heading("Licence and credits"))
            .child(div().text_sm().child(
                "Free software under the GNU AGPL 3.0 or later.",
            ))
            .child(div().text_sm().text_color(p.muted).child(
                "Built with GPUI Kit, FFmpeg, yt-dlp and whisper.cpp; Instagram posting ports the aiograpi protocol; TikTok upload steps follow TiktokAutoUploader and YouTube Shorts steps follow youtube-studio; Bluesky uses the AT Protocol and Facebook Reels the Graph API; platform icons from Simple Icons.",
            ))
            .child(div().text_sm().text_color(p.muted).child(
                "Tip: Ctrl+K (Cmd+K on macOS) finds any action.",
            ))
            .into_any_element()
    }
}
