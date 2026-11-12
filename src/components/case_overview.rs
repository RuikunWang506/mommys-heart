use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::components::A;

use crate::components::case_contacts::CaseContactsPanel;
use crate::components::case_plan::CasePlanCard;
use crate::components::case_questionnaires::intake_status;
use crate::helpers::format::initials;
use crate::helpers::visibility::Visibility;
use crate::server_fns::case_contacts::{list_case_contacts, CaseContact, CaseContactRole};
use crate::server_fns::case_notes::{
    list_case_notes, CaseNoteListFilters, CaseNoteListItem, CaseNoteState,
};
use crate::server_fns::case_properties::CaseProperty;
use crate::server_fns::err_text;
use crate::state::AppState;

const CARD: &str =
    "rounded-card border border-slate-800 bg-slate-900 px-[22px] pt-5 pb-[22px] shadow-card";
const CARD_HEAD: &str =
    "-mx-[22px] flex items-center justify-between gap-2 border-b border-slate-800 px-[22px] pb-3.5";
const SMALL_BUTTON: &str = "rounded-full border border-slate-700 bg-slate-900 px-3 py-1 text-sm font-semibold text-slate-200 hover:bg-slate-950";
const LINK: &str = "text-sm font-semibold text-primary-300 underline-offset-2 hover:underline";
const RECENT_LIMIT: usize = 4;

