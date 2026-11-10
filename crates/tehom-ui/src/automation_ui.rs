use super::*;
use gpui_kit::assets::IconName;
use gpui_kit::component::sidebar::{Sidebar, SidebarMenu, SidebarMenuItem};
use gpui_kit::gpui::{Div, rgb};
use tehom_automation::Automation;
use tehom_core::{
    AccountLogin, AiConfig, AiProvider, Appearance, AutomationSettings, CaptionStyle, Cue,
    SoundEffect,
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Page {
    Home,
    Sources,
    Clips,
    Settings,
    About,
}

pub(crate) struct Backend {
    pub(crate) runtime: tokio::runtime::Runtime,
    pub(crate) engine: Automation,
}

pub(crate) enum Action {
    Add(String, AutomationMode),
    File(PathBuf, AutomationMode),
    Check,
    Mode(i64, AutomationMode),
    Resume(i64),
    Ai(AiConfig),
    /// Remote provider with explicit consent, plus a key to store if given.
    Byok(AiConfig, String),
    SaveRevision(i64, u64, u64),
    Retry(i64),
    Import(i64, PathBuf),
    Cancel(i64),
    SaveOriginal(i64),
    ExportTranscript(i64),
    ExportDirectory(PathBuf),
    ExportProfile(tehom_core::ExportProfile),
    Onboarded,
    CheckUpdates(bool),
    SaveCaptions(i64, Vec<Cue>, CaptionStyle, SoundEffect),
    /// Saves the captions, then makes the clip's video again.
    RenderClip(i64, Vec<Cue>, CaptionStyle, SoundEffect),
    DefaultCaptionStyle(CaptionStyle),
    DownloadSpeechModel,
    /// Username and password; the password lives only in this action.
    InstagramLogin(String, String),
    InstagramTwoFactor(String),
    RemoveAccount(i64),
    /// Clip, account, caption.
    Publish(i64, i64, String),
    AutoPublish(Option<i64>),
    Automation(AutomationSettings),
    Appearance(Appearance),
}

/// What the view does after an action, besides showing its message.
enum Followup {
    TwoFactor(Option<String>, bool),
    SignedIn,
    /// A key was saved: list that provider's models again.
    Models,
}

/// Settings cards in order, for jumping to one.
pub(crate) const CARD_AI: usize = 1;
pub(crate) const CARD_LOOK: usize = 2;
pub(crate) const CARD_SPEECH: usize = 3;
pub(crate) const CARD_INSTAGRAM: usize = 4;
pub(crate) const CARD_AUTOMATION: usize = 5;

impl Tehom {
    pub fn start_automation(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self._engine_task.is_some() {
            return;
        }
        self._engine_task = Some(Self::start_engine(self.engine_stop.clone(), window, cx));
        self._snapshot_task = Some(Self::watch_engine(window, cx));
    }

    pub(super) fn start_engine(
        stop: Arc<AtomicBool>,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Task<()> {
        let initialization = cx.background_spawn(async move {
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .map_err(|e| e.to_string())?;
            let directories = directories::ProjectDirs::from("org", "Tehom", "Tehom")
                .ok_or("Could not find a data folder")?;
            let data = directories.data_local_dir().to_path_buf();
            let engine = runtime
                .block_on(Automation::open(data.clone()))
                .map_err(|e| e.to_string())?;
            let snapshot = runtime
                .block_on(engine.snapshot())
                .map_err(|e| e.to_string())?;
            if snapshot.export_directory == data.join("exports")
                && let Some(videos) =
                    directories::UserDirs::new().and_then(|d| d.video_dir().map(PathBuf::from))
            {
                runtime
                    .block_on(engine.set_export_directory(videos.join("Tehom")))
                    .map_err(|e| e.to_string())?;
            }
            Ok::<_, String>(Arc::new(Backend { runtime, engine }))
        });
        cx.spawn_in(window, async move |view, cx| {
            let backend = match initialization.await {
                Ok(backend) => backend,
                Err(error) => {
                    let _ = view.update(cx, |view, cx| {
                        view.status = "Sources could not be loaded. See the details.".into();
                        view.details = Some(error);
                        cx.notify();
                    });
                    return;
                }
            };
            let boot = backend.clone();
            let result = cx
                .background_spawn(async move {
                    boot.runtime
                        .block_on(boot.engine.snapshot())
                        .map_err(|e| e.to_string())
                })
                .await;
            if view
                .update_in(cx, |view, window, cx| {
                    view.backend = Some(backend.clone());
                    if let Ok(snapshot) = result {
                        view.ai_endpoint.update(cx, |state, cx| {
                            state.set_value(snapshot.ai.endpoint.clone(), window, cx)
                        });
                        view.ai_model.update(cx, |state, cx| {
                            state.set_value(snapshot.ai.model.clone(), window, cx)
                        });
                        view.engine_snapshot = Some(snapshot);
                        view.sync_prefs(window, cx);
                    }
                    view.status = "Add a channel or feed to start.".into();
                    cx.notify();
                })
                .is_err()
            {
                return;
            }
            let mut last_check: Option<std::time::Instant> = None;
            while !stop.load(Ordering::Acquire) {
                let worker = backend.clone();
                // The interval is read each round so a saved change applies at once.
                let (checked, result) = cx
                    .background_spawn(async move {
                        worker.runtime.block_on(async {
                            let minutes = worker
                                .engine
                                .automation()
                                .await
                                .map_or(30, |settings| settings.check_minutes);
                            let check = last_check.is_none_or(|at| {
                                at.elapsed() >= Duration::from_secs(u64::from(minutes) * 60)
                            });
                            let result = async {
                                if check {
                                    worker.engine.check_now().await?;
                                }
                                worker.engine.process_next().await
                            }
                            .await;
                            (check, result.map_err(|e| e.to_string()))
                        })
                    })
                    .await;
                if checked {
                    last_check = Some(std::time::Instant::now());
                }
                if let Err(error) = result {
                    eprintln!("Tehom automation: {error}");
                }
                if view.read_with(cx, |_, _| ()).is_err() {
                    break;
                }
                cx.background_executor().timer(Duration::from_secs(2)).await;
            }
        })
    }

    pub(super) fn watch_engine(window: &Window, cx: &mut Context<Self>) -> Task<()> {
        cx.spawn_in(window, async move |view, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(2)).await;
                let backend = match view.read_with(cx, |view, _| view.backend.clone()) {
                    Ok(Some(backend)) => backend,
                    Ok(None) => continue,
                    Err(_) => break,
                };
                let snapshot = cx
                    .background_spawn(async move {
                        backend
                            .runtime
                            .block_on(backend.engine.snapshot())
                            .map_err(|e| e.to_string())
                    })
                    .await;
                if view
                    .update(cx, |view, cx| {
                        match snapshot {
                            Ok(snapshot) => view.engine_snapshot = Some(snapshot),
                            Err(error) => eprintln!("Tehom state: {error}"),
                        };
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
    }

    pub(crate) fn act(&mut self, action: Action, window: &Window, cx: &mut Context<Self>) {
        let Some(backend) = self.backend.clone() else {
            return;
        };
        if self.source_busy {
            return;
        }
        self.source_busy = true;
        self.details = None;
        self.status = "Working…".into();
        cx.notify();
        // Newer actions report the engine's own plain-language reason.
        let own_words = matches!(
            action,
            Action::SaveCaptions(..)
                | Action::RenderClip(..)
                | Action::DefaultCaptionStyle(_)
                | Action::DownloadSpeechModel
                | Action::InstagramLogin(..)
                | Action::InstagramTwoFactor(_)
                | Action::RemoveAccount(_)
                | Action::Publish(..)
                | Action::AutoPublish(_)
                | Action::Automation(_)
                | Action::Appearance(_)
        );
        let signing_in = matches!(
            action,
            Action::InstagramLogin(..) | Action::InstagramTwoFactor(_)
        );
        let speech = matches!(action, Action::DownloadSpeechModel);
        let task = cx.background_spawn(async move {
            backend
                .runtime
                .block_on(async {
                    let mut followup = None;
                    let message = match action {
                        Action::Add(url, mode) => {
                            let source=backend.engine.add_source(&url,mode).await?;
                            if source.channel_id.starts_with("video:") {
                                "Video added. Follow its progress below.".into()
                            } else if source.error.is_some() {
                                "Channel saved. Its feed is not reachable yet; Tehom will retry.".into()
                            } else if source.channel_id.starts_with("feed:") {
                                format!("Feed \"{}\" added. Tehom watches it for new videos.", source.title)
                            } else {"Channel added. The latest video and every new one will be processed.".into()}
                        }
                        Action::File(path, mode) => {
                            backend.engine.import_video(path, mode).await?;
                            "Video added. If it needs captions, add them from the job list.".into()
                        }
                        Action::Check => format!(
                            "Sources checked. {} new videos found.",
                            backend.engine.check_now().await?
                        ),
                        Action::Mode(id, mode) => {
                            backend.engine.set_mode(id, mode).await?;
                            "Mode updated.".into()
                        }
                        Action::Resume(id) => {
                            backend.engine.resume_source(id).await?;
                            "Automation resumed.".into()
                        }
                        Action::Ai(ai) => {
                            backend.engine.set_ai(ai).await?;
                            "Model settings saved.".into()
                        }
                        Action::Byok(ai, key) => {
                            let provider = ai.provider;
                            if provider.uses_api_key() && !key.trim().is_empty() {
                                tokio::task::spawn_blocking(move || {
                                    tehom_automation::store_api_key(provider, &key)
                                })
                                .await??;
                            }
                            anyhow::ensure!(
                                !provider.uses_api_key()
                                    || tehom_automation::api_key(provider)?.is_some(),
                                "Paste your {} API key first",
                                provider.label()
                            );
                            backend.engine.set_ai(ai).await?;
                            followup = Some(Followup::Models);
                            format!("{} is ready. Captions will be sent there for analysis.", provider.label())
                        }
                        Action::SaveRevision(id, start, end) => {
                            backend.engine.save_clip_revision(id, start, end).await?;
                            "New draft saved. Earlier drafts and files are kept.".into()
                        }
                        Action::Import(id,path)=>{backend.engine.import_transcript(id,path,None).await?;"Captions saved. Choose Retry to make the clips.".into()}
                        Action::Retry(id) => {
                            backend.engine.retry_video(id).await?;
                            "Queued again.".into()
                        }
                        Action::Cancel(id) => {
                            backend.engine.cancel_video(id).await?;
                            "Job cancelled.".into()
                        }
                        Action::SaveOriginal(id) => {
                            let folder = backend.engine.snapshot().await?.export_directory;
                            let saved = backend.engine.save_original(id, folder).await?;
                            format!("Original saved: {}", saved.display())
                        }
                        Action::ExportTranscript(id) => {
                            let folder = backend.engine.snapshot().await?.export_directory;
                            tokio::fs::create_dir_all(&folder).await?;
                            let path = folder.join(format!("tehom-transcript-{id}.srt"));
                            backend.engine.export_transcript(id, path.clone()).await?;
                            format!(
                                "Captions saved for editing: {}. Edit them, then add them back with \"Add captions\".",
                                path.display()
                            )
                        }
                        Action::CheckUpdates(enabled) => {
                            backend.engine.set_check_updates(enabled).await?;
                            if enabled {
                                "Tehom will look for updates when it starts.".into()
                            } else {
                                "Automatic update checks are off.".into()
                            }
                        }
                        Action::Onboarded => {
                            backend.engine.mark_onboarded().await?;
                            "You are all set. Paste a link to make your first clips.".into()
                        }
                        Action::ExportProfile(profile) => {
                            backend.engine.set_export_profile(profile).await?;
                            "Clip shape saved for new clips.".into()
                        }
                        Action::ExportDirectory(path) => {
                            backend.engine.set_export_directory(path).await?;
                            "Clip folder changed.".into()
                        }
                        Action::SaveCaptions(id, cues, style, sound) => {
                            backend.engine.save_clip_captions(id, cues, style, sound).await?;
                            "Captions saved. Choose \"Make the video again\" to put them in the MP4.".into()
                        }
                        Action::RenderClip(id, cues, style, sound) => {
                            backend.engine.save_clip_captions(id, cues, style, sound).await?;
                            let path = backend.engine.render_clip(id).await?;
                            format!("Video made again: {}", path.display())
                        }
                        Action::DefaultCaptionStyle(style) => {
                            backend.engine.set_default_caption_style(style).await?;
                            "Caption look saved for new clips.".into()
                        }
                        Action::DownloadSpeechModel => {
                            backend.engine.download_speech_model().await?;
                            "Captions on this computer are ready. Videos without captions can now be used.".into()
                        }
                        Action::InstagramLogin(username, password) => {
                            match backend.engine.instagram_login(&username, &password).await? {
                                AccountLogin::Done(account) => {
                                    followup = Some(Followup::SignedIn);
                                    format!("Instagram connected as @{}.", account.username)
                                }
                                AccountLogin::TwoFactor { hint, app } => {
                                    followup = Some(Followup::TwoFactor(hint, app));
                                    "Instagram asked for a sign-in code.".into()
                                }
                            }
                        }
                        Action::InstagramTwoFactor(code) => {
                            let account = backend.engine.instagram_two_factor(&code).await?;
                            followup = Some(Followup::SignedIn);
                            format!("Instagram connected as @{}.", account.username)
                        }
                        Action::RemoveAccount(id) => {
                            backend.engine.remove_account(id).await?;
                            "Account removed from Tehom.".into()
                        }
                        Action::Publish(clip, account, caption) => {
                            let publication = backend.engine.publish_clip(clip, account, &caption).await?;
                            match publication.state.as_str() {
                                "published" => "Posted to Instagram.".into(),
                                "failed" => format!(
                                    "Could not post: {}",
                                    publication.error.unwrap_or_default()
                                ),
                                _ => "Instagram is still processing the post.".into(),
                            }
                        }
                        Action::Automation(settings) => {
                            backend.engine.set_automation(settings).await?;
                            "Automation settings saved.".into()
                        }
                        Action::Appearance(appearance) => {
                            backend.engine.set_appearance(appearance).await?;
                            "Appearance saved.".into()
                        }
                        Action::AutoPublish(account) => {
                            backend.engine.set_auto_publish(account).await?;
                            if account.is_some() {
                                "New clips from MP4 sources will be posted automatically.".into()
                            } else {
                                "Automatic posting is off.".into()
                            }
                        }
                    };
                    Ok::<_, anyhow::Error>((backend.engine.snapshot().await?, message, followup))
                })
                .map_err(|e| e.to_string())
        });
        cx.spawn_in(window, async move |view, cx| {
            let result = task.await;
            let _ = view.update_in(cx, |view, window, cx| {
                view.source_busy = false;
                if speech {
                    view.speech_downloading = false;
                    view.speech = None;
                }
                match result {
                    Ok((snapshot, message, followup)) => {
                        view.engine_snapshot = Some(snapshot);
                        if view.onboarding.is_some() {
                            view.guide_note = Some(message.clone());
                        }
                        view.status = message;
                        match followup {
                            Some(Followup::TwoFactor(hint, app)) => {
                                view.instagram.two_factor = Some((hint, app));
                            }
                            Some(Followup::SignedIn) => {
                                view.instagram.two_factor = None;
                                view.instagram.open = false;
                                view.instagram.note = None;
                                view.instagram
                                    .username
                                    .update(cx, |state, cx| state.set_value("", window, cx));
                            }
                            Some(Followup::Models) => view.model_list = None,
                            None => {}
                        }
                    }
                    Err(error) => {
                        if view.onboarding.is_some() {
                            view.guide_note = Some(error.clone());
                        }
                        if signing_in {
                            view.instagram.note = Some(error.clone());
                        }
                        if own_words {
                            view.status = error;
                        } else {
                            view.status =
                                "That did not finish. Check the link and your settings.".into();
                            view.details = Some(error);
                        }
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn save_draft_revision(
        &mut self,
        id: i64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let values = (
            self.start.read(cx).value().parse::<f64>(),
            self.end.read(cx).value().parse::<f64>(),
        );
        if let (Ok(start), Ok(end)) = values
            && start.is_finite()
            && end.is_finite()
            && start >= 0.
            && end > start
            && end <= self.total
        {
            self.act(
                Action::SaveRevision(
                    id,
                    (start * 1000.).round() as u64,
                    (end * 1000.).floor() as u64,
                ),
                window,
                cx,
            );
        } else {
            self.status = "Enter a start and end inside the video.".into();
            cx.notify();
        }
    }

    pub(crate) fn choose_input_file(
        &mut self,
        subtitle_for: Option<i64>,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        let picker = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(
                if subtitle_for.is_some() {
                    "Choose an SRT or VTT caption file"
                } else {
                    "Choose a video to make clips from"
                }
                .into(),
            ),
        });
        cx.spawn_in(window, async move |view, cx| match picker.await {
            Ok(Ok(Some(paths))) => {
                if let Some(path) = paths.into_iter().next() {
                    let _ = view.update_in(cx, |view, window, cx| {
                        let action = match subtitle_for {
                            Some(id) => Action::Import(id, path),
                            None => Action::File(path, view.source_mode),
                        };
                        view.act(action, window, cx)
                    });
                }
            }
            Ok(Ok(None)) => {}
            error => {
                let _ = view.update(cx, |view, cx| {
                    view.status = "The file could not be chosen.".into();
                    view.details = Some(format!("{error:?}"));
                    cx.notify();
                });
            }
        })
        .detach();
    }

    pub(crate) fn choose_export_directory(&mut self, window: &Window, cx: &mut Context<Self>) {
        let picker = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Folder where clips are saved".into()),
        });
        cx.spawn_in(window, async move |view, cx| {
            if let Ok(Ok(Some(paths))) = picker.await
                && let Some(path) = paths.into_iter().next()
            {
                let _ = view.update_in(cx, |view, window, cx| {
                    view.act(Action::ExportDirectory(path), window, cx)
                });
            }
        })
        .detach();
    }

    pub(crate) fn navigate(&mut self, page: Page, cx: &mut Context<Self>) {
        self.page = page;
        if page != Page::Clips
            && let Some(preview) = &self.preview
            && let Err(error) = preview.set_paused(true)
        {
            eprintln!("Pause preview: {error}");
        }
        cx.notify();
    }

    fn source_form(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = crate::theme::palette(cx);
        let unavailable = self.backend.is_none() || self.source_busy;
        div().flex().flex_col().gap_4().p(px(24.)).bg(p.background).border_1().border_color(p.border).rounded(px(12.))
            .child(div().text_lg().child("Add a YouTube or Kick channel, a video or an RSS feed"))
            .child(Input::new(&self.source_url).h(px(44.)).id("channel-url").aria_label("Channel, video or RSS feed address").disabled(unavailable))
            .child(div().text_sm().text_color(p.muted).child("A YouTube or Kick channel, a video, or the link of any RSS or Atom feed with videos."))
            .child(div().flex().gap_2()
                .child(Button::new("mode-draft").h(px(44.)).label("Drafts").when(self.source_mode==AutomationMode::AutoDraft,|b|b.primary())
                    .on_click(cx.listener(|view,_,_,cx|{view.source_mode=AutomationMode::AutoDraft;cx.notify();})))
                .child(Button::new("mode-export").h(px(44.)).label("MP4 files").when(self.source_mode==AutomationMode::AutoExport,|b|b.primary())
                    .on_click(cx.listener(|view,_,_,cx|{view.source_mode=AutomationMode::AutoExport;cx.notify();}))))
            .child(div().text_sm().text_color(p.muted).child(match self.source_mode {AutomationMode::AutoExport=>"Clips are saved to your folder automatically. Channels and feeds are watched for new videos.",AutomationMode::Manual=>"New videos are listed; you choose which ones to make clips from.",_=>"Tehom prepares drafts for you to review. Channels and feeds are watched for new videos."}))
            .child(Button::new("add-channel").h(px(44.)).primary().label(if self.source_busy {"Adding…"} else {"Add link"}).disabled(unavailable)
                .on_click(cx.listener(|view,_,window,cx|view.act(Action::Add(view.source_url.read(cx).value().to_string(),view.source_mode),window,cx))))
            .child(Button::new("import-local-video").h(px(44.)).label("Make clips from a file").disabled(unavailable)
                .on_click(cx.listener(|view,_,window,cx| view.choose_input_file(None, window, cx))))
            .child(div().text_sm().text_color(p.muted).child("The file is read where it is. Keep it in place until the clips are ready."))
            .into_any_element()
    }

    fn source_list(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = crate::theme::palette(cx);
        let sources = self
            .engine_snapshot
            .as_ref()
            .map(|s| s.sources.clone())
            .unwrap_or_default();
        div()
            .flex()
            .flex_col()
            .gap_3()
            .when(sources.is_empty(), |v| {
                v.child(div().p_5().text_color(p.muted).child("No sources yet."))
            })
            .children(sources.into_iter().map(|source| {
                let id = source.id;
                div()
                    .p_4()
                    .bg(p.background)
                    .border_1()
                    .border_color(p.border)
                    .rounded(px(12.))
                    .flex()
                    .flex_col()
                    .gap_2()
                    .children(platform_badge(&source))
                    .child(div().text_sm().text_color(p.muted).child(
                        if source.channel_id.starts_with("local:") {
                            "Local file"
                        } else if source.channel_id.starts_with("video:") {
                            "Single video"
                        } else if source.channel_id.starts_with("feed:") {
                            "RSS feed, watching for new videos"
                        } else {
                            "Channel, watching for new videos"
                        },
                    ))
                    .child(div().text_lg().child(source.title))
                    .child(div().text_sm().text_color(p.muted).child(source.url))
                    .child(div().text_sm().child(match source.mode {
                        AutomationMode::Paused => "Paused",
                        AutomationMode::AutoExport => "MP4 files",
                        AutomationMode::AutoDraft => "Drafts",
                        AutomationMode::Manual => "Manual",
                    }))
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(
                                Button::new(("source-pause", id as usize))
                                    .h(px(44.))
                                    .label(if source.mode == AutomationMode::Paused {
                                        "Resume"
                                    } else {
                                        "Pause"
                                    })
                                    .disabled(self.source_busy)
                                    .on_click(cx.listener(move |view, _, window, cx| {
                                        view.act(
                                            if source.mode == AutomationMode::Paused {
                                                Action::Resume(id)
                                            } else {
                                                Action::Mode(id, AutomationMode::Paused)
                                            },
                                            window,
                                            cx,
                                        )
                                    })),
                            )
                            .child(
                                Button::new(("source-export", id as usize))
                                    .h(px(44.))
                                    .label("MP4 files")
                                    .disabled(self.source_busy)
                                    .on_click(cx.listener(move |view, _, window, cx| {
                                        view.act(
                                            Action::Mode(id, AutomationMode::AutoExport),
                                            window,
                                            cx,
                                        )
                                    })),
                            ),
                    )
                    .when_some(source.error, |v, error| {
                        v.child(div().text_sm().text_color(p.text).child(error))
                    })
            }))
            .into_any_element()
    }

    fn job_list(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = crate::theme::palette(cx);
        let videos = self
            .engine_snapshot
            .as_ref()
            .map(|s| s.videos.clone())
            .unwrap_or_default();
        div()
            .flex()
            .flex_col()
            .gap_3()
            .when(videos.is_empty(), |v| {
                v.child(
                    div()
                        .text_color(p.muted)
                        .child("New videos will appear here."),
                )
            })
            .children(videos.into_iter().take(20).map(|video| {
                let id = video.id;
                let local_captions_missing = video.state == "needs_input"
                    && video.error.as_deref().is_some_and(|error| {
                        error.contains("local transcription")
                            || error.contains("whisper")
                            || error.contains("speech model")
                    });
                div()
                    .p_4()
                    .bg(p.background)
                    .border_1()
                    .border_color(p.border)
                    .rounded(px(12.))
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(video.title)
                    .child(
                        div()
                            .text_sm()
                            .text_color(p.muted)
                            .child(match video.state.as_str() {
                                "discovered" => "Not started",
                                "queued" => "Queued",
                                "running" => "Making clips",
                                "complete" | "completed" => "Done",
                                "needs_input" => "Needs your input",
                                "cancelled" => "Cancelled",
                                "retryable" => "Can be retried",
                                _ => "Stopped",
                            }),
                    )
                    .when_some(video.progress, |v, progress| {
                        v.child(
                            div()
                                .text_sm()
                                .child(format!("Exporting {:.0}%", progress * 100.0)),
                        )
                    })
                    .when_some(video.error, |v, error| {
                        v.child(div().text_sm().child(error))
                    })
                    .when(
                        matches!(video.state.as_str(), "needs_input" | "failed"),
                        |v| {
                            v.child(
                                Button::new(("upload-caption", id as usize))
                                    .h(px(44.))
                                    .label("Add captions")
                                    .disabled(self.source_busy)
                                    .on_click(cx.listener(move |view, _, window, cx| {
                                        view.choose_input_file(Some(id), window, cx)
                                    })),
                            )
                        },
                    )
                    .when(local_captions_missing, |v| {
                        v.child(
                            Button::new(("setup-speech", id as usize))
                                .h(px(44.))
                                .primary()
                                .label(self.speech_setup_label())
                                .disabled(self.speech_downloading || self.source_busy)
                                .on_click(cx.listener(|view, _, window, cx| {
                                    view.set_up_speech(window, cx)
                                })),
                        )
                    })
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .when(
                                video.state != "running"
                                    && !matches!(video.state.as_str(), "complete" | "completed"),
                                |v| {
                                    v.child(
                                        Button::new(("retry", id as usize))
                                            .h(px(44.))
                                            .label(if video.state == "discovered" {
                                                "Make clips"
                                            } else {
                                                "Retry"
                                            })
                                            .disabled(self.source_busy)
                                            .on_click(cx.listener(move |view, _, window, cx| {
                                                view.act(Action::Retry(id), window, cx)
                                            })),
                                    )
                                },
                            )
                            .when(
                                video.state != "running" && video.transcript_language.is_some()
                                    || matches!(video.state.as_str(), "complete" | "completed"),
                                |v| {
                                    v.child(
                                        Button::new(("export-transcript", id as usize))
                                            .h(px(44.))
                                            .label("Export captions")
                                            .disabled(self.source_busy)
                                            .on_click(cx.listener(move |view, _, window, cx| {
                                                view.act(Action::ExportTranscript(id), window, cx)
                                            })),
                                    )
                                },
                            )
                            .when(video.state != "running", |v| {
                                v.child(
                                    Button::new(("save-original", id as usize))
                                        .h(px(44.))
                                        .label("Save original")
                                        .disabled(self.source_busy)
                                        .on_click(cx.listener(move |view, _, window, cx| {
                                            view.act(Action::SaveOriginal(id), window, cx)
                                        })),
                                )
                            })
                            .when(video.state == "running" || video.state == "queued", |v| {
                                v.child(
                                    Button::new(("cancel-job", id as usize))
                                        .h(px(44.))
                                        .label("Cancel")
                                        .disabled(self.source_busy)
                                        .on_click(cx.listener(move |view, _, window, cx| {
                                            view.act(Action::Cancel(id), window, cx)
                                        })),
                                )
                            }),
                    )
            }))
            .into_any_element()
    }

    pub(super) fn ai_card(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = crate::theme::palette(cx);
        let unavailable = self.backend.is_none() || self.source_busy;
        let saved = self
            .engine_snapshot
            .as_ref()
            .map(|s| s.ai.provider)
            .unwrap_or_default();
        let provider = self.ai_provider.unwrap_or(saved);
        // Only the presence of a key is checked; the key itself never reaches the view.
        let key_saved = provider.uses_api_key()
            && match self.ai_key_saved.get() {
                Some((cached, saved)) if cached == provider => saved,
                _ => {
                    let saved = tehom_automation::api_key(provider).ok().flatten().is_some();
                    self.ai_key_saved.set(Some((provider, saved)));
                    saved
                }
            };

        div()
                    .p_5()
                    .bg(p.background)
                    .border_1()
                    .border_color(p.border)
                    .rounded(px(12.))
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(div().text_lg().child("AI"))
                    .child(div().text_sm().text_color(p.muted).child(
                        "Choose who picks the clips: an API key, your Codex or Claude Code subscription, or a model on this computer.",
                    ))
                    .child(div().flex().flex_wrap().gap_2().children(
                        AiProvider::ALL.into_iter().enumerate().map(|(index, option)| {
                            Button::new(("ai-provider", index))
                                .h(px(44.))
                                .label(option.label())
                                .when(provider == option, |button| button.primary())
                                .disabled(unavailable)
                                .on_click(cx.listener(move |view, _, window, cx| {
                                    view.ai_provider = Some(option);
                                    view.ai_key_saved.set(None);
                                    view.model_query
                                        .update(cx, |state, cx| state.set_value("", window, cx));
                                    view.ai_model.update(cx, |state, cx| {
                                        state.set_value(option.default_model(), window, cx)
                                    });
                                    cx.notify();
                                }))
                        }),
                    ))
                    .when(!provider.is_remote(), |card| {
                        card.child(div().text_sm().text_color(p.muted).child(
                            "Captions are sent only to the local model server you choose.",
                        ))
                        .child(
                            Input::new(&self.ai_endpoint)
                                .h(px(44.))
                                .aria_label("Local model server")
                                .disabled(unavailable),
                        )
                        .child(self.model_picker(provider, unavailable, cx))
                        .child(
                            div()
                                .flex()
                                .gap_2()
                                .child(
                                    Button::new("save-ai")
                                        .h(px(44.))
                                        .primary()
                                        .label("Save model")
                                        .disabled(unavailable)
                                        .on_click(cx.listener(|view, _, window, cx| {
                                            view.act(
                                                Action::Ai(AiConfig {
                                                    endpoint: view
                                                        .ai_endpoint
                                                        .read(cx)
                                                        .value()
                                                        .to_string(),
                                                    model: view.ai_model.read(cx).value().to_string(),
                                                    ..Default::default()
                                                }),
                                                window,
                                                cx,
                                            )
                                        })),
                                ),
                        )
                    })
                    .when(provider.program().is_some(), |card| {
                        card.child(div().text_sm().text_color(p.muted).child(format!(
                            "Uses the {} CLI you are signed in to on this computer, {}. No API key is needed. Leave the model empty for its default.",
                            provider.label(),
                            if provider == AiProvider::ClaudeCode {
                                "with no tools enabled"
                            } else {
                                "in its read-only sandbox"
                            }
                        )))
                    })
                    .when(provider.is_remote(), |card| {
                        card.child(self.model_picker(provider, unavailable, cx))
                        .when(provider.uses_api_key(), |card| {
                            card.child(
                                Input::new(&self.ai_key)
                                    .h(px(44.))
                                    .aria_label("API key")
                                    .disabled(unavailable),
                            )
                        })
                        .when(provider.uses_api_key(), |card| card.child(div().text_sm().text_color(p.muted).child(if key_saved {
                            format!("A {} key is saved. Paste a new one to replace it.", provider.label())
                        } else {
                            "Your key is stored in the system keyring, never in Tehom's files.".to_string()
                        })))
                        .child(div().text_sm().child(format!(
                            "Captions of each video are sent to {} to choose clips. Videos stay on this computer.",
                            provider.label()
                        )))
                        .child(
                            Button::new("save-byok")
                                .h(px(44.))
                                .primary()
                                .label(format!("Allow and use {}", provider.label()))
                                .disabled(unavailable)
                                .on_click(cx.listener(move |view, _, window, cx| {
                                    let key = view.ai_key.read(cx).value().to_string();
                                    view.ai_key.update(cx, |state, cx| state.set_value("", window, cx));
                                    view.ai_key_saved.set(None);
                                    view.act(
                                        Action::Byok(
                                            AiConfig {
                                                model: view.ai_model.read(cx).value().to_string(),
                                                provider,
                                                remote_consent: true,
                                                ..Default::default()
                                            },
                                            key,
                                        ),
                                        window,
                                        cx,
                                    )
                                })),
                        )
                    })
            .into_any_element()
    }

    /// The Settings page's children; their order matches the `CARD_*` indexes.
    fn settings(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let p = crate::theme::palette(cx);
        let unavailable = self.backend.is_none() || self.source_busy;
        vec![
            div().text_3xl().child("Settings").into_any_element(),
            self.ai_card(cx),
            self.default_look_card(cx),
            self.speech_card(cx),
            self.instagram_card(cx),
            self.automation_card(cx),
            div()
                .p_5()
                .bg(p.background)
                .border_1()
                .border_color(p.border)
                .rounded(px(12.))
                .flex()
                .flex_col()
                .gap_3()
                .child(div().text_lg().child("Clip folder"))
                .when_some(
                    self.engine_snapshot
                        .as_ref()
                        .map(|s| s.export_directory.display().to_string()),
                    |v, path| v.child(div().text_sm().child(path)),
                )
                .child(
                    Button::new("choose-export")
                        .h(px(44.))
                        .label("Choose folder")
                        .disabled(unavailable)
                        .on_click(cx.listener(|view, _, window, cx| {
                            view.choose_export_directory(window, cx)
                        })),
                )
                .child(div().text_lg().child("Clip shape"))
                .child(div().flex().gap_2().children(
                    tehom_core::ExportProfile::ALL.into_iter().enumerate().map(
                        |(index, profile)| {
                            let selected = self
                                .engine_snapshot
                                .as_ref()
                                .is_some_and(|s| s.export_profile == profile);
                            Button::new(("export-profile", index))
                                .h(px(44.))
                                .label(match profile {
                                    tehom_core::ExportProfile::Vertical => "Vertical 1080 × 1920",
                                    tehom_core::ExportProfile::Horizontal => {
                                        "Horizontal 1920 × 1080"
                                    }
                                    tehom_core::ExportProfile::Original => "Original size",
                                })
                                .disabled(unavailable)
                                .when(selected, |button| button.bg(p.surface).text_color(p.text))
                                .on_click(cx.listener(move |view, _, window, cx| {
                                    view.act(Action::ExportProfile(profile), window, cx)
                                }))
                        },
                    ),
                ))
                .into_any_element(),
            self.appearance_card(cx),
        ]
    }
}

impl Render for Tehom {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.check_onboarding();
        self.maybe_check_updates(window, cx);
        self.maybe_list_models(window, cx);
        self.maybe_check_speech(window, cx);
        self.sync_studio(window, cx);
        self.sync_prefs(window, cx);
        let update_tag = self
            .available_update()
            .map(|status| status.latest.tag.clone());
        let p = crate::theme::palette(cx);
        let menu = SidebarMenu::new().children(
            [
                (Page::Home, "Home", IconName::House),
                (Page::Sources, "Sources", IconName::Rss),
                (Page::Clips, "Clips", IconName::Scissors),
                (Page::Settings, "Settings", IconName::Settings),
                (Page::About, "About Tehom", IconName::Info),
            ]
            .into_iter()
            .map(|(page, label, icon)| {
                SidebarMenuItem::new(label)
                    .h(px(48.))
                    .text_lg()
                    .accessibility_label(label)
                    .icon(icon)
                    .active(self.page == page)
                    .on_click(cx.listener(move |view, _, _, cx| view.navigate(page, cx)))
            }),
        );
        let sidebar = Sidebar::<SidebarMenu>::new("sidebar")
            .collapsible(false)
            .w(px(self.sidebar_px()))
            .bg(p.background)
            .header(
                div().p_4().flex().flex_col().gap_6().child(
                    div()
                        .flex()
                        .items_center()
                        .gap_3()
                        .child(
                            svg()
                                .path("brand/icon.svg")
                                .size(px(30.))
                                .text_color(p.text),
                        )
                        .child(div().text_xl().child("Tehom")),
                ),
            )
            .child(menu)
            .footer(
                div()
                    .p_4()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .when_some(update_tag, |footer, tag| {
                        footer.child(
                            Button::new("update-banner")
                                .h(px(44.))
                                .primary()
                                .label(format!("Update to {tag}"))
                                .on_click(
                                    cx.listener(|view, _, _, cx| view.navigate(Page::About, cx)),
                                ),
                        )
                    })
                    .child(div().text_xs().text_color(p.muted).child(format!(
                        "Tehom {}",
                        tehom_automation::update::CURRENT_VERSION
                    )))
                    .child(
                        Button::new("local-video")
                            .h(px(44.))
                            .ghost()
                            .label("Open a video")
                            .icon(IconName::FolderOpen)
                            .disabled(self.busy)
                            .on_click(cx.listener(|view, _, window, cx| view.pick(window, cx))),
                    ),
            );
        let content = match self.page {
            Page::Clips if self.source.is_some() || self.busy => self.render_editor(cx),
            Page::Settings => div()
                .id("settings-scroll")
                .flex_1()
                .min_w_0()
                .h_full()
                .overflow_y_scroll()
                .track_scroll(&self.settings_scroll)
                .px(px(40.))
                .py(px(32.))
                .flex()
                .flex_col()
                .gap_4()
                .children(self.settings(cx))
                .into_any_element(),
            Page::About => div()
                .id("about-scroll")
                .flex_1()
                .min_w_0()
                .h_full()
                .overflow_y_scroll()
                .px(px(40.))
                .py(px(32.))
                .flex()
                .flex_col()
                .gap_4()
                .child(div().text_3xl().child("About Tehom"))
                .child(self.about_card(cx))
                .into_any_element(),
            Page::Home | Page::Sources => div()
                .id("sources-scroll")
                .flex_1()
                .min_w_0()
                .h_full()
                .overflow_y_scroll()
                .px(px(40.))
                .py(px(32.))
                .flex()
                .flex_col()
                .gap(px(32.))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(div().text_3xl().child(if self.page == Page::Home {
                            "Make clips from a channel"
                        } else {
                            "Sources"
                        }))
                        .child(
                            Button::new("check-sources")
                                .h(px(44.))
                                .label("Check now")
                                .disabled(self.backend.is_none() || self.source_busy)
                                .on_click(cx.listener(|view, _, window, cx| {
                                    view.act(Action::Check, window, cx)
                                })),
                        ),
                )
                .child(
                    div()
                        .text_color(p.muted)
                        .child("Add a channel or RSS feed once and Tehom follows its new videos."),
                )
                .child(self.source_form(cx))
                .child(self.source_list(cx))
                .when(self.page == Page::Home, |v| {
                    v.child(div().text_lg().child("Recent jobs"))
                        .child(self.job_list(cx))
                })
                .into_any_element(),
            Page::Clips => {
                let clips = self
                    .engine_snapshot
                    .as_ref()
                    .map(|s| s.clips.clone())
                    .unwrap_or_default();
                div()
                    .id("clips-scroll")
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .overflow_y_scroll()
                    .p_6()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .child(div().text_3xl().child("Clips"))
                    .when(clips.is_empty(), |v| {
                        v.child("Your clips will appear here.")
                    })
                    .children(clips.into_iter().map(|clip| {
                        let path = clip.output_path.clone().filter(|_| clip.output_available);
                        let export_missing = clip.output_path.is_some() && !clip.output_available;
                        let video = self.engine_snapshot.as_ref().and_then(|snapshot| {
                            snapshot
                                .videos
                                .iter()
                                .find(|video| video.id == clip.video_id)
                        });
                        let original_missing = video
                            .is_some_and(|video| video.local_path.is_some() && !video.local_available);
                        let original = video
                            .filter(|video| video.local_available)
                            .and_then(|video| video.local_path.clone());
                        let range = (clip.start_ms, clip.end_ms);
                        let publish = clip.clone();
                        div()
                            .p_4()
                            .rounded(px(12.))
                            .bg(p.background).border_1().border_color(p.border)
                            .flex()
                            .flex_col()
                            .gap_2()
                            .child(clip.title)
                            .child(format!(
                                "{:.1}–{:.1} sn · {:.1} saniye",
                                clip.start_ms as f64 / 1000.,
                                clip.end_ms as f64 / 1000.,
                                (clip.end_ms - clip.start_ms) as f64 / 1000.
                            ))
                            .when(original_missing, |v| {
                                v.child(div().text_sm().text_color(p.text).child(
                                    "The original video is missing, so this draft cannot be edited. Put the file back and try again.",
                                ))
                            })
                            .when(export_missing, |v| {
                                v.child(div().text_sm().text_color(p.text).child(
                                    "The exported file was moved or deleted.",
                                ))
                            })
                            .when_some(original, |v, original| {
                                v.child(
                                    Button::new(("draft-edit", clip.id as usize)).h(px(44.))
                                        .label("Edit draft")
                                        .disabled(self.busy || self.source_busy)
                                        .on_click(cx.listener(move |view, _, window, cx| {
                                            view.load(original.clone(), window, cx);
                                            view.pending_range = Some(range);
                                            view.editing_clip = Some(clip.id);
                                        })),
                                )
                            })
                            .when_some(path, |v, path| {
                                v.child(
                                    Button::new(("clip-open", clip.id as usize)).h(px(44.))
                                        .label("Open clip")
                                        .on_click(cx.listener(move |view, _, window, cx| {
                                            view.load(path.clone(), window, cx)
                                        })),
                                )
                                .child(self.publish_row(&publish, cx))
                            })
                    }))
                    .child(self.job_list(cx))
                    .into_any_element()
            }
        };
        let overlay = self.overlay(cx);
        div()
            .size_full()
            .relative()
            .track_focus(&self.focus)
            .key_context("Tehom")
            .on_action(cx.listener(Self::toggle_palette))
            .on_action(cx.listener(Self::close_palette))
            .on_drag_move(cx.listener(Self::on_sidebar_drag))
            .on_drop(cx.listener(|view, _: &crate::prefs::ResizeSidebar, _, cx| {
                view.resize_sidebar(view.sidebar_px(), true, cx)
            }))
            .flex()
            .bg(p.background)
            .text_color(p.text)
            .child(
                div()
                    .relative()
                    .h_full()
                    .flex_none()
                    .child(sidebar)
                    .child(self.sidebar_edge(cx)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .flex()
                    .flex_col()
                    .child(content)
                    .when(self.page != Page::Clips, |v| {
                        v.child(
                            div()
                                .p_4()
                                .border_t_1()
                                .border_color(p.border)
                                .text_sm()
                                .child(self.status.clone()),
                        )
                    })
                    .when(self.details.is_some() && self.page != Page::Clips, |v| {
                        v.child(
                            div()
                                .px_4()
                                .pb_4()
                                .text_sm()
                                .text_color(p.text)
                                .child(self.details.clone().unwrap_or_default()),
                        )
                    }),
            )
            .children(overlay)
    }
}

/// A coloured badge with the platform name, so the source is recognisable at a glance.
fn platform_badge(source: &tehom_core::Source) -> Option<Div> {
    let (glyph, name, background, foreground) = if source.url.contains("kick.com") {
        ("brand/kick.svg", "Kick", 0x53FC18, 0x0B0E0F)
    } else if source.url.contains("youtube.com") {
        ("brand/youtube.svg", "YouTube", 0xFF0000, 0xFFFFFF)
    } else {
        return None;
    };
    Some(
        div()
            .flex()
            .items_center()
            .gap_2()
            .child(
                div()
                    .size(px(28.))
                    .rounded(px(6.))
                    .bg(rgb(background))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(svg().path(glyph).size(px(22.)).text_color(rgb(foreground))),
            )
            .child(div().text_sm().child(name)),
    )
}
