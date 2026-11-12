use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::components::A;
use leptos_router::hooks::{use_navigate, use_params_map};

use crate::components::guard::require_login;
use crate::components::layout::Layout;
use crate::components::loading::Loading;
use crate::helpers::format::encode_query;
use crate::server_fns::case_notes::{
    add_case_note_addendum, admin_inspect_case_note_draft, case_note_access,
    create_and_finalize_case_note, discard_case_note_draft, file_case_note_document,
    finalize_case_note_draft, list_case_notes, load_case_note, load_case_note_document,
    AddendumCategory, CaseNoteAddendumInput, CaseNoteDetail, CaseNoteDocument, CaseNoteDraftInput,
    CaseNoteInteractionType, CaseNoteListFilters, CaseNoteListItem, CaseNoteState,
    CaseNoteValidationError, CompletionOutcome, ContactCategory, ContactDirection,
    InformationSource, ServiceArea, UrgencyLevel, MAX_LONG_TEXT_CHARS, MAX_MEDIUM_TEXT_CHARS,
    MAX_MULTISELECT_CHOICES, MAX_NARRATIVE_CHARS, MAX_SHORT_TEXT_CHARS, SAFETY_WARNING,
};
use crate::server_fns::err_text;
use crate::state::{now_stamp, today, AppState};

const PAGE_SIZE: i64 = 10;
const INPUT_CLASS: &str = "w-full rounded-xl border border-slate-700 bg-slate-900 px-3 py-2 text-sm text-slate-100 placeholder:text-slate-500 focus:border-primary-500 focus:outline-none focus:ring-2 focus:ring-primary-500/40 disabled:cursor-not-allowed disabled:opacity-60";
const LABEL_CLASS: &str = "block text-xs font-medium text-slate-400";
const PANEL_CLASS: &str = "rounded-card border border-slate-800 bg-slate-900 shadow-card p-4";

trait ChoiceLabel: Copy {
    fn choice_label(self) -> &'static str;
}

macro_rules! impl_choice_label {
    ($($ty:ty),+ $(,)?) => {
        $(impl ChoiceLabel for $ty {
            fn choice_label(self) -> &'static str { self.label() }
        })+
    };
}

impl_choice_label!(
    ServiceArea,
    InformationSource,
    AddendumCategory,
    CaseNoteInteractionType,
    ContactCategory,
    ContactDirection,
    CompletionOutcome,
    UrgencyLevel,
    CaseNoteState,
);

fn push_error(errors: &mut Vec<CaseNoteValidationError>, field: &str, message: &str) {
    errors.push(CaseNoteValidationError {
        field: field.to_string(),
        message: message.to_string(),
    });
}

fn duration_label(minutes: Option<i32>) -> String {
    match minutes {
        Some(minutes) if minutes > 0 => {
            let hours = minutes / 60;
            let mins = minutes % 60;
            if hours > 0 && mins > 0 {
                format!("{hours}h {mins}m")
            } else if hours > 0 {
                format!("{hours}h")
            } else {
                format!("{mins}m")
            }
        }
        _ => "Not calculated".to_string(),
    }
}

fn char_count(value: &str, max: usize) -> String {
    format!("{} / {max} characters", value.chars().count())
}

fn local_date() -> String {
    let stamp = now_stamp();
    stamp.get(0..10).unwrap_or_default().to_string()
}

fn empty_addendum(signature_name: String) -> CaseNoteAddendumInput {
    CaseNoteAddendumInput {
        reason: String::new(),
        information: String::new(),
        affected_categories: Vec::new(),
        follow_up: String::new(),
        signature_name,
    }
}

fn toggle_choice<T: Copy + Eq>(choices: &mut Vec<T>, choice: T, enabled: bool) {
    if enabled {
        if !choices.contains(&choice) && choices.len() < MAX_MULTISELECT_CHOICES {
            choices.push(choice);
        }
    } else {
        choices.retain(|item| *item != choice);
    }
}

fn labels<T: ChoiceLabel>(choices: &[T]) -> String {
    if choices.is_empty() {
        "Not provided".to_string()
    } else {
        choices
            .iter()
            .map(|choice| choice.choice_label())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

fn value_or_placeholder(value: String) -> String {
    if value.trim().is_empty() {
        "Not provided".to_string()
    } else {
        value
    }
}

fn state_badge(state: CaseNoteState) -> &'static str {
    match state {
        CaseNoteState::Draft => "bg-amber-500/15 text-amber-300 ring-1 ring-amber-500/30",
        CaseNoteState::Finalized => "bg-emerald-500/15 text-emerald-300 ring-1 ring-emerald-500/30",
        CaseNoteState::Discarded => "bg-slate-500/15 text-slate-400 ring-1 ring-slate-500/30",
        CaseNoteState::Legacy => "bg-sky-500/15 text-sky-300 ring-1 ring-sky-500/30",
    }
}

fn urgency_badge(urgency: Option<UrgencyLevel>) -> &'static str {
    match urgency {
        Some(UrgencyLevel::Urgent) => "bg-rose-500/15 text-rose-300 ring-1 ring-rose-500/30",
        Some(UrgencyLevel::Elevated) => "bg-amber-500/15 text-amber-300 ring-1 ring-amber-500/30",
        Some(UrgencyLevel::Routine) => "bg-slate-700/50 text-slate-300 ring-1 ring-slate-600/40",
        None => "bg-slate-800 text-slate-500 ring-1 ring-slate-700",
    }
}

#[component]
fn SafetyWarning() -> impl IntoView {
    view! {
        <div class="rounded-xl border border-amber-500/40 bg-amber-500/10 px-4 py-3 text-sm text-amber-100" role="alert">
            <strong class="font-semibold">"Safety warning: "</strong>
            {SAFETY_WARNING}
        </div>
    }
}

#[component]
fn FieldError(
    errors: RwSignal<Vec<CaseNoteValidationError>>,
    field: &'static str,
) -> impl IntoView {
    view! {
        <Show when=move || errors.get().iter().any(|err| err.field.as_str() == field)>
            <ul class="mt-1 space-y-0.5 text-xs text-rose-300" aria-live="polite">
                {move || {
                    errors
                        .get()
                        .into_iter()
                        .filter(move |err| err.field.as_str() == field)
                        .map(|err| view! { <li>{err.message}</li> })
                        .collect_view()
                }}
            </ul>
        </Show>
    }
}

#[component]
fn DetailRow(label: &'static str, value: String) -> impl IntoView {
    let empty = value == "Not provided" || value.trim().is_empty();
    view! {
        <div class="border-b border-slate-800 py-2 last:border-b-0">
            <dt class="text-xs font-medium text-slate-500">{label}</dt>
            <dd class=move || if empty { "mt-1 whitespace-pre-wrap text-sm italic text-slate-500" } else { "mt-1 whitespace-pre-wrap text-sm text-slate-200" }>
                {if empty { "Not provided".to_string() } else { value }}
            </dd>
        </div>
    }
}

#[component]
fn StructuredNoteReadOnly(draft: CaseNoteDraftInput, total_minutes: Option<i32>) -> impl IntoView {
    view! {
        <div class="grid gap-4 lg:grid-cols-2">
            <section class=PANEL_CLASS>
                <h2 class="text-sm font-semibold text-slate-200">"Activity"</h2>
                <dl class="mt-3">
                    <DetailRow label="Activity date" value=value_or_placeholder(draft.activity_date.clone()) />
                    <DetailRow label="Start time" value=value_or_placeholder(draft.start_time.clone()) />
                    <DetailRow label="End time" value=value_or_placeholder(draft.end_time.clone()) />
                    <DetailRow label="Duration" value=duration_label(total_minutes.or_else(|| draft.calculated_total_minutes())) />
                    <DetailRow label="Location" value=value_or_placeholder(draft.location.clone()) />
                    <DetailRow label="Delayed entry reason" value=value_or_placeholder(draft.delayed_entry_reason.clone()) />
                </dl>
            </section>
            <section class=PANEL_CLASS>
                <h2 class="text-sm font-semibold text-slate-200">"Interaction"</h2>
                <dl class="mt-3">
                    <DetailRow label="Primary interaction" value=draft.primary_interaction.map(|v| v.label().to_string()).unwrap_or_else(|| "Not provided".to_string()) />
                    <DetailRow label="Contact category" value=draft.contact_category.map(|v| v.label().to_string()).unwrap_or_else(|| "Not provided".to_string()) />
                    <DetailRow label="Contact direction" value=draft.contact_direction.map(|v| v.label().to_string()).unwrap_or_else(|| "Not provided".to_string()) />
                    <DetailRow label="Completion outcome" value=draft.completion_outcome.map(|v| v.label().to_string()).unwrap_or_else(|| "Not provided".to_string()) />
                    <DetailRow label="Participants" value=value_or_placeholder(draft.participant_summary.clone()) />
                    <DetailRow label="Service areas" value=labels(&draft.service_areas) />
                </dl>
            </section>
            <section class=PANEL_CLASS>
                <h2 class="text-sm font-semibold text-slate-200">"Facts and actions"</h2>
                <dl class="mt-3">
                    <DetailRow label="Purpose/objective" value=value_or_placeholder(draft.purpose.clone()) />
                    <DetailRow label="Client-reported information" value=value_or_placeholder(draft.client_reported_info.clone()) />
                    <DetailRow label="Verified/observed information" value=value_or_placeholder(draft.verified_observed_info.clone()) />
                    <DetailRow label="Information sources" value=labels(&draft.information_sources) />
                    <DetailRow label="Actions taken" value=value_or_placeholder(draft.actions_taken.clone()) />
                    <DetailRow label="Outcome/client response" value=value_or_placeholder(draft.outcome_response.clone()) />
                    <DetailRow label="Progress/barriers" value=value_or_placeholder(draft.progress_barriers.clone()) />
                </dl>
            </section>
            <section class=PANEL_CLASS>
                <h2 class="text-sm font-semibold text-slate-200">"Urgency and follow-up"</h2>
                <dl class="mt-3">
                    <DetailRow label="Urgency" value=draft.urgency.map(|v| v.label().to_string()).unwrap_or_else(|| "Not provided".to_string()) />
                    <DetailRow label="Urgency details" value=value_or_placeholder(draft.urgency_details.clone()) />
                    <DetailRow label="Next steps" value=if draft.next_steps_not_applicable { "Not applicable".to_string() } else { value_or_placeholder(draft.next_steps.clone()) } />
                    <DetailRow label="Narrative" value=value_or_placeholder(draft.narrative.clone()) />
                </dl>
            </section>
        </div>
    }
}

