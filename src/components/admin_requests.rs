//! Shared request center for one admin-request kind.

use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::components::A;

use crate::server_fns::admin_requests::{
    decide_admin_request, list_active_admin_requests_by_kind, list_admin_request_history_by_kind,
    AdminRequest, AdminRequestKind, AdminRequestStatus,
};
use crate::server_fns::err_text;
use crate::state::AppState;

const HISTORY_PAGE_SIZE: i64 = 20;
const MAX_HISTORY_WINDOW: i64 = 1_000;

fn status_classes(status: AdminRequestStatus) -> &'static str {
    match status {
        AdminRequestStatus::Pending => "bg-amber-500/15 text-amber-300 ring-1 ring-amber-500/30",
        AdminRequestStatus::Approved => {
            "bg-emerald-500/15 text-emerald-300 ring-1 ring-emerald-500/30"
        }
        AdminRequestStatus::Denied => "bg-rose-500/15 text-rose-300 ring-1 ring-rose-500/30",
    }
}

fn active_heading(kind: AdminRequestKind) -> &'static str {
    match kind {
        AdminRequestKind::CaseCapabilities => "Active case-permission requests",
        AdminRequestKind::Role => "Active role requests",
        AdminRequestKind::InformationAccess => "Active information-access requests",
    }
}

fn history_heading(kind: AdminRequestKind) -> &'static str {
    match kind {
        AdminRequestKind::CaseCapabilities => "Case-permission request history",
        AdminRequestKind::Role => "Role request history",
        AdminRequestKind::InformationAccess => "Information-access request history",
    }
}

fn active_empty_text(kind: AdminRequestKind, is_site_admin: bool) -> &'static str {
    match (kind, is_site_admin) {
        (AdminRequestKind::CaseCapabilities, true) => {
            "No case-permission requests are waiting for review."
        }
        (AdminRequestKind::Role, true) => "No role requests are waiting for review.",
        (AdminRequestKind::InformationAccess, true) => {
            "No information-access requests are waiting for review."
        }
        (AdminRequestKind::CaseCapabilities, false) => {
            "You have no active case-permission requests."
        }
        (AdminRequestKind::Role, false) => "You have no active role requests.",
        (AdminRequestKind::InformationAccess, false) => {
            "You have no active information-access requests."
        }
    }
}

fn history_empty_text(kind: AdminRequestKind) -> &'static str {
    match kind {
        AdminRequestKind::CaseCapabilities => "No case-permission requests have been decided yet.",
        AdminRequestKind::Role => "No role requests have been decided yet.",
        AdminRequestKind::InformationAccess => {
            "No information-access requests have been decided yet."
        }
    }
}

fn active_loading_text(kind: AdminRequestKind) -> &'static str {
    match kind {
        AdminRequestKind::CaseCapabilities => "Loading case-permission requests...",
        AdminRequestKind::Role => "Loading role requests...",
        AdminRequestKind::InformationAccess => "Loading information-access requests...",
    }
}

fn history_loading_text(kind: AdminRequestKind) -> &'static str {
    match kind {
        AdminRequestKind::CaseCapabilities => "Loading case-permission history...",
        AdminRequestKind::Role => "Loading role request history...",
        AdminRequestKind::InformationAccess => "Loading information-access history...",
    }
}

