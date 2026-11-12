use std::collections::BTreeMap;

use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::components::A;

use crate::helpers::case_questionnaires::{
    american_separator, answered_but_not_required, answers_for, followups_for_answers, is_complete,
    required_followups, validate_answers, CaseQuestionnaire, QuestionnaireBlock,
    QuestionnaireInput, QuestionnaireQuestion, GENERAL_QUESTIONNAIRE,
};
use crate::server_fns::case_properties::{self, CaseProperty};
use crate::server_fns::contacts::search_active_contacts;
use crate::server_fns::err_text;
use crate::server_fns::organizations::search_active_organizations;

const INPUT_CLASS: &str = "mt-1 w-full rounded-xl border border-slate-700 bg-slate-900 px-3 py-2.5 text-sm text-slate-100 placeholder:text-slate-500 focus:border-primary-500 focus:outline-none focus:ring-2 focus:ring-primary-500/30";

#[component]
pub fn CaseQuestionnaires(
    case_id: String,
    properties: Vec<CaseProperty>,
    can_complete: bool,
    on_saved: Callback<()>,
) -> impl IntoView {
    let general_complete = is_complete(&properties, &GENERAL_QUESTIONNAIRE);
    let followups = required_followups(&properties);
    let active_followups = followups
        .iter()
        .filter(|required| !is_complete(&properties, required.questionnaire))
        .copied()
        .collect::<Vec<_>>();
    let completed_followups = followups
        .iter()
        .filter(|required| is_complete(&properties, required.questionnaire))
        .copied()
        .collect::<Vec<_>>();

    let active_views = active_followups
        .into_iter()
        .map(|required| {
            questionnaire_card(
                case_id.clone(),
                required.questionnaire,
                Some(required.reason),
                &properties,
                can_complete,
                on_saved,
            )
        })
        .collect_view();

    let active_count = if general_complete {
        followups
            .iter()
            .filter(|required| !is_complete(&properties, required.questionnaire))
            .count()
    } else {
        1
    };

    let active_tasks = if general_complete {
        if active_count == 0 {
            view! {
                <div class="rounded-lg border border-emerald-500/30 bg-emerald-500/10 px-4 py-3">
                    <p class="text-sm font-medium text-emerald-300">"No active intake tasks"</p>
                    <p class="mt-1 text-xs text-emerald-200/70">
                        "The currently required questionnaires are complete."
                    </p>
                </div>
            }
            .into_any()
        } else {
            view! { <div class="space-y-4">{active_views}</div> }.into_any()
        }
    } else {
        questionnaire_card(
            case_id.clone(),
            &GENERAL_QUESTIONNAIRE,
            None,
            &properties,
            can_complete,
            on_saved,
        )
    };

    let mut completed_views = Vec::new();
    if general_complete {
        completed_views.push(questionnaire_card(
            case_id.clone(),
            &GENERAL_QUESTIONNAIRE,
            None,
            &properties,
            can_complete,
            on_saved,
        ));
    }
    completed_views.extend(completed_followups.into_iter().map(|required| {
        questionnaire_card(
            case_id.clone(),
            required.questionnaire,
            Some(required.reason),
            &properties,
            can_complete,
            on_saved,
        )
    }));
    // Answers whose trigger was later retracted. Shown read-only so the work
    // stays visible and auditable instead of disappearing from every screen;
    // the server refuses to save a follow-up the intake no longer requires.
    completed_views.extend(answered_but_not_required(&properties).into_iter().map(
        |questionnaire| {
            questionnaire_card(
                case_id.clone(),
                questionnaire,
                Some("No longer required by the current general intake answers."),
                &properties,
                false,
                on_saved,
            )
        },
    ));
    let has_completed = !completed_views.is_empty();
    let completed_section = if has_completed {
        view! {
            <section>
                <div class="mb-3">
                    <h2 class="text-lg font-semibold text-slate-100">"Completed questionnaires"</h2>
                    <p class="mt-1 text-sm text-slate-500">
                        "Review or update previously submitted intake answers."
                    </p>
                </div>
                <div class="space-y-4 rounded-card border border-slate-800 bg-slate-900 shadow-card p-4">
                    {completed_views}
                </div>
            </section>
        }
        .into_any()
    } else {
        ().into_any()
    };

    view! {
        <div class="space-y-6">
            <section>
                <div class="mb-3 flex flex-wrap items-center justify-between gap-2">
                    <div>
                        <h2 class="text-lg font-semibold text-slate-100">"Active intake tasks"</h2>
                        <p class="mt-1 text-sm text-slate-500">
                            "Complete these forms to move the intake forward."
                        </p>
                    </div>
                    <span class=if active_count == 0 {
                        "rounded-full bg-emerald-500/15 px-2.5 py-1 text-xs font-medium text-emerald-300 ring-1 ring-emerald-500/30"
                    } else {
                        "rounded-full bg-amber-500/15 px-2.5 py-1 text-xs font-medium text-amber-300 ring-1 ring-amber-500/30"
                    }>
                        {format!("{active_count} active")}
                    </span>
                </div>
                <div class="rounded-card border border-amber-500/30 bg-slate-900 shadow-card p-4">
                    {active_tasks}
                </div>
            </section>

            {completed_section}
        </div>
    }
}