#[component]
pub fn CaseNotesPanel(case_id: String, can_add: bool) -> impl IntoView {
    let state = expect_context::<AppState>();
    let case_id_sv = StoredValue::new(case_id);
    let filters = RwSignal::new(CaseNoteListFilters::default());
    let applied_filters = RwSignal::new(CaseNoteListFilters::default());
    let filter_errors = RwSignal::new(Vec::<CaseNoteValidationError>::new());
    let items = RwSignal::new(Vec::<CaseNoteListItem>::new());
    let total = RwSignal::new(0i64);
    let window = RwSignal::new(PAGE_SIZE);
    let loading = RwSignal::new(false);
    let load_error = RwSignal::new(None::<String>);
    let reload = RwSignal::new(0u32);

    Effect::new(move |_| {
        if !state.is_authenticated() {
            return;
        }
        let case_id = case_id_sv.get_value();
        let applied = applied_filters.get();
        let limit = window.get();
        reload.track();
        loading.set(true);
        spawn_local(async move {
            match list_case_notes(case_id, applied, 0, limit).await {
                Ok(page) => {
                    items.set(page.items);
                    total.set(page.total);
                    load_error.set(None);
                }
                Err(err) => load_error.set(Some(err_text(err))),
            }
            loading.set(false);
        });
    });

    let apply_filters = move |_| match filters.get_untracked().validate() {
        Ok(valid) => {
            filter_errors.set(Vec::new());
            window.set(PAGE_SIZE);
            applied_filters.set(valid);
        }
        Err(errors) => filter_errors.set(errors),
    };

    let clear_filters = move |_| {
        let empty = CaseNoteListFilters::default();
        filters.set(empty.clone());
        applied_filters.set(empty);
        filter_errors.set(Vec::new());
        window.set(PAGE_SIZE);
    };

    let list_view = move || {
        if let Some(message) = load_error.get() {
            return view! { <p class="text-sm text-rose-300">"Could not load case notes: " {message}</p> }.into_any();
        }
        let rows = items.get();
        if rows.is_empty() {
            let message = if loading.get() {
                "Loading case notes…"
            } else {
                "No case notes match these filters."
            };
            return view! { <p class="text-sm text-slate-500">{message}</p> }.into_any();
        }
        let current_user_id = state.current_user_summary.get().map(|user| user.id);
        rows.into_iter()
            .map(|note| {
                let href = format!("/cases/{}/notes/{}", note.case_id, note.id);
                let state_label = note.state.label();
                let interaction = note.primary_interaction.map(|v| v.label()).unwrap_or("Unspecified");
                let urgency = note.urgency.map(|v| v.label()).unwrap_or("No urgency");
                let own = current_user_id.as_deref() == Some(note.author_user_id.as_str());
                let has_addenda = note.addendum_count > 0;
                let addendum_count = note.addendum_count;
                let finalized_at = note.finalized_at.clone();
                let has_finalized_at = !finalized_at.is_empty();
                let action = match note.state {
                    CaseNoteState::Draft if own => "Resume draft",
                    CaseNoteState::Draft => "Inspect draft",
                    CaseNoteState::Discarded => "View discarded draft",
                    _ => "Open note",
                };
                view! {
                    <article class="rounded-lg border border-slate-800 bg-slate-950 p-3">
                        <div class="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
                            <div class="min-w-0">
                                <div class="flex flex-wrap items-center gap-2">
                                    <span class=format!("inline-flex rounded-full px-2 py-0.5 text-xs font-medium {}", state_badge(note.state))>{state_label}</span>
                                    <span class=format!("inline-flex rounded-full px-2 py-0.5 text-xs font-medium {}", urgency_badge(note.urgency))>{urgency}</span>
                                    <span class="text-xs text-slate-500">{interaction}</span>
                                </div>
                                <h3 class="mt-2 text-sm font-semibold text-slate-100">
                                    {if note.activity_date.is_empty() { "No activity date".to_string() } else { note.activity_date.clone() }}
                                    " · " {duration_label(note.total_minutes)}
                                </h3>
                                <p class="mt-1 text-xs text-slate-500">
                                    "By " {note.author.clone()} " · created " {note.created_at.clone()}
                                    <Show when=move || has_finalized_at>
                                        " · finalized " {finalized_at.clone()}
                                    </Show>
                                    <Show when=move || has_addenda>
                                        " · " {addendum_count} " addendum" {if addendum_count == 1 { "" } else { "s" }}
                                    </Show>
                                </p>
                            </div>
                            <A href=href attr:class="shrink-0 rounded-full border border-primary-500/40 px-3 py-1.5 text-sm font-medium text-primary-300 hover:bg-primary-500/10">
                                {action}
                            </A>
                        </div>
                    </article>
                }
            })
            .collect_view()
            .into_any()
    };

    let footer = move || {
        let shown = items.get().len() as i64;
        let total_count = total.get();
        if total_count == 0 {
            return ().into_any();
        }
        view! {
            <div class="flex flex-col gap-2 text-xs text-slate-500 sm:flex-row sm:items-center sm:justify-between">
                <span>"Showing " {shown} " of " {total_count} " structured/legacy staff-visible notes"</span>
                <Show when=move || shown < total.get()>
                    <button
                        type="button"
                        prop:disabled=move || loading.get()
                        on:click=move |_| window.update(|count| *count += PAGE_SIZE)
                        class="rounded-full border border-slate-700 px-3 py-1.5 text-sm font-medium text-slate-200 hover:bg-slate-800 disabled:opacity-50"
                    >
                        {move || if loading.get() { "Loading…" } else { "Load more" }}
                    </button>
                </Show>
            </div>
        }
        .into_any()
    };

    let new_href = format!("/cases/{}/notes/new", case_id_sv.get_value());

    view! {
        <section class="space-y-4">
            <SafetyWarning />
            <div class=PANEL_CLASS>
                <div class="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
                    <div>
                        <h2 class="text-lg font-semibold text-slate-100">"Case notes"</h2>
                        <p class="mt-1 text-sm text-slate-500">"Structured staff notes, finalized legacy notes, drafts you own, and administratively visible draft rows."</p>
                    </div>
                    <Show when=move || can_add>
                        <A href=new_href.clone() attr:class="rounded-full bg-primary-500 px-4 py-2 text-sm font-semibold text-white hover:bg-primary-600">
                            "+ New structured note"
                        </A>
                    </Show>
                </div>
                <div class="mt-4 grid gap-3 md:grid-cols-2 xl:grid-cols-4">
                    <div>
                        <label class=LABEL_CLASS>"From date"</label>
                        <input
                            type="date"
                            class=INPUT_CLASS
                            prop:value=move || filters.get().start_date
                            on:input=move |ev| filters.update(|f| f.start_date = event_target_value(&ev))
                        />
                        <FieldError errors=filter_errors field="start_date" />
                    </div>
                    <div>
                        <label class=LABEL_CLASS>"To date"</label>
                        <input
                            type="date"
                            class=INPUT_CLASS
                            prop:value=move || filters.get().end_date
                            on:input=move |ev| filters.update(|f| f.end_date = event_target_value(&ev))
                        />
                        <FieldError errors=filter_errors field="end_date" />
                    </div>
                    <div>
                        <label class=LABEL_CLASS>"Author name"</label>
                        <input
                            class=INPUT_CLASS
                            maxlength=MAX_SHORT_TEXT_CHARS
                            placeholder="Name contains…"
                            prop:value=move || filters.get().author
                            on:input=move |ev| filters.update(|f| f.author = event_target_value(&ev))
                        />
                        <FieldError errors=filter_errors field="author" />
                    </div>
                    <div>
                        <label class=LABEL_CLASS>"Keyword"</label>
                        <input
                            class=INPUT_CLASS
                            maxlength=MAX_SHORT_TEXT_CHARS
                            placeholder="Search note text or author"
                            prop:value=move || filters.get().keyword
                            on:input=move |ev| filters.update(|f| f.keyword = event_target_value(&ev))
                        />
                        <FieldError errors=filter_errors field="keyword" />
                    </div>
                    <div>
                        <label class=LABEL_CLASS>"Interaction"</label>
                        <select class=INPUT_CLASS on:change=move |ev| {
                            let value = event_target_value(&ev);
                            filters.update(|f| f.primary_interaction = CaseNoteInteractionType::from_slug(&value));
                        }>
                            <option value="" selected=move || filters.get().primary_interaction.is_none()>"All interactions"</option>
                            {CaseNoteInteractionType::ALL.iter().copied().map(|choice| view! {
                                <option value=choice.slug() selected=move || filters.get().primary_interaction == Some(choice)>{choice.label()}</option>
                            }).collect_view()}
                        </select>
                    </div>
                    <div>
                        <label class=LABEL_CLASS>"State"</label>
                        <select class=INPUT_CLASS on:change=move |ev| {
                            let value = event_target_value(&ev);
                            filters.update(|f| f.state = CaseNoteState::from_slug(&value));
                        }>
                            <option value="" selected=move || filters.get().state.is_none()>"All states"</option>
                            {CaseNoteState::ALL.iter().copied().map(|choice| view! {
                                <option value=choice.slug() selected=move || filters.get().state == Some(choice)>{choice.label()}</option>
                            }).collect_view()}
                        </select>
                    </div>
                    <div>
                        <label class=LABEL_CLASS>"Urgency"</label>
                        <select class=INPUT_CLASS on:change=move |ev| {
                            let value = event_target_value(&ev);
                            filters.update(|f| f.urgency = UrgencyLevel::from_slug(&value));
                        }>
                            <option value="" selected=move || filters.get().urgency.is_none()>"All urgency levels"</option>
                            {UrgencyLevel::ALL.iter().copied().map(|choice| view! {
                                <option value=choice.slug() selected=move || filters.get().urgency == Some(choice)>{choice.label()}</option>
                            }).collect_view()}
                        </select>
                    </div>
                    <div class="flex items-end gap-2">
                        <button type="button" on:click=apply_filters class="rounded-full bg-primary-500 px-3 py-2 text-sm font-semibold text-white hover:bg-primary-600">"Apply"</button>
                        <button type="button" on:click=clear_filters class="rounded-full border border-slate-700 px-3 py-2 text-sm font-medium text-slate-300 hover:bg-slate-800">"Clear"</button>
                    </div>
                </div>
            </div>
            <div class="space-y-3">{list_view}</div>
            {footer}
        </section>
    }
}

