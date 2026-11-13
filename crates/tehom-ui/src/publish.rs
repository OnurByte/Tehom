//! Posting clips: the per-clip "Post" row and the Connections cards.
//! Instagram and TikTok work; YouTube Shorts is shown as coming soon.
use super::*;
use crate::automation_ui::Action;
use gpui_kit::Focusable as _;
use gpui_kit::assets::IconName;
use tehom_core::{Clip, Platform};

/// Instagram's caption limit, in characters.
pub(crate) const CAPTION_LIMIT: usize = 2200;

/// The sign-in form. The password field is read only when signing in and
/// cleared right after; it is never copied into view state or messages.
pub(crate) struct InstagramForm {
    pub(crate) open: bool,
    pub(crate) username: Entity<InputState>,
    password: Entity<InputState>,
    code: Entity<InputState>,
    /// Instagram asked for a code: (where it was sent, from an authenticator app).
    pub(crate) two_factor: Option<(Option<String>, bool)>,
    /// The last sign-in error, as the engine wrote it.
    pub(crate) note: Option<String>,
    /// The account waiting for "Remove" to be confirmed.
    pub(crate) confirm_remove: Option<i64>,
}

impl InstagramForm {
    pub(crate) fn new(window: &mut Window, cx: &mut Context<Tehom>) -> Self {
        Self {
            open: false,
            username: cx.new(|cx| InputState::new(window, cx).placeholder("Username")),
            password: cx.new(|cx| {
                InputState::new(window, cx)
                    .masked(true)
                    .placeholder("Password")
            }),
            code: cx.new(|cx| InputState::new(window, cx).placeholder("Code")),
            two_factor: None,
            note: None,
            confirm_remove: None,
        }
    }
}

/// "@name", with the platform when it is not Instagram.
pub(crate) fn account_label(account: &tehom_core::Account) -> String {
    match account.platform {
        Platform::Instagram => format!("@{}", account.username),
        platform => format!("@{} ({})", account.username, platform.label()),
    }
}

pub(crate) fn icon(platform: Platform) -> gpui_kit::component::Icon {
    gpui_kit::component::Icon::empty().path(match platform {
        Platform::Instagram => "icons/instagram.svg",
        Platform::YoutubeShorts => "icons/youtubeshorts.svg",
        Platform::Tiktok => "icons/tiktok.svg",
    })
}

impl Tehom {
    /// Settings, scrolled to one of its cards.
    pub(crate) fn open_settings_at(&mut self, card: usize, cx: &mut Context<Self>) {
        self.navigate(Page::Settings, cx);
        self.settings_scroll.scroll_to_top_of_item(card);
    }

    pub(crate) fn connect_instagram(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.instagram.open = true;
        // The guide shows the same card itself.
        if self.onboarding.is_none() {
            self.navigate(Page::Connections, cx);
        }
        let handle = self.instagram.username.read(cx).focus_handle(cx);
        window.focus(&handle, cx);
    }

    fn open_publish_form(
        &mut self,
        clip: &Clip,
        platform: Platform,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let on_platform = self.accounts_on(platform);
        if on_platform.is_empty() {
            match platform {
                Platform::Instagram => self.connect_instagram(window, cx),
                _ => {
                    self.navigate(Page::Connections, cx);
                    self.status = format!("Connect {} first.", platform.label());
                }
            }
            return;
        }
        if self.publish_open == Some(clip.id) {
            self.publish_open = None;
        } else {
            self.publish_open = Some(clip.id);
            let title = clip.title.clone();
            self.publish_caption
                .update(cx, |state, cx| state.set_value(title, window, cx));
            if !on_platform
                .iter()
                .any(|a| Some(a.id) == self.publish_account)
            {
                self.publish_account = on_platform
                    .iter()
                    .find(|a| !a.needs_login)
                    .or(on_platform.first())
                    .map(|a| a.id);
            }
        }
        cx.notify();
    }

