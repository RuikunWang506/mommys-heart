use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::components::case_overview::month_day;
use crate::server_fns::case_plan::{
    add_plan_action, list_case_plan, list_plan_assignees, log_task_attempt, record_task_outcome,
    AttemptResult, CasePlanAction, CaseTask, PlanAssignee, TaskOutcome,
};
use crate::server_fns::err_text;
use crate::state::{now_stamp, AppState};

const CARD: &str =
    "rounded-card border border-slate-800 bg-slate-900 px-6 pt-[22px] pb-4 shadow-card";
const PILL: &str =
    "inline-flex items-center gap-1.5 rounded-full px-[11px] py-[3px] text-[13px] font-semibold leading-5 whitespace-nowrap";
const FORM: &str = "grid gap-x-4 gap-y-3 rounded-[14px] bg-[#fdfaf5] px-4 py-3.5 sm:grid-cols-2";
const FIELD: &str = "flex min-w-0 flex-col gap-1.5";
const FIELD_LABEL: &str = "text-[13.5px] font-bold text-[#4a4038]";
const FIELD_INPUT: &str = "min-h-[42px] w-full rounded-xl border border-[#d9ccb8] bg-slate-900 px-3.5 py-2 text-[14.5px] text-slate-100 focus:border-primary-500 focus:outline-none";
const HINT: &str = "text-[12.5px] text-slate-500";
const PRIMARY: &str = "inline-flex min-h-[34px] items-center rounded-full bg-primary-600 px-3.5 text-[13.5px] font-semibold text-white hover:bg-primary-700";
const SECONDARY: &str = "inline-flex min-h-[34px] items-center gap-1.5 rounded-full border border-slate-700 bg-slate-900 px-3.5 text-[13.5px] font-semibold text-slate-200 hover:bg-slate-950";
const CHIP: &str = "inline-flex min-h-9 cursor-pointer items-center rounded-full border px-3.5 text-[13.5px] font-semibold has-[:focus-visible]:outline has-[:focus-visible]:outline-2 has-[:focus-visible]:outline-[#1a66a8]";
const CHIP_ON: &str = "border-[#e9a866] bg-[#fdf0e1] text-[#7c2d12]";
const CHIP_OFF: &str = "border-[#d9ccb8] bg-slate-900 text-[#4a4038]";

#[derive(Clone, PartialEq)]
struct PlanRow {
    title: String,
    added: String,
    task: CaseTask,
}