#[component]
fn DraftForm(
    draft: RwSignal<CaseNoteDraftInput>,
    errors: RwSignal<Vec<CaseNoteValidationError>>,
) -> impl IntoView {
    let duration = move || draft.get().calculated_total_minutes();
    let delayed_reason_visible = move || {
        let current = draft.get();
        !current.activity_date.is_empty()
            && (current.activity_date != today() || !current.delayed_entry_reason.is_empty())
    };
    let urgency_details_visible = move || {
        let current = draft.get();
        matches!(
            current.urgency,
            Some(UrgencyLevel::Elevated | UrgencyLevel::Urgent)
        ) || !current.urgency_details.is_empty()
    };

    view! {
        <div class="space-y-5">
            <section class=PANEL_CLASS>
                <div class="mb-4">
                    <h2 class="text-sm font-semibold text-slate-200">"Activity timing"</h2>
                    <p class="mt-1 text-xs text-slate-500">"Dates and times must reflect the actual case activity. Duration is calculated from start and end time."</p>
                </div>
                <div class="grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
                    <div>
                        <label class=LABEL_CLASS for="case-note-activity-date">"Activity date"</label>
                        <input
                            id="case-note-activity-date"
                            type="date"
                            class=INPUT_CLASS
                            prop:value=move || draft.get().activity_date
                            on:input=move |ev| draft.update(|d| d.activity_date = event_target_value(&ev))
                        />
                        <FieldError errors=errors field="activity_date" />
                    </div>
                    <div>
                        <label class=LABEL_CLASS for="case-note-start-time">"Start time"</label>
                        <input
                            id="case-note-start-time"
                            type="time"
                            class=INPUT_CLASS
                            prop:value=move || draft.get().start_time
                            on:input=move |ev| draft.update(|d| d.start_time = event_target_value(&ev))
                        />
                        <FieldError errors=errors field="start_time" />
                    </div>
                    <div>
                        <label class=LABEL_CLASS for="case-note-end-time">"End time"</label>
                        <input
                            id="case-note-end-time"
                            type="time"
                            class=INPUT_CLASS
                            prop:value=move || draft.get().end_time
                            on:input=move |ev| draft.update(|d| d.end_time = event_target_value(&ev))
                        />
                        <FieldError errors=errors field="end_time" />
                    </div>
                    <div>
                        <span class=LABEL_CLASS>"Duration preview"</span>
                        <div class="mt-1 rounded-lg border border-slate-800 bg-slate-950 px-3 py-2 text-sm text-slate-200" aria-live="polite">
                            {move || duration_label(duration())}
                        </div>
                        <FieldError errors=errors field="total_minutes" />
                    </div>
                </div>
                <div class="mt-4">
                    <label class=LABEL_CLASS for="case-note-location">"Location"</label>
                    <input
                        id="case-note-location"
                        class=INPUT_CLASS
                        maxlength=MAX_SHORT_TEXT_CHARS
                        placeholder="Where the interaction or activity occurred"
                        prop:value=move || draft.get().location
                        on:input=move |ev| draft.update(|d| d.location = event_target_value(&ev))
                    />
                    <p class="mt-1 text-xs text-slate-600">{move || char_count(&draft.get().location, MAX_SHORT_TEXT_CHARS)}</p>
                    <FieldError errors=errors field="location" />
                </div>
                <Show when=delayed_reason_visible>
                    <div class="mt-4 rounded-lg border border-amber-500/20 bg-amber-500/5 p-3">
                        <label class=LABEL_CLASS for="case-note-delayed-reason">"Delayed entry reason"</label>
                        <textarea
                            id="case-note-delayed-reason"
                            class=INPUT_CLASS
                            rows="3"
                            maxlength=MAX_MEDIUM_TEXT_CHARS
                            placeholder="Required when the activity date is not today."
                            prop:value=move || draft.get().delayed_entry_reason
                            on:input=move |ev| draft.update(|d| d.delayed_entry_reason = event_target_value(&ev))
                        ></textarea>
                        <p class="mt-1 text-xs text-slate-600">{move || char_count(&draft.get().delayed_entry_reason, MAX_MEDIUM_TEXT_CHARS)}</p>
                        <FieldError errors=errors field="delayed_entry_reason" />
                    </div>
                </Show>
            </section>

            <section class=PANEL_CLASS>
                <div class="mb-4">
                    <h2 class="text-sm font-semibold text-slate-200">"Interaction details"</h2>
                    <p class="mt-1 text-xs text-slate-500">"Classify the contact, participants, service areas, and result."</p>
                </div>
                <div class="grid gap-4 sm:grid-cols-2 xl:grid-cols-4">
                    <div>
                        <label class=LABEL_CLASS>"Primary interaction"</label>
                        <select class=INPUT_CLASS on:change=move |ev| {
                            let value = event_target_value(&ev);
                            draft.update(|d| d.primary_interaction = CaseNoteInteractionType::from_slug(&value));
                        }>
                            <option value="" selected=move || draft.get().primary_interaction.is_none()>"Choose one"</option>
                            {CaseNoteInteractionType::ALL.iter().copied().map(|choice| view! {
                                <option value=choice.slug() selected=move || draft.get().primary_interaction == Some(choice)>{choice.label()}</option>
                            }).collect_view()}
                        </select>
                        <FieldError errors=errors field="primary_interaction" />
                    </div>
                    <div>
                        <label class=LABEL_CLASS>"Contact category"</label>
                        <select class=INPUT_CLASS on:change=move |ev| {
                            let value = event_target_value(&ev);
                            draft.update(|d| d.contact_category = ContactCategory::from_slug(&value));
                        }>
                            <option value="" selected=move || draft.get().contact_category.is_none()>"Choose one"</option>
                            {ContactCategory::ALL.iter().copied().map(|choice| view! {
                                <option value=choice.slug() selected=move || draft.get().contact_category == Some(choice)>{choice.label()}</option>
                            }).collect_view()}
                        </select>
                        <FieldError errors=errors field="contact_category" />
                    </div>
                    <div>
                        <label class=LABEL_CLASS>"Contact direction"</label>
                        <select class=INPUT_CLASS on:change=move |ev| {
                            let value = event_target_value(&ev);
                            draft.update(|d| d.contact_direction = ContactDirection::from_slug(&value));
                        }>
                            <option value="" selected=move || draft.get().contact_direction.is_none()>"Choose one"</option>
                            {ContactDirection::ALL.iter().copied().map(|choice| view! {
                                <option value=choice.slug() selected=move || draft.get().contact_direction == Some(choice)>{choice.label()}</option>
                            }).collect_view()}
                        </select>
                        <FieldError errors=errors field="contact_direction" />
                    </div>
                    <div>
                        <label class=LABEL_CLASS>"Completion outcome"</label>
                        <select class=INPUT_CLASS on:change=move |ev| {
                            let value = event_target_value(&ev);
                            draft.update(|d| d.completion_outcome = CompletionOutcome::from_slug(&value));
                        }>
                            <option value="" selected=move || draft.get().completion_outcome.is_none()>"Choose one"</option>
                            {CompletionOutcome::ALL.iter().copied().map(|choice| view! {
                                <option value=choice.slug() selected=move || draft.get().completion_outcome == Some(choice)>{choice.label()}</option>
                            }).collect_view()}
                        </select>
                        <FieldError errors=errors field="completion_outcome" />
                    </div>
                </div>
                <div class="mt-4">
                    <label class=LABEL_CLASS for="case-note-participants">"Participant summary"</label>
                    <textarea
                        id="case-note-participants"
                        class=INPUT_CLASS
                        rows="3"
                        maxlength=MAX_MEDIUM_TEXT_CHARS
                        placeholder="Who participated or was contacted; avoid unnecessary sensitive detail."
                        prop:value=move || draft.get().participant_summary
                        on:input=move |ev| draft.update(|d| d.participant_summary = event_target_value(&ev))
                    ></textarea>
                    <p class="mt-1 text-xs text-slate-600">{move || char_count(&draft.get().participant_summary, MAX_MEDIUM_TEXT_CHARS)}</p>
                    <FieldError errors=errors field="participant_summary" />
                </div>
                <div class="mt-4">
                    <span class=LABEL_CLASS>"Service areas"</span>
                    <p class="mt-1 text-xs text-slate-500">"Choose up to " {MAX_MULTISELECT_CHOICES} "."</p>
                    <div class="mt-2 grid gap-2 sm:grid-cols-2 lg:grid-cols-3">
                        {ServiceArea::ALL.iter().copied().map(|choice| view! {
                            <label class="flex items-center gap-2 rounded-lg border border-slate-800 bg-slate-950 px-3 py-2 text-sm text-slate-300">
                                <input
                                    type="checkbox"
                                    class="h-4 w-4 rounded border-slate-600 bg-slate-950"
                                    prop:checked=move || draft.get().service_areas.contains(&choice)
                                    on:change=move |ev| {
                                        let checked = event_target_checked(&ev);
                                        draft.update(|d| toggle_choice(&mut d.service_areas, choice, checked));
                                    }
                                />
                                {choice.label()}
                            </label>
                        }).collect_view()}
                    </div>
                    <FieldError errors=errors field="service_areas" />
                </div>
            </section>

            <section class=PANEL_CLASS>
                <div class="mb-4">
                    <h2 class="text-sm font-semibold text-slate-200">"Purpose, information, and action"</h2>
                    <p class="mt-1 text-xs text-slate-500">"Separate client-reported information from verified or directly observed information."</p>
                </div>
                <div class="space-y-4">
                    <div>
                        <label class=LABEL_CLASS for="case-note-purpose">"Purpose/objective"</label>
                        <textarea
                            id="case-note-purpose"
                            class=INPUT_CLASS
                            rows="4"
                            maxlength=MAX_LONG_TEXT_CHARS
                            prop:value=move || draft.get().purpose
                            on:input=move |ev| draft.update(|d| d.purpose = event_target_value(&ev))
                        ></textarea>
                        <p class="mt-1 text-xs text-slate-600">{move || char_count(&draft.get().purpose, MAX_LONG_TEXT_CHARS)}</p>
                        <FieldError errors=errors field="purpose" />
                    </div>
                    <div>
                        <label class=LABEL_CLASS for="case-note-client-reported">"Client-reported information"</label>
                        <textarea
                            id="case-note-client-reported"
                            class=INPUT_CLASS
                            rows="4"
                            maxlength=MAX_LONG_TEXT_CHARS
                            prop:value=move || draft.get().client_reported_info
                            on:input=move |ev| draft.update(|d| d.client_reported_info = event_target_value(&ev))
                        ></textarea>
                        <p class="mt-1 text-xs text-slate-600">{move || char_count(&draft.get().client_reported_info, MAX_LONG_TEXT_CHARS)}</p>
                        <FieldError errors=errors field="client_reported_info" />
                    </div>
                    <div>
                        <label class=LABEL_CLASS for="case-note-observed">"Verified/observed information"</label>
                        <textarea
                            id="case-note-observed"
                            class=INPUT_CLASS
                            rows="4"
                            maxlength=MAX_LONG_TEXT_CHARS
                            prop:value=move || draft.get().verified_observed_info
                            on:input=move |ev| draft.update(|d| d.verified_observed_info = event_target_value(&ev))
                        ></textarea>
                        <p class="mt-1 text-xs text-slate-600">{move || char_count(&draft.get().verified_observed_info, MAX_LONG_TEXT_CHARS)}</p>
                        <FieldError errors=errors field="verified_observed_info" />
                    </div>
                    <div>
                        <span class=LABEL_CLASS>"Information sources"</span>
                        <p class="mt-1 text-xs text-slate-500">"Choose up to " {MAX_MULTISELECT_CHOICES} "."</p>
                        <div class="mt-2 grid gap-2 sm:grid-cols-2 lg:grid-cols-3">
                            {InformationSource::ALL.iter().copied().map(|choice| view! {
                                <label class="flex items-center gap-2 rounded-lg border border-slate-800 bg-slate-950 px-3 py-2 text-sm text-slate-300">
                                    <input
                                        type="checkbox"
                                        class="h-4 w-4 rounded border-slate-600 bg-slate-950"
                                        prop:checked=move || draft.get().information_sources.contains(&choice)
                                        on:change=move |ev| {
                                            let checked = event_target_checked(&ev);
                                            draft.update(|d| toggle_choice(&mut d.information_sources, choice, checked));
                                        }
                                    />
                                    {choice.label()}
                                </label>
                            }).collect_view()}
                        </div>
                        <FieldError errors=errors field="information_sources" />
                    </div>
                    <div>
                        <label class=LABEL_CLASS for="case-note-actions">"Actions taken"</label>
                        <textarea
                            id="case-note-actions"
                            class=INPUT_CLASS
                            rows="4"
                            maxlength=MAX_LONG_TEXT_CHARS
                            prop:value=move || draft.get().actions_taken
                            on:input=move |ev| draft.update(|d| d.actions_taken = event_target_value(&ev))
                        ></textarea>
                        <p class="mt-1 text-xs text-slate-600">{move || char_count(&draft.get().actions_taken, MAX_LONG_TEXT_CHARS)}</p>
                        <FieldError errors=errors field="actions_taken" />
                    </div>
                    <div>
                        <label class=LABEL_CLASS for="case-note-outcome">"Outcome/client response"</label>
                        <textarea
                            id="case-note-outcome"
                            class=INPUT_CLASS
                            rows="4"
                            maxlength=MAX_LONG_TEXT_CHARS
                            prop:value=move || draft.get().outcome_response
                            on:input=move |ev| draft.update(|d| d.outcome_response = event_target_value(&ev))
                        ></textarea>
                        <p class="mt-1 text-xs text-slate-600">{move || char_count(&draft.get().outcome_response, MAX_LONG_TEXT_CHARS)}</p>
                        <FieldError errors=errors field="outcome_response" />
                    </div>
                    <div>
                        <label class=LABEL_CLASS for="case-note-progress-barriers">"Progress and barriers"</label>
                        <textarea
                            id="case-note-progress-barriers"
                            class=INPUT_CLASS
                            rows="4"
                            maxlength=MAX_LONG_TEXT_CHARS
                            placeholder="Describe progress made and barriers encountered."
                            prop:value=move || draft.get().progress_barriers
                            on:input=move |ev| draft.update(|d| d.progress_barriers = event_target_value(&ev))
                        ></textarea>
                        <p class="mt-1 text-xs text-slate-600">{move || char_count(&draft.get().progress_barriers, MAX_LONG_TEXT_CHARS)}</p>
                        <FieldError errors=errors field="progress_barriers" />
                    </div>
                </div>
            </section>

            <section class=PANEL_CLASS>
                <div class="mb-4">
                    <h2 class="text-sm font-semibold text-slate-200">"Urgency, next steps, and narrative"</h2>
                    <p class="mt-1 text-xs text-slate-500">"Urgent concerns still require direct escalation outside this note."</p>
                </div>
                <div class="grid gap-4 sm:grid-cols-2">
                    <div>
                        <label class=LABEL_CLASS>"Urgency"</label>
                        <select class=INPUT_CLASS on:change=move |ev| {
                            let value = event_target_value(&ev);
                            draft.update(|d| d.urgency = UrgencyLevel::from_slug(&value));
                        }>
                            <option value="" selected=move || draft.get().urgency.is_none()>"Choose one"</option>
                            {UrgencyLevel::ALL.iter().copied().map(|choice| view! {
                                <option value=choice.slug() selected=move || draft.get().urgency == Some(choice)>{choice.label()}</option>
                            }).collect_view()}
                        </select>
                        <FieldError errors=errors field="urgency" />
                    </div>
                    <div class="rounded-lg border border-amber-500/20 bg-amber-500/5 px-3 py-2 text-xs text-amber-100">
                        {SAFETY_WARNING}
                    </div>
                </div>
                <Show when=urgency_details_visible>
                    <div class="mt-4">
                        <label class=LABEL_CLASS for="case-note-urgency-details">"Urgency details"</label>
                        <textarea
                            id="case-note-urgency-details"
                            class=INPUT_CLASS
                            rows="4"
                            maxlength=MAX_LONG_TEXT_CHARS
                            placeholder="Required for elevated or urgent notes; include escalation steps taken."
                            prop:value=move || draft.get().urgency_details
                            on:input=move |ev| draft.update(|d| d.urgency_details = event_target_value(&ev))
                        ></textarea>
                        <p class="mt-1 text-xs text-slate-600">{move || char_count(&draft.get().urgency_details, MAX_LONG_TEXT_CHARS)}</p>
                        <FieldError errors=errors field="urgency_details" />
                    </div>
                </Show>
                <div class="mt-4 space-y-2">
                    <label class="inline-flex items-center gap-2 text-sm text-slate-300">
                        <input
                            type="checkbox"
                            class="h-4 w-4 rounded border-slate-600 bg-slate-950"
                            prop:checked=move || draft.get().next_steps_not_applicable
                            on:change=move |ev| {
                                let checked = event_target_checked(&ev);
                                draft.update(|d| {
                                    d.next_steps_not_applicable = checked;
                                    if checked {
                                        d.next_steps.clear();
                                    }
                                });
                            }
                        />
                        "Next steps are not applicable"
                    </label>
                    <label class=LABEL_CLASS for="case-note-next-steps">"Next steps/follow-up"</label>
                    <textarea
                        id="case-note-next-steps"
                        class=INPUT_CLASS
                        rows="4"
                        maxlength=MAX_LONG_TEXT_CHARS
                        prop:disabled=move || draft.get().next_steps_not_applicable
                        prop:value=move || draft.get().next_steps
                        on:input=move |ev| draft.update(|d| d.next_steps = event_target_value(&ev))
                    ></textarea>
                    <p class="mt-1 text-xs text-slate-600">{move || char_count(&draft.get().next_steps, MAX_LONG_TEXT_CHARS)}</p>
                    <FieldError errors=errors field="next_steps" />
                </div>
                <div class="mt-4">
                    <label class=LABEL_CLASS for="case-note-narrative">"Concise factual narrative"</label>
                    <textarea
                        id="case-note-narrative"
                        class=INPUT_CLASS
                        rows="7"
                        maxlength=MAX_NARRATIVE_CHARS
                        placeholder="Use objective, factual language. Do not include attachments or PDF content here."
                        prop:value=move || draft.get().narrative
                        on:input=move |ev| draft.update(|d| d.narrative = event_target_value(&ev))
                    ></textarea>
                    <p class="mt-1 text-xs text-slate-600">{move || char_count(&draft.get().narrative, MAX_NARRATIVE_CHARS)}</p>
                    <FieldError errors=errors field="narrative" />
                </div>
            </section>
        </div>
    }
}

