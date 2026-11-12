//! The admin activity feed: what people have been doing across the site.
//!
//! Like [`crate::components::email_failures::EmailFailureLog`] and
//! [`crate::components::change_log::ChangeLog`], this is its own data source: it
//! fetches [`crate::server_fns::admin_activity`] independently, shows its own
//! loading state, and paginates ("Load more") so the whole history is never
//! pulled into the browser at once.
//!
//! It is the in-app half of the "Admin activity alert" notification category —
//! the other half is the daily digest email, which summarizes exactly these rows.

use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::components::A;

use crate::components::loading::Loading;
use crate::server_fns::admin_activity::{
    list_admin_activity_page, AdminActivityCategory, AdminActivityEvent,
};
use crate::server_fns::err_text;
use crate::state::AppState;

/// How many rows load per page; each "Load more" grows the window by this much.
const PAGE_SIZE: i64 = 20;

/// The badge colors for each category, matching the app's status-chip palette.
fn category_classes(category: AdminActivityCategory) -> &'static str {
    match category {
        AdminActivityCategory::CaseCreated => {
            "bg-emerald-500/15 text-emerald-300 ring-1 ring-emerald-500/30"
        }
        AdminActivityCategory::CaseNote => "bg-sky-500/15 text-sky-300 ring-1 ring-sky-500/30",
        AdminActivityCategory::CaseInformation => {
            "bg-amber-500/15 text-amber-300 ring-1 ring-amber-500/30"
        }
        AdminActivityCategory::Document => {
            "bg-primary-500/15 text-primary-300 ring-1 ring-primary-500/30"
        }
        AdminActivityCategory::Contact => "bg-slate-700/40 text-slate-300 ring-1 ring-slate-600",
    }
}

/// A single rendered activity row.
fn event_row(event: AdminActivityEvent) -> AnyView {
    let href = event.href();
    let subject_name = if event.subject_name.is_empty() {
        event.subject_id.clone()
    } else {
        event.subject_name.clone()
    };
    let subject = match href {
        Some(href) => view! {
            <A href=href attr:class="text-slate-200 underline decoration-slate-600 hover:text-white">
                {subject_name}
            </A>
        }
        .into_any(),
        None => view! { <span class="text-slate-200">{subject_name}</span> }.into_any(),
    };
    view! {
        <div class="rounded-lg border border-slate-800 bg-slate-950 p-3 text-xs">
            <div class="flex flex-wrap items-center justify-between gap-2">
                <span class=format!(
                    "inline-flex items-center rounded-full px-2 py-0.5 text-[0.65rem] font-semibold {}",
                    category_classes(event.category),
                )>{event.category.label()}</span>
                <span class="text-slate-500">{event.at}</span>
            </div>
            <div class="mt-1.5 text-slate-400">
                <span class="font-medium text-slate-200">{event.actor}</span>
                " " {event.summary} " \u{2014} " {subject}
            </div>
        </div>
    }
    .into_any()
}

/// An independently-fetched, paginated feed of recorded site activity, newest
/// first. Requires operations-admin permissions (the server function enforces
/// this too).
#[component]
pub fn AdminActivityFeed() -> impl IntoView {
    let state = expect_context::<AppState>();

    let rows = RwSignal::new(Vec::<AdminActivityEvent>::new());
    let total = RwSignal::new(0i64);
    let window = RwSignal::new(PAGE_SIZE);
    let loading = RwSignal::new(false);
    let error = RwSignal::new(None::<String>);
    // `None` is "every category"; changing it restarts from the first page.
    let category = RwSignal::new(None::<AdminActivityCategory>);

    // Reset the window whenever the filter changes, so a new filter starts from
    // the top rather than keeping an old deep offset.
    Effect::new(move |previous: Option<Option<AdminActivityCategory>>| {
        let selected = category.get();
        if previous.is_some_and(|p| p != selected) {
            window.set(PAGE_SIZE);
        }
        selected
    });

    // Fetch `[0, window)` whenever the window or filter changes (server
    // functions run in the browser after hydration, so wait for a session).
    Effect::new(move |_| {
        let count = window.get();
        let selected = category.get();
        if !state.is_authenticated() {
            return;
        }
        loading.set(true);
        error.set(None);
        spawn_local(async move {
            match list_admin_activity_page(selected, 0, count).await {
                Ok(page) => {
                    rows.set(page.items);
                    total.set(page.total);
                }
                Err(e) => error.set(Some(err_text(e))),
            }
            loading.set(false);
        });
    });

    let on_filter = move |ev| {
        let value = event_target_value(&ev);
        category.set(AdminActivityCategory::from_slug(&value));
    };

    let body = move || {
        if let Some(msg) = error.get() {
            return view! { <p class="text-xs text-rose-300">{msg}</p> }.into_any();
        }
        // Spinner only on the very first load, so a "Load more" refetch doesn't
        // blank out the already-visible list.
        if loading.get() && rows.with(Vec::is_empty) {
            return view! { <Loading label="Loading activity\u{2026}" /> }.into_any();
        }
        let items = rows.get();
        if items.is_empty() {
            return view! {
                <p class="text-xs text-slate-500">"No activity recorded yet."</p>
            }
            .into_any();
        }
        let shown = items.len() as i64;
        let total_n = total.get();
        let list = items.into_iter().map(event_row).collect_view();
        let footer = if shown < total_n {
            view! {
                <div class="flex items-center gap-3">
                    <button
                        on:click=move |_| window.update(|w| *w += PAGE_SIZE)
                        prop:disabled=move || loading.get()
                        class="rounded-full border border-slate-700 px-2 py-1 text-xs font-medium text-slate-300 hover:bg-slate-800 disabled:opacity-50"
                    >
                        {move || if loading.get() { "Loading\u{2026}" } else { "Load more" }}
                    </button>
                    <span class="text-xs text-slate-500">
                        "Showing " {shown} " of " {total_n}
                    </span>
                </div>
            }
            .into_any()
        } else {
            view! { <p class="text-xs text-slate-500">{total_n} " event(s) recorded"</p> }
                .into_any()
        };
        view! {
            <div class="space-y-2">{list}</div>
            {footer}
        }
        .into_any()
    };

    view! {
        <div class="mt-3 space-y-3">
            <label class="flex items-center gap-2 text-xs text-slate-400">
                "Show"
                <select
                    on:change=on_filter
                    class="rounded-xl border border-slate-700 bg-slate-900 px-2 py-1 text-xs text-slate-100 focus:border-primary-500 focus:outline-none"
                >
                    <option value="">"Everything"</option>
                    {AdminActivityCategory::ALL
                        .iter()
                        .map(|c| view! { <option value=c.slug()>{c.label()}</option> })
                        .collect_view()}
                </select>
            </label>
            {body}
        </div>
    }
}
