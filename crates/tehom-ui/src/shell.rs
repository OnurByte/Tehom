//! Window-level shell: the Ctrl+K / Cmd+K command palette and the first-run
//! guide. Both follow the same monochrome design system as the pages.
use super::*;
use crate::automation_ui::Action;
use gpui_kit::{App, FocusHandle, Focusable, actions, component::input::InputEvent};
use tehom_core::ExportProfile;

actions!(tehom, [TogglePalette, ClosePalette]);

/// The palette shortcut: Cmd+K on macOS, Ctrl+K elsewhere.
pub const PALETTE_KEY: &str = if cfg!(target_os = "macos") {
    "cmd-k"
} else {
    "ctrl-k"
};

/// Key bindings for the shell.
pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        gpui_kit::KeyBinding::new(PALETTE_KEY, TogglePalette, None),
        gpui_kit::KeyBinding::new("escape", ClosePalette, Some("CommandPalette")),
    ]);
}

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Command {
    Go(Page),
    AddLink,
    FromFile,
    OpenVideo,
    CheckSources,
    ChooseFolder,
    Shape(ExportProfile),
    Guide,
    CheckUpdates,
}

impl Command {
    pub(crate) const ALL: [(Command, &'static str); 15] = [
        (Command::AddLink, "Add a channel or video"),
        (Command::FromFile, "Make clips from a file"),
        (Command::OpenVideo, "Open a video to edit"),
        (Command::CheckSources, "Check sources for new videos"),
        (Command::Go(Page::Home), "Go to Home"),
        (Command::Go(Page::Sources), "Go to Sources"),
        (Command::Go(Page::Clips), "Go to Clips"),
        (Command::Go(Page::Settings), "Choose AI model or API key"),
        (Command::ChooseFolder, "Choose clip folder"),
        (
            Command::Shape(ExportProfile::Vertical),
            "Clip shape: vertical 1080 × 1920",
        ),
        (
            Command::Shape(ExportProfile::Horizontal),
            "Clip shape: horizontal 1920 × 1080",
        ),
        (
            Command::Shape(ExportProfile::Original),
            "Clip shape: original size",
        ),
        (Command::Guide, "Show the welcome guide"),
        (Command::CheckUpdates, "Check for updates"),
        (Command::Go(Page::Settings), "About Tehom and version"),
    ];

    /// Commands whose title contains every word of the query.
    pub(crate) fn matching(query: &str) -> Vec<(Command, &'static str)> {
        let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
        Self::ALL
            .into_iter()
            .filter(|(_, title)| {
                let title = title.to_lowercase();
                words.iter().all(|word| title.contains(word))
            })
            .collect()
    }
}

pub(crate) struct CommandPalette {
    query: Entity<InputState>,
    _events: Subscription,
}

impl Tehom {
    pub(crate) fn shell_focus(&self) -> &FocusHandle {
        &self.focus
    }

    pub(crate) fn toggle_palette(
        &mut self,
        _: &TogglePalette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.palette.take().is_some() {
            window.focus(&self.focus, cx);
        } else {
            let query = cx.new(|cx| InputState::new(window, cx).placeholder("Type a command"));
            let events = cx.subscribe_in(&query, window, |view, _, event, window, cx| {
                if let InputEvent::PressEnter { .. } = event {
                    view.run_first_command(window, cx);
                }
            });
            let handle = query.read(cx).focus_handle(cx);
            window.focus(&handle, cx);
            self.palette = Some(CommandPalette {
                query,
                _events: events,
            });
        }
        cx.notify();
    }

    pub(crate) fn close_palette(
        &mut self,
        _: &ClosePalette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.palette.take().is_some() {
            window.focus(&self.focus, cx);
            cx.notify();
        }
    }

    fn run_first_command(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let query = self
            .palette
            .as_ref()
            .map(|palette| palette.query.read(cx).value().to_string())
            .unwrap_or_default();
        if let Some((command, _)) = Command::matching(&query).into_iter().next() {
            self.run_command(command, window, cx);
        }
    }

