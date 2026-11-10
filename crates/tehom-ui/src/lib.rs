mod about;
mod automation_ui;
mod shell;
pub mod theme;
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
    px, svg,
};
pub use shell::{ClosePalette, TogglePalette, bind_keys};
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
    /// Masked; read only when saving and never shown or stored by the view.
    ai_key: Entity<InputState>,
    /// The provider chosen in Settings; `None` follows the saved configuration.
    ai_provider: Option<tehom_core::AiProvider>,
    /// Whether a key is saved for a provider, cached so rendering never
    /// queries the keyring; cleared when the provider or key changes.
    ai_key_saved: std::cell::Cell<Option<(tehom_core::AiProvider, bool)>>,
    /// Root focus, so window shortcuts work before any field is focused.
    focus: gpui_kit::FocusHandle,
    palette: Option<shell::CommandPalette>,
    /// First-run guide step, when shown.
    onboarding: Option<u8>,
    onboarding_checked: bool,
    /// Result of the last action taken inside the guide.
    guide_note: Option<String>,
    update: about::UpdateState,
    update_checked: bool,
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
    /// The root focus handle; focus it after opening the window.
    pub fn focus_handle(&self) -> gpui_kit::FocusHandle {
        self.shell_focus().clone()
    }
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let source_url = cx.new(|cx| {
            InputState::new(window, cx).placeholder("YouTube or Kick channel, or a video link")
        });
        let ai_endpoint =
            cx.new(|cx| InputState::new(window, cx).default_value("http://127.0.0.1:11434"));
        let ai_model = cx.new(|cx| InputState::new(window, cx).placeholder("Model name"));
        let ai_key = cx.new(|cx| {
            InputState::new(window, cx)
                .masked(true)
                .placeholder("Paste your API key")
        });
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
                                    "The video could not be played. Try another file.".into();
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
            ai_key,
            ai_provider: None,
            ai_key_saved: std::cell::Cell::new(None),
            focus: cx.focus_handle(),
            palette: None,
            onboarding: None,
            onboarding_checked: false,
            guide_note: None,
            update: about::UpdateState::Idle,
            update_checked: false,
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
            status: "Choose a video on your computer to begin.".into(),
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
        self.status = "Opening video…".into();
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
                        view.status = "Ready. Choose the start and end, then save the MP4.".into();
                    }
                    Err(error) => {
                        view.status =
                            "The video could not be opened. Check the file and the GStreamer plug-ins.".into();
                        view.details = Some(error.to_string());
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    pub(crate) fn pick(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let picker = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Choose video".into()),
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
                    view.status = "The file picker could not open. Try again.".into();
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
            self.status = "Volume could not be changed.".into();
            self.details = Some(error.to_string());
        }
        cx.notify();
    }

    fn seek(&mut self, seconds: f64, cx: &mut Context<Self>) {
        if let Some(preview) = &self.preview
            && let Err(error) = preview.seek(Duration::from_secs_f64(seconds.clamp(0., self.total)))
        {
            self.status = "Could not move to that time.".into();
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
            self.status = "Enter start and end in seconds, for example 12.5".into();
            cx.notify();
            return;
        };
        if !start.is_finite()
            || !end.is_finite()
            || start < 0.
            || end <= start
            || end > self.total + 0.0005
        {
            self.status = "The end must be after the start and inside the video.".into();
            cx.notify();
            return;
        }
        let end = end.min(self.total);
        let parent = input.parent().unwrap_or(std::path::Path::new("."));
        let picker = cx.prompt_for_new_path(parent, Some("tehom-clip.mp4"));
        self.busy = true;
        self.details = None;
        cx.spawn_in(window, async move |view, cx| {
            let output = match picker.await {
                Ok(Ok(output)) => output,
                error => {
                    let _ = view.update(cx, |view, cx| {
                        view.busy = false;
                        view.status = "The save location could not be chosen. Try again.".into();
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
                view.status = "Saving clip…".into();
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
                    Ok(()) => view.status = format!("MP4 saved: {}", saved_path.display()),
                    Err(error) => {
                        view.status = match error.kind() {
                            std::io::ErrorKind::AlreadyExists => {
                                "A file with this name exists. Choose another name."
                            }
                            std::io::ErrorKind::Interrupted => "Clip export cancelled.",
                            std::io::ErrorKind::StorageFull => "There is not enough disk space for this clip.",
                            _ => "The clip could not be saved. Check the location and free disk space.",
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
        let p = crate::theme::palette(cx);
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
            .bg(p.background)
            .text_color(p.text)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(div().text_xl().child("Clip editor"))
                    .child(
                        Button::new("open").h(px(44.))
                            .label("Open video")
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
                    .bg(p.background).border_1().border_color(p.border)
                    .rounded(px(12.))
                    .items_center()
                    .justify_center()
                    .overflow_hidden()
                    .when_some(self.image.clone(), |element, image| {
                        element.child(img(image).size_full().object_fit(ObjectFit::Contain))
                    })
                    .when(self.image.is_none(), |element| {
                        element.child("Video preview")
                    }),
            )
            .child(
                div()
                    .id("timeline-keyboard")
                    .test_support()
                    .focusable()
                    .tab_stop(true)
                    .aria_label(
                        "Timeline. Use the arrow keys to move, Home and End to jump to the start or end.",
                    )
                    .border_1()
                    .border_color(p.border)
                    .rounded_md()
                    .p_2()
                    .focus(move |style| style.border_color(p.text))
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
                            .accessibility_label("Choose a time in the video")
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
                        Button::new("back").h(px(44.))
                            .label("−5 sn")
                            .disabled(!ready)
                            .on_click(cx.listener(move |view, _, _, cx| {
                                view.seek((position - 5.).max(0.), cx)
                            })),
                    )
                    .child(
                        Button::new("play").h(px(44.))
                            .label(if snapshot.paused { "Play" } else { "Pause" })
                            .disabled(!ready)
                            .on_click(cx.listener(|view, _, _, cx| {
                                if let Some(preview) = &view.preview
                                    && let Err(error) =
                                        preview.set_paused(!preview.snapshot().paused)
                                {
                                    view.status = "Playback could not be changed.".into();
                                    view.details = Some(error.to_string());
                                }
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("forward").h(px(44.))
                            .label("+5 sn")
                            .disabled(!ready)
                            .on_click(
                                cx.listener(move |view, _, _, cx| view.seek(position + 5., cx)),
                            ),
                    )
                    .child(format!("{position:.2} / {:.2} saniye", self.total))
                    .child(
                        Button::new("volume-down").h(px(44.))
                            .label("Volume down")
                            .disabled(!ready)
                            .on_click(cx.listener(|view, _, _, cx| view.adjust_volume(-0.1, cx))),
                    )
                    .child(format!("Volume {:.0}%", snapshot.volume * 100.))
                    .child(
                        Button::new("volume-up").h(px(44.))
                            .label("Volume up")
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
                            .child("Start (seconds)")
                            .child(
                                Input::new(&self.start).h(px(44.))
                                    .id("start")
                                    .disabled(self.busy || !ready)
                                    .aria_label("Clip start, seconds"),
                            ),
                    )
                    .child(
                        Button::new("mark-start").h(px(44.))
                            .label("Start here")
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
                            .child("End (seconds)")
                            .child(
                                Input::new(&self.end).h(px(44.))
                                    .id("end")
                                    .disabled(self.busy || !ready)
                                    .aria_label("Clip end, seconds"),
                            ),
                    )
                    .child(
                        Button::new("mark-end").h(px(44.))
                            .label("End here")
                            .disabled(!ready || self.busy)
                            .on_click(cx.listener(move |view, _, window, cx| {
                                view.end.update(cx, |state, cx| {
                                    state.set_value(format!("{position:.3}"), window, cx)
                                });
                            })),
                    )
                    .when_some(self.editing_clip, |element, id| {
                        element.child(
                            Button::new("save-draft").h(px(44.))
                                .label("Save new draft")
                                .disabled(!ready || self.busy || self.source_busy)
                                .on_click(cx.listener(move |view, _, window, cx| {
                                    view.save_draft_revision(id, window, cx)
                                })),
                        )
                    })
                    .child(
                        Button::new("save").h(px(44.))
                            .primary()
                            .label("Save MP4")
                            .disabled(!ready || self.busy)
                            .on_click(cx.listener(|view, _, window, cx| view.save(window, cx))),
                    ),
            )
            .when(self.editing_clip.is_some(), |element| {
                element.child(
                    div().text_sm().child(
                        "A draft must be 20 to 90 seconds. Earlier drafts and files are kept.",
                    ),
                )
            })
            .when(self.cancellation.is_some(), |element| {
                element.child(
                    Button::new("cancel").h(px(44.))
                        .label("Cancel")
                        .on_click(cx.listener(|view, _, _, cx| {
                            if let Some(cancelled) = &view.cancellation {
                                cancelled.store(true, Ordering::Release);
                            }
                            view.status = "Cancelling…".into();
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
                    Button::new("details").h(px(44.))
                        .label(if self.show_details {
                            "Hide details"
                        } else {
                            "Details"
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
        let bytes: Option<&'static [u8]> = match path {
            "brand/icon.svg" => Some(include_bytes!("../../../assets/brand/icon.svg")),
            "brand/youtube.svg" => Some(include_bytes!("../../../assets/brand/youtube.svg")),
            "brand/kick.svg" => Some(include_bytes!("../../../assets/brand/kick.svg")),
            _ => None,
        };
        if let Some(bytes) = bytes {
            return Ok(Some(std::borrow::Cow::Borrowed(bytes)));
        }
        assets::AllAssets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut paths = assets::AllAssets.list(path)?;
        for brand in ["brand/icon.svg", "brand/youtube.svg", "brand/kick.svg"] {
            if brand.starts_with(path) {
                paths.push(brand.into());
            }
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
        let (handle, view) = cx.update(|cx| {
            open_window(WindowOptions::default(), cx, |window, cx| {
                cx.new(|cx| Tehom::new(window, cx))
            })
            .unwrap()
        });
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            assert_eq!(
                window.find_by_label("YouTube or Kick address").label(),
                Some("YouTube or Kick address")
            );
            assert_eq!(window.find("add-channel").disabled(), Some(true));
            assert_eq!(window.find("import-local-video").disabled(), Some(true));
            // The second sidebar entry opens Sources.
            window.click("0-1", cx);
        })
        .unwrap();
        cx.run_until_parked();
        cx.update_window(handle, |_, _, cx| {
            assert!(view.read(cx).page == Page::Sources);
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
                window
                    .find_by_label("Choose a time in the video")
                    .disabled(),
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

    fn snapshot(directory: &std::path::Path, files_present: bool) -> EngineSnapshot {
        let original = directory.join("source.mp4");
        let export = directory.join("clip.mp4");
        if files_present {
            std::fs::write(&original, b"video").unwrap();
            std::fs::write(&export, b"clip").unwrap();
        }
        EngineSnapshot {
            sources: Vec::new(),
            videos: vec![tehom_core::Video {
                id: 1,
                source_id: 1,
                external_id: "abcdefghijk".into(),
                title: "Fixture".into(),
                url: "https://www.youtube.com/watch?v=abcdefghijk".into(),
                state: "complete".into(),
                error: None,
                local_path: Some(original),
                transcript_language: Some("tr".into()),
                local_available: files_present,
                progress: None,
            }],
            clips: vec![tehom_core::Clip {
                id: 1,
                video_id: 1,
                title: "Clip".into(),
                start_ms: 0,
                end_ms: 20_000,
                output_path: Some(export),
                output_available: files_present,
            }],
            ai: Default::default(),
            export_directory: directory.to_path_buf(),
            export_profile: tehom_core::ExportProfile::Horizontal,
            events: Vec::new(),
            onboarded: true,
            check_updates: false,
        }
    }

    fn render_page(
        cx: &mut TestAppContext,
        page: Page,
        snapshot: EngineSnapshot,
        check: impl FnOnce(&mut gpui_kit::Window),
    ) {
        let (handle, _) = cx.update(|cx| {
            open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds {
                        origin: point(px(0.), px(0.)),
                        size: size(px(1100.), px(2400.)),
                    })),
                    ..Default::default()
                },
                cx,
                |window, cx| {
                    cx.new(|cx| {
                        let mut view = Tehom::new(window, cx);
                        view.page = page;
                        view.engine_snapshot = Some(snapshot);
                        view
                    })
                },
            )
            .unwrap()
        });
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            check(window);
        })
        .unwrap();
    }

    #[gpui_kit::test]
    fn clip_library_hides_actions_for_missing_files_and_offers_new_ones(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let present = tempfile::tempdir().unwrap();
        render_page(cx, Page::Clips, snapshot(present.path(), true), |window| {
            assert_eq!(window.find_all_by_label("Edit draft").len(), 1);
            assert_eq!(window.find_all_by_label("Open clip").len(), 1);
            assert_eq!(window.find_all_by_label("Save original").len(), 1);
            assert_eq!(window.find_all_by_label("Export captions").len(), 1);
        });
        let missing = tempfile::tempdir().unwrap();
        render_page(cx, Page::Clips, snapshot(missing.path(), false), |window| {
            assert!(window.find_all_by_label("Edit draft").is_empty());
            assert!(window.find_all_by_label("Open clip").is_empty());
            // The original can be downloaded again, so saving it stays available.
            assert_eq!(window.find_all_by_label("Save original").len(), 1);
        });
        let settings = tempfile::tempdir().unwrap();
        render_page(
            cx,
            Page::Settings,
            snapshot(settings.path(), true),
            |window| {
                for label in [
                    "Vertical 1080 × 1920",
                    "Horizontal 1920 × 1080",
                    "Original size",
                ] {
                    assert_eq!(window.find_all_by_label(label).len(), 1, "{label}");
                }
            },
        );
    }

    #[test]
    fn palette_commands_filter_by_every_word() {
        assert_eq!(
            shell::Command::matching("").len(),
            shell::Command::ALL.len()
        );
        let shapes = shell::Command::matching("clip shape");
        assert_eq!(shapes.len(), 3);
        let titles: Vec<_> = shell::Command::matching("go CLIPS")
            .into_iter()
            .map(|(_, t)| t)
            .collect();
        assert_eq!(titles, ["Go to Clips"]);
        assert!(shell::Command::matching("publish to tiktok").is_empty());
    }

    fn open_view(
        cx: &mut TestAppContext,
        snapshot: EngineSnapshot,
    ) -> (gpui_kit::AnyWindowHandle, Entity<Tehom>) {
        cx.update(|cx| {
            shell::bind_keys(cx);
            open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds {
                        origin: point(px(0.), px(0.)),
                        size: size(px(1100.), px(1400.)),
                    })),
                    ..Default::default()
                },
                cx,
                |window, cx| {
                    cx.new(|cx| {
                        let mut view = Tehom::new(window, cx);
                        view.engine_snapshot = Some(snapshot);
                        view
                    })
                },
            )
            .unwrap()
        })
    }

    #[gpui_kit::test]
    fn ctrl_k_opens_the_palette_and_escape_closes_it(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let dir = tempfile::tempdir().unwrap();
        let (handle, view) = open_view(cx, snapshot(dir.path(), true));
        cx.update_window(handle, |_, window, cx| {
            let focus = view.read(cx).focus_handle();
            window.focus(&focus, cx);
            window.render_frame(cx);
            window.dispatch_action(Box::new(TogglePalette), cx);
        })
        .unwrap();
        cx.run_until_parked();
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            assert!(view.read(cx).palette.is_some());
            assert!(window.try_find_by_label("Command").is_some());
            window.dispatch_action(Box::new(ClosePalette), cx);
        })
        .unwrap();
        cx.run_until_parked();
        cx.update_window(handle, |_, _, cx| {
            assert!(view.read(cx).palette.is_none());
        })
        .unwrap();
    }

    #[gpui_kit::test]
    fn first_run_guide_walks_through_ai_and_folder(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let dir = tempfile::tempdir().unwrap();
        let mut fresh = snapshot(dir.path(), true);
        fresh.onboarded = false;
        let (handle, view) = open_view(cx, fresh);
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            assert_eq!(view.read(cx).onboarding, Some(0));
            assert!(window.try_find_by_label("Get started").is_some());
            assert!(window.try_find_by_label("Skip setup").is_some());
            window.click("onboarding-next", cx);
            window.render_frame(cx);
            assert_eq!(view.read(cx).onboarding, Some(1));
            for provider in [
                "OpenAI",
                "Anthropic",
                "Google Gemini",
                "OpenRouter",
                "Local model",
            ] {
                assert!(window.try_find_by_label(provider).is_some(), "{provider}");
            }
            assert!(window.try_find_by_label("Continue without AI").is_some());
            window.click("onboarding-next", cx);
            window.render_frame(cx);
            assert!(window.try_find_by_label("Finish").is_some());
            assert!(window.try_find_by_label("Back").is_some());
        })
        .unwrap();
    }

    #[gpui_kit::test]
    fn choosing_a_remote_provider_shows_a_masked_key_field_and_consent(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let dir = tempfile::tempdir().unwrap();
        let (handle, view) = open_view(cx, snapshot(dir.path(), true));
        cx.update_window(handle, |_, window, cx| {
            view.update(cx, |view, cx| {
                view.page = Page::Settings;
                view.ai_provider = Some(tehom_core::AiProvider::Anthropic);
                view.ai_key_saved
                    .set(Some((tehom_core::AiProvider::Anthropic, false)));
                cx.notify();
            });
            window.render_frame(cx);
            let key = window.find_by_label("API key");
            assert_eq!(key.value(), None, "the key field must not expose its value");
            assert!(
                window
                    .try_find_by_label("Allow and use Anthropic")
                    .is_some()
            );
            assert!(window.try_find_by_label("Local model server").is_none());
        })
        .unwrap();
    }

    #[gpui_kit::test]
    fn about_card_offers_an_available_update_and_the_sidebar_says_so(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let dir = tempfile::tempdir().unwrap();
        let (handle, view) = open_view(cx, snapshot(dir.path(), true));
        cx.update_window(handle, |_, window, cx| {
            view.update(cx, |view, cx| {
                view.page = Page::Settings;
                view.update = about::UpdateState::Ready(tehom_automation::update::UpdateStatus {
                    current: tehom_automation::update::CURRENT_VERSION.into(),
                    available: true,
                    latest: tehom_automation::update::Release {
                        tag: "v9.9.9".into(),
                        version: (9, 9, 9),
                        url: "https://github.com/OnurByte/Tehom/releases/tag/v9.9.9".into(),
                        notes: String::new(),
                    },
                });
                cx.notify();
            });
            window.render_frame(cx);
            assert!(window.try_find_by_label("Update to v9.9.9").is_some());
            assert!(window.try_find_by_label("Install update").is_some());
            for label in [
                "What's new",
                "Open clip folder",
                "Open data folder",
                "Report a problem",
            ] {
                assert!(window.try_find_by_label(label).is_some(), "{label}");
            }
        })
        .unwrap();
    }
}