#[component]
pub fn AdminRequestCenter(
    kind: AdminRequestKind,
    is_site_admin: bool,
    reload: RwSignal<u32>,
) -> impl IntoView {
    let state = expect_context::<AppState>();
    let active = RwSignal::new(Vec::<AdminRequest>::new());
    let history = RwSignal::new(Vec::<AdminRequest>::new());
    let history_total = RwSignal::new(0i64);
    let history_window = RwSignal::new(HISTORY_PAGE_SIZE);
    let history_open = RwSignal::new(false);
    let active_loading = RwSignal::new(true);
    let history_loading = RwSignal::new(false);
    let active_error = RwSignal::new(None::<String>);
    let history_error = RwSignal::new(None::<String>);
    let active_generation = RwSignal::new(0u64);
    let history_generation = RwSignal::new(0u64);
    let history_id = format!("admin-request-history-{}", kind.slug());
    let active_title = active_heading(kind);
    let history_title = history_heading(kind);

    Effect::new(move |_| {
        reload.track();
        match kind {
            AdminRequestKind::CaseCapabilities => {
                state.admin_case_request_pending.track();
            }
            AdminRequestKind::Role => {
                state.admin_role_request_pending.track();
            }
            AdminRequestKind::InformationAccess => {
                state.admin_information_request_pending.track();
            }
        }
        if !state.is_authenticated() {
            return;
        }
        active_loading.set(true);
        active_generation.update(|generation| *generation += 1);
        let generation = active_generation.get_untracked();
        spawn_local(async move {
            let response = list_active_admin_requests_by_kind(kind).await;
            if active_generation.get_untracked() != generation {
                return;
            }
            match response {
                Ok(items) => {
                    let count = items.len() as i64;
                    active.set(items);
                    active_error.set(None);
                    if is_site_admin {
                        match kind {
                            AdminRequestKind::CaseCapabilities => {
                                if state.admin_case_request_pending.get_untracked() != count {
                                    state.admin_case_request_pending.set(count);
                                }
                            }
                            AdminRequestKind::Role => {
                                if state.admin_role_request_pending.get_untracked() != count {
                                    state.admin_role_request_pending.set(count);
                                }
                            }
                            AdminRequestKind::InformationAccess => {
                                if state.admin_information_request_pending.get_untracked() != count
                                {
                                    state.admin_information_request_pending.set(count);
                                }
                            }
                        }
                    }
                }
                Err(error) => active_error.set(Some(err_text(error))),
            }
            active_loading.set(false);
        });
    });

    Effect::new(move |_| {
        if !history_open.get() {
            return;
        }
        let count = history_window.get();
        reload.track();
        if !state.is_authenticated() {
            return;
        }
        history_loading.set(true);
        history_generation.update(|generation| *generation += 1);
        let generation = history_generation.get_untracked();
        spawn_local(async move {
            let response = list_admin_request_history_by_kind(kind, 0, count).await;
            if history_generation.get_untracked() != generation {
                return;
            }
            match response {
                Ok(page) => {
                    history.set(page.items);
                    history_total.set(page.total);
                    history_error.set(None);
                }
                Err(error) => history_error.set(Some(err_text(error))),
            }
            history_loading.set(false);
        });
    });

    let request_list = move |items: Vec<AdminRequest>, actionable: bool| {
        items
            .into_iter()
            .map(|request| {
                view! {
                    <RequestCard request=request actionable=actionable reload=reload />
                }
            })
            .collect_view()
    };

    let active_section = move || {
        if let Some(error) = active_error.get() {
            return view! { <p class="text-sm text-rose-300" role="alert">{error}</p> }.into_any();
        }
        if active_loading.get() && active.with(Vec::is_empty) {
            return view! { <p class="text-sm text-slate-500">{active_loading_text(kind)}</p> }
                .into_any();
        }
        let items = active.get();
        if items.is_empty() {
            view! {
                <div class="border-l-2 border-emerald-500/50 py-2 pl-3">
                    <p class="text-sm text-slate-400">{active_empty_text(kind, is_site_admin)}</p>
                </div>
            }
            .into_any()
        } else {
            view! { <div class="space-y-3">{request_list(items, is_site_admin)}</div> }.into_any()
        }
    };

    let history_section = move || {
        if let Some(error) = history_error.get() {
            return view! { <p class="text-sm text-rose-300" role="alert">{error}</p> }.into_any();
        }
        if history_loading.get() && history.with(Vec::is_empty) {
            return view! { <p class="text-sm text-slate-500">{history_loading_text(kind)}</p> }
                .into_any();
        }
        let items = history.get();
        if items.is_empty() {
            return view! {
                <p class="text-sm text-slate-500">{history_empty_text(kind)}</p>
            }
            .into_any();
        }
        let shown = items.len() as i64;
        let total = history_total.get();
        let footer = if shown < total && history_window.get() < MAX_HISTORY_WINDOW {
            view! {
                <div class="mt-3 flex items-center gap-3">
                    <button
                        type="button"
                        on:click=move |_| {
                            history_window.update(|window| {
                                *window = (*window + HISTORY_PAGE_SIZE).min(MAX_HISTORY_WINDOW)
                            })
                        }
                        prop:disabled=move || history_loading.get()
                        class="rounded-full border border-slate-700 px-2.5 py-1.5 text-xs font-medium text-slate-300 hover:bg-slate-800 disabled:opacity-50"
                    >
                        {move || if history_loading.get() { "Loading..." } else { "Load more" }}
                    </button>
                    <span class="text-xs text-slate-500">"Showing " {shown} " of " {total}</span>
                </div>
            }
            .into_any()
        } else if shown < total {
            view! {
                <p class="mt-3 text-xs text-slate-500">
                    "Only the first 1,000 matching requests are shown."
                </p>
            }
            .into_any()
        } else {
            view! { <p class="mt-3 text-xs text-slate-500">{total} " request(s) in history"</p> }
                .into_any()
        };
        view! {
            <div class="space-y-3">{request_list(items, false)}</div>
            {footer}
        }
        .into_any()
    };

    view! {
        <div class="space-y-8">
            <section>
                <div class="mb-4 flex flex-wrap items-end justify-between gap-3">
                    <div class="flex items-center gap-2">
                        <h2 class="text-base font-semibold text-slate-100">{active_title}</h2>
                        <span class="inline-flex min-w-5 items-center justify-center rounded-full bg-amber-500/15 px-1.5 py-0.5 text-xs font-semibold text-amber-300">
                            {move || active.get().len()}
                        </span>
                    </div>
                    <button
                        type="button"
                        on:click=move |_| reload.update(|value| *value += 1)
                        prop:disabled=move || {
                            active_loading.get() || (history_open.get() && history_loading.get())
                        }
                        class="rounded-full border border-slate-700 px-2.5 py-1.5 text-xs font-medium text-slate-300 hover:bg-slate-800 disabled:opacity-50"
                    >
                        "Refresh"
                    </button>
                </div>
                {active_section}
            </section>
            <section class="border-t border-slate-800 pt-6">
                <h2>
                    <button
                        type="button"
                        on:click=move |_| history_open.update(|open| *open = !*open)
                        aria-expanded=move || history_open.get().to_string()
                        aria-controls=history_id.clone()
                        class="flex w-full items-center justify-between gap-3 text-left"
                    >
                        <span class="text-base font-semibold text-slate-100">{history_title}</span>
                        <span class="text-xs font-medium text-slate-400">
                            {move || if history_open.get() { "Hide" } else { "Show" }}
                        </span>
                    </button>
                </h2>
                <Show when=move || history_open.get()>
                    <div id=history_id.clone() class="mt-4">
                        {history_section}
                    </div>
                </Show>
            </section>
        </div>
    }
}

