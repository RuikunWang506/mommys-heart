//! Focused user-management workspace for administrators.

use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::components::A;
use leptos_router::hooks::use_query_map;

use crate::components::admin_requests::AdminRequestCenter;
use crate::components::admin_user_card::UserCard;
use crate::components::admin_volunteers::PendingApplications;
use crate::components::loading::Loading;
use crate::server_fns::admin_requests::AdminRequestKind;
use crate::server_fns::err_text;
use crate::server_fns::users::{
    list_user_directory_page, load_admin_user, User, UserDirectoryItem, UserDirectoryRoleGroup,
};
use crate::state::AppState;

const PAGE_SIZE: i64 = 10;
const MAX_WINDOW: i64 = 1_000;

fn badge(classes: &str) -> String {
    format!("inline-flex items-center rounded-full px-2 py-0.5 text-xs font-medium {classes}")
}

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
            _ => encoded.push_str(&format!("%{:02X}", byte)),
        }
    }
    encoded
}

fn directory_query_suffix(
    search: &str,
    volunteers: i64,
    clients: i64,
    other: i64,
    deactivated: i64,
) -> String {
    if search.trim().is_empty()
        && volunteers == PAGE_SIZE
        && clients == PAGE_SIZE
        && other == PAGE_SIZE
        && deactivated == PAGE_SIZE
    {
        return String::new();
    }
    format!(
        "?q={}&vw={volunteers}&cw={clients}&ow={other}&dw={deactivated}",
        encode_query_value(search.trim())
    )
}