pub fn intake_status(properties: &[CaseProperty]) -> (usize, String) {
    let general_complete = is_complete(properties, &GENERAL_QUESTIONNAIRE);
    let active_count = if general_complete {
        required_followups(properties)
            .iter()
            .filter(|required| !is_complete(properties, required.questionnaire))
            .count()
    } else {
        1
    };
    let summary = if !general_complete {
        "Complete the general intake questionnaire to identify any follow-up work.".to_string()
    } else if active_count == 0 {
        "All currently required intake questionnaires are complete.".to_string()
    } else if active_count == 1 {
        "One follow-up questionnaire needs attention.".to_string()
    } else {
        format!("{active_count} follow-up questionnaires need attention.")
    };
    (active_count, summary)
}

#[component]
pub fn CaseIntakeTaskSummary(case_id: String, properties: Vec<CaseProperty>) -> impl IntoView {
    let (active_count, summary) = intake_status(&properties);
    let href = format!("/cases/{case_id}/intake");

    view! {
        <section>
            <div class="mb-3">
                <h2 class="text-lg font-semibold text-slate-100">"Active tasks"</h2>
                <p class="mt-1 text-sm text-slate-500">"Work that needs attention on this case."</p>
            </div>
            <div class="rounded-card border border-amber-500/30 bg-slate-900 shadow-card p-4">
                <div class="flex flex-col gap-4 sm:flex-row sm:items-center sm:justify-between">
                    <div>
                        <div class="flex flex-wrap items-center gap-2">
                            <h3 class="text-sm font-semibold text-slate-100">"Case intake"</h3>
                            <span class=if active_count == 0 {
                                "rounded-full bg-emerald-500/15 px-2 py-0.5 text-xs font-medium text-emerald-300"
                            } else {
                                "rounded-full bg-amber-500/15 px-2 py-0.5 text-xs font-medium text-amber-300"
                            }>
                                {if active_count == 0 {
                                    "Complete".to_string()
                                } else {
                                    format!("{active_count} active")
                                }}
                            </span>
                        </div>
                        <p class="mt-1 text-sm text-slate-400">{summary}</p>
                    </div>
                    <A
                        href=href
                        attr:class="shrink-0 rounded-full bg-primary-500 px-4 py-2 text-center text-sm font-semibold text-white hover:bg-primary-600"
                    >
                        "Open intake"
                    </A>
                </div>
            </div>
        </section>
    }
}