#[component]
fn RequestCard(request: AdminRequest, actionable: bool, reload: RwSignal<u32>) -> impl IntoView {
    let state = expect_context::<AppState>();
    let request_id = StoredValue::new(request.id.clone());
    let decision_note = RwSignal::new(String::new());
    let deciding = RwSignal::new(false);
    let error = RwSignal::new(None::<String>);
    let decision_note_id = format!("admin-request-note-{}", request.id);

    let decide = move |approve: bool| {
        if deciding.get_untracked() {
            return;
        }
        deciding.set(true);
        error.set(None);
        let request_id = request_id.get_value();
        let note = decision_note.get_untracked();
        spawn_local(async move {
            match decide_admin_request(request_id, approve, note).await {
                Ok(_) => {
                    reload.update(|value| *value += 1);
                    state.refresh_badges();
                }
                Err(request_error) => error.set(Some(err_text(request_error))),
            }
            deciding.set(false);
        });
    };

    let context = match (&request.case_name, &request.case_id) {
        (Some(case_name), Some(case_id)) => Some(format!("Case: {case_name} ({case_id})")),
        (Some(case_name), None) => Some(format!("Case: {case_name}")),
        (None, Some(case_id)) => Some(format!("Case ID: {case_id}")),
        (None, None) => None,
    };
    let decided = match (&request.decided_by_name, &request.decided_at) {
        (Some(name), Some(at)) => Some(format!("Decided by {name} on {at}")),
        _ => None,
    };
    let status_class = format!(
        "inline-flex rounded-full px-2 py-0.5 text-xs font-medium {}",
        status_classes(request.status)
    );
    let status_label = request.status.label();
    let kind_label = request.kind.label();
    let change_summary = request.change_summary();
    let requested_by_id = request.requested_by_id;
    let requested_by_name = request.requested_by_name;
    let target_user_id = request.target_user_id;
    let target_user_name = request.target_user_name;
    let case_id = request.case_id;
    let case_name = request.case_name;
    let created_at = request.created_at;
    let request_note = request.request_note;
    let has_request_note = !request_note.is_empty();
    let decision_note_text = request.decision_note;
    let has_decision_note = !decision_note_text.is_empty();
    let target_profile_href = format!("/profile/{target_user_id}");
    let requester_profile_href = format!("/profile/{requested_by_id}");
    let case_href = case_id.as_ref().map(|id| format!("/admin/cases/{id}"));
    let case_link_label = match (&case_name, &case_id) {
        (Some(case_name), _) => format!("View case: {case_name}"),
        (None, Some(case_id)) => format!("View case: {case_id}"),
        (None, None) => "View case".to_string(),
    };
    let show_requester_profile = requested_by_id != target_user_id;
    let link_class = "inline-flex items-center rounded text-xs font-medium text-primary-300 underline decoration-dotted underline-offset-2 hover:text-primary-200 hover:decoration-solid";

    view! {
        <article class="rounded-card border border-slate-800 bg-slate-900 shadow-card p-4">
            <div class="flex flex-wrap items-start justify-between gap-3">
                <div class="min-w-0">
                    <div class="flex flex-wrap items-center gap-2">
                        <span class=status_class>{status_label}</span>
                        <span class="text-xs font-medium text-slate-400">{kind_label}</span>
                    </div>
                    <p class="mt-2 text-sm text-slate-200">
                        <span class="font-semibold">{requested_by_name.clone()}</span>
                        " requested a change for "
                        <span class="font-semibold">{target_user_name.clone()}</span>
                        "."
                    </p>
                    <p class="mt-1 text-sm text-slate-400">{change_summary}</p>
                    {context.map(|context| view! { <p class="mt-1 text-xs text-slate-500">{context}</p> })}
                </div>
                <time class="shrink-0 text-xs text-slate-500">{created_at}</time>
            </div>
            <div class="mt-3 flex flex-wrap gap-3">
                {case_href.clone().map(|href| {
                    view! {
                        <A href=href attr:class=link_class>
                            {case_link_label.clone()}
                        </A>
                    }
                })}
                <A href=target_profile_href attr:class=link_class>
                    "View target profile: " {target_user_name.clone()}
                </A>
                {show_requester_profile.then(|| {
                    view! {
                        <A href=requester_profile_href attr:class=link_class>
                            "View requester profile: " {requested_by_name.clone()}
                        </A>
                    }
                })}
            </div>
            <Show when=move || has_request_note>
                <div class="mt-3 border-l-2 border-slate-700 pl-3 text-xs text-slate-400">
                    <p class="font-medium text-slate-300">"Request note"</p>
                    <p class="mt-1">{request_note.clone()}</p>
                </div>
            </Show>
            {decided.map(|decided| view! { <p class="mt-3 text-xs text-slate-500">{decided}</p> })}
            <Show when=move || has_decision_note>
                <p class="mt-1 text-xs text-slate-400">
                    <span class="font-medium text-slate-300">"Decision note: "</span>
                    {decision_note_text.clone()}
                </p>
            </Show>
            <Show when=move || actionable>
                <div class="mt-4 flex flex-col gap-3">
                    <div class="space-y-1">
                        <label for=decision_note_id.clone() class="text-xs font-medium text-slate-300">
                            "Decision note"
                        </label>
                        <input
                            id=decision_note_id.clone()
                            class="min-w-0 w-full rounded-xl border border-slate-700 bg-slate-900 px-3 py-2 text-sm text-slate-100 placeholder:text-slate-500"
                            placeholder="Optional context for the requester"
                            maxlength="1000"
                            prop:disabled=move || deciding.get()
                            prop:value=move || decision_note.get()
                            on:input=move |event| decision_note.set(event_target_value(&event))
                        />
                    </div>
                    <div class="flex flex-col gap-2 sm:flex-row sm:items-center">
                        <button
                            type="button"
                            on:click=move |_| decide(true)
                            prop:disabled=move || deciding.get()
                            class="rounded-full bg-emerald-600 px-3 py-2 text-sm font-semibold text-white hover:bg-emerald-500 disabled:opacity-50"
                        >
                            "Approve"
                        </button>
                        <button
                            type="button"
                            on:click=move |_| decide(false)
                            prop:disabled=move || deciding.get()
                            class="rounded-full border border-rose-500/40 px-3 py-2 text-sm font-semibold text-rose-300 hover:bg-rose-500/10 disabled:opacity-50"
                        >
                            "Deny"
                        </button>
                    </div>
                </div>
            </Show>
            <Show when=move || error.get().is_some()>
                <p class="mt-2 text-xs text-rose-300" role="alert">
                    {move || error.get().unwrap_or_default()}
                </p>
            </Show>
        </article>
    }
}