#[component]
pub fn CaseOverview(
    case_id: String,
    properties: Signal<Vec<CaseProperty>>,
    client_first_name: String,
    show_intake: bool,
    show_case_info: bool,
    show_people: bool,
    can_edit_people: bool,
    show_plan: bool,
    can_edit_plan: bool,
    can_note_plan: bool,
    on_open_intake: Callback<()>,
    on_open_notes: Callback<()>,
) -> impl IntoView {
    let state = expect_context::<AppState>();
    let viewer_id = StoredValue::new(
        state
            .current_user_summary
            .with_untracked(|user| user.as_ref().map(|user| user.id.clone()))
            .unwrap_or_default(),
    );
    let case_sv = StoredValue::new(case_id.clone());
    let notes = RwSignal::new(Vec::<CaseNoteListItem>::new());
    let drafts = RwSignal::new(Vec::<CaseNoteListItem>::new());
    let notes_loading = RwSignal::new(true);
    let notes_error = RwSignal::new(String::new());
    let contacts = RwSignal::new(Vec::<CaseContact>::new());
    let contacts_error = RwSignal::new(String::new());
    let managing_people = RwSignal::new(false);

    {
        let case_id = case_id.clone();
        spawn_local(async move {
            let recent =
                list_case_notes(case_id.clone(), CaseNoteListFilters::default(), 0, 20).await;
            let draft_filter = CaseNoteListFilters {
                state: Some(CaseNoteState::Draft),
                ..CaseNoteListFilters::default()
            };
            let own_drafts = list_case_notes(case_id, draft_filter, 0, 100).await;
            let _ = notes_loading.try_set(false);
            match (recent, own_drafts) {
                (Ok(recent), Ok(own_drafts)) => {
                    let _ = notes.try_set(recent.items);
                    let _ = drafts.try_set(own_drafts.items);
                }
                (Err(e), _) | (_, Err(e)) => {
                    let _ = notes_error.try_set(err_text(e));
                }
            }
        });
    }
    let load_contacts = move || {
        let case_id = case_sv.get_value();
        spawn_local(async move {
            match list_case_contacts(case_id).await {
                Ok(found) => {
                    let _ = contacts.try_set(found);
                    let _ = contacts_error.try_set(String::new());
                }
                Err(e) => {
                    let _ = contacts_error.try_set(err_text(e));
                }
            }
        });
    };
    if show_people {
        load_contacts();
        // The summary reflects changes made in the full list once it closes.
        Effect::new(move |was_open: Option<bool>| {
            let open = managing_people.get();
            if was_open == Some(true) && !open {
                load_contacts();
            }
            open
        });
    }

    let intake = Memo::new(move |_| properties.with(|all| intake_status(all)));
    let intake_item = move || {
        let (intake_active, intake_summary) = intake.get();
        (show_intake && intake_active > 0).then(|| {
            view! {
                <li class="flex flex-wrap items-center justify-between gap-3 py-3">
                    <div class="min-w-0">
                        <p class="flex flex-wrap items-center gap-2 font-semibold">
                            "Case intake"
                            <span class="rounded-full bg-amber-500/15 px-2 py-0.5 text-xs font-semibold text-amber-300">
                                {format!("{intake_active} active")}
                            </span>
                        </p>
                        <p class="mt-0.5 text-sm text-slate-500">{intake_summary.clone()}</p>
                    </div>
                    <A
                        href=format!("/cases/{}/intake", case_sv.get_value())
                        attr:class="shrink-0 rounded-full bg-primary-600 px-4 py-2 text-sm font-semibold text-white hover:bg-primary-700"
                    >
                        "Open intake"
                    </A>
                </li>
            }
        })
    };

    let draft_items = move || {
        drafts
            .get()
            .into_iter()
            .filter(|n| viewer_id.with_value(|v| n.author_user_id == *v))
            .map(|n| {
                let href = format!("/cases/{}/notes/{}", case_sv.get_value(), n.id);
                view! {
                    <li class="flex flex-wrap items-center justify-between gap-3 py-3">
                        <div class="min-w-0">
                            <p class="font-semibold">
                                {format!("Your note from {} is still a draft", month_day(&n.activity_date))}
                            </p>
                            <p class="mt-0.5 text-sm text-slate-500">
                                "Only you and admins can see it until it's finalized."
                            </p>
                        </div>
                        <A
                            href=href
                            attr:class="shrink-0 rounded-full border border-slate-700 bg-slate-900 px-4 py-2 text-sm font-semibold text-slate-200 hover:bg-slate-950"
                        >
                            "Open draft"
                        </A>
                    </li>
                }
            })
            .collect::<Vec<_>>()
    };

    let needs_count = move || {
        let own_drafts = drafts.with(|all| {
            all.iter()
                .filter(|n| viewer_id.with_value(|v| n.author_user_id == *v))
                .count()
        });
        own_drafts + usize::from(show_intake && intake.with(|(active, _)| *active > 0))
    };

    let needs_you = view! {
        <section class=CARD aria-labelledby="needs-you-heading">
            <div class="flex items-center gap-3">
                <h2 id="needs-you-heading" class="text-xl font-semibold">"What needs you today"</h2>
                <span class="rounded-full bg-slate-950 px-2.5 py-0.5 text-xs font-semibold text-slate-400">
                    {move || match needs_count() {
                        1 => "1 item".to_string(),
                        n => format!("{n} items"),
                    }}
                </span>
            </div>
            <Show
                when=move || { needs_count() > 0 }
                fallback=move || {
                    let message = if notes_loading.get() {
                        "Loading\u{2026}".to_string()
                    } else if !notes_error.get().is_empty() {
                        notes_error.get()
                    } else {
                        "Nothing needs you on this case right now.".to_string()
                    };
                    view! { <p class="mt-3 text-sm text-slate-500">{message}</p> }
                }
            >
                <ul class="mt-1 divide-y divide-slate-800">
                    {intake_item}
                    {draft_items}
                </ul>
            </Show>
        </section>
    };

    let people_summary = move || {
        if !show_people {
            return ().into_any();
        }
        let manage = move |_| managing_people.set(true);
        view! {
            <section class=CARD aria-labelledby="people-heading">
                <div class=CARD_HEAD>
                    <h2 id="people-heading" class="text-[19px] font-semibold">"People on this case"</h2>
                    {can_edit_people.then(|| view! {
                        <button type="button" on:click=manage class=SMALL_BUTTON>"+ Add"</button>
                    })}
                </div>
                <Show when=move || !contacts_error.get().is_empty()>
                    <p class="mt-3 text-sm text-rose-400">{move || contacts_error.get()}</p>
                </Show>
                <ul class="mt-4 space-y-4">
                    {move || {
                        contacts
                            .get()
                            .into_iter()
                            .map(|c| {
                                let detail = person_detail(&c);
                                view! {
                                    <li class="flex items-start gap-3">
                                        <span
                                            aria-hidden="true"
                                            class=format!("flex h-10 w-10 shrink-0 items-center justify-center rounded-full text-sm font-bold {}", avatar_classes(c.role))
                                        >
                                            {initials(&c.contact_name)}
                                        </span>
                                        <div class="min-w-0 flex-1">
                                            <p class="truncate font-bold">{c.contact_name}</p>
                                            <p class="text-sm text-slate-500">{detail}</p>
                                        </div>
                                        {can_edit_people.then(|| view! {
                                            <button type="button" on:click=manage class=SMALL_BUTTON>"Manage"</button>
                                        })}
                                    </li>
                                }
                            })
                            .collect::<Vec<_>>()
                    }}
                </ul>
                <button
                    type="button"
                    on:click=move |_| managing_people.update(|open| *open = !*open)
                    aria-expanded=move || managing_people.get().to_string()
                    class=format!("mt-3.5 {LINK}")
                >
                    {move || if managing_people.get() { "Hide the full list" } else { "Manage everyone on this case" }}
                </button>
            </section>
        }
        .into_any()
    };

    let client_label = if client_first_name.is_empty() {
        "The client can see these".to_string()
    } else {
        format!("{client_first_name} can see these")
    };
    let case_info = view! {
        <section class=CARD aria-labelledby="case-info-heading">
            <div class=CARD_HEAD>
                <h2 id="case-info-heading" class="text-[19px] font-semibold">"Case information"</h2>
            </div>
            <p class="mt-3.5 flex items-center gap-1.5 text-sm font-semibold text-slate-400">
                <svg class="h-4 w-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true"><path d="M2 12s4-7 10-7 10 7 10 7-4 7-10 7S2 12 2 12z"></path><circle cx="12" cy="12" r="3"></circle></svg>
                {client_label}
            </p>
            <dl class="mt-2 space-y-2 text-[14.5px]">
                {move || properties
                    .get()
                    .into_iter()
                    .filter(|p| p.visibility == Visibility::Shared)
                    .filter(|p| !p.value.trim().is_empty())
                    .map(|p| view! {
                        <div class="flex justify-between gap-3">
                            <dt class="text-slate-500">{p.key}</dt>
                            <dd class="text-right">{p.value}</dd>
                        </div>
                    })
                    .collect::<Vec<_>>()}
            </dl>
            {move || {
                let restricted_sections = properties.with(|all| {
                    let restricted: Vec<CaseProperty> = all
                        .iter()
                        .filter(|p| p.visibility != Visibility::Shared)
                        .cloned()
                        .collect();
                    section_fill_counts(&restricted)
                });
                (!restricted_sections.is_empty()).then(|| {
                let last = restricted_sections.len() - 1;
                view! {
                    <div class="mt-4 rounded-[14px] border border-[#fcd34d] bg-[#fffdf5] px-4 py-3.5">
                        <p class="flex items-center gap-1.5 text-sm font-semibold text-[#7a4a06]">
                            <svg class="h-4 w-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true"><rect x="5" y="11" width="14" height="10" rx="2"></rect><path d="M8 11V7a4 4 0 0 1 8 0v4"></path></svg>
                            "Volunteers only"
                        </p>
                        <p class="mt-1 text-sm text-[#5b4a2c]">
                            {restricted_sections
                                .into_iter()
                                .enumerate()
                                .map(|(i, (name, filled, total))| view! {
                                    <button
                                        type="button"
                                        on:click=move |_| on_open_intake.run(())
                                        class="font-semibold text-primary-300 underline underline-offset-2"
                                    >
                                        {name}
                                    </button>
                                    {format!(" {filled} of {total} filled in")}
                                    {(i < last).then_some(" \u{b7} ")}
                                })
                                .collect::<Vec<_>>()}
                        </p>
                    </div>
                }
                })
            }}
        </section>
    };

    let recent_activity = view! {
        <section class=CARD aria-labelledby="activity-heading">
            <div class=CARD_HEAD>
                <h2 id="activity-heading" class="text-[19px] font-semibold">"Recent activity"</h2>
            </div>
            {move || {
                let items: Vec<_> = notes
                    .get()
                    .into_iter()
                    .filter(|n| n.state != CaseNoteState::Discarded)
                    .take(RECENT_LIMIT)
                    .map(|n| {
                        let mine = viewer_id.with_value(|v| n.author_user_id == *v);
                        let kind = n
                            .primary_interaction
                            .map(|kind| format!("{} note", kind.label()))
                            .unwrap_or_else(|| "Note".to_string());
                        let (what, dot) = if n.state == CaseNoteState::Draft {
                            let who = if mine { "You".to_string() } else { n.author.clone() };
                            (format!("{who} started a draft note"), "bg-amber-500")
                        } else {
                            (format!("{kind} finalized by {}", n.author), "bg-slate-700")
                        };
                        view! {
                            <li class="relative">
                                <span class=format!("absolute -left-[25px] top-1.5 h-3 w-3 rounded-full border-2 border-slate-900 {dot}")></span>
                                <p class="text-sm text-slate-500">{month_day(&n.activity_date)}</p>
                                <p>{what}</p>
                            </li>
                        }
                    })
                    .collect();
                if items.is_empty() {
                    let message = if notes_loading.get() {
                        "Loading\u{2026}".to_string()
                    } else if !notes_error.get().is_empty() {
                        notes_error.get()
                    } else {
                        "No notes on this case yet.".to_string()
                    };
                    view! { <p class="mt-4 text-sm text-slate-500">{message}</p> }.into_any()
                } else {
                    view! {
                        <ol class="mt-4 ml-1.5 space-y-3.5 border-l-2 border-slate-800 pl-[18px]">{items}</ol>
                    }
                    .into_any()
                }
            }}
            <button type="button" on:click=move |_| on_open_notes.run(()) class=format!("mt-3.5 {LINK}")>
                "See all activity"
            </button>
        </section>
    };

    view! {
        <div class="grid items-start gap-6 xl:grid-cols-[minmax(0,1fr)_20rem]">
            <div class="min-w-0 space-y-6">
                {needs_you}
                {show_plan.then(|| view! { <CasePlanCard case_id=case_sv.get_value() can_edit=can_edit_plan can_note=can_note_plan /> })}
                <Show when=move || managing_people.get()>
                    <CaseContactsPanel case_id=case_sv.get_value() can_edit=can_edit_people />
                </Show>
            </div>
            <aside class="space-y-6" aria-label="Case at a glance">
                {people_summary}
                {show_case_info.then_some(case_info)}
                {recent_activity}
            </aside>
        </div>
    }
}