#[component]
pub fn CasePlanCard(case_id: String, can_edit: bool, can_note: bool) -> impl IntoView {
    let state = expect_context::<AppState>();
    let viewer_id = StoredValue::new(
        state
            .current_user_summary
            .with_untracked(|user| user.as_ref().map(|user| user.id.clone()))
            .unwrap_or_default(),
    );
    let case_sv = StoredValue::new(case_id);
    let plan = RwSignal::new(Vec::<CasePlanAction>::new());
    let assignees = RwSignal::new(Vec::<PlanAssignee>::new());
    let adding = RwSignal::new(false);
    let saving_action = RwSignal::new(false);
    let loading = RwSignal::new(true);
    let load_error = RwSignal::new(String::new());
    let error = RwSignal::new(String::new());
    let today = StoredValue::new(now_stamp()[..10].to_string());

    // Reloads also run after a save, when the card may already be gone, so
    // nothing here assumes it is still mounted.
    let reload = move || {
        let Some(case_id) = case_sv.try_get_value() else {
            return;
        };
        spawn_local(async move {
            let found = list_case_plan(case_id).await;
            let _ = loading.try_set(false);
            match found {
                Ok(found) => {
                    let _ = plan.try_set(found);
                    let _ = load_error.try_set(String::new());
                }
                Err(e) => {
                    let _ = load_error.try_set(err_text(e));
                }
            }
        })
    };
    reload();
    if can_edit {
        let case_id = case_sv.get_value();
        spawn_local(async move {
            match list_plan_assignees(case_id).await {
                Ok(found) => {
                    let _ = assignees.try_set(found);
                }
                Err(e) => {
                    let _ = error.try_set(err_text(e));
                }
            }
        });
    }

    let rows = Memo::new(move |_| {
        let mut open = Vec::new();
        let mut closed = Vec::new();
        for action in plan.get() {
            let added = format!(
                "Added by {}, {}",
                action.added_by_name,
                month_day(&action.added_on)
            );
            for task in action.tasks {
                let row = PlanRow {
                    title: action.title.clone(),
                    added: added.clone(),
                    task,
                };
                if row.task.outcome.is_some() {
                    closed.push(row);
                } else {
                    open.push(row);
                }
            }
        }
        let today = today.get_value();
        open.sort_by_key(|row| {
            let due = &row.task.follow_up_on;
            let overdue = !due.is_empty() && *due < today;
            (!overdue, due.is_empty(), due.clone())
        });
        (open, closed)
    });

    let title = RwSignal::new(String::new());
    let assignee = RwSignal::new(String::new());
    let follow_up = RwSignal::new(String::new());
    let save_action = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        if saving_action.get_untracked() {
            return;
        }
        saving_action.set(true);
        let request = add_plan_action(
            case_sv.get_value(),
            title.get_untracked(),
            assignee.get_untracked(),
            follow_up.get_untracked(),
        );
        spawn_local(async move {
            let result = request.await;
            let _ = saving_action.try_set(false);
            match result {
                Ok(()) => {
                    let _ = title.try_set(String::new());
                    let _ = follow_up.try_set(String::new());
                    let _ = error.try_set(String::new());
                    let _ = adding.try_set(false);
                    reload();
                }
                Err(e) => {
                    let _ = error.try_set(err_text(e));
                }
            }
        });
    };

    let add_form = move || {
        adding.get().then(|| {
            view! {
                <form on:submit=save_action aria-label="Add an action" class=format!("mt-4 {FORM}")>
                    <div class=format!("{FIELD} sm:col-span-2")>
                        <label for="plan-title" class=FIELD_LABEL>"Action"</label>
                        <input id="plan-title" class=FIELD_INPUT required bind:value=title placeholder="What needs to happen" />
                    </div>
                    <div class=FIELD>
                        <label for="plan-assignee" class=FIELD_LABEL>"Assigned to"</label>
                        <select id="plan-assignee" class=FIELD_INPUT required bind:value=assignee>
                            <option value="">"Choose someone on this case"</option>
                            {move || {
                                assignees
                                    .get()
                                    .into_iter()
                                    .map(|a| view! { <option value=a.id>{a.name}</option> })
                                    .collect::<Vec<_>>()
                            }}
                        </select>
                    </div>
                    <div class=FIELD>
                        <label for="plan-follow-up" class=FIELD_LABEL>"Due date"</label>
                        <input id="plan-follow-up" type="date" min=today.get_value() class=FIELD_INPUT bind:value=follow_up />
                    </div>
                    <div class="flex gap-2 sm:col-span-2">
                        <button type="submit" class=format!("{PRIMARY} disabled:opacity-60") prop:disabled=move || saving_action.get()>
                            {move || if saving_action.get() { "Saving\u{2026}" } else { "Save action" }}
                        </button>
                        <button type="button" class=SECONDARY on:click=move |_| adding.set(false)>"Cancel"</button>
                    </div>
                </form>
            }
        })
    };

    // One callback for every row, so a save that finishes after another
    // save rebuilt the list still reloads it.
    let on_saved = Callback::new(move |_| reload());
    let item = move |row: PlanRow| {
        let can_update =
            can_edit || (can_note && viewer_id.with_value(|v| *v == row.task.assignee_id));
        view! {
            <TaskItem
                row=row
                today=today.get_value()
                can_update=can_update
                on_saved=on_saved
            />
        }
    };

    let open_list = move || {
        let (open, closed) = rows.get();
        if open.is_empty() && closed.is_empty() {
            let message = if loading.get() {
                "Loading\u{2026}".to_string()
            } else if !load_error.get().is_empty() {
                load_error.get()
            } else {
                "No approved actions yet.".to_string()
            };
            return view! { <p class="py-4 text-sm text-slate-500">{message}</p> }.into_any();
        }
        view! {
            <ul class="mt-4 mb-3 flex flex-col gap-3">{open.into_iter().map(item).collect::<Vec<_>>()}</ul>
        }
        .into_any()
    };

    let closed_list = move || {
        let closed = rows.get().1;
        (!closed.is_empty()).then(|| {
            let count = closed.len();
            view! {
                <details class="group border-t border-slate-800 pt-3">
                    <summary class="flex min-h-10 cursor-pointer list-none items-center gap-2 font-bold text-[#4a4038] [&::-webkit-details-marker]:hidden">
                        <svg class="h-4 w-4 transition-transform group-open:rotate-90" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" aria-hidden="true"><path d="M9 6l6 6-6 6"></path></svg>
                        {format!("Closed ({count})")}
                    </summary>
                    <ul class="mt-2 mb-2 flex flex-col gap-3">{closed.into_iter().map(item).collect::<Vec<_>>()}</ul>
                </details>
            }
        })
    };

    let summary = move || {
        let open = rows.with(|(open, _)| open.len());
        match open {
            0 => "Nothing open".to_string(),
            1 => "1 open".to_string(),
            n => format!("{n} open \u{b7} overdue first"),
        }
    };

    view! {
        <section class=CARD aria-labelledby="case-plan-heading">
            <div class="-mx-6 flex flex-wrap items-start justify-between gap-3 border-b border-slate-800 px-6 pb-3.5">
                <div class="min-w-0 flex-[1_1_260px]">
                    <p class="flex flex-wrap items-center gap-2">
                        <span class=format!("{PILL} bg-[#e3f5ec] text-[#11633f]")>"Approved by staff"</span>
                        <span class=format!("{PILL} border border-[#fcd34d] bg-[#fffdf5] text-[#7a4a06]")>
                            <svg class="h-[13px] w-[13px]" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" aria-hidden="true"><rect x="5" y="11" width="14" height="10" rx="2"></rect><path d="M8 11V7a4 4 0 0 1 8 0v4"></path></svg>
                            "Volunteers only"
                        </span>
                    </p>
                    <h2 id="case-plan-heading" class="mt-2.5 text-2xl font-semibold">"Approved case plan"</h2>
                    <p class="mt-0.5 text-[13.5px] text-slate-500">{summary}</p>
                </div>
                {can_edit
                    .then(|| {
                        view! {
                            <button type="button" class=format!("shrink-0 {SECONDARY}") on:click=move |_| adding.set(true)>
                                <svg class="h-4 w-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" aria-hidden="true"><path d="M12 5v14M5 12h14"></path></svg>
                                "Add action"
                            </button>
                        }
                    })}
            </div>
            {add_form}
            <Show when=move || !error.get().is_empty()>
                <p role="alert" class="mt-3 text-sm text-rose-300">{move || error.get()}</p>
            </Show>
            <Show when=move || {
                !load_error.get().is_empty() && rows.with(|(open, closed)| !open.is_empty() || !closed.is_empty())
            }>
                <p role="alert" class="mt-3 text-sm text-rose-300">{move || load_error.get()}</p>
            </Show>
            {open_list}
            {closed_list}
        </section>
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Panel {
    Closed,
    Attempt,
    Outcome,
}

#[component]
fn TaskItem(
    row: PlanRow,
    today: String,
    can_update: bool,
    on_saved: Callback<()>,
) -> impl IntoView {
    let PlanRow { title, added, task } = row;
    let panel = RwSignal::new(Panel::Closed);
    let busy = RwSignal::new(false);
    let error = RwSignal::new(String::new());
    let attempt_result = RwSignal::new(AttemptResult::CouldNotReach);
    let next_try = RwSignal::new(String::new());
    let attempt_note = RwSignal::new(String::new());
    let outcome = RwSignal::new(TaskOutcome::Completed);
    let outcome_notes = RwSignal::new(String::new());
    let task_id = StoredValue::new(task.id.clone());
    let ids = StoredValue::new(format!("task-{}", task.id));
    let title_sv = StoredValue::new(title.clone());
    let today_sv = StoredValue::new(today.clone());

    let open_panel = move |which: Panel| {
        error.set(String::new());
        panel.set(which);
    };
    // A successful save reloads the list, which replaces this row, so only
    // the failure path touches this row's signals afterwards.
    let finish = move |result: Result<(), ServerFnError>| {
        let _ = busy.try_set(false);
        match result {
            Ok(()) => {
                let _ = on_saved.try_run(());
            }
            Err(e) => {
                let _ = error.try_set(err_text(e));
            }
        }
    };
    let save_attempt = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        let request = log_task_attempt(
            task_id.get_value(),
            attempt_result.get_untracked(),
            attempt_note.get_untracked(),
            next_try.get_untracked(),
        );
        spawn_local(async move { finish(request.await) });
    };
    let save_outcome = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        let request = record_task_outcome(
            task_id.get_value(),
            outcome.get_untracked(),
            outcome_notes.get_untracked(),
        );
        spawn_local(async move { finish(request.await) });
    };

    let due = task.follow_up_on.clone();
    let overdue = task.outcome.is_none() && !due.is_empty() && due < today;
    let pill = match task.outcome {
        None if overdue => view! {
            <span class=format!("{PILL} bg-[#fbe4e1] text-[#9b2c22]")>
                <svg class="h-3 w-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 3l9.5 17h-19z"></path><path d="M12 10v4M12 17.5v.01"></path></svg>
                {format!("Overdue since {}", month_day(&due))}
            </span>
        }
        .into_any(),
        None => view! {
            <span class=format!("{PILL} bg-[#fef3c7] text-[#7a4a06]")>
                <svg class="h-3 w-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.6" aria-hidden="true"><circle cx="12" cy="12" r="9"></circle><path d="M12 7v5l3 2"></path></svg>
                "Active"
            </span>
        }
        .into_any(),
        Some(TaskOutcome::Completed) => view! {
            <span class=format!("{PILL} bg-[#e3f5ec] text-[#11633f]")>
                <svg class="h-3 w-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3" aria-hidden="true"><path d="M5 12l5 5L20 7"></path></svg>
                {format!("Completed {}", month_day(&task.outcome_recorded_on))}
            </span>
        }
        .into_any(),
        Some(other) => view! {
            <span class=format!("{PILL} bg-[#f3ece1] text-[#5b4f43]")>{other.label()}</span>
        }
        .into_any(),
    };

    let when = match (&task.outcome, due.is_empty()) {
        (Some(_), _) | (None, true) => String::new(),
        (None, false) if overdue => format!(" \u{b7} Was due {}", month_day(&due)),
        (None, false) if task.last_attempt.is_some() => {
            format!(" \u{b7} Next try {}, {}", weekday(&due), month_day(&due))
        }
        (None, false) => format!(" \u{b7} Due {}", month_day(&due)),
    };

    let with_note = |text: &str| {
        if text.is_empty() {
            String::new()
        } else {
            format!(" {text}")
        }
    };
    let note_box = match (task.outcome, &task.last_attempt) {
        (Some(outcome), _) => Some((
            format!("Outcome, {}:", month_day(&task.outcome_recorded_on)),
            format!("{}.{}", outcome.label(), with_note(&task.outcome_notes)),
        )),
        (None, Some(attempt)) => Some((
            format!("Last attempt, {}:", month_day(&attempt.recorded_on)),
            format!("{}.{}", attempt.result.label(), with_note(&attempt.note)),
        )),
        (None, None) => None,
    }
    .map(|(heading, body)| {
        view! {
            <p class="col-span-2 mt-2.5 rounded-xl border-l-[3px] border-[#e9a866] bg-[#fdfaf5] px-3 py-2.5 text-[13.5px]">
                <strong>{heading}</strong>
                " "
                {body}
            </p>
        }
    });

    let attempt_form = move || {
        view! {
            <form on:submit=save_attempt aria-label=format!("Log an attempt for {}", title_sv.get_value()) class=format!("col-span-2 mt-3 {FORM}")>
                <fieldset class="sm:col-span-2">
                    <legend class=FIELD_LABEL>"What happened"</legend>
                    <div class="mt-1.5 flex flex-wrap gap-2">
                        {AttemptResult::ALL
                            .iter()
                            .map(|&choice| {
                                view! {
                                    <label class=move || format!("{CHIP} {}", if attempt_result.get() == choice { CHIP_ON } else { CHIP_OFF })>
                                        <input
                                            type="radio"
                                            class="sr-only"
                                            name=format!("{}-result", ids.get_value())
                                            prop:checked=move || attempt_result.get() == choice
                                            on:change=move |_| attempt_result.set(choice)
                                        />
                                        {choice.label()}
                                    </label>
                                }
                            })
                            .collect::<Vec<_>>()}
                    </div>
                </fieldset>
                <div class=FIELD>
                    <label class=FIELD_LABEL for=format!("{}-next", ids.get_value())>
                        "Next try " <span class="text-[#9b2c22]">"*"</span>
                    </label>
                    <input id=format!("{}-next", ids.get_value()) type="date" required min=today_sv.get_value() class=FIELD_INPUT bind:value=next_try />
                </div>
                <div class=FIELD>
                    <label class=FIELD_LABEL for=format!("{}-note", ids.get_value())>"Note (optional)"</label>
                    <input id=format!("{}-note", ids.get_value()) class=FIELD_INPUT bind:value=attempt_note />
                </div>
                <p class=format!("{HINT} sm:col-span-2")>
                    "The task stays open. Use \u{201c}Record final outcome\u{201d} when it's done, declined or no longer needed."
                </p>
                <div class="flex gap-2 sm:col-span-2">
                    <button type="submit" class=format!("{PRIMARY} disabled:opacity-60") prop:disabled=move || busy.get()>"Save attempt"</button>
                    <button type="button" class=SECONDARY on:click=move |_| panel.set(Panel::Closed)>"Cancel"</button>
                </div>
            </form>
        }
    };

    let outcome_form = move || {
        view! {
            <form on:submit=save_outcome aria-label=format!("Record final outcome for {}", title_sv.get_value()) class=format!("col-span-2 mt-3 {FORM}")>
                <fieldset class="sm:col-span-2">
                    <legend class=FIELD_LABEL>"Final outcome"</legend>
                    <div class="mt-1.5 flex flex-wrap gap-2">
                        {TaskOutcome::ALL
                            .iter()
                            .map(|&choice| {
                                view! {
                                    <label class=move || format!("{CHIP} {}", if outcome.get() == choice { CHIP_ON } else { CHIP_OFF })>
                                        <input
                                            type="radio"
                                            class="sr-only"
                                            name=format!("{}-outcome", ids.get_value())
                                            prop:checked=move || outcome.get() == choice
                                            on:change=move |_| outcome.set(choice)
                                        />
                                        {choice.label()}
                                    </label>
                                }
                            })
                            .collect::<Vec<_>>()}
                    </div>
                </fieldset>
                <div class=format!("{FIELD} sm:col-span-2")>
                    <label class=FIELD_LABEL for=format!("{}-outcome-notes", ids.get_value())>"Outcome notes"</label>
                    <textarea id=format!("{}-outcome-notes", ids.get_value()) rows="2" class=FIELD_INPUT bind:value=outcome_notes></textarea>
                </div>
                <p class=format!("{HINT} sm:col-span-2")>"This closes the task and moves it to Closed."</p>
                <div class="flex gap-2 sm:col-span-2">
                    <button type="submit" class=format!("{PRIMARY} disabled:opacity-60") prop:disabled=move || busy.get()>"Save outcome"</button>
                    <button type="button" class=SECONDARY on:click=move |_| panel.set(Panel::Closed)>"Cancel"</button>
                </div>
            </form>
        }
    };

    let actions = (task.outcome.is_none() && can_update).then(|| {
        view! {
            <Show when=move || panel.get() == Panel::Closed>
                <div class="col-span-2 mt-2 flex flex-wrap gap-2">
                    <button type="button" class=SECONDARY on:click=move |_| open_panel(Panel::Attempt)>"Log an attempt"</button>
                    <button type="button" class=SECONDARY on:click=move |_| open_panel(Panel::Outcome)>"Record final outcome"</button>
                </div>
            </Show>
            <Show when=move || panel.get() == Panel::Attempt>{attempt_form}</Show>
            <Show when=move || panel.get() == Panel::Outcome>{outcome_form}</Show>
            <Show when=move || !error.get().is_empty()>
                <p role="alert" class="col-span-2 mt-2 text-sm text-rose-300">{move || error.get()}</p>
            </Show>
        }
    });

    view! {
        <li class="grid grid-cols-[minmax(0,1fr)_auto] items-start gap-x-3.5 gap-y-0.5 rounded-2xl border border-slate-800 px-[18px] py-4">
            <p class="text-base font-bold leading-snug">{title}</p>
            <span class="self-start">{pill}</span>
            <p class="col-span-2 mt-1.5 text-[14.5px] font-semibold text-[#4a4038]">
                {format!("{}{when}", task.assignee_name)}
            </p>
            <p class=format!("col-span-2 {HINT}")>{added}</p>
            {note_box}
            {actions}
        </li>
    }
}

fn weekday(date: &str) -> &'static str {
    const NAMES: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    const OFFSETS: [i32; 12] = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    let part = |range: std::ops::Range<usize>| date.get(range).and_then(|p| p.parse::<i32>().ok());
    let (Some(mut year), Some(month @ 1..=12), Some(day)) = (part(0..4), part(5..7), part(8..10))
    else {
        return "";
    };
    if month < 3 {
        year -= 1;
    }
    let index = (year + year / 4 - year / 100 + year / 400 + OFFSETS[month as usize - 1] + day) % 7;
    NAMES[index as usize]
}
