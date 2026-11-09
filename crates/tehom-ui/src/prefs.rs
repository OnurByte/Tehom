//! Settings cards for automation and appearance, and the resizable sidebar.
use super::*;
use crate::automation_ui::Action;
use gpui_kit::{Axis, DragMoveEvent, Empty, base::HandleEdge, component::resizable::resize_handle};
use tehom_core::{Appearance, AutomationSettings};

/// The number fields of the Automation card.
pub(crate) struct AutomationForm {
    pub(crate) minutes: Entity<InputState>,
    pub(crate) clips: Entity<InputState>,
    pub(crate) shortest: Entity<InputState>,
    pub(crate) longest: Entity<InputState>,
    /// Choices made with buttons, saved together with the numbers.
    pub(crate) edit: AutomationSettings,
    /// The saved settings the fields were last filled from.
    pub(crate) seeded: Option<AutomationSettings>,
    /// Why the last "Save" was refused.
    pub(crate) note: Option<String>,
}

impl AutomationForm {
    pub(crate) fn new(window: &mut Window, cx: &mut Context<Tehom>) -> Self {
        let mut field = || cx.new(|cx| InputState::new(window, cx));
        Self {
            minutes: field(),
            clips: field(),
            shortest: field(),
            longest: field(),
            edit: AutomationSettings::default(),
            seeded: None,
            note: None,
        }
    }
}

/// Carried while the sidebar edge is dragged.
#[derive(Clone)]
pub(crate) struct ResizeSidebar;

impl Render for ResizeSidebar {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

/// How far one arrow-key press moves the sidebar edge.
const SIDEBAR_STEP: f32 = 20.;

impl Tehom {
    /// Fills the Automation fields, the new-link mode, the sidebar width and
    /// the appearance whenever the saved values change.
    pub(crate) fn sync_prefs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(snapshot) = &self.engine_snapshot else {
            return;
        };
        let saved = snapshot.automation.clone();
        if self.sidebar_width.is_none() {
            self.sidebar_width = Some(snapshot.sidebar_width as f32);
        }
        if self.appearance != Some(snapshot.appearance) {
            self.appearance = Some(snapshot.appearance);
            crate::theme::set_appearance(snapshot.appearance, cx);
        }
        if self.automation.seeded.as_ref() == Some(&saved) {
            return;
        }
        let form = &mut self.automation;
        for (field, value) in [
            (&form.minutes, saved.check_minutes),
            (&form.clips, saved.max_clips),
            (&form.shortest, saved.min_clip_seconds),
            (&form.longest, saved.max_clip_seconds),
        ] {
            field.update(cx, |state, cx| {
                state.set_value(value.to_string(), window, cx)
            });
        }
        form.edit = saved.clone();
        form.seeded = Some(saved.clone());
        form.note = None;
        self.source_mode = saved.new_source_mode;
    }

    /// The card's values, or a plain reason they cannot be saved.
    pub(crate) fn automation_from_form(
        &self,
        cx: &gpui_kit::App,
    ) -> std::result::Result<AutomationSettings, String> {
        let form = &self.automation;
        let number = |field: &Entity<InputState>| {
            field
                .read(cx)
                .value()
                .trim()
                .parse::<u32>()
                .map_err(|_| "Enter whole numbers, such as 30.".to_string())
        };
        let settings = AutomationSettings {
            check_minutes: number(&form.minutes)?,
            max_clips: number(&form.clips)?,
            min_clip_seconds: number(&form.shortest)?,
            max_clip_seconds: number(&form.longest)?,
            ..form.edit.clone()
        };
        match settings.problem() {
            Some(problem) => Err(problem.to_string()),
            None => Ok(settings),
        }
    }

    pub(crate) fn save_automation(&mut self, window: &Window, cx: &mut Context<Self>) {
        match self.automation_from_form(cx) {
            Ok(settings) => {
                self.automation.note = None;
                self.act(Action::Automation(settings), window, cx);
            }
            Err(problem) => {
                self.automation.note = Some(problem);
                cx.notify();
            }
        }
    }