#[component]
fn FinalizePanel(
    signature: RwSignal<String>,
    accuracy_confirmed: RwSignal<bool>,
    errors: RwSignal<Vec<CaseNoteValidationError>>,
) -> impl IntoView {
    view! {
        <section class="rounded-xl border border-primary-500/30 bg-primary-500/5 p-4">
            <h2 class="text-sm font-semibold text-primary-200">"Finalize note"</h2>
            <p class="mt-1 text-sm text-slate-400">
                "Finalized notes are read-only. Later corrections must be recorded as immutable addenda."
            </p>
            <label class="mt-3 flex items-start gap-2 text-sm text-slate-300">
                <input
                    type="checkbox"
                    class="mt-1 h-4 w-4 rounded border-slate-600 bg-slate-950"
                    prop:checked=move || accuracy_confirmed.get()
                    on:change=move |ev| accuracy_confirmed.set(event_target_checked(&ev))
                />
                <span>"I confirm this note is accurate to the best of my knowledge and follows Mommy's Heart case note policy."</span>
            </label>
            <Show when=move || errors.get().iter().any(|err| err.field == "accuracy_confirmed")>
                <p class="mt-1 text-xs text-rose-300">"Confirm accuracy before finalizing."</p>
            </Show>
            <div class="mt-3">
                <label class=LABEL_CLASS for="case-note-signature">"Typed signature"</label>
                <input
                    id="case-note-signature"
                    class=INPUT_CLASS
                    maxlength=MAX_SHORT_TEXT_CHARS
                    placeholder="Type your full account name"
                    prop:value=move || signature.get()
                    on:input=move |ev| signature.set(event_target_value(&ev))
                />
                <p class="mt-1 text-xs text-slate-500">"Your typed signature must match your current account name."</p>
                <FieldError errors=errors field="signature_name" />
            </div>
        </section>
    }
}

