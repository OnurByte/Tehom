use super::*;
use gpui_kit::assets::IconName;
use gpui_kit::component::sidebar::{Sidebar, SidebarMenu};
use tehom_automation::Automation;
use tehom_core::AiConfig;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Page {
    Home,
    Sources,
    Clips,
    Settings,
}

pub(super) struct Backend {
    runtime: tokio::runtime::Runtime,
    engine: Automation,
}

enum Action {
    Add(String, AutomationMode),
    File(PathBuf, AutomationMode),
    Check,
    Mode(i64, AutomationMode),
    Resume(i64),
    Ai(AiConfig),
    Models,
    SaveRevision(i64, u64, u64),
    Retry(i64),
    Import(i64, PathBuf),
    Cancel(i64),
    SaveOriginal(i64),
    ExportDirectory(PathBuf),
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
                .ok_or("Veri klasörü belirlenemedi")?;
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
                        view.status = "Kaynaklar açılamadı. Ayrıntıları kontrol et.".into();
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
                    view.status = "Bir kanal ekleyerek otomasyonu başlatabilirsin.".into();
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

    fn act(&mut self, action: Action, window: &Window, cx: &mut Context<Self>) {
        let Some(backend) = self.backend.clone() else {
            return;
        };
        if self.source_busy {
            return;
        }
        self.source_busy = true;
        self.details = None;
        self.status = "İşleniyor…".into();
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
                                "Video kaydedildi. İş durumunu aşağıdan takip edebilirsin.".into()
                            } else if source.error.is_some() {
                                "Kanal kaydedildi. Akışa erişim bekleniyor; otomatik yeniden denenecek.".into()
                            } else {"Kanal eklendi. Son video ve bundan sonraki yeni videolar izleniyor.".into()}
                        }
                        Action::File(path, mode) => {
                            backend.engine.import_video(path, mode).await?;
                            "Video kaydedildi. Altyazı gerekiyorsa iş listesinden yükleyebilirsin.".into()
                        }
                        Action::Check => format!(
                            "Kaynaklar kontrol edildi. {} yeni video bulundu.",
                            backend.engine.check_now().await?
                        ),
                        Action::Mode(id, mode) => {
                            backend.engine.set_mode(id, mode).await?;
                            "Otomasyon modu güncellendi.".into()
                        }
                        Action::Resume(id) => {
                            backend.engine.resume_source(id).await?;
                            "Otomasyon devam ediyor.".into()
                        }
                        Action::Ai(ai) => {
                            backend.engine.set_ai(ai).await?;
                            "Model ayarları kaydedildi.".into()
                        }
                        Action::SaveRevision(id, start, end) => {
                            backend.engine.save_clip_revision(id, start, end).await?;
                            "Yeni taslak kaydedildi. Önceki taslak ve dosyalar korundu.".into()
                        }
                        Action::Models => {
                            models = Some(backend.engine.local_models().await?);
                            "Yerel modeller kontrol edildi.".into()
                        }
                        Action::Import(id,path)=>{backend.engine.import_transcript(id,path,None).await?;"Altyazı kaydedildi. Yeniden dene ile klipleri oluşturabilirsin.".into()}
                        Action::Retry(id) => {
                            backend.engine.retry_video(id).await?;
                            "Video yeniden kuyruğa alındı.".into()
                        }
                        Action::Cancel(id) => {
                            backend.engine.cancel_video(id).await?;
                            "Video işi iptal edildi.".into()
                        }
                        Action::SaveOriginal(id) => {
                            let folder = backend.engine.snapshot().await?.export_directory;
                            let saved = backend.engine.save_original(id, folder).await?;
                            format!("Orijinal video kaydedildi: {}", saved.display())
                        }
                        Action::ExportDirectory(path) => {
                            backend.engine.set_export_directory(path).await?;
                            "Kayıt klasörü değiştirildi.".into()
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
                            "İşlem tamamlanamadı. Bağlantıyı ve ayarları kontrol et.".into();
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
            self.status = "Videonun içinde geçerli başlangıç ve bitiş saniyeleri yaz.".into();
            cx.notify();
        }
    }

    fn choose_input_file(
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
                    "SRT veya VTT altyazı dosyası seç"
                } else {
                    "Klip oluşturmak için video seç"
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
                    view.status = "Dosya seçilemedi.".into();
                    view.details = Some(format!("{error:?}"));
                    cx.notify();
                });
            }
        })
        .detach();
    }

    fn choose_export_directory(&mut self, window: &Window, cx: &mut Context<Self>) {
        let picker = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Kliplerin kaydedileceği klasör".into()),
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

    fn navigate(&mut self, page: Page, cx: &mut Context<Self>) {
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
        let unavailable = self.backend.is_none() || self.source_busy;
        div().flex().flex_col().gap_4().p_5().bg(rgb(0x262626)).rounded_lg()
            .child(div().text_lg().child("YouTube kanalını veya videonu ekle"))
            .child(Input::new(&self.source_url).id("channel-url").aria_label("YouTube kanal veya video adresi").disabled(unavailable))
            .child(div().flex().gap_2()
                .child(Button::new("mode-draft").label("Otomatik taslak").when(self.source_mode==AutomationMode::AutoDraft,|b|b.primary())
                    .on_click(cx.listener(|view,_,_,cx|{view.source_mode=AutomationMode::AutoDraft;cx.notify();})))
                .child(Button::new("mode-export").label("Otomatik MP4").when(self.source_mode==AutomationMode::AutoExport,|b|b.primary())
                    .on_click(cx.listener(|view,_,_,cx|{view.source_mode=AutomationMode::AutoExport;cx.notify();}))))
            .child(div().text_sm().text_color(rgb(0xdde1e6)).child(match self.source_mode {AutomationMode::AutoExport=>"Klipler seçtiğin klasöre kaydedilir. Kanal eklersen yeni videolar da otomatik işlenir.",_=>"Klip taslakları hazırlanır. Kanal eklersen yeni videolar da izlenir."}))
            .child(Button::new("add-channel").primary().label(if self.source_busy {"Ekleniyor…"} else {"Bağlantıyı ekle"}).disabled(unavailable)
                .on_click(cx.listener(|view,_,window,cx|view.act(Action::Add(view.source_url.read(cx).value().to_string(),view.source_mode),window,cx))))
            .child(Button::new("import-local-video").label("Dosyadan klip oluştur").disabled(unavailable)
                .on_click(cx.listener(|view,_,window,cx| view.choose_input_file(None, window, cx))))
            .child(div().text_sm().text_color(rgb(0xdde1e6)).child("Özgün dosya yerinde kullanılır. İş bitene kadar taşımayın veya silmeyin."))
            .into_any_element()
    }

    fn source_list(&self, cx: &mut Context<Self>) -> AnyElement {
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
                v.child(
                    div()
                        .p_5()
                        .text_color(rgb(0xdde1e6))
                        .child("Henüz bir kaynak eklenmedi."),
                )
            })
            .children(sources.into_iter().map(|source| {
                let id = source.id;
                div()
                    .p_4()
                    .bg(rgb(0x262626))
                    .rounded_lg()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(div().text_sm().text_color(rgb(0xdde1e6)).child(
                        if source.channel_id.starts_with("local:") {
                            "Yerel video"
                        } else if source.channel_id.starts_with("video:") {
                            "Tek video"
                        } else {
                            "Kanal · yeni videolar izleniyor"
                        },
                    ))
                    .child(div().text_lg().child(source.title))
                    .child(div().text_sm().text_color(rgb(0xdde1e6)).child(source.url))
                    .child(div().text_sm().child(match source.mode {
                        AutomationMode::Paused => "Duraklatıldı",
                        AutomationMode::AutoExport => "Otomatik MP4",
                        AutomationMode::AutoDraft => "Otomatik taslak",
                        AutomationMode::Manual => "Manuel",
                    }))
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(
                                Button::new(("source-pause", id as usize))
                                    .label(if source.mode == AutomationMode::Paused {
                                        "Devam et"
                                    } else {
                                        "Duraklat"
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
                                    .label("Otomatik MP4")
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
                        v.child(div().text_sm().text_color(rgb(0xee5396)).child(error))
                    })
            }))
            .into_any_element()
    }

    fn job_list(&self, cx: &mut Context<Self>) -> AnyElement {
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
                        .text_color(rgb(0xdde1e6))
                        .child("Yeni videolar burada görünecek."),
                )
            })
            .children(videos.into_iter().take(20).map(|video| {
                let id = video.id;
                div()
                    .p_4()
                    .bg(rgb(0x262626))
                    .rounded_lg()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(video.title)
                    .child(div().text_sm().text_color(rgb(0x78a9ff)).child(
                        match video.state.as_str() {
                            "discovered" => "İşlenmedi",
                            "queued" => "Sırada",
                            "running" => "Klipler hazırlanıyor",
                            "complete" | "completed" => "Tamamlandı",
                            "needs_input" => "İnceleme gerekiyor",
                            "cancelled" => "İptal edildi",
                            "retryable" => "Yeniden denenebilir",
                            _ => "İşlem durdu",
                        },
                    ))
                    .when_some(video.error, |v, error| {
                        v.child(div().text_sm().child(error))
                    })
                    .when(
                        matches!(video.state.as_str(), "needs_input" | "failed"),
                        |v| {
                            v.child(
                                Button::new(("upload-caption", id as usize))
                                    .label("Altyazı yükle")
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
                                            .label(if video.state == "discovered" {
                                                "Klip oluştur"
                                            } else {
                                                "Yeniden dene"
                                            })
                                            .disabled(self.source_busy)
                                            .on_click(cx.listener(move |view, _, window, cx| {
                                                view.act(Action::Retry(id), window, cx)
                                            })),
                                    )
                                },
                            )
                            .when(video.state != "running", |v| {
                                v.child(
                                    Button::new(("save-original", id as usize))
                                        .label("Orijinali kaydet")
                                        .disabled(self.source_busy)
                                        .on_click(cx.listener(move |view, _, window, cx| {
                                            view.act(Action::SaveOriginal(id), window, cx)
                                        })),
                                )
                            })
                            .when(video.state == "running" || video.state == "queued", |v| {
                                v.child(
                                    Button::new(("cancel-job", id as usize))
                                        .label("İptal et")
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

    fn settings(&self, cx: &mut Context<Self>) -> AnyElement {
        let unavailable = self.backend.is_none() || self.source_busy;
        div()
            .flex()
            .flex_col()
            .gap_4()
            .child(div().text_2xl().child("Ayarlar"))
            .child(
                div()
                    .p_5()
                    .bg(rgb(0x262626))
                    .rounded_lg()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(div().text_lg().child("Yerel AI modeli"))
                    .child(
                        div()
                            .text_sm()
                            .child("Analiz içeriği seçtiğin yerel model sunucusuna gönderilir."),
                    )
                    .child(
                        Input::new(&self.ai_endpoint)
                            .aria_label("Yerel model sunucusu")
                            .disabled(unavailable),
                    )
                    .child(
                        Input::new(&self.ai_model)
                            .aria_label("AI modeli")
                            .disabled(unavailable),
                    )
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(
                                Button::new("save-ai")
                                    .primary()
                                    .label("Modeli kaydet")
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
                                            }),
                                            window,
                                            cx,
                                        )
                                    })),
                            )
                            .child(
                                Button::new("find-models")
                                    .label("Yerel modelleri bul")
                                    .disabled(unavailable)
                                    .on_click(cx.listener(|view, _, window, cx| {
                                        view.act(Action::Models, window, cx)
                                    })),
                            ),
                    )
                    .children(
                        self.available_models
                            .iter()
                            .enumerate()
                            .map(|(index, model)| {
                                let value = model.clone();
                                Button::new(("model", index)).label(model.clone()).on_click(
                                    cx.listener(move |view, _, window, cx| {
                                        view.ai_model.update(cx, |state, cx| {
                                            state.set_value(value.clone(), window, cx)
                                        });
                                    }),
                                )
                            }),
                    ),
            )
            .child(
                div()
                    .p_5()
                    .bg(rgb(0x262626))
                    .rounded_lg()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(div().text_lg().child("Klip kayıt klasörü"))
                    .when_some(
                        self.engine_snapshot
                            .as_ref()
                            .map(|s| s.export_directory.display().to_string()),
                        |v, path| v.child(div().text_sm().child(path)),
                    )
                    .child(
                        Button::new("choose-export")
                            .label("Klasör seç")
                            .disabled(unavailable)
                            .on_click(cx.listener(|view, _, window, cx| {
                                view.choose_export_directory(window, cx)
                            })),
                    ),
            )
            .into_any_element()
    }
}

