//! Admin dashboard tab: browse users (server-side paginated + searchable) and
//! manage their global role and per-case capabilities.

use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::components::admin_user_card::UserCard;
use crate::components::email_failures::EmailFailureLog;
use crate::server_fns::err_text;
use crate::server_fns::users::User;
use crate::state::AppState;

/// How many users the admin list loads per "page" (each "Load more" click grows
/// the visible window by this much).
const PAGE_SIZE: i64 = 4;

#[component]
pub fn CaseAccessTab(
    /// Whether this tab is the one on screen; the fetch is deferred until it is.
    #[prop(into)]
    active: Signal<bool>,
    is_site_admin: bool,
    actor_user_id: String,
    reload: RwSignal<u32>,
) -> impl IntoView {
    let state = expect_context::<AppState>();
    let actor_user_id = StoredValue::new(actor_user_id);
    // Search text (bound to the input for instant feedback) and its debounced
    // mirror (drives the actual fetch, so we don't hit the server on every
    // keystroke). The fetched window of users plus the total match count.
    let query = RwSignal::new(String::new());
    let debounced_query = RwSignal::new(String::new());
    let results = RwSignal::new(Vec::<User>::new());
    let total = RwSignal::new(0i64);
    // How many rows the current window requests; grows on "Load more".
    let window = RwSignal::new(PAGE_SIZE);
    let loading = RwSignal::new(false);
    let load_error = RwSignal::new(None::<String>);
    // Whether the collapsible "Email delivery failures" panel is open. Mounting
    // the viewer only on open defers its fetch until the admin asks for it.
    let failures_open = RwSignal::new(false);

    // (Re)load the window whenever the debounced query, window size, or reload
    // tick changes — but only once a session is confirmed (server functions run
    // in the browser after hydration). We always fetch `[0, window)` so both
    // search changes and post-mutation refreshes are handled by one code path.
    Effect::new(move |_| {
        if !active.get() {
            return;
        }
        let count = window.get();
        let q = debounced_query.get();
        reload.track();
        if !state.has_operations_admin_permissions() {
            return;
        }
        loading.set(true);
        spawn_local(async move {
            match crate::server_fns::users::list_users_page(0, count, q).await {
                Ok(page) => {
                    results.set(page.items);
                    total.set(page.total);
                    load_error.set(None);
                }
                Err(e) => load_error.set(Some(err_text(e))),
            }
            loading.set(false);
        });
    });

    let list = move || {
        if let Some(msg) = load_error.get() {
            return view! {
                <p class="text-sm text-rose-300">"Could not load users: " {msg}</p>
            }
            .into_any();
        }
        let items = results.get();
        if items.is_empty() {
            let text = if loading.get() {
                "Loading\u{2026}"
            } else {
                "No users match your search."
            };
            return view! { <p class="text-sm text-slate-500">{text}</p> }.into_any();
        }
        items
            .into_iter()
            .map(|u| {
                view! {
                    <UserCard
                        user=u
                        reload=reload
                        actor_user_id=actor_user_id.get_value()
                        is_site_admin=is_site_admin
                    />
                }
                .into_any()
            })
            .collect_view()
            .into_any()
    };

    let footer = move || {
        let shown = results.get().len() as i64;
        let tot = total.get();
        if tot == 0 {
            return ().into_any();
        }
        let more = shown < tot;
        view! {
            <div class="mt-4 flex items-center justify-between">
                <p class="text-xs text-slate-500">"Showing " {shown} " of " {tot}</p>
                <Show when=move || more>
                    <button
                        on:click=move |_| window.update(|w| *w += PAGE_SIZE)
                        prop:disabled=move || loading.get()
                        class="rounded-full border border-slate-700 px-3 py-1.5 text-sm font-medium text-slate-200 hover:bg-slate-800 disabled:opacity-50"
                    >
                        {move || if loading.get() { "Loading\u{2026}" } else { "Load more" }}
                    </button>
                </Show>
            </div>
        }
        .into_any()
    };

    // Debounce the search: update the visible input immediately, but wait 1s of
    // idle typing before firing the fetch (and resetting the window).
    let mut on_search = debounce(std::time::Duration::from_secs(1), move |val: String| {
        window.set(PAGE_SIZE);
        debounced_query.set(val);
    });

    view! {
        <p class="mb-6 text-sm text-slate-400">
            {if is_site_admin {
                "Manage global roles and case capabilities."
            } else {
                "Manage your case access and request changes for other users."
            }}
        </p>
        <input
            class="mb-4 w-full rounded-xl border border-slate-700 bg-slate-900 px-3 py-2 text-sm text-slate-100 placeholder:text-slate-500 focus:border-primary-500 focus:outline-none focus:ring-2 focus:ring-primary-500/40"
            placeholder="Search users by name or email"
            prop:value=move || query.get()
            on:input=move |ev| {
                let val = event_target_value(&ev);
                query.set(val.clone());
                on_search(val);
            }
        />
        <div class="space-y-4">{list}</div>
        {footer}
        <div class="mb-6 rounded-card border border-slate-800 bg-slate-900 shadow-card p-5">
            <div class="flex items-center justify-between">
                <div>
                    <h2 class="text-sm font-semibold text-slate-200">
                        "Email delivery failures"
                    </h2>
                    <p class="mt-0.5 text-xs text-slate-500">
                        "Outbound emails that failed to send, newest first \u{2014} check here instead of the server logs."
                    </p>
                </div>
                <button
                    on:click=move |_| failures_open.update(|o| *o = !*o)
                    class="rounded-full border border-slate-700 px-2 py-1 text-xs font-medium text-slate-300 hover:bg-slate-800"
                >
                    {move || if failures_open.get() { "Hide" } else { "Show" }}
                </button>
            </div>
            <Show when=move || failures_open.get()>
                <EmailFailureLog />
            </Show>
        </div>
    }
}
