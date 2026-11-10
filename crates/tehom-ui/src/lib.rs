mod about;
mod automation_ui;
mod models_ui;
mod prefs;
mod publish;
mod shell;
mod studio;
pub mod theme;
use automation_ui::{Backend, Page};
use gpui_kit::TestSupportExt as _;
use gpui_kit::{
    AnyElement, AssetSource, Entity, KeyDownEvent, ObjectFit, PathPromptOptions, RenderImage,
    Result, Role, ScrollHandle, SharedString, Subscription, Task, Window, assets,
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
    /// When the last automatic update check started.
    update_checked_at: Option<std::time::Instant>,
    automation: prefs::AutomationForm,
    /// Dragged or saved sidebar width; `None` until the saved one is known.
    sidebar_width: Option<f32>,
    sidebar_focus: gpui_kit::FocusHandle,
    /// The appearance last applied from the saved settings or a choice.
    appearance: Option<tehom_core::Appearance>,
    source_mode: AutomationMode,
    source_busy: bool,
    backend: Option<Arc<Backend>>,
    engine_snapshot: Option<EngineSnapshot>,
    /// Models the chosen provider offers, strongest first.
    available_models: Vec<tehom_core::ModelInfo>,
    /// Which provider `available_models` belongs to and how loading went.
    model_list: Option<(tehom_core::AiProvider, models_ui::ModelList)>,
    model_query: Entity<InputState>,
    /// The "Other…" free-text model field is open.
    model_other: bool,
    /// Caption edits for the clip open in the Studio; kept across snapshot
    /// refreshes and re-seeded only when another clip is opened.
    studio: Option<studio::Studio>,
    /// Unsaved "Caption look for new clips" draft.
    default_look: Option<tehom_core::CaptionStyle>,
    speech: Option<tehom_core::SpeechStatus>,
    speech_checking: bool,
    speech_downloading: bool,
    settings_scroll: ScrollHandle,
    /// The clip whose "Post to Instagram" form is open.
    publish_open: Option<i64>,
    publish_caption: Entity<InputState>,
    publish_account: Option<i64>,
    instagram: publish::InstagramForm,
    _input_watch: Vec<Subscription>,
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
            InputState::new(window, cx)
                .placeholder("YouTube or Kick channel, a video or an RSS feed link")
        });
        let ai_endpoint =
            cx.new(|cx| InputState::new(window, cx).default_value("http://127.0.0.1:11434"));
        let ai_model = cx.new(|cx| InputState::new(window, cx).placeholder("Model name"));
        let ai_key = cx.new(|cx| {
            InputState::new(window, cx)
                .masked(true)
                .placeholder("Paste your API key")
        });
        let model_query = cx.new(|cx| InputState::new(window, cx).placeholder("Search all models"));
        let publish_caption =
            cx.new(|cx| InputState::new(window, cx).placeholder("Write a caption"));
        // These fields change what the view shows (filtered list, counter).
        let input_watch = vec![
            cx.observe(&model_query, |_, _, cx| cx.notify()),
            cx.observe(&publish_caption, |_, _, cx| cx.notify()),
        ];
        let instagram = publish::InstagramForm::new(window, cx);
        let automation = prefs::AutomationForm::new(window, cx);
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
            update_checked_at: None,
            automation,
            sidebar_width: None,
            sidebar_focus: cx.focus_handle().tab_stop(true),
            appearance: None,
            source_mode: AutomationMode::AutoDraft,
            source_busy: false,
            backend: None,
            engine_snapshot: None,
            available_models: Vec::new(),
            model_list: None,
            model_query,
            model_other: false,
            studio: None,
            default_look: None,
            speech: None,
            speech_checking: false,
            speech_downloading: false,
            settings_scroll: ScrollHandle::new(),
            publish_open: None,
            publish_caption,
            publish_account: None,
            instagram,
            _input_watch: input_watch,
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
        let overlay = self.caption_overlay(position, cx);
        let panel = self.studio_panel(cx);
        let editor = div()
            .flex_1()
            .min_w_0()
            .h_full()
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
                    // The video stage stays black in both themes so caption
                    // colours preview as they will look in the clip.
                    .bg(crate::theme::DARK.background).text_color(crate::theme::DARK.edge)
                    .border_1().border_color(p.border)
                    .rounded(px(12.))
                    .relative()
                    .items_center()
                    .justify_center()
                    .overflow_hidden()
                    .when_some(self.image.clone(), |element, image| {
                        element.child(img(image).size_full().object_fit(ObjectFit::Contain))
                    })
                    .when(self.image.is_none(), |element| {
                        element.child("Video preview")
                    })
                    .children(overlay),
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
            });
        div()
            .size_full()
            .flex()
            .bg(p.background)
            .text_color(p.text)
            .child(editor)
            .children(panel)
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