#[component]
fn AddendumForm(
    note_id: String,
    input: RwSignal<CaseNoteAddendumInput>,
    errors: RwSignal<Vec<CaseNoteValidationError>>,
    busy: RwSignal<bool>,
    message: RwSignal<Option<String>>,
    on_added: Callback<()>,
) -> impl IntoView {
    let note_id = StoredValue::new(note_id);
    let submit = move |_| {
        let input_value = input.get_untracked();
        let mut client_errors = Vec::new();
        if input_value.reason.trim().is_empty() {
            push_error(&mut client_errors, "reason", "Addendum reason is required.");
        }
        if input_value.information.trim().is_empty() {
            push_error(
                &mut client_errors,
                "information",
                "Supplemental or corrected information is required.",
            );
        }
        if input_value.affected_categories.is_empty() {
            push_error(
                &mut client_errors,
                "affected_categories",
                "Choose at least one affected category.",
            );
        }
        if input_value.signature_name.trim().is_empty() {
            push_error(
                &mut client_errors,
                "signature_name",
                "Type your full name to sign the addendum.",
            );
        }
        if !client_errors.is_empty() {
            errors.set(client_errors);
            return;
        }
        errors.set(Vec::new());
        message.set(None);
        busy.set(true);
        let id = note_id.get_value();
        spawn_local(async move {
            match add_case_note_addendum(id, input_value).await {
                Ok(_) => {
                    message.set(Some("Addendum added.".to_string()));
                    on_added.run(());
                }
                Err(err) => message.set(Some(err_text(err))),
            }
            busy.set(false);
        });
    };

    view! {
        <section class=PANEL_CLASS>
            <h2 class="text-sm font-semibold text-slate-200">"Add immutable addendum"</h2>
            <p class="mt-1 text-sm text-slate-500">
                "Use an addendum for corrections or supplemental facts. Existing finalized or legacy note text is never edited."
            </p>
            <div class="mt-4 space-y-4">
                <div>
                    <label class=LABEL_CLASS for="addendum-reason">"Reason"</label>
                    <textarea
                        id="addendum-reason"
                        class=INPUT_CLASS
                        rows="3"
                        maxlength=MAX_MEDIUM_TEXT_CHARS
                        prop:value=move || input.get().reason
                        on:input=move |ev| input.update(|i| i.reason = event_target_value(&ev))
                    ></textarea>
                    <p class="mt-1 text-xs text-slate-600">{move || char_count(&input.get().reason, MAX_MEDIUM_TEXT_CHARS)}</p>
                    <FieldError errors=errors field="reason" />
                </div>
                <div>
                    <label class=LABEL_CLASS for="addendum-information">"Supplemental or corrected information"</label>
                    <textarea
                        id="addendum-information"
                        class=INPUT_CLASS
                        rows="5"
                        maxlength=MAX_LONG_TEXT_CHARS
                        prop:value=move || input.get().information
                        on:input=move |ev| input.update(|i| i.information = event_target_value(&ev))
                    ></textarea>
                    <p class="mt-1 text-xs text-slate-600">{move || char_count(&input.get().information, MAX_LONG_TEXT_CHARS)}</p>
                    <FieldError errors=errors field="information" />
                </div>
                <div>
                    <span class=LABEL_CLASS>"Affected categories"</span>
                    <div class="mt-2 grid gap-2 sm:grid-cols-2 lg:grid-cols-3">
                        {AddendumCategory::ALL.iter().copied().map(|choice| view! {
                            <label class="flex items-center gap-2 rounded-lg border border-slate-800 bg-slate-950 px-3 py-2 text-sm text-slate-300">
                                <input
                                    type="checkbox"
                                    class="h-4 w-4 rounded border-slate-600 bg-slate-950"
                                    prop:checked=move || input.get().affected_categories.contains(&choice)
                                    on:change=move |ev| {
                                        input.update(|i| toggle_choice(&mut i.affected_categories, choice, event_target_checked(&ev)));
                                    }
                                />
                                {choice.label()}
                            </label>
                        }).collect_view()}
                    </div>
                    <FieldError errors=errors field="affected_categories" />
                </div>
                <div>
                    <label class=LABEL_CLASS for="addendum-follow-up">"Follow-up"</label>
                    <textarea
                        id="addendum-follow-up"
                        class=INPUT_CLASS
                        rows="3"
                        maxlength=MAX_LONG_TEXT_CHARS
                        prop:value=move || input.get().follow_up
                        on:input=move |ev| input.update(|i| i.follow_up = event_target_value(&ev))
                    ></textarea>
                    <p class="mt-1 text-xs text-slate-600">{move || char_count(&input.get().follow_up, MAX_LONG_TEXT_CHARS)}</p>
                    <FieldError errors=errors field="follow_up" />
                </div>
                <div>
                    <label class=LABEL_CLASS for="addendum-signature">"Typed signature"</label>
                    <input
                        id="addendum-signature"
                        class=INPUT_CLASS
                        maxlength=MAX_SHORT_TEXT_CHARS
                        prop:value=move || input.get().signature_name
                        on:input=move |ev| input.update(|i| i.signature_name = event_target_value(&ev))
                    />
                    <FieldError errors=errors field="signature_name" />
                </div>
                <Show when=move || message.get().is_some()>
                    <p class="text-sm text-slate-300">{move || message.get().unwrap_or_default()}</p>
                </Show>
                <button
                    type="button"
                    prop:disabled=move || busy.get()
                    on:click=submit
                    class="rounded-full bg-primary-500 px-4 py-2 text-sm font-semibold text-white hover:bg-primary-600 disabled:opacity-50"
                >
                    {move || if busy.get() { "Adding…" } else { "Add addendum" }}
                </button>
            </div>
        </section>
    }
}