impl Render for Tehom {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let menu = div().flex().flex_col().gap_1().children(
            [
                (Page::Home, "Başlangıç", IconName::House),
                (Page::Sources, "Kaynaklar", IconName::Rss),
                (Page::Clips, "Klipler", IconName::Scissors),
                (Page::Settings, "Ayarlar", IconName::Settings),
            ]
            .into_iter()
            .enumerate()
            .map(|(index, (page, label, icon))| {
                Button::new(("nav", index))
                    .ghost()
                    .label(label)
                    .icon(icon)
                    .w_full()
                    .justify_start()
                    .when(self.page == page, |button| {
                        button.bg(rgb(0x30283d)).text_color(rgb(0xbe95ff))
                    })
                    .on_click(cx.listener(move |view, _, _, cx| view.navigate(page, cx)))
            }),
        );
        let sidebar = Sidebar::<SidebarMenu>::new("sidebar")
            .collapsible(false)
            .w(px(220.))
            .bg(rgb(0x1c1c1c))
            .header(
                div()
                    .p_4()
                    .flex()
                    .flex_col()
                    .gap_6()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_3()
                            .child(
                                svg()
                                    .path("brand/icon.svg")
                                    .size(px(30.))
                                    .text_color(rgb(0xf2f4f8)),
                            )
                            .child(div().text_xl().child("Tehom")),
                    )
                    .child(menu),
            )
            .footer(
                div()
                    .p_4()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(0x42be65))
                            .child("●  Yerel çalışma"),
                    )
                    .child(
                        Button::new("local-video")
                            .ghost()
                            .label("Video dosyası aç")
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
                .p_6()
                .child(self.settings(cx))
                .into_any_element(),
            Page::Home | Page::Sources => div()
                .id("sources-scroll")
                .flex_1()
                .min_w_0()
                .h_full()
                .overflow_y_scroll()
                .p_6()
                .flex()
                .flex_col()
                .gap_6()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(div().text_2xl().child(if self.page == Page::Home {
                            "Kanalından klipler oluştur"
                        } else {
                            "Kaynaklar"
                        }))
                        .child(
                            Button::new("check-sources")
                                .label("Şimdi kontrol et")
                                .disabled(self.backend.is_none() || self.source_busy)
                                .on_click(cx.listener(|view, _, window, cx| {
                                    view.act(Action::Check, window, cx)
                                })),
                        ),
                )
                .child(div().text_color(rgb(0xdde1e6)).child(
                    "Kanalını bir kez ekle. Tehom yeni videoları RSS üzerinden takip etsin.",
                ))
                .child(self.source_form(cx))
                .child(self.source_list(cx))
                .when(self.page == Page::Home, |v| {
                    v.child(div().text_lg().child("Son işler"))
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
                    .child(div().text_2xl().child("Klipler"))
                    .when(clips.is_empty(), |v| {
                        v.child("Hazırlanan klipler burada görünecek.")
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
                            .rounded_lg()
                            .bg(rgb(0x262626))
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
                                v.child(div().text_sm().text_color(rgb(0xee5396)).child(
                                    "Özgün video bulunamadı; taslak düzenlenemez. Dosyayı geri koyup yeniden dene.",
                                ))
                            })
                            .when(export_missing, |v| {
                                v.child(div().text_sm().text_color(rgb(0xee5396)).child(
                                    "Dışa aktarılan dosya taşınmış veya silinmiş.",
                                ))
                            })
                            .when_some(original, |v, original| {
                                v.child(
                                    Button::new(("draft-edit", clip.id as usize))
                                        .label("Taslağı düzenle")
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
                                    Button::new(("clip-open", clip.id as usize))
                                        .label("Klibi aç")
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
        div()
            .size_full()
            .flex()
            .bg(rgb(0x161616))
            .text_color(rgb(0xf2f4f8))
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
                                .border_color(rgb(0x393939))
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
                                .text_color(rgb(0xee5396))
                                .child(self.details.clone().unwrap_or_default()),
                        )
                    }),
            )
    }
}
