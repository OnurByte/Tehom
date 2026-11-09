//! Studio: caption editing for a library clip, the caption look controls it
//! shares with Settings, and the "Videos without captions" card.
use super::*;
use crate::automation_ui::Action;
use gpui_kit::{component::switch::Switch, uniform_list};
use tehom_core::{CaptionPosition, CaptionSize, CaptionStyle, Cue, SoundEffect};

/// Text and outline presets; the names are what people see and hear.
pub(crate) const COLORS: [(&str, &str); 7] = [
    ("White", "#FFFFFF"),
    ("Yellow", "#FFE600"),
    ("Green", "#3DDC84"),
    ("Cyan", "#00E5FF"),
    ("Pink", "#FF4FD8"),
    ("Red", "#FF3B30"),
    ("Black", "#000000"),
];
const ROW_HEIGHT: f32 = 52.;

/// Which caption look a control edits.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Look {
    Studio,
    Default,
}

pub(crate) struct Studio {
    pub(crate) clip_id: i64,
    clip_start_ms: u64,
    pub(crate) lines: Vec<(Cue, Entity<InputState>)>,
    pub(crate) style: CaptionStyle,
    pub(crate) sound: SoundEffect,
}

/// `0:03` for 3 400 ms.
pub(crate) fn clock(ms: u64) -> String {
    let seconds = ms / 1000;
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

fn color(hex: &str) -> Option<gpui_kit::Rgba> {
    u32::from_str_radix(hex.trim_start_matches('#'), 16)
        .ok()
        .map(gpui_kit::rgb)
}

impl Tehom {
    /// Seeds the Studio when a different clip is opened for editing; edits to
    /// the open clip survive every snapshot refresh.
    pub(crate) fn sync_studio(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.editing_clip else {
            self.studio = None;
            return;
        };
        if self
            .studio
            .as_ref()
            .is_some_and(|studio| studio.clip_id == id)
        {
            return;
        }
        let Some(clip) = self
            .engine_snapshot
            .as_ref()
            .and_then(|s| s.clips.iter().find(|clip| clip.id == id))
            .cloned()
        else {
            return;
        };
        let lines = clip
            .captions
            .into_iter()
            .map(|cue| {
                let text = cue.text.clone();
                (
                    cue,
                    cx.new(|cx| InputState::new(window, cx).default_value(text)),
                )
            })
            .collect();
        self.studio = Some(Studio {
            clip_id: id,
            clip_start_ms: clip.start_ms,
            lines,
            style: clip.caption_style,
            sound: clip.sound_effect,
        });
    }

    fn edit_look(
        &mut self,
        look: Look,
        change: impl FnOnce(&mut CaptionStyle, &mut SoundEffect),
        cx: &mut Context<Self>,
    ) {
        match look {
            Look::Studio => {
                if let Some(studio) = &mut self.studio {
                    change(&mut studio.style, &mut studio.sound);
                }
            }
            Look::Default => {
                let mut style = self.default_look.take().unwrap_or_else(|| {
                    self.engine_snapshot
                        .as_ref()
                        .map(|s| s.caption_style.clone())
                        .unwrap_or_default()
                });
                change(&mut style, &mut SoundEffect::None);
                self.default_look = Some(style);
            }
        }
        cx.notify();
    }

    /// The caption look controls, used by the Studio and by Settings.
    /// `sound` is shown only for a clip, since only clips have an intro sound.
    pub(crate) fn caption_look(
        &self,
        look: Look,
        style: &CaptionStyle,
        sound: Option<SoundEffect>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = crate::theme::palette(cx);
        let base = if look == Look::Studio { 0 } else { 100 };
        let view = cx.entity().downgrade();
        let switch = |id: (&'static str, usize),
                      label: &'static str,
                      checked: bool,
                      set: fn(&mut CaptionStyle, bool)| {
            let view = view.clone();
            Switch::new(id)
                .checked(checked)
                .label(label)
                .accessibility_label(label)
                .on_click(move |checked: &bool, _, cx: &mut gpui_kit::App| {
                    let checked = *checked;
                    let _ = view.update(cx, |view, cx| {
                        view.edit_look(look, |style, _| set(style, checked), cx)
                    });
                })
        };
        let choice = |id: (&'static str, usize), label: String, selected: bool| {
            Button::new(id)
                .h(px(44.))
                .label(label)
                .when(selected, |button| button.primary())
                .toggled(selected)
        };
        let swatches = |name: &'static str, what: &'static str, current: &str, outline: bool| {
            div()
                .flex()
                .flex_wrap()
                .gap_2()
                .children(COLORS.iter().enumerate().map(|(index, (label, hex))| {
                    let selected = current.eq_ignore_ascii_case(hex);
                    let value = *hex;
                    Button::new((name, base + index))
                        .h(px(44.))
                        .label(*label)
                        .accessibility_label(format!("{what}: {label}"))
                        .when(selected, |button| button.primary())
                        .toggled(selected)
                        .child(
                            div()
                                .size(px(14.))
                                .rounded_full()
                                // Content colours: a mid-grey ring keeps black
                                // and white swatches visible in both themes.
                                .border_1()
                                .border_color(p.edge)
                                .when_some(color(hex), |swatch, color| swatch.bg(color)),
                        )
                        .on_click(cx.listener(move |view, _, _, cx| {
                            view.edit_look(
                                look,
                                |style, _| {
                                    if outline {
                                        style.outline = value.into();
                                    } else {
                                        style.color = value.into();
                                    }
                                },
                                cx,
                            )
                        }))
                }))
        };
        let heading = |text: &'static str| div().text_sm().text_color(p.muted).child(text);
        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(switch(
                ("look-enabled", base),
                "Show captions",
                style.enabled,
                |style, on| style.enabled = on,
            ))
            .child(heading("Text colour"))
            .child(swatches("look-color", "Text colour", &style.color, false))
            .child(heading("Outline colour"))
            .child(swatches(
                "look-outline",
                "Outline colour",
                &style.outline,
                true,
            ))
            .child(heading("Size"))
            .child(
                div().flex().flex_wrap().gap_2().children(
                    [
                        (CaptionSize::Small, "Small"),
                        (CaptionSize::Medium, "Medium"),
                        (CaptionSize::Large, "Large"),
                    ]
                    .into_iter()
                    .enumerate()
                    .map(|(index, (size, label))| {
                        choice(
                            ("look-size", base + index),
                            label.into(),
                            style.size == size,
                        )
                        .on_click(cx.listener(move |view, _, _, cx| {
                            view.edit_look(look, |style, _| style.size = size, cx)
                        }))
                    }),
                ),
            )
            .child(heading("Position"))
            .child(
                div().flex().flex_wrap().gap_2().children(
                    [
                        (CaptionPosition::Top, "Top"),
                        (CaptionPosition::Middle, "Middle"),
                        (CaptionPosition::Bottom, "Bottom"),
                    ]
                    .into_iter()
                    .enumerate()
                    .map(|(index, (position, label))| {
                        choice(
                            ("look-position", base + index),
                            label.into(),
                            style.position == position,
                        )
                        .on_click(cx.listener(move |view, _, _, cx| {
                            view.edit_look(look, |style, _| style.position = position, cx)
                        }))
                    }),
                ),
            )
            .child(switch(
                ("look-caps", base),
                "ALL CAPS",
                style.uppercase,
                |style, on| style.uppercase = on,
            ))
            .when_some(sound, |card, sound| {
                card.child(heading("Sound at the start")).child(
                    div().flex().flex_wrap().gap_2().children(
                        [
                            (SoundEffect::None, "None"),
                            (SoundEffect::Whoosh, "Whoosh"),
                            (SoundEffect::Pop, "Pop"),
                        ]
                        .into_iter()
                        .enumerate()
                        .map(|(index, (effect, label))| {
                            choice(("look-sound", base + index), label.into(), sound == effect)
                                .accessibility_label(format!("Sound at the start: {label}"))
                                .on_click(cx.listener(move |view, _, _, cx| {
                                    view.edit_look(look, |_, sound| *sound = effect, cx)
                                }))
                        }),
                    ),
                )
            })
            .into_any_element()
    }

    /// The current caption line drawn over the preview frame with the chosen
    /// colour, size and position. A rough guide: the outline is not drawn.
    pub(crate) fn caption_overlay(&self, position: f64, cx: &gpui_kit::App) -> Option<AnyElement> {
        let studio = self.studio.as_ref()?;
        if !studio.style.enabled || self.image.is_none() {
            return None;
        }
        let at = ((position * 1000.) as u64).checked_sub(studio.clip_start_ms)?;
        let (_, input) = studio
            .lines
            .iter()
            .find(|(cue, _)| cue.start_ms <= at && at < cue.end_ms)?;
        let mut text = input.read(cx).value().to_string();
        if studio.style.uppercase {
            text = text.to_uppercase();
        }
        let line = div()
            .px_3()
            .text_center()
            .when_some(color(&studio.style.color), |line, color| {
                line.text_color(color)
            })
            .map(|line| match studio.style.size {
                CaptionSize::Small => line.text_lg(),
                CaptionSize::Medium => line.text_2xl(),
                CaptionSize::Large => line.text_3xl(),
            })
            .child(text);
        Some(
            div()
                .absolute()
                .inset_0()
                .p_6()
                .flex()
                .flex_col()
                .items_center()
                .map(|layer| match studio.style.position {
                    CaptionPosition::Top => layer.justify_start(),
                    CaptionPosition::Middle => layer.justify_center(),
                    CaptionPosition::Bottom => layer.justify_end(),
                })
                .child(line)
                .into_any_element(),
        )
    }

    fn studio_cues(&self, cx: &gpui_kit::App) -> std::result::Result<Vec<Cue>, String> {
        let Some(studio) = &self.studio else {
            return Ok(Vec::new());
        };
        studio
            .lines
            .iter()
            .enumerate()
            .map(|(index, (cue, input))| {
                let text = input.read(cx).value().trim().to_string();
                if text.is_empty() {
                    Err(format!(
                        "Caption line {} is empty. Type some words or remove the line.",
                        index + 1
                    ))
                } else {
                    Ok(Cue {
                        text,
                        ..cue.clone()
                    })
                }
            })
            .collect()
    }

    fn save_studio(&mut self, render: bool, window: &Window, cx: &mut Context<Self>) {
        let Some(studio) = &self.studio else {
            return;
        };
        let (id, style, sound) = (studio.clip_id, studio.style.clone(), studio.sound);
        let cues = match self.studio_cues(cx) {
            Ok(cues) => cues,
            Err(message) => {
                self.status = message;
                cx.notify();
                return;
            }
        };
        self.act(
            if render {
                Action::RenderClip(id, cues, style, sound)
            } else {
                Action::SaveCaptions(id, cues, style, sound)
            },
            window,
            cx,
        );
        if render && self.source_busy {
            self.status =
                "Making the video again with your captions. This can take a minute…".into();
        }
    }

    /// The Captions panel beside the editor, when a library clip is open.
    pub(crate) fn studio_panel(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let studio = self.studio.as_ref()?;
        let p = crate::theme::palette(cx);
        let unavailable = self.backend.is_none() || self.source_busy;
        let count = studio.lines.len();
        let clip = self
            .engine_snapshot
            .as_ref()
            .and_then(|s| s.clips.iter().find(|clip| clip.id == studio.clip_id))
            .cloned();
        let list = uniform_list(
            "caption-lines",
            count,
            cx.processor(|view, range: std::ops::Range<usize>, _, cx| {
                let p = crate::theme::palette(cx);
                let Some(studio) = &view.studio else {
                    return Vec::new();
                };
                let start = studio.clip_start_ms;
                range
                    .filter_map(|index| {
                        let (cue, input) = studio.lines.get(index)?;
                        let at = cue.start_ms;
                        let time = clock(at);
                        Some(
                            div()
                                .h(px(ROW_HEIGHT))
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(
                                    Button::new(("caption-time", index))
                                        .h(px(44.))
                                        .w(px(64.))
                                        .label(time.clone())
                                        .accessibility_label(format!("Play from {time}"))
                                        .on_click(cx.listener(move |view, _, _, cx| {
                                            view.seek((start + at) as f64 / 1000., cx)
                                        })),
                                )
                                .child(
                                    div().flex_1().min_w_0().child(
                                        Input::new(input)
                                            .h(px(44.))
                                            .aria_label(format!("Caption at {time}")),
                                    ),
                                )
                                .child(
                                    Button::new(("caption-remove", index))
                                        .h(px(44.))
                                        .ghost()
                                        .label("Remove")
                                        .accessibility_label(format!("Remove caption at {time}"))
                                        .text_color(p.text)
                                        .on_click(cx.listener(move |view, _, _, cx| {
                                            if let Some(studio) = &mut view.studio
                                                && index < studio.lines.len()
                                            {
                                                studio.lines.remove(index);
                                            }
                                            cx.notify();
                                        })),
                                ),
                        )
                    })
                    .collect()
            }),
        )
        .h(px(ROW_HEIGHT * count.clamp(1, 7) as f32));
        Some(
            div()
                .id("studio-panel")
                .w(px(400.))
                .flex_shrink_0()
                .h_full()
                .overflow_y_scroll()
                .border_l_1()
                .border_color(p.border)
                .p_5()
                .flex()
                .flex_col()
                .gap_4()
                .child(div().text_xl().child("Captions"))
                .child(div().text_sm().text_color(p.muted).child(if count == 0 {
                    "This clip has no caption lines.".to_string()
                } else {
                    format!("{count} lines. Click a time to watch that moment.")
                }))
                .when(count > 0, |panel| panel.child(list))
                .child(div().text_lg().child("Look"))
                .child(self.caption_look(
                    Look::Studio,
                    &studio.style,
                    Some(studio.sound),
                    cx,
                ))
                .child(
                    div().text_sm().text_color(p.muted).child(
                        "The caption over the preview is a rough guide. The finished video shows the exact look.",
                    ),
                )
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap_2()
                        .child(
                            Button::new("save-captions")
                                .h(px(44.))
                                .label("Save captions")
                                .disabled(unavailable)
                                .on_click(cx.listener(|view, _, window, cx| {
                                    view.save_studio(false, window, cx)
                                })),
                        )
                        .child(
                            Button::new("render-clip")
                                .h(px(44.))
                                .primary()
                                .label("Make the video again")
                                .disabled(unavailable)
                                .on_click(cx.listener(|view, _, window, cx| {
                                    view.save_studio(true, window, cx)
                                })),
                        ),
                )
                .child(div().text_sm().text_color(p.muted).child(
                    "Making the video again saves your captions first and replaces this clip's MP4 file.",
                ))
                .when_some(clip.filter(|clip| clip.output_available), |panel, clip| {
                    panel.child(self.publish_row(&clip, cx))
                })
                .into_any_element(),
        )
    }

    /// Settings: the look given to clips made from now on.
    pub(crate) fn default_look_card(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = crate::theme::palette(cx);
        let unavailable = self.backend.is_none() || self.source_busy;
        let style = self.default_look.clone().unwrap_or_else(|| {
            self.engine_snapshot
                .as_ref()
                .map(|s| s.caption_style.clone())
                .unwrap_or_default()
        });
        let saved = self
            .engine_snapshot
            .as_ref()
            .is_some_and(|s| s.caption_style == style);
        div()
            .p_5()
            .bg(p.background)
            .border_1()
            .border_color(p.border)
            .rounded(px(12.))
            .flex()
            .flex_col()
            .gap_3()
            .child(div().text_lg().child("Caption look for new clips"))
            .child(div().text_sm().text_color(p.muted).child(
                "Tehom writes captions into every new clip. Change one clip's captions in the Studio.",
            ))
            .child(self.caption_look(Look::Default, &style, None, cx))
            .child(
                Button::new("save-default-look")
                    .h(px(44.))
                    .primary()
                    .label(if saved { "Look saved" } else { "Save look" })
                    .disabled(unavailable || saved)
                    .on_click(cx.listener(move |view, _, window, cx| {
                        view.act(Action::DefaultCaptionStyle(style.clone()), window, cx)
                    })),
            )
            .into_any_element()
    }

    /// Reads the speech-to-text readiness once Settings is shown.
    pub(crate) fn maybe_check_speech(&mut self, window: &Window, cx: &mut Context<Self>) {
        if self.speech.is_some() || self.speech_checking {
            return;
        }
        let Some(backend) = self.backend.clone() else {
            return;
        };
        self.speech_checking = true;
        let task =
            cx.background_spawn(
                async move { backend.runtime.block_on(backend.engine.speech_status()) },
            );
        cx.spawn_in(window, async move |view, cx| {
            let status = task.await;
            let _ = view.update(cx, |view, cx| {
                view.speech_checking = false;
                view.speech = Some(status);
                cx.notify();
            });
        })
        .detach();
    }

    /// One click: downloads the speech model, or, when the speech tool
    /// itself is missing, shows where to get it.
    pub(crate) fn set_up_speech(&mut self, window: &Window, cx: &mut Context<Self>) {
        if self.speech.as_ref().is_some_and(|s| s.program.is_none()) {
            self.open_settings_at(crate::automation_ui::CARD_SPEECH, cx);
            return;
        }
        self.act(Action::DownloadSpeechModel, window, cx);
        if self.source_busy {
            self.speech_downloading = true;
            self.status =
                "Setting up captions on this computer. This can take a few minutes…".into();
        }
    }

    pub(crate) fn speech_setup_label(&self) -> &'static str {
        if self.speech_downloading {
            "Setting up captions… (downloading about 150 MB)"
        } else {
            "Set up captions on this computer"
        }
    }

    pub(crate) fn speech_card(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = crate::theme::palette(cx);
        let unavailable = self.backend.is_none() || self.source_busy;
        let status = self.speech.as_ref();
        let program = status.map(|s| s.program.is_some());
        let model = status.map(|s| s.model.is_some());
        let state = match (program, model) {
            (None, _) | (_, None) => "Checking this computer…",
            (Some(false), _) => "The speech tool is missing from this copy of Tehom.",
            (Some(true), Some(true)) => "Ready. Videos without captions get captions made here.",
            (Some(true), Some(false)) => {
                "Not set up yet. One click downloads what is needed (about 150 MB)."
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
            .child(div().text_lg().child("Videos without captions"))
            .child(div().text_sm().text_color(p.muted).child(
                "When a video has no captions of its own, Tehom writes them on this computer. The video and its sound never leave it.",
            ))
            .child(
                div()
                    .id("speech-state")
                    .test_support()
                    .aria_label(state)
                    .text_sm()
                    .child(state),
            )
            .when(program == Some(false), |card| match self.update_button(cx) {
                Some(button) => card
                    .child(div().text_sm().child("Update Tehom to get the built-in speech tool."))
                    .child(button),
                None => card.child(div().text_sm().child(
                    "Update Tehom to get the built-in speech tool, or install whisper.cpp (its whisper-cli program) so it is on your PATH, then restart Tehom.",
                )),
            })
            .when(program == Some(true) && model == Some(false), |card| {
                card.child(
                    Button::new("download-speech-model")
                        .h(px(44.))
                        .primary()
                        .label(self.speech_setup_label())
                        .disabled(unavailable || self.speech_downloading)
                        .on_click(cx.listener(|view, _, window, cx| view.set_up_speech(window, cx))),
                )
            })
            .into_any_element()
    }
}
