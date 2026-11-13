//! Window-level shell: the Ctrl+K / Cmd+K search palette and the first-run
//! guide.
use super::*;
use crate::automation_ui::Action;
use gpui_kit::assets::IconName;
use gpui_kit::component::command::{Command as PaletteList, CommandItem, CommandState};
use gpui_kit::{App, FocusHandle, Focusable, MouseButton, actions, relative};
use tehom_core::{Accent, Appearance, ExportProfile};

actions!(tehom, [TogglePalette, ClosePalette]);

/// The palette shortcut: Cmd+K on macOS, Ctrl+K elsewhere.
pub const PALETTE_KEY: &str = if cfg!(target_os = "macos") {
    "cmd-k"
} else {
    "ctrl-k"
};

/// How many steps the first-run guide has.
const GUIDE_STEPS: u8 = 6;

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
    ConnectInstagram,
    /// Settings, scrolled to a card (`automation_ui::CARD_*`).
    Card(usize),
    Look(Appearance),
    Accent(Accent),
    /// Widen (positive) or narrow the sidebar by this many pixels.
    Sidebar(i16),
}

impl Command {
    /// Every palette entry, in the order shown.
    pub(crate) const ALL: &[(Command, &str, IconName)] = &[
        (Command::AddLink, "Add a channel or video", IconName::Plus),
        (
            Command::FromFile,
            "Make clips from a file",
            IconName::Upload,
        ),
        (
            Command::OpenVideo,
            "Open a video in the Studio",
            IconName::FolderOpen,
        ),
        (
            Command::CheckSources,
            "Check sources for new videos",
            IconName::Rss,
        ),
        (Command::Go(Page::Home), "Go to Home", IconName::House),
        (Command::Go(Page::Clips), "Go to Clips", IconName::Scissors),
        (
            Command::Go(Page::Studio),
            "Go to Studio",
            IconName::Clapperboard,
        ),
        (
            Command::Go(Page::Automations),
            "Go to Automations",
            IconName::Zap,
        ),
        (
            Command::Go(Page::Connections),
            "Go to Connections",
            IconName::Plug,
        ),
        (
            Command::Go(Page::Settings),
            "Go to Settings",
            IconName::Settings,
        ),
        (
            Command::ConnectInstagram,
            "Connect Instagram",
            IconName::Plug,
        ),
        (
            Command::Card(crate::automation_ui::CARD_AI),
            "Choose AI model or API key",
            IconName::Sparkles,
        ),
        (
            Command::ChooseFolder,
            "Choose clip folder",
            IconName::FolderOpen,
        ),
        (
            Command::Shape(ExportProfile::Vertical),
            "Clip shape: vertical 1080 × 1920",
            IconName::Video,
        ),
        (
            Command::Shape(ExportProfile::Horizontal),
            "Clip shape: horizontal 1920 × 1080",
            IconName::Video,
        ),
        (
            Command::Shape(ExportProfile::Original),
            "Clip shape: original size",
            IconName::Video,
        ),
        (
            Command::Card(crate::automation_ui::CARD_LOOK),
            "Caption look for new clips",
            IconName::Wand,
        ),
        (
            Command::Card(crate::automation_ui::CARD_SPEECH),
            "Speech model for videos without captions",
            IconName::Wand,
        ),
        (
            Command::Look(Appearance::Light),
            "Appearance: light",
            IconName::Palette,
        ),
        (
            Command::Look(Appearance::Dark),
            "Appearance: dark",
            IconName::Palette,
        ),
        (
            Command::Look(Appearance::System),
            "Appearance: same as this computer",
            IconName::Palette,
        ),
        (
            Command::Accent(Accent::Violet),
            "Colour: violet",
            IconName::Palette,
        ),
        (
            Command::Accent(Accent::Blue),
            "Colour: blue",
            IconName::Palette,
        ),
        (
            Command::Accent(Accent::Green),
            "Colour: green",
            IconName::Palette,
        ),
        (
            Command::Accent(Accent::Orange),
            "Colour: orange",
            IconName::Palette,
        ),
        (
            Command::Accent(Accent::Pink),
            "Colour: pink",
            IconName::Palette,
        ),
        (
            Command::Accent(Accent::Mono),
            "Colour: black and white",
            IconName::Palette,
        ),
        (
            Command::Sidebar(40),
            "Make the sidebar wider",
            IconName::Settings,
        ),
        (
            Command::Sidebar(-40),
            "Make the sidebar narrower",
            IconName::Settings,
        ),
        (Command::Guide, "Show the welcome guide", IconName::Info),
        (Command::CheckUpdates, "Check for updates", IconName::Info),
        (Command::Go(Page::About), "About Tehom", IconName::Info),
        (
            Command::Go(Page::Automations),
            "Posting rules for each source",
            IconName::Zap,
        ),
        (
            Command::Go(Page::Automations),
            "How often Tehom checks for videos",
            IconName::Zap,
        ),
        (
            Command::Go(Page::Connections),
            "Accounts Tehom can post to",
            IconName::Plug,
        ),
    ];
}

