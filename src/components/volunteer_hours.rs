//! Private volunteer-hour ledger embedded in eligible user profiles.

use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::server_fns::err_text;
use crate::server_fns::volunteer_hours::{
    add_my_volunteer_hours, delete_my_volunteer_hours, update_my_volunteer_hours, VolunteerHour,
    VolunteerHourInput, VolunteerHours,
};
use crate::state::today;

const INPUT_CLASS: &str = "w-full rounded-xl border border-slate-700 bg-slate-900 px-3 py-2 text-sm text-slate-100 placeholder:text-slate-500 focus:border-primary-500 focus:outline-none focus:ring-2 focus:ring-primary-500/40";
const LABEL_CLASS: &str = "text-xs font-medium text-slate-400";
const SECTION_CLASS: &str = "rounded-card border border-slate-800 bg-slate-900 shadow-card p-4";

#[derive(Clone, Default)]
struct HourDraft {
    service_date: String,
    hours: String,
    description: String,
}

impl HourDraft {
    fn new() -> Self {
        Self {
            service_date: today(),
            hours: String::new(),
            description: String::new(),
        }
    }

    fn from_entry(entry: &VolunteerHour) -> Self {
        Self {
            service_date: entry.service_date.clone(),
            hours: hours_input_value(entry.duration_minutes),
            description: entry.description.clone(),
        }
    }

    fn to_input(&self) -> Result<VolunteerHourInput, String> {
        let hours = self
            .hours
            .trim()
            .parse::<f64>()
            .map_err(|_| "Enter the number of hours worked.".to_string())?;
        if !hours.is_finite() || hours <= 0.0 || hours > 24.0 {
            return Err("Hours must be greater than zero and no more than 24.".to_string());
        }
        let duration_minutes = (hours * 60.0).round() as i32;
        Ok(VolunteerHourInput {
            service_date: self.service_date.clone(),
            duration_minutes,
            description: self.description.clone(),
        })
    }
}

fn hours_input_value(minutes: i32) -> String {
    let value = minutes as f64 / 60.0;
    if minutes % 60 == 0 {
        format!("{value:.0}")
    } else {
        format!("{value:.2}")
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string()
    }
}

fn duration_text(minutes: i64) -> String {
    let hours = minutes / 60;
    let remaining = minutes % 60;
    match (hours, remaining) {
        (0, minutes) => format!("{minutes}m"),
        (hours, 0) => format!("{hours}h"),
        (hours, minutes) => format!("{hours}h {minutes}m"),
    }
}

