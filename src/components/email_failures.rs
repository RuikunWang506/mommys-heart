//! The admin viewer for the durable email-delivery-failure log.
//!
//! Like [`crate::components::change_log::ChangeLog`], this is its own data
//! source: it fetches [`crate::server_fns::email_failures`] independently, on
//! demand, shows its own loading state, and paginates ("Load more") so the whole
//! history is never pulled into the browser at once. Mount it only when the
//! admin opens the panel.

use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::components::loading::Loading;
use crate::server_fns::email_failures::{list_email_failures_page, EmailFailure};
use crate::server_fns::err_text;
use crate::state::AppState;

/// How many rows load per page; each "Load more" grows the window by this much.
const PAGE_SIZE: i64 = 20;

/// A single rendered failure row.
fn failure_row(e: EmailFailure) -> AnyView {
    view! {
        <div class="rounded-lg border border-slate-800 bg-slate-950 p-3 text-xs">
            <div class="flex flex-wrap items-center justify-between gap-2">
                <span class="font-medium text-slate-200">{e.recipient}</span>
                <span class="text-slate-500">{e.at}</span>
            </div>
            <div class="mt-1 text-slate-400">
                <span class="text-slate-300">{e.context}</span> " · \"" {e.subject} "\""
            </div>
            <div class="mt-1 break-words text-rose-300">{e.error}</div>
        </div>
    }
    .into_any()
}

/// An independently-fetched, paginated list of recorded email delivery
/// failures, newest first. Requires operations-admin permissions (the server
/// function enforces this too).
#[component]
pub fn EmailFailureLog() -> impl IntoView {
    let state = expect_context::<AppState>();

    let rows = RwSignal::new(Vec::<EmailFailure>::new());
    let total = RwSignal::new(0i64);
    let window = RwSignal::new(PAGE_SIZE);
    let loading = RwSignal::new(false);
    let error = RwSignal::new(None::<String>);

    // Fetch `[0, window)` whenever the window grows (server functions run in the
    // browser after hydration, so wait for a confirmed session).
    Effect::new(move |_| {
        let count = window.get();
        if !state.is_authenticated() {
            return;
        }
        loading.set(true);
        error.set(None);
        spawn_local(async move {
            match list_email_failures_page(0, count).await {
                Ok(page) => {
                    rows.set(page.items);
                    total.set(page.total);
                }
                Err(e) => error.set(Some(err_text(e))),
            }
            loading.set(false);
        });
    });

    let body = move || {
        if let Some(msg) = error.get() {
            return view! { <p class="text-xs text-rose-300">{msg}</p> }.into_any();
        }
        // Spinner only on the very first load, so a "Load more" refetch doesn't
        // blank out the already-visible list.
        if loading.get() && rows.with(Vec::is_empty) {
            return view! { <Loading label="Loading email failures\u{2026}" /> }.into_any();
        }
        let items = rows.get();
        if items.is_empty() {
            return view! {
                <p class="text-xs text-slate-500">"No email delivery failures recorded."</p>
            }
            .into_any();
        }
        let shown = items.len() as i64;
        let total_n = total.get();
        let list = items.into_iter().map(failure_row).collect_view();
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
            view! { <p class="text-xs text-slate-500">{total_n} " failure(s) recorded"</p> }
                .into_any()
        };
        view! {
            <div class="space-y-2">{list}</div>
            {footer}
        }
        .into_any()
    };

    view! { <div class="mt-3 space-y-3">{body}</div> }
}