    /// Every connected account Tehom can post to.
    pub(crate) fn publish_accounts(&self) -> Vec<tehom_core::Account> {
        self.engine_snapshot
            .as_ref()
            .map(|s| {
                s.accounts
                    .iter()
                    .filter(|a| a.platform.available())
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }
    pub(crate) fn accounts_on(&self, platform: Platform) -> Vec<tehom_core::Account> {
        let mut accounts = self.publish_accounts();
        accounts.retain(|a| a.platform == platform);
        accounts
    }
    /// The platform of an account, Instagram when it is unknown.
    pub(crate) fn platform_of(&self, account: i64) -> Platform {
        self.publish_accounts()
            .iter()
            .find(|a| a.id == account)
            .map_or(Platform::Instagram, |a| a.platform)
    }

    /// "Post" with Instagram, YouTube Shorts and TikTok, plus the form and
    /// the latest result for this clip.
    pub(crate) fn publish_row(&self, clip: &Clip, cx: &mut Context<Self>) -> AnyElement {
        let p = crate::theme::palette(cx);
        let unavailable = self.backend.is_none() || self.source_busy;
        let id = clip.id;
        let key = id as usize;
        let accounts = self.publish_accounts();
        let latest = self
            .engine_snapshot
            .as_ref()
            .and_then(|s| s.publications.iter().find(|p| p.clip_id == id))
            .cloned();
        let form_open = self.publish_open == Some(id);
        let caption = self.publish_caption.read(cx).value().to_string();
        let length = caption.chars().count();
        let account = accounts
            .iter()
            .find(|a| Some(a.id) == self.publish_account)
            .cloned();
        let open_clip = clip.clone();
        let target = account.as_ref().map_or(Platform::Instagram, |a| a.platform);
        let latest_platform = latest
            .as_ref()
            .map_or(target, |l| self.platform_of(l.account_id));
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_2()
                    .child(div().text_sm().child("Post"))
                    .children(Platform::ALL.into_iter().map(|platform| {
                        let soon = !platform.available();
                        let (name, label) = match platform {
                            Platform::Instagram => ("post-instagram", "Instagram".to_string()),
                            Platform::YoutubeShorts => {
                                ("post-shorts", format!("{} · Soon", platform.label()))
                            }
                            Platform::Tiktok => ("post-tiktok", "TikTok".to_string()),
                        };
                        let clip = open_clip.clone();
                        Button::new((name, key))
                            .h(px(44.))
                            .icon(icon(platform))
                            .label(label)
                            .accessibility_label(if soon {
                                format!("{}, coming soon", platform.label())
                            } else {
                                platform.label().to_string()
                            })
                            .when(form_open && !soon && target == platform, |button| button.primary())
                            .disabled(soon)
                            .on_click(cx.listener(move |view, _, window, cx| {
                                view.open_publish_form(&clip, platform, window, cx)
                            }))
                    })),
            )
            .when_some(latest.clone(), |row, publication| {
                let name = latest_platform.label();
                let message = match publication.state.as_str() {
                    "uploading" => format!("Posting to {name}…"),
                    "published" => format!("Posted to {name}."),
                    _ => format!(
                        "Could not post: {}",
                        publication
                            .error
                            .clone()
                            .unwrap_or_else(|| format!("{name} did not accept the clip."))
                    ),
                };
                let url = publication.url.clone().filter(|_| publication.state == "published");
                let retry = (publication.state == "failed").then_some(publication.account_id);
                let retry_caption = if form_open { caption.clone() } else { open_clip.title.clone() };
                row.child(div().text_sm().child(message))
                    .when_some(url, |row, url| {
                        row.child(
                            Button::new(("open-post", key))
                                .h(px(44.))
                                .label(format!("Open on {name}"))
                                .on_click(move |_, _, cx: &mut gpui_kit::App| cx.open_url(&url)),
                        )
                    })
                    .when_some(retry, |row, account| {
                        row.child(
                            Button::new(("retry-post", key))
                                .h(px(44.))
                                .label("Retry")
                                .accessibility_label(format!("Retry posting to {name}"))
                                .disabled(unavailable)
                                .on_click(cx.listener(move |view, _, window, cx| {
                                    view.act(
                                        Action::Publish(id, account, retry_caption.clone()),
                                        window,
                                        cx,
                                    )
                                })),
                        )
                    })
            })
            .when(form_open, |row| {
                row.child(
                    div()
                        .p_4()
                        .border_1()
                        .border_color(p.border)
                        .rounded(px(12.))
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(
                            Input::new(&self.publish_caption)
                                .h(px(44.))
                                .aria_label(format!("{} caption", target.label()))
                                .disabled(unavailable),
                        )
                        .child(div().text_sm().text_color(p.muted).child(
                            if length > CAPTION_LIMIT {
                                format!(
                                    "{length} / {CAPTION_LIMIT} characters. Remove {} to post.",
                                    length - CAPTION_LIMIT
                                )
                            } else {
                                format!("{length} / {CAPTION_LIMIT} characters")
                            },
                        ))
                        .when(accounts.len() > 1, |form| {
                            form.child(div().flex().flex_wrap().gap_2().children(
                                accounts.iter().enumerate().map(|(index, a)| {
                                    let selected = Some(a.id) == self.publish_account;
                                    let account = a.id;
                                    Button::new(("publish-account", index))
                                        .h(px(44.))
                                        .label(account_label(a))
                                        .when(selected, |button| button.primary())
                                        .toggled(selected)
                                        .on_click(cx.listener(move |view, _, _, cx| {
                                            view.publish_account = Some(account);
                                            cx.notify();
                                        }))
                                }),
                            ))
                        })
                        .when(account.as_ref().is_some_and(|a| a.needs_login), |form| {
                            form.child(div().text_sm().child(
                                "This account needs you to sign in again in Connections before posting.",
                            ))
                        })
                        .child(
                            Button::new(("publish-post", key))
                                .h(px(44.))
                                .primary()
                                .label(format!("Post to {}", target.label()))
                                .disabled(
                                    unavailable
                                        || length > CAPTION_LIMIT
                                        || account.as_ref().is_none_or(|a| a.needs_login),
                                )
                                .on_click(cx.listener(move |view, _, window, cx| {
                                    let Some(account) = view.publish_account else {
                                        return;
                                    };
                                    let caption =
                                        view.publish_caption.read(cx).value().to_string();
                                    view.act(Action::Publish(id, account, caption), window, cx);
                                    if view.source_busy {
                                        view.status = format!("Posting to {}…", view.platform_of(account).label());
                                    }
                                })),
                        ),
                )
            })
            .into_any_element()
    }

    fn sign_in(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let username = self.instagram.username.read(cx).value().trim().to_string();
        let password = self.instagram.password.read(cx).value().to_string();
        self.instagram
            .password
            .update(cx, |state, cx| state.set_value("", window, cx));
        if username.is_empty() || password.is_empty() {
            self.instagram.note = Some("Enter your Instagram username and password.".into());
            cx.notify();
            return;
        }
        self.instagram.note = None;
        self.act(Action::InstagramLogin(username, password), window, cx);
    }

    fn confirm_code(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let code = self.instagram.code.read(cx).value().trim().to_string();
        self.instagram
            .code
            .update(cx, |state, cx| state.set_value("", window, cx));
        if code.is_empty() {
            self.instagram.note = Some("Enter the code first.".into());
            cx.notify();
            return;
        }
        self.instagram.note = None;
        self.act(Action::InstagramTwoFactor(code), window, cx);
    }

    /// The connected accounts of one platform, each with its Remove (and
    /// "Sign in again") controls.
    fn account_rows(&self, platform: Platform, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let p = crate::theme::palette(cx);
        let unavailable = self.backend.is_none() || self.source_busy;
        let form = &self.instagram;
        let accounts = self.accounts_on(platform);
        accounts
            .iter()
            .enumerate()
            .map(|(index, account)| {
                {
                    let id = account.id;
                    let key = index + platform as usize * 1000;
                    let username = account.username.clone();
                    let confirming = form.confirm_remove == Some(id);
                    div()
                        .flex()
                        .flex_wrap()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .flex_1()
                                .flex()
                                .flex_col()
                                .child(format!("@{}", account.username))
                                .when(!account.full_name.is_empty(), |name| {
                                    name.child(
                                        div()
                                            .text_sm()
                                            .text_color(p.muted)
                                            .child(account.full_name.clone()),
                                    )
                                }),
                        )
                        .when(account.needs_login, |row| {
                            row.child(
                                Button::new(("account-sign-in", key))
                                    .h(px(44.))
                                    .label("Sign in again")
                                    .accessibility_label(format!("Sign in again as @{username}"))
                                    .disabled(unavailable)
                                    .on_click(cx.listener(move |view, _, window, cx| {
                                        if platform == Platform::Instagram {
                                            let name = username.clone();
                                            view.instagram.username.update(cx, |state, cx| {
                                                state.set_value(name, window, cx)
                                            });
                                            view.connect_instagram(window, cx);
                                        } else {
                                            view.sign_in_tiktok(window, cx);
                                        }
                                    })),
                            )
                        })
                        .when(!confirming, |row| {
                            row.child(
                                Button::new(("account-remove", key))
                                    .h(px(44.))
                                    .label("Remove")
                                    .accessibility_label(format!("Remove @{}", account.username))
                                    .disabled(unavailable)
                                    .on_click(cx.listener(move |view, _, _, cx| {
                                        view.instagram.confirm_remove = Some(id);
                                        cx.notify();
                                    })),
                            )
                        })
                        .when(confirming, |row| {
                            row.child(div().text_sm().child(format!(
                                "Remove @{} from Tehom? Your posts stay on {}.",
                                account.username,
                                platform.label()
                            )))
                            .child(
                                Button::new(("account-remove-yes", key))
                                    .h(px(44.))
                                    .primary()
                                    .label("Yes, remove")
                                    .disabled(unavailable)
                                    .on_click(cx.listener(move |view, _, window, cx| {
                                        view.instagram.confirm_remove = None;
                                        view.act(Action::RemoveAccount(id), window, cx)
                                    })),
                            )
                            .child(
                                Button::new(("account-remove-no", key))
                                    .h(px(44.))
                                    .label("Keep")
                                    .on_click(cx.listener(|view, _, _, cx| {
                                        view.instagram.confirm_remove = None;
                                        cx.notify();
                                    })),
                            )
                        })
                }
                .into_any_element()
            })
            .collect()
    }