#[component]
fn AddendaList(note: CaseNoteDetail) -> impl IntoView {
    if note.addenda.is_empty() {
        return view! { <p class="text-sm text-slate-500">"No addenda recorded."</p> }.into_any();
    }
    note.addenda
        .into_iter()
        .map(|addendum| {
            view! {
                <article class="rounded-lg border border-slate-800 bg-slate-950 p-3">
                    <div class="flex flex-wrap items-center justify-between gap-2">
                        <h3 class="text-sm font-semibold text-slate-200">"Addendum signed " {addendum.signed_at.clone()}</h3>
                        <span class="text-xs text-slate-500">{addendum.author.clone()}</span>
                    </div>
                    <dl class="mt-2 space-y-2">
                        <DetailRow label="Reason" value=value_or_placeholder(addendum.reason.clone()) />
                        <DetailRow label="Information" value=value_or_placeholder(addendum.information.clone()) />
                        <DetailRow label="Affected categories" value=labels(&addendum.affected_categories) />
                        <DetailRow label="Follow-up" value=value_or_placeholder(addendum.follow_up.clone()) />
                        <DetailRow label="Signature" value=value_or_placeholder(addendum.signature_name.clone()) />
                    </dl>
                </article>
            }
        })
        .collect_view()
        .into_any()
}

/// The note's filed document in the case's SharePoint `Case Notes` folder.
///
/// The document is the record as a person reads it, and the library is where it
/// lives — so this panel only ever *links* to it. Download goes through the same
/// case-documents route as every other case file, and "Open in SharePoint" goes
/// straight to the library.
#[component]
fn FiledRecordPanel(note_id: String, case_id: String, can_file: bool) -> impl IntoView {
    let note_sv = StoredValue::new(note_id);
    let case_sv = StoredValue::new(case_id);
    let document = RwSignal::new(None::<CaseNoteDocument>);
    let loading = RwSignal::new(true);
    let busy = RwSignal::new(false);
    let error = RwSignal::new(String::new());

    Effect::new(move |_| {
        let id = note_sv.get_value();
        if id.is_empty() {
            return;
        }
        loading.set(true);
        spawn_local(async move {
            match load_case_note_document(id).await {
                Ok(found) => {
                    document.set(Some(found));
                    error.set(String::new());
                }
                Err(e) => error.set(err_text(e)),
            }
            loading.set(false);
        });
    });

    let file_now = move |_| {
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        error.set(String::new());
        let id = note_sv.get_value();
        spawn_local(async move {
            match file_case_note_document(id).await {
                Ok(found) => document.set(Some(found)),
                Err(e) => error.set(err_text(e)),
            }
            busy.set(false);
        });
    };

    let body = move || {
        if loading.get() {
            return view! { <Loading label="Checking the filed record\u{2026}" /> }.into_any();
        }
        let Some(doc) = document.get() else {
            return ().into_any();
        };
        if !doc.is_filed() {
            return view! {
                <div class="space-y-3">
                    <p class="text-sm text-slate-400">
                        "This note has not been written to the case's Case Notes folder yet. \
                         Filing runs in the background just after a note is finalized, so this \
                         usually clears on its own within a few seconds."
                    </p>
                    <Show when=move || can_file>
                        <button
                            type="button"
                            prop:disabled=move || busy.get()
                            on:click=file_now
                            class="rounded-full border border-slate-700 px-3 py-2 text-sm font-semibold text-slate-200 hover:bg-slate-800 disabled:opacity-50"
                        >
                            {move || if busy.get() { "Filing\u{2026}" } else { "File it now" }}
                        </button>
                    </Show>
                </div>
            }
            .into_any();
        }

        let download_href = format!(
            "/api/cases/{}/documents/download?path={}",
            case_sv.get_value(),
            encode_query(&doc.path)
        );
        let web_url = doc.web_url.clone();
        let has_web_url = !web_url.is_empty();
        let stale = !doc.is_current;
        view! {
            <div class="space-y-3">
                <div>
                    <p class="text-sm font-medium text-slate-200">{doc.file_name.clone()}</p>
                    <p class="mt-1 text-xs text-slate-500">
                        "In this case's Case Notes folder · filed " {doc.filed_at.clone()}
                    </p>
                </div>
                <Show when=move || stale>
                    <p class="rounded-lg border border-amber-500/30 bg-amber-500/10 px-3 py-2 text-xs text-amber-100">
                        "An addendum was added after this document was written. The document is \
                         being written again; reload in a moment to get the complete record."
                    </p>
                </Show>
                <div class="flex flex-wrap gap-2">
                    <a
                        href=download_href
                        class="rounded-full border border-primary-500/40 px-3 py-1.5 text-sm font-medium text-primary-300 hover:bg-primary-500/10"
                    >
                        "Download"
                    </a>
                    <Show when=move || has_web_url>
                        <a
                            href=web_url.clone()
                            target="_blank"
                            rel="noreferrer"
                            class="rounded-full border border-slate-700 px-3 py-1.5 text-sm font-medium text-slate-300 hover:bg-slate-800"
                        >
                            "Open in SharePoint"
                        </a>
                    </Show>
                    <Show when=move || can_file && stale>
                        <button
                            type="button"
                            prop:disabled=move || busy.get()
                            on:click=file_now
                            class="rounded-full border border-slate-700 px-3 py-1.5 text-sm font-medium text-slate-300 hover:bg-slate-800 disabled:opacity-50"
                        >
                            {move || if busy.get() { "Filing\u{2026}" } else { "Write it again now" }}
                        </button>
                    </Show>
                </div>
            </div>
        }
        .into_any()
    };

    view! {
        <section class=PANEL_CLASS>
            <h2 class="text-sm font-semibold text-slate-200">"Filed record"</h2>
            <p class="mt-1 text-xs text-slate-500">
                "Every finalized note is filed as a Word document in this case's Case Notes \
                 folder, alongside the rest of the case's paperwork. That folder is where the \
                 document lives."
            </p>
            <div class="mt-3">{body}</div>
            <Show when=move || !error.get().is_empty()>
                <p class="mt-3 text-sm text-rose-300">{move || error.get()}</p>
            </Show>
        </section>
    }
}

