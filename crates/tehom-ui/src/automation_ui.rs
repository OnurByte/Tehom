use super::*;
use gpui_kit::assets::IconName;
use gpui_kit::component::sidebar::{Sidebar, SidebarMenu, SidebarMenuItem};
use tehom_automation::Automation;
use tehom_core::{AiConfig, AiProvider};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Page {
    Home,
    Sources,
    Clips,
    Settings,
}

pub(super) struct Backend {
    runtime: tokio::runtime::Runtime,
    engine: Automation,
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
    Models,
    SaveRevision(i64, u64, u64),
    Retry(i64),
    Import(i64, PathBuf),
    Cancel(i64),
    SaveOriginal(i64),
    ExportTranscript(i64),
    ExportDirectory(PathBuf),
    ExportProfile(tehom_core::ExportProfile),
    Onboarded,
}

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
                    }
                    view.status = "Add a channel to start.".into();
                    cx.notify();
                })
                .is_err()
            {
                return;
            }
            let mut last_check = std::time::Instant::now() - Duration::from_secs(1801);
            while !stop.load(Ordering::Acquire) {
                let check = last_check.elapsed() >= Duration::from_secs(1800);
                if check {
                    last_check = std::time::Instant::now();
                }
                let worker = backend.clone();
                let result = cx
                    .background_spawn(async move {
                        worker
                            .runtime
                            .block_on(async {
                                if check {
                                    worker.engine.check_now().await?;
                                }
                                worker.engine.process_next().await
                            })
                            .map_err(|e| e.to_string())
                    })
                    .await;
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
        let task = cx.background_spawn(async move {
            backend
                .runtime
                .block_on(async {
                    let mut models = None;
                    let message = match action {
                        Action::Add(url, mode) => {
                            let source=backend.engine.add_source(&url,mode).await?;
                            if source.channel_id.starts_with("video:") {
                                "Video added. Follow its progress below.".into()
                            } else if source.error.is_some() {
                                "Channel saved. Its feed is not reachable yet; Tehom will retry.".into()
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
                            if !key.trim().is_empty() {
                                tokio::task::spawn_blocking(move || {
                                    tehom_automation::store_api_key(provider, &key)
                                })
                                .await??;
                            }
                            anyhow::ensure!(
                                tehom_automation::api_key(provider)?.is_some(),
                                "Paste your {} API key first",
                                provider.label()
                            );
                            backend.engine.set_ai(ai).await?;
                            format!("{} is ready. Captions will be sent there for analysis.", provider.label())
                        }
                        Action::SaveRevision(id, start, end) => {
                            backend.engine.save_clip_revision(id, start, end).await?;
                            "New draft saved. Earlier drafts and files are kept.".into()
                        }
                        Action::Models => {
                            models = Some(backend.engine.local_models().await?);
                            "Local models checked.".into()
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
                    };
                    Ok::<_, anyhow::Error>((backend.engine.snapshot().await?, message, models))
                })
                .map_err(|e| e.to_string())
        });
        cx.spawn_in(window, async move |view, cx| {
            let result = task.await;
            let _ = view.update(cx, |view, cx| {
                view.source_busy = false;
                match result {
                    Ok((snapshot, message, models)) => {
                        view.engine_snapshot = Some(snapshot);
                        view.status = message;
                        if let Some(models) = models {
                            view.available_models = models;
                        }
                    }
                    Err(error) => {
                        view.status =
                            "That did not finish. Check the link and your settings.".into();
                        view.details = Some(error);
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
            .child(div().text_lg().child("Add a YouTube channel or video"))
            .child(Input::new(&self.source_url).h(px(44.)).id("channel-url").aria_label("YouTube channel or video address").disabled(unavailable))
            .child(div().flex().gap_2()
                .child(Button::new("mode-draft").h(px(44.)).label("Drafts").when(self.source_mode==AutomationMode::AutoDraft,|b|b.primary())
                    .on_click(cx.listener(|view,_,_,cx|{view.source_mode=AutomationMode::AutoDraft;cx.notify();})))
                .child(Button::new("mode-export").h(px(44.)).label("MP4 files").when(self.source_mode==AutomationMode::AutoExport,|b|b.primary())
                    .on_click(cx.listener(|view,_,_,cx|{view.source_mode=AutomationMode::AutoExport;cx.notify();}))))
            .child(div().text_sm().text_color(p.muted).child(match self.source_mode {AutomationMode::AutoExport=>"Clips are saved to your folder automatically. Channels are watched for new videos.",_=>"Tehom prepares drafts for you to review. Channels are watched for new videos."}))
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
                    .child(div().text_sm().text_color(p.muted).child(
                        if source.channel_id.starts_with("local:") {
                            "Local file"
                        } else if source.channel_id.starts_with("video:") {
                            "Single video"
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
        let key_saved = provider.is_remote()
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
                        "Choose where clip selection runs: a model on this computer, or your own API key.",
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
                        .child(
                            Input::new(&self.ai_model)
                                .h(px(44.))
                                .aria_label("AI model")
                                .disabled(unavailable),
                        )
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
                                )
                                .child(
                                    Button::new("find-models")
                                        .h(px(44.))
                                        .label("Find local models")
                                        .disabled(unavailable)
                                        .on_click(cx.listener(|view, _, window, cx| {
                                            view.act(Action::Models, window, cx)
                                        })),
                                ),
                        )
                    })
                    .when(provider.is_remote(), |card| {
                        card.child(
                            Input::new(&self.ai_model)
                                .h(px(44.))
                                .aria_label("AI model")
                                .disabled(unavailable),
                        )
                        .child(
                            Input::new(&self.ai_key)
                                .h(px(44.))
                                .aria_label("API key")
                                .disabled(unavailable),
                        )
                        .child(div().text_sm().text_color(p.muted).child(if key_saved {
                            format!("A {} key is saved. Paste a new one to replace it.", provider.label())
                        } else {
                            "Your key is stored in the system keyring, never in Tehom's files.".to_string()
                        }))
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
                    .children(
                        self.available_models
                            .iter()
                            .enumerate()
                            .map(|(index, model)| {
                                let value = model.clone();
                                Button::new(("model", index))
                                    .h(px(44.))
                                    .label(model.clone())
                                    .on_click(cx.listener(move |view, _, window, cx| {
                                        view.ai_model.update(cx, |state, cx| {
                                            state.set_value(value.clone(), window, cx)
                                        });
                                    }))
                            }),
                    )
            .into_any_element()
    }

    fn settings(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = crate::theme::palette(cx);
        let unavailable = self.backend.is_none() || self.source_busy;
        div()
            .flex()
            .flex_col()
            .gap_4()
            .child(div().text_3xl().child("Settings"))
            .child(self.ai_card(cx))
            .child(
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
                                        tehom_core::ExportProfile::Vertical => {
                                            "Vertical 1080 × 1920"
                                        }
                                        tehom_core::ExportProfile::Horizontal => {
                                            "Horizontal 1920 × 1080"
                                        }
                                        tehom_core::ExportProfile::Original => "Original size",
                                    })
                                    .disabled(unavailable)
                                    .when(selected, |button| {
                                        button.bg(p.surface).text_color(p.text)
                                    })
                                    .on_click(cx.listener(move |view, _, window, cx| {
                                        view.act(Action::ExportProfile(profile), window, cx)
                                    }))
                            },
                        ),
                    )),
            )
            .into_any_element()
    }
}

impl Render for Tehom {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.check_onboarding();
        let p = crate::theme::palette(cx);
        let menu = SidebarMenu::new().children(
            [
                (Page::Home, "Home", IconName::House),
                (Page::Sources, "Sources", IconName::Rss),
                (Page::Clips, "Clips", IconName::Scissors),
                (Page::Settings, "Settings", IconName::Settings),
            ]
            .into_iter()
            .map(|(page, label, icon)| {
                SidebarMenuItem::new(label)
                    .accessibility_label(label)
                    .icon(icon)
                    .active(self.page == page)
                    .on_click(cx.listener(move |view, _, _, cx| view.navigate(page, cx)))
            }),
        );
        let sidebar = Sidebar::<SidebarMenu>::new("sidebar")
            .collapsible(false)
            .w(px(220.))
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
                    .child(
                        div()
                            .text_xs()
                            .text_color(p.muted)
                            .child("Runs on this computer"),
                    )
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
                .px(px(40.))
                .py(px(32.))
                .child(self.settings(cx))
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
                        .child("Add a channel once and Tehom follows its new videos."),
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
            .flex()
            .bg(p.background)
            .text_color(p.text)
            .child(sidebar)
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