fn questionnaire_card(
    case_id: String,
    questionnaire: &'static CaseQuestionnaire,
    reason: Option<&'static str>,
    properties: &[CaseProperty],
    can_complete: bool,
    on_saved: Callback<()>,
) -> AnyView {
    let complete = is_complete(properties, questionnaire);
    let answers = RwSignal::new(answers_for(properties, questionnaire));
    let expanded = RwSignal::new(!complete);
    let pending = RwSignal::new(false);
    let error = RwSignal::new(String::new());

    let save = move || {
        if pending.get_untracked() {
            return;
        }
        let submitted = answers.get_untracked();
        if let Err(message) = validate_answers(questionnaire, &submitted) {
            error.set(message);
            return;
        }

        let case_id = case_id.clone();
        pending.set(true);
        error.set(String::new());
        spawn_local(async move {
            match case_properties::save_case_questionnaire(
                case_id,
                questionnaire.slug.to_string(),
                submitted,
            )
            .await
            {
                Ok(()) => {
                    pending.set(false);
                    expanded.set(false);
                    on_saved.run(());
                }
                Err(server_error) => {
                    pending.set(false);
                    error.set(err_text(server_error));
                }
            }
        });
    };

    // Built once; each block shows or hides itself reactively. Rebuilding the
    // list on every answer change would recreate the inputs while they are
    // being typed into.
    let questions = questionnaire
        .blocks
        .iter()
        .map(|block| {
            let block = *block;
            let rendered = block_view(block, answers);
            let hidden = move || {
                if answers.with(|values| block.show_when.holds(values)) {
                    ""
                } else {
                    "hidden"
                }
            };
            view! { <div class=hidden>{rendered}</div> }.into_any()
        })
        .collect::<Vec<_>>();

    let trigger_preview = (questionnaire.slug == GENERAL_QUESTIONNAIRE.slug).then(|| {
        view! {
            <div class="rounded-lg border border-sky-500/30 bg-sky-500/10 p-3">
                <h4 class="text-sm font-semibold text-sky-200">"This intake will create"</h4>
                {move || {
                    let triggered = followups_for_answers(&answers.get());
                    if triggered.is_empty() {
                        return view! {
                            <p class="mt-1 text-xs text-sky-200/70">
                                "No additional forms based on the answers selected so far."
                            </p>
                        }
                            .into_any();
                    }
                    view! {
                        <ul class="mt-2 space-y-2">
                            {triggered
                                .into_iter()
                                .map(|required| view! {
                                    <li class="text-xs text-sky-100">
                                        <span class="font-semibold">{required.questionnaire.title}</span>
                                        <span class="block text-sky-200/70">{required.reason}</span>
                                    </li>
                                })
                                .collect_view()}
                        </ul>
                    }
                        .into_any()
                }}
            </div>
        }
    });

    view! {
        <article class="rounded-lg border border-slate-800 bg-slate-950 p-4">
            <div class="flex items-start justify-between gap-3">
                <div>
                    <div class="flex flex-wrap items-center gap-2">
                        <h3 class="text-sm font-semibold text-slate-100">{questionnaire.title}</h3>
                        <span class=if complete {
                            "rounded-full bg-emerald-500/15 px-2 py-0.5 text-xs font-medium text-emerald-300"
                        } else {
                            "rounded-full bg-amber-500/15 px-2 py-0.5 text-xs font-medium text-amber-300"
                        }>
                            {if complete { "Complete" } else { "Needed" }}
                        </span>
                    </div>
                    <p class="mt-1 text-xs text-slate-400">{questionnaire.description}</p>
                    {reason.map(|text| view! {
                        <p class="mt-2 text-xs font-medium text-amber-300">{text}</p>
                    })}
                </div>
                {if complete && can_complete {
                    view! {
                        <button
                            type="button"
                            on:click=move |_| expanded.update(|open| *open = !*open)
                            class="shrink-0 rounded-full border border-slate-700 px-2.5 py-1.5 text-xs font-medium text-slate-300 hover:bg-slate-800"
                        >
                            {move || if expanded.get() { "Close" } else { "Edit answers" }}
                        </button>
                    }
                        .into_any()
                } else {
                    ().into_any()
                }}
            </div>

            <form
                class=move || {
                    if expanded.get() && can_complete {
                        "mt-4 space-y-4 border-t border-slate-800 pt-4"
                    } else {
                        "hidden"
                    }
                }
                on:submit=move |event| {
                    event.prevent_default();
                    save();
                }
            >
                {questions}
                {trigger_preview}
                <Show when=move || !error.get().is_empty()>
                    <p class="text-sm text-rose-300" role="alert">{move || error.get()}</p>
                </Show>
                <button
                    type="submit"
                    prop:disabled=move || pending.get()
                    class="rounded-full bg-primary-500 px-4 py-2 text-sm font-semibold text-white hover:bg-primary-600 disabled:cursor-not-allowed disabled:opacity-60"
                >
                    {move || if pending.get() { "Saving\u{2026}" } else { "Submit questionnaire" }}
                </button>
            </form>

            <Show when=move || !can_complete && !complete>
                <p class="mt-3 text-xs text-slate-500">
                    "Add notes access is required to complete this questionnaire."
                </p>
            </Show>
        </article>
    }
    .into_any()
}