    pub(crate) fn run_command(
        &mut self,
        command: Command,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.palette = None;
        window.focus(&self.focus, cx);
        match command {
            Command::Go(page) => self.navigate(page, cx),
            Command::AddLink => {
                self.navigate(Page::Home, cx);
                let handle = self.source_url.read(cx).focus_handle(cx);
                window.focus(&handle, cx);
            }
            Command::FromFile => self.choose_input_file(None, window, cx),
            Command::OpenVideo => self.pick(window, cx),
            Command::CheckSources => self.act(Action::Check, window, cx),
            Command::ChooseFolder => self.choose_export_directory(window, cx),
            Command::Shape(profile) => self.act(Action::ExportProfile(profile), window, cx),
            Command::Guide => self.onboarding = Some(0),
            Command::CheckUpdates => {
                self.navigate(Page::Settings, cx);
                self.check_for_updates(window, cx);
            }
        }
        cx.notify();
    }

    /// Opens the first-run guide once the saved state is known.
    pub(crate) fn check_onboarding(&mut self) {
        if !self.onboarding_checked
            && let Some(snapshot) = &self.engine_snapshot
        {
            self.onboarding_checked = true;
            if !snapshot.onboarded {
                self.onboarding = Some(0);
            }
        }
    }

    fn finish_onboarding(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.onboarding = None;
        self.navigate(Page::Home, cx);
        self.act(Action::Onboarded, window, cx);
    }