    pub(crate) fn automation_card(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = crate::theme::palette(cx);
        let unavailable = self.backend.is_none() || self.source_busy;
        let edit = &self.automation.edit;
        let group = |title: &'static str| div().pt_2().text_lg().child(title);
        let hint = |text: &'static str| div().text_sm().text_color(p.muted).child(text);
        let number = |field: &Entity<InputState>, label: &'static str, unit: &'static str| {
            div()
                .flex()
                .items_center()
                .gap_3()
                .child(div().min_w(px(220.)).child(label))
                .child(
                    Input::new(field)
                        .h(px(44.))
                        .w(px(120.))
                        .aria_label(label)
                        .disabled(unavailable),
                )
                .child(div().text_sm().text_color(p.muted).child(unit))
        };
        let choice = |id: (&'static str, usize), label: &'static str, selected: bool| {
            Button::new(id)
                .h(px(44.))
                .label(label)
                .when(selected, |button| button.primary())
                .toggled(selected)
                .disabled(unavailable)
        };
        let modes = [
            (AutomationMode::Manual, "Only list them"),
            (AutomationMode::AutoDraft, "Make drafts"),
            (AutomationMode::AutoExport, "Save MP4 files"),
        ];
        div()
            .p_5()
            .bg(p.background)
            .border_1()
            .border_color(p.border)
            .rounded(px(12.))
            .flex()
            .flex_col()
            .gap_3()
            .child(div().text_lg().child("Automation"))
            .child(hint(
                "How Tehom watches your channels and feeds on its own. Posting to Instagram and the caption look have their own cards above; the clip folder is below.",
            ))
            .child(group("How often"))
            .child(number(&self.automation.minutes, "Check for new videos every", "minutes (1 to 1440)"))
            .child(hint("Tehom looks at every channel and feed this often while it is open."))
            .child(group("Clips"))
            .child(div().child("What happens to new videos"))
            .child(div().flex().flex_wrap().gap_2().children(modes.into_iter().enumerate().map(
                |(index, (mode, label))| {
                    choice(("new-source-mode", index), label, edit.new_source_mode == mode)
                        .accessibility_label(format!("New links: {label}"))
                        .on_click(cx.listener(move |view, _, _, cx| {
                            view.automation.edit.new_source_mode = mode;
                            cx.notify();
                        }))
                },
            )))
            .child(hint("Used for links you add from now on. You can still change each source."))
            .child(number(&self.automation.clips, "Clips per video, at most", "clips (1 to 20)"))
            .child(number(&self.automation.shortest, "Shortest clip", "seconds (5 or more)"))
            .child(number(&self.automation.longest, "Longest clip", "seconds (180 or less)"))
            .child(hint("The AI is asked for clips of this length, and others are left out."))
            .child(group("Files"))
            .child(div().flex().flex_wrap().gap_2().children(
                [(true, "Keep the downloaded video"), (false, "Delete it after the MP4 files are saved")]
                    .into_iter()
                    .enumerate()
                    .map(|(index, (keep, label))| {
                        choice(("keep-downloads", index), label, edit.keep_downloads == keep)
                            .on_click(cx.listener(move |view, _, _, cx| {
                                view.automation.edit.keep_downloads = keep;
                                cx.notify();
                            }))
                    }),
            ))
            .child(hint("Deleting saves space. Drafts keep their video so you can still edit them."))
            .child(group("When there are no captions"))
            .child(div().flex().flex_wrap().gap_2().children(
                [(true, "Make captions on this computer"), (false, "Wait for me to add captions")]
                    .into_iter()
                    .enumerate()
                    .map(|(index, (on, label))| {
                        choice(("transcribe-locally", index), label, edit.transcribe_locally == on)
                            .on_click(cx.listener(move |view, _, _, cx| {
                                view.automation.edit.transcribe_locally = on;
                                cx.notify();
                            }))
                    }),
            ))
            .child(hint("Captions made here never leave this computer."))
            .when_some(self.automation.note.clone(), |card, note| {
                card.child(
                    div()
                        .id("automation-note")
                        .test_support()
                        .aria_label(note.clone())
                        .text_sm()
                        .child(note),
                )
            })
            .child(
                Button::new("save-automation")
                    .h(px(44.))
                    .primary()
                    .label("Save automation settings")
                    .disabled(unavailable)
                    .on_click(cx.listener(|view, _, window, cx| view.save_automation(window, cx))),
            )
            .into_any_element()
    }

    pub(crate) fn appearance_card(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = crate::theme::palette(cx);
        let current = self.appearance.unwrap_or_default();
        div()
            .p_5()
            .bg(p.background)
            .border_1()
            .border_color(p.border)
            .rounded(px(12.))
            .flex()
            .flex_col()
            .gap_3()
            .child(div().text_lg().child("Appearance"))
            .child(
                div()
                    .text_sm()
                    .text_color(p.muted)
                    .child("Light or dark. \"Same as this computer\" follows your system setting."),
            )
            .child(
                div().flex().flex_wrap().gap_2().children(
                    [
                        (Appearance::System, "Same as this computer"),
                        (Appearance::Light, "Light"),
                        (Appearance::Dark, "Dark"),
                    ]
                    .into_iter()
                    .enumerate()
                    .map(|(index, (appearance, label))| {
                        let selected = current == appearance;
                        Button::new(("appearance", index))
                            .h(px(44.))
                            .label(label)
                            .accessibility_label(format!("Appearance: {label}"))
                            .when(selected, |button| button.primary())
                            .toggled(selected)
                            .on_click(cx.listener(move |view, _, window, cx| {
                                view.choose_appearance(appearance, window, cx)
                            }))
                    }),
                ),
            )
            .into_any_element()
    }

    /// Applies at once, then saves.
    pub(crate) fn choose_appearance(
        &mut self,
        appearance: Appearance,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        self.appearance = Some(appearance);
        crate::theme::set_appearance(appearance, cx);
        self.act(Action::Appearance(appearance), window, cx);
        cx.notify();
    }

    pub(crate) fn sidebar_px(&self) -> f32 {
        self.sidebar_width
            .unwrap_or(tehom_core::SIDEBAR_WIDTH as f32)
    }

    /// Sets the width within its bounds; `save` stores it.
    pub(crate) fn resize_sidebar(&mut self, width: f32, save: bool, cx: &mut Context<Self>) {
        let (low, high) = (
            *tehom_core::SIDEBAR_WIDTHS.start() as f32,
            *tehom_core::SIDEBAR_WIDTHS.end() as f32,
        );
        let width = width.round().clamp(low, high);
        self.sidebar_width = Some(width);
        if save && let Some(backend) = self.backend.clone() {
            cx.background_spawn(async move {
                if let Err(error) = backend
                    .runtime
                    .block_on(backend.engine.set_sidebar_width(width as u32))
                {
                    eprintln!("Sidebar width: {error}");
                }
            })
            .detach();
        }
        cx.notify();
    }

    /// The draggable, keyboard-adjustable edge on the right of the sidebar.
    pub(crate) fn sidebar_edge(&self, cx: &mut Context<Self>) -> impl IntoElement {
        resize_handle::<ResizeSidebar, ResizeSidebar>("sidebar-edge", Axis::Horizontal)
            .inside(HandleEdge::Trailing)
            .on_drag(ResizeSidebar, |drag, _, _, cx| cx.new(|_| drag.clone()))
            .track_focus(&self.sidebar_focus)
            .on_key_down(cx.listener(|view, event: &KeyDownEvent, _, cx| {
                let step = match event.keystroke.key.as_str() {
                    "left" => -SIDEBAR_STEP,
                    "right" => SIDEBAR_STEP,
                    _ => return,
                };
                cx.stop_propagation();
                view.resize_sidebar(view.sidebar_px() + step, true, cx);
            }))
    }

    /// Follows a drag of the sidebar edge; the window's left is the sidebar's.
    pub(crate) fn on_sidebar_drag(
        &mut self,
        event: &DragMoveEvent<ResizeSidebar>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.resize_sidebar(f32::from(event.event.position.x), false, cx);
    }
}