#[component]
pub fn NewCaseNotePage() -> impl IntoView {
    let state = expect_context::<AppState>();
    let params = use_params_map();
    let navigate = use_navigate();
    let case_id = StoredValue::new(params.read_untracked().get("case_id").unwrap_or_default());
    let draft = RwSignal::new(CaseNoteDraftInput::default());
    let errors = RwSignal::new(Vec::<CaseNoteValidationError>::new());
    let signature = RwSignal::new(
        state
            .current_user_summary
            .get_untracked()
            .map(|user| user.full_name())
            .unwrap_or_default(),
    );
    let accuracy_confirmed = RwSignal::new(false);
    let busy = RwSignal::new(false);
    let message = RwSignal::new(None::<String>);
    let access_checked = RwSignal::new(false);
    let route_allowed = RwSignal::new(false);

    Effect::new(move |_| {
        if !state.is_authenticated() {
            return;
        }
        let id = case_id.get_value();
        spawn_local(async move {
            route_allowed.set(
                case_note_access(id)
                    .await
                    .is_ok_and(|access| access.can_add),
            );
            access_checked.set(true);
        });
    });

    let finalize = Callback::new({
        let navigate = navigate.clone();
        move |_: ()| {
            if busy.get_untracked() {
                return;
            }
            let case_id = case_id.get_value();
            let input = draft.get_untracked();
            let signature_value = signature.get_untracked();
            let mut client_errors = Vec::new();
            let today = local_date();
            let current_name = state
                .current_user_summary
                .get_untracked()
                .map(|user| user.full_name())
                .unwrap_or_default();
            if let Err(errs) = input.validate_for_finalization(
                &today,
                &signature_value,
                &current_name,
                accuracy_confirmed.get_untracked(),
            ) {
                client_errors.extend(errs);
            }
            if !accuracy_confirmed.get_untracked() {
                push_error(
                    &mut client_errors,
                    "accuracy_confirmed",
                    "Confirm accuracy before finalizing.",
                );
            }
            if !client_errors.is_empty() {
                errors.set(client_errors);
                return;
            }
            errors.set(Vec::new());
            busy.set(true);
            message.set(None);
            let navigate = navigate.clone();
            spawn_local(async move {
                match create_and_finalize_case_note(case_id.clone(), input, signature_value, true)
                    .await
                {
                    Ok(finalized) => navigate(
                        &format!("/cases/{case_id}/notes/{}", finalized.id),
                        Default::default(),
                    ),
                    Err(err) => message.set(Some(err_text(err))),
                }
                busy.set(false);
            });
        }
    });

    require_login(state, move || {
        if !access_checked.get() {
            return view! { <Layout title="New case note".to_string()><Loading label="Checking access…" /></Layout> }.into_any();
        }
        if !route_allowed.get() {
            return view! { <Layout title="New case note".to_string()><p class="text-sm text-slate-400">"Case note not found."</p></Layout> }.into_any();
        }
        view! {
            <Layout title="New case note".to_string()>
                <div class="mx-auto max-w-6xl space-y-5">
                    <div class="flex flex-col gap-2 sm:flex-row sm:items-center sm:justify-between">
                        <div>
                            <A href=format!("/cases") attr:class="text-sm text-primary-300 hover:text-primary-200">"← Back to cases"</A>
                            <h1 class="mt-2 text-2xl font-semibold text-slate-100">"New structured case note"</h1>
                            <p class="text-sm text-slate-500">"Case " {case_id.get_value()}</p>
                        </div>
                        <button type="button" prop:disabled=move || busy.get() on:click=move |_| finalize.run(()) class="rounded-full bg-primary-500 px-3 py-2 text-sm font-semibold text-white hover:bg-primary-600 disabled:opacity-50">
                            {move || if busy.get() { "Creating…" } else { "Create case note" }}
                        </button>
                    </div>
                    <SafetyWarning />
                    <DraftForm draft=draft errors=errors />
                    <FinalizePanel signature=signature accuracy_confirmed=accuracy_confirmed errors=errors />
                    <Show when=move || message.get().is_some()>
                        <p class="rounded-lg border border-rose-500/30 bg-rose-500/10 px-3 py-2 text-sm text-rose-200">{move || message.get().unwrap_or_default()}</p>
                    </Show>
                </div>
            </Layout>
        }
        .into_any()
    })
}

