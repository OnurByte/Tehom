//! The model picker: the models a provider offers, with a plain strength word
//! for remote ones, a search over long lists and a typed-name fallback.
use super::*;
use crate::automation_ui::Action;
use tehom_core::{AiConfig, AiProvider, ModelInfo, Strength};

/// Rows shown before searching; the list arrives strongest first.
const FIRST: usize = 12;
/// Most rows a search shows at once.
const SEARCH_LIMIT: usize = 40;

pub(crate) enum ModelList {
    Loading,
    Ready,
    Failed(String),
}

pub(crate) fn strength_word(strength: Strength) -> &'static str {
    match strength {
        Strength::Fast => "Fast",
        Strength::Balanced => "Balanced",
        Strength::Strongest => "Most capable",
    }
}

impl Tehom {
    pub(crate) fn current_provider(&self) -> AiProvider {
        self.ai_provider.unwrap_or_else(|| {
            self.engine_snapshot
                .as_ref()
                .map(|s| s.ai.provider)
                .unwrap_or_default()
        })
    }

    /// Loads the list for the provider on screen once, when the AI card shows.
    pub(crate) fn maybe_list_models(&mut self, window: &Window, cx: &mut Context<Self>) {
        let visible = self.page == Page::Settings || self.onboarding == Some(1);
        let provider = self.current_provider();
        if visible
            && self.backend.is_some()
            && self.model_list.as_ref().is_none_or(|(p, _)| *p != provider)
        {
            self.list_models(provider, window, cx);
        }
    }

