mod automation_ui;
use automation_ui::{Backend, Page};
use gpui_kit::TestSupportExt as _;
use gpui_kit::{
    AnyElement, AssetSource, Entity, KeyDownEvent, ObjectFit, PathPromptOptions, RenderImage,
    Result, Role, SharedString, Subscription, Task, Window, assets,
    component::{
        Disableable,
        button::{Button, ButtonVariants},
        input::{Input, InputState},
        slider::{Slider, SliderEvent, SliderState},
    },
    div, img,
    prelude::*,
    px, rgb, svg,
};
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tehom_core::{AutomationMode, EngineSnapshot};
use tehom_media::preview::Preview;

pub struct Tehom {
    page: Page,
    source_url: Entity<InputState>,
    ai_endpoint: Entity<InputState>,
    ai_model: Entity<InputState>,
    source_mode: AutomationMode,
    source_busy: bool,
    backend: Option<Arc<Backend>>,
    engine_snapshot: Option<EngineSnapshot>,
    available_models: Vec<String>,
    engine_stop: Arc<AtomicBool>,
    _engine_task: Option<Task<()>>,
    _snapshot_task: Option<Task<()>>,
    pending_range: Option<(u64, u64)>,
    editing_clip: Option<i64>,
    source: Option<PathBuf>,
    preview: Option<Preview>,
    image: Option<Arc<RenderImage>>,
    start: Entity<InputState>,
    end: Entity<InputState>,
    timeline: Entity<SliderState>,
    timeline_position: f32,
    scrubbing: bool,
    _timeline_events: Subscription,
    _timeline_value: Subscription,
    total: f64,
    metadata: String,
    busy: bool,
    cancellation: Option<Arc<AtomicBool>>,
    status: String,
    details: Option<String>,
    show_details: bool,
    _refresh: Task<()>,
}

impl Tehom {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let source_url = cx.new(|cx| {
            InputState::new(window, cx).placeholder("YouTube kanal veya video bağlantısı")
        });
        let ai_endpoint =
            cx.new(|cx| InputState::new(window, cx).default_value("http://127.0.0.1:11434"));
        let ai_model = cx.new(|cx| InputState::new(window, cx).placeholder("Yerel model adı"));
        let engine_stop = Arc::new(AtomicBool::new(false));
        let start = cx.new(|cx| InputState::new(window, cx).default_value("0"));
        let end = cx.new(|cx| InputState::new(window, cx).default_value("0"));
        let timeline = cx.new(|_| SliderState::new().min(0.).max(100.).step(0.001));
        let timeline_events = cx.subscribe(&timeline, |view, _, event, _| {
            view.scrubbing = matches!(event, SliderEvent::Change(_));
        });
        let timeline_value = cx.observe(&timeline, |view, timeline, cx| {
            let position = timeline.read(cx).value().end();
            if position.is_finite() && position != view.timeline_position && view.total > 0. {
                view.timeline_position = position;
                view.seek(f64::from(position) / 100. * view.total, cx);
            }
        });
        let refresh = cx.spawn_in(window, async move |view, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(33))
                    .await;
                if view
                    .update_in(cx, |view, window, cx| {
                        if let Some(preview) = &view.preview {
                            if let Some(frame) = preview.take_frame()
                                && let Some(buffer) = image::RgbaImage::from_raw(
                                    frame.width,
                                    frame.height,
                                    frame.bgra,
                                )
                            {
                                let image =
                                    Arc::new(RenderImage::new(vec![image::Frame::new(buffer)]));
                                if let Some(previous) = view.image.replace(image)
                                    && let Err(error) = window.drop_image(previous)
                                {
                                    eprintln!("Preview texture: {error}");
                                }
                            }
                            let snapshot = preview.snapshot();
                            if !view.scrubbing && view.total > 0. {
                                let position = (snapshot.position.as_secs_f64() / view.total * 100.)
                                    .clamp(0., 100.)
                                    as f32;
                                if position != view.timeline_position {
                                    view.timeline_position = position;
                                    view.timeline.update(cx, |state, cx| {
                                        state.set_value(position, window, cx)
                                    });
                                }
                            }
                            if let Some(error) = snapshot.error {
                                view.status =
                                    "Video oynatılamadı. Başka bir dosya seçebilirsin.".into();
                                view.details = Some(error);
                            }
                            cx.notify();
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        });
        Self {
            page: Page::Home,
            source_url,
            ai_endpoint,
            ai_model,
            source_mode: AutomationMode::AutoDraft,
            source_busy: false,
            backend: None,
            engine_snapshot: None,
            available_models: Vec::new(),
            engine_stop,
            _engine_task: None,
            _snapshot_task: None,
            pending_range: None,
            editing_clip: None,
            source: None,
            preview: None,
            image: None,
            start,
            end,
            timeline,
            timeline_position: 0.,
            scrubbing: false,
            _timeline_events: timeline_events,
            _timeline_value: timeline_value,
            total: 0.,
            metadata: String::new(),
            busy: false,
            cancellation: None,
            status: "Başlamak için bilgisayarından bir video seç.".into(),
            details: None,
            show_details: false,
            _refresh: refresh,
        }
    }