    /// The palette and the guide, drawn above the pages.
    pub(crate) fn overlay(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let p = crate::theme::palette(cx);
        if let Some(palette) = &self.palette {
            let query = palette.query.read(cx).value().to_string();
            let commands = Command::matching(&query);
            return Some(
                div()
                    .absolute()
                    .inset_0()
                    .bg(gpui_kit::Hsla::from(p.background).opacity(0.7))
                    .flex()
                    .justify_center()
                    .items_start()
                    .pt(px(96.))
                    .key_context("CommandPalette")
                    .on_action(cx.listener(Self::close_palette))
                    .child(
                        div()
                            .id("command-palette")
                            .w(px(560.))
                            .max_h(px(480.))
                            .bg(p.background)
                            .border_1()
                            .border_color(p.border)
                            .rounded(px(12.))
                            .p(px(12.))
                            .flex()
                            .flex_col()
                            .gap_2()
                            .child(Input::new(&palette.query).h(px(44.)).aria_label("Command"))
                            .when(commands.is_empty(), |list| {
                                list.child(
                                    div()
                                        .px(px(12.))
                                        .py(px(12.))
                                        .text_sm()
                                        .text_color(p.muted)
                                        .child("No matching command"),
                                )
                            })
                            .children(commands.into_iter().enumerate().map(
                                |(index, (command, title))| {
                                    div()
                                        .id(("command", index))
                                        .h(px(44.))
                                        .px(px(12.))
                                        .flex()
                                        .items_center()
                                        .justify_between()
                                        .rounded(px(8.))
                                        .cursor_pointer()
                                        .when(index == 0, |row| row.bg(p.surface))
                                        .hover(|row| row.bg(p.surface))
                                        .child(title)
                                        .when(index == 0, |row| {
                                            row.child(
                                                div().text_sm().text_color(p.muted).child("Enter"),
                                            )
                                        })
                                        .on_click(cx.listener(move |view, _, window, cx| {
                                            view.run_command(command, window, cx)
                                        }))
                                },
                            )),
                    )
                    .into_any_element(),
            );
        }
        let step = self.onboarding?;
        let snapshot = self.engine_snapshot.as_ref();
        // Ready once a remote provider is allowed or a local model is named.
        let ai_ready = snapshot.is_some_and(|s| {
            (s.ai.provider.is_remote() && s.ai.remote_consent) || !s.ai.model.trim().is_empty()
        });
        let (title, body) = match step {
            0 => (
                "Paste a link. Get clips as MP4 files.",
                "Tehom watches YouTube channels or single videos, picks the moments worth keeping and saves them as clips. It never posts anything for you.",
            ),
            1 => (
                "Choose who picks the clips",
                "Use an API key (OpenAI, Anthropic, Gemini, OpenRouter), your Codex or Claude Code subscription, or a model on this computer. You can also skip this and trim clips by hand.",
            ),
            _ => (
                "Where should clips go?",
                "Finished MP4 files are saved here. Existing files are never replaced.",
            ),
        };
        let folder = snapshot
            .map(|s| s.export_directory.display().to_string())
            .unwrap_or_default();
        Some(
            div()
                .id("onboarding")
                .absolute()
                .inset_0()
                .bg(p.background)
                .overflow_y_scroll()
                // A column whose child keeps its height, so short windows scroll
                // instead of clipping the buttons.
                .flex()
                .flex_col()
                .items_center()
                .child(
                    div()
                        .flex_shrink_0()
                        .w(px(640.))
                        .py(px(48.))
                        .flex()
                        .flex_col()
                        .gap(px(24.))
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
                                .child(div().text_2xl().child("Tehom")),
                        )
                        .child(
                            div()
                                .text_sm()
                                .text_color(p.muted)
                                .child(format!("Step {} of 3", step + 1)),
                        )
                        .child(div().text_3xl().child(title))
                        .child(div().text_color(p.muted).child(body))
                        .when(step == 1, |page| {
                            page.child(self.ai_card(cx)).when(ai_ready, |page| {
                                page.child(div().text_sm().child(format!(
                                    "Ready: {} ({}).",
                                    snapshot.map(|s| s.ai.provider.label()).unwrap_or_default(),
                                    snapshot
                                        .map(|s| s.ai.model.clone())
                                        .filter(|model| !model.is_empty())
                                        .unwrap_or_else(|| "default model".into())
                                )))
                            })
                        })
                        // The status bar is hidden behind the guide, so echo it here.
                        .when_some(self.guide_note.clone(), |page, note| {
                            page.child(div().text_sm().text_color(p.muted).child(note))
                        })
                        .when(step == 2, |page| {
                            page.child(
                                div()
                                    .p(px(16.))
                                    .border_1()
                                    .border_color(p.border)
                                    .rounded(px(12.))
                                    .child(folder),
                            )
                            .child(
                                Button::new("onboarding-folder")
                                    .h(px(44.))
                                    .label("Choose folder")
                                    .on_click(cx.listener(|view, _, window, cx| {
                                        view.choose_export_directory(window, cx)
                                    })),
                            )
                        })
                        .child(
                            div()
                                .flex()
                                .gap_2()
                                .child(
                                    Button::new("onboarding-next")
                                        .h(px(44.))
                                        .primary()
                                        .label(match step {
                                            0 => "Get started",
                                            1 if ai_ready => "Continue",
                                            1 => "Continue without AI",
                                            _ => "Finish",
                                        })
                                        .on_click(cx.listener(move |view, _, window, cx| {
                                            view.guide_note = None;
                                            if step >= 2 {
                                                view.finish_onboarding(window, cx);
                                            } else {
                                                view.onboarding = Some(step + 1);
                                                cx.notify();
                                            }
                                        })),
                                )
                                .when(step > 0, |row| {
                                    row.child(
                                        Button::new("onboarding-back")
                                            .h(px(44.))
                                            .label("Back")
                                            .on_click(cx.listener(move |view, _, _, cx| {
                                                view.guide_note = None;
                                                view.onboarding = Some(step - 1);
                                                cx.notify();
                                            })),
                                    )
                                })
                                .when(step == 0, |row| {
                                    row.child(
                                        Button::new("onboarding-skip")
                                            .h(px(44.))
                                            .ghost()
                                            .label("Skip setup")
                                            .on_click(cx.listener(|view, _, window, cx| {
                                                view.finish_onboarding(window, cx)
                                            })),
                                    )
                                }),
                        )
                        .child(div().text_sm().text_color(p.muted).child(
                            "Tip: press Ctrl+K (Cmd+K on macOS) anywhere to find any action.",
                        )),
                )
                .into_any_element(),
        )
    }
}
