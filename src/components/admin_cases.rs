//! The cases waiting for an admin to accept or decline them.

use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::components::A;

use crate::server_fns::cases::{
    list_pending_case_requests, set_case_review_decision, CaseStatus, CaseSummary,
};
use crate::server_fns::err_text;
use crate::state::AppState;

#[component]
pub fn CaseRequests(reload: RwSignal<u32>) -> impl IntoView {
    let state = expect_context::<AppState>();
    let items = RwSignal::new(Vec::<CaseSummary>::new());
    let loading = RwSignal::new(true);
    let load_error = RwSignal::new(None::<String>);
    let request_generation = RwSignal::new(0u64);

    Effect::new(move |_| {
        reload.track();
        if !state.has_operations_admin_permissions() {
            return;
        }
        loading.set(true);
        request_generation.update(|generation| *generation += 1);
        let generation = request_generation.get_untracked();
        spawn_local(async move {
            let response = list_pending_case_requests().await;
            if request_generation.get_untracked() != generation {
                return;
            }
            match response {
                Ok(list) => {
                    // Keep the badge honest against what is on screen.
                    state.cases_pending_review.set(list.len() as i64);
                    items.set(list);
                    load_error.set(None);
                }
                Err(e) => load_error.set(Some(err_text(e))),
            }
            loading.set(false);
        });
    });

    move || {
        if let Some(msg) = load_error.get() {
            return view! { <p class="text-sm text-rose-300">"Could not load case requests: " {msg}</p> }
                .into_any();
        }
        if loading.get() && items.with(Vec::is_empty) {
            return view! { <p class="text-sm text-slate-500">"Loading\u{2026}"</p> }.into_any();
        }
        let list = items.get();
        if list.is_empty() {
            return view! {
                <div class="border-l-2 border-emerald-500/50 py-2 pl-3">
                    <p class="text-sm text-slate-400">"No case requests are waiting for review."</p>
                </div>
            }
            .into_any();
        }
        view! {
            <div class="space-y-3">
                {list
                    .into_iter()
                    .map(|c| view! { <CaseRequestCard case=c reload=reload /> })
                    .collect_view()}
            </div>
        }
        .into_any()
    }
}

/// One case awaiting a decision, with the context needed to make it.
#[component]
fn CaseRequestCard(case: CaseSummary, reload: RwSignal<u32>) -> impl IntoView {
    let state = expect_context::<AppState>();
    let case_id = StoredValue::new(case.id.clone());
    let detail_href = format!("/admin/cases/{}", case.id);
    let saving = RwSignal::new(false);
    let error = RwSignal::new(None::<String>);
    // Declining reveals an inline reason field; the reason is part of the decision.
    let declining = RwSignal::new(false);
    let reason = RwSignal::new(String::new());

    let decline_reason_id = format!("case-decline-reason-{}", case.id);

    let decide = move |accept: bool, reason_text: String| {
        saving.set(true);
        error.set(None);
        spawn_local(async move {
            match set_case_review_decision(case_id.get_value(), accept, reason_text).await {
                Ok(()) => {
                    // Reloading re-reads the list, which resets the badge with it.
                    reload.update(|r| *r += 1);
                    state.refresh_badges();
                }
                Err(e) => {
                    error.set(Some(err_text(e)));
                    saving.set(false);
                }
            }
        });
    };

    let accept = move |_| decide(true, String::new());
    let confirm_decline = move |_| {
        let text = reason.get_untracked().trim().to_string();
        if text.is_empty() {
            error.set(Some(
                "Please give a reason so the client knows why.".to_string(),
            ));
            return;
        }
        decide(false, text);
    };

    view! {
        <div class="rounded-card border border-slate-800 bg-slate-900 shadow-card p-4">
            <div class="flex flex-wrap items-center justify-between gap-2">
                <span class="min-w-0 truncate font-medium text-slate-100">{case.name.clone()}</span>
                <span class=format!(
                    "rounded-full px-2 py-0.5 text-xs font-medium {}",
                    CaseStatus::PendingReview.badge_classes(),
                )>{CaseStatus::PendingReview.label()}</span>
            </div>
            <p class="mt-2 text-xs text-slate-400">"Client: " {case.owner_full_name()}</p>

            {move || {
                error.get().map(|msg| view! {
                    <p class="mt-2 text-xs text-rose-300" role="alert">{msg}</p>
                })
            }}

            <Show when=move || !declining.get()>
                <div class="mt-3 flex flex-wrap items-center gap-2">
                    <A
                        href=detail_href.clone()
                        attr:class="rounded-full border border-slate-700 px-3 py-1.5 text-xs font-medium text-slate-200 hover:bg-slate-800"
                    >
                        "View details"
                    </A>
                    <button
                        on:click=accept
                        prop:disabled=move || saving.get()
                        class="rounded-full bg-emerald-600 px-3 py-1.5 text-xs font-semibold text-white hover:bg-emerald-500 disabled:opacity-50"
                    >
                        "Accept"
                    </button>
                    <button
                        on:click=move |_| declining.set(true)
                        prop:disabled=move || saving.get()
                        class="rounded-full border border-rose-500/40 px-3 py-1.5 text-xs font-semibold text-rose-300 hover:bg-rose-500/10 disabled:opacity-50"
                    >
                        "Decline"
                    </button>
                </div>
            </Show>

            <Show when=move || declining.get()>
                <div class="mt-3 space-y-2">
                    <label for=decline_reason_id.clone() class="block text-xs font-medium text-slate-300">
                        "Reason (shown to the client)"
                    </label>
                    <textarea
                        id=decline_reason_id.clone()
                        rows="2"
                        class="w-full rounded-xl border border-slate-700 bg-slate-900 px-3 py-2 text-sm text-slate-100 placeholder:text-slate-500 focus:border-primary-500 focus:outline-none"
                        placeholder="e.g. Outside our service area; referred to Lakeside Legal Aid."
                        prop:disabled=move || saving.get()
                        prop:value=move || reason.get()
                        on:input=move |ev| reason.set(event_target_value(&ev))
                    ></textarea>
                    <div class="flex gap-2">
                        <button
                            on:click=confirm_decline
                            prop:disabled=move || saving.get()
                            class="rounded-full bg-rose-600 px-3 py-1.5 text-xs font-semibold text-white hover:bg-rose-500 disabled:opacity-50"
                        >
                            {move || if saving.get() { "Saving\u{2026}" } else { "Confirm decline" }}
                        </button>
                        <button
                            on:click=move |_| {
                                declining.set(false);
                                error.set(None);
                            }
                            class="rounded-full border border-slate-700 px-3 py-1.5 text-xs font-medium text-slate-300 hover:bg-slate-800"
                        >
                            "Cancel"
                        </button>
                    </div>
                </div>
            </Show>
        </div>
    }
}