/// One display block: its heading and the questions and repeat groups currently
/// visible within it.
/// One display block: its heading, questions, and repeat groups.
///
/// Every control is built **once** and then shown or hidden by a `Show`, rather
/// than rebuilt from a reactive closure. Rebuilding on each answer change would
/// recreate the DOM nodes mid-typing, so the field being edited would lose focus
/// and drop characters.
fn block_view(
    block: QuestionnaireBlock,
    answers: RwSignal<BTreeMap<String, String>>,
) -> AnyView {
    let questions = block
        .questions
        .iter()
        .map(|question| {
            let question = *question;
            let control =
                question_control(question.key.to_string(), question.label.to_string(), question, answers);
            let hidden = move || {
                if answers.with(|values| question.show_when.holds(values)) {
                    ""
                } else {
                    "hidden"
                }
            };
            view! { <div class=hidden>{control}</div> }.into_any()
        })
        .collect::<Vec<_>>();

    // Keyed on the entry index, so changing the count only adds or removes the
    // entries that actually changed: typing inside "Child 1" never rebuilds it.
    // This also avoids materialising every entry up to `max` on first render.
    let repeats = block
        .repeats
        .iter()
        .map(|repeat| {
            let repeat = *repeat;
            view! {
                <div class="space-y-3">
                    <For
                        each=move || 0..answers.with(|values| repeat.entries(values))
                        key=|index| *index
                        let:index
                    >
                        <fieldset class="rounded-lg border border-slate-800 bg-slate-900/40 p-3">
                            <legend class="px-1 text-xs font-semibold text-slate-300">
                                {format!("{} {}", repeat.noun, index + 1)}
                            </legend>
                            <div class="space-y-3">
                                {repeat
                                    .fields
                                    .iter()
                                    .map(|field| {
                                        let field = *field;
                                        let control = question_control(
                                            repeat.field_key(index, &field),
                                            repeat.field_label(index, &field),
                                            field,
                                            answers,
                                        );
                                        let hidden = move || {
                                            if answers
                                                .with(|values| field.show_when.holds(values))
                                            {
                                                ""
                                            } else {
                                                "hidden"
                                            }
                                        };
                                        view! { <div class=hidden>{control}</div> }
                                    })
                                    .collect_view()}
                            </div>
                        </fieldset>
                    </For>
                </div>
            }
            .into_any()
        })
        .collect::<Vec<_>>();

    view! {
        <section class="space-y-4 border-t border-slate-800 pt-4 first:border-t-0 first:pt-0">
            <div>
                <h4 class="text-sm font-semibold text-slate-100">{block.title}</h4>
                <p class="mt-0.5 text-xs text-slate-500">{block.description}</p>
            </div>
            <div class="space-y-4">{questions}</div>
            <div class="space-y-3">{repeats}</div>
        </section>
    }
    .into_any()
}

/// A typeahead over the contact or organization directory that degrades to a
/// plain text box: intake volunteers may not hold the information-management
/// grant those searches require, and that must not block the form.
#[component]
fn DirectoryLookup(
    storage_key: String,
    question: QuestionnaireQuestion,
    answers: RwSignal<BTreeMap<String, String>>,
    organizations: bool,
) -> impl IntoView {
    let key = StoredValue::new(storage_key);
    let answer = move || answers.with(|values| values.get(&key.get_value()).cloned().unwrap_or_default());
    let set_answer = move |value: String| {
        answers.update(|values| {
            values.insert(key.get_value(), value);
        });
    };

    let required_now =
        move || question.required && answers.with(|values| question.show_when.holds(values));

    let results = RwSignal::new(Vec::<(String, String)>::new());
    let open = RwSignal::new(false);
    // Once a search is refused we stop asking and behave as a text field.
    let unavailable = RwSignal::new(false);
    let generation = RwSignal::new(0u32);

    let search = move |query: String| {
        if unavailable.get_untracked() || query.trim().len() < 2 {
            results.set(Vec::new());
            return;
        }
        generation.update(|value| *value += 1);
        let mine = generation.get_untracked();
        spawn_local(async move {
            let found = if organizations {
                search_active_organizations(query)
                    .await
                    .map(|list| list.into_iter().map(|o| (o.id, o.name)).collect::<Vec<_>>())
            } else {
                search_active_contacts(query)
                    .await
                    .map(|list| list.into_iter().map(|c| (c.id, c.label)).collect::<Vec<_>>())
            };
            match found {
                Ok(list) => {
                    if generation.get_untracked() == mine {
                        results.set(list);
                    }
                }
                // No access, or the lookup failed: fall back to free text.
                Err(_) => unavailable.set(true),
            }
        });
    };

    view! {
        <div class="relative">
            <input
                class=INPUT_CLASS
                required=required_now
                prop:value=answer
                on:focus=move |_| open.set(true)
                on:blur=move |_| open.set(false)
                on:input=move |event| {
                    let value = event_target_value(&event);
                    set_answer(value.clone());
                    search(value);
                }
            />
            <Show when=move || open.get() && !results.get().is_empty()>
                <ul
                    data-directory-results
                    tabindex="-1"
                    class="absolute z-10 mt-1 max-h-48 w-full overflow-auto rounded-xl border border-slate-700 bg-slate-900 py-1 shadow-lg"
                >
                    {move || {
                        results
                            .get()
                            .into_iter()
                            .map(|(_, name)| {
                                let chosen = name.clone();
                                view! {
                                    <li>
                                        <button
                                            type="button"
                                            class="block w-full px-3 py-1.5 text-left text-sm text-slate-200 hover:bg-slate-800"
                                            // mousedown fires before the input's
                                            // blur, so the pick is not lost.
                                            on:mousedown=move |event| {
                                                event.prevent_default();
                                                set_answer(chosen.clone());
                                                results.set(Vec::new());
                                                open.set(false);
                                            }
                                        >
                                            {name.clone()}
                                        </button>
                                    </li>
                                }
                            })
                            .collect_view()
                    }}
                </ul>
            </Show>
            <Show when=move || unavailable.get()>
                <p class="mt-1 text-xs text-slate-500">
                    "Directory search unavailable \u{2014} type the name instead."
                </p>
            </Show>
        </div>
    }
}