#[component]
pub fn VolunteerHoursPanel(
    initial_hours: VolunteerHours,
    is_self: bool,
    #[prop(into)] display_name: String,
) -> impl IntoView {
    let hours = RwSignal::new(initial_hours);
    let action_error = RwSignal::new(None::<String>);
    let saving = RwSignal::new(false);
    let form_open = RwSignal::new(false);
    let editing_id = RwSignal::new(None::<String>);
    let pending_delete = RwSignal::new(None::<String>);
    let draft = RwSignal::new(HourDraft::default());

    let open_add = move |_| {
        editing_id.set(None);
        pending_delete.set(None);
        action_error.set(None);
        draft.set(HourDraft::new());
        form_open.set(true);
    };

    let cancel_form = move |_| {
        form_open.set(false);
        editing_id.set(None);
        action_error.set(None);
    };

    let save = move |_| {
        if saving.get_untracked() {
            return;
        }
        let input = match draft.get_untracked().to_input() {
            Ok(input) => input,
            Err(error) => {
                action_error.set(Some(error));
                return;
            }
        };
        let entry_id = editing_id.get_untracked();
        saving.set(true);
        action_error.set(None);
        spawn_local(async move {
            let result = match entry_id {
                Some(entry_id) => update_my_volunteer_hours(entry_id, input).await,
                None => add_my_volunteer_hours(input).await,
            };
            match result {
                Ok(data) => {
                    hours.set(data);
                    form_open.set(false);
                    editing_id.set(None);
                }
                Err(error) => action_error.set(Some(err_text(error))),
            }
            saving.set(false);
        });
    };

    let body = move || {
        let data = hours.get();
        if data.entries.is_empty() {
            return view! {
                <div class="rounded-lg border border-dashed border-slate-700 px-4 py-6 text-center">
                    <p class="text-sm text-slate-400">"No volunteer hours have been logged yet."</p>
                </div>
            }
            .into_any();
        }

        data.entries
            .into_iter()
            .map(|entry| {
                let edit_entry = entry.clone();
                let delete_id = entry.id.clone();
                let confirm_id = entry.id.clone();
                let confirmed_delete_id = StoredValue::new(entry.id.clone());
                let is_confirming = move || pending_delete.get().as_deref() == Some(confirm_id.as_str());
                let controls = if is_self {
                    view! {
                        <div class="flex shrink-0 items-center gap-2">
                            <Show
                                when=is_confirming
                                fallback=move || {
                                    let edit_entry = edit_entry.clone();
                                    let delete_id = delete_id.clone();
                                    view! {
                                        <button
                                            type="button"
                                            on:click=move |_| {
                                                editing_id.set(Some(edit_entry.id.clone()));
                                                pending_delete.set(None);
                                                action_error.set(None);
                                                draft.set(HourDraft::from_entry(&edit_entry));
                                                form_open.set(true);
                                            }
                                            class="rounded-md border border-slate-700 px-2 py-1 text-xs font-medium text-slate-300 hover:bg-slate-800"
                                        >
                                            "Edit"
                                        </button>
                                        <button
                                            type="button"
                                            on:click=move |_| pending_delete.set(Some(delete_id.clone()))
                                            class="rounded-md border border-rose-500/30 px-2 py-1 text-xs font-medium text-rose-300 hover:bg-rose-500/10"
                                        >
                                            "Delete"
                                        </button>
                                    }
                                }
                            >
                                <button
                                    type="button"
                                    on:click=move |_| pending_delete.set(None)
                                    class="rounded-md border border-slate-700 px-2 py-1 text-xs font-medium text-slate-300 hover:bg-slate-800"
                                >
                                    "Cancel"
                                </button>
                                <button
                                    type="button"
                                    prop:disabled=move || saving.get()
                                    on:click=move |_| {
                                        if saving.get_untracked() {
                                            return;
                                        }
                                        let entry_id = confirmed_delete_id.get_value();
                                        saving.set(true);
                                        action_error.set(None);
                                        spawn_local(async move {
                                            match delete_my_volunteer_hours(entry_id).await {
                                                Ok(data) => {
                                                    hours.set(data);
                                                    pending_delete.set(None);
                                                }
                                                Err(error) => action_error.set(Some(err_text(error))),
                                            }
                                            saving.set(false);
                                        });
                                    }
                                    class="rounded-md bg-rose-500/15 px-2 py-1 text-xs font-semibold text-rose-300 hover:bg-rose-500/25 disabled:opacity-60"
                                >
                                    "Confirm"
                                </button>
                            </Show>
                        </div>
                    }
                    .into_any()
                } else {
                    ().into_any()
                };
                let description = if entry.description.is_empty() {
                    ().into_any()
                } else {
                    view! { <p class="mt-1 text-sm text-slate-400">{entry.description}</p> }.into_any()
                };
                view! {
                    <li class="flex items-start justify-between gap-4 border-b border-slate-800 py-3 last:border-b-0">
                        <div class="min-w-0">
                            <div class="flex flex-wrap items-baseline gap-x-3 gap-y-1">
                                <span class="text-sm font-semibold text-slate-200">
                                    {duration_text(entry.duration_minutes as i64)}
                                </span>
                                <time class="text-xs text-slate-500">{entry.service_date}</time>
                            </div>
                            {description}
                        </div>
                        {controls}
                    </li>
                }
            })
            .collect_view()
            .into_any()
    };

    view! {
        <section class=SECTION_CLASS>
            <div class="flex flex-wrap items-start justify-between gap-3">
                <div>
                    <h3 class="text-sm font-semibold text-slate-200">"Volunteer hours"</h3>
                    <p class="mt-1 text-xs text-slate-500">
                        {if is_self {
                            "Only you and users with operations-admin permissions can see these hours.".to_string()
                        } else {
                            format!("Visible to you because you have operations-admin permissions and are viewing {display_name}.")
                        }}
                    </p>
                </div>
                <div class="flex items-center gap-3">
                    <div class="text-right">
                        <p class="text-xs text-slate-500">"All-time total"</p>
                        <p class="text-lg font-semibold text-primary-300">
                            {move || duration_text(hours.get().total_minutes)}
                        </p>
                    </div>
                    <Show when=move || is_self && !form_open.get()>
                        <button
                            type="button"
                            on:click=open_add
                            class="rounded-full bg-primary-500 px-3 py-1.5 text-sm font-semibold text-white hover:bg-primary-600"
                        >
                            "Log hours"
                        </button>
                    </Show>
                </div>
            </div>

            <Show when=move || form_open.get()>
                <div class="mt-4 rounded-lg border border-slate-700 bg-slate-950/50 p-3">
                    <div class="flex items-center justify-between gap-3">
                        <h4 class="text-sm font-semibold text-slate-200">
                            {move || if editing_id.get().is_some() { "Edit hours" } else { "Log hours" }}
                        </h4>
                        <button
                            type="button"
                            on:click=cancel_form
                            class="rounded-md border border-slate-700 px-2 py-1 text-xs font-medium text-slate-300 hover:bg-slate-800"
                        >
                            "Cancel"
                        </button>
                    </div>
                    <div class="mt-3 grid gap-3 sm:grid-cols-2">
                        <label class=LABEL_CLASS>
                            "Date"
                            <input
                                type="date"
                                class=INPUT_CLASS
                                prop:value=move || draft.get().service_date
                                on:input=move |event| {
                                    draft.update(|value| value.service_date = event_target_value(&event));
                                }
                            />
                        </label>
                        <label class=LABEL_CLASS>
                            "Hours"
                            <input
                                type="number"
                                min="0.25"
                                max="24"
                                step="0.25"
                                placeholder="2.5"
                                class=INPUT_CLASS
                                prop:value=move || draft.get().hours
                                on:input=move |event| {
                                    draft.update(|value| value.hours = event_target_value(&event));
                                }
                            />
                        </label>
                    </div>
                    <label class="mt-3 block text-xs font-medium text-slate-400">
                        "Description (optional)"
                        <textarea
                            rows="2"
                            maxlength="500"
                            placeholder="What did you work on?"
                            class=INPUT_CLASS
                            prop:value=move || draft.get().description
                            on:input=move |event| {
                                draft.update(|value| value.description = event_target_value(&event));
                            }
                        ></textarea>
                    </label>
                    <Show when=move || action_error.get().is_some()>
                        <p class="mt-3 text-sm text-rose-300">{move || action_error.get().unwrap_or_default()}</p>
                    </Show>
                    <button
                        type="button"
                        on:click=save
                        prop:disabled=move || saving.get()
                        class="mt-3 rounded-full bg-primary-500 px-3 py-1.5 text-sm font-semibold text-white hover:bg-primary-600 disabled:opacity-60"
                    >
                        {move || if saving.get() { "Saving..." } else { "Save hours" }}
                    </button>
                </div>
            </Show>

            <Show when=move || action_error.get().is_some() && !form_open.get()>
                <p class="mt-3 text-sm text-rose-300">{move || action_error.get().unwrap_or_default()}</p>
            </Show>
            <ul class="mt-3">{body}</ul>
        </section>
    }
}