fn short_date(raw: &str) -> String {
    raw.get(..10).unwrap_or(raw).to_string()
}

pub(crate) fn month_day(raw: &str) -> String {
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let month = raw.get(5..7).and_then(|m| m.parse::<usize>().ok());
    let day = raw.get(8..10).and_then(|d| d.parse::<u32>().ok());
    match (month, day) {
        (Some(m @ 1..=12), Some(d)) => format!("{} {d}", MONTHS[m - 1]),
        _ => short_date(raw),
    }
}

fn avatar_classes(role: CaseContactRole) -> &'static str {
    match role {
        CaseContactRole::Client => "bg-[#e3f5ec] text-[#11633f]",
        CaseContactRole::Attorney => "bg-[#e3effa] text-[#1a5d99]",
        CaseContactRole::EmergencyContact => "bg-[#fbe4e1] text-[#9b2c22]",
        _ => "bg-[#f3ece1] text-[#5b4f43]",
    }
}

fn person_detail(contact: &CaseContact) -> String {
    let preferred = match contact.role {
        CaseContactRole::Client => [&contact.phone, &contact.email, &contact.note],
        _ => [&contact.organization_name, &contact.note, &contact.phone],
    };
    match preferred.into_iter().find(|v| !v.trim().is_empty()) {
        Some(extra) => format!("{} \u{b7} {extra}", contact.role.label()),
        None => contact.role.label().to_string(),
    }
}

fn section_fill_counts(properties: &[CaseProperty]) -> Vec<(String, usize, usize)> {
    let mut sections: Vec<(String, usize, usize)> = Vec::new();
    for p in properties {
        if p.section.trim().is_empty() || p.section.starts_with("Questionnaire") {
            continue;
        }
        let filled = usize::from(!p.value.trim().is_empty());
        match sections.iter_mut().find(|(name, _, _)| *name == p.section) {
            Some(entry) => {
                entry.1 += filled;
                entry.2 += 1;
            }
            None => sections.push((p.section.clone(), filled, 1)),
        }
    }
    sections
}