/// Files embedded in the binary: the brand mark and the Simple Icons (CC0)
/// platform glyphs, see assets/README.md.
const OWN_ASSETS: [(&str, &[u8]); 6] = [
    (
        "brand/youtube.svg",
        include_bytes!("../../../assets/brand/youtube.svg"),
    ),
    (
        "brand/kick.svg",
        include_bytes!("../../../assets/brand/kick.svg"),
    ),
    (
        "brand/icon.svg",
        include_bytes!("../../../assets/brand/icon.svg"),
    ),
    (
        "icons/instagram.svg",
        include_bytes!("../../../assets/icons/instagram.svg"),
    ),
    (
        "icons/youtubeshorts.svg",
        include_bytes!("../../../assets/icons/youtubeshorts.svg"),
    ),
    (
        "icons/tiktok.svg",
        include_bytes!("../../../assets/icons/tiktok.svg"),
    ),
];

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<std::borrow::Cow<'static, [u8]>>> {
        if let Some((_, bytes)) = OWN_ASSETS.iter().find(|(name, _)| *name == path) {
            return Ok(Some(std::borrow::Cow::Borrowed(bytes)));
        }
        assets::AllAssets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut paths = assets::AllAssets.list(path)?;
        paths.extend(
            OWN_ASSETS
                .iter()
                .filter(|(name, _)| name.starts_with(path))
                .map(|(name, _)| SharedString::from(*name)),
        );
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
                window
                    .find_by_label("Channel, video or RSS feed address")
                    .label(),
                Some("Channel, video or RSS feed address")
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
                captions: Vec::new(),
                caption_style: Default::default(),
                sound_effect: Default::default(),
            }],
            ai: Default::default(),
            export_directory: directory.to_path_buf(),
            export_profile: tehom_core::ExportProfile::Horizontal,
            events: Vec::new(),
            onboarded: true,
            check_updates: false,
            caption_style: Default::default(),
            accounts: Vec::new(),
            publications: Vec::new(),
            auto_publish_account: None,
            automation: Default::default(),
            appearance: Default::default(),
            sidebar_width: tehom_core::SIDEBAR_WIDTH,
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
        for (query, title) in [
            ("instagram", "Connect Instagram"),
            ("caption look", "Caption look for new clips"),
            ("speech model", "Speech model for videos without captions"),
        ] {
            let titles: Vec<_> = shell::Command::matching(query)
                .into_iter()
                .map(|(_, t)| t)
                .collect();
            assert_eq!(titles, [title], "{query}");
        }
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
            // The optional captions step can be skipped.
            assert_eq!(view.read(cx).onboarding, Some(2));
            assert!(window.try_find_by_label("Skip for now").is_some());
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
                view.page = Page::About;
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

    #[gpui_kit::test]
    fn publish_row_offers_instagram_and_marks_the_others_soon(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let dir = tempfile::tempdir().unwrap();
        render_page(cx, Page::Clips, snapshot(dir.path(), true), |window| {
            assert_ne!(window.find_by_label("Instagram").disabled(), Some(true));
            for label in ["YouTube Shorts, coming soon", "TikTok, coming soon"] {
                assert_eq!(
                    window.find_by_label(label).disabled(),
                    Some(true),
                    "{label}"
                );
            }
        });
        // A clip without its MP4 file has nothing to post.
        let missing = tempfile::tempdir().unwrap();
        render_page(cx, Page::Clips, snapshot(missing.path(), false), |window| {
            assert!(window.try_find_by_label("Instagram").is_none());
        });
    }

    #[gpui_kit::test]
    fn model_list_shows_strength_words_only_for_remote_providers(cx: &mut TestAppContext) {
        use tehom_core::{AiProvider, ModelInfo, Strength};
        cx.update(gpui_kit::init);
        let dir = tempfile::tempdir().unwrap();
        let (handle, view) = open_view(cx, snapshot(dir.path(), true));
        let model = |id: &str, strength: Option<Strength>| ModelInfo {
            id: id.into(),
            label: id.into(),
            strength,
            note: strength.map(|_| "A plain sentence.".into()),
        };
        cx.update_window(handle, |_, window, cx| {
            view.update(cx, |view, cx| {
                view.page = Page::Settings;
                view.ai_provider = Some(AiProvider::Openai);
                view.ai_key_saved.set(Some((AiProvider::Openai, true)));
                view.model_list = Some((AiProvider::Openai, models_ui::ModelList::Ready));
                view.available_models = vec![
                    model("big", Some(Strength::Strongest)),
                    model("middle", Some(Strength::Balanced)),
                    model("small", Some(Strength::Fast)),
                ];
                cx.notify();
            });
            window.render_frame(cx);
            for label in ["big, Most capable", "middle, Balanced", "small, Fast"] {
                assert!(window.try_find_by_label(label).is_some(), "{label}");
            }
            view.update(cx, |view, cx| {
                view.ai_provider = Some(AiProvider::Local);
                view.model_list = Some((AiProvider::Local, models_ui::ModelList::Ready));
                view.available_models = vec![model("llama3", None)];
                cx.notify();
            });
            window.render_frame(cx);
            assert!(window.try_find_by_label("llama3").is_some());
            for word in ["Fast", "Balanced", "Most capable"] {
                assert!(
                    window
                        .try_find_by_label(&format!("llama3, {word}"))
                        .is_none(),
                    "{word}"
                );
            }
        })
        .unwrap();
    }

    #[gpui_kit::test]
    fn model_list_searches_long_lists_instead_of_showing_every_row(cx: &mut TestAppContext) {
        use tehom_core::{AiProvider, ModelInfo, Strength};
        cx.update(gpui_kit::init);
        let dir = tempfile::tempdir().unwrap();
        let (handle, view) = open_view(cx, snapshot(dir.path(), true));
        cx.update_window(handle, |_, window, cx| {
            view.update(cx, |view, cx| {
                view.page = Page::Settings;
                view.ai_provider = Some(AiProvider::Openrouter);
                view.ai_key_saved.set(Some((AiProvider::Openrouter, true)));
                view.model_list = Some((AiProvider::Openrouter, models_ui::ModelList::Ready));
                view.available_models = (0..300)
                    .map(|index| ModelInfo {
                        id: format!("model-{index}"),
                        label: format!("model-{index}"),
                        strength: Some(Strength::Balanced),
                        note: None,
                    })
                    .collect();
                cx.notify();
            });
            window.render_frame(cx);
            assert!(window.try_find_by_label("Search models").is_some());
            assert!(window.try_find_by_label("model-0, Balanced").is_some());
            assert!(window.try_find_by_label("model-299, Balanced").is_none());
        })
        .unwrap();
    }

    #[gpui_kit::test]
    fn captions_panel_lists_the_clip_lines_and_its_look(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let dir = tempfile::tempdir().unwrap();
        let mut fixture = snapshot(dir.path(), true);
        fixture.clips[0].captions = vec![
            tehom_core::Cue {
                start_ms: 0,
                end_ms: 2_000,
                text: "Hello there".into(),
            },
            tehom_core::Cue {
                start_ms: 3_400,
                end_ms: 5_000,
                text: "Second line".into(),
            },
        ];
        fixture.clips[0].caption_style.enabled = false;
        let source = dir.path().join("source.mp4");
        let (handle, view) = open_view(cx, fixture);
        cx.update_window(handle, |_, window, cx| {
            view.update(cx, |view, cx| {
                view.page = Page::Clips;
                view.source = Some(source);
                view.editing_clip = Some(1);
                cx.notify();
            });
            window.render_frame(cx);
            window.render_frame(cx);
            assert!(view.read(cx).studio.is_some());
            assert!(window.try_find_by_label("Caption at 0:00").is_some());
            assert!(window.try_find_by_label("Caption at 0:03").is_some());
            assert!(window.try_find_by_label("Play from 0:03").is_some());
            assert_eq!(window.find_by_label("Show captions").checked(), Some(false));
            assert!(window.try_find_by_label("Text colour: White").is_some());
            assert!(
                window
                    .try_find_by_label("Sound at the start: Whoosh")
                    .is_some()
            );
            assert!(window.try_find_by_label("Make the video again").is_some());
        })
        .unwrap();
        // A snapshot refresh keeps unsaved edits for the open clip.
        cx.update_window(handle, |_, window, cx| {
            view.update(cx, |view, cx| {
                view.studio.as_mut().unwrap().lines.remove(0);
                let fresh = view.engine_snapshot.clone();
                view.engine_snapshot = fresh;
                cx.notify();
            });
            window.render_frame(cx);
            assert_eq!(view.read(cx).studio.as_ref().unwrap().lines.len(), 1);
        })
        .unwrap();
    }

    #[gpui_kit::test]
    fn two_factor_code_field_appears_after_instagram_asks_for_it(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let dir = tempfile::tempdir().unwrap();
        let (handle, view) = open_view(cx, snapshot(dir.path(), true));
        cx.update_window(handle, |_, window, cx| {
            view.update(cx, |view, cx| {
                view.page = Page::Settings;
                view.instagram.open = true;
                cx.notify();
            });
            window.render_frame(cx);
            assert_eq!(
                window.find_by_label("Instagram password").value(),
                None,
                "the password field must not expose its value"
            );
            assert!(window.try_find_by_label("Sign-in code").is_none());
            view.update(cx, |view, cx| {
                view.instagram.two_factor = Some((None, true));
                cx.notify();
            });
            window.render_frame(cx);
            assert!(window.try_find_by_label("Sign-in code").is_some());
            assert!(
                window
                    .try_find_by_label("Enter the 6-digit code from your authenticator app")
                    .is_some()
            );
            assert!(window.try_find_by_label("Instagram password").is_none());
            view.update(cx, |view, cx| {
                view.instagram.two_factor = Some((Some("+90 *** ** 12".into()), false));
                cx.notify();
            });
            window.render_frame(cx);
            assert!(
                window
                    .try_find_by_label("Enter the code Instagram sent by SMS to +90 *** ** 12")
                    .is_some()
            );
        })
        .unwrap();
    }

    /// A real engine in a temporary folder, so dispatched actions run.
    fn backend(directory: &std::path::Path) -> Arc<Backend> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .unwrap();
        let engine = runtime
            .block_on(tehom_automation::Automation::open(directory.join("data")))
            .unwrap();
        Arc::new(Backend { runtime, engine })
    }

    #[gpui_kit::test]
    fn automation_card_shows_saved_values_and_refuses_out_of_range_numbers(
        cx: &mut TestAppContext,
    ) {
        cx.update(gpui_kit::init);
        let dir = tempfile::tempdir().unwrap();
        let mut fixture = snapshot(dir.path(), true);
        fixture.automation = tehom_core::AutomationSettings {
            check_minutes: 15,
            max_clips: 3,
            min_clip_seconds: 12,
            max_clip_seconds: 45,
            new_source_mode: AutomationMode::Manual,
            ..Default::default()
        };
        let (handle, view) = open_view(cx, fixture);
        let engine = backend(dir.path());
        cx.update_window(handle, |_, window, cx| {
            view.update(cx, |view, cx| {
                view.page = Page::Settings;
                view.backend = Some(engine.clone());
                cx.notify();
            });
            window.render_frame(cx);
            for (label, value) in [
                ("Check for new videos every", "15"),
                ("Clips per video, at most", "3"),
                ("Shortest clip", "12"),
                ("Longest clip", "45"),
            ] {
                assert_eq!(window.find_by_label(label).value(), Some(value), "{label}");
            }
            assert!(
                window
                    .try_find_by_label("New links: Only list them")
                    .is_some()
            );
            // New links start in the saved mode.
            assert_eq!(view.read(cx).source_mode, AutomationMode::Manual);
            view.update(cx, |view, cx| {
                view.automation
                    .minutes
                    .update(cx, |state, cx| state.set_value("0", window, cx));
            });
            view.update(cx, |view, cx| view.save_automation(window, cx));
            window.render_frame(cx);
            assert!(
                window
                    .try_find_by_label("Check for new videos every 1 to 1440 minutes.")
                    .is_some()
            );
            assert!(!view.read(cx).source_busy, "nothing may be saved");
            view.update(cx, |view, cx| {
                view.automation
                    .minutes
                    .update(cx, |state, cx| state.set_value("20", window, cx));
            });
            view.update(cx, |view, cx| view.save_automation(window, cx));
            assert!(view.read(cx).source_busy, "valid settings are saved");
        })
        .unwrap();
        cx.run_until_parked();
        let saved = engine.runtime.block_on(engine.engine.automation()).unwrap();
        assert_eq!(saved.check_minutes, 20);
        assert_eq!(saved.new_source_mode, AutomationMode::Manual);
    }

    #[gpui_kit::test]
    fn appearance_choice_applies_at_once_and_is_saved(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let dir = tempfile::tempdir().unwrap();
        let (handle, view) = open_view(cx, snapshot(dir.path(), true));
        let engine = backend(dir.path());
        for (label, dark) in [("Appearance: Dark", true), ("Appearance: Light", false)] {
            cx.update_window(handle, |_, window, cx| {
                view.update(cx, |view, cx| {
                    view.page = Page::Settings;
                    view.backend = Some(engine.clone());
                    view.source_busy = false;
                    cx.notify();
                });
                window.render_frame(cx);
                assert!(window.try_find_by_label(label).is_some(), "{label}");
                let choice = if dark {
                    tehom_core::Appearance::Dark
                } else {
                    tehom_core::Appearance::Light
                };
                view.update(cx, |view, cx| view.choose_appearance(choice, window, cx));
                assert_eq!(gpui_kit::component::Theme::global(cx).mode.is_dark(), dark);
            })
            .unwrap();
            cx.run_until_parked();
            let saved = engine.runtime.block_on(engine.engine.snapshot()).unwrap();
            assert_eq!(
                saved.appearance,
                if dark {
                    tehom_core::Appearance::Dark
                } else {
                    tehom_core::Appearance::Light
                }
            );
        }
    }

    #[gpui_kit::test]
    fn sidebar_width_is_bounded_and_saved(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let dir = tempfile::tempdir().unwrap();
        let mut fixture = snapshot(dir.path(), true);
        fixture.sidebar_width = 320;
        let (handle, view) = open_view(cx, fixture);
        let engine = backend(dir.path());
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            assert_eq!(view.read(cx).sidebar_px(), 320.);
            view.update(cx, |view, cx| {
                view.backend = Some(engine.clone());
                view.resize_sidebar(9999., false, cx);
                assert_eq!(view.sidebar_px(), 480.);
                view.resize_sidebar(360., true, cx);
            });
        })
        .unwrap();
        cx.run_until_parked();
        let saved = engine.runtime.block_on(engine.engine.snapshot()).unwrap();
        assert_eq!(saved.sidebar_width, 360);
    }

    #[gpui_kit::test]
    fn about_page_lists_every_author_link_and_the_project_links(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let dir = tempfile::tempdir().unwrap();
        render_page(cx, Page::About, snapshot(dir.path(), true), |window| {
            for (label, _) in about::AUTHOR_LINKS {
                assert!(window.try_find_by_label(label).is_some(), "{label}");
            }
            for label in [
                "Source code",
                "Report a problem",
                "All releases",
                about::AUTHOR,
            ] {
                assert!(window.try_find_by_label(label).is_some(), "{label}");
            }
        });
    }

    #[gpui_kit::test]
    fn speech_setup_is_one_button_with_a_busy_state(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let dir = tempfile::tempdir().unwrap();
        let (handle, view) = open_view(cx, snapshot(dir.path(), true));
        let status = |program: bool, model: bool| tehom_core::SpeechStatus {
            program: program.then(|| PathBuf::from("speech")),
            model: model.then(|| PathBuf::from("model")),
        };
        cx.update_window(handle, |_, window, cx| {
            view.update(cx, |view, cx| {
                view.page = Page::Settings;
                view.speech = Some(status(true, false));
                cx.notify();
            });
            window.render_frame(cx);
            assert!(
                window
                    .try_find_by_label("Set up captions on this computer")
                    .is_some()
            );
            view.update(cx, |view, cx| {
                view.speech_downloading = true;
                cx.notify();
            });
            window.render_frame(cx);
            assert_eq!(
                window
                    .find_by_label("Setting up captions… (downloading about 150 MB)")
                    .disabled(),
                Some(true)
            );
            view.update(cx, |view, cx| {
                view.speech_downloading = false;
                view.speech = Some(status(true, true));
                cx.notify();
            });
            window.render_frame(cx);
            assert!(
                window
                    .try_find_by_label("Ready. Videos without captions get captions made here.")
                    .is_some()
            );
            assert!(
                window
                    .try_find_by_label("Set up captions on this computer")
                    .is_none()
            );
            view.update(cx, |view, cx| {
                view.speech = Some(status(false, false));
                cx.notify();
            });
            window.render_frame(cx);
            assert!(
                window
                    .try_find_by_label("Set up captions on this computer")
                    .is_none()
            );
            assert!(
                window
                    .try_find_by_label("The speech tool is missing from this copy of Tehom.")
                    .is_some()
            );
        })
        .unwrap();
    }

    #[test]
    fn new_palette_entries_are_found() {
        for (query, title) in [
            ("automation", "Automation settings"),
            ("about", "About Tehom"),
            ("dark", "Appearance: dark"),
            ("wider", "Make the sidebar wider"),
        ] {
            let titles: Vec<_> = shell::Command::matching(query)
                .into_iter()
                .map(|(_, t)| t)
                .collect();
            assert_eq!(titles, [title], "{query}");
        }
    }
}