impl Command {
    /// Other words people search with, besides the title.
    fn keywords(self) -> &'static str {
        match self {
            Command::ConnectInstagram | Command::Go(Page::Connections) => {
                "post share reels account sign in login tiktok youtube shorts"
            }
            Command::Go(Page::Automations) => "auto post schedule source channel rules",
            Command::Go(Page::Studio) | Command::OpenVideo => {
                "edit editor trim cut captions subtitles timeline"
            }
            Command::Go(Page::Clips) => "watch play videos library",
            Command::Look(_) | Command::Accent(_) => "theme color palette",
            Command::Card(_) => "settings subtitles",
            _ => "",
        }
    }
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
            let state = cx.new(|cx| CommandState::new(window, cx));
            state.update(cx, |state, cx| state.focus(window, cx));
            self.palette = Some(state);
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
            Command::AddLink => self.focus_link_field(window, cx),
            Command::FromFile => self.choose_input_file(None, window, cx),
            Command::OpenVideo => self.pick(window, cx),
            Command::CheckSources => self.act(Action::Check, window, cx),
            Command::ChooseFolder => self.choose_export_directory(window, cx),
            Command::Shape(profile) => self.act(Action::ExportProfile(profile), window, cx),
            Command::Guide => self.onboarding = Some(0),
            Command::ConnectInstagram => self.connect_instagram(window, cx),
            Command::Card(card) => self.open_settings_at(card, cx),
            Command::Look(appearance) => self.choose_appearance(appearance, window, cx),
            Command::Accent(accent) => self.choose_accent(accent, window, cx),
            Command::Sidebar(change) => {
                self.resize_sidebar(self.sidebar_px() + f32::from(change), true, cx)
            }
            Command::CheckUpdates => {
                self.navigate(Page::About, cx);
                self.check_for_updates(window, cx);
            }
        }
        cx.notify();
    }

    /// Home, with the caret in the link field.
    pub(crate) fn focus_link_field(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.navigate(Page::Home, cx);
        let handle = self.source_url.read(cx).focus_handle(cx);
        window.focus(&handle, cx);
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
        if let Some(state) = &self.palette {
            let view = cx.entity().downgrade();
            let cancel = view.clone();
            return Some(
                div()
                    .id("palette-backdrop")
                    .absolute()
                    .inset_0()
                    .bg(gpui_kit::Hsla::from(p.background).opacity(0.7))
                    .flex()
                    .justify_center()
                    .items_start()
                    .pt(px(96.))
                    .key_context("CommandPalette")
                    .on_action(cx.listener(Self::close_palette))
                    // A click beside the box closes it; one inside does not.
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|view, _, window, cx| {
                            view.close_palette(&ClosePalette, window, cx)
                        }),
                    )
                    .child(
                        div()
                            .id("command-palette")
                            .w(px(560.))
                            .max_w(relative(0.9))
                            .shadow_lg()
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .child(
                                PaletteList::new(state)
                                    .placeholder("Search for anything…")
                                    .max_h(px(380.))
                                    .items(Command::ALL.iter().map(|(command, title, icon)| {
                                        CommandItem::new()
                                            .label(*title)
                                            .icon(*icon)
                                            .keywords([command.keywords()])
                                    }))
                                    .on_confirm(move |index, window, cx| {
                                        if let Some((command, ..)) = Command::ALL.get(index.row) {
                                            let _ = view.update(cx, |view, cx| {
                                                view.run_command(*command, window, cx)
                                            });
                                        }
                                    })
                                    .on_cancel(move |window, cx| {
                                        let _ = cancel.update(cx, |view, cx| {
                                            view.close_palette(&ClosePalette, window, cx)
                                        });
                                    })
                                    .empty(|_, _, _| {
                                        div().p_4().text_sm().child("Nothing matches. Try another word.")
                                    })
                                    .footer(move |_, _, _| {
                                        div()
                                            .px_3()
                                            .py_2()
                                            .border_t_1()
                                            .border_color(p.border)
                                            .text_xs()
                                            .text_color(p.muted)
                                            .child("Arrow keys to move · Enter to choose · Esc to close")
                                    }),
                            ),
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
        let speech_ready = self
            .speech
            .as_ref()
            .is_some_and(|s| s.program.is_some() && s.model.is_some());
        let speech_missing = self.speech.as_ref().is_some_and(|s| s.program.is_none());
        let connected = !self.accounts_on(tehom_core::Platform::Instagram).is_empty();
        let has_source = snapshot.is_some_and(|s| !s.sources.is_empty());
        let (title, body) = match step {
            0 => (
                "Turn long videos into short clips",
                "Tehom watches YouTube and Kick channels, finds the moments worth keeping, adds captions and saves them as vertical clips. Nothing is posted unless you turn that on.",
            ),
            1 => (
                "Choose who picks the clips",
                "An AI reads each video's captions and picks the best moments. Use an API key (OpenAI, Anthropic, Gemini, OpenRouter), your Codex or Claude Code subscription, or a model on this computer. You can skip this and trim clips by hand.",
            ),
            2 => (
                "Make captions on this computer",
                "Recommended. When a video has no captions, Tehom writes them here; nothing is sent anywhere. This downloads about 150 MB once. You can do it later in Settings.",
            ),
            3 => (
                "Where should clips go?",
                "Finished MP4 files are saved here. Existing files are never replaced.",
            ),
            4 => (
                "Post clips to Instagram (optional)",
                "Connect an account and Tehom can post finished clips as Reels, by hand or automatically. You can do this later in Connections.",
            ),
            _ => (
                "Add your first channel or video",
                "Paste a YouTube or Kick link. Tehom starts on the newest video right away and you can follow it on Home.",
            ),
        };
        let folder = snapshot
            .map(|s| s.export_directory.display().to_string())
            .unwrap_or_default();
        let how = |number: &'static str, title: &'static str, text: &'static str| {
            div()
                .flex_1()
                .min_w_0()
                .p_4()
                .bg(p.surface)
                .rounded(px(12.))
                .flex()
                .flex_col()
                .gap_2()
                .child(
                    div()
                        .size(px(32.))
                        .rounded_full()
                        .bg(p.accent)
                        .text_color(p.on_accent)
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(number),
                )
                .child(div().child(title))
                .child(div().text_sm().text_color(p.muted).child(text))
        };
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
                        .w(px(680.))
                        .max_w(relative(0.94))
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
                                .flex()
                                .items_center()
                                .gap_2()
                                .children((0..GUIDE_STEPS).map(|index| {
                                    div()
                                        .h(px(6.))
                                        .flex_1()
                                        .rounded_full()
                                        .bg(if index <= step { p.accent } else { p.border })
                                }))
                                .child(
                                    div()
                                        .pl_2()
                                        .text_sm()
                                        .text_color(p.muted)
                                        .child(format!("Step {} of {GUIDE_STEPS}", step + 1)),
                                ),
                        )
                        .child(div().text_3xl().child(title))
                        .child(div().text_color(p.muted).child(body))
                        .when(step == 0, |page| {
                            page.child(
                                div()
                                    .flex()
                                    .gap_3()
                                    .child(how("1", "Paste a link", "A channel, one video or a file on this computer."))
                                    .child(how("2", "Tehom makes clips", "It picks the moments, crops them tall and adds captions."))
                                    .child(how("3", "Watch, edit, post", "Play clips here, fix them in the Studio, post to Instagram.")),
                            )
                            .child(div().child("Pick your colour"))
                            .child(self.accent_row(cx))
                        })
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
                        .when(step == 2, |page| {
                            page.when(speech_ready, |page| {
                                page.child(div().text_sm().child("Captions on this computer are ready."))
                            })
                            .when(speech_missing, |page| {
                                page.child(div().text_sm().child(
                                    "This copy of Tehom has no built-in speech tool yet. Update Tehom later from About Tehom.",
                                ))
                            })
                            .when(!speech_ready && !speech_missing, |page| {
                                page.child(
                                    Button::new("onboarding-speech")
                                        .h(px(44.))
                                        .primary()
                                        .label(self.speech_setup_label())
                                        .disabled(self.backend.is_none() || self.speech_downloading || self.source_busy)
                                        .on_click(cx.listener(|view, _, window, cx| {
                                            view.set_up_speech(window, cx)
                                        })),
                                )
                            })
                        })
                        .when(step == 3, |page| {
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
                        .when(step == 4, |page| page.child(self.instagram_card(cx)))
                        .when(step == 5, |page| page.child(self.source_form(cx)))
                        // The status bar is hidden behind the guide, so echo it here.
                        .when_some(self.guide_note.clone(), |page, note| {
                            page.child(div().text_sm().text_color(p.muted).child(note))
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
                                            2 if speech_ready => "Continue",
                                            4 if connected => "Continue",
                                            2 | 4 => "Skip for now",
                                            3 => "Continue",
                                            _ if has_source => "Finish",
                                            _ => "Finish without a link",
                                        })
                                        .on_click(cx.listener(move |view, _, window, cx| {
                                            view.guide_note = None;
                                            if step + 1 >= GUIDE_STEPS {
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
                            "Tip: press Ctrl+K (Cmd+K on macOS) anywhere to search for any action.",
                        )),
                )
                .into_any_element(),
        )
    }
}
