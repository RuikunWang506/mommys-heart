//! Focused case-management workspace for administrators.

use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::components::A;
use leptos_router::hooks::use_query_map;

use crate::components::admin_cases::CaseRequests;
use crate::components::admin_requests::AdminRequestCenter;
use crate::components::case_tabs::CaseTab;
use crate::components::loading::Loading;
use crate::pages::cases::CaseDetail;
use crate::server_fns::admin_requests::AdminRequestKind;
use crate::server_fns::capabilities::CaseCapability;
use crate::server_fns::cases::{admin_case_summary, admin_list_cases_page, CaseSummary};
use crate::server_fns::err_text;
use crate::state::AppState;

const PAGE_SIZE: i64 = 10;
const MAX_WINDOW: i64 = 1_000;

fn parse_window(value: Option<String>) -> i64 {
    value
        .and_then(|value| value.parse::<i64>().ok())
        .map(|value| value.clamp(1, MAX_WINDOW))
        .unwrap_or(PAGE_SIZE)
}

fn encode_query_value(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(char::from(byte));
            }
            b' ' => encoded.push_str("%20"),
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}

fn directory_query_suffix(search: &str, window: i64) -> String {
    if search.trim().is_empty() && window == PAGE_SIZE {
        String::new()
    } else {
        format!("?q={}&w={window}", encode_query_value(search.trim()))
    }
}

fn badge(classes: &str) -> String {
    format!("inline-flex items-center rounded-full px-2 py-0.5 text-xs font-medium {classes}")
}

fn access_label(caps: &[CaseCapability]) -> (&'static str, &'static str) {
    if caps.is_empty() {
        (
            "No access",
            "bg-slate-700/40 text-slate-300 ring-1 ring-slate-600",
        )
    } else if caps.len() == CaseCapability::ALL.len() {
        (
            "Full access",
            "bg-primary-500/15 text-primary-300 ring-1 ring-primary-500/30",
        )
    } else if caps.contains(&CaseCapability::EditCase) {
        (
            "Manager",
            "bg-primary-500/15 text-primary-300 ring-1 ring-primary-500/30",
        )
    } else if caps.contains(&CaseCapability::UploadEvidence)
        || caps.contains(&CaseCapability::AddNotes)
        || caps.contains(&CaseCapability::SendMessages)
    {
        (
            "Contributor",
            "bg-sky-500/15 text-sky-300 ring-1 ring-sky-500/30",
        )
    } else {
        (
            "Viewer",
            "bg-slate-500/15 text-slate-300 ring-1 ring-slate-500/30",
        )
    }
}