/// Checkboxes whose ticked values are stored as one comma-separated string.
#[component]
fn MultiSelectControl(
    storage_key: String,
    options: &'static [&'static str],
    answers: RwSignal<BTreeMap<String, String>>,
) -> impl IntoView {
    let key = StoredValue::new(storage_key);
    let selected = move || {
        answers.with(|values| {
            values
                .get(&key.get_value())
                .map(|value| {
                    value
                        .split(american_separator())
                        .map(|part| part.trim().to_string())
                        .filter(|part| !part.is_empty())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        })
    };
    let toggle = move |option: &'static str, on: bool| {
        let mut current = selected();
        current.retain(|value| value != option);
        if on {
            current.push(option.to_string());
        }
        answers.update(|values| {
            values.insert(key.get_value(), current.join(", "));
        });
    };

    view! {
        <div class="mt-1 grid gap-1.5 sm:grid-cols-2">
            {options
                .iter()
                .map(|option| {
                    let option = *option;
                    view! {
                        <label class="flex items-center gap-2 text-sm text-slate-300">
                            <input
                                type="checkbox"
                                class="h-4 w-4 rounded border-slate-700 bg-slate-950 text-primary-500"
                                prop:checked=move || selected().iter().any(|value| value == option)
                                on:change=move |event| toggle(option, event_target_checked(&event))
                            />
                            {option}
                        </label>
                    }
                })
                .collect_view()}
        </div>
    }
}

/// A dropdown that reveals a free-text box when "Other" (or an unlisted stored
/// value) is in play.
#[component]
fn SelectOtherControl(
    storage_key: String,
    options: &'static [&'static str],
    question: QuestionnaireQuestion,
    answers: RwSignal<BTreeMap<String, String>>,
) -> impl IntoView {
    let key = StoredValue::new(storage_key);
    let answer = move || answers.with(|values| values.get(&key.get_value()).cloned().unwrap_or_default());
    let set_answer = move |value: String| {
        answers.update(|values| {
            values.insert(key.get_value(), value);
        });
    };
    let required_now =
        move || question.required && answers.with(|values| question.show_when.holds(values));

    // A stored value that is not one of the options is free text already.
    let is_other = move || {
        let current = answer();
        !current.is_empty() && !options.contains(&current.as_str())
    };
    let show_other = RwSignal::new(false);

    view! {
        <>
            <select
                class=INPUT_CLASS
                required=required_now
                on:change=move |event| {
                    let value = event_target_value(&event);
                    if value == "Other" {
                        show_other.set(true);
                        set_answer(String::new());
                    } else {
                        show_other.set(false);
                        set_answer(value);
                    }
                }
            >
                <option value="">"Select an answer"</option>
                {options
                    .iter()
                    .map(|option| {
                        let option = *option;
                        view! {
                            <option value=option selected=move || answer() == option>{option}</option>
                        }
                    })
                    .collect_view()}
            </select>
            <Show when=move || show_other.get() || is_other()>
                <input
                    class=format!("{INPUT_CLASS} mt-2")
                    placeholder="Please specify"
                    prop:value=move || if is_other() { answer() } else { String::new() }
                    on:input=move |event| set_answer(event_target_value(&event))
                />
            </Show>
        </>
    }
}