    pub(crate) fn list_models(
        &mut self,
        provider: AiProvider,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        let Some(backend) = self.backend.clone() else {
            return;
        };
        let config = AiConfig {
            provider,
            endpoint: if provider.is_remote() {
                AiConfig::default().endpoint
            } else {
                self.ai_endpoint.read(cx).value().to_string()
            },
            ..Default::default()
        };
        self.model_list = Some((provider, ModelList::Loading));
        self.available_models.clear();
        cx.notify();
        let task = cx.background_spawn(async move {
            backend
                .runtime
                .block_on(backend.engine.models(&config))
                .map_err(|error| error.to_string())
        });
        cx.spawn_in(window, async move |view, cx| {
            let result = task.await;
            let _ = view.update(cx, |view, cx| {
                // A newer request for another provider wins.
                if view
                    .model_list
                    .as_ref()
                    .is_some_and(|(p, _)| *p == provider)
                {
                    view.model_list = Some((
                        provider,
                        match result {
                            Ok(models) => {
                                view.available_models = models;
                                ModelList::Ready
                            }
                            Err(error) => ModelList::Failed(error),
                        },
                    ));
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Uses a model from the list, saved the way the card saves it: a local
    /// model at once, a remote one only once sending to it was allowed.
    fn choose_model(
        &mut self,
        provider: AiProvider,
        id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.ai_model
            .update(cx, |state, cx| state.set_value(id.clone(), window, cx));
        let allowed = self
            .engine_snapshot
            .as_ref()
            .is_some_and(|s| s.ai.provider == provider && s.ai.remote_consent);
        if !provider.is_remote() {
            let endpoint = self.ai_endpoint.read(cx).value().to_string();
            self.act(
                Action::Ai(AiConfig {
                    endpoint,
                    model: id,
                    ..Default::default()
                }),
                window,
                cx,
            );
        } else if allowed {
            self.act(
                Action::Byok(
                    AiConfig {
                        model: id,
                        provider,
                        remote_consent: true,
                        ..Default::default()
                    },
                    String::new(),
                ),
                window,
                cx,
            );
        }
        cx.notify();
    }

    pub(crate) fn model_picker(
        &self,
        provider: AiProvider,
        unavailable: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = crate::theme::palette(cx);
        let state = self
            .model_list
            .as_ref()
            .filter(|(p, _)| *p == provider)
            .map(|(_, state)| state);
        let chosen = self.ai_model.read(cx).value().to_string();
        let query = self.model_query.read(cx).value().trim().to_lowercase();
        let models = &self.available_models;
        let shown: Vec<&ModelInfo> = if query.is_empty() {
            models.iter().take(FIRST).collect()
        } else {
            models
                .iter()
                .filter(|m| {
                    m.label.to_lowercase().contains(&query) || m.id.to_lowercase().contains(&query)
                })
                .take(SEARCH_LIMIT)
                .collect()
        };
        let chosen_hidden = !chosen.is_empty() && !shown.iter().any(|m| m.id == chosen);
        let note = |text: String| div().text_sm().text_color(p.muted).child(text);
        let message = match state {
            None | Some(ModelList::Loading) => Some("Looking for models…".to_string()),
            Some(ModelList::Failed(error)) if error.starts_with("Needs input: add your") => {
                Some("Add your key to see the models.".into())
            }
            Some(ModelList::Failed(error)) => Some(format!(
                "The list of models could not be loaded. {}",
                error.trim_start_matches("Needs input: ")
            )),
            Some(ModelList::Ready) if models.is_empty() => Some(match provider {
                AiProvider::Codex => {
                    "Codex uses its own default model, so there is nothing to choose.".into()
                }
                AiProvider::Local => {
                    "No models were found. Start your model server, or type a name with Other…"
                        .into()
                }
                _ => "No models were listed. Type a name with Other…".into(),
            }),
            Some(ModelList::Ready) if !query.is_empty() && shown.is_empty() => {
                Some("No model matches that search.".into())
            }
            Some(ModelList::Ready) => None,
        };
        let failed = matches!(state, Some(ModelList::Failed(_)));
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
                    .child(div().flex_1().text_sm().child("Model"))
                    .child(
                        Button::new("refresh-models")
                            .h(px(44.))
                            .label(if failed { "Retry" } else { "Refresh list" })
                            .disabled(
                                self.backend.is_none()
                                    || matches!(state, Some(ModelList::Loading)),
                            )
                            .on_click(cx.listener(move |view, _, window, cx| {
                                view.list_models(provider, window, cx)
                            })),
                    )
                    .child(
                        Button::new("model-other")
                            .h(px(44.))
                            .ghost()
                            .label("Other…")
                            .accessibility_label("Type a model name")
                            .toggled(self.model_other)
                            .on_click(cx.listener(|view, _, _, cx| {
                                view.model_other = !view.model_other;
                                cx.notify();
                            })),
                    ),
            )
            .when(models.len() > FIRST, |picker| {
                picker
                    .child(
                        Input::new(&self.model_query)
                            .h(px(44.))
                            .aria_label("Search models"),
                    )
                    .when(query.is_empty(), |picker| {
                        picker.child(note(format!(
                            "Showing the {FIRST} strongest of {} models. Search to see the rest.",
                            models.len()
                        )))
                    })
            })
            .when_some(message, |picker, message| picker.child(note(message)))
            .when(chosen_hidden, |picker| {
                picker.child(div().text_sm().child(format!("Chosen: {chosen}")))
            })
            .children(shown.into_iter().enumerate().map(|(index, model)| {
                let selected = model.id == chosen;
                let id = model.id.clone();
                let word = model.strength.map(strength_word);
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                Button::new(("model", index))
                                    .h(px(44.))
                                    .flex_1()
                                    .label(model.label.clone())
                                    .accessibility_label(match word {
                                        Some(word) => format!("{}, {word}", model.label),
                                        None => model.label.clone(),
                                    })
                                    .when(selected, |button| button.primary())
                                    .toggled(selected)
                                    .disabled(unavailable)
                                    .on_click(cx.listener(move |view, _, window, cx| {
                                        view.choose_model(provider, id.clone(), window, cx)
                                    })),
                            )
                            // Local models carry no rating, so nothing is shown.
                            .when_some(word, |row, word| {
                                row.child(
                                    div()
                                        .px_2()
                                        .py_1()
                                        .border_1()
                                        .border_color(p.border)
                                        .rounded(px(8.))
                                        .text_xs()
                                        .child(word),
                                )
                            }),
                    )
                    .when(provider.is_remote(), |row| {
                        row.when_some(model.note.clone(), |row, text| {
                            row.child(div().text_sm().text_color(p.muted).child(text))
                        })
                        .when(model.id != model.label, |row| {
                            row.child(
                                div()
                                    .text_xs()
                                    .text_color(p.muted)
                                    .child(model.id.clone()),
                            )
                        })
                    })
            }))
            .when(self.model_other, |picker| {
                picker
                    .child(
                        Input::new(&self.ai_model)
                            .h(px(44.))
                            .aria_label("AI model")
                            .disabled(unavailable),
                    )
                    .child(note(if provider.is_remote() {
                        "Type the model name exactly as the provider writes it, then allow it below.".into()
                    } else {
                        "Type the model name exactly as your model server lists it, then save.".into()
                    }))
            })
            .into_any_element()
    }
}