#[component]
pub fn ManageUsers(
    is_site_admin: bool,
    actor_user_id: String,
    reload: RwSignal<u32>,
    selected_user_id: Option<String>,
) -> impl IntoView {
    let state = expect_context::<AppState>();
    let actor_user_id = StoredValue::new(actor_user_id);
    let query_map = use_query_map();

    if let Some(selected_user_id) = selected_user_id {
        let selected_user_id = StoredValue::new(selected_user_id);
        let selected_user = RwSignal::new(None::<User>);
        let load_error = RwSignal::new(None::<String>);
        let loading = RwSignal::new(true);

        Effect::new(move |_| {
            reload.track();
            if !state.has_operations_admin_permissions() {
                return;
            }
            let user_id = selected_user_id.get_value();
            if user_id.trim().is_empty() {
                load_error.set(Some("User not found.".to_string()));
                loading.set(false);
                return;
            }
            loading.set(true);
            load_error.set(None);
            spawn_local(async move {
                match load_admin_user(user_id).await {
                    Ok(user) => {
                        selected_user.set(Some(user));
                        load_error.set(None);
                    }
                    Err(error) => load_error.set(Some(err_text(error))),
                }
                loading.set(false);
            });
        });

        let current_query = query_map.get_untracked();
        let back_suffix = directory_query_suffix(
            &current_query.get("q").unwrap_or_default(),
            parse_window(current_query.get("vw")),
            parse_window(current_query.get("cw")),
            parse_window(current_query.get("ow")),
            parse_window(current_query.get("dw")),
        );
        let back_href = format!("/admin/users{back_suffix}");
        let profile_href = format!("/profile/{}", selected_user_id.get_value());

        let detail = move || {
            if let Some(message) = load_error.get() {
                return view! {
                    <div class="rounded-xl border border-rose-500/30 bg-rose-500/10 p-4 text-sm text-rose-200" role="alert">
                        {message}
                    </div>
                }
                .into_any();
            }
            if loading.get() {
                return view! {
                    <div class="rounded-card border border-slate-800 bg-slate-900 shadow-card p-4">
                        <Loading label="Loading user details…" />
                    </div>
                }
                .into_any();
            }
            match selected_user.get() {
                Some(user) => view! {
                    <UserCard
                        user=user
                        reload=reload
                        actor_user_id=actor_user_id.get_value()
                        is_site_admin=is_site_admin
                    />
                }
                .into_any(),
                None => ().into_any(),
            }
        };

        return view! {
            <div class="space-y-4">
                <div class="flex flex-wrap gap-2">
                    <A
                        href=back_href
                        attr:class="inline-flex items-center gap-1.5 rounded-full border border-slate-700 px-3 py-1.5 text-sm font-medium text-slate-200 hover:bg-slate-800"
                    >
                        "← Back to users"
                    </A>
                    <A
                        href=profile_href
                        attr:class="inline-flex items-center rounded-full border border-slate-700 px-3 py-1.5 text-sm font-medium text-slate-200 hover:bg-slate-800"
                    >
                        "View profile"
                    </A>
                </div>
                {detail}
            </div>
        }
        .into_any();
    }

    let initial_query = query_map.get_untracked();
    let initial_search = initial_query.get("q").unwrap_or_default();
    let initial_volunteer_window = parse_window(initial_query.get("vw"));
    let initial_client_window = parse_window(initial_query.get("cw"));
    let initial_other_window = parse_window(initial_query.get("ow"));
    let initial_deactivated_window = parse_window(initial_query.get("dw"));
    drop(initial_query);

    let query = RwSignal::new(initial_search.clone());
    let debounced_query = RwSignal::new(initial_search);

    let volunteers = RwSignal::new(Vec::<UserDirectoryItem>::new());
    let volunteer_total = RwSignal::new(0i64);
    let volunteer_window = RwSignal::new(initial_volunteer_window);
    let volunteer_loading = RwSignal::new(false);
    let volunteer_error = RwSignal::new(None::<String>);
    let volunteer_generation = RwSignal::new(0u64);

    let clients = RwSignal::new(Vec::<UserDirectoryItem>::new());
    let client_total = RwSignal::new(0i64);
    let client_window = RwSignal::new(initial_client_window);
    let client_loading = RwSignal::new(false);
    let client_error = RwSignal::new(None::<String>);
    let client_generation = RwSignal::new(0u64);

    let others = RwSignal::new(Vec::<UserDirectoryItem>::new());
    let other_total = RwSignal::new(0i64);
    let other_window = RwSignal::new(initial_other_window);
    let other_loading = RwSignal::new(false);
    let other_error = RwSignal::new(None::<String>);
    let other_generation = RwSignal::new(0u64);

    let deactivated = RwSignal::new(Vec::<UserDirectoryItem>::new());
    let deactivated_total = RwSignal::new(0i64);
    let deactivated_window = RwSignal::new(initial_deactivated_window);
    let deactivated_loading = RwSignal::new(false);
    let deactivated_error = RwSignal::new(None::<String>);
    let deactivated_generation = RwSignal::new(0u64);

    Effect::new(move |_| {
        let count = volunteer_window.get();
        let search = debounced_query.get();
        reload.track();
        if !state.has_operations_admin_permissions() {
            return;
        }
        volunteer_loading.set(true);
        volunteer_generation.update(|generation| *generation += 1);
        let generation = volunteer_generation.get_untracked();
        spawn_local(async move {
            let response =
                list_user_directory_page(0, count, search, UserDirectoryRoleGroup::Volunteer).await;
            if volunteer_generation.get_untracked() != generation {
                return;
            }
            match response {
                Ok(page) => {
                    volunteers.set(page.items);
                    volunteer_total.set(page.total);
                    volunteer_error.set(None);
                }
                Err(error) => volunteer_error.set(Some(err_text(error))),
            }
            volunteer_loading.set(false);
        });
    });

    Effect::new(move |_| {
        let count = client_window.get();
        let search = debounced_query.get();
        reload.track();
        if !state.has_operations_admin_permissions() {
            return;
        }
        client_loading.set(true);
        client_generation.update(|generation| *generation += 1);
        let generation = client_generation.get_untracked();
        spawn_local(async move {
            let response =
                list_user_directory_page(0, count, search, UserDirectoryRoleGroup::Client).await;
            if client_generation.get_untracked() != generation {
                return;
            }
            match response {
                Ok(page) => {
                    clients.set(page.items);
                    client_total.set(page.total);
                    client_error.set(None);
                }
                Err(error) => client_error.set(Some(err_text(error))),
            }
            client_loading.set(false);
        });
    });

    Effect::new(move |_| {
        let count = other_window.get();
        let search = debounced_query.get();
        reload.track();
        if !state.has_operations_admin_permissions() {
            return;
        }
        other_loading.set(true);
        other_generation.update(|generation| *generation += 1);
        let generation = other_generation.get_untracked();
        spawn_local(async move {
            let response =
                list_user_directory_page(0, count, search, UserDirectoryRoleGroup::Other).await;
            if other_generation.get_untracked() != generation {
                return;
            }
            match response {
                Ok(page) => {
                    others.set(page.items);
                    other_total.set(page.total);
                    other_error.set(None);
                }
                Err(error) => other_error.set(Some(err_text(error))),
            }
            other_loading.set(false);
        });
    });

    Effect::new(move |_| {
        let count = deactivated_window.get();
        let search = debounced_query.get();
        reload.track();
        if !state.has_operations_admin_permissions() {
            return;
        }
        deactivated_loading.set(true);
        deactivated_generation.update(|generation| *generation += 1);
        let generation = deactivated_generation.get_untracked();
        spawn_local(async move {
            let response =
                list_user_directory_page(0, count, search, UserDirectoryRoleGroup::Deactivated)
                    .await;
            if deactivated_generation.get_untracked() != generation {
                return;
            }
            match response {
                Ok(page) => {
                    deactivated.set(page.items);
                    deactivated_total.set(page.total);
                    deactivated_error.set(None);
                }
                Err(error) => deactivated_error.set(Some(err_text(error))),
            }
            deactivated_loading.set(false);
        });
    });

    let query_suffix = Signal::derive(move || {
        directory_query_suffix(
            &query.get(),
            volunteer_window.get(),
            client_window.get(),
            other_window.get(),
            deactivated_window.get(),
        )
    });
    let search_empty = Signal::derive(move || query.get().trim().is_empty());

    let mut on_search = debounce(
        std::time::Duration::from_millis(500),
        move |value: String| {
            volunteer_window.set(PAGE_SIZE);
            client_window.set(PAGE_SIZE);
            other_window.set(PAGE_SIZE);
            deactivated_window.set(PAGE_SIZE);
            debounced_query.set(value);
        },
    );

    view! {
        <div class="space-y-10">
            <AdminRequestCenter
                kind=AdminRequestKind::InformationAccess
                is_site_admin=is_site_admin
                reload=reload
            />

            <AdminRequestCenter
                kind=AdminRequestKind::Role
                is_site_admin=is_site_admin
                reload=reload
            />

            <PendingApplications
                active=Signal::derive(|| true)
                reload=reload
                is_site_admin=is_site_admin
                show_empty=true
            />

            <section class="space-y-4 border-t border-slate-800 pt-8">
                <div>
                    <h2 class="text-base font-semibold text-slate-100">"Manage users"</h2>
                    <p class="mt-1 text-sm text-slate-400">
                        "Search once, then browse exact-role sections for volunteers, clients, and administrators."
                    </p>
                </div>
                <div>
                    <label for="admin-user-search" class="block text-sm font-medium text-slate-200">
                        "Search users"
                    </label>
                    <input
                        id="admin-user-search"
                        type="search"
                        class="mt-2 w-full rounded-xl border border-slate-700 bg-slate-900 px-3 py-2 text-sm text-slate-100 placeholder:text-slate-500 focus:border-primary-500 focus:outline-none focus:ring-2 focus:ring-primary-500/40"
                        placeholder="Name, email, or user ID"
                        prop:value=move || query.get()
                        on:input=move |event| {
                            let value = event_target_value(&event);
                            query.set(value.clone());
                            on_search(value);
                        }
                    />
                </div>
            </section>

            <div class="grid gap-6 xl:grid-cols-3 xl:items-start">
                <UserDirectorySection
                    title="Volunteers"
                    subtitle="Exact Volunteer accounts, including volunteer-agreement status and case-access summaries."
                    empty_text="No volunteer accounts found."
                    empty_search_text="No volunteer accounts match your search."
                    items=volunteers
                    total=volunteer_total
                    window=volunteer_window
                    loading=volunteer_loading
                    load_error=volunteer_error
                    search_empty=search_empty
                    query_suffix=query_suffix
                    actor_user_id=actor_user_id.get_value()
                />
                <UserDirectorySection
                    title="Clients"
                    subtitle="Exact Client accounts, separate from volunteers and administrators."
                    empty_text="No client accounts found."
                    empty_search_text="No client accounts match your search."
                    items=clients
                    total=client_total
                    window=client_window
                    loading=client_loading
                    load_error=client_error
                    search_empty=search_empty
                    query_suffix=query_suffix
                    actor_user_id=actor_user_id.get_value()
                />
                <UserDirectorySection
                    title="Other"
                    subtitle="Operations admins and site admins. Exact-role grouping keeps admin accounts out of Volunteers."
                    empty_text="No operations-admin or site-admin accounts found."
                    empty_search_text="No operations-admin or site-admin accounts match your search."
                    items=others
                    total=other_total
                    window=other_window
                    loading=other_loading
                    load_error=other_error
                    search_empty=search_empty
                    query_suffix=query_suffix
                    actor_user_id=actor_user_id.get_value()
                />
            </div>

            // Full width and below the three working sections: a retired
            // duplicate leaves those lists but stays searchable and reversible.
            <UserDirectorySection
                title="Deactivated accounts"
                subtitle="Retired accounts. They cannot sign in and are hidden from the sections above, the volunteer list, and every picker. Open one to reactivate it."
                empty_text="No deactivated accounts."
                empty_search_text="No deactivated accounts match your search."
                items=deactivated
                total=deactivated_total
                window=deactivated_window
                loading=deactivated_loading
                load_error=deactivated_error
                search_empty=search_empty
                query_suffix=query_suffix
                actor_user_id=actor_user_id.get_value()
            />
        </div>
    }
    .into_any()
}

