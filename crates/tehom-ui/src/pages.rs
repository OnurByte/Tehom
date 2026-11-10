//! The pages behind the sidebar: Home, Clips, Automations, Connections and
//! the Studio's start screen.
use super::*;
use crate::automation_ui::{Action, platform_badge};
use gpui_kit::assets::IconName;
use gpui_kit::{App, Stateful, gpui::Div};
use tehom_core::Platform;

/// A scrolling page with its title and one line saying what it is for.
pub(crate) fn page(
    id: &'static str,
    title: &'static str,
    hint: &'static str,
    cx: &App,
) -> Stateful<Div> {
    let p = crate::theme::palette(cx);
    div()
        .id(id)
        .flex_1()
        .min_w_0()
        .h_full()
        .overflow_y_scroll()
        .px(px(40.))
        .py(px(32.))
        .flex()
        .flex_col()
        .gap_4()
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(div().text_3xl().child(title))
                .child(div().text_color(p.muted).child(hint)),
        )
}

impl Tehom {
    fn check_button(&self, cx: &mut Context<Self>) -> Button {
        Button::new("check-sources")
            .h(px(44.))
            .icon(IconName::Rss)
            .label("Check for new videos")
            .disabled(self.backend.is_none() || self.source_busy)
            .on_click(cx.listener(|view, _, window, cx| view.act(Action::Check, window, cx)))
    }

    pub(crate) fn home_page(&self, cx: &mut Context<Self>) -> AnyElement {
        page(
            "home-scroll",
            "Make clips from a video",
            "Paste a link. Tehom finds the best moments, crops them tall and adds captions.",
            cx,
        )
        .child(self.source_form(cx))
        .child(
            div()
                .pt_4()
                .flex()
                .items_center()
                .justify_between()
                .child(div().text_lg().child("What Tehom is working on"))
                .child(self.check_button(cx)),
        )
        .child(self.job_list(cx))
        .into_any_element()
    }

