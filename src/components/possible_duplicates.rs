//! "Possible duplicates" on Manage Cases: cases with the same name, and
//! contacts that share a name, email, or phone number.
//!
//! The check runs only when someone asks for it, and it never changes a record.
//! It lists the facts that help decide which record to keep (case number,
//! status, clients, how much work is on each) with a link to open each one.

use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::components::A;

use crate::components::loading::Loading;
use crate::helpers::format::badge_pill;
use crate::server_fns::duplicates::{
    possible_duplicates, set_duplicate_dismissed, DuplicateCase, DuplicateContact, DuplicateKind,
    DuplicateReport, MAX_GROUPS,
};
use crate::server_fns::err_text;

fn plural(count: i64, one: &str, many: &str) -> String {
    if count == 1 {
        format!("1 {one}")
    } else {
        format!("{count} {many}")
    }
}

#[component]
pub fn PossibleDuplicates() -> impl IntoView {
    let report = RwSignal::new(None::<DuplicateReport>);
    let loading = RwSignal::new(false);
    let error = RwSignal::new(String::new());
    let show_dismissed = RwSignal::new(false);

    let load = move || {
        loading.set(true);
        error.set(String::new());
        let include_dismissed = show_dismissed.get_untracked();
        spawn_local(async move {
            match possible_duplicates(include_dismissed).await {
                Ok(found) => report.set(Some(found)),
                Err(e) => error.set(err_text(e)),
            }
            loading.set(false);
        });
    };
    let run = move |_| load();

    // Mark or unmark a group, then reload so the lists and counts stay true.
    let set_dismissed = Callback::new(
        move |(kind, ids, dismissed): (DuplicateKind, Vec<String>, bool)| {
            error.set(String::new());
            spawn_local(async move {
                match set_duplicate_dismissed(kind, ids, dismissed).await {
                    Ok(()) => load(),
                    Err(e) => error.set(err_text(e)),
                }
            });
        },
    );

    let results = move || {
        if loading.get() {
            return view! { <Loading label="Checking for duplicates…" /> }.into_any();
        }
        let Some(found) = report.get() else {
            return ().into_any();
        };
        let hidden = found.dismissed_case_groups + found.dismissed_contact_groups;
        view! {
            <div class="space-y-8">
                {(hidden > 0).then(|| view! {
                    <label class="flex items-center gap-2 text-sm text-slate-400">
                        <input
                            type="checkbox"
                            class="h-4 w-4 rounded border-slate-600 bg-slate-950"
                            prop:checked=move || show_dismissed.get()
                            on:change=move |event| {
                                show_dismissed.set(event_target_checked(&event));
                                load();
                            }
                        />
                        {format!(
                            "Show {} marked as not duplicates",
                            plural(hidden as i64, "group", "groups")
                        )}
                    </label>
                })}
                <CaseGroups found=found.clone() set_dismissed=set_dismissed />
                <ContactGroups found=found set_dismissed=set_dismissed />
            </div>
        }
        .into_any()
    };

    view! {
        <section class="space-y-4 border-t border-slate-800 pt-8" id="possible-duplicates">
            <div class="flex flex-wrap items-start justify-between gap-3">
                <div>
                    <h2 class="text-base font-semibold text-slate-100">"Possible duplicates"</h2>
                    <p class="mt-1 text-sm text-slate-400">
                        "Cases with the same name, and contacts that share a name, email, or phone number. Nothing is changed here: open each record, decide which one to keep, then withdraw or archive the extra one. If they are different people, mark the group as not duplicates."
                    </p>
                </div>
                <button
                    type="button"
                    on:click=run
                    prop:disabled=move || loading.get()
                    class="rounded-full border border-slate-700 px-3 py-1.5 text-sm font-medium text-slate-200 hover:bg-slate-800 disabled:opacity-50"
                >
                    {move || if report.with(Option::is_some) { "Check again" } else { "Check for duplicates" }}
                </button>
            </div>
            <Show when=move || !error.get().is_empty()>
                <p class="text-sm text-rose-300">{move || error.get()}</p>
            </Show>
            {results}
        </section>
    }
}

#[component]
fn CaseGroups(
    found: DuplicateReport,
    set_dismissed: Callback<(DuplicateKind, Vec<String>, bool)>,
) -> impl IntoView {
    let count = found.case_groups.len();
    let heading = if count == 0 {
        "No cases share a name.".to_string()
    } else {
        format!(
            "{} with the same name{}",
            plural(count as i64, "group of cases", "groups of cases"),
            if found.case_groups_truncated {
                format!(" (showing the first {MAX_GROUPS})")
            } else {
                String::new()
            }
        )
    };
    view! {
        <div class="space-y-3">
            <h3 class="text-sm font-semibold text-slate-200">{heading}</h3>
            {found
                .case_groups
                .into_iter()
                .map(|group| {
                    let ids = group.ids();
                    let dismissed = group.dismissed;
                    let label = if dismissed { "Marked as not duplicates" } else { "Same name" };
                    view! {
                        <div class="rounded-card border border-slate-800 bg-slate-900 shadow-card">
                            <GroupBar
                                label=label
                                kind=DuplicateKind::Case
                                ids=ids
                                dismissed=dismissed
                                set_dismissed=set_dismissed
                            />
                            <div class="divide-y divide-slate-800">
                                {group.cases.into_iter().map(|case| view! { <CaseRow case=case /> }).collect_view()}
                            </div>
                        </div>
                    }
                })
                .collect_view()}
        </div>
    }
}