#[component]
fn UserDirectorySection(
    title: &'static str,
    subtitle: &'static str,
    empty_text: &'static str,
    empty_search_text: &'static str,
    items: RwSignal<Vec<UserDirectoryItem>>,
    total: RwSignal<i64>,
    window: RwSignal<i64>,
    loading: RwSignal<bool>,
    load_error: RwSignal<Option<String>>,
    search_empty: Signal<bool>,
    query_suffix: Signal<String>,
    actor_user_id: String,
) -> impl IntoView {
    let rows = move || {
        if let Some(message) = load_error.get() {
            return view! {
                <p class="text-sm text-rose-300" role="alert">
                    "Could not load " {title.to_lowercase()} ": " {message}
                </p>
            }
            .into_any();
        }

        let suffix = query_suffix.get();
        let directory_items = items.get();
        if directory_items.is_empty() {
            let text = if loading.get() {
                "Loading…"
            } else if search_empty.get() {
                empty_text
            } else {
                empty_search_text
            };
            return view! { <p class="text-sm text-slate-500" aria-live="polite">{text}</p> }
                .into_any();
        }

        directory_items
            .into_iter()
            .map(|user| {
                let name = user.full_name();
                let profile_href = format!("/profile/{}", user.id);
                let manage_href = format!("/admin/users/{}{}", user.id, suffix);
                let role_badge = badge(user.role.badge_classes());
                let agreement_badge = user.agreement.map(|agreement| {
                    let badge_class = badge(agreement.badge_classes());
                    view! {
                        <span class=badge_class>
                            "Volunteer agreement: " {agreement.label()}
                        </span>
                    }
                });
                let is_you = user.id == actor_user_id;
                view! {
                    <article class="rounded-lg border border-slate-800 bg-slate-950 p-4">
                        <div class="flex flex-col gap-3">
                            <div class="min-w-0">
                                <div class="flex flex-wrap items-center gap-2">
                                    <p class="font-semibold text-slate-100">{name}</p>
                                    <span class=role_badge>{user.role.label()}</span>
                                    {agreement_badge}
                                    {is_you.then(|| view! {
                                        <span class="inline-flex items-center rounded-full bg-slate-800 px-2 py-0.5 text-xs font-medium text-slate-300">
                                            "You"
                                        </span>
                                    })}
                                </div>
                                <p class="mt-1 break-all text-sm text-slate-400">{user.email}</p>
                                <p class="mt-2 text-xs text-slate-500">
                                    "Case access: " {user.assignment_summary}
                                </p>
                            </div>
                            <div class="flex flex-col gap-2 sm:flex-row sm:flex-wrap sm:items-center">
                                <A
                                    href=profile_href
                                    attr:class="inline-flex items-center justify-center rounded-full border border-slate-700 px-3 py-1.5 text-sm font-medium text-slate-200 hover:bg-slate-800"
                                >
                                    "View profile"
                                </A>
                                <A
                                    href=manage_href
                                    attr:class="inline-flex items-center justify-center rounded-full border border-primary-500/40 bg-primary-500/10 px-3 py-1.5 text-sm font-medium text-primary-300 hover:bg-primary-500/20"
                                >
                                    "Manage"
                                </A>
                            </div>
                        </div>
                    </article>
                }
            })
            .collect_view()
            .into_any()
    };

    let footer = move || {
        let shown = items.get().len() as i64;
        let all = total.get();
        if all == 0 {
            return ().into_any();
        }
        view! {
            <div class="flex flex-col gap-3 border-t border-slate-800 pt-4 sm:flex-row sm:items-center sm:justify-between">
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

    view! {
        <section class="rounded-card border border-slate-800 bg-slate-900 shadow-card p-5">
            <div class="mb-4 flex flex-wrap items-start justify-between gap-3">
                <div>
                    <div class="flex flex-wrap items-center gap-2">
                        <h2 class="text-base font-semibold text-slate-100">{title}</h2>
                        <span class="inline-flex min-w-5 items-center justify-center rounded-full bg-slate-800 px-1.5 py-0.5 text-[0.65rem] font-semibold leading-none text-slate-200">
                            {move || total.get()}
                        </span>
                    </div>
                    <p class="mt-1 text-sm text-slate-400">{subtitle}</p>
                </div>
            </div>
            <div class="space-y-3">{rows}</div>
            <div class="mt-4">{footer}</div>
        </section>
    }
}