    /// One card per clip: watch it, edit it, post it.
    fn clip_cards(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let p = crate::theme::palette(cx);
        let Some(snapshot) = &self.engine_snapshot else {
            return Vec::new();
        };
        snapshot
            .clips
            .iter()
            .map(|clip| {
                let id = clip.id;
                let key = id as usize;
                let path = clip.output_path.clone().filter(|_| clip.output_available);
                let export_missing = clip.output_path.is_some() && !clip.output_available;
                let video = snapshot.videos.iter().find(|video| video.id == clip.video_id);
                let original_missing =
                    video.is_some_and(|video| video.local_path.is_some() && !video.local_available);
                let original = video
                    .filter(|video| video.local_available)
                    .and_then(|video| video.local_path.clone());
                let range = (clip.start_ms, clip.end_ms);
                let busy = self.busy || self.source_busy;
                crate::theme::card(cx)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_3()
                            .child(div().flex_1().min_w_0().text_lg().child(clip.title.clone()))
                            .child(
                                div()
                                    .px_2()
                                    .py_1()
                                    .rounded(px(6.))
                                    .bg(p.surface)
                                    .text_sm()
                                    .child(format!(
                                        "{} long",
                                        crate::studio::clock(clip.end_ms - clip.start_ms)
                                    )),
                            ),
                    )
                    .child(div().text_sm().text_color(p.muted).child(format!(
                        "{}From {} to {} of the video{}",
                        video.map(|v| format!("{} · ", v.title)).unwrap_or_default(),
                        crate::studio::clock(clip.start_ms),
                        crate::studio::clock(clip.end_ms),
                        if path.is_some() { "" } else { " · Draft, not saved as MP4 yet" },
                    )))
                    .when(original_missing, |v| {
                        v.child(div().text_sm().child(
                            "The original video is missing, so this clip cannot be edited. Put the file back and try again.",
                        ))
                    })
                    .when(export_missing, |v| {
                        v.child(div().text_sm().child("The MP4 file was moved or deleted."))
                    })
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .gap_2()
                            .when_some(path.clone(), |row, path| {
                                row.child(
                                    Button::new(("clip-watch", key))
                                        .h(px(44.))
                                        .primary()
                                        .icon(IconName::Play)
                                        .label("Watch")
                                        .disabled(self.busy)
                                        .on_click(cx.listener(move |view, _, window, cx| {
                                            view.open_video(path.clone(), None, None, true, window, cx)
                                        })),
                                )
                            })
                            .when_some(original, |row, original| {
                                row.child(
                                    Button::new(("clip-edit", key))
                                        .h(px(44.))
                                        .icon(IconName::Clapperboard)
                                        .label("Edit in Studio")
                                        .disabled(busy)
                                        .on_click(cx.listener(move |view, _, window, cx| {
                                            view.open_video(
                                                original.clone(),
                                                Some(range),
                                                Some(id),
                                                false,
                                                window,
                                                cx,
                                            )
                                        })),
                                )
                            }),
                    )
                    .when(path.is_some(), |card| card.child(self.publish_row(clip, cx)))
                    .into_any_element()
            })
            .collect()
    }

    /// Shown where clips would be, with the way to get some.
    fn no_clips(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = crate::theme::palette(cx);
        crate::theme::card(cx)
            .items_center()
            .py(px(40.))
            .child(IconName::Scissors)
            .child(div().text_lg().child("No clips yet"))
            .child(
                div()
                    .text_sm()
                    .text_color(p.muted)
                    .child("Paste a link on Home and your clips show up here."),
            )
            .child(
                Button::new("no-clips-add")
                    .h(px(44.))
                    .primary()
                    .label("Add a link")
                    .on_click(cx.listener(|view, _, window, cx| view.focus_link_field(window, cx))),
            )
            .into_any_element()
    }

    pub(crate) fn clips_page(&self, cx: &mut Context<Self>) -> AnyElement {
        let cards = self.clip_cards(cx);
        page(
            "clips-scroll",
            "Clips",
            "Watch your clips, fix them in the Studio and post them.",
            cx,
        )
        .when(cards.is_empty(), |v| v.child(self.no_clips(cx)))
        .children(cards)
        .into_any_element()
    }

    /// The Studio before a video is open.
    pub(crate) fn studio_start(&self, cx: &mut Context<Self>) -> AnyElement {
        let cards = self.clip_cards(cx);
        page(
            "studio-scroll",
            "Studio",
            "Trim a clip, fix its captions and change how they look. Pick a clip or open any video.",
            cx,
        )
        .child(
            div().flex().child(
                Button::new("open")
                    .h(px(44.))
                    .icon(IconName::FolderOpen)
                    .label("Open a video file")
                    .disabled(self.busy)
                    .on_click(cx.listener(|view, _, window, cx| view.pick(window, cx))),
            ),
        )
        .when(cards.is_empty(), |v| v.child(self.no_clips(cx)))
        .children(cards)
        .into_any_element()
    }

    pub(crate) fn automations_page(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = crate::theme::palette(cx);
        let unavailable = self.backend.is_none() || self.source_busy;
        let sources = self
            .engine_snapshot
            .as_ref()
            .map(|s| s.sources.clone())
            .unwrap_or_default();
        let accounts = self.instagram_accounts();
        let default_account = self
            .engine_snapshot
            .as_ref()
            .and_then(|s| s.auto_publish_account)
            .and_then(|id| accounts.iter().find(|a| a.id == id))
            .map(|a| format!("@{}", a.username));
        let choice = |id: (&'static str, usize), label: String, selected: bool| {
            Button::new(id)
                .h(px(44.))
                .label(label)
                .when(selected, |button| button.primary())
                .toggled(selected)
                .disabled(unavailable)
        };
        let cards = sources.into_iter().map(|source| {
            let id = source.id;
            let key = id as usize;
            let paused = source.mode == AutomationMode::Paused;
            let route = source.publish_account;
            let target = match route {
                Some(0) => None,
                Some(account) => accounts
                    .iter()
                    .find(|a| a.id == account)
                    .map(|a| format!("@{}", a.username)),
                None => default_account.clone(),
            };
            let summary = match source.mode {
                AutomationMode::Paused => {
                    "Paused. Nothing happens until you resume it.".to_string()
                }
                AutomationMode::Manual => "New video → listed on Home, waiting for you".to_string(),
                AutomationMode::AutoDraft => {
                    "New video → draft clips to review in the Studio".to_string()
                }
                AutomationMode::AutoExport => match &target {
                    Some(account) => {
                        format!("New video → finished MP4 clips → posted to {account}")
                    }
                    None => "New video → finished MP4 clips in your folder".to_string(),
                },
            };
            let title = source.title.clone();
            crate::theme::card(cx)
                .children(platform_badge(&source))
                .child(div().text_lg().child(source.title.clone()))
                .child(div().text_sm().text_color(p.muted).child(format!(
                    "{} · {}",
                    if source.channel_id.starts_with("local:") {
                        "File on this computer"
                    } else if source.channel_id.starts_with("video:") {
                        "Single video"
                    } else if source.channel_id.starts_with("feed:") {
                        "RSS feed, followed for new videos"
                    } else {
                        "Channel, followed for new videos"
                    },
                    source.url
                )))
                .child(
                    div()
                        .id(("automation-summary", key))
                        .test_support()
                        .aria_label(summary.clone())
                        .p_3()
                        .rounded(px(8.))
                        .bg(p.surface)
                        .child(summary),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(p.muted)
                        .child("When a new video arrives"),
                )
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap_2()
                        .children(
                            [
                                (AutomationMode::Manual, "Only list it"),
                                (AutomationMode::AutoDraft, "Make drafts"),
                                (AutomationMode::AutoExport, "Save MP4 files"),
                            ]
                            .into_iter()
                            .enumerate()
                            .map(|(index, (mode, label))| {
                                choice(
                                    ("source-mode", key * 4 + index),
                                    label.into(),
                                    source.mode == mode,
                                )
                                .accessibility_label(format!("{title}: {label}"))
                                .on_click(cx.listener(
                                    move |view, _, window, cx| {
                                        view.act(Action::Mode(id, mode), window, cx)
                                    },
                                ))
                            }),
                        )
                        .child(
                            Button::new(("source-pause", key))
                                .h(px(44.))
                                .ghost()
                                .icon(if paused {
                                    IconName::Play
                                } else {
                                    IconName::Pause
                                })
                                .label(if paused { "Resume" } else { "Pause" })
                                .disabled(unavailable)
                                .on_click(cx.listener(move |view, _, window, cx| {
                                    view.act(
                                        if paused {
                                            Action::Resume(id)
                                        } else {
                                            Action::Mode(id, AutomationMode::Paused)
                                        },
                                        window,
                                        cx,
                                    )
                                })),
                        ),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(p.muted)
                        .child("Post its clips to"),
                )
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap_2()
                        .child(
                            choice(
                                ("route-default", key),
                                format!(
                                    "Default ({})",
                                    default_account.as_deref().unwrap_or("off")
                                ),
                                route.is_none(),
                            )
                            .accessibility_label(format!("{title}: post to the default account"))
                            .on_click(cx.listener(
                                move |view, _, window, cx| {
                                    view.act(Action::SourcePublish(id, None), window, cx)
                                },
                            )),
                        )
                        .children(accounts.iter().enumerate().map(|(index, account)| {
                            let account_id = account.id;
                            choice(
                                ("route-account", key * 1000 + index),
                                format!("@{}", account.username),
                                route == Some(account_id),
                            )
                            .accessibility_label(format!("{title}: post to @{}", account.username))
                            .disabled(unavailable || account.needs_login)
                            .on_click(cx.listener(
                                move |view, _, window, cx| {
                                    view.act(
                                        Action::SourcePublish(id, Some(account_id)),
                                        window,
                                        cx,
                                    )
                                },
                            ))
                        }))
                        .child(
                            choice(("route-off", key), "Nowhere".into(), route == Some(0))
                                .accessibility_label(format!("{title}: do not post"))
                                .on_click(cx.listener(move |view, _, window, cx| {
                                    view.act(Action::SourcePublish(id, Some(0)), window, cx)
                                })),
                        ),
                )
                .when(
                    source.mode != AutomationMode::AutoExport && route != Some(0),
                    |card| {
                        card.child(div().text_sm().text_color(p.muted).child(
                            "Clips are posted only when this source is set to \"Save MP4 files\".",
                        ))
                    },
                )
                .when_some(source.error, |card, error| {
                    card.child(div().text_sm().child(error))
                })
                .into_any_element()
        });
        let cards: Vec<_> = cards.collect();
        page(
            "automations-scroll",
            "Automations",
            "Every channel you follow is an automation: Tehom checks it, makes clips and can post them to the account you choose.",
            cx,
        )
        .child(
            div()
                .flex()
                .flex_wrap()
                .gap_2()
                .child(
                    Button::new("new-automation")
                        .h(px(44.))
                        .primary()
                        .icon(IconName::Plus)
                        .label("New automation")
                        .on_click(cx.listener(|view, _, window, cx| {
                            view.focus_link_field(window, cx)
                        })),
                )
                .child(self.check_button(cx)),
        )
        .when(cards.is_empty(), |v| {
            v.child(
                crate::theme::card(cx)
                    .items_center()
                    .py(px(40.))
                    .child(IconName::Zap)
                    .child(div().text_lg().child("No automations yet"))
                    .child(div().text_sm().text_color(p.muted).child(
                        "Add a YouTube or Kick channel and Tehom makes clips from every new video.",
                    )),
            )
        })
        .children(cards)
        .child(self.default_posting_card(cx))
        .child(self.automation_card(cx))
        .into_any_element()
    }

    pub(crate) fn connections_page(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = crate::theme::palette(cx);
        page(
            "connections-scroll",
            "Connections",
            "The accounts Tehom can post your clips to.",
            cx,
        )
        .child(self.instagram_card(cx))
        .child(self.unavailable_card(Platform::YoutubeShorts, cx))
        .child(self.unavailable_card(Platform::Tiktok, cx))
        .child(div().text_sm().text_color(p.muted).child(
            "Videos come from YouTube, Kick and RSS feeds without signing in. Add them on Home.",
        ))
        .into_any_element()
    }
}