#[component]
pub fn ManageCases(
    is_site_admin: bool,
    reload: RwSignal<u32>,
    selected_case_id: Option<String>,
) -> impl IntoView {
    let state = expect_context::<AppState>();
    let selected_case_id = StoredValue::new(selected_case_id);
    let query_map = use_query_map();
    let initial_query = query_map.get_untracked();
    let initial_search = initial_query.get("q").unwrap_or_default();
    let initial_window = parse_window(initial_query.get("w"));
    drop(initial_query);
    let query = RwSignal::new(initial_search.clone());
    let debounced_query = RwSignal::new(initial_search);
    let results = RwSignal::new(Vec::<CaseSummary>::new());
    let total = RwSignal::new(0i64);
    let window = RwSignal::new(initial_window);
    let loading = RwSignal::new(false);
    let load_error = RwSignal::new(None::<String>);
    let request_generation = RwSignal::new(0u64);

    Effect::new(move |_| {
        let count = window.get();
        let q = debounced_query.get();
        reload.track();
        if !state.has_operations_admin_permissions() || selected_case_id.get_value().is_some() {
            return;
        }
        loading.set(true);
        request_generation.update(|generation| *generation += 1);
        let generation = request_generation.get_untracked();
        spawn_local(async move {
            let response = admin_list_cases_page(0, count, q).await;
            if request_generation.get_untracked() != generation {
                return;
            }
            match response {
                Ok(page) => {
                    results.set(page.items);
                    total.set(page.total);
                    load_error.set(None);
                }
                Err(error) => load_error.set(Some(err_text(error))),
            }
            loading.set(false);
        });
    });

    let detail_summary = RwSignal::new(None::<CaseSummary>);
    let detail_loading = RwSignal::new(selected_case_id.get_value().is_some());
    let detail_error = RwSignal::new(None::<String>);
    let open_folder = RwSignal::new(String::new());
    let tab = RwSignal::new(CaseTab::Overview);
    let detail_generation = RwSignal::new(0u64);

    Effect::new(move |_| {
        let Some(case_id) = selected_case_id.get_value() else {
            return;
        };
        reload.track();
        if !state.has_operations_admin_permissions() {
            return;
        }
        detail_loading.set(true);
        detail_error.set(None);
        detail_generation.update(|generation| *generation += 1);
        let generation = detail_generation.get_untracked();
        spawn_local(async move {
            let response = admin_case_summary(case_id).await;
            if detail_generation.get_untracked() != generation {
                return;
            }
            match response {
                Ok(summary) => {
                    if summary.is_none() {
                        detail_error.set(Some("Case not found.".to_string()));
                    }
                    detail_summary.set(summary);
                }
                Err(error) => detail_error.set(Some(err_text(error))),
            }
            detail_loading.set(false);
        });
    });

    let current_query = query_map.get_untracked();
    let back_suffix = directory_query_suffix(
        &current_query.get("q").unwrap_or_default(),
        parse_window(current_query.get("w")),
    );
    let back_href = StoredValue::new(format!("/admin/cases{back_suffix}"));
    // The admin's own user page, where an operations admin can grant themselves
    // capabilities on a case they cannot currently open.
    let access_href = StoredValue::new(
        state
            .current_user_summary
            .get_untracked()
            .map(|user| format!("/admin/users/{}", user.id))
            .unwrap_or_else(|| "/admin/users".to_string()),
    );

    let detail_view = move || {
        if let Some(message) = detail_error.get() {
            return view! {
                <div class="space-y-4">
                    <BackToCases href=back_href.get_value() />
                    <div class="rounded-xl border border-rose-500/30 bg-rose-500/10 p-4 text-sm text-rose-200">
                        {message}
                    </div>
                </div>
            }
            .into_any();
        }
        if detail_loading.get() {
            return view! {
                <div class="space-y-4">
                    <BackToCases href=back_href.get_value() />
                    <div class="rounded-card border border-slate-800 bg-slate-900 shadow-card p-4">
                        <Loading label="Loading case details…" />
                    </div>
                </div>
            }
            .into_any();
        }
        match detail_summary.get() {
            // Without `ViewCase` the case body would load nothing, so say why
            // and point at the page where an admin can grant themselves access.
            Some(summary) if !summary.capabilities.contains(&CaseCapability::ViewCase) => {
                view! {
                    <div class="space-y-4">
                        <BackToCases href=back_href.get_value() />
                        <div class="rounded-card border border-slate-800 bg-slate-900 shadow-card p-4">
                            <h2 class="text-lg font-semibold text-slate-100">{summary.name.clone()}</h2>
                            <p class="mt-2 text-sm text-slate-400">
                                "You do not have access to this case. An operations admin can give themselves access from the user directory."
                            </p>
                            <A
                                href=access_href.get_value()
                                attr:class="mt-3 inline-flex items-center rounded-full border border-slate-700 px-3 py-1.5 text-sm font-medium text-slate-200 hover:bg-slate-800"
                            >
                                "Manage my case access"
                            </A>
                        </div>
                    </div>
                }
                .into_any()
            }
            Some(summary) => view! {
                <div class="space-y-4">
                    <BackToCases href=back_href.get_value() />
                    <CaseDetail
                        summary=summary
                        reload=reload
                        open_folder=open_folder
                        tab=tab
                    />
                </div>
            }
            .into_any(),
            None => ().into_any(),
        }
    };

    let query_suffix = Signal::derive(move || directory_query_suffix(&query.get(), window.get()));

    let directory_rows = move || {
        if let Some(message) = load_error.get() {
            return view! {
                <p class="text-sm text-rose-300" role="alert">
                    "Could not load cases: " {message}
                </p>
            }
            .into_any();
        }
        let items = results.get();
        if items.is_empty() {
            let text = if loading.get() {
                "Loading…"
            } else if query.get().trim().is_empty() {
                "No cases found."
            } else {
                "No cases match your search."
            };
            return view! { <p class="text-sm text-slate-500" aria-live="polite">{text}</p> }
                .into_any();
        }

        items
            .into_iter()
            .map(|case| {
                let name = case.name.clone();
                let id = case.id.clone();
                let owner = case.owner_full_name();
                let status = case.status;
                let (access_text, access_classes) = access_label(&case.capabilities);
                let href = format!("/admin/cases/{id}{}", query_suffix.get());
                view! {
                    <article class="rounded-card border border-slate-800 bg-slate-900 shadow-card p-4">
                        <div class="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
                            <div class="min-w-0">
                                <div class="flex flex-wrap items-center gap-2">
                                    <A
                                        href=href.clone()
                                        attr:class="font-semibold text-primary-300 hover:text-primary-200"
                                    >
                                        {name}
                                    </A>
                                    <span class=badge(status.badge_classes())>{status.label()}</span>
                                    <span class=badge(access_classes)>{access_text}</span>
                                    {case.inactive.then(|| view! {
                                        <span class=badge("bg-amber-500/15 text-amber-300 ring-1 ring-amber-500/30")>
                                            "Inactive 30+ days"
                                        </span>
                                    })}
                                </div>
                                <p class="mt-2 text-sm text-slate-400">"Owner: " {owner}</p>
                                <p class="mt-1 font-mono text-xs text-slate-500">{id}</p>
                            </div>
                            <A
                                href=href
                                attr:class="shrink-0 rounded-full border border-slate-700 px-3 py-1.5 text-center text-sm font-medium text-slate-200 hover:bg-slate-800"
                            >
                                "View details"
                            </A>
                        </div>
                    </article>
                }
            })
            .collect_view()
            .into_any()
    };

    let footer = move || {
        let shown = results.get().len() as i64;
        let all = total.get();
        if all == 0 {
            return ().into_any();
        }
        view! {
            <div class="mt-4 flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
                <p class="text-xs text-slate-500">"Showing " {shown} " of " {all}</p>
                <Show when=move || (shown < all) && (window.get() < MAX_WINDOW)>
                    <button
                        type="button"
                        on:click=move |_| {
                            window.update(|value| *value = (*value + PAGE_SIZE).min(MAX_WINDOW))
                        }
                        prop:disabled=move || loading.get()
                        class="rounded-full border border-slate-700 px-3 py-1.5 text-sm font-medium text-slate-200 hover:bg-slate-800 disabled:opacity-50"
                    >
                        {move || if loading.get() { "Loading…" } else { "Load more" }}
                    </button>
                </Show>
                <Show when=move || (shown < all) && (window.get() >= MAX_WINDOW)>
                    <p class="text-xs text-slate-500">"Refine your search to narrow the results."</p>
                </Show>
            </div>
        }
        .into_any()
    };

    let mut on_search = debounce(
        std::time::Duration::from_millis(500),
        move |value: String| {
            window.set(PAGE_SIZE);
            debounced_query.set(value);
        },
    );

    if selected_case_id.get_value().is_some() {
        return view! { <div>{detail_view}</div> }.into_any();
    }

    view! {
        <div class="space-y-10">
            <AdminRequestCenter
                kind=AdminRequestKind::CaseCapabilities
                is_site_admin=is_site_admin
                reload=reload
            />

            <section class="space-y-4 border-t border-slate-800 pt-8">
                <div>
                    <h2 class="text-base font-semibold text-slate-100">"Pending case requests"</h2>
                    <p class="mt-1 text-sm text-slate-400">
                        "Cases clients submitted that still need an accept or decline decision."
                    </p>
                </div>
                <CaseRequests reload=reload />
            </section>

            <section class="space-y-4 border-t border-slate-800 pt-8">
                <div>
                    <h2 class="text-base font-semibold text-slate-100">"All cases"</h2>
                    <p class="mt-1 text-sm text-slate-400">
                        "Every case in the system. Site admins can open and change any of them; other admins need case permissions."
                    </p>
                </div>
                <div>
                    <label for="admin-case-search" class="block text-sm font-medium text-slate-200">
                        "Search all cases"
                    </label>
                    <input
                        id="admin-case-search"
                        type="search"
                        class="mt-2 w-full rounded-xl border border-slate-700 bg-slate-900 px-3 py-2 text-sm text-slate-100 placeholder:text-slate-500 focus:border-primary-500 focus:outline-none focus:ring-2 focus:ring-primary-500/40"
                        placeholder="Case name, ID, or owner name"
                        prop:value=move || query.get()
                        on:input=move |event| {
                            let value = event_target_value(&event);
                            query.set(value.clone());
                            on_search(value);
                        }
                    />
                </div>
                <div class="space-y-3">{directory_rows}</div>
                {footer}
            </section>
        </div>
    }
    .into_any()
}

#[component]
fn BackToCases(#[prop(into)] href: String) -> impl IntoView {
    view! {
        <A
            href=href
            attr:class="inline-flex items-center gap-1.5 rounded-full border border-slate-700 px-3 py-1.5 text-sm font-medium text-slate-200 hover:bg-slate-800"
        >
            "← Back to cases"
        </A>
    }
}
