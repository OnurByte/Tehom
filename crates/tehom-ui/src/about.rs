//! "About Tehom": version, updates, folders and links, plus the quiet
//! update check at start.
use super::*;
use crate::automation_ui::Action;
use tehom_automation::update::{self, UpdateStatus};

/// Where the update check stands for this session.
#[derive(Clone)]
pub(crate) enum UpdateState {
    Idle,
    Checking,
    Ready(UpdateStatus),
    Installing(String),
    Installed(String),
    Failed(String),
}

impl Tehom {
    /// Starts the update check once the saved preference is known.
    pub(crate) fn maybe_check_updates(&mut self, window: &Window, cx: &mut Context<Self>) {
        if self.update_checked {
            return;
        }
        let Some(snapshot) = &self.engine_snapshot else {
            return;
        };
        self.update_checked = true;
        if snapshot.check_updates {
            self.check_for_updates(window, cx);
        }
    }

    pub(crate) fn check_for_updates(&mut self, window: &Window, cx: &mut Context<Self>) {
        let Some(backend) = self.backend.clone() else {
            return;
        };
        if matches!(
            self.update,
            UpdateState::Checking | UpdateState::Installing(_)
        ) {
            return;
        }
        self.update = UpdateState::Checking;
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
                    Err(error) => UpdateState::Failed(error),
                };
                cx.notify();
            });
        })
        .detach();
    }

    fn install_update(&mut self, tag: String, window: &Window, cx: &mut Context<Self>) {
        let Some(backend) = self.backend.clone() else {
            return;
        };
        self.update = UpdateState::Installing(tag.clone());
        cx.notify();
        let target = tag.clone();
        let task = cx.background_spawn(async move {
            let folder = update::binary_dir().map_err(|error| error.to_string())?;
            backend
                .runtime
                .block_on(update::install(&target, &folder))
                .map_err(|error| error.to_string())
        });
        cx.spawn_in(window, async move |view, cx| {
            let result = task.await;
            let _ = view.update(cx, |view, cx| {
                view.update = match result {
                    Ok(()) => UpdateState::Installed(tag),
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
            UpdateState::Installing(tag) => (format!("Installing {tag}…"), None),
            UpdateState::Installed(tag) => (
                format!(
                    "Updated to {tag}. Restart Tehom to use it. `tehom update --rollback` undoes it."
                ),
                None,
            ),
            UpdateState::Failed(error) => (
                format!(
                    "The update did not finish: {}",
                    error.chars().take(160).collect::<String>()
                ),
                Some(("Try again", None)),
            ),
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
        let link = |id: &'static str, label: &'static str, url: String| {
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
            .child(div().text_sm().child(message))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_2()
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
                    .child(
                        Button::new("update-automatic")
                            .h(px(44.))
                            .label(if automatic {
                                "Check at start: on"
                            } else {
                                "Check at start: off"
                            })
                            .disabled(unavailable)
                            .on_click(cx.listener(move |view, _, window, cx| {
                                view.act(Action::CheckUpdates(!automatic), window, cx)
                            })),
                    )
                    .child(link("release-notes", "What's new", release_url)),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    .child(folder("open-clips", "Open clip folder", clips))
                    .child(folder("open-data", "Open data folder", data))
                    .child(link(
                        "project-page",
                        "Project page",
                        format!("https://github.com/{}", update::REPOSITORY),
                    ))
                    .child(link(
                        "report-problem",
                        "Report a problem",
                        format!("https://github.com/{}/issues/new", update::REPOSITORY),
                    )),
            )
            .child(
                div().text_sm().text_color(p.muted).child(
                    "AGPL-3.0-or-later ⁜ Built with GPUI Kit ⁜ Ctrl+K (Cmd+K) finds any action",
                ),
            )
            .into_any_element()
    }
}