#[component]
fn CaseRow(case: DuplicateCase) -> impl IntoView {
    let href = format!("/admin/cases/{}", case.id);
    let clients = if case.clients.is_empty() {
        "No client linked".to_string()
    } else {
        format!("Client: {}", case.clients.join(", "))
    };
    let filed_by = (!case.filed_by.is_empty()).then(|| format!("Filed by {}", case.filed_by));
    let activity = format!(
        "{} · {}{}",
        plural(case.note_count, "case note", "case notes"),
        plural(case.message_count, "message", "messages"),
        case.last_message
            .as_deref()
            .map(|at| format!(", last on {at}"))
            .unwrap_or_default()
    );
    view! {
        <div class="flex flex-col gap-1 p-4 sm:flex-row sm:items-start sm:justify-between">
            <div class="min-w-0">
                <div class="flex flex-wrap items-center gap-2">
                    <A href=href attr:class="font-semibold text-primary-300 hover:text-primary-200">
                        {case.name}
                    </A>
                    <span class="font-mono text-xs text-slate-500" title="Case number">{case.id}</span>
                    <span class=badge_pill(case.status.badge_classes())>{case.status.label()}</span>
                </div>
                <p class="mt-1 text-sm text-slate-400">
                    {clients}
                    {filed_by.map(|f| view! { <span class="text-slate-500">" · " {f}</span> })}
                </p>
            </div>
            <p class="shrink-0 text-xs text-slate-500 sm:text-right">{activity}</p>
        </div>
    }
}

#[component]
fn ContactGroups(
    found: DuplicateReport,
    set_dismissed: Callback<(DuplicateKind, Vec<String>, bool)>,
) -> impl IntoView {
    if !found.contacts_checked {
        return view! {
            <p class="text-sm text-slate-500">
                "Contacts were not checked because your account does not have access to the contact directory."
            </p>
        }
        .into_any();
    }
    let count = found.contact_groups.len();
    let heading = if count == 0 {
        "No contacts share a name, email, or phone number.".to_string()
    } else {
        format!(
            "{} that may be the same person{}",
            plural(count as i64, "group of contacts", "groups of contacts"),
            if found.contact_groups_truncated {
                format!(" (showing the first {MAX_GROUPS})")
            } else {
                String::new()
            }
        )
    };
    view! {
        <div class="space-y-3">
            <h3 class="text-sm font-semibold text-slate-200">{heading}</h3>
            {found
                .contact_groups
                .into_iter()
                .map(|group| {
                    let reasons = group
                        .reasons
                        .iter()
                        .map(|r| r.label())
                        .collect::<Vec<_>>()
                        .join(" · ");
                    let ids = group.ids();
                    let dismissed = group.dismissed;
                    let label = if dismissed {
                        format!("Marked as not duplicates ({reasons})")
                    } else {
                        reasons
                    };
                    view! {
                        <div class="rounded-card border border-slate-800 bg-slate-900 shadow-card">
                            <GroupBar
                                label=label
                                kind=DuplicateKind::Contact
                                ids=ids
                                dismissed=dismissed
                                set_dismissed=set_dismissed
                            />
                            <div class="divide-y divide-slate-800">
                                {group
                                    .contacts
                                    .into_iter()
                                    .map(|contact| view! { <ContactRow contact=contact /> })
                                    .collect_view()}
                            </div>
                        </div>
                    }
                })
                .collect_view()}
        </div>
    }
    .into_any()
}

#[component]
fn ContactRow(contact: DuplicateContact) -> impl IntoView {
    let href = format!("/contacts/{}", contact.id);
    let name = if contact.name.is_empty() {
        "(no name)".to_string()
    } else {
        contact.name.clone()
    };
    let reach = [contact.email.trim(), contact.phone.trim()]
        .into_iter()
        .filter(|v| !v.is_empty())
        .collect::<Vec<_>>()
        .join(" · ");
    let summary = format!(
        "{}{} · updated {}",
        plural(contact.case_count, "linked case", "linked cases"),
        if contact.has_account {
            " · has an account"
        } else {
            ""
        },
        contact.updated_on
    );
    view! {
        <div class="flex flex-col gap-1 p-4 sm:flex-row sm:items-start sm:justify-between">
            <div class="min-w-0">
                <div class="flex flex-wrap items-center gap-2">
                    <A href=href attr:class="font-semibold text-primary-300 hover:text-primary-200">
                        {name}
                    </A>
                    {contact
                        .types
                        .iter()
                        .map(|t| view! { <span class=badge_pill(t.badge_classes())>{t.label()}</span> })
                        .collect_view()}
                </div>
                {(!reach.is_empty())
                    .then(|| view! { <p class="mt-1 truncate text-sm text-slate-400">{reach}</p> })}
            </div>
            <p class="shrink-0 text-xs text-slate-500 sm:text-right">{summary}</p>
        </div>
    }
}

/// A group's header: why it was grouped, and the button that marks it as not
/// duplicates (or lists it again).
#[component]
fn GroupBar(
    #[prop(into)] label: String,
    kind: DuplicateKind,
    ids: Vec<String>,
    dismissed: bool,
    set_dismissed: Callback<(DuplicateKind, Vec<String>, bool)>,
) -> impl IntoView {
    let ids = StoredValue::new(ids);
    let (text, title) = if dismissed {
        (
            "List again",
            "Show this group in the possible duplicates again",
        )
    } else {
        (
            "Not duplicates",
            "These are different records. Hide this group until another matching record appears.",
        )
    };
    view! {
        <div class="flex items-center justify-between gap-3 border-b border-slate-800 px-4 py-2">
            <p class="text-xs font-medium text-slate-400">{label}</p>
            <button
                type="button"
                title=title
                on:click=move |_| set_dismissed.run((kind, ids.get_value(), !dismissed))
                class="shrink-0 rounded-full border border-slate-700 px-2.5 py-1 text-xs font-medium text-slate-300 hover:bg-slate-800"
            >
                {text}
            </button>
        </div>
    }
}
