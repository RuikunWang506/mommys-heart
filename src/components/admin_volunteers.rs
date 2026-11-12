//! Admin dashboard tab: every volunteer account and whether they have completed
//! the volunteer agreement. Each name links to that user's profile.

use std::collections::HashMap;

use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::components::profile_link::ProfileLink;
use crate::server_fns::err_text;
use crate::server_fns::users::{list_volunteers_page, VolunteerListItem};
use crate::server_fns::volunteer_applicants::{
    decide_volunteer_applicant, list_volunteer_applicants, renew_volunteer_setup_link,
    withdraw_volunteer_applicant,
};
use crate::server_fns::volunteers::{
    decide_volunteer_application, list_pending_volunteer_applications,
};
use crate::state::AppState;

/// How many volunteers each "Load more" click adds to the visible window.
const PAGE_SIZE: i64 = 20;

#[component]
pub fn VolunteersTab(
    /// Whether this tab is the one on screen; the fetch is deferred until it is.
    #[prop(into)]
    active: Signal<bool>,
    /// Only site admins may decide an application, because approving grants the
    /// Volunteer role. Everyone else sees the queue read-only.
    is_site_admin: bool,
) -> impl IntoView {
    let state = expect_context::<AppState>();
    // Bumped after a decision so both the pending queue and the volunteer list
    // below it refetch.
    let reload = RwSignal::new(0u32);
    // Instant-feedback search text plus its debounced mirror, which drives the
    // fetch so we don't hit the server on every keystroke.
    let query = RwSignal::new(String::new());
    let debounced_query = RwSignal::new(String::new());
    let results = RwSignal::new(Vec::<VolunteerListItem>::new());
    let total = RwSignal::new(0i64);
    let window = RwSignal::new(PAGE_SIZE);
    let loading = RwSignal::new(false);
    let load_error = RwSignal::new(None::<String>);

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
            match list_volunteers_page(0, count, q).await {
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

    let rows = move || {
        if let Some(msg) = load_error.get() {
            return view! {
                <p class="text-sm text-rose-300">"Could not load volunteers: " {msg}</p>
            }
            .into_any();
        }
        let items = results.get();
        if items.is_empty() {
            let text = if loading.get() {
                "Loading\u{2026}"
            } else {
                "No volunteers match your search."
            };
            return view! { <p class="text-sm text-slate-500">{text}</p> }.into_any();
        }
        items
            .into_iter()
            .map(|v| {
                let name = v.full_name();
                let badge_class = format!(
                    "inline-flex items-center rounded-full px-2 py-0.5 text-xs font-medium {}",
                    v.agreement.badge_classes(),
                );
                view! {
                    <div class="flex flex-wrap items-center justify-between gap-3 border-b border-slate-800 px-4 py-3 last:border-b-0">
                        <div class="min-w-0">
                            <ProfileLink user_id=v.id name=name />
                            <p class="truncate text-xs text-slate-500">{v.email}</p>
                        </div>
                        <span class=badge_class>{v.agreement.label()}</span>
                    </div>
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

    // Wait 1s of idle typing before firing the fetch (and resetting the window).
    let mut on_search = debounce(std::time::Duration::from_secs(1), move |val: String| {
        window.set(PAGE_SIZE);
        debounced_query.set(val);
    });

    view! {
        <PendingApplications active=active reload=reload is_site_admin=is_site_admin />
        <p class="mb-4 text-sm text-slate-400">
            "Everyone with a volunteer account, and whether they have completed the volunteer agreement. Click a name to open their profile."
        </p>
        <div class="mb-4">
            <label for="admin-volunteer-search" class="block text-sm font-medium text-slate-200">
                "Search volunteers"
            </label>
            <input
                id="admin-volunteer-search"
                type="search"
                class="mt-2 w-full rounded-xl border border-slate-700 bg-slate-900 px-3 py-2 text-sm text-slate-100 placeholder:text-slate-500 focus:border-primary-500 focus:outline-none focus:ring-2 focus:ring-primary-500/40"
                placeholder="Name or email"
                prop:value=move || query.get()
                on:input=move |ev| {
                    let val = event_target_value(&ev);
                    query.set(val.clone());
                    on_search(val);
                }
            />
        </div>
        <div class="rounded-card border border-slate-800 bg-slate-900 shadow-card">{rows}</div>
        {footer}
    }
}

/// The volunteer applications waiting on a decision. Operations admins may
/// inspect the queue read-only; site admins may also approve or deny each one.
#[component]
pub fn PendingApplications(
    #[prop(into)] active: Signal<bool>,
    reload: RwSignal<u32>,
    is_site_admin: bool,
    #[prop(optional, default = false)] show_empty: bool,
) -> impl IntoView {
    let state = expect_context::<AppState>();
    let items = RwSignal::new(Vec::<QueueItem>::new());
    let load_error = RwSignal::new(None::<String>);
    let loading = RwSignal::new(false);
    let request_generation = RwSignal::new(0u64);
    // Setup links shown to the admin, by applicant id. Kept here so a reload,
    // which rebuilds every card, does not hide a link they are still copying.
    let shown_links = RwSignal::new(HashMap::<String, String>::new());

    Effect::new(move |_| {
        if !active.get() {
            return;
        }
        reload.track();
        if !state.has_operations_admin_permissions() {
            return;
        }
        loading.set(true);
        request_generation.update(|generation| *generation += 1);
        let generation = request_generation.get_untracked();
        spawn_local(async move {
            // Both queues, side by side: one from accounts that already exist,
            // one from the public volunteer signup.
            let accounts = list_pending_volunteer_applications().await;
            let applicants = list_volunteer_applicants().await;
            if request_generation.get_untracked() != generation {
                return;
            }
            match (accounts, applicants) {
                (Ok(accounts), Ok(applicants)) => {
                    let mut merged: Vec<QueueItem> = accounts
                        .into_iter()
                        .map(|volunteer| QueueItem {
                            source: Source::Account,
                            name: volunteer.full_name(),
                            id: volunteer.id,
                            email: volunteer.email,
                            skills_focus: volunteer.skills_focus,
                            agreed_at: volunteer.agreed_at,
                            pending: true,
                            setup_expires_at: String::new(),
                            setup_expired: false,
                        })
                        .collect();
                    merged.extend(applicants.into_iter().map(|applicant| QueueItem {
                        source: Source::Applicant,
                        pending: applicant.is_pending(),
                        setup_expired: applicant.setup_expired,
                        setup_expires_at: applicant.setup_expires_at.clone(),
                        name: applicant.full_name(),
                        id: applicant.id,
                        email: applicant.email,
                        skills_focus: applicant.details.skills_focus,
                        agreed_at: applicant.agreed_at,
                    }));
                    // Keep the badge honest against what is on screen. Only
                    // undecided rows count, matching what the server counts.
                    state
                        .volunteer_requests_pending
                        .set(merged.iter().filter(|item| item.pending).count() as i64);
                    // Forget links for applicants that have left the queue.
                    shown_links.update(|links| {
                        links.retain(|id, _| {
                            merged.iter().any(|item| {
                                item.source == Source::Applicant && !item.pending && &item.id == id
                            })
                        })
                    });
                    items.set(merged);
                    load_error.set(None);
                }
                (Err(e), _) | (_, Err(e)) => load_error.set(Some(err_text(e))),
            }
            loading.set(false);
        });
    });

    move || {
        let pending = items.get();
        let count = pending.len();
        let should_render = show_empty || loading.get() || load_error.get().is_some() || count > 0;
        if !should_render {
            return ().into_any();
        }

        let body = if let Some(message) = load_error.get() {
            view! {
                <p class="text-sm text-rose-300" role="alert">
                    "Could not load volunteer applications: " {message}
                </p>
            }
            .into_any()
        } else if loading.get() && count == 0 {
            view! { <p class="text-sm text-slate-500" aria-live="polite">"Loading…"</p> }.into_any()
        } else if pending.is_empty() {
            view! {
                <p class="text-sm text-slate-500" aria-live="polite">
                    "No volunteer applications are waiting for review."
                </p>
            }
            .into_any()
        } else {
            let cards = pending
                .into_iter()
                .map(|item| {
                    view! {
                        <ApplicationCard
                            item=item
                            reload=reload
                            is_site_admin=is_site_admin
                            shown_links=shown_links
                        />
                    }
                    .into_any()
                })
                .collect_view();
            view! { <div class="space-y-3">{cards}</div> }.into_any()
        };

        view! {
            <section class="rounded-card border border-slate-800 bg-slate-900 shadow-card p-5">
                <div class="mb-3 flex items-center gap-2">
                    <h2 class="text-base font-semibold text-slate-100">"Volunteer applications"</h2>
                    <span class="inline-flex min-w-5 items-center justify-center rounded-full bg-primary-500 px-1.5 py-0.5 text-[0.65rem] font-semibold leading-none text-white">
                        {count}
                    </span>
                </div>
                <p class="mb-4 text-xs text-slate-500">
                    {if is_site_admin {
                        "People who accepted the volunteer agreement and are waiting for a role decision. Approving grants the Volunteer role and either decision emails the applicant."
                    } else {
                        "People who accepted the volunteer agreement and are waiting for a site-admin decision. Operations admins can inspect this queue read-only."
                    }}
                </p>
                {body}
            </section>
        }
        .into_any()
    }
}

/// Which queue an application came from, and so which server function decides
/// it. The two are near-identical to review but not to act on: an applicant has
/// no account yet, so approving them sends a setup link rather than granting a
/// role on the spot.
#[derive(Clone, Copy, PartialEq)]
enum Source {
    /// An existing account that accepted the agreement.
    Account,
    /// A public volunteer signup, with no account behind it.
    Applicant,
}

/// One row of the review queue, whichever queue it came from.
#[derive(Clone)]
struct QueueItem {
    source: Source,
    /// The user id for [`Source::Account`], the applicant id otherwise.
    id: String,
    name: String,
    email: String,
    skills_focus: String,
    agreed_at: String,
    /// False for an approved applicant who has not yet followed their setup
    /// link: there is nothing left to decide, but they are worth showing.
    pending: bool,
    setup_expires_at: String,
    setup_expired: bool,
}

/// One pending application, with its approve/deny controls.
#[component]
fn ApplicationCard(
    item: QueueItem,
    reload: RwSignal<u32>,
    is_site_admin: bool,
    shown_links: RwSignal<HashMap<String, String>>,
) -> impl IntoView {
    let state = expect_context::<AppState>();
    let target_id = StoredValue::new(item.id.clone());
    let source = item.source;
    let is_pending = item.pending;
    let name = item.name.clone();
    let email = item.email.clone();
    let skills_focus = item.skills_focus.clone();
    let agreed_at = item.agreed_at.clone();
    let note = RwSignal::new(String::new());
    let change_email = RwSignal::new(false);
    let new_email = RwSignal::new(String::new());
    let confirm_email = RwSignal::new(String::new());
    let deciding = RwSignal::new(false);
    let error = RwSignal::new(None::<String>);
    let note_id = StoredValue::new(format!("volunteer-application-note-{}", item.id));
    let change_email_id =
        StoredValue::new(format!("volunteer-application-change-email-{}", item.id));
    let new_email_id = StoredValue::new(format!("volunteer-application-email-{}", item.id));
    let confirm_email_id =
        StoredValue::new(format!("volunteer-application-email-confirm-{}", item.id));
    let contact_href = format!("mailto:{email}");
    let setup_expired = RwSignal::new(item.setup_expired);
    let setup_expires_at = RwSignal::new(item.setup_expires_at.clone());
    let renewed_notice = RwSignal::new(None::<String>);
    let copied_link = Signal::derive(move || {
        shown_links.with(|links| links.get(&target_id.get_value()).cloned())
    });
    let link_id = StoredValue::new(format!("volunteer-application-setup-link-{}", item.id));
    let sent_to = StoredValue::new(email.clone());
    let first_name = StoredValue::new(
        item.name
            .split_whitespace()
            .next()
            .unwrap_or_default()
            .to_string(),
    );

    let renew = move |send_email: bool| {
        if deciding.get_untracked() {
            return;
        }
        deciding.set(true);
        error.set(None);
        renewed_notice.set(None);
        let target = target_id.get_value();
        shown_links.update(|links| {
            links.remove(&target);
        });
        spawn_local(async move {
            match renew_volunteer_setup_link(target.clone(), send_email).await {
                Ok(renewed) => {
                    setup_expired.set(false);
                    setup_expires_at.set(renewed.expires_at);
                    match renewed.link {
                        Some(link) => shown_links.update(|links| {
                            links.insert(target, link);
                        }),
                        // Delivery is fire-and-forget, so this cannot claim it arrived.
                        None => renewed_notice.set(Some(format!(
                            "Queued a new setup link to {}.",
                            sent_to.get_value()
                        ))),
                    }
                }
                Err(e) => error.set(Some(err_text(e))),
            }
            deciding.set(false);
        });
    };

    let withdraw = move |_| {
        if deciding.get_untracked()
            || !confirm(
                "Withdraw this approval? Their setup link stops working and they would need to apply again.",
            )
        {
            return;
        }
        deciding.set(true);
        error.set(None);
        let target = target_id.get_value();
        spawn_local(async move {
            match withdraw_volunteer_applicant(target).await {
                Ok(()) => reload.update(|value| *value += 1),
                Err(e) => error.set(Some(err_text(e))),
            }
            deciding.set(false);
        });
    };

    let decide = move |approve: bool| {
        if deciding.get_untracked() {
            return;
        }
        // Only an approval may carry an address change; the server enforces the
        // same rule and re-checks the confirmation.
        let (wanted_email, wanted_confirm) = if approve && change_email.get_untracked() {
            (new_email.get_untracked(), confirm_email.get_untracked())
        } else {
            (String::new(), String::new())
        };
        if !wanted_email.trim().is_empty()
            && wanted_email.trim().to_lowercase() != wanted_confirm.trim().to_lowercase()
        {
            error.set(Some("The two email addresses do not match.".to_string()));
            return;
        }
        deciding.set(true);
        error.set(None);
        let target = target_id.get_value();
        let decision_note = note.get_untracked();
        spawn_local(async move {
            // The same decision, but an applicant has no account to act on: it
            // sends them a setup link instead of granting a role.
            let outcome = match source {
                Source::Account => {
                    decide_volunteer_application(
                        target,
                        approve,
                        decision_note,
                        wanted_email,
                        wanted_confirm,
                    )
                    .await
                }
                Source::Applicant => {
                    decide_volunteer_applicant(
                        target,
                        approve,
                        decision_note,
                        wanted_email,
                        wanted_confirm,
                    )
                    .await
                }
            };
            match outcome {
                Ok(()) => {
                    reload.update(|value| *value += 1);
                    // The decided row leaves the queue, so keep the badge in step.
                    state.refresh_badges();
                }
                Err(e) => error.set(Some(err_text(e))),
            }
            deciding.set(false);
        });
    };

    view! {
        <div class="rounded-lg border border-slate-800 bg-slate-950 p-4">
            <div class="flex flex-wrap items-start justify-between gap-3">
                <div class="min-w-0">
                    // An applicant has no account yet, so there is no profile to
                    // link to \u{2014} their name is just their name.
                    {match source {
                        Source::Account => {
                            view! { <ProfileLink user_id=item.id name=name /> }.into_any()
                        }
                        Source::Applicant => {
                            view! {
                                <p class="truncate text-sm font-semibold text-slate-100">{name}</p>
                            }
                                .into_any()
                        }
                    }}
                    <a
                        href=contact_href
                        class="mt-1 inline-flex max-w-full truncate text-xs font-medium text-primary-300 underline decoration-dotted underline-offset-2 hover:text-primary-200 hover:decoration-solid"
                    >
                        {email.clone()}
                    </a>
                    <p class="mt-1 text-xs text-slate-500">"Agreement accepted " {agreed_at}</p>
                    <Show when=move || source == Source::Applicant>
                        <p class="mt-1 text-xs text-slate-500">
                            "New volunteer signup \u{2014} no account yet"
                        </p>
                    </Show>
                </div>
                {if is_pending {
                    view! {
                        <span class="inline-flex items-center rounded-full bg-amber-500/15 px-2 py-0.5 text-xs font-medium text-amber-300 ring-1 ring-amber-500/30">
                            "Pending review"
                        </span>
                    }
                        .into_any()
                } else {
                    view! {
                        <Show
                            when=move || setup_expired.get()
                            fallback=|| {
                                view! {
                                    <span class="inline-flex items-center rounded-full bg-sky-500/15 px-2 py-0.5 text-xs font-medium text-sky-300 ring-1 ring-sky-500/30">
                                        "Approved \u{2014} waiting on account setup"
                                    </span>
                                }
                            }
                        >
                            <span class="inline-flex items-center rounded-full bg-rose-500/15 px-2 py-0.5 text-xs font-medium text-rose-300 ring-1 ring-rose-500/30">
                                "Setup link expired"
                            </span>
                        </Show>
                    }
                        .into_any()
                }}
            </div>

            <Show when=move || error.get().is_some()>
                <p
                    class="mt-3 rounded-lg border border-rose-500/40 bg-rose-500/10 px-3 py-2 text-xs text-rose-300"
                    role="alert"
                >
                    {move || error.get().unwrap_or_default()}
                </p>
            </Show>

            // What they say they can do, so an application can be judged
            // without opening their profile.
            {(!skills_focus.is_empty())
                .then(|| {
                    view! {
                        <div class="mt-3 rounded-lg border border-slate-800 bg-slate-900/60 px-3 py-2">
                            <p class="text-xs font-medium text-slate-400">
                                "Skills and area of focus"
                            </p>
                            <p class="mt-0.5 whitespace-pre-line text-sm text-slate-200">
                                {skills_focus}
                            </p>
                        </div>
                    }
                })}

            // An approved applicant has already been decided; all that is left
            // is for them to follow their setup link.
            <Show when=move || !is_pending>
                <p class="mt-3 text-xs text-slate-500">
                    {move || {
                        let expires = setup_expires_at.get();
                        if setup_expired.get() {
                            format!(
                                "Their setup link expired on {expires}. Send a new one, or get a link to pass on yourself.",
                            )
                        } else {
                            format!(
                                "Waiting for them to choose a password and finish setting up their account. Their link expires on {expires}.",
                            )
                        }
                    }}
                </p>
            </Show>

            <Show when=move || !is_pending && is_site_admin>
                <Show when=move || renewed_notice.get().is_some()>
                    <p
                        class="mt-3 rounded-lg border border-emerald-500/40 bg-emerald-500/10 px-3 py-2 text-xs text-emerald-300"
                        role="status"
                    >
                        {move || renewed_notice.get().unwrap_or_default()}
                    </p>
                </Show>
                <Show when=move || copied_link.get().is_some()>
                    <div class="mt-3 rounded-lg border border-slate-800 bg-slate-900/60 px-3 py-2">
                        <label
                            class="mb-1 block text-xs font-medium text-slate-300"
                            for=link_id.get_value()
                        >
                            "New setup link"
                        </label>
                        <input
                            id=link_id.get_value()
                            readonly
                            class="min-w-0 w-full rounded-lg border border-slate-700 bg-slate-950 px-2 py-1.5 font-mono text-xs text-slate-100"
                            prop:value=move || copied_link.get().unwrap_or_default()
                            on:focus=move |event| select_all(&event)
                        />
                        <p class="mt-1 text-xs text-slate-500">
                            {move || {
                                format!(
                                    "Anyone with this link can finish this account, so send it only to {} directly, such as in Teams or a text.",
                                    first_name.get_value(),
                                )
                            }}
                        </p>
                    </div>
                </Show>
                <div class="mt-3 flex flex-col gap-2 sm:flex-row sm:items-center">
                    <button
                        type="button"
                        on:click=move |_| renew(true)
                        prop:disabled=move || deciding.get()
                        class="rounded-lg bg-primary-500 px-3 py-1.5 text-sm font-semibold text-white hover:bg-primary-600 disabled:opacity-50"
                    >
                        "Resend setup email"
                    </button>
                    <button
                        type="button"
                        on:click=move |_| renew(false)
                        prop:disabled=move || deciding.get()
                        class="rounded-lg border border-slate-700 px-3 py-1.5 text-sm font-medium text-slate-200 hover:bg-slate-800 disabled:opacity-50"
                    >
                        "Get setup link"
                    </button>
                    <button
                        type="button"
                        on:click=withdraw
                        prop:disabled=move || deciding.get()
                        class="rounded-lg border border-rose-500/40 px-3 py-1.5 text-sm font-medium text-rose-300 hover:bg-rose-500/10 disabled:opacity-50"
                    >
                        "Withdraw approval"
                    </button>
                </div>
            </Show>

            <Show
                when=move || is_site_admin && is_pending
                fallback=move || {
                    (is_pending && !is_site_admin)
                        .then(|| {
                            view! {
                                <p class="mt-3 text-xs text-slate-500">
                                    "Only site admins can approve or deny volunteer applications."
                                </p>
                            }
                        })
                }
            >
                <div class="mt-3 space-y-3">
                    <div>
                        <label
                            class="mb-1 block text-xs font-medium text-slate-300"
                            for=note_id.get_value()
                        >
                            "Decision note"
                        </label>
                        <input
                            id=note_id.get_value()
                            class="min-w-0 w-full rounded-xl border border-slate-700 bg-slate-900 px-2 py-1.5 text-sm text-slate-100 placeholder:text-slate-500"
                            placeholder="Optional context included in the email"
                            maxlength="1000"
                            prop:disabled=move || deciding.get()
                            prop:value=move || note.get()
                            on:input=move |event| note.set(event_target_value(&event))
                        />
                    </div>

                    // Optional: give them their official volunteer address as
                    // the account's sign-in email, at the moment of approval.
                    <div class="rounded-lg border border-slate-800 bg-slate-900/60 px-3 py-2">
                        <label class="flex items-center gap-2 text-xs font-medium text-slate-300">
                            <input
                                id=change_email_id.get_value()
                                type="checkbox"
                                class="h-4 w-4 rounded border-slate-700 bg-slate-950"
                                prop:disabled=move || deciding.get()
                                prop:checked=move || change_email.get()
                                on:change=move |event| change_email
                                    .set(event_target_checked(&event))
                            />
                            "Change their email address"
                        </label>
                        <Show when=move || change_email.get()>
                            <div class="mt-2 space-y-2">
                                <p class="text-xs text-slate-500">
                                    "This replaces the address they sign in with. Their password is unchanged."
                                </p>
                                <div>
                                    <label
                                        class="mb-1 block text-xs font-medium text-slate-300"
                                        for=new_email_id.get_value()
                                    >
                                        "Official volunteer email"
                                    </label>
                                    <input
                                        id=new_email_id.get_value()
                                        type="email"
                                        class="min-w-0 w-full rounded-xl border border-slate-700 bg-slate-900 px-2 py-1.5 text-sm text-slate-100 placeholder:text-slate-500"
                                        placeholder="name@example.org"
                                        maxlength="254"
                                        autocomplete="off"
                                        prop:disabled=move || deciding.get()
                                        prop:value=move || new_email.get()
                                        on:input=move |event| new_email
                                            .set(event_target_value(&event))
                                    />
                                </div>
                                <div>
                                    <label
                                        class="mb-1 block text-xs font-medium text-slate-300"
                                        for=confirm_email_id.get_value()
                                    >
                                        "Confirm email"
                                    </label>
                                    <input
                                        id=confirm_email_id.get_value()
                                        type="email"
                                        class="min-w-0 w-full rounded-xl border border-slate-700 bg-slate-900 px-2 py-1.5 text-sm text-slate-100 placeholder:text-slate-500"
                                        placeholder="Type it again"
                                        maxlength="254"
                                        autocomplete="off"
                                        prop:disabled=move || deciding.get()
                                        prop:value=move || confirm_email.get()
                                        on:input=move |event| confirm_email
                                            .set(event_target_value(&event))
                                    />
                                </div>
                            </div>
                        </Show>
                    </div>
                    <div class="flex flex-col gap-2 sm:flex-row sm:items-center">
                        <button
                            type="button"
                            on:click=move |_| decide(true)
                            prop:disabled=move || deciding.get()
                            class="rounded-full bg-primary-500 px-3 py-1.5 text-sm font-semibold text-white hover:bg-primary-600 disabled:opacity-50"
                        >
                            "Approve"
                        </button>
                        <button
                            type="button"
                            on:click=move |_| decide(false)
                            prop:disabled=move || deciding.get()
                            class="rounded-full border border-rose-500/40 px-3 py-1.5 text-sm font-medium text-rose-300 hover:bg-rose-500/10 disabled:opacity-50"
                        >
                            "Deny"
                        </button>
                    </div>
                </div>
            </Show>
        </div>
    }
}

/// Browser `confirm`, or a refusal during SSR where no dialog exists.
fn confirm(message: &str) -> bool {
    #[cfg(feature = "hydrate")]
    {
        web_sys::window()
            .and_then(|window| window.confirm_with_message(message).ok())
            .unwrap_or(false)
    }
    #[cfg(not(feature = "hydrate"))]
    {
        let _ = message;
        false
    }
}

fn select_all(event: &leptos::ev::FocusEvent) {
    #[cfg(feature = "hydrate")]
    {
        use wasm_bindgen::JsCast;
        if let Some(input) = event
            .target()
            .and_then(|target| target.dyn_into::<web_sys::HtmlInputElement>().ok())
        {
            input.select();
        }
    }
    #[cfg(not(feature = "hydrate"))]
    let _ = event;
}