    /// Settings: connected Instagram accounts, signing in and automatic posting.
    pub(crate) fn instagram_card(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = crate::theme::palette(cx);
        let unavailable = self.backend.is_none() || self.source_busy;
        let accounts = self.accounts_on(Platform::Instagram);
        let form = &self.instagram;
        div()
            .p_5()
            .bg(p.background)
            .border_1()
            .border_color(p.border)
            .rounded(px(12.))
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        svg()
                            .path("icons/instagram.svg")
                            .size(px(20.))
                            .text_color(p.text),
                    )
                    .child(div().text_lg().child("Instagram")),
            )
            .child(div().text_sm().text_color(p.muted).child(
                "Tehom signs in the way the Instagram phone app does, not through an official connection. Instagram may ask you to confirm the sign-in in its app, and it may restrict accounts that post from other apps.",
            ))
            .when(accounts.is_empty(), |card| {
                card.child(div().text_sm().child("No account connected yet."))
            })
            .when(!accounts.is_empty(), |card| {
                card.child(div().text_sm().text_color(p.muted).child(
                    "Choose which source posts to which account in Automations.",
                ))
            })
            .children(self.account_rows(Platform::Instagram, cx))
            .when(!form.open, |card| {
                card.child(
                    div().flex().child(
                        Button::new("connect-instagram")
                            .h(px(44.))
                            .primary()
                            .icon(IconName::Plus)
                            .label("Connect Instagram")
                            .disabled(self.backend.is_none())
                            .on_click(cx.listener(|view, _, window, cx| {
                                view.connect_instagram(window, cx)
                            })),
                    ),
                )
            })
            .when(form.open && form.two_factor.is_none(), |card| {
                card.child(
                    Input::new(&form.username)
                        .h(px(44.))
                        .aria_label("Instagram username")
                        .disabled(unavailable),
                )
                .child(
                    Input::new(&form.password)
                        .h(px(44.))
                        .aria_label("Instagram password")
                        .disabled(unavailable),
                )
                .child(div().text_sm().text_color(p.muted).child(
                    "Your password is sent only to Instagram and is not saved. Tehom keeps the sign-in in the system keyring.",
                ))
                .child(
                    div()
                        .flex()
                        .gap_2()
                        .child(
                            Button::new("instagram-sign-in")
                                .h(px(44.))
                                .primary()
                                .label("Sign in")
                                .disabled(unavailable)
                                .on_click(cx.listener(|view, _, window, cx| {
                                    view.sign_in(window, cx)
                                })),
                        )
                        .child(
                            Button::new("instagram-cancel")
                                .h(px(44.))
                                .label("Cancel")
                                .on_click(cx.listener(|view, _, window, cx| {
                                    view.instagram.open = false;
                                    view.instagram.note = None;
                                    view.instagram
                                        .password
                                        .update(cx, |state, cx| state.set_value("", window, cx));
                                    cx.notify();
                                })),
                        ),
                )
            })
            .when_some(form.two_factor.clone(), |card, (hint, app)| {
                let instruction = if app {
                    "Enter the 6-digit code from your authenticator app".to_string()
                } else {
                    match hint {
                        Some(hint) => format!("Enter the code Instagram sent by SMS to {hint}"),
                        None => "Enter the code Instagram sent by SMS".to_string(),
                    }
                };
                card.child(
                    div()
                        .id("two-factor-instruction")
                        .test_support()
                        .aria_label(instruction.clone())
                        .text_sm()
                        .child(instruction),
                )
                .child(
                    Input::new(&form.code)
                        .h(px(44.))
                        .aria_label("Sign-in code")
                        .disabled(unavailable),
                )
                .child(
                    div()
                        .flex()
                        .gap_2()
                        .child(
                            Button::new("instagram-code")
                                .h(px(44.))
                                .primary()
                                .label("Confirm code")
                                .disabled(unavailable)
                                .on_click(cx.listener(|view, _, window, cx| {
                                    view.confirm_code(window, cx)
                                })),
                        )
                        .child(
                            Button::new("instagram-code-cancel")
                                .h(px(44.))
                                .label("Start over")
                                .on_click(cx.listener(|view, _, _, cx| {
                                    view.instagram.two_factor = None;
                                    view.instagram.note = None;
                                    cx.notify();
                                })),
                        ),
                )
            })
            .when_some(form.note.clone(), |card, note| {
                card.child(div().text_sm().child(note))
            })
            .into_any_element()
    }

    pub(crate) fn sign_in_tiktok(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.act(Action::TiktokLogin, window, cx);
        if self.source_busy {
            self.status = "Sign in to TikTok in the browser window that opened.".into();
        }
    }

    /// Connections: TikTok accounts. Sign-in happens in a browser window.
    pub(crate) fn tiktok_card(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = crate::theme::palette(cx);
        let accounts = self.accounts_on(Platform::Tiktok);
        crate::theme::card(cx)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(icon(Platform::Tiktok))
                    .child(div().text_lg().child("TikTok"))
                    .child(
                        div()
                            .px_2()
                            .py_1()
                            .rounded(px(6.))
                            .bg(p.surface)
                            .text_xs()
                            .text_color(p.muted)
                            .child("Experimental"),
                    ),
            )
            .child(div().text_sm().text_color(p.muted).child(
                "Tehom opens a Chrome, Chromium, Edge or Brave window with its own profile, and you sign in to TikTok there; Tehom never sees your password. Posting uses that window too, so it opens while a clip uploads. This is not an official connection: TikTok may restrict accounts that post from other apps.",
            ))
            .when(accounts.is_empty(), |card| {
                card.child(div().text_sm().child("No account connected yet."))
            })
            .children(self.account_rows(Platform::Tiktok, cx))
            .child(
                div().flex().child(
                    Button::new("connect-tiktok")
                        .h(px(44.))
                        .primary()
                        .icon(IconName::Plus)
                        .label("Connect TikTok")
                        .disabled(self.backend.is_none() || self.source_busy)
                        .on_click(cx.listener(|view, _, window, cx| {
                            view.sign_in_tiktok(window, cx)
                        })),
                ),
            )
            .into_any_element()
    }

    /// Automations: the account used by every source set to "Default".
    pub(crate) fn default_posting_card(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = crate::theme::palette(cx);
        let unavailable = self.backend.is_none() || self.source_busy;
        let accounts = self.publish_accounts();
        let auto = self
            .engine_snapshot
            .as_ref()
            .and_then(|s| s.auto_publish_account);
        crate::theme::card(cx)
            .child(div().text_lg().child("Default posting account"))
            .child(div().text_sm().text_color(p.muted).child(
                "Used by every source set to \"Default\". Only sources that save finished MP4 files post their clips.",
            ))
            .when(accounts.is_empty(), |card| {
                card.child(div().text_sm().child("No account connected yet.")).child(
                    Button::new("posting-connect")
                        .h(px(44.))
                        .label("Open Connections")
                        .on_click(cx.listener(|view, _, _, cx| {
                            view.navigate(Page::Connections, cx)
                        })),
                )
            })
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    .child(
                        Button::new("auto-publish-off")
                            .h(px(44.))
                            .label("Off")
                            .accessibility_label("Do not post automatically")
                            .when(auto.is_none(), |button| button.primary())
                            .toggled(auto.is_none())
                            .disabled(unavailable || accounts.is_empty())
                            .on_click(cx.listener(|view, _, window, cx| {
                                view.act(Action::AutoPublish(None), window, cx)
                            })),
                    )
                    .children(accounts.iter().enumerate().map(|(index, account)| {
                        let id = account.id;
                        let selected = auto == Some(id);
                        Button::new(("auto-publish", index))
                            .h(px(44.))
                            .label(account_label(account))
                            .accessibility_label(format!(
                                "Post automatically to @{}",
                                account.username
                            ))
                            .when(selected, |button| button.primary())
                            .toggled(selected)
                            .disabled(unavailable || account.needs_login)
                            .on_click(cx.listener(move |view, _, window, cx| {
                                view.act(Action::AutoPublish(Some(id)), window, cx)
                            }))
                    })),
            )
            .into_any_element()
    }

    /// Connections: a platform Tehom cannot post to yet.
    pub(crate) fn unavailable_card(
        &self,
        platform: Platform,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = crate::theme::palette(cx);
        let name = platform.label();
        crate::theme::card(cx)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(icon(platform))
                    .child(div().text_lg().child(name))
                    .child(
                        div()
                            .px_2()
                            .py_1()
                            .rounded(px(6.))
                            .bg(p.surface)
                            .text_xs()
                            .text_color(p.muted)
                            .child("Not available yet"),
                    ),
            )
            .child(div().text_sm().text_color(p.muted).child(format!(
                "Tehom cannot post to {name} yet. For now, save the MP4 and upload it yourself."
            )))
            .child(
                div().flex().child(
                    Button::new(("connect-soon", platform as usize))
                        .h(px(44.))
                        .label(format!("Connect {name}"))
                        .accessibility_label(format!("{name}, not available yet"))
                        .disabled(true),
                ),
            )
            .into_any_element()
    }
}