/// One labelled question. `storage_key` is the question's key, or the indexed
/// key when the question is a field of a repeat group.
fn question_control(
    storage_key: String,
    label: String,
    question: QuestionnaireQuestion,
    answers: RwSignal<BTreeMap<String, String>>,
) -> AnyView {
    let key = StoredValue::new(storage_key.clone());
    let answer = move || answers.with(|values| values.get(&key.get_value()).cloned().unwrap_or_default());
    let set_answer = move |value: String| {
        answers.update(|values| {
            values.insert(key.get_value(), value);
        });
    };
    // Only require what is visible: the browser refuses to submit a form with an
    // invalid `required` field, and cannot focus one inside a hidden container,
    // so a hidden-but-required field silently blocks the whole questionnaire.
    let required_now =
        move || question.required && answers.with(|values| question.show_when.holds(values));

    let text_input = move |kind: &'static str| {
        view! {
            <input
                type=kind
                class=INPUT_CLASS
                required=required_now
                prop:value=answer
                on:input=move |event| set_answer(event_target_value(&event))
            />
        }
        .into_any()
    };

    let control = match question.input {
        QuestionnaireInput::Select(options) => view! {
            <select
                class=INPUT_CLASS
                required=required_now
                on:change=move |event| set_answer(event_target_value(&event))
            >
                <option value="">"Select an answer"</option>
                {options
                    .iter()
                    .map(|option| {
                        let option = *option;
                        view! {
                            <option value=option selected=move || answer() == option>{option}</option>
                        }
                    })
                    .collect_view()}
            </select>
        }
        .into_any(),
        QuestionnaireInput::SelectOther(options) => view! {
            <SelectOtherControl
                storage_key=storage_key.clone()
                options=options
                question=question
                answers=answers
            />
        }
        .into_any(),
        QuestionnaireInput::MultiSelect(options) => view! {
            <MultiSelectControl storage_key=storage_key.clone() options=options answers=answers />
        }
        .into_any(),
        QuestionnaireInput::Text => text_input("text"),
        QuestionnaireInput::Date => text_input("date"),
        QuestionnaireInput::Email => text_input("email"),
        QuestionnaireInput::Phone => text_input("tel"),
        QuestionnaireInput::Number => view! {
            <input
                type="number"
                min="0"
                class=INPUT_CLASS
                required=required_now
                prop:value=answer
                on:input=move |event| set_answer(event_target_value(&event))
            />
        }
        .into_any(),
        QuestionnaireInput::Currency => view! {
            <input
                type="text"
                inputmode="decimal"
                placeholder="0.00"
                class=INPUT_CLASS
                required=required_now
                prop:value=answer
                on:input=move |event| set_answer(event_target_value(&event))
            />
        }
        .into_any(),
        QuestionnaireInput::OrganizationLookup => view! {
            <DirectoryLookup
                storage_key=storage_key.clone()
                question=question
                answers=answers
                organizations=true
            />
        }
        .into_any(),
        QuestionnaireInput::ContactLookup => view! {
            <DirectoryLookup
                storage_key=storage_key.clone()
                question=question
                answers=answers
                organizations=false
            />
        }
        .into_any(),
        QuestionnaireInput::TextArea => view! {
            <textarea
                class=INPUT_CLASS
                rows="3"
                required=required_now
                prop:value=answer
                on:input=move |event| set_answer(event_target_value(&event))
            ></textarea>
        }
        .into_any(),
    };

    view! {
        <div>
            <label class="block text-sm font-medium text-slate-300">
                {label}
                {if question.required {
                    view! { <span class="text-rose-400" aria-hidden="true">" *"</span> }.into_any()
                } else {
                    view! { <span class="font-normal text-slate-500">" (optional)"</span> }.into_any()
                }}
            </label>
            {question.help.map(|text| view! {
                <p class="mt-0.5 text-xs text-slate-500">{text}</p>
            })}
            {control}
        </div>
    }
    .into_any()
}