#[component]
pub fn CaseNoteDetailPage() -> impl IntoView {
    let state = expect_context::<AppState>();
    let params = use_params_map();
    let case_id = StoredValue::new(params.read_untracked().get("case_id").unwrap_or_default());
    let note_id = StoredValue::new(params.read_untracked().get("note_id").unwrap_or_default());

    let note = RwSignal::new(None::<CaseNoteDetail>);
    let loading = RwSignal::new(true);
    let load_error = RwSignal::new(None::<String>);
    let can_add_to_case = RwSignal::new(false);
    let draft = RwSignal::new(CaseNoteDraftInput::default());
    let errors = RwSignal::new(Vec::<CaseNoteValidationError>::new());
    let signature = RwSignal::new(String::new());
    let accuracy_confirmed = RwSignal::new(false);
    let busy = RwSignal::new(false);
    let message = RwSignal::new(None::<String>);
    let addendum_errors = RwSignal::new(Vec::<CaseNoteValidationError>::new());
    let addendum_busy = RwSignal::new(false);
    let addendum_message = RwSignal::new(None::<String>);
    let addendum_input = RwSignal::new(empty_addendum(
        state
            .current_user_summary
            .get_untracked()
            .map(|user| user.full_name())
            .unwrap_or_default(),
    ));
    let reload = RwSignal::new(0u32);

    Effect::new(move |_| {
        if !state.is_authenticated() {
            return;
        }
        let id = note_id.get_value();
        let route_case_id = case_id.get_value();
        if id.is_empty() || route_case_id.is_empty() {
            return;
        }
        reload.track();
        loading.set(true);
        load_error.set(None);
        note.set(None);
        spawn_local(async move {
            let access = case_note_access(route_case_id).await.ok();
            if !access.is_some_and(|value| value.can_view) {
                load_error.set(Some("Case note not found.".to_string()));
                loading.set(false);
                return;
            }
            can_add_to_case.set(access.is_some_and(|value| value.can_add));
            match load_case_note(id).await {
                Ok(Some(detail)) => {
                    if detail.state == CaseNoteState::Draft && detail.content_revealed {
                        draft.set(detail.draft.clone());
                    }
                    note.set(Some(detail));
                }
                Ok(None) | Err(_) => load_error.set(Some("Case note not found.".to_string())),
            }
            loading.set(false);
        });
    });

    let current_user_id = move || state.current_user_summary.get().map(|user| user.id);
    let note_matches_route = move || {
        note.get()
            .is_some_and(|detail| detail.case_id == case_id.get_value())
    };
    let is_own_draft = move || {
        note.get().is_some_and(|detail| {
            detail.state == CaseNoteState::Draft
                && current_user_id().as_deref() == Some(detail.author_user_id.as_str())
        })
    };
    let is_admin_visible_draft = move || {
        note.get().is_some_and(|detail| {
            detail.state == CaseNoteState::Draft
                && detail.content_revealed
                && current_user_id().as_deref() != Some(detail.author_user_id.as_str())
                && state.has_operations_admin_permissions()
        })
    };
    let is_redacted_admin_draft = move || {
        note.get().is_some_and(|detail| {
            detail.state == CaseNoteState::Draft
                && !detail.content_revealed
                && state.has_operations_admin_permissions()
        })
    };

    let save_draft = move |_| {
        if busy.get_untracked() || !is_own_draft() {
            return;
        }
        let Some(detail) = note.get_untracked() else {
            return;
        };
        let input = draft.get_untracked();
        match input.validate_draft() {
            Ok(()) => errors.set(Vec::new()),
            Err(errs) => {
                errors.set(errs);
                return;
            }
        }
        busy.set(true);
        message.set(None);
        spawn_local(async move {
            match crate::server_fns::case_notes::save_case_note_draft(detail.id.clone(), input)
                .await
            {
                Ok(updated) => {
                    draft.set(updated.draft.clone());
                    note.set(Some(updated));
                    message.set(Some("Draft saved.".to_string()));
                }
                Err(err) => message.set(Some(err_text(err))),
            }
            busy.set(false);
        });
    };

    let finalize_draft = move |_| {
        if busy.get_untracked() || !is_own_draft() {
            return;
        }
        let Some(detail) = note.get_untracked() else {
            return;
        };
        let input = draft.get_untracked();
        let signature_value = signature.get_untracked();
        let mut client_errors = Vec::new();
        let today = local_date();
        let current_name = state
            .current_user_summary
            .get_untracked()
            .map(|user| user.full_name())
            .unwrap_or_default();
        if let Err(errs) = input.validate_for_finalization(
            &today,
            &signature_value,
            &current_name,
            accuracy_confirmed.get_untracked(),
        ) {
            client_errors.extend(errs);
        }
        if !accuracy_confirmed.get_untracked() {
            push_error(
                &mut client_errors,
                "accuracy_confirmed",
                "Confirm accuracy before finalizing.",
            );
        }
        if !client_errors.is_empty() {
            errors.set(client_errors);
            return;
        }
        errors.set(Vec::new());
        busy.set(true);
        message.set(None);
        spawn_local(async move {
            match finalize_case_note_draft(detail.id, input, signature_value, true).await {
                Ok(updated) => {
                    note.set(Some(updated));
                    message.set(Some("Note finalized.".to_string()));
                }
                Err(err) => message.set(Some(err_text(err))),
            }
            busy.set(false);
        });
    };

    let discard_draft = move |_| {
        if busy.get_untracked() || !is_own_draft() {
            return;
        }
        let Some(detail) = note.get_untracked() else {
            return;
        };
        busy.set(true);
        message.set(None);
        spawn_local(async move {
            match discard_case_note_draft(detail.id).await {
                Ok(()) => {
                    message.set(Some("Draft discarded.".to_string()));
                    reload.update(|n| *n += 1);
                }
                Err(err) => message.set(Some(err_text(err))),
            }
            busy.set(false);
        });
    };

    let inspect_draft = move |_| {
        if busy.get_untracked() || !is_redacted_admin_draft() {
            return;
        }
        let note_id = note.get_untracked().map(|detail| detail.id);
        let Some(note_id) = note_id else {
            return;
        };
        busy.set(true);
        message.set(None);
        spawn_local(async move {
            match admin_inspect_case_note_draft(note_id).await {
                Ok(inspected) => {
                    note.set(Some(inspected));
                    message.set(Some(
                        "Draft inspection recorded in the audit log.".to_string(),
                    ));
                }
                Err(err) => message.set(Some(err_text(err))),
            }
            busy.set(false);
        });
    };

    let on_added = Callback::new(move |_| {
        let signature_name = state
            .current_user_summary
            .get_untracked()
            .map(|user| user.full_name())
            .unwrap_or_default();
        addendum_input.set(empty_addendum(signature_name));
        reload.update(|n| *n += 1);
    });

    let content = move || {
        if loading.get() {
            return view! { <div class=PANEL_CLASS><Loading label="Loading case note…" /></div> }
                .into_any();
        }
        if let Some(err) = load_error.get() {
            return view! { <p class="rounded-lg border border-rose-500/30 bg-rose-500/10 px-3 py-2 text-sm text-rose-200">{err}</p> }.into_any();
        }
        if !note_matches_route() {
            return view! { <p class="text-sm text-slate-400">"Case note not found."</p> }
                .into_any();
        }
        let Some(detail) = note.get() else {
            return ().into_any();
        };
        let state_label = detail.state.label();
        let finalized_at = detail.finalized_at.clone();
        let discarded_at = detail.discarded_at.clone();
        let signature_name = detail.signature_name.clone();
        let has_finalized_at = !finalized_at.is_empty();
        let show_discarded_at =
            detail.state == CaseNoteState::Discarded && !discarded_at.is_empty();
        let header = view! {
            <div class=PANEL_CLASS>
                <div class="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
                    <div>
                        <div class="flex flex-wrap items-center gap-2">
                            <span class=format!("inline-flex rounded-full px-2 py-0.5 text-xs font-medium {}", state_badge(detail.state))>{state_label}</span>
                            <span class="text-xs text-slate-500">{detail.audience.label()}</span>
                        </div>
                        <h1 class="mt-2 text-xl font-semibold text-slate-100">"Case note " {detail.id.clone()}</h1>
                        <p class="mt-1 text-sm text-slate-500">
                            "By " {detail.author.clone()} " · created " {detail.created_at.clone()} " · updated " {detail.updated_at.clone()}
                        </p>
                        <Show when=move || has_finalized_at>
                            <p class="mt-1 text-xs text-slate-500">"Finalized " {finalized_at.clone()} " by typed signature " {signature_name.clone()}</p>
                        </Show>
                        <Show when=move || show_discarded_at>
                            <p class="mt-1 text-xs text-slate-500">"Discarded " {discarded_at.clone()}</p>
                        </Show>
                    </div>
                    <A href="/cases" attr:class="rounded-full border border-slate-700 px-3 py-1.5 text-sm font-medium text-slate-300 hover:bg-slate-800">"Back to cases"</A>
                </div>
            </div>
        };

        let body = if is_own_draft() {
            view! {
                <>
                    <DraftForm draft=draft errors=errors />
                    <FinalizePanel signature=signature accuracy_confirmed=accuracy_confirmed errors=errors />
                    <div class="flex flex-wrap gap-2">
                        <button type="button" prop:disabled=move || busy.get() on:click=save_draft class="rounded-full border border-slate-700 px-3 py-2 text-sm font-semibold text-slate-200 hover:bg-slate-800 disabled:opacity-50">"Save draft"</button>
                        <button type="button" prop:disabled=move || busy.get() on:click=finalize_draft class="rounded-full bg-primary-500 px-3 py-2 text-sm font-semibold text-white hover:bg-primary-600 disabled:opacity-50">"Finalize"</button>
                        <button type="button" prop:disabled=move || busy.get() on:click=discard_draft class="rounded-full border border-rose-500/40 px-3 py-2 text-sm font-semibold text-rose-300 hover:bg-rose-500/10 disabled:opacity-50">"Discard draft"</button>
                    </div>
                </>
            }
            .into_any()
        } else if is_redacted_admin_draft() {
            view! {
                <div class="rounded-xl border border-amber-500/30 bg-amber-500/10 p-4 text-sm text-amber-100">
                    <p>"This is another user's draft. Content remains hidden until you explicitly inspect it, which writes an audit event."</p>
                    <button type="button" prop:disabled=move || busy.get() on:click=inspect_draft class="mt-3 rounded-full bg-amber-500/20 px-3 py-2 text-sm font-semibold text-amber-100 hover:bg-amber-500/30 disabled:opacity-50">"Inspect draft and record audit"</button>
                </div>
            }
            .into_any()
        } else if is_admin_visible_draft() {
            view! {
                <div class="space-y-4">
                    <p class="rounded-lg border border-slate-700 bg-slate-900 px-3 py-2 text-sm text-slate-300">"Read-only administrative inspection; the author remains the only editor."</p>
                    <StructuredNoteReadOnly draft=detail.draft.clone() total_minutes=detail.total_minutes />
                </div>
            }
            .into_any()
        } else if detail.state == CaseNoteState::Discarded {
            view! {
                <section class=PANEL_CLASS>
                    <h2 class="text-sm font-semibold text-slate-200">"Discarded draft tombstone"</h2>
                    <p class="mt-2 text-sm text-slate-500">"Draft content was removed. Its identity, author, lifecycle time, and audit history are preserved."</p>
                </section>
            }
            .into_any()
        } else if detail.state == CaseNoteState::Legacy {
            view! {
                <div class="space-y-4">
                    <section class=PANEL_CLASS>
                        <h2 class="text-sm font-semibold text-slate-200">"Legacy shared note"</h2>
                        <p class="mt-3 whitespace-pre-wrap text-sm text-slate-200">{value_or_placeholder(detail.legacy_body.clone())}</p>
                    </section>
                    <FiledRecordPanel note_id=detail.id.clone() case_id=detail.case_id.clone() can_file=can_add_to_case.get() />
                    <section class=PANEL_CLASS>
                        <h2 class="text-sm font-semibold text-slate-200">"Addenda"</h2>
                        <div class="mt-3 space-y-3"><AddendaList note=detail.clone() /></div>
                    </section>
                    <Show when=move || can_add_to_case.get()>
                        <AddendumForm note_id=detail.id.clone() input=addendum_input errors=addendum_errors busy=addendum_busy message=addendum_message on_added=on_added />
                    </Show>
                </div>
            }
            .into_any()
        } else {
            view! {
                <div class="space-y-4">
                    <StructuredNoteReadOnly draft=detail.draft.clone() total_minutes=detail.total_minutes />
                    <FiledRecordPanel note_id=detail.id.clone() case_id=detail.case_id.clone() can_file=can_add_to_case.get() />
                    <section class=PANEL_CLASS>
                        <h2 class="text-sm font-semibold text-slate-200">"Addenda"</h2>
                        <div class="mt-3 space-y-3"><AddendaList note=detail.clone() /></div>
                    </section>
                    <Show when=move || can_add_to_case.get()>
                        <AddendumForm note_id=detail.id.clone() input=addendum_input errors=addendum_errors busy=addendum_busy message=addendum_message on_added=on_added />
                    </Show>
                </div>
            }
            .into_any()
        };

        view! {
            <div class="space-y-5">
                {header}
                {body}
                <Show when=move || message.get().is_some()>
                    <p class="rounded-lg border border-slate-700 bg-slate-900 px-3 py-2 text-sm text-slate-200">{move || message.get().unwrap_or_default()}</p>
                </Show>
            </div>
        }
        .into_any()
    };

    require_login(state, move || {
        view! {
            <Layout title="Case note".to_string()>
                <div class="mx-auto max-w-6xl space-y-5">
                    <SafetyWarning />
                    <p class="sr-only">"Case " {case_id.get_value()} " note " {note_id.get_value()}</p>
                    {content}
                </div>
            </Layout>
        }
        .into_any()
    })
}