    pub fn load(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        self.page = Page::Clips;
        self.pending_range = None;
        self.editing_clip = None;
        self.busy = true;
        self.status = "Video açılıyor…".into();
        self.details = None;
        self.preview = None;
        self.source = None;
        self.total = 0.;
        self.timeline_position = 0.;
        self.scrubbing = false;
        self.timeline
            .update(cx, |state, cx| state.set_value(0., window, cx));
        self.metadata.clear();
        if let Some(image) = self.image.take()
            && let Err(error) = window.drop_image(image)
        {
            eprintln!("Preview texture: {error}");
        }
        let task_path = path.clone();
        let task = cx.background_spawn(async move {
            let info = tehom_media::inspect(&task_path)?;
            let preview = Preview::open(&task_path)?;
            Ok::<_, std::io::Error>((info, preview))
        });
        cx.spawn_in(window, async move |view, cx| {
            let result = task.await;
            let _ = view.update_in(cx, |view, window, cx| {
                view.busy = false;
                match result {
                    Ok((info, preview)) => {
                        let duration = info.duration;
                        view.total = duration;
                        view.metadata = format!(
                            "{} · {} × {} · {:.2} saniye",
                            path.file_name().unwrap_or_default().to_string_lossy(),
                            info.width,
                            info.height,
                            duration
                        );
                        view.preview = Some(preview);
                        view.source = Some(path);
                        let (start, end) = view
                            .pending_range
                            .take()
                            .map(|(a, b)| (a as f64 / 1000., b as f64 / 1000.))
                            .unwrap_or((0., duration));
                        view.start.update(cx, |state, cx| {
                            state.set_value(format!("{start:.3}"), window, cx)
                        });
                        view.end.update(cx, |state, cx| {
                            state.set_value(format!("{end:.3}"), window, cx)
                        });
                        if let Some(preview) = &view.preview
                            && let Err(error) = preview.seek(Duration::from_secs_f64(start))
                        {
                            eprintln!("Draft preview: {error}");
                        }
                        view.status = "Hazır. Başlangıç ve bitişi seçip MP4 kaydet.".into();
                    }
                    Err(error) => {
                        view.status =
                            "Video açılamadı. Dosyayı ve video bileşenlerini kontrol et.".into();
                        view.details = Some(error.to_string());
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn pick(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let picker = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Video seç".into()),
        });
        cx.spawn_in(window, async move |view, cx| {
            let result = picker.await;
            let _ = view.update_in(cx, |view, window, cx| match result {
                Ok(Ok(Some(paths))) => {
                    if let Some(path) = paths.into_iter().next() {
                        view.load(path, window, cx);
                    }
                }
                Ok(Ok(None)) => {}
                error => {
                    view.status = "Dosya seçimi açılamadı. Tekrar deneyebilirsin.".into();
                    view.details = Some(format!("{error:?}"));
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn adjust_volume(&mut self, change: f64, cx: &mut Context<Self>) {
        if let Some(preview) = &self.preview
            && let Err(error) =
                preview.set_volume((preview.snapshot().volume + change).clamp(0., 1.))
        {
            self.status = "Ses düzeyi değiştirilemedi.".into();
            self.details = Some(error.to_string());
        }
        cx.notify();
    }

    fn seek(&mut self, seconds: f64, cx: &mut Context<Self>) {
        if let Some(preview) = &self.preview
            && let Err(error) = preview.seek(Duration::from_secs_f64(seconds.clamp(0., self.total)))
        {
            self.status = "Seçilen zamana gidilemedi.".into();
            self.details = Some(error.to_string());
        }
        cx.notify();
    }

    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(input) = self.source.clone() else {
            return;
        };
        let start = self.start.read(cx).value().parse::<f64>();
        let end = self.end.read(cx).value().parse::<f64>();
        let (Ok(start), Ok(end)) = (start, end) else {
            self.status = "Başlangıç ve bitişi saniye olarak yaz. Örnek: 12.5".into();
            cx.notify();
            return;
        };
        if !start.is_finite()
            || !end.is_finite()
            || start < 0.
            || end <= start
            || end > self.total + 0.0005
        {
            self.status = "Bitiş başlangıçtan sonra ve videonun içinde olmalı.".into();
            cx.notify();
            return;
        }
        let end = end.min(self.total);
        let parent = input.parent().unwrap_or(std::path::Path::new("."));
        let picker = cx.prompt_for_new_path(parent, Some("tehom-klip.mp4"));
        self.busy = true;
        self.details = None;
        cx.spawn_in(window, async move |view, cx| {
            let output = match picker.await {
                Ok(Ok(output)) => output,
                error => {
                    let _ = view.update(cx, |view, cx| {
                        view.busy = false;
                        view.status = "Kayıt konumu seçilemedi. Tekrar deneyebilirsin.".into();
                        view.details = Some(format!("{error:?}"));
                        cx.notify();
                    });
                    return;
                }
            };
            let Some(output) = output else {
                let _ = view.update(cx, |view, cx| {
                    view.busy = false;
                    cx.notify();
                });
                return;
            };
            let output = if output
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("mp4"))
            {
                output
            } else {
                output.with_extension("mp4")
            };
            let _ = view.update(cx, |view, cx| {
                view.status = "Klip hazırlanıyor…".into();
                cx.notify();
            });
            let saved_path = output.clone();
            let cancelled = Arc::new(AtomicBool::new(false));
            let worker_cancelled = cancelled.clone();
            let _ = view.update(cx, |view, cx| {
                view.cancellation = Some(cancelled);
                cx.notify();
            });
            let result = cx
                .background_spawn(async move {
                    tehom_media::export_cancellable(&input, &output, start, end, &worker_cancelled)
                })
                .await;
            let _ = view.update(cx, |view, cx| {
                view.busy = false;
                view.cancellation = None;
                match result {
                    Ok(()) => view.status = format!("MP4 kaydedildi: {}", saved_path.display()),
                    Err(error) => {
                        view.status = match error.kind() {
                            std::io::ErrorKind::AlreadyExists => {
                                "Bu adla bir dosya var. Başka bir ad seç."
                            }
                            std::io::ErrorKind::Interrupted => "Klip hazırlama iptal edildi.",
                            std::io::ErrorKind::StorageFull => "Klip için yeterli disk alanı yok.",
                            _ => "Klip kaydedilemedi. Konumu ve boş disk alanını kontrol et.",
                        }
                        .into();
                        view.details = Some(error.to_string());
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }
}

impl Tehom {
    fn render_editor(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let snapshot = self
            .preview
            .as_ref()
            .map(Preview::snapshot)
            .unwrap_or_default();
        let ready = self.preview.is_some();
        let position = snapshot.position.as_secs_f64();
        div()
            .size_full()
            .flex()
            .flex_col()
            .gap_4()
            .p_6()
            .bg(rgb(0x161616))
            .text_color(rgb(0xf2f4f8))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(div().text_xl().child("Klip düzenleyici"))
                    .child(
                        Button::new("open")
                            .label("Video aç")
                            .disabled(self.busy)
                            .on_click(cx.listener(|view, _, window, cx| view.pick(window, cx))),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h(px(200.))
                    .bg(rgb(0x262626))
                    .rounded_lg()
                    .items_center()
                    .justify_center()
                    .overflow_hidden()
                    .when_some(self.image.clone(), |element, image| {
                        element.child(img(image).size_full().object_fit(ObjectFit::Contain))
                    })
                    .when(self.image.is_none(), |element| {
                        element.child("Video önizlemesi")
                    }),
            )
            .child(
                div()
                    .id("timeline-keyboard")
                    .test_support()
                    .focusable()
                    .tab_stop(true)
                    .aria_label(
                        "Zaman çizelgesi. Oklarla ilerle, Home ve End ile başa veya sona git.",
                    )
                    .border_1()
                    .border_color(rgb(0x393939))
                    .rounded_md()
                    .p_2()
                    .focus(|style| style.border_color(rgb(0x78a9ff)))
                    .on_key_down(cx.listener(|view, event: &KeyDownEvent, _, cx| {
                        let position = view
                            .preview
                            .as_ref()
                            .map(|p| p.snapshot().position.as_secs_f64())
                            .unwrap_or(0.);
                        let target = match event.keystroke.key.as_str() {
                            "left" => position - 1.,
                            "right" => position + 1.,
                            "home" => 0.,
                            "end" => view.total,
                            _ => return,
                        };
                        cx.stop_propagation();
                        view.seek(target, cx);
                    }))
                    .child(
                        Slider::new(&self.timeline)
                            .disabled(!ready)
                            .accessibility_label("Videoda zaman seç")
                            .w_full(),
                    ),
            )
            .child(
                div()
                    .flex()
                    .gap_3()
                    .flex_wrap()
                    .items_center()
                    .child(
                        Button::new("back")
                            .label("−5 sn")
                            .disabled(!ready)
                            .on_click(cx.listener(move |view, _, _, cx| {
                                view.seek((position - 5.).max(0.), cx)
                            })),
                    )
                    .child(
                        Button::new("play")
                            .label(if snapshot.paused { "Oynat" } else { "Duraklat" })
                            .disabled(!ready)
                            .on_click(cx.listener(|view, _, _, cx| {
                                if let Some(preview) = &view.preview
                                    && let Err(error) =
                                        preview.set_paused(!preview.snapshot().paused)
                                {
                                    view.status = "Oynatma durumu değiştirilemedi.".into();
                                    view.details = Some(error.to_string());
                                }
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("forward")
                            .label("+5 sn")
                            .disabled(!ready)
                            .on_click(
                                cx.listener(move |view, _, _, cx| view.seek(position + 5., cx)),
                            ),
                    )
                    .child(format!("{position:.2} / {:.2} saniye", self.total))
                    .child(
                        Button::new("volume-down")
                            .label("Sesi azalt")
                            .disabled(!ready)
                            .on_click(cx.listener(|view, _, _, cx| view.adjust_volume(-0.1, cx))),
                    )
                    .child(format!("Ses: %{:.0}", snapshot.volume * 100.))
                    .child(
                        Button::new("volume-up")
                            .label("Sesi artır")
                            .disabled(!ready)
                            .on_click(cx.listener(|view, _, _, cx| view.adjust_volume(0.1, cx))),
                    ),
            )
            .child(
                div()
                    .flex()
                    .gap_3()
                    .flex_wrap()
                    .items_end()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .w(px(150.))
                            .child("Başlangıç (saniye)")
                            .child(
                                Input::new(&self.start)
                                    .id("start")
                                    .disabled(self.busy || !ready)
                                    .aria_label("Klip başlangıcı, saniye"),
                            ),
                    )
                    .child(
                        Button::new("mark-start")
                            .label("Buradan başlat")
                            .disabled(!ready || self.busy)
                            .on_click(cx.listener(move |view, _, window, cx| {
                                view.start.update(cx, |state, cx| {
                                    state.set_value(format!("{position:.3}"), window, cx)
                                });
                            })),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .w(px(150.))
                            .child("Bitiş (saniye)")
                            .child(
                                Input::new(&self.end)
                                    .id("end")
                                    .disabled(self.busy || !ready)
                                    .aria_label("Klip bitişi, saniye"),
                            ),
                    )
                    .child(
                        Button::new("mark-end")
                            .label("Burada bitir")
                            .disabled(!ready || self.busy)
                            .on_click(cx.listener(move |view, _, window, cx| {
                                view.end.update(cx, |state, cx| {
                                    state.set_value(format!("{position:.3}"), window, cx)
                                });
                            })),
                    )
                    .when_some(self.editing_clip, |element, id| {
                        element.child(
                            Button::new("save-draft")
                                .label("Yeni taslak kaydet")
                                .disabled(!ready || self.busy || self.source_busy)
                                .on_click(cx.listener(move |view, _, window, cx| {
                                    view.save_draft_revision(id, window, cx)
                                })),
                        )
                    })
                    .child(
                        Button::new("save")
                            .primary()
                            .label("MP4 kaydet")
                            .disabled(!ready || self.busy)
                            .on_click(cx.listener(|view, _, window, cx| view.save(window, cx))),
                    ),
            )
            .when(self.editing_clip.is_some(), |element| {
                element.child(
                    div().text_sm().child(
                        "Yeni taslak 20–90 saniye olmalı. Önceki taslak ve dosyalar korunur.",
                    ),
                )
            })
            .when(self.cancellation.is_some(), |element| {
                element.child(
                    Button::new("cancel")
                        .label("İptal et")
                        .on_click(cx.listener(|view, _, _, cx| {
                            if let Some(cancelled) = &view.cancellation {
                                cancelled.store(true, Ordering::Release);
                            }
                            view.status = "İptal ediliyor…".into();
                            cx.notify();
                        })),
                )
            })
            .when(!self.metadata.is_empty(), |element| {
                element.child(div().text_sm().child(self.metadata.clone()))
            })
            .child(
                div()
                    .id("status")
                    .role(Role::Status)
                    .aria_label(self.status.clone())
                    .text_sm()
                    .child(self.status.clone()),
            )
            .when(self.details.is_some(), |element| {
                element.child(
                    Button::new("details")
                        .label(if self.show_details {
                            "Ayrıntıları gizle"
                        } else {
                            "Ayrıntılar"
                        })
                        .on_click(cx.listener(|view, _, _, cx| {
                            view.show_details = !view.show_details;
                            cx.notify();
                        })),
                )
            })
            .when(self.show_details, |element| {
                element.when_some(self.details.clone(), |element, details| {
                    element.child(div().text_sm().child(details))
                })
            })
            .into_any_element()
    }
}

impl Drop for Tehom {
    fn drop(&mut self) {
        self.engine_stop.store(true, Ordering::Release);
        if let Some(cancelled) = &self.cancellation {
            cancelled.store(true, Ordering::Release);
        }
    }
}

pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<std::borrow::Cow<'static, [u8]>>> {
        if path == "brand/icon.svg" {
            return Ok(Some(std::borrow::Cow::Borrowed(include_bytes!(
                "../../../assets/brand/icon.svg"
            ))));
        }
        assets::AllAssets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut paths = assets::AllAssets.list(path)?;
        if "brand/icon.svg".starts_with(path) {
            paths.push("brand/icon.svg".into());
        }
        Ok(paths)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::test::{TestQueryExt, TestWindowExt};
    use gpui_kit::{Bounds, TestAppContext, WindowBounds, WindowOptions, open_window, point, size};

    #[gpui_kit::test]
    fn home_leads_with_channel_automation(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let (handle, _) = cx.update(|cx| {
            open_window(WindowOptions::default(), cx, |window, cx| {
                cx.new(|cx| Tehom::new(window, cx))
            })
            .unwrap()
        });
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            assert_eq!(
                window
                    .find_by_label("YouTube kanal veya video adresi")
                    .label(),
                Some("YouTube kanal veya video adresi")
            );
            assert_eq!(window.find("add-channel").disabled(), Some(true));
            assert_eq!(window.find("import-local-video").disabled(), Some(true));
            assert_eq!(window.find_by_label("Kaynaklar").label(), Some("Kaynaklar"));
        })
        .unwrap();
    }

    #[gpui_kit::test]
    fn timeline_pointer_seeks_the_real_decoder(cx: &mut TestAppContext) {
        let directory = tempfile::tempdir().unwrap();
        let input = directory.path().join("video.mp4");
        assert!(
            std::process::Command::new("ffmpeg")
                .args([
                    "-nostdin",
                    "-v",
                    "error",
                    "-y",
                    "-f",
                    "lavfi",
                    "-i",
                    "testsrc2=size=160x90:rate=10",
                    "-t",
                    "10",
                    "-c:v",
                    "libx264"
                ])
                .arg(&input)
                .status()
                .unwrap()
                .success()
        );
        let preview = Preview::open(&input).unwrap();
        cx.update(gpui_kit::init);
        let (handle, view) = cx.update(|cx| {
            open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds {
                        origin: point(px(0.), px(0.)),
                        size: size(px(1100.), px(800.)),
                    })),
                    ..Default::default()
                },
                cx,
                |window, cx| {
                    cx.new(|cx| {
                        let mut view = Tehom::new(window, cx);
                        view.page = Page::Clips;
                        view.total = 10.;
                        view.source = Some(input);
                        view.preview = Some(preview);
                        view
                    })
                },
            )
            .unwrap()
        });
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            assert_ne!(
                window.find_by_label("Videoda zaman seç").disabled(),
                Some(true)
            );
            window.click("timeline-keyboard", cx);
        })
        .unwrap();
        cx.run_until_parked();
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        loop {
            let position = cx.update(|cx| {
                view.read(cx)
                    .preview
                    .as_ref()
                    .unwrap()
                    .snapshot()
                    .position
                    .as_secs_f64()
            });
            if (position - 5.).abs() < 0.2 {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "pointer seek stayed at {position}"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        cx.update_window(handle, |_, window, cx| {
            window.click("timeline-keyboard", cx);
            window.press("right", cx);
        })
        .unwrap();
        cx.run_until_parked();
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        loop {
            let position = cx.update(|cx| {
                view.read(cx)
                    .preview
                    .as_ref()
                    .unwrap()
                    .snapshot()
                    .position
                    .as_secs_f64()
            });
            if (position - 6.).abs() < 0.2 {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "keyboard seek stayed at {position}"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}
